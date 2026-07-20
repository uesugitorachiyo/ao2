import json
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "validate_github_draft_pr_readback.py"
HEAD_SHA = "a" * 40


def timestamp(delta: timedelta = timedelta()) -> str:
    return (
        datetime.now(timezone.utc) + delta
    ).isoformat().replace("+00:00", "Z")


def valid_readback() -> dict:
    return {
        "schema_version": "ao2.github-draft-pr-readback.v1",
        "captured_at": timestamp(),
        "repository": "uesugitorachiyo/ao-crucible",
        "pull_request": {
            "number": 9,
            "state": "OPEN",
            "isDraft": True,
            "mergedAt": None,
            "headRefOid": HEAD_SHA,
            "baseRefName": "main",
            "statusCheckRollup": [
                {
                    "name": "test",
                    "status": "COMPLETED",
                    "conclusion": "SUCCESS",
                }
            ],
        },
        "boundaries": {
            "issue_write_performed": False,
            "ready_for_review_performed": False,
            "review_approval_performed": False,
            "merge_performed": False,
        },
    }


def run_validator(tmp_path: Path, payload: dict, *extra: str) -> subprocess.CompletedProcess:
    readback = tmp_path / "readback.json"
    readback.write_text(json.dumps(payload), encoding="utf-8")
    return subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--input",
            str(readback),
            "--expected-head-sha",
            HEAD_SHA,
            "--max-age-seconds",
            "3600",
            *extra,
        ],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )


def test_valid_draft_with_successful_ci_passes(tmp_path):
    result = run_validator(tmp_path, valid_readback())

    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary["status"] == "passed"
    assert summary["promotion_eligible"] is False
    assert summary["draft_only"] is True
    assert summary["all_checks_successful"] is True


def test_failed_pending_and_missing_ci_fail_closed(tmp_path):
    for status, conclusion in [
        ("COMPLETED", "FAILURE"),
        ("IN_PROGRESS", None),
    ]:
        payload = valid_readback()
        payload["pull_request"]["statusCheckRollup"][0].update(
            status=status,
            conclusion=conclusion,
        )
        result = run_validator(tmp_path, payload)
        assert result.returncode == 1
        assert "status checks must all be completed successfully" in result.stderr

    payload = valid_readback()
    payload["pull_request"]["statusCheckRollup"] = []
    result = run_validator(tmp_path, payload)
    assert result.returncode == 1
    assert "at least one status check is required" in result.stderr


def test_stale_wrong_head_and_non_draft_readbacks_fail_closed(tmp_path):
    payload = valid_readback()
    payload["captured_at"] = timestamp(timedelta(hours=-2))
    result = run_validator(tmp_path, payload)
    assert result.returncode == 1
    assert "readback is stale" in result.stderr

    payload = valid_readback()
    payload["pull_request"]["headRefOid"] = "b" * 40
    result = run_validator(tmp_path, payload)
    assert result.returncode == 1
    assert "headRefOid does not match" in result.stderr

    payload = valid_readback()
    payload["pull_request"]["isDraft"] = False
    result = run_validator(tmp_path, payload)
    assert result.returncode == 1
    assert "pull request must remain draft" in result.stderr


def test_malformed_unsafe_or_oversized_readbacks_fail_closed(tmp_path):
    payload = valid_readback()
    payload["unexpected"] = True
    result = run_validator(tmp_path, payload)
    assert result.returncode == 1
    assert "unexpected top-level fields" in result.stderr

    payload = valid_readback()
    payload["boundaries"]["merge_performed"] = True
    result = run_validator(tmp_path, payload)
    assert result.returncode == 1
    assert "boundary flags must all be false" in result.stderr

    oversized = tmp_path / "oversized.json"
    oversized.write_text(" " * (1024 * 1024 + 1), encoding="utf-8")
    result = subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--input",
            str(oversized),
            "--expected-head-sha",
            HEAD_SHA,
        ],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 1
    assert "input exceeds 1048576 bytes" in result.stderr
