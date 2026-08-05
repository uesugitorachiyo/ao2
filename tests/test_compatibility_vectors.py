import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
VECTOR_PATH = ROOT / "tests" / "fixtures" / "compatibility" / "ao2-execution-receipt-v0.5.8.json"
COVENANT_AO2_VECTOR_PATH = (
    ROOT
    / "tests"
    / "fixtures"
    / "compatibility"
    / "covenant-approval-ticket-to-ao2-approved-execution-v0.1.json"
)

AO2_TAG_TARGET = "a879ae7969a26d13432c7cc402174861b2444c05"
CP_TAG_TARGET = "5de3541e9007e12d95b125e7f911c02932e21479"
MANIFEST_DIGEST = "7818def468eb212f949c38480c810cbd8c6e5717b43333767781fef96c2ee135"
COVENANT_VECTOR_AO2_TAG_TARGET = "pending-v0.5.2-release-prep-merge"


def load_vector() -> dict:
    with VECTOR_PATH.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def load_covenant_ao2_vector() -> dict:
    with COVENANT_AO2_VECTOR_PATH.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def walk_strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for child in value.values():
            yield from walk_strings(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk_strings(child)


def test_ao2_execution_receipt_vector_matches_current_public_pair():
    vector = load_vector()

    assert vector["schema_version"] == "ao.compatibility.execution-receipt-vector.v1"
    assert vector["vector_id"] == "ao2-v0.5.8-execution-receipt-to-control-plane-evidence-event"
    assert vector["edge"] == "ao2.execution_receipt -> ao2-control-plane.evidence_event"

    producer = vector["producer"]
    assert producer == {
        "repository": "ao2",
        "version": "v0.5.8",
        "release_url": "https://github.com/uesugitorachiyo/ao2/releases/tag/v0.5.8",
        "tag_target": AO2_TAG_TARGET,
        "approved_manifest_digest": MANIFEST_DIGEST,
    }

    consumer = vector["consumer"]
    assert consumer == {
        "repository": "ao2-control-plane",
        "version": "v0.1.19",
        "release_url": "https://github.com/uesugitorachiyo/ao2-control-plane/releases/tag/v0.1.19",
        "tag_target": CP_TAG_TARGET,
    }


def test_ao2_execution_receipt_vector_derives_expected_control_plane_event():
    vector = load_vector()
    receipt = vector["execution_receipt"]
    event = vector["expected_control_plane_event"]

    assert receipt["schema_version"] == "ao2.execution-receipt.v1"
    assert receipt["receipt_id"] == "ao2-v0.5.8-provider-free-doctor-smoke"
    assert receipt["status"] == "passed"
    assert receipt["provider_execution_required"] is False
    assert receipt["workflow"] == "provider_free_doctor_smoke"
    assert receipt["command"] == "ao2 doctor --json"
    assert receipt["release"]["version"] == "v0.5.8"
    assert receipt["release"]["tag_target"] == AO2_TAG_TARGET

    assert event["schema_version"] == "ao2-control-plane.evidence-event.v1"
    assert event["event_type"] == "ao2.execution_receipt.observed"
    assert event["producer_receipt_id"] == receipt["receipt_id"]
    assert event["producer_schema_version"] == receipt["schema_version"]
    assert event["producer_status"] == receipt["status"]
    assert event["producer_release_version"] == receipt["release"]["version"]
    assert event["producer_release_tag_target"] == receipt["release"]["tag_target"]
    assert event["observed_evidence_path"] == receipt["evidence_path"]
    assert event["status"] == "accepted"

    bridge = vector["compatibility_bridge"]
    assert bridge == {
        "predecessor_producer_version": "v0.5.6",
        "predecessor_producer_tag_target": "5706ec9cf3a108d20984973975c2a56b905a8173",
        "predecessor_consumer_version": "v0.1.18",
        "predecessor_consumer_tag_target": "6257ec23fde726d4a0133c5b62231881fb6aaa9a",
        "contract_change": "unchanged",
        "producer_schema_version": receipt["schema_version"],
        "consumer_schema_version": event["schema_version"],
    }

    assert vector["release_evidence"] == {
        "promotion_plan_sha256": "9e988764ba7232663ba3ca23bcaabe229f0c915084cdc402fbc4202b624f5f6d",
        "physical_windows_evidence_sha256": "b0e64aeb386f5a1ca5884b52cb63b9e2bb1ebc98101cb2e0ce06e0bafccdd27c",
    }


def test_ao2_execution_receipt_vector_is_public_safe_and_non_authorizing():
    vector = load_vector()
    receipt = vector["execution_receipt"]
    event = vector["expected_control_plane_event"]

    boundary = vector["boundary"]
    assert boundary == {
        "provider_pilot": False,
        "external_user_contact": False,
        "release_or_tag_created": False,
        "upload_or_deployment": False,
        "rsi_work": False,
        "rsi_remains_denied": True,
    }

    assert receipt["authority"]["requires_provider_credentials"] is False
    assert receipt["authority"]["approves_execution"] is False
    assert receipt["authority"]["permits_release"] is False
    assert event["authority"]["control_plane_approves_execution"] is False
    assert event["authority"]["mutates_ao2_artifacts"] is False
    assert event["authority"]["permits_release"] is False

    for text in walk_strings(vector):
        assert "/Users/" not in text
        assert "Documents/canary-test" not in text
        assert "tt" not in text.lower().split("/")
        assert "module" not in text.lower()
        assert not re.search(r"(token|secret|password|credential)", text, re.IGNORECASE)


def test_covenant_approval_ticket_vector_maps_to_ao2_approved_execution_request():
    vector = load_covenant_ao2_vector()

    assert (
        vector["schema_version"]
        == "ao.compatibility.covenant-approval-ticket-to-ao2-approved-execution-vector.v1"
    )
    assert vector["vector_id"] == "ao-covenant-approval-ticket-to-ao2-approved-execution-v1"
    assert vector["edge"] == "ao-covenant.approval_ticket -> ao2.approved_execution_request"
    assert vector["producer"]["repository"] == "ao-covenant"
    assert vector["consumer"]["repository"] == "ao2"
    assert vector["consumer"]["version"] == "v0.5.2"
    assert vector["consumer"]["tag_target"] == COVENANT_VECTOR_AO2_TAG_TARGET

    ticket = vector["covenant_approval_ticket"]
    request = vector["expected_ao2_approved_execution_request"]
    assert ticket["schema_version"] == "ao.covenant.approval-ticket.v1"
    assert ticket["approval_state"] == "approved"
    assert request["schema_version"] == "ao2.approved-execution-request.v1"
    assert request["approval_ticket_id"] == ticket["ticket_id"]
    assert request["action_digest"] == ticket["action_digest"]
    assert request["required_digest_field"] == "action_digest"
    assert request["provider_execution_required"] is False
    assert request["release_or_publish_allowed"] is False
    assert request["status"] == "accepted_for_provider_free_execution"


def test_covenant_approval_ticket_vector_is_public_safe_and_non_authorizing():
    vector = load_covenant_ao2_vector()

    assert vector["boundaries"] == {
        "release_or_publish": False,
        "creates_tag": False,
        "uploads_assets": False,
        "deploys": False,
        "contacts_external_users": False,
        "provider_pilot": False,
        "promotion_requested": False,
        "promotion_granted": False,
        "executes_work": False,
        "approves_work": False,
        "mutates_repositories": False,
        "calls_providers": False,
        "rsi_work": False,
        "rsi_remains_denied": True,
    }

    for text in walk_strings(vector):
        assert "/Users/" not in text
        assert "Documents/canary-test" not in text
        assert "tt" not in text.lower().split("/")
        assert "module" not in text.lower()
        assert not re.search(r"(token|secret|password|credential)", text, re.IGNORECASE)
