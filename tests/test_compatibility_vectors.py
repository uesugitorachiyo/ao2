import hashlib
import json
import re
from datetime import datetime
from pathlib import Path

from scripts.ao2_windows_outbound_worker import canonical_json_bytes


ROOT = Path(__file__).resolve().parents[1]
VECTOR_PATH = ROOT / "tests" / "fixtures" / "compatibility" / "ao2-execution-receipt-v0.5.9.json"
COVENANT_AO2_VECTOR_PATH = (
    ROOT
    / "tests"
    / "fixtures"
    / "compatibility"
    / "covenant-approval-ticket-to-ao2-approved-execution-v0.1.json"
)

AO2_TAG_TARGET = "fec09515dfe4e550eeaddc7da497b1fe912012b4"
CP_TAG_TARGET = "5de3541e9007e12d95b125e7f911c02932e21479"
MANIFEST_DIGEST = "5f82c24b239c50dadb72e2bfafe1a310b04724cfacff5acee88f5164ec3c59cd"
VECTOR_SHA256 = "00ee9978b5325bc40d5d5de8f63227716d2ca2fe88c81182fdf6e68448d15a7d"
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


def canonical_sha256(value: object) -> str:
    return hashlib.sha256(canonical_json_bytes(value)).hexdigest()


def test_ao2_execution_receipt_vector_matches_current_public_pair():
    vector = load_vector()

    assert vector["schema_version"] == "ao.compatibility.execution-receipt-vector.v1"
    assert hashlib.sha256(VECTOR_PATH.read_bytes()).hexdigest() == VECTOR_SHA256
    assert vector["vector_id"] == "ao2-v0.5.9-execution-receipt-to-control-plane-evidence-event"
    assert vector["edge"] == "ao2.execution_receipt -> ao2-control-plane.evidence_event"

    producer = vector["producer"]
    assert producer == {
        "repository": "ao2",
        "version": "v0.5.9",
        "release_url": "https://github.com/uesugitorachiyo/ao2/releases/tag/v0.5.9",
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
    assert receipt["receipt_id"] == "ao2-v0.5.9-provider-free-doctor-smoke"
    assert receipt["status"] == "passed"
    assert receipt["provider_execution_required"] is False
    assert receipt["workflow"] == "provider_free_doctor_smoke"
    assert receipt["command"] == "ao2 doctor --json"
    assert receipt["release"]["version"] == "v0.5.9"
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
        "predecessor_producer_version": "v0.5.8",
        "predecessor_producer_tag_target": "a879ae7969a26d13432c7cc402174861b2444c05",
        "predecessor_consumer_version": "v0.1.19",
        "predecessor_consumer_tag_target": CP_TAG_TARGET,
        "contract_change": "unchanged",
        "producer_schema_version": receipt["schema_version"],
        "consumer_schema_version": event["schema_version"],
    }

    assert vector["release_evidence"] == {
        "promotion_plan_sha256": "4e61e689432e9eddb7885448bd7bf2a70ccb46cc8ca5103be76ec9814d09c591",
        "physical_windows_evidence_sha256": "df4384874bb2f89c67fe0b5c588cfbcbb89d2e50b123595dd5d1ca4a5b38a8f0",
    }

    assert vector["evidence_binding"] == {
        "request_id": receipt["receipt_id"],
        "observation_id": event["event_id"],
        "artifact_id": receipt["evidence_path"],
        "producer_schema": receipt["schema_version"],
        "consumer_schema": event["schema_version"],
        "execution_receipt_sha256": "84ccd9515b32fe4d0de76c4c9183cf3f913c3232bfb4e23efd29af4f425907a8",
        "expected_control_plane_event_sha256": "3cfd8f473eb7941929cd6627fee98b9e4a5d813734d83ad036f25afe7fc8750e",
        "generated_at_utc": "2026-08-07T20:40:05Z",
        "fresh_until_utc": "2026-08-08T18:58:58.048305Z",
        "producer_verifier_base_sha": "1ea4c482ad105227a5701f6b8eafcd16c42d06e9",
        "consumer_verifier_base_sha": "eb420864794ceb9ebadef8f3f551772095edb758",
    }
    binding = vector["evidence_binding"]
    assert binding["execution_receipt_sha256"] == canonical_sha256(receipt)
    assert binding["expected_control_plane_event_sha256"] == canonical_sha256(event)
    generated = datetime.fromisoformat(binding["generated_at_utc"].replace("Z", "+00:00"))
    fresh_until = datetime.fromisoformat(binding["fresh_until_utc"].replace("Z", "+00:00"))
    verification_time = datetime.fromisoformat("2026-08-08T00:00:00+00:00")
    assert generated <= verification_time <= fresh_until
    assert 0 < (fresh_until - generated).total_seconds() <= 24 * 60 * 60


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
