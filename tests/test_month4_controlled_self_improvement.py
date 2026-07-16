import hashlib
import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = ROOT / "tests" / "fixtures" / "controlled-self-improvement"
EVIDENCE_PACK = FIXTURE_ROOT / "dry-run-evidence-pack.v0.1.json"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def walk_strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for child in value.values():
            yield from walk_strings(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk_strings(child)


def test_controlled_self_improvement_dry_run_fixture_rolls_back_and_denies_live_authority():
    pack = json.loads(EVIDENCE_PACK.read_text(encoding="utf-8"))

    assert pack["schema_version"] == "ao2.controlled-self-improvement-dry-run-evidence-pack.v0.1"
    assert pack["status"] == "dry_run_passed"
    assert pack["proposal"]["proposal_id"] == "month4-fixture-only-self-change-proposal"
    assert pack["policy"]["approval_state"] == "required"
    assert pack["policy"]["execution_scope"] == "temporary_fixture_workspace"

    authority = pack["authority"]
    assert authority == {
        "dry_run_only": True,
        "live_self_modification_authorized": False,
        "provider_execution_required": False,
        "provider_execution_performed": False,
        "live_repository_mutation_performed": False,
        "rsi_authorized": False,
        "promotion_requested": False,
    }

    workspace = pack["fixture_workspace"]
    before = FIXTURE_ROOT / workspace["before_path"]
    during = FIXTURE_ROOT / workspace["during_path"]
    after = FIXTURE_ROOT / workspace["after_path"]

    assert sha256(before) == workspace["before_sha256"]
    assert sha256(during) == workspace["during_sha256"]
    assert sha256(after) == workspace["after_sha256"]
    assert workspace["before_sha256"] != workspace["during_sha256"]
    assert workspace["before_sha256"] == workspace["after_sha256"]

    rollback = pack["rollback"]
    assert rollback["rollback_verified"] is True
    assert rollback["restored_sha256"] == workspace["before_sha256"]
    assert rollback["scope"] == "temporary_fixture_workspace"

    replay = pack["approval_replay"]
    assert replay["required_digest_field"] == "action_digest"
    assert replay["wrong_digest_rejected"] is True
    assert replay["correct_digest_accepted_for_dry_run"] is True
    assert replay["correct_digest_grants_live_authority"] is False


def test_controlled_self_improvement_dry_run_fixture_is_public_safe():
    pack = json.loads(EVIDENCE_PACK.read_text(encoding="utf-8"))

    for text in walk_strings(pack):
        assert "/Users/" not in text
        assert "Documents/canary-test" not in text
        assert "tt" not in text.lower().split("/")
        assert "module" not in text.lower()
        assert not re.search(r"(token|secret|password|credential)", text, re.IGNORECASE)
