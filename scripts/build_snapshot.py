#!/usr/bin/env python3
"""Build immutable native validation executables with exact input provenance."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

from package import ROOT, files_under, source_identity


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--target-dir', type=Path, default=Path('target'))
    parser.add_argument('--test-hooks', action='store_true')
    parser.add_argument('--lto', choices=['thin', 'off'], default='thin')
    parser.add_argument('--codegen-units', type=int, choices=[1, 16], default=1)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit('Output must be a new directory; previous evidence is never overwritten.')
    environment = os.environ.copy()
    environment.update(CARGO_ENCODED_RUSTFLAGS='', CARGO_TARGET_DIR=str(args.target_dir.resolve()),
                       CARGO_PROFILE_RELEASE_LTO=args.lto, CARGO_PROFILE_RELEASE_CODEGEN_UNITS=str(args.codegen_units),
                       CARGO_PROFILE_RELEASE_STRIP='symbols', CARGO_PROFILE_RELEASE_OPT_LEVEL='3',
                       CARGO_PROFILE_RELEASE_PANIC='unwind', CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS='false')
    if platform.system() == 'Darwin':
        environment['MACOSX_DEPLOYMENT_TARGET'] = '11.0'
    command = ['cargo', 'build', '--locked', '--offline', '--release', '--no-default-features',
               '--bin', 'clack', '--jobs', '2']
    if args.test_hooks:
        command += ['--features', 'test-hooks']
    else:
        command += ['--example', 'kernel_bench', '--example', 'render_bench']
    before = source_identity()
    paths = [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock']
    for directory in ('src', 'data', 'vendor'):
        paths.extend(files_under(ROOT / directory))
    inputs = [{'path': str(path.relative_to(ROOT)), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
              for path in sorted(paths)]
    subprocess.run(command, cwd=ROOT, env=environment, check=True)
    if source_identity() != before:
        raise SystemExit('Source changed during build; no immutable snapshot was published.')
    args.output.mkdir(parents=True)
    artifacts = []
    for name in (['clack'] if args.test_hooks else ['clack', 'examples/kernel_bench', 'examples/render_bench']):
        source = args.target_dir / 'release' / name
        destination = args.output / Path(name).name
        shutil.copy2(source, destination)
        artifacts.append({'path': str(destination.resolve()), 'sha256': hashlib.sha256(destination.read_bytes()).hexdigest(),
                          'bytes': destination.stat().st_size})
    report = {'schema_version': 1, 'created_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'source_identity_sha256': before, 'inputs': inputs, 'command': command,
              'rustc': subprocess.check_output(['rustc', '-Vv'], text=True), 'host': platform.platform(),
              'profile': {key: value for key, value in environment.items() if key.startswith('CARGO_PROFILE_RELEASE_')},
              'cargo_encoded_rustflags': '', 'macos_deployment_target': environment.get('MACOSX_DEPLOYMENT_TARGET'),
              'test_hooks_enabled': args.test_hooks, 'artifacts': artifacts}
    (args.output / 'build-manifest.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: report[key] for key in ('source_identity_sha256', 'command', 'artifacts')}, indent=2))


if __name__ == '__main__':
    main()
