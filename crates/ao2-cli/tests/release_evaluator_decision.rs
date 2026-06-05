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

fn readiness_bridge(status: &str, blockers: &[&str]) -> serde_json::Value {
    let blockers_json: Vec<serde_json::Value> = blockers
        .iter()
        .map(|s| serde_json::Value::String((*s).to_string()))
        .collect();
    serde_json::json!({
        "schema": "factory-v3/hermes-ao-bridge/v1",
        "action": "release-readiness-status",
        "status": status,
        "frontend_status": {
            "status": status,
            "release_version": "0.4.79",
            "release_tag": "v0.4.79",
            "gate_count": 8,
            "blocked_gate_count": blockers.len(),
            "blocker_count": blockers.len(),
            "factory_v3_evaluator_closer_required": true,
            "control_plane_approves_release": false,
            "next_action": "factory-v3 evaluator-closer may review this readiness summary",
        },
        "readiness_snapshot": {
            "schema_version": "ao2.cp-release-readiness.v1",
            "status": status,
            "release": {"version": "0.4.79", "release_tag": "v0.4.79"},
            "blockers": blockers_json,
            "operator_decision": {
                "factory_v3_evaluator_closer_required": true,
                "control_plane_approves_release": false,
            },
        },
        "links": {
            "release_readiness_json": "http://127.0.0.1:8744/api/v1/release/readiness.json",
            "release_candidate_handoff_json": "http://127.0.0.1:8744/api/v1/release/handoff.json",
        },
        "trust_boundary": {"mode": "release_readiness_read_only"},
    })
}

fn handoff_checklist(status: &str) -> serde_json::Value {
    let blocked = status != "ready_for_evaluator_closer";
    let observed = if blocked { "planned" } else { "live_complete" };
    let check_status = if blocked { "blocked" } else { "passed" };
    let blockers: Vec<serde_json::Value> = if blocked {
        vec![serde_json::Value::String(
            "provider_acceptance: expected live_complete, observed planned".to_string(),
        )]
    } else {
        Vec::new()
    };
    serde_json::json!({
        "schema": "factory-v3/ao2-release-handoff-checklist/v1",
        "status": status,
        "release": {"version": "0.4.79", "release_tag": "v0.4.79"},
        "checks": [
            {
                "id": "provider_acceptance",
                "label": "Provider acceptance",
                "observed": observed,
                "expected": "live_complete",
                "status": check_status,
            }
        ],
        "blockers": blockers,
        "operator_decision": {
            "factory_v3_evaluator_closer_required": true,
            "control_plane_approves_release": false,
        },
        "trust_boundary": {
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
        },
    })
}

fn support_bundle_status(
    status: &str,
    candidate_correlation: &str,
    version: &str,
) -> serde_json::Value {
    let missing_count: u64 = if status == "assembled" { 0 } else { 1 };
    serde_json::json!({
        "schema": "factory-v3/hermes-ao-bridge/v1",
        "action": "release-support-bundle-status",
        "frontend_status": {
            "status": status,
            "release_candidate_version": version,
            "release_tag": format!("v{version}"),
            "candidate_correlation": candidate_correlation,
            "required_artifact_count": 6,
            "missing_artifact_count": missing_count,
            "control_plane_approves_release": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "next_action": "factory-v3 evaluator-closer reviews this assembled same-candidate bundle",
        },
        "support_bundle_snapshot": {
            "schema_version": "ao2.cp-release-support-bundle.v1",
            "release_assembly": {
                "schema_version": "ao2.cp-release-assembly.v1",
                "status": status,
                "release_candidate_version": version,
                "release_tag": format!("v{version}"),
                "candidate_correlation": candidate_correlation,
                "required_artifacts": [
                    {"id": "release_publication", "status": "observed"},
                    {"id": "phase1_checklist", "status": "observed"},
                    {"id": "phase1_decision", "status": "observed"},
                    {"id": "three_os_smoke", "status": "observed"},
                    {
                        "id": "provider_acceptance_codex",
                        "status": "observed",
                        "release_candidate_version": version,
                    },
                    {
                        "id": "provider_acceptance_claude",
                        "status": "observed",
                        "release_candidate_version": version,
                    },
                ],
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_approves_release": false,
            },
        },
    })
}

fn run_ao2_evaluator_decision(
    readiness: &Path,
    checklist: &Path,
    support_bundle: &Path,
    out: &Path,
) -> serde_json::Value {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let output = Command::new(ao2)
        .args([
            "release",
            "evaluator-decision-build",
            "--readiness",
            readiness.to_str().expect("utf8"),
            "--handoff-checklist",
            checklist.to_str().expect("utf8"),
            "--support-bundle-status",
            support_bundle.to_str().expect("utf8"),
            "--write-json",
            out.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run ao2 release evaluator-decision-build");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("evaluator decision is valid json")
}

#[test]
fn evaluator_decision_accepts_ready_release() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    let checklist = tmp.path().join("checklist.json");
    let support = tmp.path().join("support.json");
    let out = tmp.path().join("decision.json");
    write_json(&readiness, &readiness_bridge("ready", &[]));
    write_json(&checklist, &handoff_checklist("ready_for_evaluator_closer"));
    write_json(
        &support,
        &support_bundle_status("assembled", "matched", "0.4.79"),
    );

    let decision = run_ao2_evaluator_decision(&readiness, &checklist, &support, &out);
    assert_eq!(
        decision["schema"],
        "factory-v3/ao2-release-evaluator-decision/v1"
    );
    assert_eq!(decision["status"], "accepted");
    assert_eq!(decision["decision"], "accept_phase1_release_candidate");
    assert_eq!(
        decision["blockers"]
            .as_array()
            .expect("blockers array")
            .len(),
        0
    );
    assert_eq!(
        decision["trust_boundary"]["control_plane_approves_release"],
        false
    );
    assert_eq!(
        decision["trust_boundary"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    let on_disk = fs::read_to_string(&out).expect("decision written");
    assert!(on_disk.contains("\"status\": \"accepted\""));
}

#[test]
fn evaluator_decision_rejects_attention_release() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    let checklist = tmp.path().join("checklist.json");
    let support = tmp.path().join("support.json");
    let out = tmp.path().join("decision.json");
    write_json(
        &readiness,
        &readiness_bridge(
            "attention",
            &["something_blocked: expected ready, observed attention"],
        ),
    );
    write_json(&checklist, &handoff_checklist("blocked"));
    write_json(
        &support,
        &support_bundle_status("attention", "mismatched", "0.4.79"),
    );

    let decision = run_ao2_evaluator_decision(&readiness, &checklist, &support, &out);
    assert_eq!(decision["status"], "rejected");
    assert_eq!(decision["decision"], "reject_phase1_release_candidate");
    let blockers = decision["blockers"].as_array().expect("blockers array");
    assert!(
        !blockers.is_empty(),
        "expected at least one blocker, got {decision:?}"
    );
}

#[test]
fn evaluator_decision_self_reference_exception_applies() {
    // Construct the case where readiness/checklist/support-bundle are all
    // blocked solely because the evaluator decision itself is missing —
    // the self-reference exception should fire and the decision should
    // be accepted, with applicable checks reporting
    // `passed_pending_self_reference`.
    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    let checklist = tmp.path().join("checklist.json");
    let support = tmp.path().join("support.json");
    let out = tmp.path().join("decision.json");

    let readiness_payload = serde_json::json!({
        "schema_version": "ao2.cp-release-readiness.v1",
        "status": "attention",
        "release": {"version": "0.4.79", "release_tag": "v0.4.79"},
        "blockers": [
            "release_evaluator_decision: expected accepted, observed missing",
        ],
        "operator_decision": {
            "factory_v3_evaluator_closer_required": true,
            "control_plane_approves_release": false,
        },
    });

    let checklist_payload = serde_json::json!({
        "schema": "factory-v3/ao2-release-handoff-checklist/v1",
        "status": "blocked",
        "release": {"version": "0.4.79", "release_tag": "v0.4.79"},
        "checks": [],
        "blockers": [
            "handoff_status: expected ready, observed attention",
        ],
        "operator_decision": {
            "factory_v3_evaluator_closer_required": true,
            "control_plane_approves_release": false,
        },
    });

    let support_payload = serde_json::json!({
        "schema_version": "ao2.cp-release-support-bundle.v1",
        "frontend_status": {
            "status": "attention",
            "release_candidate_version": "0.4.79",
            "missing_artifact_count": 0,
        },
        "release_assembly": {
            "schema_version": "ao2.cp-release-assembly.v1",
            "status": "attention",
            "release_candidate_version": "0.4.79",
            "candidate_correlation": "mismatched",
            "candidate_correlation_detail": {
                "blockers": [
                    "candidate_correlation: expected matched, observed mismatched",
                ],
                "release_version": "0.4.79",
                "release_tag": "v0.4.79",
                "codex_acceptance_version": "0.4.79",
                "claude_acceptance_version": "0.4.79",
                "three_os_version": "0.4.79",
                "release_evaluator_version": "unknown",
                "release_evaluator_tag": "unknown",
            },
            "control_plane_approves_release": false,
        },
    });

    write_json(&readiness, &readiness_payload);
    write_json(&checklist, &checklist_payload);
    write_json(&support, &support_payload);

    let decision = run_ao2_evaluator_decision(&readiness, &checklist, &support, &out);
    assert_eq!(
        decision["self_reference_exception"]["status"], "applied",
        "expected self-reference exception to apply, got {decision:?}"
    );
    assert_eq!(decision["status"], "accepted");
    // The exception causes blocked checks to be reported as
    // `passed_pending_self_reference` instead of `blocked`.
    let checks = decision["checks"].as_array().expect("checks");
    assert!(checks
        .iter()
        .any(|c| c["status"] == "passed_pending_self_reference"));
}

#[test]
fn evaluator_decision_blocks_when_control_plane_approves() {
    // Trust boundary: control plane must never approve release. If a
    // payload claims otherwise, the decision MUST be rejected even when
    // every other check passes.
    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    let checklist = tmp.path().join("checklist.json");
    let support = tmp.path().join("support.json");
    let out = tmp.path().join("decision.json");

    let mut readiness_payload = readiness_bridge("ready", &[]);
    readiness_payload["readiness_snapshot"]["operator_decision"]
        ["control_plane_approves_release"] = serde_json::Value::Bool(true);
    write_json(&readiness, &readiness_payload);
    write_json(&checklist, &handoff_checklist("ready_for_evaluator_closer"));
    write_json(
        &support,
        &support_bundle_status("assembled", "matched", "0.4.79"),
    );

    let decision = run_ao2_evaluator_decision(&readiness, &checklist, &support, &out);
    assert_eq!(decision["status"], "rejected");
    let blockers: Vec<String> = decision["blockers"]
        .as_array()
        .expect("blockers array")
        .iter()
        .map(|v| v.as_str().expect("blocker is string").to_string())
        .collect();
    assert!(
        blockers
            .iter()
            .any(|b| b.contains("control plane must not approve release")),
        "expected trust_boundary blocker, got {blockers:?}"
    );
}

#[test]
fn evaluator_decision_parity_with_factory_v3() {
    // Byte-equal parity (under canonical JSON sort) against the
    // factory-v3 Python producer, when the script is available. The
    // factory-v3 script is the read-only audit oracle; AO2 is the
    // canonical producer.
    let factory_root = match std::env::var("FACTORY_V3_ROOT") {
        Ok(path) => std::path::PathBuf::from(path),
        Err(_) => {
            // Default sibling layout: ../factory-v3 relative to repo root
            let mut candidate = std::env::current_dir().expect("cwd");
            candidate.pop();
            candidate.push("factory-v3");
            candidate
        }
    };
    let script = factory_root.join("scripts/ao2_release_evaluator_decision.py");
    if !script.is_file() {
        eprintln!(
            "factory-v3 script not found at {}; skipping parity check",
            script.display()
        );
        return;
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    let checklist = tmp.path().join("checklist.json");
    let support = tmp.path().join("support.json");
    let ao2_out = tmp.path().join("ao2.json");
    let f3_out = tmp.path().join("factory-v3.json");

    write_json(&readiness, &readiness_bridge("ready", &[]));
    write_json(&checklist, &handoff_checklist("ready_for_evaluator_closer"));
    write_json(
        &support,
        &support_bundle_status("assembled", "matched", "0.4.79"),
    );

    // AO2 producer
    let _ = run_ao2_evaluator_decision(&readiness, &checklist, &support, &ao2_out);

    // factory-v3 Python producer (read-only audit role)
    let py_status = Command::new("python3")
        .args([
            script.to_str().expect("utf8 script"),
            "--readiness",
            readiness.to_str().expect("utf8"),
            "--handoff-checklist",
            checklist.to_str().expect("utf8"),
            "--support-bundle-status",
            support.to_str().expect("utf8"),
            "--write-json",
            f3_out.to_str().expect("utf8"),
        ])
        .status()
        .expect("invoke factory-v3 evaluator decision script");
    assert!(py_status.success(), "factory-v3 python producer failed");

    let ao2_value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&ao2_out).expect("read ao2 out"))
            .expect("ao2 json");
    let f3_value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&f3_out).expect("read f3 out"))
            .expect("factory-v3 json");

    // Strip the `evidence` block before parity comparison: it contains
    // the absolute input paths, which differ between AO2 and factory-v3
    // invocations because each is given its own copy of the fixtures.
    let mut ao2_canonical = ao2_value.clone();
    let mut f3_canonical = f3_value.clone();
    ao2_canonical
        .as_object_mut()
        .expect("object")
        .remove("evidence");
    f3_canonical
        .as_object_mut()
        .expect("object")
        .remove("evidence");

    let ao2_text = serde_json::to_string(&ao2_canonical).expect("serialise");
    let f3_text = serde_json::to_string(&f3_canonical).expect("serialise");
    assert_eq!(
        ao2_text, f3_text,
        "AO2 producer must be byte-equal to factory-v3 Python producer (canonical)\nAO2:\n{ao2_text}\nfactory-v3:\n{f3_text}"
    );
}

#[test]
fn evaluator_decision_errors_on_missing_readiness_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let missing = tmp.path().join("does-not-exist.json");
    let checklist = tmp.path().join("checklist.json");
    let support = tmp.path().join("support.json");
    let out = tmp.path().join("decision.json");
    write_json(&checklist, &handoff_checklist("ready_for_evaluator_closer"));
    write_json(
        &support,
        &support_bundle_status("assembled", "matched", "0.4.79"),
    );

    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let output = Command::new(ao2)
        .args([
            "release",
            "evaluator-decision-build",
            "--readiness",
            missing.to_str().expect("utf8"),
            "--handoff-checklist",
            checklist.to_str().expect("utf8"),
            "--support-bundle-status",
            support.to_str().expect("utf8"),
            "--write-json",
            out.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run ao2");
    assert!(
        !output.status.success(),
        "expected failure when readiness file is missing, stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does-not-exist.json") || stderr.contains("No such file"),
        "expected helpful error mentioning missing path, got: {stderr}"
    );
}

#[test]
fn evaluator_decision_errors_on_malformed_readiness_json() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    let checklist = tmp.path().join("checklist.json");
    let support = tmp.path().join("support.json");
    let out = tmp.path().join("decision.json");
    fs::write(&readiness, b"{not valid json at all").expect("write malformed");
    write_json(&checklist, &handoff_checklist("ready_for_evaluator_closer"));
    write_json(
        &support,
        &support_bundle_status("assembled", "matched", "0.4.79"),
    );

    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let output = Command::new(ao2)
        .args([
            "release",
            "evaluator-decision-build",
            "--readiness",
            readiness.to_str().expect("utf8"),
            "--handoff-checklist",
            checklist.to_str().expect("utf8"),
            "--support-bundle-status",
            support.to_str().expect("utf8"),
            "--write-json",
            out.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run ao2");
    assert!(
        !output.status.success(),
        "expected failure on malformed readiness JSON, stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn evaluator_decision_emits_evidence_block_with_input_paths() {
    // Regression: the decision JSON MUST include an `evidence` block that
    // records the absolute input paths so downstream auditors can trace
    // back to the readiness / checklist / support-bundle inputs.
    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    let checklist = tmp.path().join("checklist.json");
    let support = tmp.path().join("support.json");
    let out = tmp.path().join("decision.json");
    write_json(&readiness, &readiness_bridge("ready", &[]));
    write_json(&checklist, &handoff_checklist("ready_for_evaluator_closer"));
    write_json(
        &support,
        &support_bundle_status("assembled", "matched", "0.4.79"),
    );

    let decision = run_ao2_evaluator_decision(&readiness, &checklist, &support, &out);
    let evidence = decision["evidence"]
        .as_object()
        .expect("evidence block present");
    assert!(!evidence.is_empty(), "evidence block must not be empty");
    let evidence_text = serde_json::to_string(evidence).expect("serialise evidence");
    let readiness_str = readiness.to_str().expect("utf8");
    let checklist_str = checklist.to_str().expect("utf8");
    let support_str = support.to_str().expect("utf8");
    assert!(
        evidence_text.contains(readiness_str)
            || evidence_text.contains(
                readiness
                    .file_name()
                    .expect("filename")
                    .to_str()
                    .expect("utf8"),
            ),
        "evidence should reference the readiness input path, got: {evidence_text}"
    );
    assert!(
        evidence_text.contains(checklist_str)
            || evidence_text.contains(
                checklist
                    .file_name()
                    .expect("filename")
                    .to_str()
                    .expect("utf8"),
            ),
        "evidence should reference the checklist input path, got: {evidence_text}"
    );
    assert!(
        evidence_text.contains(support_str)
            || evidence_text.contains(
                support
                    .file_name()
                    .expect("filename")
                    .to_str()
                    .expect("utf8"),
            ),
        "evidence should reference the support-bundle input path, got: {evidence_text}"
    );
}

#[test]
fn evaluator_decision_markdown_records_verdict_and_blockers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let readiness = tmp.path().join("readiness.json");
    let checklist = tmp.path().join("checklist.json");
    let support = tmp.path().join("support.json");
    let out = tmp.path().join("decision.json");
    let out_md = tmp.path().join("decision.md");
    write_json(
        &readiness,
        &readiness_bridge(
            "attention",
            &["provider_acceptance: expected ready, observed attention"],
        ),
    );
    write_json(&checklist, &handoff_checklist("blocked"));
    write_json(
        &support,
        &support_bundle_status("attention", "mismatched", "0.4.79"),
    );

    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let output = Command::new(ao2)
        .args([
            "release",
            "evaluator-decision-build",
            "--readiness",
            readiness.to_str().expect("utf8"),
            "--handoff-checklist",
            checklist.to_str().expect("utf8"),
            "--support-bundle-status",
            support.to_str().expect("utf8"),
            "--write-json",
            out.to_str().expect("utf8"),
            "--write-md",
            out_md.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run ao2");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let md = fs::read_to_string(&out_md).expect("decision markdown written");
    assert!(
        md.contains("rejected") || md.contains("reject"),
        "markdown should record the rejection verdict, got:\n{md}"
    );
    assert!(
        md.contains("provider_acceptance"),
        "markdown should list the failing blocker, got:\n{md}"
    );
}
