//! Coverage for the `ao2-core` primitive types and helpers.
//!
//! These are the shared building blocks every other crate depends on — IDs,
//! digests, the event envelope, and the serde wire shapes. They had no direct
//! tests; this file pins the contracts other crates rely on: digest stability,
//! the `RunStatus` snake_case wire form, and the `AoEvent` envelope invariants
//! (deterministic trace id, payload digest, correlation == run id).

use ao2_core::{
    new_id, sha256_hex, Actor, AoEvent, ArtifactRef, ClosureReport, PolicyDecision,
    PolicyIntegrityBinding, RunStatus,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct EventHashVectorSet {
    legacy_algorithm: String,
    policy_bound_algorithm: String,
    vectors: Vec<EventHashVector>,
}

#[derive(Debug, Deserialize)]
struct EventHashVector {
    name: String,
    payload: serde_json::Value,
    legacy_payload_digest: String,
    policy_integrity: PolicyIntegrityBinding,
    policy_bound_payload_digest: String,
}

#[test]
fn new_id_is_prefixed_and_unique() {
    let a = new_id("evt");
    let b = new_id("evt");
    assert!(a.starts_with("evt-"), "id not prefixed: {a}");
    assert!(b.starts_with("evt-"));
    assert_ne!(a, b, "ids should be unique");
    // prefix + '-' + uuid (36 chars).
    assert_eq!(a.len(), "evt-".len() + 36);
}

#[test]
fn sha256_hex_matches_known_vectors_and_is_stable() {
    // RFC/NIST known answers — guards against an accidental hasher/format change.
    assert_eq!(
        sha256_hex(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let digest = sha256_hex("ao2");
    assert_eq!(digest.len(), 64);
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(digest, sha256_hex("ao2"), "digest must be deterministic");
    // Accepts both &str and &[u8] via AsRef<[u8]>, with the same result.
    assert_eq!(sha256_hex("ao2"), sha256_hex(b"ao2".as_slice()));
}

#[test]
fn actor_constructors_have_stable_ids_and_kinds() {
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
fn run_status_serializes_to_snake_case() {
    let cases = [
        (RunStatus::Created, "\"created\""),
        (RunStatus::WaitingForApproval, "\"waiting_for_approval\""),
        (
            RunStatus::AcceptedWithConcerns,
            "\"accepted_with_concerns\"",
        ),
        (RunStatus::Replaying, "\"replaying\""),
    ];
    for (status, expected) in cases {
        let encoded = serde_json::to_string(&status).unwrap();
        assert_eq!(encoded, expected);
        let decoded: RunStatus = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, status);
    }
}

#[test]
fn ao_event_new_sets_envelope_invariants() {
    let payload = serde_json::json!({ "k": "v", "n": 7 });
    let event = AoEvent::new(
        "run-7",
        "wf-main",
        "task.started",
        Some("planner"),
        Some("task-1"),
        Actor::system(),
        payload.clone(),
    );

    assert!(event.event_id.starts_with("evt-"));
    assert_eq!(event.event_type, "task.started");
    assert_eq!(event.run_id, "run-7");
    assert_eq!(event.workflow_id, "wf-main");
    assert_eq!(event.role_id.as_deref(), Some("planner"));
    assert_eq!(event.task_id.as_deref(), Some("task-1"));
    // correlation_id defaults to the run id; causation starts empty.
    assert_eq!(event.correlation_id, "run-7");
    assert!(event.causation_id.is_none());
    assert_eq!(event.schema_version, "ao2.event.v1");
    assert_eq!(event.sensitivity, "internal");

    // trace id is a deterministic 32-hex prefix of sha256("trace:<run_id>").
    assert_eq!(event.trace_id.len(), 32);
    assert_eq!(event.trace_id, sha256_hex("trace:run-7")[..32]);
    // span id is a 16-hex slice (non-deterministic: includes a timestamp).
    assert_eq!(event.span_id.len(), 16);
    assert!(event.span_id.chars().all(|c| c.is_ascii_hexdigit()));

    // payload digest is the sha256 of the serialized payload.
    let expected_digest = sha256_hex(serde_json::to_vec(&payload).unwrap());
    assert_eq!(event.payload_digest, expected_digest);
    assert_eq!(event.payload, payload);
}

#[test]
fn policy_integrity_binding_changes_the_canonical_event_digest() {
    let payload = serde_json::json!({"operation":"sandbox:apply"});
    let baseline = PolicyIntegrityBinding {
        policy_identity: "ao2-policy.local".to_string(),
        policy_version: "v1".to_string(),
        policy_digest: "a".repeat(64),
    };
    let changed_identity = PolicyIntegrityBinding {
        policy_identity: "ao-covenant.production".to_string(),
        ..baseline.clone()
    };
    let changed_version = PolicyIntegrityBinding {
        policy_version: "v2".to_string(),
        ..baseline.clone()
    };
    let changed_digest = PolicyIntegrityBinding {
        policy_digest: "b".repeat(64),
        ..baseline.clone()
    };

    let event = |binding| {
        AoEvent::new(
            "run-policy",
            "workflow",
            "policy.evaluated",
            None,
            None,
            Actor::system(),
            payload.clone(),
        )
        .with_policy_integrity(binding)
    };
    let baseline_event = event(baseline);

    assert_ne!(
        baseline_event.payload_digest,
        event(changed_identity).payload_digest
    );
    assert_ne!(
        baseline_event.payload_digest,
        event(changed_version).payload_digest
    );
    assert_ne!(
        baseline_event.payload_digest,
        event(changed_digest).payload_digest
    );
}

#[test]
fn event_hash_vectors_preserve_legacy_and_policy_bound_migration_contracts() {
    let vectors: EventHashVectorSet = serde_json::from_str(include_str!(
        "../../../tests/fixtures/event-hash-vectors.json"
    ))
    .expect("event hash vector fixture parses");
    assert_eq!(vectors.legacy_algorithm, "ao2.event.payload.v1");
    assert_eq!(
        vectors.policy_bound_algorithm,
        "ao2.event.policy-integrity.v2"
    );
    assert!(!vectors.vectors.is_empty());

    for vector in vectors.vectors {
        assert_eq!(
            AoEvent::canonical_payload_digest(&vector.payload, None),
            vector.legacy_payload_digest,
            "legacy {}",
            vector.name
        );
        assert_eq!(
            AoEvent::canonical_payload_digest(&vector.payload, Some(&vector.policy_integrity)),
            vector.policy_bound_payload_digest,
            "policy-bound {}",
            vector.name
        );
    }
}

#[test]
fn ao_event_round_trips_through_serde() {
    let event = AoEvent::new(
        "run-rt",
        "wf",
        "evt.kind",
        None,
        None,
        Actor::human_local(),
        serde_json::json!({ "ok": true }),
    );
    let json = serde_json::to_string(&event).unwrap();
    let decoded: AoEvent = serde_json::from_str(&json).unwrap();

    assert_eq!(decoded.event_id, event.event_id);
    assert_eq!(decoded.payload_digest, event.payload_digest);
    assert_eq!(decoded.correlation_id, event.correlation_id);
    assert!(decoded.role_id.is_none());
    assert!(decoded.task_id.is_none());
    assert_eq!(decoded.payload, event.payload);
}

#[test]
fn wire_structs_round_trip_through_serde() {
    // The plain data carriers must survive a JSON round-trip unchanged so the
    // event store / API boundary can rely on their shape.
    let artifact = ArtifactRef {
        artifact_id: "art-1".to_string(),
        artifact_type: "plan".to_string(),
        uri: "ao2://artifacts/art-1".to_string(),
        media_type: "application/json".to_string(),
        digest: sha256_hex("art"),
        producer: "planner".to_string(),
        input_refs: vec!["art-0".to_string()],
        sensitivity: "internal".to_string(),
    };
    let decoded: ArtifactRef =
        serde_json::from_str(&serde_json::to_string(&artifact).unwrap()).unwrap();
    assert_eq!(decoded.artifact_id, artifact.artifact_id);
    assert_eq!(decoded.input_refs, artifact.input_refs);

    let decision = PolicyDecision {
        decision_id: "pol-1".to_string(),
        principal: "role:exec".to_string(),
        action: "git:push".to_string(),
        resource: "origin/main".to_string(),
        request_digest: sha256_hex("req"),
        decision: "requires_approval".to_string(),
        reason: "risky".to_string(),
        policy_version: "v1".to_string(),
        approval_ticket_id: None,
        created_at: chrono::Utc::now(),
    };
    let decoded: PolicyDecision =
        serde_json::from_str(&serde_json::to_string(&decision).unwrap()).unwrap();
    assert_eq!(decoded.decision, "requires_approval");
    assert!(decoded.approval_ticket_id.is_none());

    let closure = ClosureReport {
        verdict: "accepted".to_string(),
        acceptance_criteria_results: vec!["ac-1: pass".to_string()],
        evidence_refs: vec!["art-1".to_string()],
        unresolved_concerns: vec![],
        blockers: vec![],
        policy_exceptions: vec![],
        cost_summary: "$0.01".to_string(),
        created_at: chrono::Utc::now(),
    };
    let decoded: ClosureReport =
        serde_json::from_str(&serde_json::to_string(&closure).unwrap()).unwrap();
    assert_eq!(decoded.verdict, "accepted");
    assert_eq!(decoded.acceptance_criteria_results.len(), 1);
}
