#!/usr/bin/env python3
"""Validate the stable specification ledger; this never certifies behavior by itself."""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent
# The original specification was updated only to name the executable clack.
SPEC_SHA256 = "f5abb810aed5c4db6daec34e3f1df6f6d18215601454a3932203ef64fd3d1273"
ROW = re.compile(r"^\| ([A-Z]+-(?:[0-9]{3}|[A-Z]+)) \|")


def rows(path: Path) -> list[list[str]]:
    return [
        [cell.strip() for cell in line.strip().strip("|").split("|")]
        for line in path.read_text(encoding="utf-8").splitlines()
        if ROW.match(line)
    ]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--require-local-complete", action="store_true")
    args = parser.parse_args()
    ledger_path = ROOT / "docs/coverage.md"
    ledger = rows(ledger_path)
    ids = [row[0] for row in ledger]
    expected = {
        f"{prefix}-{number:03}"
        for prefix, count in {
            "P": 10, "UI": 30, "KEY": 11, "MODE": 13, "DATA": 15,
            "ENG": 13, "RULE": 9, "TIME": 6, "INPUT": 4, "SCORE": 15,
            "SAMPLE": 4, "UNI": 6, "CFG": 18, "PRACT": 5, "PACE": 2,
            "REVIEW": 3, "REC": 9, "ARCH": 24, "PERF": 21,
            "STORE": 13, "PRIV": 7, "CLI": 14, "TERM": 12,
            "AT": 21, "TEST": 5, "CI": 6, "SHIP": 7, "DOD": 4,
        }.items()
        for number in range(1, count + 1)
    } | {"GATE-A", "GATE-B", "GATE-C", "GATE-D", "GATE-FINAL"}
    errors = []
    if set(ids) != expected or len(ids) != len(expected):
        errors.append({"missing": sorted(expected - set(ids)),
                       "unexpected": sorted(set(ids) - expected),
                       "duplicates": sorted(key for key, count in Counter(ids).items() if count > 1)})
    actual_spec_hash = hashlib.sha256((ROOT / "SPEC.md").read_bytes()).hexdigest()
    if actual_spec_hash != SPEC_SHA256:
        errors.append("Specification bytes differ from the audited source")
    audited = {}
    for path in sorted((ROOT / "docs").glob("audit-*-final.md")):
        for entry in rows(path):
            audited.setdefault(entry[0], []).append(str(path.relative_to(ROOT)))
    pending_local = []
    pending_external = []
    entries = []
    for row in ledger:
        if len(row) != 6:
            errors.append(f"{row[0]}: expected six ledger columns")
            continue
        key, requirement, stage, implementation, validation, evidence = row
        if implementation not in {"Done", "Partial", "Pending", "N/A with rationale"}:
            errors.append(f"{key}: invalid implementation status")
        if validation not in {"Pass", "Fail", "Pending local", "Pending external", "N/A with rationale"}:
            errors.append(f"{key}: invalid validation status")
        if not requirement or not evidence or evidence == "—" or stage not in {"A", "B", "C", "D"}:
            errors.append(f"{key}: missing required description, evidence or stage")
        if implementation in {"Pending", "Partial"} and validation == "Pass":
            errors.append(f"{key}: a partial/pending implementation cannot be marked passed")
        if key not in audited:
            errors.append(f"{key}: absent from all final individual audits")
        if validation in {"Pending local", "Fail"} or implementation == "Pending":
            pending_local.append(key)
        if validation == "Pending external":
            pending_external.append(key)
        entries.append({"id": key, "stage": stage, "implementation": implementation,
                        "validation": validation, "audits": audited.get(key, [])})
    if args.require_local_complete and pending_local:
        errors.append("Required local validation or implementation remains")
    result = {
        "schema_version": 1,
        "scope": "Ledger structure and individual audit coverage only; behavioral evidence remains in the referenced reports.",
        "specification_sha256": actual_spec_hash,
        "ledger_sha256": hashlib.sha256(ledger_path.read_bytes()).hexdigest(),
        "required_count": len(expected), "observed_count": len(ids),
        "implementation_counts": dict(Counter(row[3] for row in ledger if len(row) == 6)),
        "validation_counts": dict(Counter(row[4] for row in ledger if len(row) == 6)),
        "pending_local": pending_local, "pending_external": pending_external,
        "errors": errors, "requirements": entries,
    }
    encoded = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
        print(json.dumps({key: value for key, value in result.items() if key != "requirements"}, indent=2))
    else:
        print(encoded, end="")
    raise SystemExit(1 if errors else 0)


if __name__ == "__main__":
    main()
