#!/usr/bin/env python3
"""Unix PTY integration checks for clack; no third-party Python packages required.

The PTY is the controlling terminal. Source stdin, JSON stdout, and diagnostics
use independent pipes. This is terminal-protocol/lifecycle evidence, not an
actual emulator or a physical key-to-photon measurement. Import PtyProcess from
this module for benchmark consumers. Run --self-test before trusting the harness.
"""

from __future__ import annotations

import argparse
import codecs
import collections
import dataclasses
import errno
import hashlib
import json
import os
from pathlib import Path
import platform
import selectors
import signal
import struct
import subprocess
import sys
import tempfile
import time
import unicodedata
from typing import Any, Callable, Optional

if os.name != "posix":
    raise SystemExit("pty_test.py requires a Unix PTY; Windows console checks are separate")

import fcntl
import termios


ROOT = Path(__file__).resolve().parents[1]
MAX_CAPTURE = 8 * 1024 * 1024
MAX_OBSERVATIONS = 4096
F2 = b"\x1b[12~"
F5 = b"\x1b[15~"
CTRL_C = b"\x03"
CTRL_R = b"\x12"
BACKSPACE = b"\x7f"


def environment_report() -> dict[str, Any]:
    """Record this actual host; never reuse the earlier report's OS version."""
    uname = os.uname()
    os_build = None
    if sys.platform == "darwin":
        observed = subprocess.run(["/usr/bin/sw_vers", "-buildVersion"], capture_output=True,
                                  text=True, timeout=3, check=False)
        if observed.returncode == 0:
            os_build = observed.stdout.strip()
    return {
        "operating_system": platform.system(),
        "operating_system_version": platform.mac_ver()[0] if sys.platform == "darwin" else platform.release(),
        "operating_system_build": os_build,
        "kernel_release": uname.release,
        "architecture": platform.machine(),
        "terminal_environment": {
            "TERM": "xterm-256color",
            "COLORTERM": "truecolor",
            "NO_COLOR_nonempty": bool(os.environ.get("NO_COLOR")),
        },
    }

# The keeper remains the controlling-session leader after clack exits. macOS
# otherwise revokes the PTY immediately and post-exit tcgetattr returns ENOTTY.
# Its startup is setup work: started_ns marks the actual clack fork, externally.
SESSION_KEEPER = r'''
import json, os, signal, sys, time
status_fd, release_fd, tty_fd = map(int, sys.argv[1:4])
command = sys.argv[4:]
gate_read, gate_write = os.pipe()
started_ns = time.monotonic_ns()
pid = os.fork()
if pid == 0:
    os.close(gate_write)
    os.setpgid(0, 0)
    os.read(gate_read, 1)
    os.close(gate_read)
    os.close(status_fd)
    os.close(release_fd)
    if tty_fd > 2:
        os.close(tty_fd)
    try:
        os.execvpe(command[0], command, os.environ)
    except OSError as error:
        print('PTY child exec failed: ' + str(error), file=sys.stderr, flush=True)
        os._exit(127)
os.close(gate_read)
os.setpgid(pid, pid)
os.tcsetpgrp(tty_fd, pid)
os.write(gate_write, b'1')
os.close(gate_write)
observer_fd = os.environ.get('CLACK_TEST_OBSERVER_FD')
if observer_fd is not None:
    os.close(int(observer_fd))
os.write(status_fd, (json.dumps({'pid': pid, 'started_ns': started_ns}) + '\n').encode())
while True:
    _, status = os.waitpid(pid, os.WUNTRACED | getattr(os, 'WCONTINUED', 0))
    if os.WIFSTOPPED(status):
        os.write(status_fd, (json.dumps({'stopped': os.WSTOPSIG(status)}) + '\n').encode())
    elif hasattr(os, 'WIFCONTINUED') and os.WIFCONTINUED(status):
        os.write(status_fd, b'{"continued": true}\n')
    else:
        break
code = os.WEXITSTATUS(status) if os.WIFEXITED(status) else -os.WTERMSIG(status)
signal.signal(signal.SIGTTOU, signal.SIG_IGN)
os.tcsetpgrp(tty_fd, os.getpgrp())
os.write(status_fd, (json.dumps({'returncode': code}) + '\n').encode())
os.close(status_fd)
os.read(release_fd, 1)
os._exit(0)
'''


class CheckFailure(AssertionError):
    pass


class CheckSkipped(Exception):
    pass


def check(condition: bool, message: str) -> None:
    if not condition:
        raise CheckFailure(message)


def _bounded_append(buffer: bytearray, data: bytes, limit: int = MAX_CAPTURE) -> None:
    buffer.extend(data)
    if len(buffer) > limit:
        del buffer[: len(buffer) - limit]


def terminal_configuration(attrs: list[Any]) -> list[Any]:
    # Darwin may set PENDIN itself when canonical mode is restored. It denotes
    # queued-line reprocessing, not an application configuration preference.
    normalized = list(attrs)
    normalized[3] &= ~getattr(termios, "PENDIN", 0)
    return normalized


class Screen:
    """Bounded VT screen for Ratatui's common cursor/erase/SGR output.

    This deliberately does not claim full terminal emulation. Color rendition,
    shaping, font-dependent glyph widths, IME, and emulator quirks need actual
    emulator checks. No transcript history is retained beyond one last alternate
    screen and the bounded output buffer owned by PtyProcess.
    """

    def __init__(self, cols: int = 80, rows: int = 24):
        self.cols = max(1, cols)
        self.rows = max(1, rows)
        self.grid = self._blank()
        self.primary: Optional[list[list[str]]] = None
        self.primary_cursor = (0, 0)
        self.x = self.y = 0
        self.saved_cursor = (0, 0)
        self.cursor_visible = True
        self.cursor_style = 0
        self.alternate_screen = False
        self.bracketed_paste = False
        self.focus_reporting = False
        self.mouse_capture = False
        self.keyboard_depth = 0
        self.synchronized_output = False
        self.changed_modes: set[str] = set()
        self.last_alternate_text = ""
        self._decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self._state = "text"
        self._sequence = ""
        self._last_cell: Optional[tuple[int, int]] = None
        self._wrap_pending = False

    def _blank(self) -> list[list[str]]:
        return [[" "] * self.cols for _ in range(self.rows)]

    def resize(self, cols: int, rows: int) -> None:
        self.cols, self.rows = max(1, cols), max(1, rows)

        def fit(old: list[list[str]]) -> list[list[str]]:
            resized = self._blank()
            for y, row in enumerate(old[: self.rows]):
                resized[y][: min(len(row), self.cols)] = row[: self.cols]
            return resized

        self.grid = fit(self.grid)
        if self.primary is not None:
            self.primary = fit(self.primary)
        self.primary_cursor = (min(self.primary_cursor[0], self.cols - 1), min(self.primary_cursor[1], self.rows - 1))
        self.x, self.y = min(self.x, self.cols - 1), min(self.y, self.rows - 1)
        self._wrap_pending = False
        self._last_cell = None

    def cell(self, x: int, y: int) -> str:
        if 0 <= y < self.rows and 0 <= x < self.cols:
            return self.grid[y][x]
        return ""

    def text(self) -> str:
        return "\n".join("".join(row).rstrip() for row in self.grid).rstrip()

    def find(self, text: str) -> Optional[tuple[int, int]]:
        for y, row in enumerate(self.grid):
            for x, cell in enumerate(row):
                if cell and "".join(row[x:]).startswith(text):
                    return x, y
        return None

    def feed(self, data: bytes) -> None:
        for char in self._decoder.decode(data):
            if self._state == "charset":
                self._state = "escape" if char == "\x1b" else "text"
                continue
            if self._state == "osc":
                if char == "\x07":
                    self._state = "text"
                elif char == "\x1b":
                    self._state = "osc_escape"
                continue
            if self._state == "osc_escape":
                self._state = "text" if char == "\\" else "osc"
                continue
            if self._state == "csi":
                self._sequence += char
                if len(self._sequence) > 4096:
                    self._state, self._sequence = "text", ""
                elif "@" <= char <= "~":
                    self._csi(self._sequence)
                    self._state, self._sequence = "text", ""
                continue
            if self._state == "escape":
                self._state = "text"
                if char == "[":
                    self._state, self._sequence = "csi", ""
                elif char == "]":
                    self._state = "osc"
                elif char in "()*+%-,./":
                    self._state = "charset"
                elif char == "7":
                    self.saved_cursor = (self.x, self.y)
                elif char == "8":
                    self.x, self.y = self.saved_cursor
                elif char == "c":
                    self.grid = self._blank()
                    self.x = self.y = 0
                continue
            if char == "\x1b":
                self._state = "escape"
            elif char == "\r":
                self.x, self._wrap_pending = 0, False
            elif char == "\n":
                self._linefeed()
            elif char == "\b":
                self.x, self._wrap_pending = max(0, self.x - 1), False
            elif char == "\t":
                self.x = min(self.cols - 1, (self.x // 8 + 1) * 8)
            elif ord(char) >= 32 and char != "\x7f":
                self._put(char)

    def _linefeed(self) -> None:
        self.y += 1
        if self.y >= self.rows:
            self.grid.pop(0)
            self.grid.append([" "] * self.cols)
            self.y = self.rows - 1
        self._wrap_pending = False

    def _put(self, char: str) -> None:
        scalar = ord(char)
        previous = self.cell(*self._last_cell) if self._last_cell else ""
        extends = bool(unicodedata.combining(char)) or scalar in (0x200D, 0xFE0E, 0xFE0F)
        extends |= previous.endswith("\u200d")
        extends |= 0x1F3FB <= scalar <= 0x1F3FF
        extends |= (
            0x1F1E6 <= scalar <= 0x1F1FF
            and len(previous) == 1
            and 0x1F1E6 <= ord(previous) <= 0x1F1FF
        )
        if extends and self._last_cell:
            x, y = self._last_cell
            self.grid[y][x] += char
            return
        width = 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1
        if self._wrap_pending or self.x + width > self.cols:
            self.x = 0
            self._linefeed()
        self.grid[self.y][self.x] = char
        self._last_cell = (self.x, self.y)
        if width == 2 and self.x + 1 < self.cols:
            self.grid[self.y][self.x + 1] = ""
        next_x = self.x + width
        self.x = min(self.cols - 1, next_x)
        self._wrap_pending = next_x >= self.cols

    def _csi(self, sequence: str) -> None:
        final, raw = sequence[-1], sequence[:-1]
        private = raw.startswith("?")
        prefix = raw[:1] if raw[:1] in "?><=" else ""
        parameters = raw[1:] if prefix else raw
        parameters = parameters.strip()
        values = []
        for part in parameters.split(";"):
            try:
                values.append(int(part.split(":")[0]) if part else 0)
            except ValueError:
                values.append(0)
        first = values[0] if values else 0
        amount = first or 1
        if final in ("H", "f"):
            self.y = min(self.rows - 1, max(0, (values[0] or 1) - 1))
            self.x = min(self.cols - 1, max(0, ((values[1] if len(values) > 1 else 1) or 1) - 1))
        elif final == "A":
            self.y = max(0, self.y - amount)
        elif final == "B":
            self.y = min(self.rows - 1, self.y + amount)
        elif final == "C":
            self.x = min(self.cols - 1, self.x + amount)
        elif final == "D":
            self.x = max(0, self.x - amount)
        elif final == "E":
            self.y, self.x = min(self.rows - 1, self.y + amount), 0
        elif final == "F":
            self.y, self.x = max(0, self.y - amount), 0
        elif final == "G":
            self.x = min(self.cols - 1, max(0, amount - 1))
        elif final == "d":
            self.y = min(self.rows - 1, max(0, amount - 1))
        elif final == "J":
            if first in (2, 3):
                self.grid = self._blank()
            elif first == 0:
                self.grid[self.y][self.x :] = [" "] * (self.cols - self.x)
                for y in range(self.y + 1, self.rows):
                    self.grid[y] = [" "] * self.cols
            elif first == 1:
                for y in range(self.y):
                    self.grid[y] = [" "] * self.cols
                self.grid[self.y][: self.x + 1] = [" "] * (self.x + 1)
        elif final == "K":
            start, end = (0, self.cols) if first == 2 else ((0, self.x + 1) if first == 1 else (self.x, self.cols))
            self.grid[self.y][start:end] = [" "] * (end - start)
        elif final == "X":
            end = min(self.cols, self.x + amount)
            self.grid[self.y][self.x : end] = [" "] * (end - self.x)
        elif final == "s":
            self.saved_cursor = (self.x, self.y)
        elif final == "u" and prefix == ">":
            self.keyboard_depth += 1
            self.changed_modes.add("keyboard")
        elif final == "u" and prefix == "<":
            self.keyboard_depth = max(0, self.keyboard_depth - amount)
        elif final == "u" and not prefix:
            self.x, self.y = self.saved_cursor
        elif final == "q" and " " in raw:
            self.cursor_style = first
            self.changed_modes.add("cursor_style")
        elif final in ("h", "l") and private:
            enabled = final == "h"
            for mode in values:
                if mode in (47, 1047, 1049):
                    if enabled and not self.alternate_screen:
                        self.primary = self.grid
                        self.primary_cursor = (self.x, self.y)
                        self.grid = self._blank()
                    elif not enabled and self.alternate_screen:
                        self.last_alternate_text = self.text()
                        self.grid = self.primary if self.primary is not None else self._blank()
                        self.primary = None
                        self.x, self.y = self.primary_cursor
                    self.alternate_screen = enabled
                    self.changed_modes.add("alternate")
                elif mode == 25:
                    self.cursor_visible = enabled
                    self.changed_modes.add("cursor_visibility")
                elif mode == 2004:
                    self.bracketed_paste = enabled
                    self.changed_modes.add("paste")
                elif mode == 1004:
                    self.focus_reporting = enabled
                    self.changed_modes.add("focus")
                elif mode == 2026:
                    self.synchronized_output = enabled
                elif mode in (9, 1000, 1002, 1003, 1006, 1015):
                    self.mouse_capture = enabled
                    self.changed_modes.add("mouse")
        if final not in ("m", "h", "l", "q"):
            self._wrap_pending = False
            self._last_cell = None


class PtyProcess:
    """Child process with a real controlling PTY and bounded capture buffers.

    `read()` also drains redirected stdout/stderr/observer streams. All byte
    counters are cumulative even if callers clear retained buffers. stdout goes
    to the PTY only when capture_stdout=False. stderr is always a separate pipe.
    Child stdin is a pipe (if source_stdin is supplied), the concrete PTY slave
    when terminal_stdin=True, or /dev/null otherwise.
    """

    def __init__(
        self,
        argv: list[str],
        *,
        cols: int = 80,
        rows: int = 24,
        env: Optional[dict[str, str]] = None,
        source_stdin: Optional[bytes] = None,
        terminal_stdin: bool = False,
        capture_stdout: bool = True,
        observe: bool = False,
    ):
        if terminal_stdin and source_stdin is not None:
            raise ValueError("terminal_stdin and source_stdin are mutually exclusive")
        self.argv = [str(arg) for arg in argv]
        self.master, self.slave = os.openpty()
        self.baseline = termios.tcgetattr(self.slave)
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.screen = Screen(cols, rows)
        self.output = bytearray()
        self.stdout = bytearray()
        self.stderr = bytearray()
        self.output_bytes = self.stdout_bytes = self.stderr_bytes = 0
        self.observations: collections.deque[dict[str, Any]] = collections.deque(maxlen=MAX_OBSERVATIONS)
        self.observation_count = 0
        self.observation_error: Optional[str] = None
        self._observation_buffer = bytearray()
        self._selector = selectors.DefaultSelector()
        self._closed = False
        self._observe_fd: Optional[int] = None
        self.returncode: Optional[int] = None
        self.stopped_signal: Optional[int] = None
        self.pid: Optional[int] = None
        self._supervisor_buffer = bytearray()
        self._status_read, status_write = os.pipe()
        release_read, self._release_write = os.pipe()
        child_env = dict(os.environ)
        child_env.update({"TERM": "xterm-256color", "COLORTERM": "truecolor"})
        child_env.pop("CLACK_TEST_OBSERVER_FD", None)
        child_env.pop("CLACK_TEST_FAULT", None)
        if env:
            child_env.update(env)
        pass_fds = [self.slave, status_write, release_read]
        observer_write: Optional[int] = None
        if observe:
            self._observe_fd, observer_write = os.pipe()
            child_env["CLACK_TEST_OBSERVER_FD"] = str(observer_write)
            pass_fds.append(observer_write)

        def child_session() -> None:
            os.setsid()
            fcntl.ioctl(self.slave, termios.TIOCSCTTY, 0)

        self.started_ns = time.monotonic_ns()
        try:
            self.process = subprocess.Popen(
                [sys.executable, "-c", SESSION_KEEPER, str(status_write), str(release_read), str(self.slave), *self.argv],
                stdin=self.slave if terminal_stdin else (subprocess.PIPE if source_stdin is not None else subprocess.DEVNULL),
                stdout=subprocess.PIPE if capture_stdout else self.slave,
                stderr=subprocess.PIPE,
                env=child_env,
                pass_fds=tuple(pass_fds),
                preexec_fn=child_session,
                close_fds=True,
            )
        except BaseException:
            os.close(self.master)
            os.close(self.slave)
            if self._observe_fd is not None:
                os.close(self._observe_fd)
            if observer_write is not None:
                os.close(observer_write)
            for fd in (self._status_read, status_write, release_read, self._release_write):
                os.close(fd)
            self._selector.close()
            raise
        os.close(status_write)
        os.close(release_read)
        if observer_write is not None:
            os.close(observer_write)
        for fd, name in [
            (self.master, "tty"),
            (self.process.stdout.fileno() if self.process.stdout else None, "stdout"),
            (self.process.stderr.fileno() if self.process.stderr else None, "stderr"),
            (self._observe_fd, "observer"),
            (self._status_read, "supervisor"),
        ]:
            if fd is not None:
                os.set_blocking(fd, False)
                self._selector.register(fd, selectors.EVENT_READ, name)
        if self.process.stdin is not None:
            try:
                self.process.stdin.write(source_stdin or b"")
                self.process.stdin.close()
            except BrokenPipeError:
                pass
        deadline = time.monotonic() + 5
        while self.pid is None:
            self.read(0.01)
            if self.process.poll() is not None or time.monotonic() >= deadline:
                self.close()
                raise OSError(f"PTY session keeper did not start child: {bytes(self.stderr[-400:])!r}")

    def is_alive(self) -> bool:
        if self.returncode is None and self.process.poll() is not None:
            self.returncode = self.process.returncode
        return self.returncode is None

    def read(self, timeout: float = 0.05) -> bytes:
        tty_data = bytearray()
        for key, _ in self._selector.select(max(0.0, timeout)):
            budget = 256 * 1024
            while budget > 0:
                try:
                    data = os.read(key.fd, min(65536, budget))
                except BlockingIOError:
                    break
                except OSError as error:
                    if error.errno not in (errno.EIO, errno.EBADF):
                        raise
                    data = b""
                if not data:
                    try:
                        self._selector.unregister(key.fd)
                    except KeyError:
                        pass
                    break
                budget -= len(data)
                if key.data == "tty":
                    self.output_bytes += len(data)
                    _bounded_append(self.output, data)
                    self.screen.feed(data)
                    tty_data.extend(data)
                elif key.data == "stdout":
                    self.stdout_bytes += len(data)
                    _bounded_append(self.stdout, data)
                elif key.data == "stderr":
                    self.stderr_bytes += len(data)
                    _bounded_append(self.stderr, data)
                elif key.data == "observer":
                    self._read_observations(data)
                else:
                    self._read_supervisor(data)
        return bytes(tty_data)

    def _read_supervisor(self, data: bytes) -> None:
        self._supervisor_buffer.extend(data)
        while b"\n" in self._supervisor_buffer:
            line, _, tail = self._supervisor_buffer.partition(b"\n")
            self._supervisor_buffer = bytearray(tail)
            status = json.loads(line)
            if "pid" in status:
                self.pid = status["pid"]
                self.started_ns = status["started_ns"]
            if "returncode" in status:
                self.returncode = status["returncode"]
            if "stopped" in status:
                self.stopped_signal = status["stopped"]
            if status.get("continued"):
                self.stopped_signal = None

    def _read_observations(self, data: bytes) -> None:
        self._observation_buffer.extend(data)
        while b"\n" in self._observation_buffer:
            line, _, tail = self._observation_buffer.partition(b"\n")
            self._observation_buffer = bytearray(tail)
            if not line:
                continue
            try:
                record = json.loads(line)
                if not isinstance(record, dict):
                    raise ValueError("observation is not an object")
                self.observations.append(record)
                self.observation_count += 1
            except (ValueError, UnicodeDecodeError) as error:
                self.observation_error = str(error)
        if len(self._observation_buffer) > 65536:
            self.observation_error = "observer line exceeds 64 KiB"
            self._observation_buffer.clear()

    def send(self, data: bytes) -> None:
        pending = memoryview(data)
        deadline = time.monotonic() + 5
        while pending:
            try:
                written = os.write(self.master, pending)
                pending = pending[written:]
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise TimeoutError("PTY input consumer did not make progress")
                self.read(0.01)

    def resize(self, cols: int, rows: int) -> None:
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.screen.resize(cols, rows)
        # TIOCSWINSZ itself notifies the foreground process group on Unix.

    def signal(self, signum: int) -> None:
        if self.pid is None:
            raise ProcessLookupError("PTY application has not started")
        os.kill(self.pid, signum)
        if signum == signal.SIGCONT:
            # Some Python/Unix combinations do not expose WCONTINUED, although
            # SIGCONT still resumes the stopped foreground process normally.
            self.stopped_signal = None

    def terminal_restored(self) -> bool:
        return terminal_configuration(termios.tcgetattr(self.slave)) == terminal_configuration(self.baseline)

    def raw_mode(self) -> bool:
        attrs = termios.tcgetattr(self.slave)
        return not bool(attrs[3] & (termios.ICANON | termios.ECHO))

    def wait(self, timeout: float = 5.0) -> int:
        deadline = time.monotonic() + timeout
        while self.is_alive():
            if time.monotonic() >= deadline:
                raise TimeoutError(f"process did not exit within {timeout:.1f}s")
            self.read(min(0.05, max(0.0, deadline - time.monotonic())))
        for _ in range(3):
            self.read(0.01)
        return int(self.returncode)

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        if self.is_alive() and self.pid is not None:
            try:
                os.kill(self.pid, signal.SIGKILL)
                self.wait(timeout=2)
            except (ProcessLookupError, TimeoutError):
                pass
        try:
            os.close(self._release_write)
        except OSError:
            pass
        try:
            self.process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=2)
        self._selector.close()
        for stream in (self.process.stdout, self.process.stderr, self.process.stdin):
            if stream is not None and not stream.closed:
                stream.close()
        for fd in (self.master, self.slave, self._observe_fd, self._status_read):
            if fd is not None:
                try:
                    os.close(fd)
                except OSError:
                    pass

    def __enter__(self) -> "PtyProcess":
        return self

    def __exit__(self, *_: object) -> None:
        self.close()


def wait_for(process: PtyProcess, predicate: Callable[[], bool], description: str, timeout: float = 3.0) -> None:
    deadline = time.monotonic() + timeout
    while True:
        process.read(0.01)
        if predicate():
            return
        if not process.is_alive() or time.monotonic() >= deadline:
            screen = process.screen.text() or process.screen.last_alternate_text
            raise CheckFailure(
                f"did not observe {description}; exit={process.returncode}; "
                f"screen={screen[-600:]!r}; stderr={bytes(process.stderr[-400:])!r}"
            )


def pause(process: PtyProcess, duration: float) -> None:
    deadline = time.monotonic() + duration
    while time.monotonic() < deadline:
        process.read(min(0.02, deadline - time.monotonic()))


def latest_frame(process: PtyProcess) -> Optional[dict[str, Any]]:
    return next((item for item in reversed(process.observations) if item.get("event") == "frame"), None)


def frame_state(process: PtyProcess) -> str:
    frame = latest_frame(process)
    return str(frame.get("state", "")).lower() if frame else ""


def ready(process: PtyProcess, hooks: bool = False) -> None:
    if hooks:
        wait_for(process, lambda: frame_state(process) == "ready", "post-flush Ready observer frame (build --features test-hooks)")
    else:
        wait_for(
            process,
            lambda: process.raw_mode() and len(process.screen.text().strip()) >= 20,
            "a usable rendered Ready surface in raw mode",
        )
        # Used for integration readiness only, not as a startup measurement endpoint.
        pause(process, 0.03)


def clean_terminal(process: PtyProcess, *, expected_cursor_style: int = 0) -> None:
    actual = termios.tcgetattr(process.slave)
    changes = [index for index, (before, after) in enumerate(zip(terminal_configuration(process.baseline), terminal_configuration(actual))) if before != after]
    check(not changes, f"termios differs in fields{changes}: expected={process.baseline!r}, actual={actual!r}")
    check(not process.screen.alternate_screen, "alternate screen was not left")
    check(process.screen.cursor_visible, "cursor remained hidden")
    check(process.screen.cursor_style == expected_cursor_style, "cursor style was not restored to the expected terminal default")
    check(not process.screen.bracketed_paste, "bracketed paste remained enabled")
    check(not process.screen.focus_reporting, "focus reporting remained enabled")
    check(not process.screen.mouse_capture, "mouse capture remained enabled")
    check(process.screen.keyboard_depth == 0, "keyboard enhancement stack was not restored")
    check(not process.screen.synchronized_output, "synchronized output transaction remained open")


def one_json(process: PtyProcess) -> dict[str, Any]:
    check(b"\x1b" not in process.stdout, "stdout includes terminal escapes")
    text = bytes(process.stdout).decode("utf-8")

    def reject_constant(value: str) -> None:
        raise ValueError(f"nonfinite JSON value {value}")

    decoder = json.JSONDecoder(parse_constant=reject_constant)
    try:
        result, end = decoder.raw_decode(text.lstrip())
    except ValueError as error:
        raise CheckFailure(f"stdout is not one parseable result: {text[:300]!r}: {error}") from error
    check(not text.lstrip()[end:].strip(), "stdout contains data after its result object")
    check(isinstance(result, dict), "result must be a JSON object")
    check(result.get("export_version") == 1, "result lacks export_version=1")
    return result


@dataclasses.dataclass
class Context:
    binary: Path
    directory: Path
    hooks: bool

    def command(self, *args: str) -> list[str]:
        config = self.directory / "config.toml"
        config.write_text("schema_version = 1\n", encoding="utf-8")
        return [str(self.binary), "--config", str(config), "--data-dir", str(self.directory / "data"), "--private", *args]

    def launch(self, *args: str, **kwargs: Any) -> PtyProcess:
        kwargs.setdefault("observe", self.hooks)
        return PtyProcess(self.command(*args), **kwargs)


def case_ctrl_c(ctx: Context) -> None:
    with ctx.launch("--time", "30") as p:
        ready(p, ctx.hooks)
        p.send(b"x")
        pause(p, 0.04)
        p.send(CTRL_C)
        check(p.wait() == 130, "Ctrl-C must exit130")
        check(not p.stdout, "ordinary interactive run wrote redirected stdout")
        clean_terminal(p)


def case_invalid_source_before_raw(ctx: Context) -> None:
    private_marker = "PRIVATE_SOURCE_SENTINEL"
    marker = private_marker.encode()
    invalid_sources = [
        ("utf8", marker + b"\xff"),
        ("control", marker + b"\x1b[31m"),
        ("bidi", marker + "\u202e".encode()),
        ("cluster", marker + ("a" + "\u0301" * 32).encode()),
        ("oversized", marker + b"a" * (1024 * 1024 + 1 - len(marker))),
    ]

    def assert_rejected(process: PtyProcess, source_kind: str) -> None:
        code = process.wait()
        check(code == 2, f"invalid {source_kind} must fail with exit2 before raw mode; got {code}")
        check(not process.stdout, f"{source_kind} validation polluted reserved stdout")
        check(not process.screen.changed_modes, f"{source_kind} validation happened after terminal setup")
        check(marker not in process.stdout + process.stderr + process.output, f"{source_kind} diagnostic echoed private text")
        clean_terminal(process)

    with ctx.launch("--text", private_marker + "\x1b[31m", "--once", "--json") as p:
        assert_rejected(p, "literal control source")
    for label, content in invalid_sources:
        path = ctx.directory / f"source-{label}.txt"
        path.write_bytes(content)
        with ctx.launch("--file", str(path), "--exact", "--once", "--json") as p:
            assert_rejected(p, f"file {label} source")
        with ctx.launch("--stdin", "--exact", "--once", "--json", source_stdin=content) as p:
            assert_rejected(p, f"stdin {label} source")


def case_invalid_config_before_raw(ctx: Context) -> None:
    invalid = [
        ("syntax", b"schema_version = [\n"),
        ("schema", b"schema_version = 999\n"),
        ("unknown", b"schema_version = 1\nunknown_setting = true\n"),
        ("range", b"schema_version = 1\n[test]\nseconds = 0\n"),
        ("contradiction", b'schema_version = 1\n[rules]\nbackspace = "none"\nstop_on_error = "word"\n'),
        ("utf8", b"schema_version = 1\n\xff"),
    ]
    for label, content in invalid:
        # Context.command creates the isolated valid baseline. Replace it only
        # after obtaining argv so the invalid bytes are what clack actually reads.
        command = ctx.command("--time", "30", "--once", "--json")
        config = ctx.directory / "config.toml"
        config.write_bytes(content)
        with PtyProcess(command, observe=ctx.hooks) as p:
            check(p.wait() == 2, f"invalid explicit {label} config must exit2")
            check(not p.stdout, f"invalid {label} config wrote reserved stdout")
            check(not p.screen.changed_modes, f"invalid {label} config entered terminal setup")
            check(config.read_bytes() == content, f"invalid {label} config was overwritten")
            check(not (ctx.directory / "data").exists(), f"invalid {label} config created result data")
            clean_terminal(p)


def case_invalid_arguments_before_raw(ctx: Context) -> None:
    for arguments in [
        ("--time", "0"),
        ("--words", "10001"),
        ("--time", "30", "--words", "20"),
        ("--text", "x", "--stdin"),
        ("--quote", "--exact"),
        ("--minimum-accuracy", "101"),
        ("--json",),
    ]:
        with ctx.launch(*arguments) as p:
            check(p.wait() == 2, f"invalid arguments {arguments!r} must exit2")
            check(not p.stdout, "invalid arguments wrote stdout")
            check(not p.screen.changed_modes, "invalid arguments entered terminal setup")
            clean_terminal(p)


def case_no_controlling_terminal(ctx: Context) -> None:
    source = b"private-piped-source"
    read_fd, write_fd = os.pipe()
    os.write(write_fd, source)
    os.close(write_fd)
    child: Optional[subprocess.Popen[bytes]] = None
    try:
        child = subprocess.Popen(
            ctx.command("--stdin", "--once", "--json"),
            stdin=read_fd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            preexec_fn=os.setsid,
            close_fds=True,
        )
        try:
            output, diagnostic = child.communicate(timeout=3)
        except subprocess.TimeoutExpired as error:
            child.kill()
            child.communicate(timeout=2)
            raise CheckFailure("interactive command without a controlling terminal hung") from error
        check(child.returncode == 1, "missing interactive terminal must exit1")
        check(not output, "missing-terminal failure wrote stdout")
        check(any(word in diagnostic.lower() for word in (b"tty", b"terminal", b"console")), "missing-terminal diagnostic is unclear")
        check(os.read(read_fd, len(source) + 1) == source, "--stdin was consumed before a suitable terminal was acquired")
        check(source not in diagnostic, "missing-terminal diagnostic echoed private input")
    finally:
        os.close(read_fd)
        if child is not None and child.poll() is None:
            child.kill()
            child.wait(timeout=2)


def case_first_key_focus_json(ctx: Context) -> None:
    with ctx.launch("--text", "cat dog", "--once", "--json") as p:
        ready(p, ctx.hooks)
        anchor = p.screen.find("cat dog")
        check(anchor is not None, "Ready target text is absent")
        p.send(b"x")
        wait_for(
            p,
            lambda: p.screen.cell(*anchor) == "x"
            and (not ctx.hooks or (frame_state(p) == "running" and latest_frame(p).get("counts", {}).get("attempts_total") == 1)),
            "wrong first input at the original text coordinates",
        )
        check(p.screen.find("xat dog") == anchor, "focus transition moved or rewrapped target")
        if ctx.hooks:
            check(latest_frame(p).get("counts", {}).get("attempts_total") == 1, "first input was not exactly one attempt")
        p.send(BACKSPACE + b"cat dog")
        check(p.wait() == 0, "completed custom test must exit0")
        result = one_json(p)
        counts = result["counts"]
        check(counts["attempts_total"] == 8 and counts["attempts_correct"] == 7, "first wrong character was lost or duplicated")
        check(counts["retained_units"] == 7 and counts["credited_units"] == 7, "corrected final output has incorrect units")
        check(counts["deletion_count"] == 1, "correction was not one deletion")
        clean_terminal(p)


def case_first_input_without_ready_wait(ctx: Context) -> None:
    with ctx.launch("--text", "a", "--exact", "--once", "--json") as p:
        # This intentionally does not call ready(). The reader must not flush
        # eligible typing merely because it arrived promptly after launch.
        p.send(b"x")
        pause(p, 0.30)
        check(p.is_alive(), "early first input unexpectedly ended exact test")
        p.send(BACKSPACE + b"a")
        pause(p, 0.05)
        p.send(F5)
        check(p.wait() == 0, "promptly typed exact test did not complete")
        result = one_json(p)
        counts = result["counts"]
        check(counts["attempts_total"] == 2 and counts["attempts_correct"] == 1, "initial launch input was lost or duplicated")
        check(counts["deletion_count"] == 1 and counts["retained_units"] == 1, "early first input was not retained for correction")
        clean_terminal(p)


def case_exact_confirm(ctx: Context) -> None:
    with ctx.launch("--text", "a\tb\nc", "--exact", "--once", "--json") as p:
        ready(p, ctx.hooks)
        p.send(b"a\tb\rx")
        pause(p, 0.12)
        check(p.is_alive(), "exact test ended before final typo repair/F5 confirmation")
        p.send(BACKSPACE + b"c")
        pause(p, 0.04)
        check(p.is_alive(), "exact test ended before F5 confirmation")
        p.send(F5)
        check(p.wait() == 0, "confirmed exact test must exit0")
        result = one_json(p)
        check(result["outcome"] == "complete", "F5 did not confirm completed exact text")
        counts = result["counts"]
        check(counts["attempts_total"] == 6 and counts["attempts_correct"] == 5, "Tab/newline or corrected tail were miscounted")
        check(counts["retained_units"] == 5 and counts["credited_units"] == 5, "exact whitespace did not remain single logical units")
        clean_terminal(p)


def case_stdin_keyboard_separation(ctx: Context) -> None:
    with ctx.launch("--stdin", "--exact", "--once", "--json", source_stdin=b"a\tb\nc") as p:
        ready(p, ctx.hooks)
        check(p.screen.find("a") is not None, "piped source was not rendered")
        p.send(b"a\tb\rc")
        pause(p, 0.04)
        p.send(F5)
        check(p.wait() == 0, "piped source test failed")
        result = one_json(p)
        check(result["counts"]["attempts_total"] == 5, "stdin bytes were consumed as scored keyboard input")
        check(result["counts"]["attempts_correct"] == 5, "controlling-terminal keyboard channel did not match piped source")
        clean_terminal(p)


def case_paste_atomic(ctx: Context) -> None:
    with ctx.launch("--text", "cat dog", "--once", "--json") as p:
        ready(p, ctx.hooks)
        p.send(b"\x1b[200~unscored paste\x1b[201~")
        pause(p, 0.12)
        check(p.is_alive(), "Ready paste completed or terminated the test")
        if ctx.hooks:
            check(frame_state(p) == "ready", "Ready paste started the clock")
        p.send(b"c")
        pause(p, 0.04)
        p.send(b"\x1b[200~another paste\x1b[201~")
        pause(p, 0.04)
        p.send(b"at dog")
        check(p.wait() == 0, "paste attempt prevented subsequent typing")
        result = one_json(p)
        check(result["counts"]["attempts_total"] == 7, "paste characters leaked into attempts")
        check(result["integrity"]["paste_attempted"], "active paste was not recorded")
        check(not result["personal_best_eligible"], "paste-attempted result is record eligible")
        clean_terminal(p)


def case_deadline_duration(ctx: Context) -> None:
    with ctx.launch("--time", "1", "--once", "--json") as p:
        ready(p, ctx.hooks)
        before = time.monotonic()
        p.send(b"x")
        pause(p, 0.70)
        p.send(b"y")
        check(p.wait(timeout=4) == 0, "timed run did not complete")
        elapsed = time.monotonic() - before
        result = one_json(p)
        check(result["elapsed_us"] == 1_000_000, "timed score duration changed with terminal scheduling")
        check(result["counts"]["attempts_total"] == 2, "well-before-deadline input was lost")
        check(elapsed >= 0.85, "test completed materially before its configured deadline")
        clean_terminal(p)
    # PTY write timestamps are not reader receipt timestamps. Exact-deadline
    # exclusion is established by deterministic reducer/reader tests, separately.


def case_challenge_failure_once(ctx: Context) -> None:
    cases = [
        ("master", "cat dog", ("--difficulty", "master"), b"x", 1, 0),
        ("expert", "cat dog", ("--difficulty", "expert"), b"xat ", 4, 3),
        ("accuracy", "a" * 26, ("--minimum-accuracy", "100"), b"a" * 19 + b"x", 20, 19),
    ]
    for label, target, options, delivery, attempts, correct in cases:
        with ctx.launch("--text", target, *options, "--once", "--json") as p:
            ready(p, ctx.hooks)
            p.send(delivery)
            check(p.wait() == 0, f"{label} challenge failure must exit0 in once mode")
            result = one_json(p)
            check(result["outcome"] == "failed", f"{label} failure lost its distinct outcome")
            check(result["counts"]["attempts_total"] == attempts, f"{label} failure lost or duplicated attempts")
            check(result["counts"]["attempts_correct"] == correct, f"{label} failure has incorrect attempt accuracy")
            check(bool(result.get("reason")), f"{label} failure has no reason")
            check(not result["personal_best_eligible"], f"{label} challenge failure is record eligible")
            clean_terminal(p)


def case_resize_preserves_run(ctx: Context) -> None:
    with ctx.launch("--time", "1", "--once", "--json") as p:
        ready(p, ctx.hooks)
        p.send(b"x")
        pause(p, 0.05)
        frame = latest_frame(p)
        p.resize(120, 40)
        pause(p, 0.08)
        if ctx.hooks:
            check(frame_state(p) == "running", "ordinary resize ended or paused the test")
            check(latest_frame(p)["epoch"] == frame["epoch"], "ordinary resize changed the test epoch")
        p.send(b"y")
        check(p.wait(timeout=4) == 0, "resized test did not finish")
        result = one_json(p)
        check(result["elapsed_us"] == 1_000_000 and result["counts"]["attempts_total"] == 2, "resize changed timer or logical input")
        clean_terminal(p)


def case_too_small_interrupt(ctx: Context) -> None:
    with ctx.launch("--time", "30", "--once", "--json") as p:
        ready(p, ctx.hooks)
        p.send(b"x")
        pause(p, 0.04)
        p.resize(39, 9)
        check(p.wait() == 130, "unsafe active geometry must exit130 as interruption")
        result = one_json(p)
        check(result["outcome"] == "interrupted", "unsafe active geometry did not interrupt")
        check(not result["personal_best_eligible"], "too-small run is record eligible")
        check(bool(result.get("reason")), "geometry interruption has no reason")
        clean_terminal(p)


def case_small_ready_rejects_input_and_focus_loss_never_pauses(ctx: Context) -> None:
    with ctx.launch("--time", "1", "--once", "--json", cols=39, rows=9) as p:
        ready(p, ctx.hooks)
        check(not p.screen.mouse_capture, "ordinary startup enabled mouse capture")
        p.send(b"ignored")
        pause(p, 0.1)
        if ctx.hooks:
            check(frame_state(p) == "ready", "unsafe Ready input started a scored run")
            check(latest_frame(p)["counts"]["attempts_total"] == 0,
                  "unsafe Ready input entered scoring")
        p.resize(80, 24)
        wait_for(p, lambda: "start typing" in p.screen.text(), "safe Ready target after resize")
        pause(p, 0.05)
        p.send(b"x")
        pause(p, 0.03)
        p.send(b"\x1b[O")  # Focus lost, reported by an ordinary supporting terminal.
        pause(p, 0.03)
        p.send(b"\x1b[I\x1b[<35;10;5M")  # Focus gained and mouse motion.
        pause(p, 0.03)
        check("start typing" not in p.screen.text() and "esc commands" not in p.screen.text(),
              "focus or mouse movement restored hidden controls")
        check(p.wait(timeout=3) == 0, "focus loss paused or interrupted a normal timed run")
        result = one_json(p)
        check(result["outcome"] == "complete", "focus metadata changed normal completion")
        check(result["elapsed_us"] == 1_000_000, "focus loss changed the scored timer")
        check(result["counts"]["attempts_total"] == 1, "unsafe/terminal control input entered scoring")
        check(result["integrity"]["focus_lost"], "reported focus loss was not retained")
        clean_terminal(p)


def case_ignored_ready_input_has_no_score_or_render_work(ctx: Context) -> None:
    if not ctx.hooks:
        raise CheckSkipped("ignored-key frame accounting requires explicit test-hooks instrumentation")
    with ctx.launch("--time", "30", "--once", "--json") as p:
        ready(p, True)
        pause(p, 0.05)
        frames = sum(event.get("event") == "frame" for event in p.observations)
        output_bytes = p.output_bytes
        for _ in range(8):
            for ignored in (b" ", BACKSPACE, F5):
                p.send(ignored)
                pause(p, 0.015)
        pause(p, 0.05)
        check(frame_state(p) == "ready" and latest_frame(p)["counts"]["attempts_total"] == 0,
              "ignored Ready keys started timing or altered score")
        check(sum(event.get("event") == "frame" for event in p.observations) == frames,
              "ignored Ready text/deletion/finish scheduled a redraw")
        check(p.output_bytes == output_bytes, "ignored Ready keys emitted terminal bytes")
        p.send(b"x")
        wait_for(p, lambda: frame_state(p) == "running", "fresh eligible key after ignored input")
        p.send(CTRL_C)
        check(p.wait() == 130, "Ctrl-C failed after ignored Ready input")
        result = one_json(p)
        check(result["counts"]["attempts_total"] == 1, "fresh eligible key did not count exactly once")
        clean_terminal(p)


def case_restart_epoch(ctx: Context) -> None:
    if not ctx.hooks:
        raise CheckSkipped("requires --features test-hooks and --hooks for post-flush epoch/count observations")
    with ctx.launch("--text", "cat") as p:
        ready(p, True)
        initial_epoch = latest_frame(p)["epoch"]
        p.send(b"cat\rresidual")
        wait_for(p, lambda: frame_state(p) == "results", "Results after completed sample")
        pause(p, 0.08)
        check(frame_state(p) == "results", "queued Enter/letters restarted the completed sample")
        check(latest_frame(p)["counts"]["attempts_total"] == 3, "old queued letters mutated the result")
        p.send(CTRL_R + b"old\x1b[97;1:3u")
        wait_for(p, lambda: frame_state(p) == "ready" and latest_frame(p)["epoch"] > initial_epoch, "new Ready epoch")
        check(latest_frame(p)["counts"]["attempts_total"] == 0, "old epoch or Release input entered restarted test")
        p.send(b"c")
        wait_for(p, lambda: frame_state(p) == "running", "fresh input after restart acknowledgement")
        check(latest_frame(p)["counts"]["attempts_total"] == 1, "new epoch did not accept exactly one fresh input")
        p.send(CTRL_C)
        check(p.wait() == 130, "Ctrl-C after restart must exit130")
        clean_terminal(p)

    def target_rows(process: PtyProcess) -> list[list[str]]:
        cursor = latest_frame(process).get("cursor")
        check(isinstance(cursor, list) and len(cursor) == 2, "Ready frame has no target caret")
        x, y = cursor
        return [row[x : x + 72] for row in process.screen.grid[y : y + 3]]

    # Compare a restarted random sample with a fresh process replaying its
    # exported seed. This detects stale layout caches even when token counts and
    # per-token edit revisions happen to be identical between samples.
    with ctx.launch("--words", "12", "--seed", "1", "--once", "--json") as p:
        ready(p, True)
        initial_epoch = latest_frame(p)["epoch"]
        old_rows = target_rows(p)
        p.send(CTRL_R)
        wait_for(p, lambda: frame_state(p) == "ready" and latest_frame(p)["epoch"] > initial_epoch, "restarted random sample Ready frame")
        restarted_rows = target_rows(p)
        pause(p, 0.03)
        p.send(b"x")
        pause(p, 0.03)
        p.send(F5)
        check(p.wait() == 0, "random restart sample could not finish")
        result = one_json(p)
        restarted_seed = result["spec"]["seed"]
        check(restarted_seed != 1, "new sample retained the initial seed")
        clean_terminal(p)
    with ctx.launch("--words", "12", "--seed", str(restarted_seed)) as p:
        ready(p, True)
        replay_rows = target_rows(p)
        check(replay_rows != old_rows, "chosen restart fixture did not change its target layout")
        check(restarted_rows == replay_rows, "random restart reused stale target positions; fresh seed replay renders differently")
        p.send(CTRL_C)
        check(p.wait() == 130, "fresh replay fixture did not exit cleanly")
        clean_terminal(p)


def case_repeat_practice(ctx: Context) -> None:
    if not ctx.hooks:
        raise CheckSkipped("requires --hooks for reliable Ready/Results transition observations")
    with ctx.launch("--text", "cat") as p:
        ready(p, True)
        p.send(b"cat")
        wait_for(p, lambda: frame_state(p) == "results", "first Results")
        p.send(F2)
        wait_for(p, lambda: frame_state(p) == "ready", "repeated sample Ready")
        check(p.screen.find("cat") is not None, "repeat changed the fixed sample")
        p.send(b"cat")
        wait_for(p, lambda: frame_state(p) == "results", "repeated Results")
        check("practice" in p.screen.text().lower(), "repeated sample is not visibly labeled practice")
        p.send(CTRL_C)
        check(p.wait() == 130, "quit after repeated result must exit130")
        clean_terminal(p)


def case_associated_text(ctx: Context) -> None:
    with ctx.launch("--text", "abc", "--exact", "--once", "--json") as p:
        ready(p, ctx.hooks)
        p.send(b"\x1b[120;1:3;97u")  # release: x key, associated a, no insertion
        pause(p, 0.03)
        if ctx.hooks:
            check(frame_state(p) == "ready", "enhanced Release started the run")
        p.send(b"\x1b[120;1:1;97u\x1b[120;1:2;98u\x1b[120;1:3;99u\x1b[120;1:1;99u")
        pause(p, 0.05)
        p.send(F5)
        check(p.wait() == 0, "associated-text input failed")
        result = one_json(p)
        check(result["counts"]["attempts_total"] == 3, "enhanced key-code or Release duplicated associated text")
        check(result["counts"]["attempts_correct"] == 3, "associated text was not used as actual input")
        clean_terminal(p)
    with ctx.launch("--text", "abc", "--exact", "--once", "--json") as p:
        ready(p, ctx.hooks)
        p.send(b"a")
        pause(p, 0.04)
        # An enhanced key may carry associated text even for an essential
        # command. Physical Ctrl-C remains a command, not an inserted c.
        p.send(b"\x1b[99;5:1;99u")
        check(p.wait() == 130, "associated text shadowed the essential Ctrl-C command")
        result = one_json(p)
        check(result["outcome"] == "interrupted", "enhanced Ctrl-C did not interrupt the active test")
        check(result["counts"]["attempts_total"] == 1, "enhanced Ctrl-C text entered the score")
        clean_terminal(p)


def case_malformed_associated_text(ctx: Context) -> None:
    with ctx.launch("--time", "30", "--once", "--json") as p:
        ready(p, ctx.hooks)
        p.send(b"x")
        pause(p, 0.04)
        p.send(b"\x1b[97;1;1114112u")  # above the Unicode scalar range
        code = p.wait()
        check(code in (1, 130), "malformed input must surface runtime failure/interruption")
        result = one_json(p)
        check(result["outcome"] == "interrupted", "malformed enhanced input was silently ignored")
        check(not result["personal_best_eligible"], "malformed input result remained record eligible")
        check(result["counts"]["attempts_total"] == 1, "malformed event characters entered the score")
        clean_terminal(p)


def case_signal(ctx: Context, signum: int) -> None:
    with ctx.launch("--time", "30") as p:
        ready(p, ctx.hooks)
        p.send(b"x")
        pause(p, 0.04)
        p.signal(signum)
        check(p.wait() == 130, "supported termination signal must exit130 through safe shutdown")
        clean_terminal(p)


def case_suspend_signal(ctx: Context) -> None:
    with ctx.launch("--time", "30", "--once", "--json") as p:
        ready(p, ctx.hooks)
        p.send(b"x")
        pause(p, 0.04)
        p.signal(signal.SIGTSTP)
        wait_for(p, lambda: p.stopped_signal is not None or not p.is_alive(), "supported suspend signal handling")
        if p.stopped_signal is not None:
            # A supported job-control suspension must restore before stopping.
            clean_terminal(p)
            p.signal(signal.SIGCONT)
        code = p.wait()
        check(code == 130, f"suspended scored run must finish as interruption; exit={code}, stderr={bytes(p.stderr[-400:])!r}, stdout={bytes(p.stdout[-400:])!r}")
        result = one_json(p)
        check(result["outcome"] == "interrupted", "suspend retained a normal scored result")
        check(not result["personal_best_eligible"], "suspended run remained record eligible")
        clean_terminal(p)


def case_fault(ctx: Context, fault: str) -> None:
    if not ctx.hooks:
        raise CheckSkipped("fault injection requires explicit --features test-hooks and --hooks")
    with ctx.launch("--time", "30", env={"CLACK_TEST_FAULT": fault}) as p:
        check(p.wait() == 1, "injected recoverable runtime failure must exit1")
        check(p.output_bytes > 0 and "alternate" in p.screen.changed_modes, "fault happened before terminal setup; lifecycle path untested")
        check(not p.stdout, "runtime diagnostics entered redirected stdout")
        clean_terminal(p)


def case_overload_after_completion(ctx: Context) -> None:
    if not ctx.hooks:
        raise CheckSkipped("completion-boundary overload requires explicit test-hooks instrumentation")
    target = ctx.directory / "overload-target.txt"
    target.write_text("cat", encoding="utf-8")
    with ctx.launch("--file", str(target), "--once", "--json",
                    env={"CLACK_TEST_FAULT": "wait_overload_after_completion"}) as p:
        ready(p, True)
        p.send(b"cat")
        wait_for(p, lambda: any(event.get("event") == "completion_pending" for event in p.observations),
                 "engine completion before reader epoch closure")
        # The engine/render owner is now deliberately parked, while the sole
        # reader continues receiving. Pacing avoids overflowing the OS PTY byte
        # buffer itself; the tested limit is the application's 4,096-event queue.
        for offset in range(0, 5000, 64):
            if p.returncode is not None:
                break
            p.send(b"x" * min(64, 5000 - offset))
            pause(p, 0.002)
        check(p.wait() == 130, "overflow after engine completion must exit interrupted")
        result = one_json(p)
        check(result.get("outcome") == "interrupted", "provisional completed result escaped as a valid score")
        check(result.get("integrity", {}).get("input_overload") is True, "closed epoch lost sticky overflow")
        check(result.get("personal_best_eligible") is False, "overloaded result retained personal-best eligibility")
        check(result.get("counts", {}).get("attempts_total") == 3, "closure verdict changed immutable final typing counts")
        check(result.get("reason") == "input queue overload", "closure interruption reason was lost")
        clean_terminal(p)


CASES: dict[str, Callable[[Context], None]] = {
    "ctrl_c_cleanup": case_ctrl_c,
    "invalid_source_before_raw": case_invalid_source_before_raw,
    "invalid_config_before_raw": case_invalid_config_before_raw,
    "invalid_arguments_before_raw": case_invalid_arguments_before_raw,
    "no_controlling_terminal": case_no_controlling_terminal,
    "first_key_focus_json": case_first_key_focus_json,
    "first_input_without_ready_wait": case_first_input_without_ready_wait,
    "exact_confirm": case_exact_confirm,
    "stdin_keyboard_separation": case_stdin_keyboard_separation,
    "paste_atomic": case_paste_atomic,
    "deadline_duration": case_deadline_duration,
    "challenge_failure_once": case_challenge_failure_once,
    "resize_preserves_run": case_resize_preserves_run,
    "too_small_interrupt": case_too_small_interrupt,
    "small_ready_focus_mouse": case_small_ready_rejects_input_and_focus_loss_never_pauses,
    "ignored_ready_no_redraw": case_ignored_ready_input_has_no_score_or_render_work,
    "restart_epoch": case_restart_epoch,
    "repeat_practice": case_repeat_practice,
    "associated_text": case_associated_text,
    "malformed_associated_text": case_malformed_associated_text,
    "sigint_cleanup": lambda ctx: case_signal(ctx, signal.SIGINT),
    "sigterm_cleanup": lambda ctx: case_signal(ctx, signal.SIGTERM),
    "sighup_cleanup": lambda ctx: case_signal(ctx, signal.SIGHUP),
    "sigtstp_cleanup": case_suspend_signal,
    "panic_cleanup": lambda ctx: case_fault(ctx, "panic_after_first_frame"),
    "error_cleanup": lambda ctx: case_fault(ctx, "error_after_first_frame"),
    "overload_after_completion": case_overload_after_completion,
}


def self_test() -> None:
    charset = Screen(20, 5)
    for byte in b"cat\x1b(B\x1b)B\x1b*B\x1b+B\x1b%G\x1b-B\x1b.B\x1b/B dog":
        charset.feed(bytes([byte]))
    check(charset.text() == "cat dog", "split character-set selectors leaked visible finals")
    screen = Screen(20, 5)
    screen.feed(b"\x1b[2J\x1b[2;3Hcat\x1b[2;3Hx")
    check(screen.find("xat") == (2, 1), "screen position/overwrite decoder failed")
    wide = "\x1b[3;1H界a".encode()
    for byte in wide:
        screen.feed(bytes([byte]))
    check(screen.cell(0, 2) == "界" and screen.cell(2, 2) == "a", "split UTF-8/wide-cell decoder failed")
    screen.feed(b"\x1b[?1049h\x1b[?25l\x1b[?2004h\x1b[?1004h\x1b[6 q\x1b[>3u")
    check(screen.alternate_screen and screen.keyboard_depth == 1, "mode enable tracking failed")
    screen.feed(b"\x1b[<1u\x1b[0 q\x1b[?1004l\x1b[?2004l\x1b[?25h\x1b[?1049l")
    check(not screen.alternate_screen and screen.cursor_visible and screen.keyboard_depth == 0, "mode cleanup tracking failed")
    child = r'''
import json, os, sys, termios, tty
source = sys.stdin.buffer.read()
fd = os.open('/dev/tty', os.O_RDWR)
previous = termios.tcgetattr(fd)
try:
    tty.setraw(fd)
    os.write(fd, b'\x1b[?1049h\x1b[?25l\x1b[?2004h\x1b[6;9Hcat dog')
    received = os.read(fd, 1)
    os.write(fd, b'\x1b[6;9H' + received)
    print(json.dumps({'received': received.decode(), 'source': source.decode()}), flush=True)
    print('separate diagnostic', file=sys.stderr, flush=True)
finally:
    os.write(fd, b'\x1b[0 q\x1b[?2004l\x1b[?25h\x1b[?1049l')
    termios.tcsetattr(fd, termios.TCSANOW, previous)
    os.close(fd)
'''
    with PtyProcess([sys.executable, "-c", child], source_stdin=b"stdin-only") as p:
        wait_for(p, lambda: p.screen.find("cat dog") == (8, 5) and p.raw_mode(), "fixture controlling terminal")
        p.signal(signal.SIGSTOP)
        wait_for(p, lambda: p.stopped_signal == signal.SIGSTOP, "fixture process-group stop notification")
        p.signal(signal.SIGCONT)
        p.send(b"x")
        check(p.wait() == 0, "PTY fixture exited unsuccessfully")
        payload = json.loads(p.stdout)
        check(payload == {"received": "x", "source": "stdin-only"}, "source stdin and controlling input were mixed")
        check(b"separate diagnostic" in p.stderr, "stderr was not separately captured")
        check(p.output_bytes > 0 and p.stdout_bytes == len(p.stdout), "byte accounting failed")
        clean_terminal(p)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/clack")
    parser.add_argument("--case", action="append", choices=sorted(CASES))
    parser.add_argument("--hooks", action="store_true", help="binary was explicitly built with --features test-hooks")
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--self-test", action="store_true", help="validate harness only; does not test clack")
    parser.add_argument("--report", type=Path, help="write a versioned JSON evidence report")
    parser.add_argument("--json", action="store_true", help="print only the JSON evidence report")
    parser.add_argument("--require-all", action="store_true", help="return failure if any selected case is skipped")
    parser.add_argument("--fail-fast", action="store_true")
    parser.add_argument("--execution-label", default="unspecified", help="record native/translated execution separately from the harness host architecture")
    args = parser.parse_args()
    if args.list:
        print("\n".join(CASES))
        return 0
    if args.self_test:
        self_test()
        print("Harness self-test passed; no application or emulator validation claimed.")
        return 0
    binary = args.binary.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error(f"executable not found: {binary}; build clack explicitly first")
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    results = []
    for name in args.case or CASES:
        started = time.monotonic()
        with tempfile.TemporaryDirectory(prefix="clack-pty-") as directory:
            ctx = Context(binary, Path(directory), args.hooks)
            try:
                CASES[name](ctx)
                status, detail = "pass", "assertions satisfied"
            except CheckSkipped as error:
                status, detail = "skipped", str(error)
            except (CheckFailure, OSError, TimeoutError, ValueError, KeyError) as error:
                status, detail = "fail", str(error)
        result = {"case": name, "status": status, "seconds": round(time.monotonic() - started, 6), "detail": detail}
        results.append(result)
        if not args.json:
            print(f"{status.upper():7} {name}: {detail}")
        if status == "fail" and args.fail_fast:
            break
    check(hashlib.sha256(binary.read_bytes()).hexdigest() == binary_hash,
          "test binary changed during verification")
    report = {
        "report_version": 1,
        "kind": "unix_pty_integration",
        "binary": str(binary),
        "binary_sha256": binary_hash,
        "execution_label": args.execution_label,
        "test_hooks": args.hooks,
        "platform": sys.platform,
        "python": sys.version.split()[0],
        "environment": environment_report(),
        "created_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "terminal_policy": "synthetic xterm-256color PTY; baseline80x24 unless case resizes",
        "limitations": [
            "No actual emulator, font/IME, Windows console, tmux/SSH, system-suspend, or physical key-to-photon claim.",
            "Exact receipt-at-deadline exclusion belongs to deterministic reader/reducer fixtures, not PTY write wall times.",
            "Fault/epoch observations use an explicitly instrumented build and are not production performance measurements.",
        ],
        "results": results,
    }
    rendered = json.dumps(report, ensure_ascii=True, indent=2) + "\n"
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(rendered, encoding="utf-8")
    if args.json:
        print(rendered, end="")
    failed = any(row["status"] == "fail" for row in results)
    skipped = any(row["status"] == "skipped" for row in results)
    return 1 if failed or (args.require_all and skipped) else 0


if __name__ == "__main__":
    raise SystemExit(main())
