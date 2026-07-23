use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

fn write_pulse_task_contract(root: &Path, id: &str, title: &str, c85: bool) -> PathBuf {
    let contract = root.join(format!("{id}-task-contract.json"));
    fs::write(
        &contract,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-task-contract.v1",
            "id": id,
            "title": title,
            "classification": "COMPLEX",
            "shape": "refactor",
            "c85": c85,
            "ao2_owned_execution": true,
            "factory_v3_evaluator_closer_required": true,
            "evaluator_acceptance": "accept_non_c85_governed_task",
            "closer_acceptance": "accepted",
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
    contract
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
                "command": "cargo test --package ao2-cli --test cli_pulse --release pulse",
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
fn cli_pulse_run_once_emits_read_only_next_task_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "codex-cron scheduler override; C85 hosted GitHub Actions proof remains deferred",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "Plugin/K37 current. Exact Next Recommended Lengthy Task: Start the AO2 Pulse once-mode replacement slice.",
    )
    .unwrap();
    let out_dir = temp.path().join("pulse-once");

    let pulse = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(pulse.status.success(), "{}", stderr(&pulse));
    let json: serde_json::Value = serde_json::from_str(&stdout(&pulse)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-once.v1");
    assert_eq!(json["status"], "ready_for_operator_execution");
    assert_eq!(json["scheduler"]["active_runner"], "codex-cron");
    assert_eq!(json["scheduler"]["hermes_cron_mutated"], false);
    assert_eq!(json["c85"]["status"], "deferred");
    assert_eq!(json["c85"]["hosted_github_actions_checked"], false);
    assert_eq!(json["trust_boundary"]["ao2_execution_evidence_owner"], true);
    assert_eq!(
        json["trust_boundary"]["factory_v3_evaluator_closer_reference"],
        true
    );
    assert_eq!(json["trust_boundary"]["control_plane_observer_only"], true);
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["selected_task"]["id"], "ao2-pulse-next-safe-task");
    assert_eq!(json["selected_task"]["shape"], "greenfield");
    assert!(json["selected_task"]["recommended_command"]
        .as_str()
        .unwrap()
        .contains("ao2 pulse run --once"));
    assert!(
        json["observed_inputs"]["packet_sha256"]
            .as_str()
            .unwrap()
            .len()
            == 64
    );
    assert!(
        json["observed_inputs"]["board_sha256"]
            .as_str()
            .unwrap()
            .len()
            == 64
    );

    let persisted_path = PathBuf::from(json["artifacts"]["pulse_once"].as_str().unwrap());
    assert_eq!(persisted_path, out_dir.join("pulse-once.json"));
    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&persisted_path).unwrap()).unwrap();
    assert_eq!(persisted["schema_version"], "ao2.pulse-once.v1");
    assert_eq!(persisted["selected_task"]["id"], "ao2-pulse-next-safe-task");
}

#[test]
fn cli_pulse_run_once_records_post_c85_ready_state_when_packet_says_passed() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "Hosted C85 Release Gate passed on 2026-06-04; AO2 Pulse post-C85/plugin-ready once evidence is next.",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse once-mode on the post-C85/plugin-ready line with direct Windows progress tracked explicitly.",
    )
    .unwrap();
    let out_dir = temp.path().join("pulse-once");

    let pulse = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(pulse.status.success(), "{}", stderr(&pulse));
    let json: serde_json::Value = serde_json::from_str(&stdout(&pulse)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-once.v1");
    assert_eq!(json["c85"]["status"], "passed");
    assert_eq!(json["c85"]["hosted_github_actions_checked"], true);
    assert_eq!(json["c85"]["rerun_allowed_without_user_billing_fix"], true);
    assert_eq!(
        json["observed_inputs"]["packet_mentions_c85_deferred"],
        false
    );
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);
}

#[test]
fn cli_pulse_run_loop_consumes_decision_file_and_writes_summary() {
    let temp = tempfile::tempdir().unwrap();
    let decision = temp.path().join("decision.json");
    fs::write(
        &decision,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-event-loop-decision.v1",
            "event_loop": {
                "action": "stop",
                "reason": "operator requested stop after one iteration",
                "next_task_id": "pulse-next-safe-task"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let out_dir = temp.path().join("pulse-run-loop");
    let command = format!("\"{}\" version", env!("CARGO_BIN_EXE_ao2"));

    let run_loop = ao2([
        "pulse",
        "run-loop",
        "--command",
        &command,
        "--decision-file",
        decision.to_str().unwrap(),
        "--max-chain-runs",
        "3",
        "--max-runtime-seconds",
        "60",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--apply-root",
        temp.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(run_loop.status.success(), "{}", stderr(&run_loop));
    let json: serde_json::Value = serde_json::from_str(&stdout(&run_loop)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-event-loop-run.v1");
    assert_eq!(json["status"], "stopped");
    assert_eq!(json["iterations"], 1);
    assert_eq!(json["decision_source"], "file");
    assert_eq!(json["next_task_id"], "pulse-next-safe-task");
    assert_eq!(json["decisions"][0]["action"], "stop");
    assert_eq!(
        json["decisions"][0]["decision_file"].as_str().unwrap(),
        decision.to_string_lossy().as_ref()
    );

    let summary_path = out_dir.join("summary.json");
    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_path).unwrap()).unwrap();
    assert_eq!(summary, json);
    assert!(out_dir.join("logs").join("iteration-01.log").is_file());
}

#[test]
fn cli_pulse_eval_loop_run_once_recommends_next_task_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "Hosted C85 Release Gate passed; AO2 Pulse eval loop should recommend only.",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse executor evidence is current. Exact Next Recommended Lengthy Task: add recommendation-only Pulse eval loop.",
    )
    .unwrap();

    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();

    let chain_out_dir = temp.path().join("pulse-chain");
    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_json["artifacts"]["pulse_once"].as_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let chain_json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();

    let contract_path = write_pulse_task_contract(
        temp.path(),
        "pulse-eval-loop-input",
        "Produce Pulse executor evidence for eval-loop input",
        false,
    );
    let execute_out_dir = temp.path().join("pulse-execute");
    let execute = ao2([
        "pulse",
        "run",
        "--execute",
        "--chain-evidence",
        chain_json["artifacts"]["pulse_chain"].as_str().unwrap(),
        "--task-contract",
        contract_path.to_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        execute_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(execute.status.success(), "{}", stderr(&execute));
    let execute_json: serde_json::Value = serde_json::from_str(&stdout(&execute)).unwrap();
    let executor_path = PathBuf::from(
        execute_json["artifacts"]["pulse_executor"]
            .as_str()
            .unwrap(),
    );
    let executor_sha256 = sha256_path(&executor_path);

    let eval_out_dir = temp.path().join("pulse-eval-loop");
    let eval = ao2([
        "pulse",
        "eval-loop",
        "run",
        "--once",
        "--executor-evidence",
        executor_path.to_str().unwrap(),
        "--executor-sha256",
        executor_sha256.as_str(),
        "--verification-command",
        "cargo test --package ao2-cli --test cli_pulse --release pulse",
        "--verification-status",
        "passed",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        eval_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(eval.status.success(), "{}", stderr(&eval));
    let json: serde_json::Value = serde_json::from_str(&stdout(&eval)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-eval-loop.v1");
    assert_eq!(json["status"], "ready_for_next_pulse_task");
    assert_eq!(json["mode"], "recommendation_only");
    assert_eq!(json["loop"]["bounded"], true);
    assert_eq!(json["loop"]["max_iterations"], 1);
    assert_eq!(json["loop"]["terminal"], true);
    assert_eq!(json["evaluator"]["decision"], "recommend_next_task");
    assert_eq!(json["evaluator"]["verification_status"], "passed");
    assert_eq!(
        json["recommended_next_task"]["id"],
        "ao2-pulse-eval-loop-next-task"
    );
    assert_eq!(
        json["recommended_next_task"]["requires_operator_or_follow_on"],
        true
    );
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["hermes_cron_watchdog_mutation"], false);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);
    assert_eq!(json["side_effects"]["repo_apply"], false);

    let persisted_path = PathBuf::from(json["artifacts"]["pulse_eval_loop"].as_str().unwrap());
    assert_eq!(persisted_path, eval_out_dir.join("pulse-eval-loop.json"));
    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&persisted_path).unwrap()).unwrap();
    assert_eq!(persisted["schema_version"], "ao2.pulse-eval-loop.v1");
}

#[test]
fn cli_pulse_eval_loop_run_once_stops_when_verification_failed() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "Hosted C85 Release Gate passed; AO2 Pulse eval loop should stop on verifier failure.",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse eval loop must never continue when verification failed.",
    )
    .unwrap();
    let executor_path = write_pulse_executor_fixture(temp.path(), "macos");
    let executor_sha256 = sha256_path(&executor_path);
    let eval_out_dir = temp.path().join("pulse-eval-loop-failed");

    let eval = ao2([
        "pulse",
        "eval-loop",
        "run",
        "--once",
        "--executor-evidence",
        executor_path.to_str().unwrap(),
        "--executor-sha256",
        executor_sha256.as_str(),
        "--verification-command",
        "cargo test --package ao2-cli --test cli_pulse --release pulse",
        "--verification-status",
        "failed",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        eval_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(eval.status.success(), "{}", stderr(&eval));
    let json: serde_json::Value = serde_json::from_str(&stdout(&eval)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-eval-loop.v1");
    assert_eq!(json["status"], "blocked_by_verification");
    assert_eq!(json["mode"], "recommendation_only");
    assert_eq!(json["loop"]["bounded"], true);
    assert_eq!(json["loop"]["max_iterations"], 1);
    assert_eq!(json["loop"]["terminal"], true);
    assert_eq!(json["loop"]["continues_automatically"], false);
    assert_eq!(json["evaluator"]["decision"], "block_next_task");
    assert_eq!(json["evaluator"]["verification_status"], "failed");
    assert_eq!(json["recommended_next_task"]["status"], "blocked");
    assert_eq!(
        json["recommended_next_task"]["requires_operator_or_follow_on"],
        true
    );
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);
    assert_eq!(json["side_effects"]["repo_apply"], false);

    let persisted_path = PathBuf::from(json["artifacts"]["pulse_eval_loop"].as_str().unwrap());
    assert_eq!(persisted_path, eval_out_dir.join("pulse-eval-loop.json"));
    assert!(persisted_path.exists());
}

#[test]
fn cli_pulse_eval_loop_run_chain_consumes_terminal_once_evidence_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "Hosted C85 Release Gate passed; AO2 Pulse eval loop chain should recommend only.",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse eval-loop once evidence is current. Exact Next Recommended Lengthy Task: add bounded eval-loop chain mode.",
    )
    .unwrap();
    let executor_path = write_pulse_executor_fixture(temp.path(), "macos");
    let executor_sha256 = sha256_path(&executor_path);
    let once_out_dir = temp.path().join("pulse-eval-loop-once");
    let once = ao2([
        "pulse",
        "eval-loop",
        "run",
        "--once",
        "--executor-evidence",
        executor_path.to_str().unwrap(),
        "--executor-sha256",
        executor_sha256.as_str(),
        "--verification-command",
        "cargo test --package ao2-cli --test cli_pulse --release pulse",
        "--verification-status",
        "passed",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    let once_eval_path = PathBuf::from(once_json["artifacts"]["pulse_eval_loop"].as_str().unwrap());
    let once_eval_sha256 = sha256_path(&once_eval_path);
    let chain_out_dir = temp.path().join("pulse-eval-loop-chain");

    let chain = ao2([
        "pulse",
        "eval-loop",
        "run",
        "--chain",
        "--eval-loop-evidence",
        once_eval_path.to_str().unwrap(),
        "--eval-loop-sha256",
        once_eval_sha256.as_str(),
        "--verification-command",
        "cargo test --package ao2-cli --test cli_pulse --release pulse",
        "--verification-status",
        "passed",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-eval-loop.v1");
    assert_eq!(json["status"], "ready_for_next_pulse_task");
    assert_eq!(json["mode"], "recommendation_only");
    assert_eq!(json["loop"]["bounded"], true);
    assert_eq!(json["loop"]["terminal"], true);
    assert_eq!(json["loop"]["continues_automatically"], false);
    assert_eq!(json["loop"]["chain_depth"], 1);
    assert_eq!(
        json["prior_eval_loop"]["schema_version"],
        "ao2.pulse-eval-loop.v1"
    );
    assert_eq!(
        json["prior_eval_loop"]["status"],
        "ready_for_next_pulse_task"
    );
    assert_eq!(json["evaluator"]["decision"], "recommend_next_task");
    assert_eq!(
        json["recommended_next_task"]["id"],
        "ao2-pulse-eval-loop-chain-next-task"
    );
    assert_eq!(
        json["recommended_next_task"]["requires_operator_or_follow_on"],
        true
    );
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);
    assert_eq!(json["side_effects"]["repo_apply"], false);

    let persisted_path = PathBuf::from(json["artifacts"]["pulse_eval_loop"].as_str().unwrap());
    assert_eq!(persisted_path, chain_out_dir.join("pulse-eval-loop.json"));
    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&persisted_path).unwrap()).unwrap();
    assert_eq!(persisted["loop"]["chain_depth"], 1);
}

#[test]
fn cli_pulse_eval_loop_handoff_writes_digest_pinned_non_c85_task_contract_without_execution() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "AO2 Pulse eval-loop observer readback is complete; prepare a bounded task contract only.",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse eval-loop K37 readback is current. Exact Next Recommended Lengthy Task: write a bounded non-C85 Pulse task contract.",
    )
    .unwrap();
    let eval_loop_path = write_pulse_eval_loop_fixture(temp.path(), "macos");
    let eval_loop_sha256 = sha256_path(&eval_loop_path);
    let out_dir = temp.path().join("pulse-eval-loop-handoff");

    let handoff = ao2([
        "pulse",
        "eval-loop",
        "handoff",
        "--eval-loop-evidence",
        eval_loop_path.to_str().unwrap(),
        "--eval-loop-sha256",
        eval_loop_sha256.as_str(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(handoff.status.success(), "{}", stderr(&handoff));
    let json: serde_json::Value = serde_json::from_str(&stdout(&handoff)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-task-contract-handoff.v1");
    assert_eq!(json["status"], "task_contract_ready");
    assert_eq!(json["mode"], "contract_only");
    assert_eq!(
        json["prior_eval_loop"]["schema_version"],
        "ao2.pulse-eval-loop.v1"
    );
    assert_eq!(json["prior_eval_loop"]["sha256"], eval_loop_sha256.as_str());
    assert_eq!(
        json["selected_task"]["id"],
        "ao2-pulse-eval-loop-chain-next-task"
    );
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);
    assert_eq!(json["side_effects"]["repo_apply"], false);

    let contract_path = PathBuf::from(json["artifacts"]["task_contract"].as_str().unwrap());
    assert_eq!(contract_path, out_dir.join("pulse-task-contract.json"));
    assert!(contract_path.is_file());
    assert_eq!(
        json["artifacts"]["task_contract_sha256"],
        sha256_path(&contract_path)
    );

    let contract: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    assert_eq!(contract["schema_version"], "ao2.pulse-task-contract.v1");
    assert_eq!(contract["id"], "ao2-pulse-eval-loop-chain-next-task");
    assert_eq!(contract["classification"], "COMPLEX");
    assert_eq!(contract["shape"], "governed_eval_loop_chain");
    assert_eq!(contract["c85"], false);
    assert_eq!(contract["ao2_owned_execution"], true);
    assert_eq!(contract["factory_v3_evaluator_closer_required"], true);
    assert_eq!(contract["source_eval_loop"]["sha256"], eval_loop_sha256);
    assert_eq!(contract["side_effects"]["provider_execution"], false);
    assert_eq!(contract["side_effects"]["queue_execution"], false);
    assert_eq!(contract["side_effects"]["memory_write"], false);
    assert_eq!(contract["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(contract["side_effects"]["control_plane_mutation"], false);
}

#[test]
fn cli_pulse_run_chain_consumes_once_evidence_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "codex-cron scheduler override; C85 hosted GitHub Actions proof remains deferred because billing is blocked",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse once-mode is current. Exact Next Recommended Lengthy Task: implement AO2 Pulse chain-mode planning.",
    )
    .unwrap();
    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    let once_path = once_json["artifacts"]["pulse_once"].as_str().unwrap();
    let chain_out_dir = temp.path().join("pulse-chain");

    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_path,
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-chain.v1");
    assert_eq!(json["status"], "planned_without_execution");
    assert_eq!(json["scheduler"]["active_runner"], "codex-cron");
    assert_eq!(json["scheduler"]["hermes_cron_mutated"], false);
    assert_eq!(json["c85"]["status"], "deferred");
    assert_eq!(json["c85"]["hosted_github_actions_checked"], false);
    assert_eq!(json["trust_boundary"]["control_plane_observer_only"], true);
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);
    assert_eq!(json["prior_once"]["schema_version"], "ao2.pulse-once.v1");
    assert_eq!(json["prior_once"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(
        json["chain_steps"][0]["id"],
        "observe-pulse-once-and-select-next-safe-task"
    );
    assert_eq!(json["chain_steps"][0]["executes_task"], false);
    assert_eq!(
        json["chain_steps"][1]["id"],
        "refuse-c85-while-billing-blocked"
    );
    assert_eq!(json["chain_steps"][1]["executes_task"], false);

    let persisted_path = PathBuf::from(json["artifacts"]["pulse_chain"].as_str().unwrap());
    assert_eq!(persisted_path, chain_out_dir.join("pulse-chain.json"));
    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&persisted_path).unwrap()).unwrap();
    assert_eq!(persisted["schema_version"], "ao2.pulse-chain.v1");
    assert_eq!(
        persisted["prior_once"]["sha256"],
        json["prior_once"]["sha256"]
    );
}

#[test]
fn cli_pulse_run_chain_records_post_c85_ready_state_when_packet_and_once_say_passed() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "Hosted C85 Release Gate passed on 2026-06-04; AO2 Pulse post-C85/plugin-ready chain evidence is next.",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse chain-mode on the post-C85/plugin-ready line with direct Windows progress tracked explicitly.",
    )
    .unwrap();
    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    assert_eq!(once_json["c85"]["status"], "passed");
    let once_path = once_json["artifacts"]["pulse_once"].as_str().unwrap();
    let chain_out_dir = temp.path().join("pulse-chain");

    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_path,
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-chain.v1");
    assert_eq!(json["c85"]["status"], "passed");
    assert_eq!(json["c85"]["hosted_github_actions_checked"], true);
    assert_eq!(json["c85"]["rerun_allowed_without_user_billing_fix"], true);
    assert_eq!(
        json["observed_inputs"]["packet_mentions_c85_deferred"],
        false
    );
    assert_eq!(json["observed_inputs"]["packet_mentions_c85_passed"], true);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);
    assert!(json["chain_steps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|step| step["id"] != "refuse-c85-while-billing-blocked"));
}

#[test]
fn cli_pulse_run_execute_accepts_post_c85_passed_chain_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "Hosted C85 Release Gate passed on 2026-06-04; AO2 Pulse post-C85/plugin-ready execute evidence is next.",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse chain-mode is current after K37 observation. Exact Next Recommended Lengthy Task: execute one governed non-C85 Pulse task.",
    )
    .unwrap();
    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    assert_eq!(once_json["c85"]["status"], "passed");
    let once_path = once_json["artifacts"]["pulse_once"].as_str().unwrap();
    let chain_out_dir = temp.path().join("pulse-chain");
    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_path,
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let chain_json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();
    assert_eq!(chain_json["c85"]["status"], "passed");
    let chain_path = chain_json["artifacts"]["pulse_chain"].as_str().unwrap();
    let contract_path = write_pulse_task_contract(
        temp.path(),
        "post-c85-governed-pulse-task",
        "Execute one post-C85 governed Pulse task contract",
        false,
    );
    let execute_out_dir = temp.path().join("pulse-execute");

    let execute = ao2([
        "pulse",
        "run",
        "--execute",
        "--chain-evidence",
        chain_path,
        "--task-contract",
        contract_path.to_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        execute_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(execute.status.success(), "{}", stderr(&execute));
    let json: serde_json::Value = serde_json::from_str(&stdout(&execute)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-executor.v1");
    assert_eq!(json["status"], "executed_governed_task");
    assert_eq!(json["c85"]["status"], "passed");
    assert_eq!(json["c85"]["hosted_github_actions_checked"], true);
    assert_eq!(json["c85"]["rerun_allowed_without_user_billing_fix"], true);
    assert_eq!(
        json["observed_inputs"]["packet_mentions_c85_deferred"],
        false
    );
    assert_eq!(json["observed_inputs"]["packet_mentions_c85_passed"], true);
    assert_eq!(json["prior_chain"]["schema_version"], "ao2.pulse-chain.v1");
    assert_eq!(json["prior_chain"]["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(json["selected_task"]["id"], "post-c85-governed-pulse-task");
    assert_eq!(json["selected_task"]["c85"], false);
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["hermes_cron_watchdog_mutation"], false);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);

    let persisted_path = PathBuf::from(json["artifacts"]["pulse_executor"].as_str().unwrap());
    assert_eq!(persisted_path, execute_out_dir.join("pulse-executor.json"));
    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&persisted_path).unwrap()).unwrap();
    assert_eq!(persisted["c85"]["status"], "passed");
    assert_eq!(
        persisted["observed_inputs"]["packet_mentions_c85_passed"],
        true
    );
    let governed_task_path = PathBuf::from(
        json["artifacts"]["governed_task_evidence"]
            .as_str()
            .unwrap(),
    );
    let governed_task: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&governed_task_path).unwrap()).unwrap();
    assert_eq!(governed_task["c85"]["status"], "passed");
    let task_result_path = PathBuf::from(json["artifacts"]["pulse_task_result"].as_str().unwrap());
    let task_result: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&task_result_path).unwrap()).unwrap();
    assert_eq!(task_result["c85"]["status"], "passed");
}

#[test]
fn cli_pulse_run_execute_consumes_chain_evidence_for_one_non_c85_task() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "codex-cron scheduler override; C85 hosted GitHub Actions proof remains deferred because billing is blocked",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse chain-mode is current. Exact Next Recommended Lengthy Task: implement the AO2 Pulse executor slice.",
    )
    .unwrap();
    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    let once_path = once_json["artifacts"]["pulse_once"].as_str().unwrap();
    let chain_out_dir = temp.path().join("pulse-chain");
    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_path,
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let chain_json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();
    let chain_path = chain_json["artifacts"]["pulse_chain"].as_str().unwrap();
    let contract_path = write_pulse_task_contract(
        temp.path(),
        "observe-pulse-once-and-select-next-safe-task",
        "Observe Pulse once-mode and select next safe task",
        false,
    );
    let execute_out_dir = temp.path().join("pulse-execute");

    let execute = ao2([
        "pulse",
        "run",
        "--execute",
        "--chain-evidence",
        chain_path,
        "--task-contract",
        contract_path.to_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        execute_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(execute.status.success(), "{}", stderr(&execute));
    let json: serde_json::Value = serde_json::from_str(&stdout(&execute)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-executor.v1");
    assert_eq!(json["status"], "executed_governed_task");
    assert_eq!(json["scheduler"]["active_runner"], "codex-cron");
    assert_eq!(json["scheduler"]["hermes_cron_mutated"], false);
    assert_eq!(json["c85"]["status"], "deferred");
    assert_eq!(json["c85"]["hosted_github_actions_checked"], false);
    assert_eq!(json["trust_boundary"]["control_plane_observer_only"], true);
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["hermes_cron_watchdog_mutation"], false);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);
    assert_eq!(json["prior_chain"]["schema_version"], "ao2.pulse-chain.v1");
    assert_eq!(json["prior_chain"]["sha256"].as_str().unwrap().len(), 64);
    let executed_tasks = json["executed_tasks"].as_array().unwrap();
    assert_eq!(executed_tasks.len(), 1);
    assert_eq!(
        executed_tasks[0]["id"],
        "observe-pulse-once-and-select-next-safe-task"
    );
    assert_eq!(executed_tasks[0]["c85"], false);
    assert_eq!(
        executed_tasks[0]["execution_kind"],
        "governed_task_contract"
    );
    assert_eq!(executed_tasks[0]["evaluator_closer"]["status"], "accepted");
    assert_eq!(json["selected_task"]["c85"], false);
    assert_eq!(
        json["selected_task"]["id"],
        "observe-pulse-once-and-select-next-safe-task"
    );

    let persisted_path = PathBuf::from(json["artifacts"]["pulse_executor"].as_str().unwrap());
    assert_eq!(persisted_path, execute_out_dir.join("pulse-executor.json"));
    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&persisted_path).unwrap()).unwrap();
    assert_eq!(persisted["schema_version"], "ao2.pulse-executor.v1");
    assert_eq!(
        persisted["prior_chain"]["sha256"],
        json["prior_chain"]["sha256"]
    );
}

#[test]
fn cli_pulse_run_execute_emits_governed_task_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "codex-cron scheduler override; C85 hosted GitHub Actions proof remains deferred because billing is blocked",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse executor evidence is current. Exact Next Recommended Lengthy Task: execute one governed non-C85 Pulse task with evaluator closer evidence.",
    )
    .unwrap();
    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    let once_path = once_json["artifacts"]["pulse_once"].as_str().unwrap();
    let chain_out_dir = temp.path().join("pulse-chain");
    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_path,
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let chain_json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();
    let chain_path = chain_json["artifacts"]["pulse_chain"].as_str().unwrap();
    let contract_path = write_pulse_task_contract(
        temp.path(),
        "pulse-governed-task-contract",
        "Execute one governed Pulse task contract",
        false,
    );
    let execute_out_dir = temp.path().join("pulse-execute");

    let execute = ao2([
        "pulse",
        "run",
        "--execute",
        "--chain-evidence",
        chain_path,
        "--task-contract",
        contract_path.to_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        execute_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(execute.status.success(), "{}", stderr(&execute));
    let json: serde_json::Value = serde_json::from_str(&stdout(&execute)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-executor.v1");
    assert_eq!(json["status"], "executed_governed_task");
    assert_eq!(json["selected_task"]["c85"], false);
    let executed_tasks = json["executed_tasks"].as_array().unwrap();
    assert_eq!(executed_tasks.len(), 1);
    assert_eq!(executed_tasks[0]["c85"], false);
    assert_eq!(
        executed_tasks[0]["execution_kind"],
        "governed_task_contract"
    );
    assert_eq!(executed_tasks[0]["evaluator_closer"]["status"], "accepted");
    assert_eq!(
        executed_tasks[0]["evaluator_closer"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["hermes_cron_watchdog_mutation"], false);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);

    let task_evidence_path = PathBuf::from(
        json["artifacts"]["governed_task_evidence"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(
        task_evidence_path,
        execute_out_dir.join("pulse-governed-task.json")
    );
    assert_eq!(
        json["artifacts"]["governed_task_evidence_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    let task_evidence: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&task_evidence_path).unwrap()).unwrap();
    assert_eq!(
        task_evidence["schema_version"],
        "ao2.pulse-governed-task.v1"
    );
    assert_eq!(task_evidence["status"], "accepted");
    assert_eq!(
        task_evidence["selected_task"]["id"],
        json["selected_task"]["id"]
    );
    assert_eq!(
        task_evidence["evaluator"]["decision"],
        "accept_non_c85_governed_task"
    );
    assert_eq!(task_evidence["closer"]["status"], "accepted");
    assert_eq!(
        task_evidence["closer"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
}

#[test]
fn cli_pulse_run_execute_emits_durable_task_result() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "codex-cron scheduler override; C85 hosted GitHub Actions proof remains deferred because billing is blocked",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse task-contract evidence is current. Exact Next Recommended Lengthy Task: execute one governed non-C85 task and persist a durable task result.",
    )
    .unwrap();
    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    let once_path = once_json["artifacts"]["pulse_once"].as_str().unwrap();
    let chain_out_dir = temp.path().join("pulse-chain");
    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_path,
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let chain_json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();
    let chain_path = chain_json["artifacts"]["pulse_chain"].as_str().unwrap();
    let contract_path = write_pulse_task_contract(
        temp.path(),
        "pulse-durable-task-result",
        "Persist durable Pulse task result",
        false,
    );
    let execute_out_dir = temp.path().join("pulse-execute");

    let execute = ao2([
        "pulse",
        "run",
        "--execute",
        "--chain-evidence",
        chain_path,
        "--task-contract",
        contract_path.to_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        execute_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(execute.status.success(), "{}", stderr(&execute));
    let json: serde_json::Value = serde_json::from_str(&stdout(&execute)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-executor.v1");
    let task_result_path = PathBuf::from(json["artifacts"]["pulse_task_result"].as_str().unwrap());
    assert_eq!(
        task_result_path,
        execute_out_dir.join("pulse-task-result.json")
    );
    assert_eq!(
        json["artifacts"]["pulse_task_result_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        json["artifacts"]["pulse_task_result_sha256"],
        sha256_path(&task_result_path)
    );

    let task_result: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&task_result_path).unwrap()).unwrap();
    assert_eq!(task_result["schema_version"], "ao2.pulse-task-result.v1");
    assert_eq!(task_result["status"], "accepted");
    assert_eq!(
        task_result["execution_mode"],
        "deterministic_local_evidence"
    );
    assert_eq!(
        task_result["selected_task"]["id"],
        "pulse-durable-task-result"
    );
    assert_eq!(task_result["selected_task"]["classification"], "COMPLEX");
    assert_eq!(task_result["selected_task"]["shape"], "refactor");
    assert_eq!(
        task_result["task_contract"]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        task_result["task_contract"]["sha256"],
        json["task_contract"]["sha256"]
    );
    assert_eq!(
        task_result["prior_chain"]["sha256"],
        json["prior_chain"]["sha256"]
    );
    assert_eq!(
        task_result["governed_task_evidence"]["path"],
        json["artifacts"]["governed_task_evidence"]
    );
    assert_eq!(
        task_result["governed_task_evidence"]["sha256"],
        json["artifacts"]["governed_task_evidence_sha256"]
    );
    assert_eq!(task_result["evaluator_closer"]["status"], "accepted");
    assert_eq!(
        task_result["evaluator_closer"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert_eq!(task_result["side_effects"]["provider_execution"], false);
    assert_eq!(task_result["side_effects"]["queue_execution"], false);
    assert_eq!(task_result["side_effects"]["memory_write"], false);
    assert_eq!(task_result["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(
        task_result["side_effects"]["hermes_cron_watchdog_mutation"],
        false
    );
    assert_eq!(task_result["side_effects"]["control_plane_mutation"], false);
}

#[test]
fn cli_pulse_run_execute_dry_run_task_emits_planned_file_operations() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "codex-cron scheduler override; C85 hosted GitHub Actions proof remains deferred because billing is blocked",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse task-result evidence is current. Exact Next Recommended Lengthy Task: add a bounded dry-run plugin/readiness task adapter.",
    )
    .unwrap();
    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    let once_path = once_json["artifacts"]["pulse_once"].as_str().unwrap();
    let chain_out_dir = temp.path().join("pulse-chain");
    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_path,
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let chain_json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();
    let chain_path = chain_json["artifacts"]["pulse_chain"].as_str().unwrap();
    let contract_path = write_pulse_task_contract(
        temp.path(),
        "pulse-plugin-readiness-dry-run",
        "Plan plugin readiness maintenance without mutation",
        false,
    );
    let execute_out_dir = temp.path().join("pulse-execute");

    let execute = ao2([
        "pulse",
        "run",
        "--execute",
        "--dry-run-task",
        "--chain-evidence",
        chain_path,
        "--task-contract",
        contract_path.to_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        execute_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(execute.status.success(), "{}", stderr(&execute));
    let json: serde_json::Value = serde_json::from_str(&stdout(&execute)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-executor.v1");
    assert_eq!(json["status"], "executed_dry_run_task");
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["hermes_cron_watchdog_mutation"], false);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);
    assert_eq!(
        json["artifacts"]["pulse_dry_run_task_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );

    let dry_run_path = PathBuf::from(json["artifacts"]["pulse_dry_run_task"].as_str().unwrap());
    assert_eq!(
        dry_run_path,
        execute_out_dir.join("pulse-dry-run-task.json")
    );
    assert_eq!(
        json["artifacts"]["pulse_dry_run_task_sha256"],
        sha256_path(&dry_run_path)
    );

    let dry_run: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&dry_run_path).unwrap()).unwrap();
    assert_eq!(dry_run["schema_version"], "ao2.pulse-dry-run-task.v1");
    assert_eq!(dry_run["status"], "planned_without_mutation");
    assert_eq!(dry_run["execution_mode"], "dry_run_planned_file_operations");
    assert_eq!(
        dry_run["selected_task"]["id"],
        "pulse-plugin-readiness-dry-run"
    );
    assert_eq!(
        dry_run["task_result"]["sha256"],
        json["artifacts"]["pulse_task_result_sha256"]
    );
    assert_eq!(
        dry_run["governed_task_evidence"]["sha256"],
        json["artifacts"]["governed_task_evidence_sha256"]
    );
    assert_eq!(
        dry_run["planned_file_operations"].as_array().unwrap().len(),
        3
    );
    assert_eq!(
        dry_run["planned_file_operations"][0]["path"],
        "docs/PLUGIN-SHIPMENT-RUNBOOK.md"
    );
    assert_eq!(
        dry_run["planned_file_operations"][0]["operation"],
        "inspect_current_plugin_readiness_line"
    );
    for operation in dry_run["planned_file_operations"].as_array().unwrap() {
        assert_eq!(operation["executed"], false);
        assert_eq!(operation["mode"], "planned_only");
    }
    assert_eq!(dry_run["side_effects"]["provider_execution"], false);
    assert_eq!(dry_run["side_effects"]["queue_execution"], false);
    assert_eq!(dry_run["side_effects"]["memory_write"], false);
    assert_eq!(dry_run["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(dry_run["side_effects"]["control_plane_mutation"], false);
}

#[test]
fn cli_pulse_run_execute_apply_dry_run_writes_only_planned_operations() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "codex-cron scheduler override; C85 hosted GitHub Actions proof remains deferred because billing is blocked",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse dry-run task evidence is current. Exact Next Recommended Lengthy Task: apply the bounded non-C85 plugin/readiness maintenance task.",
    )
    .unwrap();
    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    let once_path = once_json["artifacts"]["pulse_once"].as_str().unwrap();
    let chain_out_dir = temp.path().join("pulse-chain");
    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_path,
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let chain_json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();
    let chain_path = chain_json["artifacts"]["pulse_chain"].as_str().unwrap();
    let contract_path = write_pulse_task_contract(
        temp.path(),
        "pulse-plugin-readiness-apply",
        "Apply plugin readiness maintenance through AO2",
        false,
    );
    let dry_run_out_dir = temp.path().join("pulse-dry-run");
    let dry_run_execute = ao2([
        "pulse",
        "run",
        "--execute",
        "--dry-run-task",
        "--chain-evidence",
        chain_path,
        "--task-contract",
        contract_path.to_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        dry_run_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        dry_run_execute.status.success(),
        "{}",
        stderr(&dry_run_execute)
    );
    let dry_run_json: serde_json::Value = serde_json::from_str(&stdout(&dry_run_execute)).unwrap();
    let dry_run_path = dry_run_json["artifacts"]["pulse_dry_run_task"]
        .as_str()
        .unwrap();
    let dry_run_sha256 = dry_run_json["artifacts"]["pulse_dry_run_task_sha256"]
        .as_str()
        .unwrap();
    let apply_root = temp.path().join("apply-root");
    fs::create_dir_all(apply_root.join("docs")).unwrap();
    fs::write(
        apply_root.join("docs").join("PLUGIN-SHIPMENT-RUNBOOK.md"),
        "# AO2 Codex/Claude Plugin Shipment Runbook\n\nExisting operator line.\n",
    )
    .unwrap();
    let apply_out_dir = temp.path().join("pulse-apply");

    let apply = ao2([
        "pulse",
        "run",
        "--execute",
        "--apply-dry-run",
        "--dry-run-evidence",
        dry_run_path,
        "--dry-run-sha256",
        dry_run_sha256,
        "--apply-root",
        apply_root.to_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        apply_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(apply.status.success(), "{}", stderr(&apply));
    let json: serde_json::Value = serde_json::from_str(&stdout(&apply)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-executor.v1");
    assert_eq!(json["status"], "applied_dry_run_task");
    assert_eq!(
        json["artifacts"]["pulse_apply_result_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    let apply_result_path =
        PathBuf::from(json["artifacts"]["pulse_apply_result"].as_str().unwrap());
    assert_eq!(
        apply_result_path,
        apply_out_dir.join("pulse-apply-result.json")
    );
    assert_eq!(
        json["artifacts"]["pulse_apply_result_sha256"],
        sha256_path(&apply_result_path)
    );

    let apply_result: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&apply_result_path).unwrap()).unwrap();
    assert_eq!(apply_result["schema_version"], "ao2.pulse-apply-result.v1");
    assert_eq!(apply_result["status"], "accepted");
    assert_eq!(apply_result["execution_mode"], "bounded_planned_file_apply");
    assert_eq!(
        apply_result["dry_run_task"]["sha256"].as_str(),
        Some(dry_run_sha256)
    );
    assert_eq!(
        apply_result["applied_file_operations"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    for operation in apply_result["applied_file_operations"].as_array().unwrap() {
        assert_eq!(operation["allowed_by_dry_run"], true);
    }
    assert_eq!(
        apply_result["applied_file_operations"][0]["operation"],
        "inspect_current_plugin_readiness_line"
    );
    assert_eq!(apply_result["applied_file_operations"][0]["executed"], true);
    assert_eq!(
        apply_result["applied_file_operations"][1]["path"],
        "docs/status/codex-cron-pulse-apply-result-final.md"
    );
    assert_eq!(
        apply_result["applied_file_operations"][2]["path"],
        "docs/status/hermes-governed-backend-control-plane/codex-cron-pulse-apply-result-final.md"
    );
    assert_eq!(apply_result["side_effects"]["provider_execution"], false);
    assert_eq!(apply_result["side_effects"]["queue_execution"], false);
    assert_eq!(apply_result["side_effects"]["memory_write"], false);
    assert_eq!(apply_result["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(
        apply_result["side_effects"]["hermes_cron_watchdog_mutation"],
        false
    );
    assert_eq!(
        apply_result["side_effects"]["control_plane_mutation"],
        false
    );

    let runbook = fs::read_to_string(apply_root.join("docs/PLUGIN-SHIPMENT-RUNBOOK.md")).unwrap();
    assert!(runbook.contains("AO2 Pulse apply evidence"));
    assert!(apply_root
        .join("docs/status/codex-cron-pulse-apply-result-final.md")
        .exists());
    assert!(apply_root
        .join("docs/status/hermes-governed-backend-control-plane/codex-cron-pulse-apply-result-final.md")
        .exists());
    assert!(!apply_root.join(".git").exists());
    assert!(!apply_root.join("ao").exists());
}

#[test]
fn cli_pulse_run_execute_consumes_bounded_task_contract() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "codex-cron scheduler override; C85 hosted GitHub Actions proof remains deferred because billing is blocked",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(
        &board,
        "AO2 Pulse executor evidence is current. Exact Next Recommended Lengthy Task: consume a bounded task contract.",
    )
    .unwrap();
    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    let once_path = once_json["artifacts"]["pulse_once"].as_str().unwrap();
    let chain_out_dir = temp.path().join("pulse-chain");
    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_path,
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let chain_json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();
    let chain_path = chain_json["artifacts"]["pulse_chain"].as_str().unwrap();
    let contract_path = temp.path().join("task-contract.json");
    fs::write(
        &contract_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-task-contract.v1",
            "id": "pulse-refresh-plugin-runbook",
            "title": "Refresh plugin operator runbook proof line",
            "classification": "COMPLEX",
            "shape": "refactor",
            "c85": false,
            "ao2_owned_execution": true,
            "factory_v3_evaluator_closer_required": true,
            "evaluator_acceptance": "accept_non_c85_governed_task",
            "closer_acceptance": "accepted",
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
    let contract_sha256 = sha256_path(&contract_path);
    let execute_out_dir = temp.path().join("pulse-execute");

    let execute = ao2([
        "pulse",
        "run",
        "--execute",
        "--chain-evidence",
        chain_path,
        "--task-contract",
        contract_path.to_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        execute_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(execute.status.success(), "{}", stderr(&execute));
    let json: serde_json::Value = serde_json::from_str(&stdout(&execute)).unwrap();

    assert_eq!(json["schema_version"], "ao2.pulse-executor.v1");
    assert_eq!(json["status"], "executed_governed_task");
    assert_eq!(json["selected_task"]["id"], "pulse-refresh-plugin-runbook");
    assert_eq!(
        json["selected_task"]["title"],
        "Refresh plugin operator runbook proof line"
    );
    assert_eq!(json["selected_task"]["classification"], "COMPLEX");
    assert_eq!(json["selected_task"]["shape"], "refactor");
    assert_eq!(
        json["task_contract"]["schema_version"],
        "ao2.pulse-task-contract.v1"
    );
    assert_eq!(
        json["task_contract"]["path"],
        contract_path.display().to_string()
    );
    assert_eq!(json["task_contract"]["sha256"], contract_sha256);
    assert_eq!(json["side_effects"]["provider_execution"], false);
    assert_eq!(json["side_effects"]["queue_execution"], false);
    assert_eq!(json["side_effects"]["memory_write"], false);
    assert_eq!(json["side_effects"]["mutates_ao_artifacts"], false);
    assert_eq!(json["side_effects"]["hermes_cron_watchdog_mutation"], false);
    assert_eq!(json["side_effects"]["control_plane_mutation"], false);

    let task_evidence_path = PathBuf::from(
        json["artifacts"]["governed_task_evidence"]
            .as_str()
            .unwrap(),
    );
    let task_evidence: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&task_evidence_path).unwrap()).unwrap();
    assert_eq!(
        task_evidence["task_contract"]["sha256"],
        json["task_contract"]["sha256"]
    );
    assert_eq!(
        task_evidence["selected_task"]["id"],
        "pulse-refresh-plugin-runbook"
    );
    assert_eq!(
        task_evidence["executed_task"]["evaluator_closer"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
}

#[test]
fn cli_pulse_run_execute_rejects_c85_task_contract() {
    let temp = tempfile::tempdir().unwrap();
    let packet = temp.path().join("prompt.txt");
    fs::write(
        &packet,
        "codex-cron scheduler override; C85 hosted GitHub Actions proof remains deferred because billing is blocked",
    )
    .unwrap();
    let board = temp.path().join("BOARD.md");
    fs::write(&board, "AO2 Pulse contract validation").unwrap();
    let once_out_dir = temp.path().join("pulse-once");
    let once = ao2([
        "pulse",
        "run",
        "--once",
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        once_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(once.status.success(), "{}", stderr(&once));
    let once_json: serde_json::Value = serde_json::from_str(&stdout(&once)).unwrap();
    let once_path = once_json["artifacts"]["pulse_once"].as_str().unwrap();
    let chain_out_dir = temp.path().join("pulse-chain");
    let chain = ao2([
        "pulse",
        "run",
        "--chain",
        "--once-evidence",
        once_path,
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        chain_out_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(chain.status.success(), "{}", stderr(&chain));
    let chain_json: serde_json::Value = serde_json::from_str(&stdout(&chain)).unwrap();
    let chain_path = chain_json["artifacts"]["pulse_chain"].as_str().unwrap();
    let contract_path = temp.path().join("c85-task-contract.json");
    fs::write(
        &contract_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.pulse-task-contract.v1",
            "id": "c85-hosted-actions-proof",
            "title": "Hosted GitHub Actions proof",
            "classification": "COMPLEX",
            "shape": "greenfield",
            "c85": true,
            "ao2_owned_execution": true,
            "factory_v3_evaluator_closer_required": true,
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

    let execute = ao2([
        "pulse",
        "run",
        "--execute",
        "--chain-evidence",
        chain_path,
        "--task-contract",
        contract_path.to_str().unwrap(),
        "--packet",
        packet.to_str().unwrap(),
        "--board",
        board.to_str().unwrap(),
        "--out-dir",
        temp.path().join("pulse-execute").to_str().unwrap(),
        "--json",
    ]);

    assert!(!execute.status.success());
    assert!(
        stderr(&execute).contains("ao2 pulse run --execute refuses C85 task contracts"),
        "{}",
        stderr(&execute)
    );
}

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
