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
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any


ARCHITECTURE_SHA = "8e6f247b800b60c520b4e967f7553974a20ec2f8"
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


def bind_digest(value: dict[str, Any], field: str) -> dict[str, Any]:
    value.pop(field, None)
    value[field] = canonical_digest(value)
    return value


def publication_authority(root: Path, now: datetime) -> dict[str, Any]:
    fixture_root = root / "stack" / "fixtures" / "github-issue-repair" / "v1"

    def load(name: str) -> dict[str, Any]:
        return json.loads((fixture_root / name).read_text())

    envelope = load("immutable-run-envelope.valid.json")
    candidate = load("candidate-decision.valid.json")
    governance = load("governance-decision.valid.json")
    reviewer = load("reviewer-independence.valid.json")
    envelope["created_at"] = (now - timedelta(minutes=20)).isoformat().replace(
        "+00:00", "Z"
    )
    envelope["expires_at"] = (now + timedelta(minutes=60)).isoformat().replace(
        "+00:00", "Z"
    )
    candidate["decided_at"] = (now - timedelta(minutes=15)).isoformat().replace(
        "+00:00", "Z"
    )
    governance["decided_at"] = (now - timedelta(minutes=10)).isoformat().replace(
        "+00:00", "Z"
    )
    reviewer["reviewed_at"] = (now - timedelta(minutes=8)).isoformat().replace(
        "+00:00", "Z"
    )
    return {
        "run_envelope": bind_digest(envelope, "canonical_digest"),
        "candidate_decision": bind_digest(candidate, "decision_digest"),
        "governance_decision": bind_digest(governance, "decision_digest"),
        "reviewer_independence": bind_digest(reviewer, "review_digest"),
    }


def publication_action(
    operation: str, authority: dict[str, Any], now: datetime
) -> dict[str, Any]:
    title = "Fix bounded fixture"
    body = "Repairs #101 with exact evidence."
    envelope = authority["run_envelope"]
    candidate = authority["candidate_decision"]
    governance = authority["governance_decision"]
    reviewer = authority["reviewer_independence"]
    repository = candidate["repository"]
    action = {
        "schema": "ao.architecture.autonomous-issue-repair.github-action-digest.v1",
        "run_id": candidate["run_id"],
        "repository": repository,
        "issue_number": candidate["issue_number"],
        "base_sha": candidate["base_sha"],
        "head_sha": governance["head_sha"],
        "fork": f"{envelope['routing']['fork_owner']}/{repository.split('/', 1)[1]}",
        "branch": envelope["routing"]["repair_branch"],
        "pr_title_digest": hashlib.sha256(title.encode()).hexdigest(),
        "pr_body_digest": hashlib.sha256(body.encode()).hexdigest(),
        "diff_digest": reviewer["subject_digest"],
        "required_checks": governance["required_checks"],
        "action": operation,
        "approved_at": (now - timedelta(minutes=5)).isoformat().replace("+00:00", "Z"),
        "expires_at": (now + timedelta(minutes=55)).isoformat().replace("+00:00", "Z"),
        "run_envelope_digest": envelope["canonical_digest"],
        "candidate_decision_digest": candidate["decision_digest"],
        "governance_decision_digest": governance["decision_digest"],
        "reviewer_independence_digest": reviewer["review_digest"],
    }
    action["action_digest"] = canonical_digest(action)
    return action


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
    publication_now = datetime.now(timezone.utc)
    authority = publication_authority(architecture_root, publication_now)
    push = publication_action("push_operator_fork", authority, publication_now)
    draft = publication_action("open_upstream_draft_pr", authority, publication_now)
    for action in [push, draft]:
        action_errors = validator.validate_contract_instance(
            "github_action_digest", action, root=architecture_root
        )
        if action_errors:
            raise RuntimeError(
                f"Architecture GitHub action validation failed: {action_errors}"
            )
        link_errors = validator.validate_action_digest_links(
            action,
            authority["run_envelope"],
            authority["candidate_decision"],
            authority["governance_decision"],
            authority["reviewer_independence"],
            reference_time=publication_now,
            root=architecture_root,
        )
        if link_errors:
            raise RuntimeError(
                f"Architecture GitHub action authority link failed: {link_errors}"
            )
    plan = {
        "schema_version": "ao2.github-repair-publication-plan.v1",
        "architecture_contract_commit": ARCHITECTURE_SHA,
        "authority": authority,
        "push_action": push,
        "draft_action": draft,
        "draft": {
            "title": "Fix bounded fixture",
            "body": "Repairs #101 with exact evidence.",
        },
    }
    with tempfile.TemporaryDirectory(prefix="ao2-publication-oracle-") as directory:
        plan_path = Path(directory) / "plan.json"
        plan_path.write_text(json.dumps(plan), encoding="utf-8")
        result = subprocess.run(
            [
                str(ao2_binary(args.ao2)),
                "issue",
                "publish",
                "verify",
                "--plan",
                str(plan_path),
                "--expected-push-action-digest",
                push["action_digest"],
                "--expected-draft-action-digest",
                draft["action_digest"],
                "--json",
            ],
            check=True,
            text=True,
            capture_output=True,
        )
    readback = json.loads(result.stdout)
    expected_zero_writes = {
        "github_contacted": False,
        "git_write_performed": False,
        "draft_pr_write_performed": False,
        "merge_performed": False,
    }
    for field, expected in expected_zero_writes.items():
        if readback.get(field) is not expected:
            raise RuntimeError(f"AO2 publication verification changed {field}")
    print("AO2 discovery and publication Architecture oracle passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
