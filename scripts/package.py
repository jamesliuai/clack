#!/usr/bin/env python3
"""Build a verified native package, or archive the complete source without publishing.

Python 3.11+; original Cargo.toml, lockfile and vendor fork are preserved in source
archives. Foreign packages require --cross and explicitly retain unexecuted status.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import os
import platform
import struct
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "x86_64-unknown-linux-gnu": ("linux", "x86_64"),
    "aarch64-unknown-linux-gnu": ("linux", "aarch64"),
    "x86_64-apple-darwin": ("macos", "x86_64"),
    "aarch64-apple-darwin": ("macos", "aarch64"),
    "x86_64-pc-windows-msvc": ("windows", "x86_64"),
    "x86_64-pc-windows-gnu": ("windows", "x86_64"),
}
SOURCE_DIRS = ("src", "tests", "examples", "scripts", "data", "vendor", "docs", "third-party", ".github")
SOURCE_FILES = ("Cargo.toml", "Cargo.lock", "LICENSE", "README.md", "THIRD-PARTY-NOTICES.md", "deny.toml", ".gitignore", "SPEC.md")


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def run(args: list[str], **kwargs) -> bytes:
    result = subprocess.run(args, cwd=ROOT, check=True, stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, timeout=kwargs.pop("timeout", 180), **kwargs)
    return result.stdout


def files_under(directory: Path) -> list[Path]:
    files = []
    for path in sorted(directory.rglob("*")):
        relative = path.relative_to(directory)
        if any(part in ("__pycache__", ".git", "target") for part in relative.parts) or path.suffix == ".pyc":
            continue
        if path.is_symlink():
            raise ValueError(f"package inputs must be regular files: {relative}")
        if path.is_file():
            files.append(path)
    return files


def source_identity() -> str:
    digest = hashlib.sha256()
    paths = [ROOT / "Cargo.toml", ROOT / "Cargo.lock"]
    for directory in ("src", "data", "vendor"):
        paths.extend(files_under(ROOT / directory))
    for path in sorted(paths):
        digest.update(path.relative_to(ROOT).as_posix().encode() + b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def executable_identity(data: bytes) -> tuple[str, str]:
    if data[:4] == b"\xcf\xfa\xed\xfe" and len(data) >= 8:
        cpu = struct.unpack_from("<I", data, 4)[0]
        return "macos", {0x01000007: "x86_64", 0x0100000C: "aarch64"}.get(cpu, "unknown")
    if data[:4] == b"\x7fELF" and len(data) >= 20:
        if data[4:6] != b"\x02\x01":
            raise ValueError("only 64-bit little-endian native ELF packages are supported")
        cpu = struct.unpack_from("<H", data, 18)[0]
        return "linux", {62: "x86_64", 183: "aarch64"}.get(cpu, "unknown")
    if data[:2] == b"MZ" and len(data) >= 64:
        offset = struct.unpack_from("<I", data, 60)[0]
        if offset + 6 <= len(data) and data[offset:offset + 4] == b"PE\0\0":
            cpu = struct.unpack_from("<H", data, offset + 4)[0]
            return "windows", {0x8664: "x86_64"}.get(cpu, "unknown")
    raise ValueError("unrecognized executable format")


def archive_bytes(name: str, files: dict[str, tuple[bytes, int]], epoch: int, windows: bool) -> bytes:
    output = io.BytesIO()
    if windows:
        # ZIP cannot represent dates before 1980. Metadata never uses wall time.
        import datetime
        date = datetime.datetime.fromtimestamp(max(315532800, epoch), datetime.timezone.utc)
        with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for path, (data, mode) in sorted(files.items()):
                info = zipfile.ZipInfo(f"{name}/{path}", date.timetuple()[:6])
                info.create_system = 3
                info.external_attr = (0o100000 | mode) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(info, data)
    else:
        with gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=epoch, compresslevel=9) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
                for path, (data, mode) in sorted(files.items()):
                    info = tarfile.TarInfo(f"{name}/{path}")
                    info.size, info.mode, info.mtime = len(data), mode, epoch
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    archive.addfile(info, io.BytesIO(data))
    return output.getvalue()


def package(args: argparse.Namespace) -> Path:
    cargo_manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]
    version = cargo_manifest["version"]
    epoch = args.epoch
    if epoch < 0 or epoch > 0xFFFFFFFF:
        raise ValueError("SOURCE_DATE_EPOCH must fit an unsigned 32-bit Unix timestamp")
    files: dict[str, tuple[bytes, int]] = {}
    native = not args.source
    target = args.target
    if native and target not in TARGETS:
        raise ValueError("a native --target is required unless --source was selected")
    if args.source:
        for path in SOURCE_FILES:
            source = ROOT / path
            if not source.is_file() or source.is_symlink():
                raise ValueError(f"required source release input is missing/nonregular: {path}")
            files[path] = (source.read_bytes(), 0o644)
        for directory in SOURCE_DIRS:
            for source in files_under(ROOT / directory):
                files[source.relative_to(ROOT).as_posix()] = (source.read_bytes(), 0o644)
        name = f"clack-{version}-source"
        metadata = {"kind": "source", "cargo_lock_sha256": sha((ROOT / "Cargo.lock").read_bytes())}
    else:
        environment = os.environ.copy()
        rustflag_keys = [key for key in environment if key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS")
                         or key.startswith("CARGO_TARGET_") and key.endswith("_RUSTFLAGS")]
        if any(environment.get(key, "").strip() for key in rustflag_keys):
            raise ValueError("custom RUSTFLAGS, including target-cpu=native, are forbidden for distributed packages")
        cflag_keys = [key for key in environment if key in ("CFLAGS", "CXXFLAGS", "CPPFLAGS", "HOST_CFLAGS", "TARGET_CFLAGS")
                      or key.startswith(("CFLAGS_", "CXXFLAGS_", "CPPFLAGS_"))]
        if any(environment.get(key, "").strip() for key in cflag_keys):
            raise ValueError("custom C/C++ flags, including host-specific CPU flags for bundled SQLite, are forbidden for distributed packages")
        # Explicit empty encoded flags take precedence over user/global Cargo
        # configuration, so distributed code never inherits host-specific flags.
        environment["CARGO_ENCODED_RUSTFLAGS"] = ""
        target_directory = args.target_dir.resolve()
        environment["CARGO_TARGET_DIR"] = str(target_directory)
        environment["CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS"] = "false"
        environment["CARGO_PROFILE_RELEASE_PANIC"] = "unwind"
        environment["CARGO_PROFILE_RELEASE_LTO"] = "thin"
        environment["CARGO_PROFILE_RELEASE_CODEGEN_UNITS"] = "1"
        environment["CARGO_PROFILE_RELEASE_STRIP"] = "symbols"
        environment["CARGO_PROFILE_RELEASE_OPT_LEVEL"] = "3"
        if TARGETS[target][0] == "macos":
            environment["MACOSX_DEPLOYMENT_TARGET"] = "11.0"
        original = source_identity()
        build_target = target + ("." + args.glibc if args.glibc else "")
        if args.glibc and (not args.zigbuild or "linux-gnu" not in target):
            raise ValueError("--glibc requires a Linux GNU --target and --zigbuild")
        command = ["cargo", f"+{args.toolchain}", "zigbuild" if args.zigbuild else "build", "--locked", "--offline", "--release", "--no-default-features", "--target", build_target, "-j", str(args.jobs)]
        run(command, env=environment, timeout=900)
        if original != source_identity():
            raise ValueError("source changed while the release was building; retry after edits settle")
        binary_name = "clack.exe" if TARGETS[target][0] == "windows" else "clack"
        binary = target_directory / target / "release" / binary_name
        data = binary.read_bytes()
        if executable_identity(data) != TARGETS[target]:
            raise ValueError("executable header does not match the requested target")
        if len(data) > 15 * 1024 * 1024:
            raise ValueError("release binary exceeds the documented 15MiB target")
        rustc = run(["rustc", f"+{args.toolchain}", "-vV"]).decode()
        host_target = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
        documentation_binary = binary
        documentation_command = None
        if args.cross:
            documentation_command = ["cargo", f"+{args.toolchain}", "build", "--locked", "--offline", "--release", "--no-default-features", "--target", host_target, "-j", str(args.jobs)]
            run(documentation_command, env=environment, timeout=900)
            documentation_binary = target_directory / host_target / "release" / ("clack.exe" if "windows" in host_target else "clack")
        with tempfile.TemporaryDirectory(prefix="clack-package-check-") as directory:
            config = Path(directory) / "config.toml"
            config.write_text("schema_version = 1\n")
            base = [str(documentation_binary), "--config", str(config), "--data-dir", str(Path(directory) / "data")]
            reported_version = run([str(documentation_binary), "--version"], timeout=20).decode().strip()
            if reported_version != f"clack {version}":
                raise ValueError("executable version differs from package version")
            doctor = json.loads(run([*base, "doctor", "--json"], timeout=10))
            build = doctor["build"]
            if build["debug_assertions"] or build["test_hooks_enabled"]:
                raise ValueError("only production binaries without debug assertions/test hooks can be packaged")
            if not args.cross and (build["target_os"], build["target_arch"]) != TARGETS[target]:
                raise ValueError("executed binary reports a different target")
            files["bin/" + binary_name] = (data, 0o755)
            files["share/man/man1/clack.1"] = (run([str(documentation_binary), "man"], timeout=10), 0o644)
            for shell in ("bash", "fish", "zsh", "elvish", "powershell"):
                files[f"share/completions/clack.{shell}"] = (run([str(documentation_binary), "completions", shell], timeout=10), 0o644)
        if original != source_identity():
            raise ValueError("source changed during documentation generation; retry after edits settle")
        inventory = json.loads((ROOT / "third-party/inventory.json").read_text())
        if inventory["cargo_lock_sha256"] != sha((ROOT / "Cargo.lock").read_bytes()):
            raise ValueError("dependency notices are stale; run scripts/licenses.py")
        for filename in ("README.md", "LICENSE", "THIRD-PARTY-NOTICES.md", "SPEC.md"):
            files[filename] = ((ROOT / filename).read_bytes(), 0o644)
        for filename in ("vendor/clack-private-fs/SAFETY.md", "vendor/clack-private-fs/LICENSE",
                         "vendor/crossterm/CLACK-PATCH.md", "vendor/crossterm/LICENSE"):
            files[filename] = ((ROOT / filename).read_bytes(), 0o644)
        files["install.py"] = ((ROOT / "scripts/install.py").read_bytes(), 0o644)
        for directory in ("third-party", "docs", "data"):
            for source in files_under(ROOT / directory):
                if directory == "data" and source.suffix not in (".json", ".txt"):
                    continue
                files[source.relative_to(ROOT).as_posix()] = (source.read_bytes(), 0o644)
        metadata = {
            "kind": "native", "target": target, "native_command_checks": not args.cross and target == host_target,
            "local_command_checks": not args.cross, "cross_compiled": target != host_target,
            "command_execution_host_target": host_target if not args.cross else None,
            "build": build if not args.cross else None,
            "documentation_build": build, "documentation_binary_sha256": sha(documentation_binary.read_bytes()),
            "documentation_build_command": documentation_command,
            "binary_sha256": sha(data), "binary_bytes": len(data),
            "source_identity_sha256": original, "cargo_lock_sha256": inventory["cargo_lock_sha256"],
            "build_command": command, "rustc": rustc,
            "release_profile": {"lto": "thin", "codegen_units": 1, "strip": "symbols", "opt_level": 3,
                                "panic": "unwind", "debug_assertions": False, "test_hooks": False},
            "host": platform.platform(), "rustflags": environment.get("RUSTFLAGS", ""),
            "macos_deployment_target": environment.get("MACOSX_DEPLOYMENT_TARGET"),
            "glibc_link_target": args.glibc,
            "limitations": ["Foreign-target execution was not attempted by this package command; docs were generated by a checked host binary from the same source." if args.cross else "Local executable checks on a different host architecture may use OS translation and do not establish native target hardware behavior.", "Command checks do not establish terminal emulator, other OS/architecture, cold-start, or controlled performance validation."],
        }
        name = f"clack-{version}-{target}"
    manifest = {"package_schema_version": 1, "version": version, "source_date_epoch": epoch,
                **metadata, "files": [{"path": path, "sha256": sha(data), "size": len(data), "mode": mode}
                                      for path, (data, mode) in sorted(files.items())]}
    files["manifest.json"] = (json.dumps(manifest, ensure_ascii=True, indent=2).encode() + b"\n", 0o644)
    windows = native and TARGETS[target][0] == "windows"
    artifact = archive_bytes(name, files, epoch, windows)
    args.output.mkdir(parents=True, exist_ok=True)
    destination = args.output / (name + (".zip" if windows else ".tar.gz"))
    with destination.open("xb") as output:
        output.write(artifact)
    checksum = destination.with_name(destination.name + ".sha256")
    with checksum.open("x") as output:
        output.write(f"{sha(artifact)}  {destination.name}\n")
    print(f"Created {destination} ({len(artifact)} bytes); SHA256 {sha(artifact)}")
    return destination


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", action="store_true")
    parser.add_argument("--target", choices=tuple(TARGETS))
    parser.add_argument("--toolchain", default="stable")
    parser.add_argument("--cross", action="store_true", help="do not execute the foreign target; use a separately checked host build to generate CLI docs")
    parser.add_argument("--zigbuild", action="store_true", help="build with the installed cargo-zigbuild plugin")
    parser.add_argument("--glibc", help="explicit Zig Linux GNU ABI version, for example 2.17")
    parser.add_argument("--target-dir", type=Path, default=ROOT / "target/release-packages")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    parser.add_argument("--epoch", type=int, default=int(os.environ.get("SOURCE_DATE_EPOCH", "0")))
    args = parser.parse_args()
    package(args)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        raise SystemExit(f"packaging failed: {error}")
