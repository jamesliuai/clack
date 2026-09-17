#!/usr/bin/env python3
"""Verify release archive contents and exercise local install/uninstall safely.

No build, publication, network request, or user-data access occurs. Foreign OS
executables receive format/checksum checks only. macOS x86-64 commands may run
through Rosetta on an arm64 host and are explicitly labelled as translated.
Keep the resulting report beside the archives to avoid a self-referential hash.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import platform
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile
from pathlib import Path, PurePosixPath

import install
import package


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def unpack(archive: Path, destination: Path) -> tuple[dict, Path, int]:
    raw = archive.read_bytes()
    checksum = archive.with_name(archive.name + ".sha256").read_text().split()
    if checksum != [sha(raw), archive.name]:
        raise ValueError("archive checksum file does not match " + archive.name)
    files: dict[str, tuple[bytes, int]] = {}
    total = 0

    def add(name: str, size: int, mode: int, read) -> None:
        nonlocal total
        path = install.relative_path(name)
        if name in files or len(path.parts) < 2:
            raise ValueError("duplicate or unrooted archive path")
        total += size
        if size > 64 * 1024 * 1024 or total > 512 * 1024 * 1024:
            raise ValueError("release archive exceeds verification bounds")
        data = read()
        if len(data) != size:
            raise ValueError("archive file length mismatch")
        files[name] = data, mode

    if archive.suffix == ".zip":
        with zipfile.ZipFile(io.BytesIO(raw)) as opened:
            for entry in opened.infolist():
                mode = entry.external_attr >> 16
                if entry.is_dir() or mode & 0o170000 != 0o100000:
                    raise ValueError("only regular release files are accepted")
                add(entry.filename, entry.file_size, mode & 0o777,
                    lambda entry=entry: opened.read(entry))
    else:
        with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as opened:
            for entry in opened:
                if not entry.isfile():
                    raise ValueError("only regular release files are accepted")
                add(entry.name, entry.size, entry.mode,
                    lambda entry=entry: opened.extractfile(entry).read())
    roots = {PurePosixPath(name).parts[0] for name in files}
    if len(roots) != 1:
        raise ValueError("release must contain exactly one root")
    root_name = roots.pop()
    manifest_name = root_name + "/manifest.json"
    manifest = json.loads(files[manifest_name][0])
    if manifest.get("package_schema_version") != 1:
        raise ValueError("unknown package schema")
    expected = {manifest_name}
    for entry in manifest["files"]:
        relative = install.relative_path(entry["path"])
        name = root_name + "/" + relative.as_posix()
        if name in expected:
            raise ValueError("duplicate manifest entry")
        expected.add(name)
        data, mode = files[name]
        if len(data) != entry["size"] or sha(data) != entry["sha256"] or mode != entry["mode"]:
            raise ValueError("release payload differs from manifest: " + name)
    if expected != files.keys():
        raise ValueError("release has unregistered payload files")
    for name, (data, mode) in files.items():
        path = destination / install.relative_path(name)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        path.chmod(mode)
    return manifest, destination / root_name, total


def command(binary: Path, arguments: list[str], directory: Path) -> bytes:
    config = directory / "fixture.toml"
    config.write_text("schema_version = 1\n")
    result = subprocess.run([str(binary), "--config", str(config), "--data-dir",
                             str(directory / "private-data"), *arguments],
                            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=30)
    if result.returncode != 0 or result.stderr or b"\x1b" in result.stdout:
        raise ValueError(f"packaged command failed: {arguments!r}, exit {result.returncode}, "
                         f"stderr {result.stderr.decode(errors='replace')!r}")
    if (directory / "private-data").exists():
        raise ValueError("read-only command unexpectedly created user data")
    return result.stdout


def verify(archive: Path) -> dict:
    with tempfile.TemporaryDirectory(prefix="clack-artifact-check-") as temporary:
        directory = Path(temporary)
        manifest, root, expanded_bytes = unpack(archive, directory / "expanded")
        result = {"archive": archive.name, "sha256": sha(archive.read_bytes()),
                  "archive_bytes": archive.stat().st_size, "expanded_bytes": expanded_bytes,
                  "file_count": len(manifest["files"]), "kind": manifest["kind"],
                  "manifest_sha256": sha((root / "manifest.json").read_bytes()),
                  "payload_verification": "pass", "target": manifest.get("target")}
        if manifest["kind"] == "source":
            cargo_bytes = (root / "Cargo.toml").read_bytes()
            cargo = tomllib.loads(cargo_bytes.decode())
            for dependency, expected_path in (("crossterm", "vendor/crossterm"),
                                               ("ratatui-crossterm", "vendor/ratatui-crossterm")):
                if cargo["dependencies"][dependency].get("path") != expected_path:
                    raise ValueError("source release did not preserve its terminal dependency paths")
            for filename in package.SOURCE_FILES:
                if not (root / filename).is_file():
                    raise ValueError("source release lacks " + filename)
            for filename in ("vendor/crossterm/Cargo.toml", "vendor/ratatui-crossterm/Cargo.toml", "vendor/clack-private-fs/Cargo.toml",
                             "vendor/clack-private-fs/src/lib.rs", "vendor/clack-private-fs/src/tests.rs",
                             "vendor/clack-private-fs/SAFETY.md", "src/main.rs", "src/lib.rs",
                             "tests/cli_acceptance.rs", "scripts/package.py", ".github/workflows/ci.yml"):
                if not (root / filename).is_file():
                    raise ValueError("source release lacks " + filename)
            if sha((root / "Cargo.lock").read_bytes()) != manifest["cargo_lock_sha256"]:
                raise ValueError("source lockfile checksum mismatch")
            digest = hashlib.sha256()
            sources = [root / "Cargo.toml", root / "Cargo.lock"]
            for name in ("src", "data", "vendor"):
                sources.extend(package.files_under(root / name))
            for source in sorted(sources):
                digest.update(source.relative_to(root).as_posix().encode() + b"\0")
                digest.update(hashlib.sha256(source.read_bytes()).digest())
            result["source_identity_sha256"] = digest.hexdigest()
            result["original_manifest_and_vendor_patch"] = "pass"
            return result
        target = manifest["target"]
        identity = package.TARGETS[target]
        binary = root / ("bin/clack.exe" if identity[0] == "windows" else "bin/clack")
        data = binary.read_bytes()
        if package.executable_identity(data) != identity or sha(data) != manifest["binary_sha256"]:
            raise ValueError("packaged executable target/hash mismatch")
        result.update(binary_sha256=sha(data), binary_bytes=len(data),
                      source_identity_sha256=manifest["source_identity_sha256"],
                      portable_cpu_flags=manifest["rustflags"] == "",
                      build_command=manifest["build_command"])
        if not result["portable_cpu_flags"] or len(data) > 15 * 1024 * 1024:
            raise ValueError("release packaging limits are not met")
        for filename in ("LICENSE", "README.md", "SPEC.md", "THIRD-PARTY-NOTICES.md",
                         "third-party/inventory.json", "third-party/source/option-ext-0.2.0/Cargo.toml",
                         "vendor/clack-private-fs/SAFETY.md", "vendor/clack-private-fs/LICENSE",
                         "vendor/crossterm/CLACK-PATCH.md", "vendor/crossterm/LICENSE",
                         "vendor/ratatui-crossterm/CLACK_PATCH.md", "vendor/ratatui-crossterm/LICENSE",
                         "share/man/man1/clack.1", "install.py"):
            if not (root / filename).is_file():
                raise ValueError("native package lacks " + filename)
        for shell in ("bash", "fish", "zsh", "elvish", "powershell"):
            if not (root / f"share/completions/clack.{shell}").stat().st_size:
                raise ValueError("empty generated completion")
        host_os = {"Darwin": "macos", "Linux": "linux", "Windows": "windows"}.get(platform.system())
        host_arch = {"arm64": "aarch64", "AMD64": "x86_64"}.get(platform.machine(), platform.machine())
        can_execute = identity[0] == host_os and (identity[1] == host_arch
                      or (host_os, host_arch, identity[1]) == ("macos", "aarch64", "x86_64"))
        if not can_execute:
            result["execution"] = "not run: foreign OS or unsupported host architecture"
            result["installation"] = "not run: native host required"
            return result
        version = command(binary, ["--version"], directory).decode().strip()
        if version != f"clack {manifest['version']}":
            raise ValueError("package version mismatch")
        doctor = json.loads(command(binary, ["doctor", "--json"], directory))
        build = doctor["build"]
        if build["test_hooks_enabled"] or build["debug_assertions"] or (build["target_os"], build["target_arch"]) != identity:
            raise ValueError("packaged executable does not report its production target")
        history = json.loads(command(binary, ["history", "--profile", "all", "--json"], directory))
        if history["results"]:
            raise ValueError("empty installation fixture returned history")
        for arguments, generated in [(["man"], root / "share/man/man1/clack.1"),
                *[(["completions", shell], root / f"share/completions/clack.{shell}")
                  for shell in ("bash", "fish", "zsh", "elvish", "powershell")]]:
            if command(binary, arguments, directory) != generated.read_bytes():
                raise ValueError("packaged documentation differs from its executable")
        result["execution"] = "pass: native host" if identity[1] == host_arch else "pass: OS architecture translation (Rosetta), not native x86-64 hardware"
        result["doctor_build"] = build
        prefix = directory / "installation"
        preserved = prefix / "user-data/history.keep"
        preserved.parent.mkdir(parents=True)
        preserved.write_bytes(b"untouched isolated user data")
        installed = install.install(root, prefix)
        command(installed, ["--version"], directory)
        if install.install(root, prefix) != installed:
            raise ValueError("idempotent installation returned a different binary")
        install.uninstall(prefix)
        if installed.exists() or preserved.read_bytes() != b"untouched isolated user data":
            raise ValueError("uninstall did not preserve ownership boundaries")
        result["installation"] = "pass: actual package installed, executed, idempotently reinstalled, uninstalled; user-data sentinel preserved"
        return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, default=package.ROOT / "dist")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--allow-missing-source", action="store_true")
    parser.add_argument("--single-artifact", action="store_true",
                        help="validate exactly one CI artifact without claiming a complete five-target release")
    args = parser.parse_args()
    archives = sorted([*args.directory.glob("*.tar.gz"), *args.directory.glob("*.zip")])
    if not archives:
        raise ValueError("no release archives found")
    results = [verify(archive) for archive in archives]
    if args.single_artifact and len(results) != 1:
        raise ValueError("--single-artifact requires exactly one archive")
    identities = {result["source_identity_sha256"] for result in results}
    if len(identities) != 1:
        raise ValueError("source and native archives have different source identities")
    required = {"aarch64-apple-darwin", "x86_64-apple-darwin", "aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"}
    targets = {result.get("target") for result in results}
    if not args.single_artifact and (not required <= targets or not targets.intersection({"x86_64-pc-windows-gnu", "x86_64-pc-windows-msvc"})):
        raise ValueError("not all five required release targets are packaged")
    if not args.single_artifact and not args.allow_missing_source and not any(result["kind"] == "source" for result in results):
        raise ValueError("complete source archive is missing")
    import datetime
    report = {"report_version": 1, "recorded_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "host": platform.platform(), "host_machine": platform.machine(), "artifacts": results,
              "scope": "one CI archive" if args.single_artifact else "complete required target collection",
              "status": "pass", "limitations": ["Foreign OS binaries were not executed; format and checksum checks do not establish runtime compatibility.",
              "Local command and installer checks do not establish actual-emulator, controlled-hardware or power-loss behavior.",
              "This report stays outside its archives so their SHA-256 values are not self-referential."]}
    destination = args.report or args.directory / "release-validation.json"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Verified {len(results)} archives; report {destination}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        raise SystemExit(f"release verification failed: {error}")
