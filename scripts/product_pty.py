#!/usr/bin/env python3
"""Adversarial Stage C workflows in an isolated Unix PTY.

Use an explicitly built, immutable --features test-hooks binary. These checks
reuse pty_test.PtyProcess without changing the terminal regression suite.
"""
from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time
import tomllib
from typing import Any, Callable

from pty_test import (
    CheckFailure, PtyProcess, check, clean_terminal, environment_report, frame_state, latest_frame,
    one_json, pause, ready, wait_for,
)

ROOT = Path(__file__).resolve().parents[1]
CTRL_C, CTRL_N, CTRL_P, CTRL_R = b"\x03", b"\x0e", b"\x10", b"\x12"
ESC, ENTER, DOWN, UP = b"\x1b", b"\r", b"\x1b[B", b"\x1b[A"
F3, F4, F5 = b"\x1bOR", b"\x1bOS", b"\x1b[15~"
PAGE_DOWN, HOME = b"\x1b[6~", b"\x1b[H"


@dataclasses.dataclass
class Context:
    binary: Path
    directory: Path

    @property
    def config(self) -> Path:
        return self.directory / "config.toml"

    @property
    def data(self) -> Path:
        return self.directory / "data"

    def configure(self, text: str = "schema_version = 1\n") -> None:
        self.config.write_text(text, encoding="utf-8")

    def command(self, *args: str) -> list[str]:
        return [str(self.binary), "--config", str(self.config), "--data-dir", str(self.data), *args]

    def launch(self, *args: str, **kwargs: Any) -> PtyProcess:
        kwargs.setdefault("observe", True)
        return PtyProcess(self.command(*args), **kwargs)

    def disk(self) -> dict[str, Any]:
        return tomllib.loads(self.config.read_text(encoding="utf-8"))

    def cli_json(self, *args: str) -> Any:
        result = subprocess.run(self.command(*args, "--json"), capture_output=True, timeout=8, check=False)
        check(result.returncode == 0, f"noninteractive command failed: {result.stderr.decode()[:240]}")
        check(b"\x1b" not in result.stdout, "noninteractive stdout has terminal escapes")
        return json.loads(result.stdout)


def screen_has(process: PtyProcess, value: str) -> bool:
    return value.lower() in process.screen.text().lower()


def seen(process: PtyProcess, value: str, timeout: float = 3.0) -> None:
    wait_for(process, lambda: screen_has(process, value), value, timeout=timeout)


def counts(process: PtyProcess) -> dict[str, Any]:
    return latest_frame(process).get("counts", {})


def commands(process: PtyProcess) -> None:
    process.send(CTRL_P)
    seen(process, "enter select")


def query(process: PtyProcess, value: str) -> None:
    process.send(value.encode("utf-8"))
    seen(process, "> " + value)


def choose(process: PtyProcess, value: str) -> None:
    commands(process)
    query(process, value)
    process.send(ENTER)


def close_commands(process: PtyProcess) -> None:
    old_epoch = latest_frame(process)["epoch"]
    process.send(ESC)
    wait_for(process, lambda: not screen_has(process, "enter select") and not screen_has(process, "enter apply") and latest_frame(process)["epoch"] > old_epoch, "closed command palette and ordered new input epoch")


def quit_cleanly(process: PtyProcess) -> None:
    process.send(CTRL_C)
    check(process.wait() == 130, "Ctrl-C did not exit with code 130")
    clean_terminal(process)


def no_saved_results(context: Context) -> None:
    for database in context.data.glob("*.sqlite*") if context.data.exists() else []:
        if database.name.endswith(("-wal", "-shm")):
            continue
        with sqlite3.connect(database) as connection:
            table = connection.execute("SELECT name FROM sqlite_master WHERE type='table' AND name='results'").fetchone()
            if table:
                check(connection.execute("SELECT COUNT(*) FROM results").fetchone()[0] == 0, "private session persisted a result")


def case_palette_aborts_active_and_compact_selection_stays_visible(ctx: Context) -> None:
    ctx.configure()
    with ctx.launch("--text", "alpha beta gamma", "--private", cols=40, rows=10) as p:
        ready(p, True)
        p.send(b"a")
        wait_for(p, lambda: frame_state(p) == "running", "first input running")
        commands(p)
        wait_for(p, lambda: frame_state(p) == "results", "opening commands aborts the active test")
        before = counts(p)["attempts_total"]
        query(p, "theme")
        for _ in range(12):
            p.send(DOWN)
            pause(p, 0.015)
        seen(p, "> Theme high_contrast")
        check(counts(p)["attempts_total"] == before, "palette search entered scoring")
        close_commands(p)
        check(frame_state(p) == "ready", "leaving an aborted-test palette did not prepare a new test")
        check(counts(p)["attempts_total"] == 0, "new test retained palette input")
        quit_cleanly(p)


def case_theme_preview_cancel_and_field_only_persistence(ctx: Context) -> None:
    original = '# preserve this comment\nschema_version = 1\n[test]\nseconds = 30 # default duration\n[appearance]\ntheme = "dark"\n'
    ctx.configure(original)
    with ctx.launch("--time", "15", "--private", "--color", "always") as p:
        ready(p, True)
        commands(p)
        start = len(p.output)
        query(p, "theme warm")
        wait_for(p, lambda: b"48;2;34;29;27" in p.output[start:], "warm theme preview escape output")
        check(ctx.config.read_text() == original, "preview persisted before confirmation")
        start = len(p.output)
        close_commands(p)
        wait_for(p, lambda: b"48;2;23;26;30" in p.output[start:], "previous dark theme restored after cancel")
        check(ctx.config.read_text() == original, "cancel altered configuration")
        choose(p, "theme warm")
        seen(p, "Setting saved")
        disk = ctx.disk()
        check(disk["appearance"]["theme"] == "warm", "theme selection did not persist")
        check(disk["test"]["seconds"] == 30, "theme edit accidentally persisted CLI duration")
        check("# default duration" in ctx.config.read_text(), "field edit lost an unrelated comment")
        close_commands(p)
        choose(p, "configuration")
        seen(p, "test.seconds = 15")
        quit_cleanly(p)


def case_invalid_setting_stays_editable_and_does_not_start_timer(ctx: Context) -> None:
    ctx.configure('schema_version = 1\n[test]\nseconds = 30\n')
    with ctx.launch("--time", "1", "--private") as p:
        ready(p, True)
        choose(p, "test.seconds")
        seen(p, "enter apply")
        p.send(b"0")
        seen(p, "> 0")
        p.send(ENTER)
        seen(p, "expected")
        check(ctx.disk()["test"]["seconds"] == 30, "invalid value was persisted")
        pause(p, 1.1)
        check(frame_state(p) == "ready" and counts(p)["attempts_total"] == 0, "editor text armed or scored a test")
        p.send(b"\x7f2")
        p.send(ENTER)
        seen(p, "Setting saved")
        check(ctx.disk()["test"]["seconds"] == 2, "corrected setting could not be applied")
        close_commands(p)
        p.send(b"a")
        wait_for(p, lambda: frame_state(p) == "running" and counts(p)["attempts_total"] == 1, "one fresh input after editing")
        quit_cleanly(p)


def case_missing_source_apply_preserves_file_and_session(ctx: Context) -> None:
    source = ctx.directory / "source.txt"
    source.write_text("cat dog", encoding="utf-8")
    original = "schema_version = 1\n"
    ctx.configure(original)
    with ctx.launch("--file", str(source), "--private") as p:
        ready(p, True)
        choose(p, "test.file")
        seen(p, "enter apply")
        p.send(str(ctx.directory / "does-not-exist.txt").encode())
        p.send(ENTER)
        seen(p, "cannot open")
        check(ctx.config.read_text() == original, "failed source edit mutated config")
        check(p.is_alive() and p.raw_mode(), "failed source edit left the UI")
        p.send(ESC)
        seen(p, "enter select")
        close_commands(p)
        seen(p, "cat dog")
        quit_cleanly(p)


def case_config_replaced_with_invalid_text_preserved_on_apply_failure(ctx: Context) -> None:
    original = 'schema_version = 1\n[appearance]\ntheme = "dark"\n'
    broken = 'schema_version = "INVALID_CONFIG_SENTINEL"\n'
    ctx.configure(original)
    with ctx.launch("--private", cols=120, rows=40) as p:
        ready(p, True)
        commands(p)
        query(p, "theme warm")
        ctx.config.write_text(broken, encoding="utf-8")
        p.send(ENTER)
        seen(p, "invalid")
        check(ctx.config.read_text() == broken, "UI silently repaired or replaced externally broken config")
        check(p.is_alive() and p.raw_mode(), "config apply failure closed the terminal UI")
        ctx.config.write_text(original, encoding="utf-8")
        p.send(ENTER)
        seen(p, "Setting saved")
        check(ctx.disk()["appearance"]["theme"] == "warm", "apply could not recover after config repair")
        quit_cleanly(p)


def case_named_preset_applies_fields_and_save_captures_effective_values(ctx: Context) -> None:
    ctx.configure('schema_version = 1\n[test]\nseconds = 30\n[privacy]\nsave_results = false\n[presets.short.test]\nseconds = 7\n')
    with ctx.launch("--time", "15") as p:
        ready(p, True)
        choose(p, "preset short")
        seen(p, "Setting saved")
        check(ctx.disk()["test"]["seconds"] == 7, "explicit preset did not override CLI value and persist its field")
        close_commands(p)
        choose(p, "configuration")
        seen(p, "test.seconds = 7")
        p.send(ESC)
        wait_for(p, lambda: not screen_has(p, "Configuration"), "configuration panel closed")
        choose(p, "save named preset")
        seen(p, "enter apply")
        p.send(b"round_trip")
        p.send(ENTER)
        seen(p, "Named preset saved")
        disk = ctx.disk()
        check(disk["presets"]["round_trip"]["test"]["seconds"] == 7, "named preset did not capture effective settings")
        check(disk["presets"]["short"]["test"]["seconds"] == 7, "saving preset replaced another preset")
        quit_cleanly(p)


def case_shipped_presets_and_explicit_save_defaults(ctx: Context) -> None:
    ctx.configure('schema_version = 1\n[test]\nseconds = 30\n[privacy]\nsave_results = false\n')
    with ctx.launch("--time", "7") as p:
        ready(p, True)
        choose(p, "preset focused")
        seen(p, "Setting saved")
        disk = ctx.disk()
        check(disk["appearance"]["focus"] == "always", "focused preset did not enable focus")
        check(all(disk["status"][name] is False for name in ("progress", "wpm", "accuracy")), "focused preset did not hide status")
        check(disk["test"]["seconds"] == 30, "focused preset persisted unrelated CLI duration")
        close_commands(p)
        choose(p, "save current as default")
        seen(p, "Current settings saved as defaults")
        check(ctx.disk()["test"]["seconds"] == 7, "explicit save defaults did not capture CLI duration")
        close_commands(p)
        choose(p, "preset code")
        seen(p, "Setting saved")
        disk = ctx.disk()
        check((disk["test"]["mode"], disk["test"]["policy"], disk["test"]["completion"]) == ("code", "exact", "confirm"), "code preset did not select exact confirm mode")
        close_commands(p)
        choose(p, "preset default")
        seen(p, "Setting saved")
        disk = ctx.disk()
        check(disk["test"]["mode"] == "time" and disk["test"]["seconds"] == 30, "default preset did not restore default test")
        quit_cleanly(p)


def case_dynamic_binding_and_associated_command_never_arm_old_timer(ctx: Context) -> None:
    ctx.configure()
    with ctx.launch("--time", "1", "--private", "--enhanced-keyboard") as p:
        ready(p, True)
        choose(p, "workflow.bindings")
        seen(p, "enter apply")
        p.send(b'{new_sample="ctrl+n"}')
        p.send(ENTER)
        seen(p, "Setting saved")
        close_commands(p)
        epoch = latest_frame(p)["epoch"]
        # Ctrl-N with associated "q": command matching wins over authoritative text.
        p.send(b"\x1b[110;5;113u")
        wait_for(p, lambda: frame_state(p) == "ready" and latest_frame(p)["epoch"] > epoch, "new configured command epoch")
        pause(p, 1.1)
        check(frame_state(p) == "ready" and counts(p)["attempts_total"] == 0, "reader armed the timer for a configured command")
        p.send(b"a")
        wait_for(p, lambda: frame_state(p) == "running" and counts(p)["attempts_total"] == 1, "first text remains one attempt after custom binding")
        quit_cleanly(p)


def case_exact_tab_enter_survive_palette_roundtrip(ctx: Context) -> None:
    ctx.configure()
    with ctx.launch("--text", "\tcat\nz", "--code", "--private", "--once", "--json") as p:
        ready(p, True)
        commands(p)
        query(p, "theme light")
        close_commands(p)
        p.send(b"\t")
        wait_for(p, lambda: frame_state(p) == "running" and counts(p)["attempts_total"] == 1, "literal initial Tab starts exact test")
        p.send(b"cat\rz")
        wait_for(p, lambda: counts(p)["attempts_total"] == 6, "literal Tab and Enter count as target units")
        p.send(F5)
        check(p.wait() == 0, "exact confirm failed after palette roundtrip")
        result = one_json(p)
        check(result["counts"]["attempts_correct"] == 6 and result["outcome"] == "complete", "exact controls were shadowed by command handling")
        clean_terminal(p)


def case_repaired_missed_word_practice_and_original_settings_return(ctx: Context) -> None:
    ctx.configure()
    with ctx.launch("--text", "cat dog", "--difficulty", "expert", "--private") as p:
        ready(p, True)
        p.send(b"cax\x7ft dog")
        wait_for(p, lambda: frame_state(p) == "results", "repaired original result")
        p.send(F3)
        seen(p, "Practice missed")
        p.send(ENTER)
        wait_for(p, lambda: frame_state(p) == "ready" and screen_has(p, "practice"), "missed-word practice Ready")
        check("cat cat" in p.screen.text(), "repaired original cat was not selected for missed practice")
        check("cax" not in p.screen.text(), "practice generated the misspelling instead of original source")
        p.send(CTRL_R)
        wait_for(p, lambda: frame_state(p) == "ready" and screen_has(p, "cat dog"), "original sample settings restored")
        choose(p, "rules.difficulty")
        seen(p, "> expert")
        p.send(ESC)
        seen(p, "enter select")
        close_commands(p)
        p.send(b"cat dog")
        wait_for(p, lambda: frame_state(p) == "results", "returned original result")
        quit_cleanly(p)


def case_ineligible_practice_reason_visible_in_palette(ctx: Context) -> None:
    ctx.configure()
    with ctx.launch("--text", "cat", "--private", cols=120, rows=40) as p:
        ready(p, True)
        p.send(b"cat")
        wait_for(p, lambda: frame_state(p) == "results", "clean result")
        p.send(F3)
        seen(p, "Practice missed")
        p.send(ENTER)
        seen(p, "No mistaken or omitted words")
        check(frame_state(p) == "results", "ineligible practice changed scoring state")
        p.send(DOWN)
        p.send(ENTER)
        seen(p, "eight eligible correctly completed words")
        quit_cleanly(p)


def case_current_private_review_scrolls_and_keeps_export_redacted(ctx: Context) -> None:
    ctx.configure()
    with ctx.launch("--text", "cat dog fox owl yak eel ant bee", "--private", cols=80, rows=24) as p:
        ready(p, True)
        p.send(b"cax\x7ft do fog owl yak eel ant bee")
        wait_for(p, lambda: frame_state(p) == "results", "result with repaired and remaining mistakes")
        p.send(F4)
        seen(p, "mistaken attempts")
        p.send(b"\t")
        seen(p, "corrected while typing")
        seen(p, "exp dog")
        seen(p, "got do")
        p.send(PAGE_DOWN)
        seen(p, "exp ant")
        p.send(HOME)
        seen(p, "exp cat")
        p.send(ESC)
        wait_for(p, lambda: not screen_has(p, "tab summary/text"), "closed detailed review")
        quit_cleanly(p)
        check(not p.stdout, "private transient review wrote unsolicited stdout")
    no_saved_results(ctx)


def case_private_session_cannot_be_disabled_by_presets(ctx: Context) -> None:
    original = 'schema_version = 1\n[presets.persist.privacy]\nprivate_session = false\nsave_results = true\nstore_custom_text = true\nstore_event_trace = true\n[presets.save_only.privacy]\nsave_results = true\n'
    ctx.configure(original)
    with ctx.launch("--text", "private fixture", "--private", "--once", "--json", cols=120, rows=40) as p:
        ready(p, True)
        choose(p, "preset persist")
        seen(p, "Private mode stays active")
        check(ctx.config.read_text() == original, "forbidden private-mode preset partially persisted")
        close_commands(p)
        choose(p, "preset save_only")
        seen(p, "Setting saved")
        close_commands(p)
        p.send(b"private fixture")
        check(p.wait() == 0, "private result after persistence preset did not complete")
        clean_terminal(p)
        result = one_json(p)
        check(result["words"] == [], "private text appeared in final JSON")
    no_saved_results(ctx)


def database_count(ctx: Context) -> int:
    path = ctx.data / "history.sqlite3"
    if not path.exists():
        return 0
    try:
        with sqlite3.connect(f"file:{path}?mode=ro", uri=True, timeout=0.02) as connection:
            return connection.execute("SELECT COUNT(*) FROM results").fetchone()[0]
    except sqlite3.OperationalError:
        return 0


def seed_saved_result(ctx: Context, source: str) -> None:
    with ctx.launch("--text", source, "--once") as p:
        ready(p, True)
        p.send(source[:1].encode())
        pause(p, 0.03)
        p.send(source[1:].encode())
        check(p.wait() == 0, "seed result did not complete and close")
        clean_terminal(p)
    check(database_count(ctx) == 1, "normal completed result was not saved")


def case_historical_custom_review_requires_original_and_storage_is_private(ctx: Context) -> None:
    ctx.configure()
    source = "historical private fixture"
    seed_saved_result(ctx, source)
    database = ctx.data / "history.sqlite3"
    with sqlite3.connect(database) as connection:
        header = json.loads(connection.execute("SELECT header_json FROM results").fetchone()[0])
        check(header["spec"]["source_id"] == "custom", "custom source label was not neutral")
        check(source not in json.dumps(header), "default custom history retained source text")
        for table in ("word_summaries", "private_content", "event_traces"):
            check(connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0] == 0, f"default private content appeared in {table}")
    with ctx.launch("--text", source) as p:
        ready(p, True)
        choose(p, "history")
        seen(p, "History")
        seen(p, "matching profile")
        seen(p, "1 runs")
        p.send(ENTER)
        seen(p, "tab summary/text")
        p.send(b"\t")
        seen(p, "Original text was not retained")
        check(source not in p.screen.text(), "historical review pretended private source was retained")
        wrong = ctx.directory / "wrong-original.txt"
        original = ctx.directory / "matching-original.txt"
        wrong.write_text("wrong original fixture", encoding="utf-8")
        original.write_text(source, encoding="utf-8")
        p.send(b"o")
        seen(p, "enter apply")
        p.send(str(wrong).encode())
        p.send(ENTER)
        seen(p, "hash")
        check(source not in p.screen.text(), "mismatched original exposed unverified target")
        # Cancel this editor completely, then provide the actual matching file.
        p.send(ESC)
        seen(p, "enter select")
        p.send(ESC)
        seen(p, "Original text was not retained")
        p.send(b"o")
        seen(p, "enter apply")
        p.send(str(original).encode())
        p.send(ENTER)
        seen(p, "Original hash verified")
        seen(p, source)
        check(not screen_has(p, "got " + source), "aggregate counts were used to invent a historical input transcript")
        quit_cleanly(p)
    with sqlite3.connect(database) as connection:
        check(connection.execute("SELECT COUNT(*) FROM private_content").fetchone()[0] == 0, "attaching an original file silently persisted private text")


def case_history_filters_and_enhanced_original_shortcut(ctx: Context) -> None:
    ctx.configure()
    source = "history filter fixture"
    seed_saved_result(ctx, source)
    original = ctx.directory / "matching.txt"
    original.write_text(source, encoding="utf-8")
    with ctx.launch("--text", source, "--enhanced-keyboard", cols=120, rows=40) as p:
        ready(p, True)
        choose(p, "history")
        seen(p, "1 runs")
        p.send(b"\x1b[102;1;102u")  # associated-text 'f' opens filters
        seen(p, "enter apply")
        p.send(b'{profile="all",mode="custom",language="custom",outcome="complete",from="1970-01-01",to="2999-12-31"}')
        p.send(ENTER)
        seen(p, "History")
        seen(p, "all profiles")
        seen(p, "1 runs")
        p.send(b"f")
        seen(p, "enter apply")
        p.send(b'{profile="all",outcome="failed"}')
        p.send(ENTER)
        seen(p, "No results match this filter")
        seen(p, "0 runs")
        p.send(b"f")
        seen(p, "enter apply")
        p.send(b'{profile="all",from="2099-01-01"}')
        p.send(ENTER)
        seen(p, "No results match this filter")
        p.send(b"f")
        seen(p, "enter apply")
        p.send(b'{profile="all",mode="words"}')
        p.send(ENTER)
        seen(p, "No results match this filter")
        p.send(b"f")
        seen(p, "enter apply")
        p.send(b'{profile="current"}')
        p.send(ENTER)
        seen(p, "matching profile")
        seen(p, "1 runs")
        p.send(ENTER)
        seen(p, "tab summary/text")
        p.send(b"\t")
        seen(p, "Original text was not retained")
        p.send(b"\x1b[111;1;111u")  # associated-text 'o' must reach review command
        seen(p, "enter apply")
        p.send(str(original).encode())
        p.send(ENTER)
        seen(p, "Original hash verified")
        seen(p, source)
        quit_cleanly(p)


def case_locked_save_recovery_export_retry_is_exactly_once(ctx: Context) -> None:
    ctx.configure()
    seed_saved_result(ctx, "seed fixture")
    database = ctx.data / "history.sqlite3"
    recovery = ctx.directory / "recovery.jsonl"
    with sqlite3.connect(database, timeout=0.25) as lock:
        lock.execute("BEGIN IMMEDIATE")
        with ctx.launch("--text", "unsaved fixture", cols=120, rows=40) as p:
            ready(p, True)
            p.send(b"u")
            pause(p, 0.03)
            p.send(b"nsaved fixture")
            wait_for(p, lambda: frame_state(p) == "results", "completed result while SQLite is locked")
            seen(p, "unsaved", timeout=5)
            check(not screen_has(p, "Personal best"), "app announced a personal best before the blocked indexed save/comparison completed")
            check(database_count(ctx) == 1, "failed transaction partially inserted a result")
            choose(p, "export current")
            seen(p, "enter apply")
            p.send(str(recovery).encode())
            p.send(ENTER)
            wait_for(p, recovery.exists, "pending-result recovery export")
            seen(p, "enter select")
            exported = [json.loads(line) for line in recovery.read_text().splitlines() if line.strip()]
            check(len(exported) == 1, "recovery export duplicated current pending result")
            check(exported[0]["snapshot"]["counts"]["attempts_total"] == len("unsaved fixture"), "recovery export lost immutable scored counts")
            check("unsaved fixture" not in recovery.read_text(), "default recovery export leaked private custom text")
            original = recovery.read_bytes()
            close_commands(p)
            choose(p, "export current")
            seen(p, "enter apply")
            p.send(str(recovery).encode())
            p.send(ENTER)
            seen(p, "exist")
            check(recovery.read_bytes() == original, "recovery export overwrote an existing file")
            p.send(ESC)
            seen(p, "enter select")
            close_commands(p)
            lock.rollback()
            choose(p, "retry unsaved")
            wait_for(p, lambda: database_count(ctx) == 2, "retained result retry commit", timeout=5)
            seen(p, "enter select")
            close_commands(p)
            choose(p, "retry unsaved")
            pause(p, 0.3)
            check(database_count(ctx) == 2, "idempotent retry duplicated a committed result")
            quit_cleanly(p)
    with sqlite3.connect(database) as connection:
        count, ids = connection.execute("SELECT COUNT(*),COUNT(DISTINCT id) FROM results").fetchone()
        check((count, ids) == (2, 2), "result IDs were not committed exactly once")
        check(connection.execute("SELECT SUM(result_count) FROM profile_stats").fetchone()[0] == 2, "retry double-counted aggregate statistics")


def case_history_classification_filters_cover_overlapping_practice_flags(ctx: Context) -> None:
    ctx.configure()
    seed_saved_result(ctx, "cat")
    for kind in ("repeat", "paste", "assisted", "failed"):
        args = ["--text", "cat", "--once", "--json"]
        if kind == "assisted":
            args = ["--code", "--auto-indent", "--text", "a\n  b", "--once", "--json"]
        elif kind == "failed":
            args += ["--difficulty", "master"]
        with ctx.launch(*args) as p:
            ready(p, True)
            if kind == "repeat":
                epoch = latest_frame(p)["epoch"]
                p.send(b"\x1b[12~")
                wait_for(p, lambda: frame_state(p) == "ready" and latest_frame(p)["epoch"] > epoch,
                         "same-sample repeat before classified result")
            p.send(b"a" if kind == "assisted" else b"x" if kind == "failed" else b"c")
            if kind != "failed":
                pause(p, 0.03)
                if kind == "paste":
                    p.send(b"\x1b[200~ignored paste\x1b[201~")
                    pause(p, 0.02)
                p.send(b"\rb" if kind == "assisted" else b"at")
                if kind == "assisted":
                    pause(p, 0.02)
                    p.send(F5)
            check(p.wait() == 0, "classification fixture failed to finish: " + kind)
            result = one_json(p)
            check(result["outcome"] == ("failed" if kind == "failed" else "complete"),
                  "classification fixture produced the wrong outcome: " + kind)
            clean_terminal(p)
    check(database_count(ctx) == 5, "classification fixture did not persist five distinct results")
    with ctx.launch("--text", "cat", cols=120, rows=40) as p:
        ready(p, True)
        choose(p, "history")
        seen(p, "History")
        # Arrow-select the exposed table examples; no advanced table typing is
        # required to discover the standard/practice/paste/assisted filters.
        for steps, label, count in [(2, "standard", 1), (3, "practice", 3),
                                    (4, "paste attempted", 1), (5, "assisted code", 1)]:
            p.send(b"f")
            seen(p, "enter apply")
            for _ in range(steps):
                p.send(DOWN)
                pause(p, 0.01)
            p.send(ENTER)
            wait_for(p, lambda: label in "".join(p.screen.grid[3]).lower()
                     and screen_has(p, f"{count} runs"), "active classification and aggregate count: " + label)
        for raw, count in [('{profile="all",classification="practice",outcome="failed"}', 0),
                           ('{profile="all",outcome="failed"}', 1),
                           ('{profile="all",classification="practice",outcome="complete"}', 3)]:
            p.send(b"f")
            seen(p, "enter apply")
            p.send(raw.encode())
            p.send(ENTER)
            seen(p, f"{count} runs")
            if count == 0:
                seen(p, "No results match this filter")
        check(counts(p)["attempts_total"] == 0, "filter navigation entered typing scores")
        quit_cleanly(p)
    check(database_count(ctx) == 5, "history classification queries wrote new results")


def case_practice_return_preserves_later_private_and_theme_edits(ctx: Context) -> None:
    ctx.configure('schema_version = 1\n[appearance]\ntheme = "dark"\n')
    with ctx.launch("--text", "cat dog") as p:
        ready(p, True)
        p.send(b"cax\x7ft dog")
        wait_for(p, lambda: frame_state(p) == "results", "original result before private mode")
        wait_for(p, lambda: database_count(ctx) == 1, "pre-private result saved", timeout=5)
        p.send(F3)
        seen(p, "Practice missed")
        p.send(ENTER)
        wait_for(p, lambda: frame_state(p) == "ready" and screen_has(p, "practice"), "practice Ready")
        choose(p, "privacy.private_session")
        seen(p, "enter apply")
        p.send(b"true")
        p.send(ENTER)
        seen(p, "Setting saved")
        close_commands(p)
        choose(p, "theme warm")
        seen(p, "Setting saved")
        close_commands(p)
        p.send(CTRL_R)
        wait_for(p, lambda: frame_state(p) == "ready" and screen_has(p, "cat dog"), "normal sample after practice")
        choose(p, "privacy.private_session")
        seen(p, "> true")
        p.send(ESC)
        seen(p, "enter select")
        close_commands(p)
        choose(p, "appearance.theme")
        seen(p, "> warm")
        p.send(ESC)
        seen(p, "enter select")
        close_commands(p)
        p.send(b"cat dog")
        wait_for(p, lambda: frame_state(p) == "results", "normal result remains private after practice")
        quit_cleanly(p)
    check(database_count(ctx) == 1, "returning from practice re-enabled result persistence after private mode was locked")


def case_personal_best_pace_uses_matching_saved_record_and_is_practice(ctx: Context) -> None:
    ctx.configure()
    source = "cat"
    with ctx.launch("--text", source, "--once") as p:
        ready(p, True)
        p.send(b"c")
        pause(p, 0.2)
        p.send(b"at")
        check(p.wait() == 0, "matching personal-best seed result failed")
        clean_terminal(p)
    check(database_count(ctx) == 1, "personal-best seed was not saved")
    with ctx.launch("--text", source, "--pace", "personal_best", "--once", "--json") as p:
        ready(p, True)
        seen(p, "Personal-best pace")
        p.send(b"c")
        pause(p, 0.03)
        p.send(b"at")
        check(p.wait() == 0, "personal-best paced run failed")
        result = one_json(p)
        check(1 <= result["spec"]["pace_wpm"] <= 1000, "indexed matching best did not resolve to a numeric pace")
        check(result["personal_best_eligible"] is False, "pace-assisted run entered the standard record category")
        check(result["counts"]["attempts_total"] == 3, "pace changed manual attempt accounting")
        clean_terminal(p)
    with sqlite3.connect(ctx.data / "history.sqlite3") as connection:
        rows = connection.execute("SELECT eligible,profile_key FROM results ORDER BY created_at_utc_ms").fetchall()
        check(len(rows) == 2 and rows[0][0] == 1 and rows[1][0] == 0, "paced history eligibility did not separate standard/practice results")
        check(rows[0][1] != rows[1][1], "paced and unassisted history shared a profile")


def case_unsaved_snapshots_survive_new_results_and_quit_reports_all(ctx: Context) -> None:
    ctx.configure()
    seed_saved_result(ctx, "acknowledged seed")
    recovery = ctx.directory / "all-pending.jsonl"
    with sqlite3.connect(ctx.data / "history.sqlite3", timeout=0.25) as lock:
        lock.execute("BEGIN IMMEDIATE")
        with ctx.launch("--text", "queued", cols=120, rows=40) as p:
            ready(p, True)
            for index in range(2):
                p.send(b"q")
                pause(p, 0.03)
                p.send(b"ueued")
                wait_for(p, lambda: frame_state(p) == "results", f"queued result {index + 1}")
                seen(p, "unsaved", timeout=5)
                if index == 0:
                    p.send(CTRL_R)
                    wait_for(p, lambda: frame_state(p) == "ready", "new sample while earlier result remains unsaved")
            choose(p, "export current")
            seen(p, "enter apply")
            p.send(str(recovery).encode())
            p.send(ENTER)
            seen(p, "Exported 2 retained results")
            records = [json.loads(line) for line in recovery.read_text().splitlines() if line.strip()]
            check(len(records) == 2 and len({record["id"] for record in records}) == 2, "moving to another result lost or duplicated a pending immutable snapshot")
            check(all(record["snapshot"]["counts"]["attempts_total"] == 6 for record in records), "pending result counts changed across sample epochs")
            quit_cleanly(p)
            check(b"2 result(s) remain unsaved" in p.stderr, "quit did not report the full pending set after terminal restoration")
            check(b"\x1b" not in p.stderr and not p.stdout, "unsaved exit diagnostic contaminated reserved streams")
    check(database_count(ctx) == 1, "unacknowledged saves damaged the already acknowledged result")


def case_explicit_retained_trace_reconstructs_verified_historical_diff(ctx: Context) -> None:
    ctx.configure('schema_version = 1\n[privacy]\nstore_event_trace = true\n')
    source = "cat dog"
    with ctx.launch("--text", source, "--once") as p:
        ready(p, True)
        p.send(b"cax")
        pause(p, 0.03)
        p.send(b"\x7ft dog")
        check(p.wait() == 0, "opted-in trace result failed")
        clean_terminal(p)
    original = ctx.directory / "trace-original.txt"
    original.write_text(source, encoding="utf-8")
    with sqlite3.connect(ctx.data / "history.sqlite3") as connection:
        check(connection.execute("SELECT COUNT(*) FROM event_traces").fetchone()[0] == 1, "explicit diagnostic trace was not retained")
        check(connection.execute("SELECT COUNT(*) FROM private_content").fetchone()[0] == 0, "trace opt-in silently retained the full custom source")
    with ctx.launch("--text", source, cols=120, rows=40) as p:
        ready(p, True)
        choose(p, "history")
        seen(p, "1 runs")
        p.send(ENTER)
        seen(p, "tab summary/text")
        p.send(b"\t")
        seen(p, "Original text was not retained")
        p.send(b"o")
        seen(p, "enter apply")
        p.send(str(original).encode())
        p.send(ENTER)
        seen(p, "Original hash verified")
        seen(p, "corrected while typing")
        seen(p, "exp cat")
        seen(p, "got cat")
        seen(p, "exp dog")
        seen(p, "got dog")
        quit_cleanly(p)


def case_result_details_and_slow_selection_preferences_drive_workflow(ctx: Context) -> None:
    ctx.configure('schema_version = 1\n[workflow]\nresult_details = true\n[practice]\nselection = "slow"\n')
    source = "cat dog fox owl yak eel ant bee cow"
    with ctx.launch("--text", source, "--private", cols=120, rows=40) as p:
        ready(p, True)
        for index, word in enumerate(source.split()):
            p.send(word[:1].encode())
            pause(p, 0.02 + index * 0.002)
            p.send((word[1:] + (" " if index < 8 else "")).encode())
        seen(p, "tab summary/text")
        seen(p, "mistaken attempts")
        check(frame_state(p) == "results", "automatic result details changed engine completion")
        p.send(ESC)
        wait_for(p, lambda: not screen_has(p, "tab summary/text"), "closed automatically opened details")
        p.send(F3)
        seen(p, "> Practice slow")
        p.send(ENTER)
        wait_for(p, lambda: frame_state(p) == "ready" and screen_has(p, "practice"), "configured slow-word practice")
        check(counts(p)["attempts_total"] == 0, "opening selected slow practice carried prior input")
        p.send(CTRL_R)
        wait_for(p, lambda: frame_state(p) == "ready" and screen_has(p, source), "original settings after selected slow practice")
        quit_cleanly(p)
    no_saved_results(ctx)


def case_private_session_suppresses_explicit_text_and_trace_opt_ins(ctx: Context) -> None:
    ctx.configure('schema_version = 1\n[privacy]\nsave_results = true\nstore_custom_text = true\nstore_event_trace = true\n')
    source = "private opt-in fixture"
    with ctx.launch("--text", source, "--private", "--once", "--json") as p:
        ready(p, True)
        p.send(b"p")
        pause(p, 0.03)
        p.send(source[1:].encode())
        check(p.wait() == 0, "private run with explicit persistent defaults failed")
        result = one_json(p)
        check(result["words"] == [], "explicit saved-text preference escaped the session privacy lock")
        check(source.encode() not in p.stdout and source.encode() not in p.stderr, "private target appeared in exported result or diagnostics")
        clean_terminal(p)
    no_saved_results(ctx)
    if (ctx.data / "history.sqlite3").exists():
        with sqlite3.connect(ctx.data / "history.sqlite3") as connection:
            for table in ("word_summaries", "private_content", "event_traces"):
                check(connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0] == 0, f"private session retained {table}")


def case_explicit_doctor_probe_reports_detected_flags_and_restores_terminal(ctx: Context) -> None:
    ctx.configure()
    with ctx.launch("doctor", "--json", observe=False) as p:
        check(p.wait() == 0, "passive doctor failed")
        passive = json.loads(p.stdout)
        check(passive["capabilities"]["active_probe"] is None, "ordinary doctor performed an implicit active probe")
        check(not p.output and not p.screen.changed_modes, "ordinary doctor wrote terminal queries or changed modes")
        check(b"\x1b" not in p.stdout, "passive doctor JSON contains escape sequences")
        clean_terminal(p)
    for kind in ("supported", "unsupported", "timeout", "interrupted"):
        with ctx.launch("doctor", "--probe", "--json", observe=False) as p:
            wait_for(p, lambda: b"\x1b[?u\x1b[c" in p.output, "explicit doctor capability query")
            if kind == "supported":
                p.send(b"\x1b[?31u\x1b[?1;2c")
            elif kind == "unsupported":
                p.send(b"\x1b[?1;2c")
            elif kind == "interrupted":
                p.send(CTRL_C)
            check(p.wait(timeout=3) == (130 if kind == "interrupted" else 0), "doctor probe returned incorrect exit status for " + kind)
            check(b"\x1b" not in p.stdout and not p.stderr, "doctor probe contaminated report or error streams")
            report = json.loads(p.stdout)
            active = report["capabilities"]["active_probe"]
            check(active["status"] == kind, "doctor probe did not report actual response state")
            check(active["elapsed_ms"] < 1000, "doctor probe exceeded its bounded response budget")
            check(active["cursor_style"] is None, "doctor invented an unqueried cursor style")
            expected = True if kind == "supported" else (False if kind == "unsupported" else None)
            check(report["capabilities"]["keyboard_enhancement_support"] is expected, "doctor conflated absent evidence with unsupported capability")
            if kind == "supported":
                check(active["keyboard_enhancement_flags"] == 31, "doctor did not parse full decimal protocol flags")
            if kind == "timeout":
                check(active["elapsed_ms"] >= 350, "doctor did not wait the documented response budget")
            clean_terminal(p)


def case_overload_verdict_survives_storage_ack_and_review(ctx: Context) -> None:
    ctx.configure()
    with ctx.launch("--text", "cat", cols=120, rows=40,
                    env={"CLACK_TEST_FAULT": "wait_overload_after_completion"}) as p:
        ready(p, True)
        p.send(b"cat")
        wait_for(p, lambda: any(event.get("event") == "completion_pending" for event in p.observations),
                 "provisional completion before reader epoch closure")
        for offset in range(0, 5000, 64):
            p.send(b"x" * min(64, 5000 - offset))
            pause(p, 0.002)
        wait_for(p, lambda: database_count(ctx) == 1, "interrupted result storage commit", timeout=5)
        wait_for(p, lambda: frame_state(p) == "results" and not screen_has(p, "saving"),
                 "save acknowledgement consumed by the result view")
        # A normal save ACK clears transient Saving/Unsaved notices. The
        # persistent interruption label must come from the effective verdict,
        # while the engine's immutable completed counts stay unchanged.
        pause(p, 0.05)
        seen(p, "interrupted")
        seen(p, "input queue overload")
        check(not screen_has(p, "Personal best"), "interrupted result announced a personal best")
        check(counts(p)["attempts_total"] == 3, "post-completion input changed immutable counts")
        with sqlite3.connect(ctx.data / "history.sqlite3") as connection:
            outcome, eligible, header = connection.execute("SELECT outcome,eligible,header_json FROM results").fetchone()
            check(outcome == "interrupted" and eligible == 0, "persisted result lost closing-epoch integrity verdict")
            saved = json.loads(header)
            check(saved["integrity"]["input_overload"], "persisted integrity flag was lost")
            check(connection.execute("SELECT COUNT(*) FROM profile_bests").fetchone()[0] == 0,
                  "interrupted result entered the personal-best index")
        p.send(F4)
        seen(p, "interrupted")
        seen(p, "input queue overload")
        quit_cleanly(p)


CASES: dict[str, Callable[[Context], None]] = {
    "palette_compact_abort": case_palette_aborts_active_and_compact_selection_stays_visible,
    "theme_preview_field_persistence": case_theme_preview_cancel_and_field_only_persistence,
    "invalid_setting_recovery": case_invalid_setting_stays_editable_and_does_not_start_timer,
    "missing_source_apply": case_missing_source_apply_preserves_file_and_session,
    "external_config_error_recovery": case_config_replaced_with_invalid_text_preserved_on_apply_failure,
    "named_presets": case_named_preset_applies_fields_and_save_captures_effective_values,
    "shipped_presets_save_defaults": case_shipped_presets_and_explicit_save_defaults,
    "dynamic_binding_reader_timer": case_dynamic_binding_and_associated_command_never_arm_old_timer,
    "exact_controls_palette": case_exact_tab_enter_survive_palette_roundtrip,
    "missed_practice_return": case_repaired_missed_word_practice_and_original_settings_return,
    "practice_ineligible_reason": case_ineligible_practice_reason_visible_in_palette,
    "current_private_review": case_current_private_review_scrolls_and_keeps_export_redacted,
    "private_preset_lock": case_private_session_cannot_be_disabled_by_presets,
    "historical_custom_privacy": case_historical_custom_review_requires_original_and_storage_is_private,
    "history_filters_enhanced_original": case_history_filters_and_enhanced_original_shortcut,
    "storage_recovery_retry": case_locked_save_recovery_export_retry_is_exactly_once,
    "history_classification_filters": case_history_classification_filters_cover_overlapping_practice_flags,
    "practice_later_privacy_theme": case_practice_return_preserves_later_private_and_theme_edits,
    "personal_best_pace": case_personal_best_pace_uses_matching_saved_record_and_is_practice,
    "unsaved_across_results_and_quit": case_unsaved_snapshots_survive_new_results_and_quit_reports_all,
    "historical_explicit_trace_diff": case_explicit_retained_trace_reconstructs_verified_historical_diff,
    "result_details_slow_selection": case_result_details_and_slow_selection_preferences_drive_workflow,
    "private_trace_opt_in_suppression": case_private_session_suppresses_explicit_text_and_trace_opt_ins,
    "doctor_explicit_probe": case_explicit_doctor_probe_reports_detected_flags_and_restores_terminal,
    "overload_verdict_after_storage_ack": case_overload_verdict_survives_storage_ack_and_review,
}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True, help="immutable explicitly instrumented binary")
    parser.add_argument("--case", action="append", choices=sorted(CASES))
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--fail-fast", action="store_true")
    parser.add_argument("--execution-label", default="unspecified", help="record native/translated execution separately from the harness host architecture")
    args = parser.parse_args()
    if args.list:
        print("\n".join(CASES))
        return 0
    binary = args.binary.resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        parser.error(f"executable not found: {binary}")
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    results = []
    for name in args.case or CASES:
        started = time.monotonic()
        try:
            with tempfile.TemporaryDirectory(prefix="clack-product-pty-") as directory:
                CASES[name](Context(binary, Path(directory)))
            status, detail = "pass", "assertions satisfied"
        except (CheckFailure, OSError, TimeoutError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
            status, detail = "fail", str(error)
        results.append({"case": name, "status": status, "seconds": round(time.monotonic() - started, 6), "detail": detail})
        print(f"{status.upper():5} {name}: {detail}", flush=True)
        if status == "fail" and args.fail_fast:
            break
    check(hashlib.sha256(binary.read_bytes()).hexdigest() == binary_hash, "test binary changed during verification")
    report = {
        "report_version": 1,
        "kind": "stage_c_product_unix_pty",
        "binary": str(binary),
        "binary_sha256": binary_hash,
        "execution_label": args.execution_label,
        "test_hooks": True,
        "platform": sys.platform,
        "python": sys.version.split()[0],
        "environment": environment_report(),
        "created_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "terminal_policy": "synthetic xterm-256color PTY; isolated config/data for each case",
        "limitations": [
            "No actual emulator, physical display, font/IME, native Windows, tmux, or SSH validation is claimed.",
            "Observer frames are test instrumentation, not production performance measurements.",
        ],
        "results": results,
    }
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return int(any(row["status"] != "pass" for row in results))


if __name__ == "__main__":
    raise SystemExit(main())
