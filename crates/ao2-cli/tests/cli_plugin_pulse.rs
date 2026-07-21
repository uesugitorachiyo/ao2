use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

fn sha256_path(path: &Path) -> String {
    let body = fs::read(path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(body);
    format!("{:x}", hasher.finalize())
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

fn write_pulse_apply_result_fixture(root: &Path, platform: &str) -> PathBuf {
    let apply_result = root.join(platform).join("pulse-apply-result.json");
    fs::create_dir_all(apply_result.parent().unwrap()).unwrap();
    fs::write(
        &apply_result,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-apply-result.v1",
            "status": "accepted",
            "execution_mode": "bounded_planned_file_apply",
            "generated_at": "2026-06-02T18:00:00Z",
            "selected_task": {
                "id": "pulse-plugin-readiness-apply",
                "title": "Apply plugin readiness maintenance through AO2",
                "classification": "COMPLEX",
                "shape": "greenfield",
                "c85": false
            },
            "dry_run_task": {
                "path": format!("target/{platform}/pulse-dry-run-task.json"),
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "schema_version": "ao2.pulse-dry-run-task.v1",
                "status": "planned_without_mutation"
            },
            "prior_chain": {
                "path": format!("target/{platform}/pulse-chain.json"),
                "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "schema_version": "ao2.pulse-chain.v1",
                "status": "planned_without_execution"
            },
            "task_contract": {
                "path": format!("target/{platform}/task-contract.json"),
                "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                "schema_version": "ao2.pulse-task-contract.v1",
                "id": "pulse-plugin-readiness-apply"
            },
            "governed_task_evidence": {
                "path": format!("target/{platform}/pulse-governed-task.json"),
                "sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                "schema_version": "ao2.pulse-governed-task.v1",
                "status": "accepted"
            },
            "task_result": {
                "path": format!("target/{platform}/pulse-task-result.json"),
                "sha256": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                "schema_version": "ao2.pulse-task-result.v1",
                "status": "accepted"
            },
            "evaluator_closer": {
                "status": "accepted",
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "evaluator_decision": "accept_non_c85_governed_task",
                "closer_decision": "accepted",
                "evidence_digest_required": true
            },
            "applied_file_operations": [
                {
                    "operation": "inspect_current_plugin_readiness_line",
                    "path": "docs/PLUGIN-SHIPMENT-RUNBOOK.md",
                    "mode": "applied",
                    "executed": true,
                    "allowed_by_dry_run": true
                }
            ],
            "trust_boundary": {
                "ao2_execution_evidence_owner": true,
                "factory_v3_evaluator_closer_reference": true,
                "hermes_frontend_queue_memory_surface": true,
                "ao2_control_plane_read_only_observer": true,
                "control_plane_observer_only": true,
                "control_plane_approves_release": false,
                "control_plane_mutates_ao_artifacts": false
            },
            "side_effects": {
                "provider_execution": false,
                "queue_execution": false,
                "memory_write": false,
                "mutates_ao_artifacts": false,
                "hermes_cron_watchdog_mutation": false,
                "control_plane_mutation": false
            }
        }))
        .unwrap(),
    )
    .unwrap();
    apply_result
}

fn write_pulse_executor_fixture(root: &Path, platform: &str) -> PathBuf {
    let platform_root = root.join(platform).join("executor");
    fs::create_dir_all(&platform_root).unwrap();

    let contract_path = platform_root.join("pulse-task-contract.json");
    fs::write(
        &contract_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-task-contract.v1",
            "id": "pulse-executor-observer-refresh",
            "title": "Refresh current Pulse executor observer evidence",
            "classification": "COMPLEX",
            "shape": "refactor",
            "c85": false,
            "ao2_owned_execution": true,
            "factory_v3_evaluator_closer_required": true
        }))
        .unwrap(),
    )
    .unwrap();
    let contract_sha256 = sha256_path(&contract_path);

    let governed_task = platform_root.join("pulse-governed-task.json");
    fs::write(
        &governed_task,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-governed-task.v1",
            "status": "accepted",
            "generated_at": "2026-06-03T04:23:17Z",
            "selected_task": {
                "id": "pulse-executor-observer-refresh",
                "title": "Refresh current Pulse executor observer evidence",
                "classification": "COMPLEX",
                "shape": "refactor",
                "c85": false
            },
            "task_contract": {
                "schema_version": "ao2.pulse-task-contract.v1",
                "id": "pulse-executor-observer-refresh",
                "path": contract_path.display().to_string(),
                "sha256": contract_sha256
            },
            "executed_task": {
                "id": "pulse-executor-observer-refresh",
                "title": "Refresh current Pulse executor observer evidence",
                "status": "executed",
                "execution_kind": "governed_task_contract",
                "factory_v3_evaluator_closer_required": true,
                "c85": false,
                "provider_execution": false,
                "queue_execution": false,
                "memory_write": false,
                "mutates_ao_artifacts": false,
                "evaluator_closer": {
                    "status": "accepted",
                    "release_acceptance_owner": "factory-v3 evaluator-closer",
                    "evaluator_decision": "accept_non_c85_governed_task",
                    "closer_decision": "accepted",
                    "evidence_digest_required": true
                }
            },
            "c85": {
                "status": "passed",
                "reason": "prior chain evidence records hosted C85 Release Gate passed before Pulse execute evidence",
                "hosted_github_actions_checked": true,
                "rerun_allowed_without_user_billing_fix": true
            },
            "trust_boundary": {
                "ao2_execution_evidence_owner": true,
                "factory_v3_evaluator_closer_reference": true,
                "hermes_frontend_queue_memory_surface": true,
                "ao2_control_plane_read_only_observer": true,
                "control_plane_observer_only": true,
                "control_plane_approves_release": false,
                "control_plane_mutates_ao_artifacts": false
            },
            "side_effects": {
                "provider_execution": false,
                "queue_execution": false,
                "memory_write": false,
                "mutates_ao_artifacts": false,
                "hermes_cron_watchdog_mutation": false,
                "control_plane_mutation": false
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let governed_task_sha256 = sha256_path(&governed_task);

    let task_result = platform_root.join("pulse-task-result.json");
    fs::write(
        &task_result,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-task-result.v1",
            "status": "accepted",
            "execution_mode": "deterministic_local_evidence",
            "generated_at": "2026-06-03T04:23:17Z",
            "selected_task": {
                "id": "pulse-executor-observer-refresh",
                "title": "Refresh current Pulse executor observer evidence",
                "classification": "COMPLEX",
                "shape": "refactor",
                "c85": false
            },
            "prior_chain": {
                "path": format!("target/{platform}/pulse-chain.json"),
                "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "schema_version": "ao2.pulse-chain.v1",
                "status": "planned_without_execution"
            },
            "c85": {
                "status": "passed",
                "reason": "prior chain evidence records hosted C85 Release Gate passed before Pulse execute evidence",
                "hosted_github_actions_checked": true,
                "rerun_allowed_without_user_billing_fix": true
            },
            "task_contract": {
                "schema_version": "ao2.pulse-task-contract.v1",
                "id": "pulse-executor-observer-refresh",
                "path": contract_path.display().to_string(),
                "sha256": contract_sha256
            },
            "governed_task_evidence": {
                "path": governed_task.display().to_string(),
                "sha256": governed_task_sha256,
                "schema_version": "ao2.pulse-governed-task.v1",
                "status": "accepted"
            },
            "evaluator_closer": {
                "status": "accepted",
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "evaluator_decision": "accept_non_c85_governed_task",
                "closer_decision": "accepted",
                "evidence_digest_required": true
            },
            "trust_boundary": {
                "ao2_execution_evidence_owner": true,
                "factory_v3_evaluator_closer_reference": true,
                "hermes_frontend_queue_memory_surface": true,
                "ao2_control_plane_read_only_observer": true,
                "control_plane_observer_only": true,
                "control_plane_approves_release": false,
                "control_plane_mutates_ao_artifacts": false
            },
            "side_effects": {
                "provider_execution": false,
                "queue_execution": false,
                "memory_write": false,
                "mutates_ao_artifacts": false,
                "hermes_cron_watchdog_mutation": false,
                "control_plane_mutation": false
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let task_result_sha256 = sha256_path(&task_result);

    let executor = platform_root.join("pulse-executor.json");
    fs::write(
        &executor,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-executor.v1",
            "status": "executed_governed_task",
            "generated_at": "2026-06-03T04:23:17Z",
            "selected_task": {
                "id": "pulse-executor-observer-refresh",
                "title": "Refresh current Pulse executor observer evidence",
                "classification": "COMPLEX",
                "shape": "refactor",
                "c85": false
            },
            "prior_chain": {
                "path": format!("target/{platform}/pulse-chain.json"),
                "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "schema_version": "ao2.pulse-chain.v1",
                "status": "planned_without_execution"
            },
            "c85": {
                "status": "passed",
                "reason": "prior chain evidence records hosted C85 Release Gate passed before Pulse execute evidence",
                "hosted_github_actions_checked": true,
                "rerun_allowed_without_user_billing_fix": true
            },
            "task_contract": {
                "schema_version": "ao2.pulse-task-contract.v1",
                "id": "pulse-executor-observer-refresh",
                "path": contract_path.display().to_string(),
                "sha256": contract_sha256
            },
            "artifacts": {
                "governed_task_evidence": governed_task.display().to_string(),
                "governed_task_evidence_sha256": governed_task_sha256,
                "pulse_task_result": task_result.display().to_string(),
                "pulse_task_result_sha256": task_result_sha256
            },
            "trust_boundary": {
                "ao2_execution_evidence_owner": true,
                "factory_v3_evaluator_closer_reference": true,
                "hermes_frontend_queue_memory_surface": true,
                "ao2_control_plane_read_only_observer": true,
                "control_plane_observer_only": true,
                "control_plane_approves_release": false,
                "control_plane_mutates_ao_artifacts": false
            },
            "side_effects": {
                "provider_execution": false,
                "queue_execution": false,
                "memory_write": false,
                "mutates_ao_artifacts": false,
                "hermes_cron_watchdog_mutation": false,
                "control_plane_mutation": false
            }
        }))
        .unwrap(),
    )
    .unwrap();
    executor
}

fn write_pulse_once_fixture(root: &Path, platform: &str) -> PathBuf {
    let platform_root = root.join(platform).join("once");
    fs::create_dir_all(&platform_root).unwrap();
    let packet = platform_root.join("prompt.txt");
    fs::write(
        &packet,
        "C85 hosted GitHub Actions passed; AO2 Pulse post-C85/plugin-ready once evidence is next.",
    )
    .unwrap();
    let board = platform_root.join("BOARD.md");
    fs::write(
        &board,
        format!(
            "AO2 Pulse post-C85 once-mode evidence for {platform}; Windows progress must be explicit."
        ),
    )
    .unwrap();
    let packet_sha256 = sha256_path(&packet);
    let board_sha256 = sha256_path(&board);
    let once = platform_root.join("pulse-once.json");
    fs::write(
        &once,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-once.v1",
            "status": "ready_for_operator_execution",
            "generated_at": "2026-06-04T14:05:39Z",
            "scheduler": {
                "active_runner": "codex-cron",
                "hermes_frontend_queue_memory_concept": true,
                "hermes_cron_mutated": false,
                "fixed_interval_loop_successor": "ao2 pulse run --once"
            },
            "observed_inputs": {
                "packet": packet.display().to_string(),
                "packet_sha256": packet_sha256,
                "packet_mentions_c85_deferred": false,
                "board": board.display().to_string(),
                "board_sha256": board_sha256,
                "board_mentions_pulse": true
            },
            "selected_task": {
                "id": "ao2-pulse-next-safe-task",
                "title": "AO2 Pulse once-mode replacement slice",
                "classification": "COMPLEX",
                "shape": "greenfield",
                "reason": "Post-C85 plugin/K37 observer coverage is current; the next safe AO2 advancement is read-only Pulse once-mode evidence.",
                "recommended_command": "ao2 pulse run --once --packet <active-packet> --board <coordination-board> --out-dir <evidence-dir> --json"
            },
            "c85": {
                "status": "passed",
                "reason": "hosted C85 Release Gate passed before post-C85/plugin-ready Pulse once evidence",
                "hosted_github_actions_checked": true,
                "rerun_allowed_without_user_billing_fix": true
            },
            "trust_boundary": {
                "ao2_execution_evidence_owner": true,
                "factory_v3_evaluator_closer_reference": true,
                "hermes_frontend_queue_memory_surface": true,
                "ao2_control_plane_read_only_observer": true,
                "control_plane_observer_only": true,
                "control_plane_approves_release": false,
                "control_plane_mutates_ao_artifacts": false
            },
            "side_effects": {
                "provider_execution": false,
                "queue_execution": false,
                "memory_write": false,
                "mutates_ao_artifacts": false,
                "hermes_cron_watchdog_mutation": false,
                "control_plane_mutation": false
            },
            "artifacts": {
                "pulse_once": once.display().to_string()
            }
        }))
        .unwrap(),
    )
    .unwrap();
    once
}

fn write_pulse_chain_fixture(root: &Path, platform: &str) -> PathBuf {
    let platform_root = root.join(platform).join("chain");
    fs::create_dir_all(&platform_root).unwrap();
    let packet = platform_root.join("prompt.txt");
    fs::write(
        &packet,
        "Hosted C85 Release Gate passed on 2026-06-04; AO2 Pulse post-C85/plugin-ready chain evidence is next.",
    )
    .unwrap();
    let board = platform_root.join("BOARD.md");
    fs::write(
        &board,
        format!(
            "AO2 Pulse post-C85 chain-mode evidence for {platform}; Windows progress must be explicit."
        ),
    )
    .unwrap();
    let once = write_pulse_once_fixture(root, platform);
    let once_sha256 = sha256_path(&once);
    let packet_sha256 = sha256_path(&packet);
    let board_sha256 = sha256_path(&board);
    let chain = platform_root.join("pulse-chain.json");
    fs::write(
        &chain,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-chain.v1",
            "status": "planned_without_execution",
            "generated_at": "2026-06-04T14:55:15Z",
            "scheduler": {
                "active_runner": "codex-cron",
                "hermes_frontend_queue_memory_concept": true,
                "hermes_cron_mutated": false,
                "fixed_interval_loop_successor": "ao2 pulse run --chain"
            },
            "observed_inputs": {
                "packet": packet.display().to_string(),
                "packet_sha256": packet_sha256,
                "packet_mentions_c85_deferred": false,
                "packet_mentions_c85_passed": true,
                "board": board.display().to_string(),
                "board_sha256": board_sha256
            },
            "prior_once": {
                "path": once.display().to_string(),
                "sha256": once_sha256,
                "schema_version": "ao2.pulse-once.v1",
                "status": "ready_for_operator_execution",
                "c85": {
                    "status": "passed",
                    "hosted_github_actions_checked": true,
                    "rerun_allowed_without_user_billing_fix": true
                }
            },
            "chain_steps": [
                {
                    "id": "observe-pulse-once-and-select-next-safe-task",
                    "status": "planned",
                    "executes_task": false,
                    "reason": "Chain mode records the once-mode decision and prepares a bounded evaluator-closed next step without executing it."
                },
                {
                    "id": "prepare-operator-handoff",
                    "status": "planned",
                    "executes_task": false,
                    "reason": "A governed follow-on may execute the selected AO2 task after reviewing this post-C85 evidence."
                }
            ],
            "c85": {
                "status": "passed",
                "reason": "hosted C85 Release Gate passed before post-C85/plugin-ready Pulse chain evidence",
                "hosted_github_actions_checked": true,
                "rerun_allowed_without_user_billing_fix": true
            },
            "trust_boundary": {
                "ao2_execution_evidence_owner": true,
                "factory_v3_evaluator_closer_reference": true,
                "hermes_frontend_queue_memory_surface": true,
                "ao2_control_plane_read_only_observer": true,
                "control_plane_observer_only": true,
                "control_plane_approves_release": false,
                "control_plane_mutates_ao_artifacts": false
            },
            "side_effects": {
                "provider_execution": false,
                "queue_execution": false,
                "memory_write": false,
                "mutates_ao_artifacts": false,
                "hermes_cron_watchdog_mutation": false,
                "control_plane_mutation": false
            },
            "artifacts": {
                "pulse_chain": chain.display().to_string()
            }
        }))
        .unwrap(),
    )
    .unwrap();
    chain
}

fn write_pulse_eval_loop_fixture(root: &Path, platform: &str) -> PathBuf {
    let platform_root = root.join(platform).join("eval-loop");
    fs::create_dir_all(&platform_root).unwrap();
    let packet = platform_root.join("prompt.txt");
    fs::write(
        &packet,
        "Hosted C85 Release Gate passed; AO2 Pulse eval-loop chain should recommend only.",
    )
    .unwrap();
    let board = platform_root.join("BOARD.md");
    fs::write(
        &board,
        format!(
            "AO2 Pulse eval-loop chain evidence for {platform}; Windows progress must be explicit."
        ),
    )
    .unwrap();
    let packet_sha256 = sha256_path(&packet);
    let board_sha256 = sha256_path(&board);
    let eval_loop = platform_root.join("pulse-eval-loop.json");
    fs::write(
        &eval_loop,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-eval-loop.v1",
            "status": "ready_for_next_pulse_task",
            "mode": "recommendation_only",
            "generated_at": "2026-06-04T23:34:02Z",
            "loop": {
                "bounded": true,
                "max_iterations": 1,
                "terminal": true,
                "chain_depth": 1,
                "continues_automatically": false,
                "fixed_interval_loop_successor": "ao2 pulse eval-loop run --chain"
            },
            "observed_inputs": {
                "packet": packet.display().to_string(),
                "packet_sha256": packet_sha256,
                "board": board.display().to_string(),
                "board_sha256": board_sha256
            },
            "prior_eval_loop": {
                "path": format!("target/{platform}/pulse-eval-loop-once.json"),
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "schema_version": "ao2.pulse-eval-loop.v1",
                "status": "ready_for_next_pulse_task",
                "mode": "recommendation_only",
                "terminal": true,
                "chain_depth": 0,
                "recommended_next_task": {
                    "id": "ao2-pulse-eval-loop-next-task",
                    "status": "recommended"
                }
            },
            "verification": {
                "command": "cargo test --package ao2-cli --test cli_approval_replay --release pulse",
                "status": "passed",
                "required_for_recommendation": true
            },
            "evaluator": {
                "decision": "recommend_next_task",
                "verification_status": "passed",
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "evidence_digest_required": true
            },
            "recommended_next_task": {
                "id": "ao2-pulse-eval-loop-chain-next-task",
                "title": "Advance AO2 Pulse eval-loop chain toward the next bounded feature",
                "classification": "COMPLEX",
                "shape": "governed_eval_loop_chain",
                "status": "recommended",
                "requires_operator_or_follow_on": true,
                "recommended_command": "ao2 pulse eval-loop run --chain --eval-loop-evidence <pulse-eval-loop.json> --eval-loop-sha256 <sha256> --verification-command <cmd> --verification-status passed --packet <packet> --board <board> --out-dir <evidence-dir> --json"
            },
            "c85": {
                "status": "passed",
                "hosted_github_actions_checked": false,
                "rerun_allowed_without_user_billing_fix": false
            },
            "trust_boundary": {
                "ao2_execution_evidence_owner": true,
                "factory_v3_evaluator_closer_reference": true,
                "hermes_frontend_queue_memory_surface": true,
                "ao2_control_plane_read_only_observer": true,
                "control_plane_observer_only": true,
                "control_plane_approves_release": false,
                "control_plane_mutates_ao_artifacts": false
            },
            "side_effects": {
                "provider_execution": false,
                "queue_execution": false,
                "memory_write": false,
                "mutates_ao_artifacts": false,
                "hermes_cron_watchdog_mutation": false,
                "control_plane_mutation": false,
                "repo_apply": false
            },
            "artifacts": {
                "pulse_eval_loop": eval_loop.display().to_string()
            }
        }))
        .unwrap(),
    )
    .unwrap();
    eval_loop
}

#[test]
fn cli_plugin_pulse_apply_observer_bundle_packages_available_platform_apply_results() {
    let temp = tempfile::tempdir().unwrap();
    let mut apply_paths = Vec::new();
    let mut apply_shas = Vec::new();

    for platform in ["macos", "ubuntu"] {
        let apply_path = write_pulse_apply_result_fixture(temp.path(), platform);
        apply_shas.push(sha256_path(&apply_path));
        apply_paths.push(apply_path);
    }

    let out_dir = temp.path().join("pulse-apply-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-apply-observer-bundle",
        "--macos-apply-result",
        apply_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &apply_shas[0],
        "--ubuntu-apply-result",
        apply_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &apply_shas[1],
        "--windows-unavailable-reason",
        "direct Windows SSH returned No route to host",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();

    assert_eq!(
        json["schema_version"],
        "ao2.k37-pulse-apply-result-observer-bundle.v1"
    );
    assert_eq!(json["status"], "ready_for_k37_observation");
    assert_eq!(json["producer"], "ao2");
    assert_eq!(json["platform_count"], 2);
    assert_eq!(json["platforms"], serde_json::json!(["macos", "ubuntu"]));
    assert_eq!(
        json["unavailable_platforms"]["windows"]["status"],
        "unavailable"
    );
    assert_eq!(
        json["observed_evidence_scope"],
        serde_json::json!(["ao2.pulse-apply-result.v1"])
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    for (idx, platform) in ["macos", "ubuntu"].iter().enumerate() {
        assert_eq!(
            json["platform_apply_results"][*platform]["sha256"],
            apply_shas[idx]
        );
        assert_eq!(
            json["platform_apply_results"][*platform]["schema_version"],
            "ao2.pulse-apply-result.v1"
        );
        assert_eq!(
            json["platform_apply_results"][*platform]["status"],
            "accepted"
        );
        assert_eq!(
            json["platform_apply_results"][*platform]["side_effects"]["provider_execution"],
            false
        );
        assert!(Path::new(
            json["platform_apply_results"][*platform]["bundled_path"]
                .as_str()
                .unwrap()
        )
        .is_file());
    }

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(json["archive_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(archive_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));
    assert_eq!(json["archive_sha256"], sha256_path(archive_path));

    let bad_digest = ao2([
        "plugin",
        "pulse-apply-observer-bundle",
        "--macos-apply-result",
        apply_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-apply-result",
        apply_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &apply_shas[1],
        "--windows-unavailable-reason",
        "direct Windows SSH returned No route to host",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos pulse apply-result sha256 mismatch"));
}

#[test]
fn cli_plugin_pulse_apply_observer_bundle_verify_validates_distributed_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let mut apply_paths = Vec::new();
    let mut apply_shas = Vec::new();

    for platform in ["macos", "ubuntu"] {
        let apply_path = write_pulse_apply_result_fixture(temp.path(), platform);
        apply_shas.push(sha256_path(&apply_path));
        apply_paths.push(apply_path);
    }

    let out_dir = temp.path().join("pulse-apply-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-apply-observer-bundle",
        "--macos-apply-result",
        apply_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &apply_shas[0],
        "--ubuntu-apply-result",
        apply_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &apply_shas[1],
        "--windows-unavailable-reason",
        "direct Windows SSH returned No route to host",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let summary_path = bundle_json["summary_path"].as_str().unwrap();
    let archive_path = bundle_json["archive_path"].as_str().unwrap();
    let summary_sha256 = bundle_json["summary_sha256"].as_str().unwrap();
    let archive_sha256 = bundle_json["archive_sha256"].as_str().unwrap();

    let verify = ao2([
        "plugin",
        "pulse-apply-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        summary_sha256,
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.k37-pulse-apply-result-observer-bundle-verification.v1"
    );
    assert_eq!(verify_json["status"], "passed");
    assert_eq!(verify_json["summary_sha256"], summary_sha256);
    assert_eq!(verify_json["archive_sha256"], archive_sha256);
    assert_eq!(verify_json["platform_count"], 2);
    assert_eq!(
        verify_json["observed_evidence_scope"],
        serde_json::json!(["ao2.pulse-apply-result.v1"])
    );
    assert_eq!(verify_json["archive_contents_verified"], true);
    assert_eq!(verify_json["platform_apply_results_verified"], true);
    assert_eq!(
        verify_json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(
        verify_json["side_effects"]["would_mutate_control_plane"],
        false
    );
    assert_eq!(verify_json["factory_v3_role"], "parity_auditor");

    let bad_digest = ao2([
        "plugin",
        "pulse-apply-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("pulse apply observer bundle summary sha256 mismatch"));
}

#[test]
fn cli_plugin_pulse_apply_windows_recovery_writes_digest_pinned_runner() {
    let temp = tempfile::tempdir().unwrap();
    let macos_apply_result = write_pulse_apply_result_fixture(temp.path(), "macos");
    let ubuntu_apply_result = write_pulse_apply_result_fixture(temp.path(), "ubuntu");
    let macos_apply_result_sha256 = sha256_path(&macos_apply_result);
    let ubuntu_apply_result_sha256 = sha256_path(&ubuntu_apply_result);
    let bundle_dir = temp.path().join("pulse-apply-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-apply-observer-bundle",
        "--macos-apply-result",
        macos_apply_result.to_str().unwrap(),
        "--macos-sha256",
        &macos_apply_result_sha256,
        "--ubuntu-apply-result",
        ubuntu_apply_result.to_str().unwrap(),
        "--ubuntu-sha256",
        &ubuntu_apply_result_sha256,
        "--windows-unavailable-reason",
        "direct Windows SSH returned No route to host",
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let bundle_summary = PathBuf::from(bundle_json["summary_path"].as_str().unwrap());
    let bundle_archive = PathBuf::from(bundle_json["archive_path"].as_str().unwrap());
    let bundle_summary_sha256 = sha256_path(&bundle_summary);
    let bundle_archive_sha256 = sha256_path(&bundle_archive);

    let out_dir = temp.path().join("pulse-apply-windows-recovery");
    let recovery = ao2([
        "plugin",
        "pulse-apply-windows-recovery",
        "--apply-result",
        macos_apply_result.to_str().unwrap(),
        "--apply-result-sha256",
        &macos_apply_result_sha256,
        "--observer-bundle",
        bundle_summary.to_str().unwrap(),
        "--observer-bundle-sha256",
        &bundle_summary_sha256,
        "--observer-archive",
        bundle_archive.to_str().unwrap(),
        "--observer-archive-sha256",
        &bundle_archive_sha256,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(recovery.status.success(), "{}", stderr(&recovery));

    let json: serde_json::Value = serde_json::from_str(&stdout(&recovery)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.pulse-apply-windows-recovery.v1"
    );
    assert_eq!(json["status"], "ready_for_windows_execution");
    assert_eq!(json["platform"], "windows");
    assert_eq!(
        json["pulse_apply_result"]["source_sha256"],
        macos_apply_result_sha256
    );
    assert_eq!(
        json["observer_bundle"]["summary_sha256"],
        bundle_summary_sha256
    );
    assert_eq!(
        json["observer_bundle"]["archive_sha256"],
        bundle_archive_sha256
    );
    assert_eq!(
        json["execution"]["single_session_command"],
        "powershell -ExecutionPolicy Bypass -File .\\run-pulse-apply-proof.ps1"
    );
    assert_eq!(
        json["execution"]["produces"],
        serde_json::json!([
            "ao2.pulse-apply-result.v1",
            "ao2.k37-pulse-apply-result-observer-bundle.v1"
        ])
    );
    assert_eq!(
        json["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["provider_execution_started"], false);
    assert_eq!(json["side_effects"]["queue_mutated"], false);
    assert_eq!(json["side_effects"]["memory_written"], false);
    assert_eq!(json["side_effects"]["control_plane_mutated"], false);
    assert_eq!(json["side_effects"]["ao_artifacts_mutated"], false);
    assert_eq!(json["factory_v3_role"], "parity_auditor");

    let manifest_path = Path::new(json["manifest_path"].as_str().unwrap());
    let script_path = Path::new(json["script_path"].as_str().unwrap());
    assert!(manifest_path.is_file());
    assert!(script_path.is_file());
    assert_eq!(json["manifest_sha256"], sha256_path(manifest_path));
    assert_eq!(json["script_sha256"], sha256_path(script_path));

    for input_name in [
        "pulse-apply-result.json",
        "k37-pulse-apply-result-observer-bundle.json",
        "k37-pulse-apply-result-observer-bundle.tar.gz",
    ] {
        assert!(out_dir.join("inputs").join(input_name).is_file());
    }

    let script = fs::read_to_string(script_path).unwrap();
    assert!(script.contains("param("));
    assert!(script.contains("Join-Path $PSScriptRoot"));
    assert!(script.contains("plugin pulse-apply-observer-bundle"));
    assert!(script.contains("plugin pulse-apply-observer-bundle-verify"));
    assert!(script.contains(&macos_apply_result_sha256));
    assert!(script.contains(&bundle_summary_sha256));
    assert!(script.contains(&bundle_archive_sha256));
    assert!(script.contains("--windows-apply-result"));
    assert!(script.contains("--windows-sha256"));

    for forbidden in [
        "Bearer ",
        "sk-",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "BEGIN PRIVATE KEY",
    ] {
        assert!(
            !stdout(&recovery).contains(forbidden),
            "pulse apply Windows recovery output exposed forbidden marker {forbidden}"
        );
        assert!(
            !fs::read_to_string(manifest_path)
                .unwrap()
                .contains(forbidden),
            "pulse apply Windows recovery manifest exposed forbidden marker {forbidden}"
        );
        assert!(
            !script.contains(forbidden),
            "pulse apply Windows recovery script exposed forbidden marker {forbidden}"
        );
    }

    let bad_digest = ao2([
        "plugin",
        "pulse-apply-windows-recovery",
        "--apply-result",
        macos_apply_result.to_str().unwrap(),
        "--apply-result-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--observer-bundle",
        bundle_summary.to_str().unwrap(),
        "--observer-bundle-sha256",
        &bundle_summary_sha256,
        "--observer-archive",
        bundle_archive.to_str().unwrap(),
        "--observer-archive-sha256",
        &bundle_archive_sha256,
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("pulse apply-result sha256 mismatch"));
}

#[test]
fn cli_plugin_pulse_executor_observer_bundle_packages_three_platform_executor_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut executor_paths = Vec::new();
    let mut executor_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let executor_path = write_pulse_executor_fixture(temp.path(), platform);
        executor_shas.push(sha256_path(&executor_path));
        executor_paths.push(executor_path);
    }

    let out_dir = temp.path().join("pulse-executor-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-executor-observer-bundle",
        "--macos-executor",
        executor_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &executor_shas[0],
        "--ubuntu-executor",
        executor_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &executor_shas[1],
        "--windows-executor",
        executor_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &executor_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();

    assert_eq!(
        json["schema_version"],
        "ao2.k37-pulse-executor-observer-bundle.v1"
    );
    assert_eq!(json["status"], "ready_for_k37_observation");
    assert_eq!(json["producer"], "ao2");
    assert_eq!(json["platform_count"], 3);
    assert_eq!(
        json["platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["observed_evidence_scope"],
        serde_json::json!([
            "ao2.pulse-executor.v1",
            "ao2.pulse-governed-task.v1",
            "ao2.pulse-task-result.v1"
        ])
    );
    assert_eq!(
        json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(
        json["control_plane_observation"]["may_mutate_evidence"],
        false
    );
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(json["c85"]["status"], "passed");
    assert_eq!(json["c85"]["hosted_github_actions_checked"], true);
    assert_eq!(json["c85"]["rerun_allowed_without_user_billing_fix"], true);
    assert_eq!(
        json["platform_progress"]["schema_version"],
        "ao2.pulse-platform-progress.v1"
    );
    assert_eq!(json["platform_progress"]["status"], "closure_ready");
    assert_eq!(
        json["platform_progress"]["required_platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["platform_progress"]["blocked_platforms"],
        serde_json::json!([])
    );
    assert_eq!(
        json["platform_progress"]["windows"]["current_state"],
        "closure_ready"
    );
    assert_eq!(
        json["platform_progress"]["windows"]["state_history"],
        serde_json::json!([
            "pending",
            "reachable",
            "staged",
            "running",
            "passed",
            "evidence_collected",
            "closure_ready"
        ])
    );
    assert_eq!(
        json["task_contract"]["schema_version"],
        "ao2.pulse-task-contract.v1"
    );
    assert_eq!(json["task_contract"]["c85"], false);
    assert_eq!(json["task_contract"]["ao2_owned_execution"], true);
    assert_eq!(
        json["task_contract"]["factory_v3_evaluator_closer_required"],
        true
    );
    assert_eq!(
        json["task_result_observation"]["status"],
        "ready_for_k37_observation"
    );
    assert_eq!(
        json["task_result_observation"]["observed_platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );

    for (idx, platform) in ["macos", "ubuntu", "windows"].iter().enumerate() {
        let evidence = &json["platform_evidence"][*platform];
        assert_eq!(evidence["schema_version"], "ao2.pulse-executor.v1");
        assert_eq!(evidence["status"], "executed_governed_task");
        assert_eq!(evidence["c85"]["status"], "passed");
        assert_eq!(evidence["sha256"], executor_shas[idx]);
        assert_eq!(evidence["selected_task"]["c85"], false);
        assert_eq!(
            evidence["governed_task_evidence"]["schema_version"],
            "ao2.pulse-governed-task.v1"
        );
        assert_eq!(evidence["governed_task_evidence"]["status"], "accepted");
        assert_eq!(
            evidence["governed_task_evidence"]["c85"]["status"],
            "passed"
        );
        assert_eq!(
            evidence["pulse_task_result"]["schema_version"],
            "ao2.pulse-task-result.v1"
        );
        assert_eq!(evidence["pulse_task_result"]["status"], "accepted");
        assert_eq!(evidence["pulse_task_result"]["c85"]["status"], "passed");
        assert_eq!(
            evidence["pulse_task_result"]["governed_task_evidence"]["sha256"],
            evidence["governed_task_evidence"]["sha256"]
        );
        assert_eq!(
            evidence["pulse_task_result"]["task_contract"]["sha256"],
            evidence["task_contract"]["sha256"]
        );
        assert!(Path::new(
            evidence["bundled_paths"]["pulse_executor"]
                .as_str()
                .unwrap()
        )
        .is_file());
    }

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(json["archive_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(archive_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));
    assert_eq!(json["archive_sha256"], sha256_path(archive_path));

    let bad_digest = ao2([
        "plugin",
        "pulse-executor-observer-bundle",
        "--macos-executor",
        executor_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-executor",
        executor_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &executor_shas[1],
        "--windows-executor",
        executor_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &executor_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos pulse executor sha256 mismatch"));
}

#[test]
fn cli_plugin_pulse_once_observer_bundle_packages_three_platform_once_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut once_paths = Vec::new();
    let mut once_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let once_path = write_pulse_once_fixture(temp.path(), platform);
        once_shas.push(sha256_path(&once_path));
        once_paths.push(once_path);
    }

    let out_dir = temp.path().join("pulse-once-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-once-observer-bundle",
        "--macos-once",
        once_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &once_shas[0],
        "--ubuntu-once",
        once_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &once_shas[1],
        "--windows-once",
        once_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &once_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();

    assert_eq!(
        json["schema_version"],
        "ao2.k37-pulse-once-observer-bundle.v1"
    );
    assert_eq!(json["status"], "ready_for_k37_observation");
    assert_eq!(json["producer"], "ao2");
    assert_eq!(json["platform_count"], 3);
    assert_eq!(
        json["platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["observed_evidence_scope"],
        serde_json::json!(["ao2.pulse-once.v1"])
    );
    assert_eq!(
        json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(
        json["control_plane_observation"]["may_mutate_evidence"],
        false
    );
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(
        json["platform_progress"]["windows"]["current_state"],
        "closure_ready"
    );

    for (idx, platform) in ["macos", "ubuntu", "windows"].iter().enumerate() {
        let evidence = &json["platform_once"][*platform];
        assert_eq!(evidence["schema_version"], "ao2.pulse-once.v1");
        assert_eq!(evidence["status"], "ready_for_operator_execution");
        assert_eq!(evidence["sha256"], once_shas[idx]);
        assert_eq!(evidence["side_effects"]["control_plane_mutation"], false);
        assert!(Path::new(evidence["bundled_paths"]["pulse_once"].as_str().unwrap()).is_file());
    }

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(json["archive_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(archive_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));
    assert_eq!(json["archive_sha256"], sha256_path(archive_path));

    let bad_digest = ao2([
        "plugin",
        "pulse-once-observer-bundle",
        "--macos-once",
        once_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-once",
        once_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &once_shas[1],
        "--windows-once",
        once_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &once_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos pulse once sha256 mismatch"));
}

#[test]
fn cli_plugin_pulse_once_observer_bundle_verify_validates_distributed_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let mut once_paths = Vec::new();
    let mut once_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let once_path = write_pulse_once_fixture(temp.path(), platform);
        once_shas.push(sha256_path(&once_path));
        once_paths.push(once_path);
    }

    let out_dir = temp.path().join("pulse-once-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-once-observer-bundle",
        "--macos-once",
        once_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &once_shas[0],
        "--ubuntu-once",
        once_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &once_shas[1],
        "--windows-once",
        once_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &once_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let summary_path = bundle_json["summary_path"].as_str().unwrap();
    let archive_path = bundle_json["archive_path"].as_str().unwrap();
    let summary_sha256 = bundle_json["summary_sha256"].as_str().unwrap();
    let archive_sha256 = bundle_json["archive_sha256"].as_str().unwrap();

    let verify = ao2([
        "plugin",
        "pulse-once-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        summary_sha256,
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.k37-pulse-once-observer-bundle-verification.v1"
    );
    assert_eq!(verify_json["status"], "passed");
    assert_eq!(verify_json["summary_sha256"], summary_sha256);
    assert_eq!(verify_json["archive_sha256"], archive_sha256);
    assert_eq!(verify_json["platform_count"], 3);
    assert_eq!(verify_json["platform_once_verified"], true);
    assert_eq!(verify_json["archive_contents_verified"], true);
    assert_eq!(
        verify_json["observed_evidence_scope"],
        serde_json::json!(["ao2.pulse-once.v1"])
    );
    assert_eq!(
        verify_json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(
        verify_json["side_effects"]["would_mutate_control_plane"],
        false
    );

    let bad_digest = ao2([
        "plugin",
        "pulse-once-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("pulse once observer bundle summary sha256 mismatch"));
}

#[test]
fn cli_plugin_pulse_chain_observer_bundle_packages_three_platform_chain_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut chain_paths = Vec::new();
    let mut chain_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let chain_path = write_pulse_chain_fixture(temp.path(), platform);
        chain_shas.push(sha256_path(&chain_path));
        chain_paths.push(chain_path);
    }

    let out_dir = temp.path().join("pulse-chain-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-chain-observer-bundle",
        "--macos-chain",
        chain_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &chain_shas[0],
        "--ubuntu-chain",
        chain_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &chain_shas[1],
        "--windows-chain",
        chain_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &chain_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();

    assert_eq!(
        json["schema_version"],
        "ao2.k37-pulse-chain-observer-bundle.v1"
    );
    assert_eq!(json["status"], "ready_for_k37_observation");
    assert_eq!(json["producer"], "ao2");
    assert_eq!(json["platform_count"], 3);
    assert_eq!(
        json["platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["observed_evidence_scope"],
        serde_json::json!(["ao2.pulse-chain.v1"])
    );
    assert_eq!(
        json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(
        json["control_plane_observation"]["may_mutate_evidence"],
        false
    );
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(
        json["platform_progress"]["windows"]["current_state"],
        "closure_ready"
    );

    for (idx, platform) in ["macos", "ubuntu", "windows"].iter().enumerate() {
        let evidence = &json["platform_chain"][*platform];
        assert_eq!(evidence["schema_version"], "ao2.pulse-chain.v1");
        assert_eq!(evidence["status"], "planned_without_execution");
        assert_eq!(evidence["sha256"], chain_shas[idx]);
        assert_eq!(evidence["c85"]["status"], "passed");
        assert_eq!(evidence["side_effects"]["control_plane_mutation"], false);
        assert!(Path::new(evidence["bundled_paths"]["pulse_chain"].as_str().unwrap()).is_file());
    }

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(json["archive_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(archive_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));
    assert_eq!(json["archive_sha256"], sha256_path(archive_path));

    let bad_digest = ao2([
        "plugin",
        "pulse-chain-observer-bundle",
        "--macos-chain",
        chain_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-chain",
        chain_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &chain_shas[1],
        "--windows-chain",
        chain_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &chain_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos pulse chain sha256 mismatch"));
}

#[test]
fn cli_plugin_pulse_chain_observer_bundle_verify_validates_distributed_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let mut chain_paths = Vec::new();
    let mut chain_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let chain_path = write_pulse_chain_fixture(temp.path(), platform);
        chain_shas.push(sha256_path(&chain_path));
        chain_paths.push(chain_path);
    }

    let out_dir = temp.path().join("pulse-chain-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-chain-observer-bundle",
        "--macos-chain",
        chain_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &chain_shas[0],
        "--ubuntu-chain",
        chain_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &chain_shas[1],
        "--windows-chain",
        chain_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &chain_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let summary_path = bundle_json["summary_path"].as_str().unwrap();
    let archive_path = bundle_json["archive_path"].as_str().unwrap();
    let summary_sha256 = bundle_json["summary_sha256"].as_str().unwrap();
    let archive_sha256 = bundle_json["archive_sha256"].as_str().unwrap();

    let verify = ao2([
        "plugin",
        "pulse-chain-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        summary_sha256,
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.k37-pulse-chain-observer-bundle-verification.v1"
    );
    assert_eq!(verify_json["status"], "passed");
    assert_eq!(verify_json["summary_sha256"], summary_sha256);
    assert_eq!(verify_json["archive_sha256"], archive_sha256);
    assert_eq!(verify_json["platform_count"], 3);
    assert_eq!(verify_json["platform_chain_verified"], true);
    assert_eq!(verify_json["archive_contents_verified"], true);
    assert_eq!(
        verify_json["observed_evidence_scope"],
        serde_json::json!(["ao2.pulse-chain.v1"])
    );
    assert_eq!(
        verify_json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(
        verify_json["side_effects"]["would_mutate_control_plane"],
        false
    );

    let bad_digest = ao2([
        "plugin",
        "pulse-chain-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("pulse chain observer bundle summary sha256 mismatch"));
}

#[test]
fn cli_plugin_pulse_eval_loop_observer_bundle_packages_three_platform_eval_loop_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut eval_loop_paths = Vec::new();
    let mut eval_loop_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let eval_loop_path = write_pulse_eval_loop_fixture(temp.path(), platform);
        eval_loop_shas.push(sha256_path(&eval_loop_path));
        eval_loop_paths.push(eval_loop_path);
    }

    let out_dir = temp.path().join("pulse-eval-loop-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-eval-loop-observer-bundle",
        "--macos-eval-loop",
        eval_loop_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &eval_loop_shas[0],
        "--ubuntu-eval-loop",
        eval_loop_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &eval_loop_shas[1],
        "--windows-eval-loop",
        eval_loop_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &eval_loop_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();

    assert_eq!(
        json["schema_version"],
        "ao2.k37-pulse-eval-loop-observer-bundle.v1"
    );
    assert_eq!(json["status"], "ready_for_k37_observation");
    assert_eq!(json["producer"], "ao2");
    assert_eq!(json["platform_count"], 3);
    assert_eq!(
        json["platforms"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["observed_evidence_scope"],
        serde_json::json!(["ao2.pulse-eval-loop.v1"])
    );
    assert_eq!(
        json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(
        json["control_plane_observation"]["may_mutate_evidence"],
        false
    );
    assert_eq!(json["side_effects"]["would_execute_provider"], false);
    assert_eq!(json["side_effects"]["would_mutate_control_plane"], false);
    assert_eq!(
        json["platform_progress"]["windows"]["current_state"],
        "closure_ready"
    );

    for (idx, platform) in ["macos", "ubuntu", "windows"].iter().enumerate() {
        let evidence = &json["platform_eval_loop"][*platform];
        assert_eq!(evidence["schema_version"], "ao2.pulse-eval-loop.v1");
        assert_eq!(evidence["status"], "ready_for_next_pulse_task");
        assert_eq!(evidence["sha256"], eval_loop_shas[idx]);
        assert_eq!(evidence["loop"]["chain_depth"], 1);
        assert_eq!(evidence["loop"]["terminal"], true);
        assert_eq!(evidence["side_effects"]["repo_apply"], false);
        assert_eq!(evidence["side_effects"]["control_plane_mutation"], false);
        assert!(Path::new(
            evidence["bundled_paths"]["pulse_eval_loop"]
                .as_str()
                .unwrap()
        )
        .is_file());
    }

    let summary_path = Path::new(json["summary_path"].as_str().unwrap());
    let archive_path = Path::new(json["archive_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(archive_path.is_file());
    assert_eq!(json["summary_sha256"], sha256_path(summary_path));
    assert_eq!(json["archive_sha256"], sha256_path(archive_path));

    let bad_digest = ao2([
        "plugin",
        "pulse-eval-loop-observer-bundle",
        "--macos-eval-loop",
        eval_loop_paths[0].to_str().unwrap(),
        "--macos-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--ubuntu-eval-loop",
        eval_loop_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &eval_loop_shas[1],
        "--windows-eval-loop",
        eval_loop_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &eval_loop_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("macos pulse eval-loop sha256 mismatch"));
}

#[test]
fn cli_plugin_pulse_eval_loop_observer_bundle_verify_validates_distributed_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let mut eval_loop_paths = Vec::new();
    let mut eval_loop_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let eval_loop_path = write_pulse_eval_loop_fixture(temp.path(), platform);
        eval_loop_shas.push(sha256_path(&eval_loop_path));
        eval_loop_paths.push(eval_loop_path);
    }

    let out_dir = temp.path().join("pulse-eval-loop-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-eval-loop-observer-bundle",
        "--macos-eval-loop",
        eval_loop_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &eval_loop_shas[0],
        "--ubuntu-eval-loop",
        eval_loop_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &eval_loop_shas[1],
        "--windows-eval-loop",
        eval_loop_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &eval_loop_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let summary_path = bundle_json["summary_path"].as_str().unwrap();
    let archive_path = bundle_json["archive_path"].as_str().unwrap();
    let summary_sha256 = bundle_json["summary_sha256"].as_str().unwrap();
    let archive_sha256 = bundle_json["archive_sha256"].as_str().unwrap();

    let verify = ao2([
        "plugin",
        "pulse-eval-loop-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        summary_sha256,
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.k37-pulse-eval-loop-observer-bundle-verification.v1"
    );
    assert_eq!(verify_json["status"], "passed");
    assert_eq!(verify_json["summary_sha256"], summary_sha256);
    assert_eq!(verify_json["archive_sha256"], archive_sha256);
    assert_eq!(verify_json["platform_count"], 3);
    assert_eq!(verify_json["platform_eval_loop_verified"], true);
    assert_eq!(verify_json["archive_contents_verified"], true);
    assert_eq!(
        verify_json["observed_evidence_scope"],
        serde_json::json!(["ao2.pulse-eval-loop.v1"])
    );
    assert_eq!(
        verify_json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(
        verify_json["side_effects"]["would_mutate_control_plane"],
        false
    );

    let bad_digest = ao2([
        "plugin",
        "pulse-eval-loop-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("pulse eval-loop observer bundle summary sha256 mismatch"));
}

#[test]
fn cli_plugin_pulse_executor_observer_bundle_verify_validates_distributed_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let mut executor_paths = Vec::new();
    let mut executor_shas = Vec::new();

    for platform in ["macos", "ubuntu", "windows"] {
        let executor_path = write_pulse_executor_fixture(temp.path(), platform);
        executor_shas.push(sha256_path(&executor_path));
        executor_paths.push(executor_path);
    }

    let out_dir = temp.path().join("pulse-executor-observer-bundle");
    let bundle = ao2([
        "plugin",
        "pulse-executor-observer-bundle",
        "--macos-executor",
        executor_paths[0].to_str().unwrap(),
        "--macos-sha256",
        &executor_shas[0],
        "--ubuntu-executor",
        executor_paths[1].to_str().unwrap(),
        "--ubuntu-sha256",
        &executor_shas[1],
        "--windows-executor",
        executor_paths[2].to_str().unwrap(),
        "--windows-sha256",
        &executor_shas[2],
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let summary_path = bundle_json["summary_path"].as_str().unwrap();
    let archive_path = bundle_json["archive_path"].as_str().unwrap();
    let summary_sha256 = bundle_json["summary_sha256"].as_str().unwrap();
    let archive_sha256 = bundle_json["archive_sha256"].as_str().unwrap();

    let verify = ao2([
        "plugin",
        "pulse-executor-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        summary_sha256,
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.k37-pulse-executor-observer-bundle-verification.v1"
    );
    assert_eq!(verify_json["status"], "passed");
    assert_eq!(verify_json["summary_sha256"], summary_sha256);
    assert_eq!(verify_json["archive_sha256"], archive_sha256);
    assert_eq!(verify_json["platform_count"], 3);
    assert_eq!(verify_json["platform_executor_evidence_verified"], true);
    assert_eq!(verify_json["archive_contents_verified"], true);
    assert_eq!(
        verify_json["observed_evidence_scope"],
        serde_json::json!([
            "ao2.pulse-executor.v1",
            "ao2.pulse-governed-task.v1",
            "ao2.pulse-task-result.v1"
        ])
    );
    assert_eq!(
        verify_json["control_plane_observation"]["role"],
        "read_only_observer"
    );
    assert_eq!(
        verify_json["side_effects"]["would_mutate_control_plane"],
        false
    );

    let bad_digest = ao2([
        "plugin",
        "pulse-executor-observer-bundle-verify",
        "--summary",
        summary_path,
        "--summary-sha256",
        "0000000000000000000000000000000000000000000000000000000000000000",
        "--archive",
        archive_path,
        "--archive-sha256",
        archive_sha256,
        "--json",
    ]);
    assert!(!bad_digest.status.success());
    assert!(stderr(&bad_digest).contains("pulse executor observer bundle summary sha256 mismatch"));
}
