use std::fs;
use std::path::Path;
use std::process::Command;

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::write(
        path,
        serde_json::to_string_pretty(value).expect("serialise fixture"),
    )
    .expect("write fixture");
}

fn ready_handoff() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.cp-release-candidate-handoff.v1",
        "status": "ready",
        "release": {
            "version": "0.4.79",
            "release_tag": "v0.4.79"
        },
        "gates": {
            "release_cockpit": "ready",
            "phase1_promotion": "observed",
            "decision_signature": "present",
            "provider_acceptance": "live_complete"
        },
        "acceptance": {
            "codex": {"status": "passed", "source_class": "live"},
            "claude": {"status": "passed", "source_class": "live"}
        },
        "operator_handoff": {
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer"
        },
        "links": {
            "release_candidate_handoff_json": "http://127.0.0.1:8744/api/v1/release/handoff.json"
        }
    })
}

fn run_ao2_handoff_checklist(handoff: &Path, out: &Path, extra: &[&str]) -> serde_json::Value {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let mut args: Vec<&str> = vec![
        "release",
        "handoff-checklist-build",
        "--handoff",
        handoff.to_str().expect("utf8"),
        "--write-json",
        out.to_str().expect("utf8"),
        "--json",
    ];
    args.extend(extra.iter().copied());
    let output = Command::new(ao2)
        .args(&args)
        .output()
        .expect("run ao2 release handoff-checklist-build");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("checklist is valid json")
}

#[test]
fn handoff_checklist_ready_release() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let handoff_path = tmp.path().join("handoff.json");
    let out = tmp.path().join("checklist.json");
    write_json(&handoff_path, &ready_handoff());

    let checklist = run_ao2_handoff_checklist(&handoff_path, &out, &[]);
    assert_eq!(
        checklist["schema"],
        "factory-v3/ao2-release-handoff-checklist/v1"
    );
    assert_eq!(checklist["status"], "ready_for_evaluator_closer");
    let blockers = checklist["blockers"].as_array().expect("blockers array");
    assert!(
        blockers.is_empty(),
        "expected zero blockers, got {blockers:?}"
    );
    let checks = checklist["checks"].as_array().expect("checks array");
    assert_eq!(checks.len(), 9, "9 default checks expected");
    let on_disk = fs::read_to_string(&out).expect("checklist written");
    assert!(on_disk.contains("\"status\": \"ready_for_evaluator_closer\""));
}

#[test]
fn handoff_checklist_blocked_when_gate_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let handoff_path = tmp.path().join("handoff.json");
    let out = tmp.path().join("checklist.json");
    let mut h = ready_handoff();
    h["gates"]["decision_signature"] = serde_json::Value::String("missing".into());
    write_json(&handoff_path, &h);

    let checklist = run_ao2_handoff_checklist(&handoff_path, &out, &[]);
    assert_eq!(checklist["status"], "blocked");
    let blockers: Vec<String> = checklist["blockers"]
        .as_array()
        .expect("blockers")
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect();
    assert!(
        blockers
            .iter()
            .any(|b| b.starts_with("decision_signature:")),
        "expected decision_signature blocker, got {blockers:?}"
    );
}

#[test]
fn handoff_checklist_trust_boundary_flags_attention() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let handoff_path = tmp.path().join("handoff.json");
    let out = tmp.path().join("checklist.json");
    let mut h = ready_handoff();
    h["operator_handoff"]["mutates_ao_artifacts"] = serde_json::Value::Bool(true);
    write_json(&handoff_path, &h);

    let checklist = run_ao2_handoff_checklist(&handoff_path, &out, &[]);
    let checks = checklist["checks"].as_array().expect("checks");
    let trust = checks
        .iter()
        .find(|c| c["id"] == "trust_boundary")
        .expect("trust_boundary check present");
    assert_eq!(trust["status"], "blocked");
    assert_eq!(trust["observed"], "attention");
}

#[test]
fn handoff_checklist_repo_head_checks_added() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let handoff_path = tmp.path().join("handoff.json");
    let out = tmp.path().join("checklist.json");
    let mut h = ready_handoff();
    h["release"]["repositories"] = serde_json::json!({
        "ao2": {"head": "abc123def456", "path": "/tmp/nonexistent-ao2"},
        "ao2-control-plane": {"head": "deadbeef0000", "path": "/tmp/nonexistent-cp"}
    });
    write_json(&handoff_path, &h);

    let checklist = run_ao2_handoff_checklist(
        &handoff_path,
        &out,
        &[
            "--expected-repo-head",
            "ao2=abc123def456",
            "--expected-repo-head",
            "ao2-control-plane=deadbeef0000",
        ],
    );
    let checks = checklist["checks"].as_array().expect("checks");
    let repo_heads: Vec<&serde_json::Value> = checks
        .iter()
        .filter(|c| c["id"].as_str().unwrap_or("").starts_with("repo_head_"))
        .collect();
    assert_eq!(repo_heads.len(), 2, "two repo_head checks expected");
    assert!(repo_heads.iter().all(|c| c["status"] == "passed"));
}

#[test]
fn handoff_checklist_allow_skipped_emits_planned_or_skipped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let handoff_path = tmp.path().join("handoff.json");
    let out = tmp.path().join("checklist.json");
    write_json(
        &handoff_path,
        &serde_json::json!({"status": "planned", "reason": "release not started"}),
    );
    let checklist = run_ao2_handoff_checklist(&handoff_path, &out, &["--allow-skipped"]);
    assert_eq!(checklist["status"], "planned");
}

#[test]
fn handoff_checklist_parity_with_factory_v3() {
    let factory_root = match std::env::var("FACTORY_V3_ROOT") {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => {
            let mut candidate = std::env::current_dir().expect("cwd");
            candidate.pop();
            candidate.push("factory-v3");
            candidate
        }
    };
    let script = factory_root.join("scripts/ao2_release_handoff_checklist.py");
    if !script.is_file() {
        eprintln!(
            "factory-v3 script not found at {}; skipping parity check",
            script.display()
        );
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let handoff_path = tmp.path().join("handoff.json");
    let ao2_out = tmp.path().join("ao2.json");
    let f3_out = tmp.path().join("factory-v3.json");
    write_json(&handoff_path, &ready_handoff());

    let _ = run_ao2_handoff_checklist(&handoff_path, &ao2_out, &[]);

    let py_status = Command::new("python3")
        .args([
            script.to_str().expect("utf8 script"),
            "--handoff",
            handoff_path.to_str().expect("utf8"),
            "--write-json",
            f3_out.to_str().expect("utf8"),
        ])
        .status()
        .expect("invoke factory-v3 handoff-checklist script");
    assert!(py_status.success(), "factory-v3 python producer failed");

    let ao2_value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&ao2_out).expect("ao2 out")).expect("ao2 json");
    let f3_value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&f3_out).expect("f3 out"))
            .expect("factory-v3 json");

    let ao2_text = serde_json::to_string(&ao2_value).expect("serialise");
    let f3_text = serde_json::to_string(&f3_value).expect("serialise");
    assert_eq!(
        ao2_text, f3_text,
        "AO2 handoff checklist must be byte-equal to factory-v3 (canonical)\nAO2:\n{ao2_text}\nfactory-v3:\n{f3_text}"
    );
}

#[test]
fn handoff_checklist_errors_on_missing_input_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let missing = tmp.path().join("does-not-exist.json");
    let out = tmp.path().join("checklist.json");
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let output = Command::new(ao2)
        .args([
            "release",
            "handoff-checklist-build",
            "--handoff",
            missing.to_str().expect("utf8"),
            "--write-json",
            out.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run ao2");
    assert!(
        !output.status.success(),
        "expected failure on missing input, got stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does-not-exist.json") || stderr.contains("No such file"),
        "expected helpful error mentioning missing path, got: {stderr}"
    );
}

#[test]
fn handoff_checklist_errors_on_malformed_json() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let handoff_path = tmp.path().join("malformed.json");
    let out = tmp.path().join("checklist.json");
    fs::write(&handoff_path, b"{this is not json").expect("write malformed");
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let output = Command::new(ao2)
        .args([
            "release",
            "handoff-checklist-build",
            "--handoff",
            handoff_path.to_str().expect("utf8"),
            "--write-json",
            out.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run ao2");
    assert!(
        !output.status.success(),
        "expected failure on malformed JSON, stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn handoff_checklist_markdown_includes_status_and_blockers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let handoff_path = tmp.path().join("handoff.json");
    let out_json = tmp.path().join("checklist.json");
    let out_md = tmp.path().join("checklist.md");
    let mut h = ready_handoff();
    h["gates"]["decision_signature"] = serde_json::Value::String("missing".into());
    write_json(&handoff_path, &h);

    let _ = run_ao2_handoff_checklist(
        &handoff_path,
        &out_json,
        &["--write-md", out_md.to_str().expect("utf8")],
    );
    let md = fs::read_to_string(&out_md).expect("checklist markdown written");
    assert!(
        md.contains("blocked"),
        "markdown should mention status, got:\n{md}"
    );
    assert!(
        md.contains("decision_signature"),
        "markdown should list the failing check, got:\n{md}"
    );
}

#[test]
fn handoff_checklist_repo_head_mismatch_flags_blocker() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let handoff_path = tmp.path().join("handoff.json");
    let out = tmp.path().join("checklist.json");
    let mut h = ready_handoff();
    h["release"]["repositories"] = serde_json::json!({
        "ao2": {"head": "abc123def456", "path": "/tmp/nonexistent-ao2"}
    });
    write_json(&handoff_path, &h);

    let checklist = run_ao2_handoff_checklist(
        &handoff_path,
        &out,
        &["--expected-repo-head", "ao2=000000000000"],
    );
    let checks = checklist["checks"].as_array().expect("checks");
    let repo_head = checks
        .iter()
        .find(|c| c["id"] == "repo_head_ao2")
        .expect("repo_head_ao2 check present");
    assert_eq!(repo_head["status"], "blocked");
    assert_eq!(checklist["status"], "blocked");
}
