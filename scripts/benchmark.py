#!/usr/bin/env python3
"""External, bounded PTY measurements for clack; never a physical key-to-photon test.

Requires the Unix stdlib PTY helper in scripts/pty_test.py. See
docs/benchmark-methodology.md before using results as a release gate.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import signal
import sqlite3
import statistics
import subprocess
import sys
import tempfile
import time
from typing import Callable
from urllib.parse import quote

ROOT = Path(__file__).resolve().parent.parent
SCHEMA_VERSION = 1
F5 = b"\x1b[15~"
MAX_HOOK_BYTES = 2 * 1024 * 1024


def percentile(values: list[float], percent: float) -> float | None:
    """Nearest-rank percentile, deliberately not interpolated across observations."""
    if not values:
        return None
    ordered = sorted(values)
    return ordered[max(0, math.ceil(percent * len(ordered)) - 1)]


def summarize(values: list[float]) -> dict:
    return {
        "n": len(values),
        "min": min(values) if values else None,
        "median": statistics.median(values) if values else None,
        "p95": percentile(values, 0.95),
        "p99": percentile(values, 0.99),
        "max": max(values) if values else None,
    }


def command_text(argv: list[str]) -> str | None:
    try:
        result = subprocess.run(argv, capture_output=True, text=True, timeout=5, check=False)
        return result.stdout.strip() if result.returncode == 0 else None
    except (OSError, subprocess.SubprocessError):
        return None


def environment(binary: Path, args: argparse.Namespace) -> dict:
    uname = platform.uname()
    cpu = platform.processor() or None
    memory_bytes = None
    if sys.platform == "darwin":
        cpu = command_text(["/usr/sbin/sysctl", "-n", "machdep.cpu.brand_string"]) or cpu
        raw_memory = command_text(["/usr/sbin/sysctl", "-n", "hw.memsize"])
        memory_bytes = int(raw_memory) if raw_memory and raw_memory.isdigit() else None
    elif sys.platform.startswith("linux"):
        try:
            info = Path("/proc/cpuinfo").read_text()
            cpu = next((line.split(":", 1)[1].strip() for line in info.splitlines() if line.startswith("model name")), cpu)
            memory_line = next(line for line in Path("/proc/meminfo").read_text().splitlines() if line.startswith("MemTotal:"))
            memory_bytes = int(memory_line.split()[1]) * 1024
        except (OSError, StopIteration, ValueError):
            pass
    # This runs AFTER a declared cold launch; reading the executable before that
    # measurement would itself warm its cache.
    digest = hashlib.sha256()
    with binary.open("rb") as executable:
        for chunk in iter(lambda: executable.read(1024 * 1024), b""):
            digest.update(chunk)
    build_manifest = None
    if args.build_manifest:
        raw = args.build_manifest.read_bytes()
        if len(raw) > 4 * 1024 * 1024:
            raise ValueError("build manifest exceeds 4 MiB")
        build_manifest = {"path": str(args.build_manifest.resolve()),
                          "sha256": hashlib.sha256(raw).hexdigest(),
                          "content": json.loads(raw)}
    return {
        "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "cpu": cpu,
        "logical_cores": os.cpu_count(),
        "memory_bytes": memory_bytes,
        "os": uname.system,
        "os_release": uname.release,
        "os_version": uname.version,
        "architecture": uname.machine,
        "python": sys.version,
        "rustc": command_text(["rustc", "-Vv"]),
        "cargo": command_text(["cargo", "-V"]),
        "execution": "ssh" if os.environ.get("SSH_CONNECTION") else "local",
        "terminal_kind": "Unix pseudoterminal with in-memory VT screen parser",
        "terminal_emulator": None,
        "terminal_version": None,
        "actual_emulator_measured": False,
        "term": args.term,
        "color_environment": {"COLORTERM": "truecolor", **{key: os.environ.get(key) for key in ("NO_COLOR", "LANG", "LC_ALL")}},
        "binary": str(binary),
        "binary_sha256": digest.hexdigest(),
        "binary_bytes": binary.stat().st_size,
        "build_description": args.build_description,
        "supplied_build_manifest": build_manifest,
        "portable_cpu_target_verified": args.portable_cpu_target_confirmed,
        "controlled_reference": args.controlled_reference,
        "storage": "isolated SQLite enabled" if args.with_history else "--private; result persistence suppressed",
        "config": "isolated schema_version=1; built-in defaults, then explicit workload flags",
        "history_size_at_start": 0,
        "clock": "time.monotonic_ns / time.perf_counter_ns (monotonic, wall-clock independent)",
        "clock_resolution_s": time.get_clock_info("monotonic").resolution,
    }


def parse_cpu_time(value: str) -> float:
    """Parse ps [[days-]hours:]minutes:seconds[.fraction]."""
    value = value.strip()
    days = 0
    if "-" in value:
        day, value = value.split("-", 1)
        days = int(day)
    parts = [float(part) for part in value.split(":")]
    if not 1 <= len(parts) <= 3:
        raise ValueError("unrecognized ps CPU time")
    seconds = sum(part * (60 ** index) for index, part in enumerate(reversed(parts)))
    return days * 86400 + seconds


@dataclass
class ProcessSample:
    observed_ns: int
    cpu_seconds: float
    rss_bytes: int
    cpu_resolution_seconds: float
    method: str


def process_sample(pid: int) -> ProcessSample | None:
    if sys.platform.startswith("linux"):
        try:
            stat = Path(f"/proc/{pid}/stat").read_text()
            fields = stat[stat.rfind(")") + 2:].split()
            ticks = os.sysconf("SC_CLK_TCK")
            return ProcessSample(time.monotonic_ns(), (int(fields[11]) + int(fields[12])) / ticks,
                                 int(fields[21]) * os.sysconf("SC_PAGE_SIZE"), 1 / ticks, "/proc/PID/stat")
        except (OSError, ValueError, IndexError):
            return None
    text = command_text(["ps", "-o", "time=", "-o", "rss=", "-p", str(pid)])
    if not text:
        return None
    try:
        cpu, rss = text.split()
        resolution = 0.01 if "." in cpu else 1.0
        return ProcessSample(time.monotonic_ns(), parse_cpu_time(cpu), int(rss) * 1024, resolution, "ps time/rss")
    except ValueError:
        return None


def cpu_interval(before: ProcessSample | None, after: ProcessSample | None) -> dict:
    if before is None or after is None:
        return {"available": False, "reason": "OS process accounting unavailable"}
    elapsed = (after.observed_ns - before.observed_ns) / 1e9
    cpu = max(0.0, after.cpu_seconds - before.cpu_seconds)
    return {
        "available": elapsed > 0,
        "elapsed_seconds": elapsed,
        "cpu_seconds": cpu,
        "percent_of_one_logical_core": 100 * cpu / elapsed if elapsed > 0 else None,
        "accounting_resolution_seconds": max(before.cpu_resolution_seconds, after.cpu_resolution_seconds),
        "resolution_percent_of_interval": 100 * max(before.cpu_resolution_seconds, after.cpu_resolution_seconds) / elapsed if elapsed > 0 else None,
        "method": before.method,
        "rss_before_bytes": before.rss_bytes,
        "rss_after_bytes": after.rss_bytes,
    }


def numeric_only(value) -> bool:
    if value is None or isinstance(value, bool):
        return True
    if isinstance(value, (int, float)):
        return not isinstance(value, float) or math.isfinite(value)
    if isinstance(value, list):
        return all(numeric_only(item) for item in value)
    if isinstance(value, dict):
        return all(isinstance(key, str) and numeric_only(item) for key, item in value.items())
    return False


class Session:
    """One isolated real application process; no diagnostic writes while typing."""
    def __init__(self, args: argparse.Namespace, geometry: tuple[int, int], flags: list[str]):
        from pty_test import PtyProcess
        self.directory = tempfile.TemporaryDirectory(prefix="clack-benchmark-")
        directory = Path(self.directory.name)
        self.database = directory / "data" / "history.sqlite3"
        self.with_history = args.with_history
        config = directory / "config.toml"
        config.write_text("schema_version = 1\n")
        self.hook_path = directory / "observations.json"
        self.argv = [str(args.binary), "--config", str(config), "--data-dir", str(directory / "data"),
                     "--benchmark-output", str(self.hook_path)]
        if not args.with_history:
            self.argv.append("--private")
        self.argv.extend(flags)
        self.flags = flags
        self.geometry = geometry
        self.proc = PtyProcess(self.argv, cols=geometry[0], rows=geometry[1],
                               env={"TERM": args.term}, capture_stdout=True, observe=False)
        self.closed = False
        self.last_output_ns = None
        self.last_delivery_completed_ns = None
        self.delivery_count = 0
        self.delivery_bytes = 0
        self.max_schedule_lag_us = 0.0

    def read(self, timeout: float = 0.005) -> bytes:
        chunk = self.proc.read(timeout=max(0.0, timeout))
        if chunk:
            self.last_output_ns = time.monotonic_ns()
        # The screen holds current cells; monotonic byte totals are in the helper.
        # Discard raw display data immediately instead of accumulating a transcript.
        self.proc.output.clear()
        self.proc.stdout.clear()
        self.proc.stderr.clear()
        return chunk

    def screen_text(self) -> str:
        return self.proc.screen.text().lower()

    def wait_screen(self, predicate: Callable[[str], bool], timeout: float = 5.0) -> int:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.read(min(0.005, max(0.0, deadline - time.monotonic())))
            if predicate(self.screen_text()):
                return time.monotonic_ns()
            if not self.proc.is_alive():
                raise RuntimeError("application exited before the expected usable screen")
        raise TimeoutError("application did not render the expected screen before the deadline")

    def ready(self, timeout: float = 5.0) -> int:
        return self.wait_screen(lambda text: "start typing" in text and
                                ("esc" in text or "commands" in text) and self.proc.screen.cursor_visible,
                                timeout)

    def results(self, timeout: float = 5.0) -> int:
        return self.wait_screen(lambda text: "enter next" in text or ("f2 repeat" in text and "wpm" in text), timeout)

    def send_text(self, unit: str) -> int:
        stamp = time.monotonic_ns()
        encoded = unit.encode("utf-8")
        self.proc.send(encoded)
        self.last_delivery_completed_ns = time.monotonic_ns()
        self.delivery_count += 1
        self.delivery_bytes += len(encoded)
        return stamp

    def observe_for(self, seconds: float):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            self.read(min(0.05, max(0.0, deadline - time.monotonic())))
            if not self.proc.is_alive():
                raise RuntimeError("application exited during the measurement interval")

    def finish(self) -> dict:
        if self.closed:
            return {}
        try:
            if self.proc.is_alive():
                self.proc.send(b"\x03")
            exit_code = self.proc.wait(timeout=5)
            restored = self.proc.terminal_restored()
            observations = None
            hook_error = None
            if self.hook_path.exists():
                if self.hook_path.stat().st_size > MAX_HOOK_BYTES:
                    hook_error = "numeric observation file exceeded 2 MiB"
                else:
                    try:
                        observations = json.loads(self.hook_path.read_text())
                        if not isinstance(observations, dict) or observations.get("schema_version") != 1 or not numeric_only(observations):
                            observations = None
                            hook_error = "observation file did not match the versioned numeric schema"
                    except (ValueError, OSError):
                        hook_error = "observation file was not valid finite numeric JSON"
            else:
                hook_error = "application did not produce a numeric observation file"
            result = {
                "exit_code": exit_code,
                "terminal_restored": restored,
                "pty_output_bytes": self.proc.output_bytes,
                "stdout_bytes": self.proc.stdout_bytes,
                "stderr_bytes": self.proc.stderr_bytes,
                "delivered_units": self.delivery_count,
                "delivered_bytes": self.delivery_bytes,
                "max_send_schedule_lag_us": self.max_schedule_lag_us,
                "observations": observations,
                "observation_error": hook_error,
            }
            flags = list(self.flags)
            if "--text" in flags:
                at = flags.index("--text") + 1
                source = flags[at].encode("utf-8")
                flags[at] = f"<synthetic target: {len(source)} bytes, sha256={hashlib.sha256(source).hexdigest()}>"
            result["workload_flags"] = flags
            if observations:
                latencies = observations.get("receive_to_flush_us", [])
                if isinstance(latencies, list):
                    result["received_to_completed_flush_us"] = summarize(latencies)
                    # The full bounded raw sample array is unnecessary in the report.
                    observations.pop("receive_to_flush_us", None)
                result["production_build_verified"] = (
                    observations.get("debug_assertions") == 0 and
                    observations.get("test_hooks_enabled") == 0)
            result["history_after_exit"] = inspect_history(
                self.database, self.with_history, observations
            )
            return result
        finally:
            self.closed = True
            self.proc.close()
            self.directory.cleanup()

    def abort(self):
        if self.closed:
            return
        try:
            self.proc.signal(signal.SIGTERM)
        except (ProcessLookupError, OSError):
            pass
        try:
            self.proc.wait(timeout=2)
        except (OSError, TimeoutError):
            pass
        self.proc.close()
        self.directory.cleanup()
        self.closed = True


def inspect_history(path: Path, enabled: bool, observations: dict | None) -> dict:
    """Read isolated storage only after the measured process has exited.

    No SQL work contributes to the typing/latency/CPU interval. The comparison
    proves storage-enabled restart tests saved their finalized snapshots rather
    than merely cycling the UI and then discarding the in-memory results.
    """
    expected = None
    if observations is not None:
        expected = (observations.get("completed_runs", 0) +
                    observations.get("interrupted_runs", 0)) if enabled else 0
    result = {"enabled": enabled, "database_exists": path.exists(),
              "expected_result_count": expected, "result_count": 0,
              "private_body_rows": 0, "event_trace_rows": 0,
              "entered_text_rows": 0, "schema_version": None,
              "read_after_application_exit": True}
    if path.exists():
        try:
            with sqlite3.connect("file:" + quote(str(path)) + "?mode=ro", uri=True) as connection:
                connection.execute("PRAGMA query_only=ON")
                result["schema_version"] = connection.execute("PRAGMA user_version").fetchone()[0]
                initialized = connection.execute(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='results'"
                ).fetchone()[0] == 1
                if initialized:
                    result["result_count"] = connection.execute("SELECT COUNT(*) FROM results").fetchone()[0]
                    result["private_body_rows"] = connection.execute("SELECT COUNT(*) FROM private_content").fetchone()[0]
                    result["event_trace_rows"] = connection.execute("SELECT COUNT(*) FROM event_traces").fetchone()[0]
                    result["entered_text_rows"] = connection.execute(
                        "SELECT COUNT(*) FROM word_summaries WHERE json_extract(summary_json,'$.entered')!=''"
                    ).fetchone()[0]
                    result["max_samples_per_result"] = connection.execute(
                        "SELECT COALESCE(MAX(n),0) FROM (SELECT COUNT(*) n FROM samples GROUP BY result_id)"
                    ).fetchone()[0]
        except (OSError, sqlite3.Error) as error:
            result["error"] = type(error).__name__
    result["finalized_count_matches_storage"] = (
        result["result_count"] == expected if expected is not None else None
    )
    result["default_privacy_intact"] = (
        not result.get("error") and result["private_body_rows"] == 0 and
        result["event_trace_rows"] == 0 and result["entered_text_rows"] == 0 and
        (enabled or not result["database_exists"])
    )
    return result


def default_trace(units: int) -> list[str]:
    """Independent integer reference for the version-1, seed-42 English trace.

    It prepares public test input before any timed process measurement; it does
    not implement scoring. The prefix is checked against the committed golden.
    """
    words = (ROOT / "data/packs/english_200/words.txt").read_text().splitlines()
    state, mask, previous = 42, (1 << 64) - 1, None

    def bounded(bound: int) -> int:
        nonlocal state
        threshold = ((1 << 64) - bound) % bound
        while True:
            state = (state + 0x9e3779b97f4a7c15) & mask
            z = ((state ^ (state >> 30)) * 0xbf58476d1ce4e5b9) & mask
            z = ((z ^ (z >> 27)) * 0x94d049bb133111eb) & mask
            value = z ^ (z >> 31)
            if value >= threshold:
                return value % bound

    output = []
    while len(output) < max(units, 256):
        index = bounded(len(words) if previous is None else len(words) - 1)
        if previous is not None and index >= previous:
            index += 1
        previous = index
        output.extend(words[index] + " ")
    golden = json.loads((ROOT / "data/generator-v1-golden.json").read_text())
    expected = next(case["text"] for case in golden["cases"] if case["pack_id"] == "english_200" and case["seed"] == 42 and not case["punctuation"] and not case["numbers"])
    if not "".join(output).startswith(expected):
        raise RuntimeError("benchmark trace no longer matches generator version 1")
    return output[:units]


def paced_send(session: Session, trace: list[str], rate: float, *, read_interval: float = 0,
               on_progress: Callable[[int], None] | None = None) -> dict:
    start = time.monotonic_ns()
    next_read = time.monotonic()
    last_send = start
    for index, unit in enumerate(trace):
        due_ns = start + round(index * 1e9 / rate)
        while time.monotonic_ns() < due_ns:
            remaining = (due_ns - time.monotonic_ns()) / 1e9
            if time.monotonic() >= next_read:
                session.read(min(remaining, 0.005))
                next_read = time.monotonic() + read_interval
            else:
                time.sleep(max(0.0, min(remaining, next_read - time.monotonic(), 0.005)))
        if time.monotonic() >= next_read:
            session.read(0)
            next_read = time.monotonic() + read_interval
        session.send_text(unit)
        last_send = session.last_delivery_completed_ns
        session.max_schedule_lag_us = max(session.max_schedule_lag_us, (last_send - due_ns) / 1000)
        if on_progress:
            on_progress(index + 1)
        if not session.proc.is_alive():
            break
    elapsed = (last_send - start) / 1e9
    return {"target_units_per_second": rate, "delivery_seconds": elapsed,
            "actual_units_per_second": (session.delivery_count - 1) / elapsed if elapsed > 0 and session.delivery_count > 1 else None}


def run_safely(name: str, geometry: tuple[int, int], work: Callable[[], dict]) -> dict:
    print(f"benchmark: {name} at {geometry[0]}x{geometry[1]}", file=sys.stderr, flush=True)
    try:
        result = work()
        failures = []
        if result.get("terminal_restored") is False:
            failures.append("terminal attributes were not restored")
        if result.get("integrity_assertion", "").startswith("failed:"):
            failures.append(result["integrity_assertion"])
        if result.get("output_timeouts", 0) > 0:
            failures.append("some text deliveries produced no output within 250 ms")
        failures.extend(result.get("buffer_bound_failures", []))
        for run in result.get("runs", []):
            if run.get("terminal_restored") is False:
                failures.append("a startup run did not restore terminal attributes")
        for run in result.get("runs", [result]):
            if run.get("observation_error"):
                failures.append(run["observation_error"])
            if run.get("production_build_verified") is False:
                failures.append("the measured binary is not a production release without test hooks")
            if run.get("stdout_bytes", 0) or run.get("stderr_bytes", 0):
                failures.append("the measured application emitted unexpected stdout/stderr")
            history = run.get("history_after_exit") or {}
            if history.get("error"):
                failures.append("post-exit history inspection failed")
            if history.get("finalized_count_matches_storage") is False:
                failures.append("finalized result counts did not match post-exit SQLite history")
            if history.get("default_privacy_intact") is False:
                failures.append("default/private persistence contained forbidden text or traces")
        return {"name": name, "geometry": list(geometry), "status": "failed" if failures else "measured",
                "failures": failures, **result}
    except (OSError, RuntimeError, TimeoutError, ValueError) as error:
        return {"name": name, "geometry": list(geometry), "status": "error", "error": str(error)}


def startup(args, geometry, *, cold=False):
    values, details = [], []
    runs = 1 if cold else args.startup_runs + args.warmup_runs
    for index in range(runs):
        session = Session(args, geometry, ["--time", "30", "--language", "english_200"])
        try:
            ready_ns = session.ready()
            value = (ready_ns - session.proc.started_ns) / 1000
            measured = cold or index >= args.warmup_runs
            if measured:
                values.append(value)
                detail = session.finish()
                detail["process_to_usable_screen_us"] = value
                details.append(detail)
            else:
                session.finish()
        finally:
            session.abort()
    return {"condition": "operator-declared cold launch" if cold else "warm-cache launch after explicit warmups",
            "cold_cache_state_verified": False if cold else None,
            "cold_methodology": args.cold_methodology if cold else None,
            "observer": "Ready hint, visible caret and complete current VT screen; external receipt upper bound",
            "warmups_excluded": 0 if cold else args.warmup_runs,
            "startup_us": summarize(values), "runs": details,
            "target_p95_us": 200_000 if cold else 50_000,
            "target_is_controlled_machine_gate": bool(args.controlled_reference) and not cold}


def latency(args, geometry, *, mixed=False):
    count = args.latency_events
    mixed_pattern = ["c", "a", "f", "é", " ", "e\u0301", " ", "界", "界", " ", "👩‍💻", " "]
    trace = (mixed_pattern * (count // len(mixed_pattern) + 2))[:count] if mixed else default_trace(count)
    if mixed:
        flags = ["--text", "".join(trace) + " end", "--exact"]
    else:
        flags = ["--time", str(min(3600, max(30, math.ceil(count * 0.05) + 2))), "--seed", "42", "--language", "english_200"]
    session = Session(args, geometry, flags)
    samples, misses = [], 0
    try:
        session.ready()
        for unit in trace:
            session.read(0)
            before = session.proc.output_bytes
            delivered = session.send_text(unit)
            deadline = time.monotonic() + 0.25
            while session.proc.output_bytes == before and time.monotonic() < deadline:
                session.read(0.005)
            if session.proc.output_bytes > before:
                samples.append((session.last_output_ns - delivered) / 1000)
            else:
                misses += 1
            # Separate deliveries enough that one output cannot stand for two
            # distinct latency probes. This is not the high-rate stress workload.
            session.observe_for(0.025)
        result = session.finish()
        return {"content": "mixed-width exact UTF-8" if mixed else "default English 200, seed 42",
                "pty_delivery_to_next_output_us": summarize(samples), "output_timeouts": misses,
                "external_latency_caveat": "next PTY output may be a timer frame; use application received-to-flush data for its acceptance target",
                "target_application_p95_us": 10_000, "target_application_p99_us": 20_000, **result}
    finally:
        session.abort()


def idle(args, geometry, *, results=False):
    session = Session(args, geometry, ["--time", "1" if results else "30"])
    try:
        session.ready()
        if results:
            session.send_text("t")
            session.results(timeout=3)
        session.observe_for(0.25)
        before_bytes = session.proc.output_bytes
        before = process_sample(session.proc.pid)
        session.observe_for(args.idle_seconds)
        after = process_sample(session.proc.pid)
        interval_bytes = session.proc.output_bytes - before_bytes
        result = session.finish()
        return {"state": "Results" if results else "Ready", "requested_seconds": args.idle_seconds,
                "cpu": cpu_interval(before, after), "output_bytes_during_idle": interval_bytes,
                "target_percent_of_one_core": 0.1, "required_gate_interval_met": args.idle_seconds >= 60, **result}
    finally:
        session.abort()


def active(args, geometry):
    rate = 150 * 5 / 60
    trace = default_trace(math.floor(args.active_seconds * rate))
    session = Session(args, geometry, ["--time", str(max(1, math.ceil(args.active_seconds))), "--seed", "42", "--language", "english_200"])
    try:
        session.ready()
        before = process_sample(session.proc.pid)
        pacing = paced_send(session, trace, rate)
        session.observe_for(1 / rate + 0.005)
        after = process_sample(session.proc.pid)
        result = session.finish()
        return {"replayed_five_character_equivalent_wpm": 150, "content": "default English 200, seed 42",
                "cpu": cpu_interval(before, after), "pacing": pacing, "target_percent_of_one_core": 3.0, **result}
    finally:
        session.abort()


def memory(args, geometry):
    session = Session(args, geometry, ["--words", "1", "--language", "english_200"])
    rss = []
    try:
        session.ready()
        for index in range(args.memory_runs):
            # A wrong final nonempty word submitted with Space is a normal
            # completion; no guessed random target or timing shortcut is needed.
            session.send_text("x")
            session.send_text(" ")
            session.results()
            if index % max(1, args.memory_runs // 100) == 0 or index + 1 == args.memory_runs:
                sample = process_sample(session.proc.pid)
                if sample:
                    rss.append({"completed_runs": index + 1, "rss_bytes": sample.rss_bytes})
            session.proc.send(b"\x12")
            session.ready()
        result = session.finish()
        values = [sample["rss_bytes"] for sample in rss]
        tail = values[len(values) // 2:]
        return {"workload": "successive completed one-word normal tests and Ctrl-R restarts",
                "requested_runs": args.memory_runs, "required_1000_runs_met": args.memory_runs >= 1000,
                "rss_samples": rss, "rss_bytes": summarize(values),
                "tail_growth_bytes": tail[-1] - tail[0] if len(tail) >= 2 else None,
                "default_memory_target_bytes": 30 * 1024 * 1024,
                "limitation": "bounded reset test, not 1,000 full 30-second sessions", **result}
    finally:
        session.abort()


def stress(args, geometry, *, slow=False):
    count = math.floor(args.stress_seconds * args.stress_rate)
    trace = default_trace(count)
    session = Session(args, geometry, ["--time", str(math.ceil(args.stress_seconds) + 2), "--seed", "42", "--language", "english_200"])
    try:
        session.ready()
        pacing = paced_send(session, trace, args.stress_rate, read_interval=args.slow_read_ms / 1000 if slow else 0)
        session.observe_for(0.5)
        result = session.finish()
        hook = result.get("observations") or {}
        applied = hook.get("text_events_applied_count")
        overload = hook.get("input_overload_count")
        violations = hook.get("sequence_violation_count")
        if violations is not None and violations != 0:
            assertion = "failed: input sequence violation"
        elif overload is not None and overload > 0:
            assertion = "explicit overload interruption observed"
        elif applied is not None and violations == 0:
            assertion = "ordered delivery counts match" if applied == result["delivered_units"] else "failed: delivered/applied text counts differ"
        else:
            assertion = "unverified: application did not provide all required numeric integrity hooks"
        return {"consumer": "interval-throttled PTY reads" if slow else "continuously drained PTY",
                "read_interval_ms": args.slow_read_ms if slow else 0, "pacing": pacing,
                "requested_units": count, "integrity_assertion": assertion,
                "requested_1000_event_per_second_workload": args.stress_rate >= 1000,
                "delivered_within_one_percent_of_requested_rate": (
                    pacing["actual_units_per_second"] is not None and
                    pacing["actual_units_per_second"] >= args.stress_rate * 0.99),
                "rate_caveat": "Requested rate alone does not establish a 1,000-event/second delivered workload; inspect actual pacing and scheduling lag.",
                **result}
    finally:
        session.abort()


def zen(args, geometry):
    session = Session(args, geometry, ["--zen"])
    rss = []
    trace = list(("one two three four " * (args.zen_units // 19 + 2))[:args.zen_units])
    try:
        session.ready()
        started = time.monotonic()
        def progress(count):
            if count % 1024 == 0:
                sample = process_sample(session.proc.pid)
                if sample:
                    rss.append({"inserted_units": count, "rss_bytes": sample.rss_bytes})
        pacing = paced_send(session, trace, args.stress_rate, on_progress=progress)
        while time.monotonic() - started < args.zen_seconds:
            session.observe_for(min(30, args.zen_seconds - (time.monotonic() - started)))
            sample = process_sample(session.proc.pid)
            if sample:
                rss.append({"inserted_units": len(trace), "elapsed_seconds": time.monotonic() - started, "rss_bytes": sample.rss_bytes})
            print(f"benchmark: zen elapsed {time.monotonic() - started:.0f}s / {args.zen_seconds:.0f}s", file=sys.stderr, flush=True)
        elapsed = time.monotonic() - started
        session.proc.send(F5)
        session.results()
        result = session.finish()
        hook = result.get("observations") or {}
        bounds = []
        editable = hook.get("max_editable_units")
        chart = hook.get("max_chart_samples")
        if editable is not None and editable > 4096:
            bounds.append("zen editable window exceeded 4,096 units")
        if chart is not None and chart > 3601:
            bounds.append("zen chart window exceeded 3,601 samples")
        filled = [sample["rss_bytes"] for sample in rss if sample["inserted_units"] >= 8192]
        return {"workload": "zen until 4,096-unit retained window cycles repeatedly", "requested_units": args.zen_units,
                "window_cycles_requested": args.zen_units / 4096, "pacing": pacing, "rss_samples": rss,
                "elapsed_seconds": elapsed, "chart_saturation_interval_met": elapsed > 3601,
                "numeric_buffer_bounds_observed": editable is not None and chart is not None,
                "buffer_bound_failures": bounds,
                "delivered_text_count_matches": hook.get("text_events_applied_count") == result["delivered_units"] if "text_events_applied_count" in hook else None,
                "post_saturation_rss_bytes": summarize(filled),
                "post_saturation_growth_bytes": filled[-1] - filled[0] if len(filled) >= 2 else None, **result}
    finally:
        session.abort()


def compare_baseline(report: dict, path: Path) -> dict:
    previous = json.loads(path.read_text())
    identity = ["cpu", "os", "os_release", "architecture", "logical_cores", "memory_bytes",
                "controlled_reference", "storage", "history_size_at_start", "term", "color_environment"]
    compatible = all(report["environment"].get(key) == previous.get("environment", {}).get(key) for key in identity)
    if not report["environment"].get("controlled_reference") or not compatible:
        return {"compared": False, "reason": "requires the same named controlled machine, OS and storage options"}
    old = {(record["name"], tuple(record["geometry"])): record for record in previous["measurements"]}
    changes = []
    for current in report["measurements"]:
        prior = old.get((current["name"], tuple(current["geometry"])))
        if not prior or current.get("status") != "measured" or prior.get("status") != "measured":
            continue
        if current.get("workload_flags") != prior.get("workload_flags"):
            continue
        for metric in ("startup_us", "received_to_completed_flush_us", "pty_delivery_to_next_output_us", "rss_bytes"):
            before = (prior.get(metric) or {}).get("p95")
            after = (current.get(metric) or {}).get("p95")
            if before is not None and before > 0 and after is not None:
                increase = 100 * (after / before - 1)
                changes.append({"name": current["name"], "geometry": current["geometry"], "metric": metric,
                                "p95_increase_percent": increase, "repeat_to_investigate": increase >= 10})
    return {"compared": True, "changes": changes,
            "interpretation": "a >=10% increase requests repetition and investigation, not a single-run CI performance failure"}


def parser():
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("--binary", type=Path, default=ROOT / "target/release/clack")
    result.add_argument("--suite", choices=["quick", "full", "startup", "cold", "latency", "idle", "active", "memory", "stress", "zen"], default="quick")
    result.add_argument("--output", type=Path, help="JSON output file; omitted means stdout")
    result.add_argument("--baseline", type=Path)
    result.add_argument("--geometry", action="append", help="COLSxROWS; repeat for multiple geometries")
    result.add_argument("--term", default="xterm-256color")
    result.add_argument("--startup-runs", type=int, default=50)
    result.add_argument("--warmup-runs", type=int, default=3)
    result.add_argument("--latency-events", type=int, default=200)
    result.add_argument("--idle-seconds", type=float, default=60)
    result.add_argument("--active-seconds", type=float, default=30)
    result.add_argument("--memory-runs", type=int, default=1000)
    result.add_argument("--stress-seconds", type=float, default=5)
    result.add_argument("--stress-rate", type=float, default=1000)
    result.add_argument("--slow-read-ms", type=float, default=250)
    result.add_argument("--zen-units", type=int, default=20_480)
    result.add_argument("--zen-seconds", type=float, default=0, help="optional minimum real zen duration; use >3601 to cycle the one-second sample window")
    result.add_argument("--self-test", action="store_true", help="verify harness arithmetic, schema and golden trace without launching clack")
    result.add_argument("--with-history", action="store_true", help="enable isolated normal storage instead of --private")
    result.add_argument("--cold-methodology", help="operator's exact cache/reboot procedure; the harness never purges caches")
    result.add_argument("--controlled-reference", help="name of a dedicated reference machine; omit on shared/noisy hosts")
    result.add_argument("--build-description", default="Caller-supplied binary; compiler flags and profile are not independently verified")
    result.add_argument("--build-manifest", type=Path, help="attach the immutable JSON source/build manifest captured when this binary was built")
    result.add_argument("--portable-cpu-target-confirmed", action="store_true", help="record caller confirmation that no target-cpu=native was used")
    return result


def self_test():
    assert parse_cpu_time("1-02:03:04.50") == 93784.5
    assert parse_cpu_time("0:00.01") == 0.01
    assert percentile(list(range(1, 101)), 0.95) == 95
    assert summarize([])["p99"] is None
    assert numeric_only({"count": [1, 2, None, False]})
    assert not numeric_only({"duration": float("inf")})
    assert not numeric_only({"text": "private"})
    assert len(default_trace(10_000)) == 10_000
    interval = cpu_interval(ProcessSample(1_000_000_000, 1.0, 100, .01, "fixture"),
                            ProcessSample(61_000_000_000, 1.03, 100, .01, "fixture"))
    assert math.isclose(interval["percent_of_one_logical_core"], .05)
    accounting = process_sample(os.getpid())
    with tempfile.TemporaryDirectory(prefix="clack-benchmark-selftest-") as directory:
        result = inspect_history(Path(directory) / "missing.sqlite3", False, {"completed_runs": 5})
        assert result["default_privacy_intact"]
        assert result["finalized_count_matches_storage"]
        assert not result["database_exists"]
    print(json.dumps({"self_test": "passed", "application_launched": False,
                      "process_accounting_available": accounting is not None}, allow_nan=False))


def main(argv=None):
    cli = parser()
    args = cli.parse_args(argv)
    if args.self_test:
        self_test()
        return 0
    if os.name != "posix":
        cli.error("this PTY harness requires Unix; Windows Terminal validation needs a Windows runner")
    args.binary = args.binary.resolve()
    if not args.binary.is_file():
        cli.error("build the production release binary first, then pass --binary")
    if args.suite == "cold" and not args.cold_methodology:
        cli.error("--suite cold requires --cold-methodology describing external cache preparation")
    for name in ("startup_runs", "latency_events", "idle_seconds", "active_seconds", "memory_runs", "stress_seconds", "stress_rate", "slow_read_ms", "zen_units"):
        if not math.isfinite(getattr(args, name)) or getattr(args, name) <= 0:
            cli.error(name.replace("_", "-") + " must be positive and finite")
    if args.warmup_runs < 0:
        cli.error("warmup-runs must be nonnegative")
    if not math.isfinite(args.zen_seconds) or not 0 <= args.zen_seconds <= 43_200:
        cli.error("zen-seconds must be finite and between 0 and 43,200")
    if args.active_seconds > 3600 or args.stress_seconds > 3598:
        cli.error("timed benchmark durations must fit the application's 3,600-second limit")
    limits = {"startup_runs": 10_000, "latency_events": 10_000, "memory_runs": 100_000,
              "stress_rate": 10_000, "zen_units": 1_000_000}
    for name, maximum in limits.items():
        if getattr(args, name) > maximum:
            cli.error(name.replace("_", "-") + f" must be at most {maximum}")
    if args.stress_seconds * args.stress_rate > 1_000_000:
        cli.error("a stress trace is limited to 1,000,000 delivered units")
    try:
        geometries = [tuple(map(int, geometry.lower().split("x"))) for geometry in args.geometry] if args.geometry else [(80, 24), (120, 40), (200, 60)]
        if any(len(geometry) != 2 or not 40 <= geometry[0] <= 1000 or not 10 <= geometry[1] <= 500 for geometry in geometries):
            raise ValueError()
    except ValueError:
        cli.error("geometry must be COLSxROWS within 40x10 and 1000x500")
    if args.suite == "quick":
        geometries = geometries if args.geometry else [(120, 40)]
        args.startup_runs = min(args.startup_runs, 10)
        args.latency_events = min(args.latency_events, 25)
    measurements = []
    if args.suite == "cold":
        # Nothing above has opened/read/run the binary. Record environment and
        # hashes only after this first external launch.
        measurements.append(run_safely("cold_startup", geometries[0], lambda: startup(args, geometries[0], cold=True)))
    host = environment(args.binary, args)
    for geometry in geometries:
        suites = ["startup", "latency"] if args.suite == "quick" else ["startup", "latency", "active", "stress"] if args.suite == "full" else [args.suite]
        for suite in suites:
            if suite == "startup":
                measurements.append(run_safely("warm_startup", geometry, lambda: startup(args, geometry)))
            elif suite == "latency":
                for mixed in (False, True):
                    measurements.append(run_safely("latency_mixed" if mixed else "latency_ascii", geometry, lambda mixed=mixed: latency(args, geometry, mixed=mixed)))
            elif suite == "active":
                measurements.append(run_safely("active_150wpm", geometry, lambda: active(args, geometry)))
            elif suite == "stress":
                for slow in (False, True):
                    measurements.append(run_safely("stress_slow" if slow else "stress_healthy", geometry, lambda slow=slow: stress(args, geometry, slow=slow)))
    for suite in (["idle", "memory", "zen"] if args.suite == "full" else [args.suite]):
        geometry = (120, 40) if args.suite == "full" or not args.geometry else geometries[0]
        if suite == "idle":
            for results in (False, True):
                measurements.append(run_safely("idle_results" if results else "idle_ready", geometry, lambda results=results: idle(args, geometry, results=results)))
        elif suite == "memory":
            measurements.append(run_safely("memory_restarts", geometry, lambda: memory(args, geometry)))
        elif suite == "zen":
            measurements.append(run_safely("zen_bounds", geometry, lambda: zen(args, geometry)))
    report = {"schema_version": SCHEMA_VERSION, "environment": host, "suite": args.suite,
              "measurements": measurements,
              "unmeasured": ["physical key-to-photon latency", "actual terminal-emulator rendering and glyph/IME behavior",
                             "Windows pseudoconsole behavior", "network-traffic capture", "100,000-row history query latency",
                             "warm reducer allocations and in-memory-renderer CPU (separate Rust harnesses)",
                             "verified cold-cache state unless separately established by operator"],
              "acceptance_policy": "Measurements are evidence from the stated host. Controlled baselines and actual terminal tests remain separate gates."}
    if args.baseline:
        report["baseline_comparison"] = compare_baseline(report, args.baseline)
    encoded = json.dumps(report, indent=2, allow_nan=False) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded)
        print(f"benchmark report: {args.output}", file=sys.stderr)
    else:
        print(encoded, end="")
    return 1 if any(record["status"] in {"error", "failed"} for record in measurements) else 0


if __name__ == "__main__":
    raise SystemExit(main())
