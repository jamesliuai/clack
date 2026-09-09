#!/usr/bin/env python3
"""Actual tmux or loopback OpenSSH transport under a synthetic outer Unix PTY.

No system service, user SSH configuration, user authentication file, or existing
tmux session is changed. SSH uses ephemeral host/client keys and loopback only.
The inner runner records terminal attributes independently of the outer PTY.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import pwd
import shlex
import socket
import subprocess
import sys
import tempfile
import time
from typing import Any

from pty_test import CheckFailure, PtyProcess, check, clean_terminal, environment_report, pause, wait_for

ROOT = Path(__file__).resolve().parents[1]
REMOTE_RUNNER = r'''
import json, os, subprocess, sys, termios
directory, binary = sys.argv[1:3]
terminal = os.open('/dev/tty', os.O_RDWR)
def attrs():
    value = termios.tcgetattr(terminal)
    value[3] &= ~getattr(termios, 'PENDIN', 0)
    value[-1] = [x.hex() if isinstance(x, bytes) else x for x in value[-1]]
    return value
before = attrs()
with open(directory + '/result.json', 'wb') as out, open(directory + '/diagnostics.txt', 'wb') as err:
    result = subprocess.run([binary, '--config', directory+'/config.toml', '--data-dir', directory+'/data', '--private', '--once', '--json', *sys.argv[3:]], stdin=subprocess.DEVNULL, stdout=out, stderr=err)
after = attrs()
with open(directory + '/lifecycle.json', 'w') as report:
    json.dump({'returncode': result.returncode, 'termios_restored': before == after}, report)
os.close(terminal)
sys.exit(result.returncode)
'''


class IsolatedSsh:
    def __init__(self, directory: Path):
        self.directory = directory
        self.server: subprocess.Popen[bytes] | None = None
        self.log = None
        for key in ("host", "client"):
            subprocess.run(["/usr/bin/ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(directory/key)], check=True, timeout=5)
        authorized = directory/"authorized_keys"
        authorized.write_text((directory/"client.pub").read_text())
        authorized.chmod(0o600)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            self.port = reservation.getsockname()[1]
        self.user = pwd.getpwuid(os.getuid()).pw_name
        self.config = directory/"sshd_config"
        self.config.write_text(f"""Port {self.port}
ListenAddress 127.0.0.1
HostKey {directory}/host
PidFile {directory}/pid
AuthorizedKeysFile {directory}/authorized_keys
StrictModes yes
UsePAM no
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
PermitEmptyPasswords no
PermitRootLogin no
AllowUsers {self.user}
DisableForwarding yes
PermitTTY yes
PermitUserRC no
PrintMotd no
PrintLastLog no
LogLevel ERROR
""")
        (directory/"known_hosts").write_text(f"[127.0.0.1]:{self.port} " + (directory/"host.pub").read_text())
        validate = subprocess.run(["/usr/sbin/sshd", "-t", "-f", str(self.config)], capture_output=True, timeout=5)
        check(validate.returncode == 0, "isolated sshd configuration rejected: " + validate.stderr.decode()[-300:])

    def __enter__(self) -> "IsolatedSsh":
        self.log = (self.directory/"sshd.log").open("wb")
        self.server = subprocess.Popen(["/usr/sbin/sshd", "-D", "-e", "-f", str(self.config)], stdout=self.log, stderr=self.log)
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            if self.server.poll() is not None:
                self.close()
                raise CheckFailure("isolated sshd exited before becoming ready")
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=0.1):
                    return self
            except OSError:
                time.sleep(0.02)
        self.close()
        raise CheckFailure("isolated loopback sshd did not become ready")

    def command(self, remote: str) -> list[str]:
        return ["/usr/bin/ssh", "-F", "/dev/null", "-tt", "-p", str(self.port), "-i", str(self.directory/"client"), "-o", f"UserKnownHostsFile={self.directory}/known_hosts", "-o", "GlobalKnownHostsFile=/dev/null", "-o", "StrictHostKeyChecking=yes", "-o", "IdentitiesOnly=yes", "-o", "BatchMode=yes", "-o", "ConnectTimeout=2", "-o", "LogLevel=ERROR", f"{self.user}@127.0.0.1", remote]

    def close(self) -> None:
        if self.server is not None and self.server.poll() is None:
            self.server.terminate()
            try:
                self.server.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.server.kill()
                self.server.wait(timeout=2)
        if self.log is not None:
            self.log.close()

    def __exit__(self, *_: object) -> None:
        self.close()


def exercise(binary: Path, directory: Path, transport: str, case: str, tmux: Path | None, ssh: IsolatedSsh | None) -> dict[str, Any]:
    directory.mkdir(mode=0o700)
    (directory/"config.toml").write_text("schema_version = 1\n")
    runner = directory/"runner.py"
    runner.write_text(REMOTE_RUNNER)
    exact = case == "exact_unicode_controls"
    target = "aé界🙂\tz\nend" if exact else "cat dog"
    app_args = ["--text", target] + (["--code"] if exact else [])
    inner = shlex.join([sys.executable, str(runner), str(directory), str(binary), *app_args])
    socket_path = directory/"mux.sock"
    if transport == "tmux":
        assert tmux is not None
        config = directory/"tmux.conf"
        pipe_command = shlex.quote("cat > " + shlex.quote(str(directory/"pane-output.bin")))
        config.write_text("set -g status off\nset -g mouse off\nset -s escape-time 10\nset -g exit-empty on\n" + f"set-hook -g after-new-session {{ pipe-pane -O {pipe_command} }}\n")
        command = [str(tmux), "-S", str(socket_path), "-f", str(config), "new-session", "-s", "clack-validation", inner]
    else:
        assert ssh is not None
        command = ssh.command("exec " + inner)
    # A concrete PTY slave is necessary: tmux passes its ttyname to the detached
    # server, where a literal /dev/tty alias no longer identifies this session.
    # The inner app still receives /dev/null and writes JSON independently.
    try:
        with PtyProcess(command, terminal_stdin=True, capture_stdout=False, cols=80, rows=24) as process:
            wait_for(process, lambda: "start typing" in process.screen.text().lower() and ("dog" in process.screen.text() or "end" in process.screen.text()), "complete target through " + transport, timeout=6)
            if case == "normal_completion":
                process.send(b"c")
                pause(process, 0.03)
                process.send(b"at dog")
            elif case == "restart_resize":
                process.send(b"ca")
                pause(process, 0.04)
                process.send(b"\x12")
                pause(process, 0.06)
                process.resize(120, 40)
                pause(process, 0.05)
                process.send(b"cat dog")
            elif case == "exact_unicode_controls":
                process.send("aé界🙂\tz\rend".encode())
                pause(process, 0.06)
                process.send(b"\x1b[15~")
            elif case == "paste_integrity":
                process.send(b"c")
                pause(process, 0.03)
                process.send(b"\x1b[200~injected\x1b[201~")
                pause(process, 0.03)
                process.send(b"at dog")
            elif case == "ctrl_c_cleanup":
                process.send(b"c")
                pause(process, 0.03)
                process.send(b"\x03")
            else:
                raise CheckFailure("unknown case " + case)
            outer_code = process.wait(timeout=6)
            # tmux uses the terminal's `Se` reset capability. On this recorded
            # xterm-256color terminfo it is steady block (2), while clack's inner
            # reset is independently checked in the captured pane protocol.
            clean_terminal(process, expected_cursor_style=2 if transport == "tmux" else 0)
            lifecycle = json.loads((directory/"lifecycle.json").read_text())
            check(lifecycle["termios_restored"], "inner app terminal attributes differ after " + case)
            if transport == "tmux":
                pane_output = (directory/"pane-output.bin").read_bytes()
                for sequence in (b"\x1b[0 q", b"\x1b[?1049l", b"\x1b[?2004l", b"\x1b[?1004l", b"\x1b[?25h"):
                    check(sequence in pane_output, "inner clack omitted cleanup sequence " + repr(sequence))
            check(lifecycle["returncode"] == (130 if case == "ctrl_c_cleanup" else 0), "inner app returned unexpected status")
            if transport == "ssh":
                check(outer_code == lifecycle["returncode"], "SSH did not preserve remote exit status")
            else:
                check(outer_code == 0, "tmux session did not exit normally")
            output = (directory/"result.json").read_bytes()
            check(b"\x1b" not in output, "terminal escapes entered the independent JSON stream")
            diagnostics = (directory/"diagnostics.txt").read_bytes()
            check(not diagnostics, "inner app emitted unexpected diagnostics: " + diagnostics.decode()[-200:])
            if case != "ctrl_c_cleanup":
                result = json.loads(output)
                expected = 10 if exact else 7
                check(result["outcome"] == "complete", "transport changed completion outcome")
                check(result["counts"]["attempts_total"] == expected, "transport lost/duplicated input or retained input across restart")
                check(result["counts"]["attempts_correct"] == expected, "transport changed Unicode/literal control matching")
                if case == "paste_integrity":
                    check(result["integrity"]["paste_attempted"] and not result["personal_best_eligible"], "paste integrity flag was lost by transport")
            return {"outer_exit": outer_code, "inner_exit": lifecycle["returncode"], "inner_termios_restored": True, "outer_modes_restored": True, "outer_cursor_reset": process.screen.cursor_style, "inner_cleanup_protocol_checked": transport == "tmux"}
    finally:
        if transport == "tmux" and tmux is not None:
            subprocess.run([str(tmux), "-S", str(socket_path), "kill-server"], capture_output=True, timeout=2, check=False)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--tmux", type=Path)
    parser.add_argument("--transport", choices=["tmux", "ssh"], action="append")
    parser.add_argument("--case", choices=["normal_completion", "restart_resize", "exact_unicode_controls", "paste_integrity", "ctrl_c_cleanup"], action="append")
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--execution-label", default="unspecified", help="record native/translated application execution; outer transport remains on the observed host")
    args = parser.parse_args()
    binary = args.binary.resolve()
    tmux = args.tmux.resolve() if args.tmux else None
    original_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    transports = args.transport or ["tmux", "ssh"]
    cases = args.case or ["normal_completion", "restart_resize", "exact_unicode_controls", "paste_integrity", "ctrl_c_cleanup"]
    if "tmux" in transports:
        capabilities = subprocess.run(["/usr/bin/infocmp", "-1", "-x", "xterm-256color"], capture_output=True, text=True, timeout=3)
        check("Se=\\E[2 q," in capabilities.stdout, "this harness's tmux cursor-reset expectation requires xterm-256color Se=ESC[2 q; record the local terminfo before adapting")
    results = []
    server_log = ""
    # The tmux socket needs a short path, while StrictModes correctly refuses
    # an authorized_keys path below the world-writable /private/tmp directory.
    # Keep the ephemeral SSH credentials beneath the user's workspace instead
    # of weakening authentication checks or changing user SSH configuration.
    ssh_parent = ROOT / "target" / "transport"
    ssh_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="clack-transport-", dir="/private/tmp") as temp, \
            tempfile.TemporaryDirectory(prefix="ssh-", dir=ssh_parent) as ssh_temp:
        directory = Path(temp)
        directory.chmod(0o700)
        ssh_directory = Path(ssh_temp)
        ssh_directory.chmod(0o700)
        ssh = None
        try:
            if "ssh" in transports:
                ssh = IsolatedSsh(ssh_directory)
                ssh.__enter__()
            for transport in transports:
                for case in cases:
                    started = time.monotonic()
                    try:
                        detail = exercise(binary, directory/(transport + "-" + case), transport, case, tmux, ssh)
                        status = "pass"
                    except (CheckFailure, OSError, ValueError, TimeoutError, subprocess.TimeoutExpired) as error:
                        detail, status = str(error), "fail"
                    row = {"transport": transport, "case": case, "status": status, "seconds": round(time.monotonic()-started, 6), "detail": detail}
                    results.append(row)
                    print(f"{status.upper():5} {transport}/{case}: {detail}", flush=True)
        finally:
            if ssh is not None:
                ssh.close()
                server_log = (ssh_directory/"sshd.log").read_text()[-2000:]
    check(hashlib.sha256(binary.read_bytes()).hexdigest() == original_hash, "test binary changed while running")
    report = {"report_version": 1, "kind": "actual_local_transport_synthetic_outer_pty", "binary": str(binary), "binary_sha256": original_hash, "platform": sys.platform, "created_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "tmux_version": subprocess.run([str(tmux), "-V"], capture_output=True, text=True).stdout.strip() if tmux else None, "ssh_version": subprocess.run(["/usr/bin/ssh", "-V"], capture_output=True, text=True).stderr.strip(), "ssh_server_log": server_log, "limitations": ["Outer terminal is synthetic xterm-256color PTY, not a graphical emulator or physical display.", "SSH is an actual authenticated encrypted loopback transport; no remote network latency or remote OS is simulated.", "Separate inner and outer terminal attributes and modes are checked; no system service, user authentication file, or existing tmux session is modified."], "results": results}
    report["environment"] = environment_report()
    report["execution_label"] = args.execution_label
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2)+"\n")
    return int(any(row["status"] != "pass" for row in results))


if __name__ == "__main__":
    raise SystemExit(main())
