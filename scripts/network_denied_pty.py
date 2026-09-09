#!/usr/bin/env python3
"""Bounded macOS application workflow under process-local IP denial.

This proves operation with IP networking denied, not absence of attempted
network calls or packet traffic. Unix IPC remains allowed for the poll waker.
No system profile or service is changed. All application files are temporary.
"""
from __future__ import annotations

import argparse
import errno
import hashlib
import json
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time

from pty_test import CheckFailure, PtyProcess, check, clean_terminal, environment_report, one_json, pause, ready


PROFILE = """(version 1)
(allow default)
(deny network-bind (local ip))
(deny network-inbound (local ip))
(deny network-outbound (remote ip))
"""

CONTROL = r'''
import errno, json, socket, sys
left, right = socket.socketpair()
left.sendall(b'poll-waker')
assert right.recv(10) == b'poll-waker'
left.close()
right.close()
results = {'unix_socketpair': 'pass', 'ip': []}
for family, host, port in json.loads(sys.argv[1]):
    item = {'family': family}
    with socket.socket(family) as sock:
        sock.settimeout(1)
        item['tcp_connect_errno'] = sock.connect_ex((host, port))
    with socket.socket(family) as sock:
        try:
            sock.bind((host, 0))
            item['bind_errno'] = 0
        except OSError as error:
            item['bind_errno'] = error.errno
    with socket.socket(family, socket.SOCK_DGRAM) as sock:
        try:
            sock.sendto(b'negative-control', (host, port))
            item['udp_send_errno'] = 0
        except OSError as error:
            item['udp_send_errno'] = error.errno
    results['ip'].append(item)
print(json.dumps(results))
'''


def controls() -> dict:
    listeners = []
    addresses = []
    try:
        for family, host in [(socket.AF_INET, "127.0.0.1"), (socket.AF_INET6, "::1")]:
            listener = socket.socket(family)
            listeners.append(listener)
            listener.bind((host, 0))
            listener.listen(4)
            port = listener.getsockname()[1]
            with socket.socket(family) as client:
                client.settimeout(1)
                check(client.connect_ex((host, port)) == 0, "unrestricted loopback control failed")
            accepted, _ = listener.accept()
            accepted.close()
            addresses.append([family, host, port])
        command = ["/usr/bin/sandbox-exec", "-p", PROFILE, sys.executable, "-c", CONTROL,
                   json.dumps(addresses)]
        result = subprocess.run(command, capture_output=True, text=True, timeout=5, check=False)
        check(result.returncode == 0, "sandbox control could not run: " + result.stderr[-1200:])
        observed = json.loads(result.stdout)
        check(observed["unix_socketpair"] == "pass", "sandbox denied poll-waker style IPC")
        for item in observed["ip"]:
            for operation in ("tcp_connect_errno", "bind_errno", "udp_send_errno"):
                check(item[operation] in (errno.EPERM, errno.EACCES),
                      f"IP negative control was not permission-denied: {item}")
        observed["unrestricted_ipv4_ipv6_loopback_connect"] = "pass"
        return observed
    finally:
        for listener in listeners:
            listener.close()


def exercise(binary: Path) -> dict:
    with tempfile.TemporaryDirectory(prefix="clack-network-denied-") as raw:
        directory = Path(raw)
        config, data = directory / "config.toml", directory / "data"
        config.write_text("schema_version = 1\n")
        command = ["/usr/bin/sandbox-exec", "-p", PROFILE, str(binary), "--config", str(config),
                   "--data-dir", str(data), "--text", "cat dog", "--once", "--json"]
        with PtyProcess(command) as process:
            ready(process)
            process.send(b"c")
            pause(process, 0.03)
            process.send(b"at dog")
            check(process.wait(timeout=6) == 0, "network-denied app did not complete")
            result = one_json(process)
            check(result["outcome"] == "complete", "network-denied app result was incomplete")
            check(result["counts"]["attempts_total"] == 7, "network-denied input lost or duplicated")
            clean_terminal(process)
        databases = list(data.glob("*.sqlite3"))
        check(len(databases) == 1, "network-denied run did not create one result database")
        with sqlite3.connect(databases[0]) as database:
            check(database.execute("SELECT COUNT(*) FROM results").fetchone()[0] == 1,
                  "network-denied result was not committed exactly once")
            check(database.execute("PRAGMA quick_check").fetchone()[0] == "ok",
                  "network-denied result database integrity failed")
        return {"startup": "pass", "typing": "pass", "completed_json_result": "pass",
                "sqlite_save": "pass", "terminal_cleanup": "pass", "attempts_total": 7}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--controls-only", action="store_true")
    parser.add_argument("--execution-label", default="unspecified")
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("macOS sandbox-exec is required")
    if not args.controls_only and args.binary is None:
        parser.error("--binary is required unless --controls-only is selected")
    binary = args.binary.resolve() if args.binary else None
    digest = hashlib.sha256(binary.read_bytes()).hexdigest() if binary else None
    report = {"report_version": 1, "kind": "process_local_ip_denied_unix_pty",
              "created_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "environment": environment_report(), "execution_label": args.execution_label,
              "binary": str(binary) if binary else None, "binary_sha256": digest,
              "sandbox_profile": PROFILE,
              "limitations": ["Operation under denied IP networking is not packet capture or a trace of attempted calls.",
                              "The macOS process-local sandbox profile permits Unix IPC and otherwise leaves default permissions unchanged.",
                              "Synthetic outer PTY supplies no graphical emulator or physical display validation."]}
    try:
        report["controls"] = controls()
        if not args.controls_only:
            report["workflow"] = exercise(binary)
        if binary:
            check(hashlib.sha256(binary.read_bytes()).hexdigest() == digest,
                  "binary changed during network-denied verification")
        report["status"] = "pass"
    except (CheckFailure, OSError, TimeoutError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
        report["status"], report["detail"] = "fail", str(error)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return int(report["status"] != "pass")


if __name__ == "__main__":
    raise SystemExit(main())
