use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

const QUEUE_LIST_SCHEMA: &str = "ao2.factory-v3-compat-workbench-queue-list.v1";
const ATTESTATION_SCHEMA: &str = "factory-v3/ao2-watchdog-no-active-ao2-runs-attestation/v1";
const EXPECTED_FACTORY_V3_ROLE: &str = "parity_oracle_only";
const DEFAULT_REASON: &str = "AO2 factory queue-list snapshot reports no active entries; the overdue Hermes one-shot has no in-flight AO2 run to cancel";

fn write_queue_list(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("queue-list parent");
    }
    fs::write(path, serde_json::to_string_pretty(value).unwrap()).expect("queue-list write");
}

fn run_cancel_authority(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["factory", "cancel-authority"])
        .args(args)
        .output()
        .expect("invoke ao2 factory cancel-authority")
}

fn empty_queue_list_snapshot(queue_path: &str) -> Value {
    json!({
        "schema_version": QUEUE_LIST_SCHEMA,
        "owner": "ao2-workbench-queue",
        "factory_v3_role": "parity_oracle_only",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "queue_path": queue_path,
        "entry_count": 0,
        "continuity_contract": Value::Null,
        "entries": []
    })
}

fn queue_list_with_entries(queue_path: &str, entries: Vec<Value>) -> Value {
    let mut snapshot = empty_queue_list_snapshot(queue_path);
    let entry_count = entries.len();
    snapshot["entries"] = Value::Array(entries);
    snapshot["entry_count"] = json!(entry_count);
    snapshot
}

#[test]
fn emits_no_active_runs_attestation_for_empty_queue() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    let queue_path = "/tmp/queue.json";
    write_queue_list(&snapshot_path, &empty_queue_list_snapshot(queue_path));

    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let attestation: Value =
        serde_json::from_slice(&output.stdout).expect("attestation prints JSON");

    assert_eq!(attestation["schema"], ATTESTATION_SCHEMA);
    assert_eq!(attestation["factory_v3_role"], EXPECTED_FACTORY_V3_ROLE);
    assert_eq!(attestation["no_active_ao2_runs"], json!(true));
    assert_eq!(attestation["reason"], DEFAULT_REASON);
    assert!(
        attestation["produced_at_ms"].as_i64().unwrap() > 0,
        "produced_at_ms must be a positive integer (millis), got {}",
        attestation["produced_at_ms"]
    );
    let source = &attestation["source"];
    assert_eq!(source["schema_version"], QUEUE_LIST_SCHEMA);
    assert_eq!(source["queue_path"], queue_path);
    assert_eq!(source["entry_count"], json!(0));
    assert_eq!(source["active_entry_count"], json!(0));
    assert!(
        source["status_counts"].is_object(),
        "status_counts must be an object"
    );
    assert!(
        source["status_counts"].as_object().unwrap().is_empty(),
        "empty queue has empty status_counts"
    );
}

#[test]
fn includes_terminal_status_counts_when_no_active_entries() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    let queue_path = "/tmp/queue.json";
    let entries = vec![
        json!({"run_id": "r-a", "status": "cancelled"}),
        json!({"run_id": "r-b", "status": "cancelled"}),
        json!({"run_id": "r-c", "status": "completed"}),
    ];
    write_queue_list(
        &snapshot_path,
        &queue_list_with_entries(queue_path, entries),
    );

    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let attestation: Value = serde_json::from_slice(&output.stdout).unwrap();
    let counts = attestation["source"]["status_counts"].as_object().unwrap();
    assert_eq!(counts.get("cancelled").unwrap(), &json!(2));
    assert_eq!(counts.get("completed").unwrap(), &json!(1));
    assert_eq!(attestation["source"]["entry_count"], json!(3));
    assert_eq!(attestation["source"]["active_entry_count"], json!(0));
}

#[test]
fn refuses_when_queue_has_active_entry() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    let entries = vec![
        json!({"run_id": "r-1", "status": "running"}),
        json!({"run_id": "r-2", "status": "cancelled"}),
    ];
    write_queue_list(
        &snapshot_path,
        &queue_list_with_entries("/tmp/queue.json", entries),
    );

    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        !output.status.success(),
        "expected refusal; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("r-1=running"),
        "stderr must list the active run_id=status pair; got: {stderr}"
    );
}

#[test]
fn refuses_when_cancel_requested_is_still_pending() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    let entries = vec![json!({"run_id": "r-1", "status": "cancel_requested"})];
    write_queue_list(
        &snapshot_path,
        &queue_list_with_entries("/tmp/queue.json", entries),
    );

    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--json",
    ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("r-1=cancel_requested"), "got: {stderr}");
}

#[test]
fn refuses_when_queue_has_queued_entry() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    let entries = vec![json!({"run_id": "r-9", "status": "queued"})];
    write_queue_list(
        &snapshot_path,
        &queue_list_with_entries("/tmp/queue.json", entries),
    );

    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--json",
    ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("r-9=queued"), "got: {stderr}");
}

#[test]
fn refuses_when_queue_list_schema_version_is_wrong() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    write_queue_list(
        &snapshot_path,
        &json!({
            "schema_version": "ao2.factory-v3-compat-workbench-queue-list.v0",
            "entries": []
        }),
    );

    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--json",
    ]);

    assert!(!output.status.success(), "expected refusal on wrong schema");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("schema_version"),
        "stderr must mention schema_version; got: {stderr}"
    );
}

#[test]
fn refuses_when_input_is_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let missing = tmp.path().join("does-not-exist.json");

    let output = run_cancel_authority(&["--queue-list-json", missing.to_str().unwrap(), "--json"]);

    assert!(
        !output.status.success(),
        "expected refusal on missing input"
    );
}

#[test]
fn refuses_when_input_is_not_json_object() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    fs::write(&snapshot_path, "[]").unwrap();

    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--json",
    ]);

    assert!(!output.status.success(), "expected refusal on non-object");
}

#[test]
fn writes_attestation_to_out_path_when_requested() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    let out_path = tmp.path().join("nested/attestation.json");
    write_queue_list(
        &snapshot_path,
        &empty_queue_list_snapshot("/tmp/queue.json"),
    );

    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--out",
        out_path.to_str().unwrap(),
    ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out_path.exists(), "out file should be created");
    let written: Value = serde_json::from_str(&fs::read_to_string(&out_path).unwrap()).unwrap();
    assert_eq!(written["schema"], ATTESTATION_SCHEMA);
    assert_eq!(written["no_active_ao2_runs"], json!(true));
}

#[test]
fn custom_reason_overrides_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    write_queue_list(
        &snapshot_path,
        &empty_queue_list_snapshot("/tmp/queue.json"),
    );

    let custom = "operator manually attested no active AO2 runs";
    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--reason",
        custom,
        "--json",
    ]);

    assert!(output.status.success());
    let attestation: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(attestation["reason"], custom);
}

#[test]
fn produced_at_ms_is_overridable_for_determinism() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    write_queue_list(
        &snapshot_path,
        &empty_queue_list_snapshot("/tmp/queue.json"),
    );

    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--produced-at-ms",
        "1748140000000",
        "--json",
    ]);

    assert!(output.status.success());
    let attestation: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(attestation["produced_at_ms"], json!(1_748_140_000_000_i64));
}

#[test]
fn attestation_field_set_matches_factory_v3_producer_contract() {
    // Bit-for-bit field contract with factory-v3
    // scripts/ao2_watchdog_cancel_authority_producer.py — the AO2-native
    // producer must remain a drop-in replacement so factory-v3's
    // validator (_validate_attestation in
    // scripts/ao2_watchdog_cancel_ownership.py) accepts it verbatim.
    let tmp = tempfile::tempdir().expect("tempdir");
    let snapshot_path = tmp.path().join("queue-list.json");
    write_queue_list(
        &snapshot_path,
        &empty_queue_list_snapshot("/tmp/queue.json"),
    );

    let output = run_cancel_authority(&[
        "--queue-list-json",
        snapshot_path.to_str().unwrap(),
        "--produced-at-ms",
        "1748140000000",
        "--json",
    ]);
    assert!(output.status.success());
    let attestation: Value = serde_json::from_slice(&output.stdout).unwrap();
    let top = attestation.as_object().unwrap();
    let mut top_keys: Vec<&String> = top.keys().collect();
    top_keys.sort();
    let expected_top: Vec<&str> = vec![
        "factory_v3_role",
        "no_active_ao2_runs",
        "produced_at_ms",
        "reason",
        "schema",
        "source",
    ];
    let actual_top: Vec<&str> = top_keys.iter().map(|s| s.as_str()).collect();
    assert_eq!(actual_top, expected_top, "top-level keys");

    let source = attestation["source"].as_object().unwrap();
    let mut src_keys: Vec<&String> = source.keys().collect();
    src_keys.sort();
    let expected_src: Vec<&str> = vec![
        "active_entry_count",
        "entry_count",
        "queue_path",
        "schema_version",
        "status_counts",
    ];
    let actual_src: Vec<&str> = src_keys.iter().map(|s| s.as_str()).collect();
    assert_eq!(actual_src, expected_src, "source-level keys");
}
