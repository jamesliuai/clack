#!/usr/bin/env python3
"""Review the locked native dependency graph and reproduce bundled notices.

No network is used. Run cargo fetch --locked explicitly beforehand if this
machine does not yet have the locked crate sources. Requires Python 3.11+.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TARGETS = (
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin", "aarch64-apple-darwin", "x86_64-pc-windows-msvc", "x86_64-pc-windows-gnu",
)
# A changed/new expression requires an explicit review, not an automatic choice.
CHOICES = {
    "MIT": ["MIT"], "MIT OR Apache-2.0": ["MIT"],
    "MIT/Apache-2.0": ["MIT"], "Apache-2.0 OR MIT": ["MIT"],
    "Apache-2.0/MIT": ["MIT"], "Apache-2.0 / MIT": ["MIT"],
    "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT": ["MIT"],
    "MIT OR Apache-2.0 OR LGPL-2.1-or-later": ["MIT"],
    "BSD-2-Clause OR Apache-2.0 OR MIT": ["MIT"],
    "Unlicense/MIT": ["MIT"], "Unlicense OR MIT": ["MIT"],
    "Zlib OR Apache-2.0 OR MIT": ["MIT"], "MIT OR Apache-2.0 OR Zlib": ["MIT"],
    "Apache-2.0": ["Apache-2.0"], "Apache-2.0 OR BSL-1.0": ["Apache-2.0"],
    "Zlib": ["Zlib"], "MPL-2.0": ["MPL-2.0"],
    "(MIT OR Apache-2.0) AND Unicode-3.0": ["MIT", "Unicode-3.0"],
}
LOCAL_PACKAGES = {
    ("crossterm", "0.29.0"): ROOT / "vendor/crossterm/Cargo.toml",
    ("clack-private-fs", "1.0.0"): ROOT / "vendor/clack-private-fs/Cargo.toml",
}


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def cargo(*args: str) -> str:
    result = subprocess.run(["cargo", *args], cwd=ROOT, check=True,
                            stdout=subprocess.PIPE, text=True)
    return result.stdout


def native_graphs() -> dict[tuple[str, str], list[str]]:
    graphs: dict[tuple[str, str], list[str]] = {}
    for target in TARGETS:
        tree = cargo("tree", "--locked", "--offline", "--target", target,
                     "--edges", "normal,build", "--prefix", "none", "--format", "{p}")
        for line in tree.splitlines():
            match = re.match(r"^([A-Za-z0-9_-]+) v([^ ]+)", line)
            if match:
                key = (match.group(1), match.group(2))
                if target not in graphs.setdefault(key, []):
                    graphs[key].append(target)
    return graphs


def notice_files(directory: Path) -> list[Path]:
    return sorted(p for p in directory.rglob("*") if p.is_file() and not p.is_symlink()
                  and any(word in p.name.lower() for word in ("license", "copying", "notice", "copyright"))
                  and "test" not in str(p.relative_to(directory)).lower())


def generate(destination: Path) -> dict:
    metadata = json.loads(cargo("metadata", "--locked", "--offline", "--format-version", "1"))
    graphs = native_graphs()
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    checksums = {(p["name"], p["version"]): p.get("checksum") for p in lock["package"]}
    packages = []
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        name, version = package["name"], package["version"]
        if name == "clack-local":
            continue
        expression = package.get("license")
        if expression not in CHOICES:
            raise ValueError(f"unreviewed license expression: {name} {version}: {expression}")
        key = (name, version)
        if key not in checksums:
            raise ValueError(f"package missing from Cargo.lock: {name} {version}")
        source = package.get("source")
        if source is None and LOCAL_PACKAGES.get(key) != Path(package["manifest_path"]):
            raise ValueError(f"unreviewed local dependency: {name} {version}")
        if source is not None and source != "registry+https://github.com/rust-lang/crates.io-index":
            raise ValueError(f"unreviewed dependency source: {name} {version}")
        directory = Path(package["manifest_path"]).parent
        selected_targets = sorted(graphs.get(key, []))
        notices = []
        for path in notice_files(directory):
            relative = Path("licenses") / f"{name}-{version}" / path.relative_to(directory)
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            data = path.read_bytes()
            target.write_bytes(data)
            notices.append({"path": relative.as_posix(), "sha256": sha(data)})
        if selected_targets and not notices:
            raise ValueError(f"native release dependency has no license notice: {name} {version}")
        corresponding_source = None
        if expression == "MPL-2.0":
            # Include the complete, unmodified covered crate alongside binaries.
            corresponding_source = f"source/{name}-{version}"
            output = destination / corresponding_source
            for path in sorted(directory.rglob("*")):
                if path.is_file() and not path.is_symlink() and path.name != ".cargo-ok":
                    target = output / path.relative_to(directory)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(path.read_bytes())
        packages.append({
            "name": name, "version": version, "spdx_expression": expression,
            "selected_licenses": CHOICES[expression], "source": source or directory.relative_to(ROOT).as_posix(),
            "registry_checksum": checksums[key], "repository": package.get("repository"),
            "native_release_targets": selected_targets, "license_notices": notices,
            "corresponding_source": corresponding_source,
            "scope_note": None if selected_targets else "Locked development or other-target dependency; not in the native release graphs.",
        })
    report = {"schema_version": 1, "cargo_lock_sha256": sha((ROOT / "Cargo.lock").read_bytes()),
              "scope": "All locked dependencies; target membership is normal/build edges for the five OS/architecture configurations including both Windows MSVC and GNU ABIs, and may include build-only tools.",
              "targets": list(TARGETS), "packages": packages}
    (destination / "inventory.json").write_text(json.dumps(report, ensure_ascii=True, indent=2) + "\n")
    return report


def tree_hashes(directory: Path) -> dict[str, str]:
    return {p.relative_to(directory).as_posix(): sha(p.read_bytes())
            for p in sorted(directory.rglob("*")) if p.is_file()}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if committed notices differ from the lockfile")
    args = parser.parse_args()
    destination = ROOT / "third-party"
    with tempfile.TemporaryDirectory(prefix="clack-licenses-") as temporary:
        staged = Path(temporary)
        report = generate(staged)
        expected = tree_hashes(staged)
        if args.check:
            actual = tree_hashes(destination)
            if actual != expected:
                changed = sorted(k for k in actual.keys() | expected.keys() if actual.get(k) != expected.get(k))
                raise ValueError("third-party notices require regeneration: " + ", ".join(changed[:12]))
        else:
            if destination.exists():
                shutil.rmtree(destination)
            shutil.copytree(staged, destination)
    print(f"Reviewed {len(report['packages'])} locked dependencies; {len(expected)} notice/source files {'verified' if args.check else 'generated'}.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"license review failed: {error}")
