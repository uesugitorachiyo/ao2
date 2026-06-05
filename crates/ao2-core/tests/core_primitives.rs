//! Coverage for the ao2-core primitives that underpin everything else.
//!
//! `sha256_hex` and `new_id` are the foundation of every content address,
//! event id, trace id, and the policy action digest across both the agent and
//! the control-plane. `RunStatus` is a wire enum whose snake_case spellings are
//! a serialization contract. `AoEvent::new` derives the audit-trail correlation
//! fields. None of these had a direct test (only `obligations.rs` was covered).

use ao2_core::{new_id, sha256_hex, Actor, AoEvent, RunStatus};
use serde_json::json;

// ---- sha256_hex ----------------------------------------------------------

#[test]
fn sha256_hex_matches_known_nist_vectors() {
    // Pin against the standard, not just self-consistency: if the hashing ever
    // changed algorithm or encoding, content addresses computed here would
    // silently stop matching peers (the control-plane, signature verifiers)
    // that compute SHA-256 the canonical way.
    assert_eq!(
        sha256_hex(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "SHA-256 of the empty string"
    );
    assert_eq!(
        sha256_hex("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "SHA-256(\"abc\") — FIPS 180-2 example"
    );
}

#[test]
fn sha256_hex_is_deterministic_well_formed_and_input_sensitive() {
    let a = sha256_hex("the quick brown fox");
    assert_eq!(a, sha256_hex("the quick brown fox"), "deterministic");
    assert_eq!(a.len(), 64);
    assert!(
        a.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "must be 64 lowercase hex chars"
    );
    assert_ne!(
        a,
        sha256_hex("the quick brown fox."),
        "a one-byte change must change the digest"
    );
}

#[test]
fn sha256_hex_accepts_any_byte_source_with_consistent_results() {
    // The signature is `impl AsRef<[u8]>`; the same bytes via different concrete
    // types must hash identically.
    let s: &str = "payload";
    let owned: String = "payload".to_string();
    let slice: &[u8] = b"payload";
    let vec: Vec<u8> = b"payload".to_vec();
    let expected = sha256_hex(s);
    assert_eq!(sha256_hex(owned), expected);
    assert_eq!(sha256_hex(slice), expected);
    assert_eq!(sha256_hex(vec), expected);
}

// ---- new_id --------------------------------------------------------------

#[test]
fn new_id_is_prefixed_and_carries_a_valid_uuid() {
    let id = new_id("run");
    let suffix = id
        .strip_prefix("run-")
        .expect("id must be `<prefix>-<uuid>`");
    uuid::Uuid::parse_str(suffix).expect("suffix must be a valid UUID");
}

#[test]
fn new_id_is_unique_per_call() {
    let a = new_id("evt");
    let b = new_id("evt");
    assert_ne!(a, b, "each id must be unique even for the same prefix");
}

#[test]
fn new_id_handles_an_empty_prefix() {
    let id = new_id("");
    let suffix = id.strip_prefix('-').expect("empty prefix yields `-<uuid>`");
    uuid::Uuid::parse_str(suffix).expect("suffix must be a valid UUID");
}

// ---- RunStatus serde contract --------------------------------------------

#[test]
fn run_status_serializes_to_the_expected_snake_case_wire_strings() {
    // The snake_case spelling is a persisted wire contract — a rename would
    // orphan every previously written record. Pin all twelve variants.
    let cases = [
        (RunStatus::Created, "created"),
        (RunStatus::Compiled, "compiled"),
        (RunStatus::Queued, "queued"),
        (RunStatus::Running, "running"),
        (RunStatus::WaitingForApproval, "waiting_for_approval"),
        (RunStatus::Blocked, "blocked"),
        (RunStatus::Failed, "failed"),
        (RunStatus::Rejected, "rejected"),
        (RunStatus::Accepted, "accepted"),
        (RunStatus::AcceptedWithConcerns, "accepted_with_concerns"),
        (RunStatus::Canceled, "canceled"),
        (RunStatus::Replaying, "replaying"),
    ];
    assert_eq!(cases.len(), 12, "every RunStatus variant must be listed");

    for (status, wire) in cases {
        let serialized = serde_json::to_value(status).unwrap();
        assert_eq!(serialized, json!(wire), "{status:?} serializes to `{wire}`");
        // Round-trip back to the same variant.
        let parsed: RunStatus = serde_json::from_value(json!(wire)).unwrap();
        assert_eq!(parsed, status, "`{wire}` parses back to {status:?}");
    }
}

#[test]
fn run_status_rejects_unknown_wire_values() {
    let parsed: Result<RunStatus, _> = serde_json::from_value(json!("nonexistent_state"));
    assert!(
        parsed.is_err(),
        "unknown status strings must not deserialize"
    );
}

// ---- Actor factories -----------------------------------------------------

#[test]
fn actor_factories_produce_stable_ids_and_kinds() {
    let system = Actor::system();
    assert_eq!(system.id, "system:ao2");
    assert_eq!(system.kind, "system");

    let human = Actor::human_local();
    assert_eq!(human.id, "human:local-user");
    assert_eq!(human.kind, "human");

    let role = Actor::role("planner");
    assert_eq!(role.id, "role:planner");
    assert_eq!(role.kind, "agent_role");
}

#[test]
fn actor_round_trips_through_json() {
    let actor = Actor::role("reviewer");
    let json = serde_json::to_string(&actor).unwrap();
    let back: Actor = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, actor.id);
    assert_eq!(back.kind, actor.kind);
}

// ---- AoEvent::new --------------------------------------------------------

#[test]
fn ao_event_new_populates_correlation_and_digest_fields() {
    let payload = json!({"k": "v", "n": 1});
    let event = AoEvent::new(
        "run-123",
        "wf-abc",
        "task.started",
        Some("role:planner"),
        Some("task-7"),
        Actor::system(),
        payload.clone(),
    );

    assert!(event.event_id.starts_with("evt-"));
    assert_eq!(event.run_id, "run-123");
    assert_eq!(event.workflow_id, "wf-abc");
    assert_eq!(event.event_type, "task.started");
    assert_eq!(event.role_id.as_deref(), Some("role:planner"));
    assert_eq!(event.task_id.as_deref(), Some("task-7"));
    assert_eq!(event.schema_version, "ao2.event.v1");
    assert_eq!(event.sensitivity, "internal");
    // correlation defaults to the run; nothing caused this event yet.
    assert_eq!(event.correlation_id, "run-123");
    assert!(event.causation_id.is_none());

    // payload_digest is the SHA-256 of the serialized payload.
    let expected_digest = sha256_hex(serde_json::to_vec(&payload).unwrap());
    assert_eq!(event.payload_digest, expected_digest);

    // trace/span ids are truncated hex of known length.
    assert_eq!(event.trace_id.len(), 32);
    assert_eq!(event.span_id.len(), 16);
    assert!(event.trace_id.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(event.span_id.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn ao_event_trace_id_is_deterministic_per_run() {
    // trace_id derives only from run_id, so all events of a run share a trace —
    // the property that lets the audit log stitch a run together.
    let mk =
        |run: &str| AoEvent::new(run, "wf", "e", None, None, Actor::system(), json!({})).trace_id;
    assert_eq!(mk("run-A"), mk("run-A"), "same run → same trace_id");
    assert_ne!(
        mk("run-A"),
        mk("run-B"),
        "different run → different trace_id"
    );
    // And it matches the documented derivation.
    assert_eq!(mk("run-A"), sha256_hex("trace:run-A")[..32].to_string());
}

#[test]
fn ao_event_optional_fields_are_none_when_absent() {
    let event = AoEvent::new("r", "w", "e", None, None, Actor::human_local(), json!(null));
    assert!(event.role_id.is_none());
    assert!(event.task_id.is_none());
}

#[test]
fn ao_event_round_trips_through_json() {
    let event = AoEvent::new(
        "run-x",
        "wf-y",
        "evt.kind",
        None,
        None,
        Actor::system(),
        json!({"a": [1, 2, 3]}),
    );
    let serialized = serde_json::to_string(&event).unwrap();
    let back: AoEvent = serde_json::from_str(&serialized).unwrap();
    assert_eq!(back.event_id, event.event_id);
    assert_eq!(back.payload_digest, event.payload_digest);
    assert_eq!(back.trace_id, event.trace_id);
    assert_eq!(back.payload, event.payload);
}
