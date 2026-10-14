#!/usr/bin/env python3
"""Build all staged crates.io packages without publishing or using checkout paths.

Cargo 1.95 stages interdependent workspace packages in a temporary local registry.
The temporary workspace includes the terminal forks, which remain standalone in
the development checkout. The shared verification lockfile may need additional
registry dependencies to reconcile the standalone crates' development graphs.
"""
from __future__ import annotations

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path

import package


def stage_workspace(destination: Path) -> None:
    for name in package.SOURCE_FILES:
        shutil.copy2(package.ROOT / name, destination / name)
    for directory in package.SOURCE_DIRS:
        for source in package.files_under(package.ROOT / directory, distribution=True):
            target = destination / source.relative_to(package.ROOT)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
    manifest = destination / "Cargo.toml"
    text = manifest.read_text()
    original = 'members = ["vendor/clack-private-fs"]'
    expanded = 'members = ["vendor/clack-private-fs", "vendor/crossterm", "vendor/ratatui-crossterm"]'
    excluded = 'exclude = ["vendor/crossterm", "vendor/ratatui-crossterm"]\n'
    if text.count(original) != 1 or text.count(excluded) != 1:
        raise ValueError("update staged workspace membership for the changed Cargo manifest")
    manifest.write_text(text.replace(original, expanded).replace(excluded, ""))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--toolchain", default="1.95.0")
    parser.add_argument("--target-dir", type=Path, default=package.ROOT / "target/crate-packages")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="clack-crate-verification-") as temporary:
        workspace = Path(temporary)
        stage_workspace(workspace)
        command = [
            "cargo", f"+{args.toolchain}", "package", "--workspace",
            "--allow-dirty", "--target-dir", str(args.target_dir.resolve()),
        ]
        subprocess.run(command, cwd=workspace, check=True)


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.returncode) from None
