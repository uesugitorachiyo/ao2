import hashlib
import json
import re
from datetime import datetime
from pathlib import Path

from scripts.ao2_windows_outbound_worker import canonical_json_bytes


ROOT = Path(__file__).resolve().parents[1]
VECTOR_PATH = ROOT / "tests" / "fixtures" / "compatibility" / "ao2-execution-receipt-v0.5.10.json"
COVENANT_AO2_VECTOR_PATH = (
    ROOT
    / "tests"
    / "fixtures"
    / "compatibility"
    / "covenant-approval-ticket-to-ao2-approved-execution-v0.1.json"
)

AO2_TAG_TARGET = "9f4f8a8cf596127a982627b4af25c90a9a842095"
CP_TAG_TARGET = "5de3541e9007e12d95b125e7f911c02932e21479"
MANIFEST_DIGEST = "a44bb65d59f46f3c3bf469dc7b26f0688fbf640f4f04ee9932a5a8fe186aeee3"
VECTOR_SHA256 = "fd7260329ea3c436436cd1572cba5abda72f5a9959b1157d5e61f595ae91857e"
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
    assert vector["vector_id"] == "ao2-v0.5.10-execution-receipt-to-control-plane-evidence-event"
    assert vector["edge"] == "ao2.execution_receipt -> ao2-control-plane.evidence_event"

    producer = vector["producer"]
    assert producer == {
        "repository": "ao2",
        "version": "v0.5.10",
        "release_url": "https://github.com/uesugitorachiyo/ao2/releases/tag/v0.5.10",
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
    assert receipt["receipt_id"] == "ao2-v0.5.10-provider-free-doctor-smoke"
    assert receipt["status"] == "passed"
    assert receipt["provider_execution_required"] is False
    assert receipt["workflow"] == "provider_free_doctor_smoke"
    assert receipt["command"] == "ao2 doctor --json"
    assert receipt["release"]["version"] == "v0.5.10"
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

    assert "compatibility_bridge" not in vector
    assert vector["native_qualification"] == {
        "ao2_version": "v0.5.10",
        "control_plane_version": "v0.1.19",
        "hosted_windows_run_id": 31279647320,
        "macos_summary_sha256": "5bf46636400d9f4709ab901f010c57fd329500d858bcea829f8f393dd93d9ba6",
        "linux_summary_sha256": "c22f56c4e3e6f1cdac5af698f0a7ec3ed8f18dd5ddeb9ae6b93b5f24de332cd3",
        "physical_windows_summary_sha256": "ac30e17c0eaa338ad2672a55736ffb90c82b86d1623f2a41f8d991e0a017a353",
        "architecture_edges_tested": 16,
        "architecture_edges_failed": 0,
    }

    assert vector["release_evidence"] == {
        "promotion_plan_sha256": "0e1ae4663eb09c3135b66326177855cb8d93bab84d776b130114c5d2c344dd21",
        "physical_windows_evidence_sha256": "a46f869c2c3512746ae686d65935b1612c1ef1ac0788f16bcd7de0d719268d81",
    }

    assert vector["evidence_binding"] == {
        "request_id": receipt["receipt_id"],
        "observation_id": event["event_id"],
        "artifact_id": receipt["evidence_path"],
        "producer_schema": receipt["schema_version"],
        "consumer_schema": event["schema_version"],
        "execution_receipt_sha256": "355c1543695b7af01d485b004ec86003cb84887589b29520e902fcd654505703",
        "expected_control_plane_event_sha256": "4699d618c7cd568ae08c8206af756ff5f45314d03ea35f91036bf768ae555d8c",
        "generated_at_utc": "2026-08-09T02:55:00Z",
        "fresh_until_utc": "2026-08-10T02:55:00Z",
        "producer_verifier_base_sha": "e77a4927f42533ae6d5fd8c1de5d43c4d6a10f2a",
        "consumer_verifier_base_sha": "5dc00501419be9f634db047cfa5b92d24aaa1129",
    }
    binding = vector["evidence_binding"]
    assert binding["execution_receipt_sha256"] == canonical_sha256(receipt)
    assert binding["expected_control_plane_event_sha256"] == canonical_sha256(event)
    generated = datetime.fromisoformat(binding["generated_at_utc"].replace("Z", "+00:00"))
    fresh_until = datetime.fromisoformat(binding["fresh_until_utc"].replace("Z", "+00:00"))
    verification_time = datetime.fromisoformat("2026-08-09T03:00:00+00:00")
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
