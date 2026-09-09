#!/usr/bin/env python3
"""Install or uninstall an extracted clack native package without touching user data.

Requires Python 3.11+. Installation is optional: the native binary can also be
copied manually. This helper never changes PATH or overwrites an existing binary.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import stat
import tempfile
from pathlib import Path, PurePosixPath


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def relative_path(raw: str) -> Path:
    if not isinstance(raw, str):
        raise ValueError("installation paths must be text")
    path = PurePosixPath(raw)
    if not raw or path.as_posix() != raw or "\\" in raw or ":" in raw or path.is_absolute() or any(p in (".", "..") for p in path.parts):
        raise ValueError("invalid path in installation manifest")
    if any(any(ord(c) < 32 for c in part) for part in path.parts):
        raise ValueError("control character in installation path")
    return Path(*path.parts)


def link_like(path: Path) -> bool:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return False
    return stat.S_ISLNK(metadata.st_mode) or bool(
        getattr(metadata, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT)


def no_links(root: Path, relative: Path) -> Path:
    path = root
    for part in relative.parts:
        path = path / part
        if link_like(path):
            raise ValueError("symbolic links and reparse points are not followed during installation")
    return path


def ordinary_directories(root: Path):
    """Walk package-owned directories without entering any reparse point."""
    pending = [root]
    while pending:
        directory = pending.pop()
        for child in directory.iterdir():
            if link_like(child):
                continue
            if child.is_dir():
                yield child
                pending.append(child)


def regular_file(root: Path, relative: Path) -> Path:
    path = no_links(root, relative)
    if not path.is_file():
        raise ValueError("manifest references a missing/nonregular package file")
    return path


def read_manifest(package: Path) -> tuple[dict, str]:
    path = regular_file(package, Path("manifest.json"))
    if path.stat().st_size > 1024 * 1024:
        raise ValueError("package manifest exceeds 1MiB")
    data = path.read_bytes()
    manifest = json.loads(data)
    if manifest.get("package_schema_version") != 1 or manifest.get("kind") != "native":
        raise ValueError("installation requires a version1 native package")
    entries = manifest.get("files")
    if not isinstance(entries, list) or not 1 <= len(entries) <= 2000:
        raise ValueError("invalid package file count")
    seen = set()
    for entry in entries:
        relative = relative_path(entry["path"])
        if relative in seen:
            raise ValueError("duplicate package file")
        seen.add(relative)
        path = regular_file(package, relative)
        if path.stat().st_size != entry["size"] or path.stat().st_size > 64 * 1024 * 1024:
            raise ValueError("package file size differs from its manifest")
        if sha(path.read_bytes()) != entry["sha256"]:
            raise ValueError("package checksum mismatch")
    return manifest, sha(data)


def load_record(prefix: Path) -> tuple[Path, dict]:
    path = regular_file(prefix, Path("share/clack/install.json"))
    if path.stat().st_size > 1024 * 1024:
        raise ValueError("installation record is too large")
    record = json.loads(path.read_bytes())
    if record.get("install_schema_version") != 1:
        raise ValueError("unknown installation record version")
    if record["binary"] not in ("bin/clack", "bin/clack.exe"):
        raise ValueError("unknown registered binary path")
    if not re.fullmatch(r"share/clack/package-[0-9a-f]{20}", record["bundle"]):
        raise ValueError("unknown registered bundle path")
    if not isinstance(record["files"], list) or not 1 <= len(record["files"]) <= 2001:
        raise ValueError("invalid registered file count")
    paths = [relative_path(entry["path"]) for entry in record["files"]]
    if len(set(paths)) != len(paths):
        raise ValueError("duplicate registered file")
    return path, record


def install(package: Path, prefix: Path) -> Path:
    package, prefix = package.resolve(), prefix.resolve()
    manifest, package_hash = read_manifest(package)
    targets = {
        "aarch64-apple-darwin": ("Darwin", "aarch64"),
        "x86_64-apple-darwin": ("Darwin", "x86_64"),
        "aarch64-unknown-linux-gnu": ("Linux", "aarch64"),
        "x86_64-unknown-linux-gnu": ("Linux", "x86_64"),
        "x86_64-pc-windows-msvc": ("Windows", "x86_64"),
        "x86_64-pc-windows-gnu": ("Windows", "x86_64"),
    }
    identity = targets.get(manifest["target"])
    host_os = platform.system()
    if identity is None:
        raise ValueError("unsupported package target")
    if identity[0] != host_os:
        raise ValueError("package operating system does not match this installation host")
    machine = platform.machine().lower()
    host_arch = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
    if identity[1] != host_arch and (host_os, host_arch, identity[1]) != ("Darwin", "aarch64", "x86_64"):
        raise ValueError("package does not match this processor architecture")
    suffix = ".exe" if "windows" in manifest["target"] else ""
    binary_relative = Path("bin") / ("clack" + suffix)
    binary = regular_file(package, binary_relative)
    binary_hash = sha(binary.read_bytes())
    if binary_hash != manifest["binary_sha256"]:
        raise ValueError("binary differs from package identity")
    destination = no_links(prefix, binary_relative)
    record_path = no_links(prefix, Path("share/clack/install.json"))
    if record_path.exists() or record_path.is_symlink():
        _, old = load_record(prefix)
        if old["package_sha256"] == package_hash and destination.is_file() and not destination.is_symlink() and sha(destination.read_bytes()) == binary_hash:
            print(f"Already installed: {destination}")
            return destination
        raise ValueError("another installation exists; uninstall it explicitly before installing a new package")
    if destination.exists() or destination.is_symlink():
        raise ValueError("refusing to overwrite an existing binary")
    bundle_relative = Path("share/clack") / ("package-" + package_hash[:20])
    bundle = no_links(prefix, bundle_relative)
    if bundle.exists() or bundle.is_symlink():
        raise ValueError("installation bundle already exists without an ownership record")
    destination.parent.mkdir(parents=True, exist_ok=True)
    record_path.parent.mkdir(parents=True, exist_ok=True)
    published_binary = published_bundle = False
    with tempfile.TemporaryDirectory(prefix=".clack-install-", dir=record_path.parent) as temporary:
        temporary = Path(temporary)
        staged_bundle = temporary / "package"
        staged_bundle.mkdir()
        for entry in manifest["files"]:
            relative = relative_path(entry["path"])
            source = regular_file(package, relative)
            output = staged_bundle / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, output)
            output.chmod(0o755 if entry["path"] == binary_relative.as_posix() else 0o644)
        shutil.copyfile(package / "manifest.json", staged_bundle / "manifest.json")
        record = {"install_schema_version": 1, "package_sha256": package_hash,
                  "binary": binary_relative.as_posix(), "binary_sha256": binary_hash,
                  "bundle": bundle_relative.as_posix(),
                  "files": [{"path": entry["path"], "sha256": entry["sha256"]} for entry in manifest["files"]]}
        record["files"].append({"path": "manifest.json", "sha256": package_hash})
        record_bytes = json.dumps(record, ensure_ascii=True, indent=2).encode() + b"\n"
        staged_record = temporary / "install.json"
        staged_record.write_bytes(record_bytes)
        staged_record.chmod(0o600)
        try:
            staged_bundle.rename(bundle)
            published_bundle = True
            # Atomic publication without replacement. Source and destination are
            # in the selected prefix; no partially copied executable is visible.
            os.link(bundle / binary_relative, destination)
            published_binary = True
            os.link(staged_record, record_path)
        except Exception:
            if published_binary and destination.is_file() and sha(destination.read_bytes()) == binary_hash:
                destination.unlink()
            if published_bundle:
                shutil.rmtree(bundle)
            raise
    print(f"Installed {destination}; add {destination.parent} to PATH if needed.")
    print(f"Documentation, completions, man page and notices: {bundle}")
    return destination


def uninstall(prefix: Path) -> None:
    prefix = prefix.resolve()
    record_path, record = load_record(prefix)
    binary = regular_file(prefix, relative_path(record["binary"]))
    if sha(binary.read_bytes()) != record["binary_sha256"]:
        raise ValueError("installed binary was modified; refusing to remove it")
    bundle = no_links(prefix, relative_path(record["bundle"]))
    preserved = []
    removable = []
    for entry in record["files"]:
        relative = relative_path(entry["path"])
        path = bundle / relative
        if not path.exists() and not path.is_symlink():
            continue
        try:
            checked = regular_file(bundle, relative)
        except ValueError:
            checked = None
        if checked is None or sha(checked.read_bytes()) != entry["sha256"]:
            preserved.append(relative.as_posix())
        else:
            removable.append(path)
    binary.unlink()
    for path in removable:
        path.unlink()
    for directory in sorted(ordinary_directories(bundle), key=lambda p: len(p.parts), reverse=True):
        try:
            directory.rmdir()
        except OSError:
            pass
    try:
        bundle.rmdir()
    except OSError:
        pass
    record_path.unlink()
    print("Removed the registered binary and unchanged package files. Configuration and history were preserved.")
    if preserved or bundle.exists():
        print("Modified or unregistered package files were preserved in " + str(bundle))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--prefix", type=Path, default=Path.home() / ".local")
    parser.add_argument("--uninstall", action="store_true")
    args = parser.parse_args()
    if args.uninstall:
        uninstall(args.prefix)
    else:
        install(args.package, args.prefix)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, KeyError, TypeError, OSError, json.JSONDecodeError) as error:
        raise SystemExit(f"installation failed: {error}")
