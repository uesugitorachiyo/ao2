import json
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
FIXTURE = REPO_ROOT / "tests" / "fixtures" / "github-issue-month3-repair-evidence.json"


def test_month3_repair_evidence_requires_regression_replay_rollback_and_resume():
    evidence = json.loads(FIXTURE.read_text(encoding="utf-8"))
    assert evidence["schema_version"] == "ao2.github-issue-month3-repair-evidence.v0.1"
    assert evidence["status"] == "verified"
    assert evidence["source_fixture"] == "deterministic_authentic_bug"

    pre_patch = evidence["pre_patch"]
    assert pre_patch["regression_test_written_before_fix"] is True
    assert pre_patch["expected_failure_observed"] is True
    assert pre_patch["negative_control_passed"] is True

    for key, value in evidence["post_patch"].items():
        assert value is True, f"post_patch.{key} must be true"

    approval = evidence["approval"]
    assert approval["status"] == "accepted"
    assert approval["required_digest_field"] == "action_digest"
    assert len(approval["action_digest"]) == 64
    assert approval["bypass_suggested"] is False

    replay = evidence["replay"]
    assert replay["status"] == "accepted"
    assert replay["evidence_digest"] == replay["replay_digest"]
    assert replay["digest_failures"] == []

    rollback = evidence["rollback"]
    assert rollback["before_digest"] == rollback["after_rollback_digest"]
    assert rollback["before_digest"] != rollback["after_repair_digest"]
    assert rollback["exact_state_restored"] is True

    resume = evidence["resume"]
    assert resume["resumed_after_interruption"] is True
    assert resume["duplicate_edits"] is False
    assert resume["stale_resume_rejected"] is True

    for action, value in evidence["denied_actions"].items():
        assert value is False, f"{action} must remain denied"
