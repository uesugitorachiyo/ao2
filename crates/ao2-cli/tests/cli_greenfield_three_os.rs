use std::fs;
use std::process::Command;

#[test]
fn cli_greenfield_three_os_smoke_gate_accepts_macos_ubuntu_windows_governed_runs() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("greenfield-three-os-gate.json");
    let write_governed_run = |os_label: &str| {
        let path = temp
            .path()
            .join(format!("{os_label}-greenfield-governed-run.json"));
        fs::write(
            &path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "ao2.greenfield-governed-run.v1",
                "status": "accepted",
                "run_id": format!("greenfield-{os_label}"),
                "artifacts": {
                    "greenfield_ingest": format!("target/{os_label}/greenfield-ingest.json"),
                    "plan": format!("target/{os_label}/plan.json"),
                    "governed_run": format!("target/{os_label}/governed-run.json"),
                    "packed_evidence": format!("target/{os_label}/evidence-pack.json"),
                    "evaluator_decision": format!("target/{os_label}/evaluator-decision.json"),
                    "greenfield_governed_run": path.display().to_string()
                },
                "greenfield_governed_run_checklist": {
                    "ao2_ingested_plain_spec": true,
                    "ao2_generated_work_request": true,
                    "ao2_generated_runspec": true,
                    "ao2_executed_generated_governed_plan": true,
                    "ao2_verified_primary_run_result": true,
                    "ao2_packed_primary_evidence": true,
                    "ao2_signed_evaluator_closure": true,
                    "factory_v3_drives_workflow": false,
                    "factory_v3_role": "parity_oracle_only",
                    "control_plane_role": "read_only_observer_after_signed_evidence"
                },
                "trust_boundary": {
                    "execution_owner": "ao2",
                    "release_acceptance_owner": "factory-v3 evaluator-closer",
                    "factory_v3_role": "parity_oracle_only",
                    "control_plane_role": "read_only_observer_after_signed_evidence",
                    "control_plane_approves_release": false,
                    "mutates_ao_artifacts": false,
                    "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        path
    };
    let macos = write_governed_run("macos");
    let ubuntu = write_governed_run("ubuntu");
    let windows = write_governed_run("windows");
    let macos_arg = format!("macos={}", macos.display());
    let ubuntu_arg = format!("ubuntu={}", ubuntu.display());
    let windows_arg = format!("windows={}", windows.display());

    let gate = ao2([
        "greenfield",
        "three-os-smoke-gate",
        "--smoke",
        macos_arg.as_str(),
        "--smoke",
        ubuntu_arg.as_str(),
        "--smoke",
        windows_arg.as_str(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(gate.status.success(), "{}", stderr(&gate));
    let json: serde_json::Value = serde_json::from_str(&stdout(&gate)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.greenfield-three-os-smoke-gate.v1"
    );
    assert_eq!(json["status"], "accepted");
    assert_eq!(
        json["accepted_os"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["ao2_decision_owner"],
        "ao2-native-greenfield-three-os-smoke-gate"
    );
    assert_eq!(json["factory_v3_role"], "parity_oracle_only");
    assert_eq!(
        json["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert!(out.is_file());
}

#[test]
fn cli_greenfield_three_os_smoke_gate_rejects_control_plane_release_approval() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("greenfield-three-os-gate.json");
    let write_governed_run = |os_label: &str, control_plane_approves_release: bool| {
        let path = temp
            .path()
            .join(format!("{os_label}-greenfield-governed-run.json"));
        fs::write(
            &path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": "ao2.greenfield-governed-run.v1",
                "status": "accepted",
                "run_id": format!("greenfield-{os_label}"),
                "greenfield_governed_run_checklist": {
                    "ao2_ingested_plain_spec": true,
                    "ao2_generated_work_request": true,
                    "ao2_generated_runspec": true,
                    "ao2_executed_generated_governed_plan": true,
                    "ao2_verified_primary_run_result": true,
                    "ao2_packed_primary_evidence": true,
                    "ao2_signed_evaluator_closure": true,
                    "factory_v3_drives_workflow": false,
                    "factory_v3_role": "parity_oracle_only",
                    "control_plane_role": "read_only_observer_after_signed_evidence"
                },
                "trust_boundary": {
                    "execution_owner": "ao2",
                    "release_acceptance_owner": "factory-v3 evaluator-closer",
                    "factory_v3_role": "parity_oracle_only",
                    "control_plane_role": "read_only_observer_after_signed_evidence",
                    "control_plane_approves_release": control_plane_approves_release,
                    "mutates_ao_artifacts": false,
                    "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        path
    };
    let macos = write_governed_run("macos", false);
    let ubuntu = write_governed_run("ubuntu", false);
    let windows = write_governed_run("windows", true);
    let macos_arg = format!("macos={}", macos.display());
    let ubuntu_arg = format!("ubuntu={}", ubuntu.display());
    let windows_arg = format!("windows={}", windows.display());

    let gate = ao2([
        "greenfield",
        "three-os-smoke-gate",
        "--smoke",
        macos_arg.as_str(),
        "--smoke",
        ubuntu_arg.as_str(),
        "--smoke",
        windows_arg.as_str(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !gate.status.success(),
        "gate must reject control-plane approval drift"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&gate)).unwrap();
    assert_eq!(json["status"], "rejected");
    assert_eq!(json["accepted_os"], serde_json::json!(["macos", "ubuntu"]));
    let windows = json["per_os"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["os"] == "windows")
        .unwrap();
    assert_eq!(windows["status"], "rejected");
    assert!(windows["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason
            .as_str()
            .unwrap_or("")
            .contains("control_plane_approves_release must be false")));
}

fn ao2<const N: usize>(args: [&str; N]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
    command.args(args);
    command.env("AO2_AUTO_APPROVE_SANDBOX_PATCH", "1");
    command.env(
        "AO2_AUTO_APPROVE_SANDBOX_PATCH_APPROVER",
        "human:test-auto-approve",
    );
    command.env_remove("OPENAI_API_KEY");
    command.env_remove("ANTHROPIC_API_KEY");
    command.output().unwrap()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}
