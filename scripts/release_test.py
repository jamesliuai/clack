#!/usr/bin/env python3
"""Adversarial package/installer tests; operate only on isolated temporary files."""
from __future__ import annotations

import contextlib
import io
import json
import os
import platform
import stat
import struct
import subprocess
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import install
import package
import verify_release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="clack-release-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.package = self.root / "package"
        self.prefix = self.root / "prefix"
        self.package.mkdir()
        machine = platform.machine().lower()
        architecture = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
        self.target = architecture + {"Darwin": "-apple-darwin", "Linux": "-unknown-linux-gnu", "Windows": "-pc-windows-msvc"}[platform.system()]
        self.binary_path = "bin/clack.exe" if platform.system() == "Windows" else "bin/clack"
        self.files = {self.binary_path: b"synthetic executable fixture", "LICENSE": b"synthetic license", "share/man/man1/clack.1": b"manual"}
        for path, data in self.files.items():
            destination = self.package / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        self.manifest = {"package_schema_version": 1, "kind": "native", "version": "1.0.0", "target": self.target,
                         "binary_sha256": install.sha(self.files[self.binary_path]),
                         "files": [{"path": path, "size": len(data), "sha256": install.sha(data), "mode": 0o644} for path, data in self.files.items()]}
        self.write_manifest()
        self.quiet = contextlib.redirect_stdout(io.StringIO())
        self.quiet.__enter__()
        self.addCleanup(self.quiet.__exit__, None, None, None)

    def write_manifest(self):
        (self.package / "manifest.json").write_text(json.dumps(self.manifest))

    def installed_bundle(self):
        _, record = install.load_record(self.prefix)
        return self.prefix / record["bundle"]

    def test_install_idempotence_and_uninstall_preserve_user_data(self):
        private = self.prefix / "user-data/results.db"
        private.parent.mkdir(parents=True)
        private.write_bytes(b"must remain")
        binary = install.install(self.package, self.prefix)
        self.assertEqual(binary.read_bytes(), self.files[self.binary_path])
        self.assertEqual(install.install(self.package, self.prefix), binary)
        if os.name != "nt":
            self.assertEqual(binary.stat().st_mode & 0o777, 0o755)
            self.assertEqual((self.prefix / "share/clack/install.json").stat().st_mode & 0o777, 0o600)
        install.uninstall(self.prefix)
        self.assertFalse(binary.exists())
        self.assertEqual(private.read_bytes(), b"must remain")
        self.assertFalse((self.prefix / "share/clack/install.json").exists())

    def test_checksum_failure_precedes_any_installation(self):
        (self.package / "LICENSE").write_bytes(b"tampered license")
        with self.assertRaisesRegex(ValueError, "size|checksum"):
            install.install(self.package, self.prefix)
        self.assertFalse(self.prefix.exists())

    def test_refuses_existing_unowned_binary(self):
        binary = self.prefix / self.binary_path
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"user binary")
        with self.assertRaisesRegex(ValueError, "overwrite"):
            install.install(self.package, self.prefix)
        self.assertEqual(binary.read_bytes(), b"user binary")

    def test_modified_installed_binary_is_not_removed(self):
        binary = install.install(self.package, self.prefix)
        binary.write_bytes(b"user modification")
        with self.assertRaisesRegex(ValueError, "modified"):
            install.uninstall(self.prefix)
        self.assertEqual(binary.read_bytes(), b"user modification")

    def test_modified_and_unregistered_bundle_files_survive_uninstall(self):
        install.install(self.package, self.prefix)
        bundle = self.installed_bundle()
        (bundle / "LICENSE").write_bytes(b"modified license")
        (bundle / "personal.txt").write_bytes(b"personal")
        install.uninstall(self.prefix)
        self.assertEqual((bundle / "LICENSE").read_bytes(), b"modified license")
        self.assertEqual((bundle / "personal.txt").read_bytes(), b"personal")
        self.assertFalse((bundle / "share/man/man1/clack.1").exists())

    def test_failed_publication_rolls_back_only_new_owned_files(self):
        original_link = os.link
        def fail_record(source, destination, *args, **kwargs):
            if Path(destination).name == "install.json":
                raise OSError("injected record publication error")
            return original_link(source, destination, *args, **kwargs)
        with mock.patch.object(install.os, "link", side_effect=fail_record):
            with self.assertRaisesRegex(OSError, "injected"):
                install.install(self.package, self.prefix)
        self.assertFalse((self.prefix / self.binary_path).exists())
        self.assertEqual(list((self.prefix / "share/clack").iterdir()), [])

    def test_manifest_paths_reject_traversal_aliases_and_duplicates(self):
        for path in ("../outside", "/absolute", "C:/absolute", "a\\b", "a/../b", "./a", "a//b", "a\x00b"):
            with self.subTest(path=repr(path)), self.assertRaises(ValueError):
                install.relative_path(path)
        self.manifest["files"].append(self.manifest["files"][0])
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, "duplicate"):
            install.install(self.package, self.prefix)
        self.assertFalse(self.prefix.exists())

    @unittest.skipIf(os.name == "nt", "creating Windows symlinks depends on developer mode")
    def test_symlink_package_file_and_destination_parent_are_refused(self):
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "LICENSE").write_bytes(self.files["LICENSE"])
        (self.package / "LICENSE").unlink()
        (self.package / "LICENSE").symlink_to(outside / "LICENSE")
        with self.assertRaisesRegex(ValueError, "symbolic"):
            install.install(self.package, self.prefix)
        (self.package / "LICENSE").unlink()
        (self.package / "LICENSE").write_bytes(self.files["LICENSE"])
        self.prefix.mkdir()
        (self.prefix / "bin").symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symbolic"):
            install.install(self.package, self.prefix)
        self.assertFalse((outside / Path(self.binary_path).name).exists())

    @unittest.skipIf(os.name == "nt", "creating Windows symlinks depends on developer mode")
    def test_uninstall_never_follows_replaced_bundle_parent(self):
        install.install(self.package, self.prefix)
        bundle = self.installed_bundle()
        manual = bundle / "share/man/man1/clack.1"
        manual.unlink()
        manual.parent.rmdir()
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "clack.1").write_bytes(b"manual")
        manual.parent.symlink_to(outside, target_is_directory=True)
        install.uninstall(self.prefix)
        self.assertEqual((outside / "clack.1").read_bytes(), b"manual")
        self.assertTrue(manual.parent.is_symlink())

    def test_windows_reparse_attributes_block_traversal_and_preserve_bundle_children(self):
        install.install(self.package, self.prefix)
        bundle = self.installed_bundle().resolve()
        protected = bundle / "share/man/man1"
        (protected / "empty-user-directory").mkdir()
        original = Path.lstat
        def windows_reparse(path, *args, **kwargs):
            metadata = original(path, *args, **kwargs)
            if path == protected:
                return SimpleNamespace(st_mode=metadata.st_mode,
                                       st_file_attributes=stat.FILE_ATTRIBUTE_REPARSE_POINT)
            return metadata
        with mock.patch.object(Path, "lstat", windows_reparse):
            with self.assertRaisesRegex(ValueError, "reparse"):
                install.no_links(bundle, Path("share/man/man1/clack.1"))
            install.uninstall(self.prefix)
        self.assertEqual((protected / "clack.1").read_bytes(), b"manual")
        self.assertTrue((protected / "empty-user-directory").is_dir())

    @unittest.skipUnless(os.name == "nt", "requires a native Windows directory junction")
    def test_native_windows_junction_uninstall_preserves_matching_external_files(self):
        install.install(self.package, self.prefix)
        bundle = self.installed_bundle()
        junction = bundle / "share/man/man1"
        (junction / "clack.1").unlink()
        junction.rmdir()
        outside = self.root / "outside junction"
        outside.mkdir()
        (outside / "clack.1").write_bytes(b"manual")
        (outside / "empty-user-directory").mkdir()
        # Only generated fixture paths enter this command, each quoted. Refuse
        # expansion characters rather than invoking cmd with an ambiguous path.
        self.assertFalse(any(character in str(junction) + str(outside) for character in '%!"\r\n'))
        # Pass cmd's command line directly: list2cmdline would backslash-escape
        # the embedded quotes using CRT rules, which cmd does not understand.
        result = subprocess.run(
            f'cmd.exe /d /v:off /s /c "mklink /J "{junction}" "{outside}""',
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)
        self.assertEqual(result.returncode, 0,
                         f"junction creation failed: {result.stdout!r} {result.stderr!r}")
        self.assertTrue(install.link_like(junction))
        install.uninstall(self.prefix)
        self.assertEqual((outside / "clack.1").read_bytes(), b"manual")
        self.assertTrue((outside / "empty-user-directory").is_dir())
        junction.rmdir()

    def test_foreign_os_package_is_not_installed(self):
        self.manifest["target"] = "x86_64-pc-windows-msvc" if platform.system() != "Windows" else "aarch64-apple-darwin"
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, "operating system"):
            install.install(self.package, self.prefix)

    def test_processor_mismatch_is_refused_before_prefix_writes(self):
        for host_os, machine, target in [
            ("Linux", "x86_64", "aarch64-unknown-linux-gnu"),
            ("Linux", "aarch64", "x86_64-unknown-linux-gnu"),
            ("Darwin", "x86_64", "aarch64-apple-darwin"),
            ("Windows", "ARM64", "x86_64-pc-windows-msvc"),
        ]:
            self.manifest["target"] = target
            self.write_manifest()
            with self.subTest(host=host_os, machine=machine), \
                 mock.patch.object(install.platform, "system", return_value=host_os), \
                 mock.patch.object(install.platform, "machine", return_value=machine):
                with self.assertRaisesRegex(ValueError, "processor architecture"):
                    install.install(self.package, self.prefix)
                self.assertFalse(self.prefix.exists())

    @unittest.skipIf(os.name == "nt", "fixture uses the macOS executable filename")
    def test_macos_arm_host_keeps_explicit_rosetta_install_compatibility(self):
        self.manifest["target"] = "x86_64-apple-darwin"
        self.write_manifest()
        with mock.patch.object(install.platform, "system", return_value="Darwin"), \
             mock.patch.object(install.platform, "machine", return_value="arm64"):
            installed = install.install(self.package, self.prefix)
        self.assertEqual(installed.read_bytes(), self.files[self.binary_path])
        install.uninstall(self.prefix)

    def test_native_headers_identify_os_and_architecture(self):
        for cpu, architecture in ((0x01000007, "x86_64"), (0x0100000C, "aarch64")):
            self.assertEqual(package.executable_identity(b"\xcf\xfa\xed\xfe" + struct.pack("<I", cpu)), ("macos", architecture))
        for cpu, architecture in ((62, "x86_64"), (183, "aarch64")):
            data = bytearray(20)
            data[:6] = b"\x7fELF\x02\x01"
            struct.pack_into("<H", data, 18, cpu)
            self.assertEqual(package.executable_identity(data), ("linux", architecture))
        data = bytearray(72)
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 60, 64)
        data[64:68] = b"PE\0\0"
        struct.pack_into("<H", data, 68, 0x8664)
        self.assertEqual(package.executable_identity(data), ("windows", "x86_64"))
        with self.assertRaises(ValueError):
            package.executable_identity(b"not an executable")

    def test_distribution_rejects_global_and_target_specific_cpu_flags(self):
        args = SimpleNamespace(epoch=0, source=False, target=self.target)
        for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS",
                    "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
                    "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS"):
            with self.subTest(key=key), mock.patch.dict(os.environ, {key: "-Ctarget-cpu=native"}), \
                    mock.patch.object(package, "run") as build:
                with self.assertRaisesRegex(ValueError, "custom RUSTFLAGS"):
                    package.package(args)
                build.assert_not_called()
        for key in ("CFLAGS", "CXXFLAGS", "CFLAGS_aarch64_apple_darwin", "TARGET_CFLAGS"):
            with self.subTest(key=key), mock.patch.dict(os.environ, {key: "-march=native"}), \
                    mock.patch.object(package, "run") as build:
                with self.assertRaisesRegex(ValueError, "custom C/C\\+\\+ flags"):
                    package.package(args)
                build.assert_not_called()

    def test_archives_are_deterministic_and_preserve_original_manifest(self):
        files = {"Cargo.toml": (b'[patch.crates-io]\ncrossterm = { path = "vendor/crossterm" }\n', 0o644), "bin/clack": (b"binary", 0o755)}
        for windows in (False, True):
            first = package.archive_bytes("clack-fixture", files, 0, windows)
            self.assertEqual(first, package.archive_bytes("clack-fixture", dict(reversed(list(files.items()))), 0, windows))
            if windows:
                with zipfile.ZipFile(io.BytesIO(first)) as archive:
                    self.assertEqual(archive.read("clack-fixture/Cargo.toml"), files["Cargo.toml"][0])
                    self.assertEqual(archive.getinfo("clack-fixture/bin/clack").external_attr >> 16 & 0o777, 0o755)
            else:
                with tarfile.open(fileobj=io.BytesIO(first), mode="r:gz") as archive:
                    self.assertEqual(archive.extractfile("clack-fixture/Cargo.toml").read(), files["Cargo.toml"][0])
                    self.assertEqual(archive.getmember("clack-fixture/bin/clack").mode, 0o755)


    def test_archive_verifier_checks_both_outer_and_payload_checksums_before_extraction(self):
        files = {"LICENSE": (b"license fixture", 0o644)}
        manifest = {"package_schema_version": 1, "kind": "native", "files": [
            {"path": name, "size": len(data), "sha256": install.sha(data), "mode": mode}
            for name, (data, mode) in files.items()]}
        files["manifest.json"] = (json.dumps(manifest).encode(), 0o644)
        for windows in (False, True):
            archive = self.root / ("fixture.zip" if windows else "fixture.tar.gz")
            raw = package.archive_bytes("clack-fixture", files, 0, windows)
            archive.write_bytes(raw)
            checksum = archive.with_name(archive.name + ".sha256")
            checksum.write_text(f"{install.sha(raw)}  {archive.name}\n")
            output = self.root / ("zip-output" if windows else "tar-output")
            parsed, extracted, _ = verify_release.unpack(archive, output)
            self.assertEqual(parsed, manifest)
            self.assertEqual((extracted / "LICENSE").read_bytes(), b"license fixture")
            checksum.write_text(f"{'0' * 64}  {archive.name}\n")
            untouched = self.root / "failed-extraction"
            with self.assertRaisesRegex(ValueError, "archive checksum"):
                verify_release.unpack(archive, untouched)
            self.assertFalse(untouched.exists())
            corrupted = dict(files)
            corrupted["LICENSE"] = (b"changed fixture", 0o644)
            raw = package.archive_bytes("clack-fixture", corrupted, 0, windows)
            archive.write_bytes(raw)
            checksum.write_text(f"{install.sha(raw)}  {archive.name}\n")
            with self.assertRaisesRegex(ValueError, "payload differs"):
                verify_release.unpack(archive, untouched)
            self.assertFalse(untouched.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
