#!/usr/bin/env python3
"""Verify AO2 bounded discovery against the pinned Architecture contracts."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ARCHITECTURE_SHA = "b8c64860003238ab45fe7c76d7e8950f80a4043b"
HEAD_SHA = "1111111111111111111111111111111111111111"
RUN_ID = "repair-run-20260728"
COMPLETED_AT = "2026-07-28T00:00:00Z"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--architecture-root", type=Path, required=True)
    parser.add_argument("--ao2", type=Path, required=True)
    return parser.parse_args()


def checked_out_architecture_root(path: Path) -> Path:
    root = path.resolve()
    head = subprocess.check_output(
        ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
    ).strip()
    if head != ARCHITECTURE_SHA:
        raise RuntimeError(
            f"Architecture checkout must be {ARCHITECTURE_SHA}, received {head}"
        )
    return root


def architecture_validator(root: Path) -> Any:
    path = root / "scripts" / "github_issue_autonomous_contracts.py"
    spec = importlib.util.spec_from_file_location("ao_architecture_contracts", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot import Architecture validator: {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def fixture_issue(number: int, updated_at: str) -> dict[str, Any]:
    return {
        "number": number,
        "state": "open",
        "updated_at": updated_at,
        "title": f"Sanitized issue {number}",
        "body": "Sanitized Architecture oracle fixture.",
        "labels": ["bug"],
        "classification": "bug",
        "reported_head_sha": HEAD_SHA,
        "fix_present_at_head": False,
        "environment_accessible": True,
        "security_sensitive": False,
        "target_in_repository": True,
        "no_existing_fix": True,
        "public_reproduction_feasible": True,
        "deterministic_local_reproduction": True,
        "expected_behavior_source": "tests",
        "bounded_policy_compatible": True,
    }


def canonical_digest(value: dict[str, Any]) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def candidate_document(
    discovery: dict[str, Any], issue_number: int, rank: int
) -> dict[str, Any]:
    issue = next(item for item in discovery["issues"] if item["number"] == issue_number)
    evidence_digests = sorted([issue["content_digest"], discovery["response_digests"][0]])
    candidate = {
        "schema": "ao.architecture.autonomous-issue-repair.candidate-decision.v1",
        "run_id": RUN_ID,
        "repository": "uesugitorachiyo/ao2",
        "base_sha": HEAD_SHA,
        "issue_number": issue_number,
        "rank": rank,
        "decision": "selected" if rank == 1 else "eligible",
        "eligibility": {
            "open_bug": True,
            "target_in_repository": True,
            "no_existing_fix": True,
            "current_head_unfixed": True,
            "security_sensitive": False,
            "public_reproduction_feasible": True,
            "deterministic_local_reproduction": True,
            "expected_behavior_grounded": True,
            "bounded_policy_compatible": True,
        },
        "reason_codes": ["selected_rank_1" if rank == 1 else f"eligible_rank_{rank}"],
        "evidence_digests": evidence_digests,
        "expected_behavior_source": "tests",
        "decided_at": COMPLETED_AT,
    }
    candidate["decision_digest"] = canonical_digest(candidate)
    return candidate


def ao2_binary(path: Path) -> Path:
    candidate = path.resolve()
    if os.name == "nt" and candidate.suffix.lower() != ".exe":
        candidate = candidate.with_suffix(".exe")
    if not candidate.is_file():
        raise RuntimeError(f"AO2 binary does not exist: {candidate}")
    return candidate


def main() -> int:
    args = parse_args()
    architecture_root = checked_out_architecture_root(args.architecture_root)
    validator = architecture_validator(architecture_root)
    envelope = {
        "repository": "uesugitorachiyo/ao2",
        "default_branch": "main",
        "head_sha": HEAD_SHA,
        "pages": [
            {
                "page": 1,
                "issues": [
                    fixture_issue(7, "2026-07-27T23:00:00Z"),
                    fixture_issue(3, "2026-07-27T22:00:00Z"),
                ],
            }
        ],
    }
    with tempfile.TemporaryDirectory(prefix="ao2-discovery-oracle-") as directory:
        envelope_path = Path(directory) / "envelope.json"
        envelope_path.write_text(json.dumps(envelope), encoding="utf-8")
        result = subprocess.run(
            [
                str(ao2_binary(args.ao2)),
                "issue",
                "discover",
                "--page-envelope",
                str(envelope_path),
                "--url",
                "https://github.com/uesugitorachiyo/ao2/issues",
                "--repository",
                "uesugitorachiyo/ao2",
                "--default-branch",
                "main",
                "--head-sha",
                HEAD_SHA,
                "--run-id",
                RUN_ID,
                "--completed-at",
                COMPLETED_AT,
                "--json",
            ],
            check=True,
            text=True,
            capture_output=True,
        )
    discovery = json.loads(result.stdout)
    discovery_errors = validator.validate_contract_instance(
        "bounded_discovery_result", discovery, root=architecture_root
    )
    if discovery_errors:
        raise RuntimeError(f"Architecture discovery validation failed: {discovery_errors}")
    for rank, issue_number in enumerate([7, 3], start=1):
        candidate = candidate_document(discovery, issue_number, rank)
        candidate_errors = validator.validate_contract_instance(
            "candidate_decision", candidate, root=architecture_root
        )
        if candidate_errors:
            raise RuntimeError(
                f"Architecture candidate validation failed for {issue_number}: {candidate_errors}"
            )
        link_errors = validator.validate_discovery_candidate_link(discovery, candidate)
        if link_errors:
            raise RuntimeError(
                f"Architecture discovery/candidate link failed for {issue_number}: {link_errors}"
            )
    print("AO2 bounded discovery Architecture oracle passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
