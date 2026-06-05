use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

const QUEUE_LIST_SCHEMA: &str = "ao2.factory-v3-compat-workbench-queue-list.v1";
const TRANSITION_SCHEMA: &str = "ao2.factory-v3-compat-workbench-queue-transition.v1";
const EXPECTED_FACTORY_V3_ROLE: &str = "parity_oracle_only";
const EXPECTED_AO2_DECISION_OWNER: &str = "ao2-workbench-queue";

fn write_queue_list(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("queue-list parent");
    }
    fs::write(path, serde_json::to_string_pretty(value).unwrap()).expect("queue-list write");
}

fn run_cancel_transition(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["factory", "cancel-transition"])
        .args(args)
        .output()
        .expect("invoke ao2 factory cancel-transition")
}

fn cancelled_queue_list(run_id: &str, terminated_pid: i64) -> Value {
    let entry = json!({
        "schema_version": "ao2.factory-v3-compat-workbench-queue-entry.v1",
        "run_id": run_id,
        "status": "cancelled",
        "attempts": 1,
        "created_at": "2026-05-25T03:30:00Z",
        "updated_at": "2026-05-25T03:45:00Z",
        "terminated_pid": terminated_pid,
        "transition_history": [
            {"at": "2026-05-25T03:30:00Z", "status": "queued",    "reason": "submitted"},
            {"at": "2026-05-25T03:35:00Z", "status": "running",   "reason": "queue runner picked up"},
            {"at": "2026-05-25T03:45:00Z", "status": "cancelled", "reason": "operator cancelled queued governed run", "terminated_pid": terminated_pid}
        ]
    });
    json!({
        "schema_version": QUEUE_LIST_SCHEMA,
        "owner": "ao2-workbench-queue",
        "factory_v3_role": EXPECTED_FACTORY_V3_ROLE,
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "queue_path": "/tmp/queue.json",
        "entry_count": 1,
        "continuity_contract": Value::Null,
        "entries": [entry]
    })
}

#[test]
fn emits_canonical_transition_for_cancelled_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    write_queue_list(&queue_list_path, &cancelled_queue_list("r-abc", 42_424));

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "42424",
        "--produced-at-ms",
        "1748140000000",
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let transition: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(transition["schema_version"], TRANSITION_SCHEMA);
    assert_eq!(transition["factory_v3_role"], EXPECTED_FACTORY_V3_ROLE);
    assert_eq!(
        transition["ao2_decision_owner"],
        EXPECTED_AO2_DECISION_OWNER
    );
    assert_eq!(transition["produced_at_ms"], json!(1_748_140_000_000_i64));

    let entry = &transition["entry"];
    assert_eq!(entry["status"], "cancelled");
    assert_eq!(entry["terminated_pid"], json!(42_424_i64));
    assert_eq!(entry["run_id"], "r-abc");
    let history = entry["transition_history"].as_array().unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history.last().unwrap()["status"], "cancelled");

    let source = &transition["source"];
    assert_eq!(source["schema_version"], QUEUE_LIST_SCHEMA);
    assert_eq!(source["queue_path"], "/tmp/queue.json");
    assert_eq!(source["run_id"], "r-abc");
    assert_eq!(source["terminated_pid"], json!(42_424_i64));
}

#[test]
fn refuses_when_run_id_is_not_in_queue_list() {
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    write_queue_list(&queue_list_path, &cancelled_queue_list("r-other", 1234));

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-missing",
        "--terminated-pid",
        "1234",
        "--json",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("r-missing"),
        "stderr should mention the missing run_id; got: {stderr}"
    );
}

#[test]
fn refuses_when_matched_entry_is_not_cancelled() {
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    let mut snapshot = cancelled_queue_list("r-abc", 4242);
    snapshot["entries"][0]["status"] = json!("running");
    write_queue_list(&queue_list_path, &snapshot);

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "4242",
        "--json",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cancelled"),
        "stderr should mention status requirement; got: {stderr}"
    );
}

#[test]
fn refuses_when_queue_list_schema_is_wrong() {
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    let mut snapshot = cancelled_queue_list("r-abc", 4242);
    snapshot["schema_version"] = json!("ao2.factory-v3-compat-workbench-queue-list.v0");
    write_queue_list(&queue_list_path, &snapshot);

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "4242",
        "--json",
    ]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("schema_version"), "got: {stderr}");
}

#[test]
fn refuses_when_input_file_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nope.json");
    let output = run_cancel_transition(&[
        "--queue-list-json",
        missing.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "4242",
        "--json",
    ]);
    assert!(!output.status.success());
}

#[test]
fn refuses_when_input_is_not_json_object() {
    let tmp = tempfile::tempdir().unwrap();
    let snapshot = tmp.path().join("queue-list.json");
    fs::write(&snapshot, "[]").unwrap();
    let output = run_cancel_transition(&[
        "--queue-list-json",
        snapshot.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "4242",
        "--json",
    ]);
    assert!(!output.status.success());
}

#[test]
fn writes_attestation_to_out_path_when_requested() {
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    let out = tmp.path().join("nested/transition.json");
    write_queue_list(&queue_list_path, &cancelled_queue_list("r-abc", 4242));

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "4242",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.exists());
    let written: Value = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(written["schema_version"], TRANSITION_SCHEMA);
    assert_eq!(written["entry"]["terminated_pid"], json!(4242_i64));
}

#[test]
fn produced_at_ms_is_overridable_for_determinism() {
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    write_queue_list(&queue_list_path, &cancelled_queue_list("r-abc", 4242));

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "4242",
        "--produced-at-ms",
        "1748140000000",
        "--json",
    ]);
    assert!(output.status.success());
    let transition: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(transition["produced_at_ms"], json!(1_748_140_000_000_i64));
}

#[test]
fn produced_at_ms_defaults_to_current_clock() {
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    write_queue_list(&queue_list_path, &cancelled_queue_list("r-abc", 4242));

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "4242",
        "--json",
    ]);
    assert!(output.status.success());
    let transition: Value = serde_json::from_slice(&output.stdout).unwrap();
    let produced = transition["produced_at_ms"].as_i64().unwrap();
    assert!(
        produced > 1_700_000_000_000,
        "produced_at_ms should look like a recent millis value, got {produced}"
    );
}

#[test]
fn refuses_when_terminated_pid_is_non_positive() {
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    write_queue_list(&queue_list_path, &cancelled_queue_list("r-abc", 4242));

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "0",
        "--json",
    ]);
    assert!(!output.status.success(), "zero pid must be rejected");
}

#[test]
fn entry_includes_run_id_for_traceability() {
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    write_queue_list(&queue_list_path, &cancelled_queue_list("r-trace-me", 9999));

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-trace-me",
        "--terminated-pid",
        "9999",
        "--json",
    ]);
    assert!(output.status.success());
    let transition: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(transition["entry"]["run_id"], "r-trace-me");
}

#[test]
fn transition_field_set_matches_factory_v3_validator_contract() {
    // Watchdog `_validate_transition` requires schema_version,
    // factory_v3_role, ao2_decision_owner, and entry.status == "cancelled".
    // `_transition_covers_pid` traverses entry.terminated_pid /
    // entry.transition_history[].terminated_pid. The AO2-native producer
    // must populate all of these so the watchdog accepts the receipt
    // verbatim.
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    write_queue_list(&queue_list_path, &cancelled_queue_list("r-abc", 4242));

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "4242",
        "--produced-at-ms",
        "1748140000000",
        "--json",
    ]);
    assert!(output.status.success());
    let transition: Value = serde_json::from_slice(&output.stdout).unwrap();
    let top = transition.as_object().unwrap();
    let mut top_keys: Vec<&str> = top.keys().map(|s| s.as_str()).collect();
    top_keys.sort();
    let expected: Vec<&str> = vec![
        "ao2_decision_owner",
        "entry",
        "factory_v3_role",
        "produced_at_ms",
        "schema_version",
        "source",
    ];
    assert_eq!(top_keys, expected, "top-level keys");

    let entry = transition["entry"].as_object().unwrap();
    let mut entry_keys: Vec<&str> = entry.keys().map(|s| s.as_str()).collect();
    entry_keys.sort();
    assert!(entry_keys.contains(&"status"), "entry.status required");
    assert!(
        entry_keys.contains(&"terminated_pid"),
        "entry.terminated_pid required (_transition_covers_pid traversal)"
    );
    assert!(entry_keys.contains(&"transition_history"));
    assert!(entry_keys.contains(&"run_id"));
}

#[test]
fn matches_entry_by_terminated_pid_in_transition_history_when_top_level_missing() {
    // Queue entries may not record `terminated_pid` at the entry root
    // (older AO2 schema). The producer must accept entries that record
    // the terminated pid only inside transition_history[].terminated_pid.
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    let mut snapshot = cancelled_queue_list("r-abc", 4242);
    // Strip the top-level terminated_pid from the entry; the transition
    // history still carries it.
    snapshot["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("terminated_pid");
    write_queue_list(&queue_list_path, &snapshot);

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "4242",
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let transition: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(transition["entry"]["terminated_pid"], json!(4242_i64));
}

#[test]
fn refuses_when_terminated_pid_not_recorded_anywhere_in_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let queue_list_path = tmp.path().join("queue-list.json");
    let mut snapshot = cancelled_queue_list("r-abc", 4242);
    // Strip terminated_pid from both top-level entry and the history
    // record so the entry can no longer prove it cancelled the pid.
    snapshot["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("terminated_pid");
    let history = snapshot["entries"][0]["transition_history"]
        .as_array_mut()
        .unwrap();
    for record in history.iter_mut() {
        record.as_object_mut().unwrap().remove("terminated_pid");
    }
    write_queue_list(&queue_list_path, &snapshot);

    let output = run_cancel_transition(&[
        "--queue-list-json",
        queue_list_path.to_str().unwrap(),
        "--run-id",
        "r-abc",
        "--terminated-pid",
        "4242",
        "--json",
    ]);
    assert!(
        !output.status.success(),
        "unbacked pid claim must be rejected"
    );
}
