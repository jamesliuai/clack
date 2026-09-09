#!/usr/bin/env python3
"""Record actual Rust compute results, executable hashes and child CPU usage."""
import argparse
import hashlib
import json
import platform
import resource
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", type=Path, required=True)
    parser.add_argument("--render", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--runs", type=int, default=30)
    args = parser.parse_args()
    results = []
    for kind, binary, arguments in [("kernel", args.kernel, []), ("render", args.render, [str(args.runs)])]:
        data = binary.read_bytes()
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        started = time.monotonic()
        child = subprocess.run([str(binary.resolve()), *arguments], check=True, text=True, capture_output=True)
        elapsed = time.monotonic() - started
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        if child.stderr:
            raise RuntimeError("compute benchmark unexpectedly wrote stderr")
        results.append({"kind": kind, "binary": str(binary), "sha256": hashlib.sha256(data).hexdigest(),
                        "binary_bytes": len(data), "process_wall_seconds": elapsed,
                        "process_user_cpu_seconds": after.ru_utime - before.ru_utime,
                        "process_system_cpu_seconds": after.ru_stime - before.ru_stime,
                        "result": json.loads(child.stdout)})
    report = {"schema_version": 1, "created_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "platform": platform.platform(), "architecture": platform.machine(), "profile": args.profile,
              "rustc": subprocess.check_output(["rustc", "-Vv"], text=True).strip(),
              "cargo_lock_sha256": hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest(),
              "scope": "Actual compute measurements on this host; not a controlled-reference, terminal-latency, or physical-keyboard result.",
              "measurements": results}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
