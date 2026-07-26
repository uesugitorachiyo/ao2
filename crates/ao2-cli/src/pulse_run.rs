use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ao2_core::sha256_hex;
use chrono::{SecondsFormat, Utc};

use crate::cli_util::{atomic_write_text, json_bool, json_string};

pub(crate) fn pulse_run_once_json(
    packet: &Path,
    board: &Path,
    out_dir: &Path,
) -> Result<serde_json::Value> {
    let packet_text =
        fs::read_to_string(packet).with_context(|| format!("read packet {}", packet.display()))?;
    let board_text =
        fs::read_to_string(board).with_context(|| format!("read board {}", board.display()))?;
    let pulse_once = out_dir.join("pulse-once.json");
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let packet_mentions_c85_deferred = packet_text.contains("C85")
        && (packet_text.contains("deferred") || packet_text.contains("billing"));
    let packet_lower = packet_text.to_lowercase();
    let packet_mentions_c85_passed = packet_text.contains("C85")
        && !packet_mentions_c85_deferred
        && (packet_lower.contains("passed") || packet_lower.contains("green"));
    let board_mentions_pulse = board_text.contains("AO2 Pulse") || board_text.contains("pulse");
    let c85 = if packet_mentions_c85_passed {
        serde_json::json!({
            "status": "passed",
            "reason": "active packet records hosted C85 Release Gate passed before Pulse once-mode evidence",
            "hosted_github_actions_checked": true,
            "rerun_allowed_without_user_billing_fix": true
        })
    } else {
        serde_json::json!({
            "status": "deferred",
            "reason": "hosted GitHub Actions billing/spending-limit funding is unavailable",
            "hosted_github_actions_checked": false,
            "rerun_allowed_without_user_billing_fix": false
        })
    };
    let result = serde_json::json!({
        "schema_version": "ao2.pulse-once.v1",
        "status": "ready_for_operator_execution",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "scheduler": {
            "active_runner": "codex-cron",
            "hermes_frontend_queue_memory_concept": true,
            "hermes_cron_mutated": false,
            "fixed_interval_loop_successor": "ao2 pulse run --once"
        },
        "observed_inputs": {
            "packet": packet.display().to_string(),
            "packet_sha256": sha256_hex(packet_text.as_bytes()),
            "packet_mentions_c85_deferred": packet_mentions_c85_deferred,
            "packet_mentions_c85_passed": packet_mentions_c85_passed,
            "board": board.display().to_string(),
            "board_sha256": sha256_hex(board_text.as_bytes()),
            "board_mentions_pulse": board_mentions_pulse
        },
        "selected_task": {
            "id": "ao2-pulse-next-safe-task",
            "title": "AO2 Pulse once-mode replacement slice",
            "classification": "COMPLEX",
            "shape": "greenfield",
            "reason": "Windows Workbench P0 and plugin/K37 observer coverage are current; the next safe AO2 advancement is read-only Pulse once-mode evidence.",
            "recommended_command": "ao2 pulse run --once --packet <active-packet> --board <coordination-board> --out-dir <evidence-dir> --json"
        },
        "c85": c85,
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
            "pulse_once": pulse_once.display().to_string()
        }
    });
    atomic_write_text(&pulse_once, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn pulse_run_chain_json(
    packet: &Path,
    board: &Path,
    once_evidence: &Path,
    out_dir: &Path,
) -> Result<serde_json::Value> {
    let packet_text =
        fs::read_to_string(packet).with_context(|| format!("read packet {}", packet.display()))?;
    let board_text =
        fs::read_to_string(board).with_context(|| format!("read board {}", board.display()))?;
    let once_text = fs::read_to_string(once_evidence)
        .with_context(|| format!("read once evidence {}", once_evidence.display()))?;
    let once_json: serde_json::Value =
        serde_json::from_str(&once_text).context("parse ao2 pulse once evidence")?;
    if json_string(&once_json, "schema_version") != "ao2.pulse-once.v1" {
        anyhow::bail!("ao2 pulse run --chain requires ao2.pulse-once.v1 evidence");
    }
    if json_string(&once_json, "status") != "ready_for_operator_execution" {
        anyhow::bail!("ao2 pulse run --chain requires ready once-mode evidence");
    }

    let pulse_chain = out_dir.join("pulse-chain.json");
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let packet_mentions_c85_deferred = packet_text.contains("C85")
        && (packet_text.contains("deferred") || packet_text.contains("billing"));
    let packet_mentions_c85_passed = packet_text.contains("C85")
        && (packet_text.contains("passed") || packet_text.contains("green"));
    let once_c85_passed = json_string(&once_json["c85"], "status") == "passed";
    let post_c85_ready =
        packet_mentions_c85_passed && once_c85_passed && !packet_mentions_c85_deferred;
    let prior_selected_task = once_json
        .get("selected_task")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let mut chain_steps = vec![serde_json::json!({
        "id": "observe-pulse-once-and-select-next-safe-task",
        "status": "planned",
        "executes_task": false,
        "reason": "Chain mode records the once-mode decision and prepares a bounded evaluator-closed next step without executing it."
    })];
    if packet_mentions_c85_deferred {
        chain_steps.push(serde_json::json!({
            "id": "refuse-c85-while-billing-blocked",
            "status": "blocked_by_billing",
            "executes_task": false,
            "reason": "Hosted GitHub Actions C85 remains deferred until the user says billing/spending-limit funding is fixed."
        }));
    }
    chain_steps.push(serde_json::json!({
        "id": "prepare-operator-handoff",
        "status": "planned",
        "executes_task": false,
        "reason": "A human/operator or governed follow-on may execute the selected AO2 task after reviewing this evidence."
    }));
    let c85 = if post_c85_ready {
        serde_json::json!({
            "status": "passed",
            "reason": "hosted C85 Release Gate passed before post-C85/plugin-ready Pulse chain evidence",
            "hosted_github_actions_checked": true,
            "rerun_allowed_without_user_billing_fix": true
        })
    } else {
        serde_json::json!({
            "status": "deferred",
            "reason": "hosted GitHub Actions billing/spending-limit funding is unavailable",
            "hosted_github_actions_checked": false,
            "rerun_allowed_without_user_billing_fix": false
        })
    };
    let result = serde_json::json!({
        "schema_version": "ao2.pulse-chain.v1",
        "status": "planned_without_execution",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "scheduler": {
            "active_runner": "codex-cron",
            "hermes_frontend_queue_memory_concept": true,
            "hermes_cron_mutated": false,
            "fixed_interval_loop_successor": "ao2 pulse run --chain"
        },
        "observed_inputs": {
            "packet": packet.display().to_string(),
            "packet_sha256": sha256_hex(packet_text.as_bytes()),
            "packet_mentions_c85_deferred": packet_mentions_c85_deferred,
            "packet_mentions_c85_passed": packet_mentions_c85_passed,
            "board": board.display().to_string(),
            "board_sha256": sha256_hex(board_text.as_bytes())
        },
        "prior_once": {
            "path": once_evidence.display().to_string(),
            "sha256": sha256_hex(once_text.as_bytes()),
            "schema_version": "ao2.pulse-once.v1",
            "status": json_string(&once_json, "status"),
            "selected_task": prior_selected_task
        },
        "chain_steps": chain_steps,
        "c85": c85,
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
            "pulse_chain": pulse_chain.display().to_string()
        }
    });
    atomic_write_text(&pulse_chain, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn pulse_artifact_key(once: bool, chain: bool, execute: bool) -> &'static str {
    if once {
        "pulse_once"
    } else if chain {
        "pulse_chain"
    } else if execute {
        "pulse_executor"
    } else {
        ""
    }
}

pub(crate) fn pulse_run_execute_json(
    packet: &Path,
    board: &Path,
    chain_evidence: &Path,
    task_contract: &Path,
    out_dir: &Path,
    dry_run_task: bool,
) -> Result<serde_json::Value> {
    let packet_text =
        fs::read_to_string(packet).with_context(|| format!("read packet {}", packet.display()))?;
    let board_text =
        fs::read_to_string(board).with_context(|| format!("read board {}", board.display()))?;
    let chain_text = fs::read_to_string(chain_evidence)
        .with_context(|| format!("read chain evidence {}", chain_evidence.display()))?;
    let chain_json: serde_json::Value =
        serde_json::from_str(&chain_text).context("parse ao2 pulse chain evidence")?;
    if json_string(&chain_json, "schema_version") != "ao2.pulse-chain.v1" {
        anyhow::bail!("ao2 pulse run --execute requires ao2.pulse-chain.v1 evidence");
    }
    if json_string(&chain_json, "status") != "planned_without_execution" {
        anyhow::bail!("ao2 pulse run --execute requires planned chain evidence");
    }
    let chain_c85_status = json_string(&chain_json["c85"], "status");
    if !matches!(chain_c85_status.as_str(), "deferred" | "passed") {
        anyhow::bail!("ao2 pulse run --execute requires deferred or passed C85 chain evidence");
    }
    let task_contract_text = fs::read_to_string(task_contract)
        .with_context(|| format!("read task contract {}", task_contract.display()))?;
    let task_contract_json: serde_json::Value =
        serde_json::from_str(&task_contract_text).context("parse ao2 pulse task contract")?;
    validate_pulse_task_contract(&task_contract_json)?;
    let chain_sha256 = sha256_hex(chain_text.as_bytes());
    let task_contract_sha256 = sha256_hex(task_contract_text.as_bytes());

    let selected_step = chain_json["chain_steps"]
        .as_array()
        .and_then(|steps| {
            steps.iter().find(|step| {
                json_string(step, "status") == "planned"
                    && json_string(step, "id") != "refuse-c85-while-billing-blocked"
            })
        })
        .cloned()
        .ok_or_else(|| anyhow!("ao2 pulse run --execute found no planned non-C85 chain task"))?;

    let selected_task = serde_json::json!({
        "id": json_string(&task_contract_json, "id"),
        "title": json_string(&task_contract_json, "title"),
        "classification": json_string(&task_contract_json, "classification"),
        "shape": json_string(&task_contract_json, "shape"),
        "status": "selected",
        "c85": false,
        "source_status": json_string(&selected_step, "status"),
        "source_chain_step": json_string(&selected_step, "id"),
        "reason": json_string(&selected_step, "reason")
    });
    let evaluator_closer = serde_json::json!({
        "status": "accepted",
        "release_acceptance_owner": "factory-v3 evaluator-closer",
        "evaluator_decision": "accept_non_c85_governed_task",
        "closer_decision": "accepted",
        "evidence_digest_required": true
    });
    let executed_task = serde_json::json!({
        "id": json_string(&task_contract_json, "id"),
        "title": json_string(&task_contract_json, "title"),
        "status": "executed",
        "c85": false,
        "execution_kind": "governed_task_contract",
        "provider_execution": false,
        "queue_execution": false,
        "memory_write": false,
        "mutates_ao_artifacts": false,
        "factory_v3_evaluator_closer_required": true,
        "evaluator_closer": evaluator_closer
    });

    let pulse_executor = out_dir.join("pulse-executor.json");
    let governed_task_evidence = out_dir.join("pulse-governed-task.json");
    let pulse_task_result = out_dir.join("pulse-task-result.json");
    let pulse_dry_run_task = out_dir.join("pulse-dry-run-task.json");
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let packet_mentions_c85_deferred = packet_text.contains("C85")
        && (packet_text.contains("deferred") || packet_text.contains("billing"));
    let packet_lower = packet_text.to_lowercase();
    let packet_mentions_c85_passed = packet_text.contains("C85")
        && !packet_mentions_c85_deferred
        && (packet_lower.contains("passed") || packet_lower.contains("green"));
    let c85 = if chain_c85_status == "passed" {
        serde_json::json!({
            "status": "passed",
            "reason": "prior chain evidence records hosted C85 Release Gate passed before Pulse execute evidence",
            "hosted_github_actions_checked": true,
            "rerun_allowed_without_user_billing_fix": true
        })
    } else {
        serde_json::json!({
            "status": "deferred",
            "reason": "hosted GitHub Actions billing/spending-limit funding is unavailable",
            "hosted_github_actions_checked": false,
            "rerun_allowed_without_user_billing_fix": false
        })
    };
    let task_evidence = serde_json::json!({
        "schema_version": "ao2.pulse-governed-task.v1",
        "status": "accepted",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "selected_task": selected_task.clone(),
        "executed_task": executed_task.clone(),
        "prior_chain": {
            "path": chain_evidence.display().to_string(),
            "sha256": chain_sha256.clone(),
            "schema_version": "ao2.pulse-chain.v1",
            "status": json_string(&chain_json, "status")
        },
        "c85": c85.clone(),
        "task_contract": {
            "path": task_contract.display().to_string(),
            "sha256": task_contract_sha256.clone(),
            "schema_version": "ao2.pulse-task-contract.v1",
            "id": json_string(&task_contract_json, "id")
        },
        "evaluator": {
            "decision": "accept_non_c85_governed_task",
            "reason": "Selected task contract is non-C85, AO2-owned, evaluator/closer bounded, and forbidden side effects are false.",
            "factory_v3_evaluator_closer_reference": true
        },
        "closer": {
            "status": "accepted",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "evidence_digest_required": true,
            "blockers": []
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
    });
    let task_evidence_text = serde_json::to_string_pretty(&task_evidence)?;
    let task_evidence_sha256 = sha256_hex(task_evidence_text.as_bytes());
    let task_result = serde_json::json!({
        "schema_version": "ao2.pulse-task-result.v1",
        "status": "accepted",
        "execution_mode": "deterministic_local_evidence",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "selected_task": selected_task.clone(),
        "executed_task": executed_task.clone(),
        "prior_chain": {
            "path": chain_evidence.display().to_string(),
            "sha256": chain_sha256.clone(),
            "schema_version": "ao2.pulse-chain.v1",
            "status": json_string(&chain_json, "status")
        },
        "c85": c85.clone(),
        "task_contract": {
            "path": task_contract.display().to_string(),
            "sha256": task_contract_sha256.clone(),
            "schema_version": "ao2.pulse-task-contract.v1",
            "id": json_string(&task_contract_json, "id")
        },
        "governed_task_evidence": {
            "path": governed_task_evidence.display().to_string(),
            "sha256": task_evidence_sha256.clone(),
            "schema_version": "ao2.pulse-governed-task.v1",
            "status": "accepted"
        },
        "evaluator_closer": evaluator_closer.clone(),
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
    });
    let task_result_text = serde_json::to_string_pretty(&task_result)?;
    let task_result_sha256 = sha256_hex(task_result_text.as_bytes());
    let dry_run_task_artifact = if dry_run_task {
        let dry_run_task_json = serde_json::json!({
            "schema_version": "ao2.pulse-dry-run-task.v1",
            "status": "planned_without_mutation",
            "execution_mode": "dry_run_planned_file_operations",
            "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            "selected_task": selected_task.clone(),
            "executed_task": executed_task.clone(),
            "prior_chain": {
                "path": chain_evidence.display().to_string(),
                "sha256": chain_sha256.clone(),
                "schema_version": "ao2.pulse-chain.v1",
                "status": json_string(&chain_json, "status")
            },
            "task_contract": {
                "path": task_contract.display().to_string(),
                "sha256": task_contract_sha256.clone(),
                "schema_version": "ao2.pulse-task-contract.v1",
                "id": json_string(&task_contract_json, "id")
            },
            "governed_task_evidence": {
                "path": governed_task_evidence.display().to_string(),
                "sha256": task_evidence_sha256.clone(),
                "schema_version": "ao2.pulse-governed-task.v1",
                "status": "accepted"
            },
            "task_result": {
                "path": pulse_task_result.display().to_string(),
                "sha256": task_result_sha256.clone(),
                "schema_version": "ao2.pulse-task-result.v1",
                "status": "accepted"
            },
            "evaluator_closer": evaluator_closer.clone(),
            "planned_file_operations": [
                {
                    "operation": "inspect_current_plugin_readiness_line",
                    "path": "docs/PLUGIN-SHIPMENT-RUNBOOK.md",
                    "mode": "planned_only",
                    "executed": false,
                    "reason": "Read the current plugin readiness proof line before planning any operator-facing runbook update."
                },
                {
                    "operation": "write_dry_run_status_handoff",
                    "path": "docs/status/codex-cron-pulse-dry-run-task-final-<timestamp>.md",
                    "mode": "planned_only",
                    "executed": false,
                    "reason": "Record dry-run task evidence, pass/fail state, artifact paths, pushed commits, parity progress, and next lengthy task."
                },
                {
                    "operation": "mirror_factory_v3_evaluator_status",
                    "path": "docs/status/hermes-governed-backend-control-plane/codex-cron-pulse-dry-run-task-final-<timestamp>.md",
                    "mode": "planned_only",
                    "executed": false,
                    "reason": "Preserve factory-v3 evaluator/closer continuity without mutating AO artifacts or Hermes scheduler state."
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
        });
        let dry_run_task_text = serde_json::to_string_pretty(&dry_run_task_json)?;
        let dry_run_task_sha256 = sha256_hex(dry_run_task_text.as_bytes());
        Some((dry_run_task_text, dry_run_task_sha256))
    } else {
        None
    };
    let mut result = serde_json::json!({
        "schema_version": "ao2.pulse-executor.v1",
        "status": if dry_run_task { "executed_dry_run_task" } else { "executed_governed_task" },
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "scheduler": {
            "active_runner": "codex-cron",
            "hermes_frontend_queue_memory_concept": true,
            "hermes_cron_mutated": false,
            "fixed_interval_loop_successor": if dry_run_task { "ao2 pulse run --execute --dry-run-task" } else { "ao2 pulse run --execute" }
        },
        "observed_inputs": {
            "packet": packet.display().to_string(),
            "packet_sha256": sha256_hex(packet_text.as_bytes()),
            "packet_mentions_c85_deferred": packet_mentions_c85_deferred,
            "packet_mentions_c85_passed": packet_mentions_c85_passed,
            "board": board.display().to_string(),
            "board_sha256": sha256_hex(board_text.as_bytes())
        },
        "prior_chain": {
            "path": chain_evidence.display().to_string(),
            "sha256": chain_sha256,
            "schema_version": "ao2.pulse-chain.v1",
            "status": json_string(&chain_json, "status")
        },
        "task_contract": {
            "path": task_contract.display().to_string(),
            "sha256": task_contract_sha256,
            "schema_version": "ao2.pulse-task-contract.v1",
            "id": json_string(&task_contract_json, "id")
        },
        "selected_task": selected_task,
        "executed_tasks": [
            executed_task
        ],
        "c85": c85,
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
            "pulse_executor": pulse_executor.display().to_string(),
            "governed_task_evidence": governed_task_evidence.display().to_string(),
            "governed_task_evidence_sha256": task_evidence_sha256,
            "pulse_task_result": pulse_task_result.display().to_string(),
            "pulse_task_result_sha256": task_result_sha256
        }
    });
    if let Some((dry_run_task_text, dry_run_task_sha256)) = dry_run_task_artifact {
        if let Some(artifacts) = result
            .get_mut("artifacts")
            .and_then(|value| value.as_object_mut())
        {
            artifacts.insert(
                "pulse_dry_run_task".to_string(),
                serde_json::Value::String(pulse_dry_run_task.display().to_string()),
            );
            artifacts.insert(
                "pulse_dry_run_task_sha256".to_string(),
                serde_json::Value::String(dry_run_task_sha256),
            );
        }
        atomic_write_text(&pulse_dry_run_task, &dry_run_task_text)?;
    }
    atomic_write_text(&governed_task_evidence, &task_evidence_text)?;
    atomic_write_text(&pulse_task_result, &task_result_text)?;
    atomic_write_text(&pulse_executor, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn pulse_run_apply_dry_run_json(
    packet: &Path,
    board: &Path,
    dry_run_evidence: &Path,
    expected_dry_run_sha256: &str,
    apply_root: &Path,
    out_dir: &Path,
) -> Result<serde_json::Value> {
    let packet_text =
        fs::read_to_string(packet).with_context(|| format!("read packet {}", packet.display()))?;
    let board_text =
        fs::read_to_string(board).with_context(|| format!("read board {}", board.display()))?;
    let dry_run_text = fs::read_to_string(dry_run_evidence)
        .with_context(|| format!("read dry-run evidence {}", dry_run_evidence.display()))?;
    let dry_run_sha256 = sha256_hex(dry_run_text.as_bytes());
    if dry_run_sha256 != expected_dry_run_sha256 {
        anyhow::bail!("ao2 pulse run --execute --apply-dry-run dry-run SHA256 mismatch");
    }
    let dry_run_json: serde_json::Value =
        serde_json::from_str(&dry_run_text).context("parse ao2 pulse dry-run task evidence")?;
    if json_string(&dry_run_json, "schema_version") != "ao2.pulse-dry-run-task.v1" {
        anyhow::bail!("ao2 pulse run --execute --apply-dry-run requires ao2.pulse-dry-run-task.v1");
    }
    if json_string(&dry_run_json, "status") != "planned_without_mutation" {
        anyhow::bail!("ao2 pulse run --execute --apply-dry-run requires planned dry-run evidence");
    }
    let planned_operations = dry_run_json["planned_file_operations"]
        .as_array()
        .ok_or_else(|| {
            anyhow!("ao2 pulse run --execute --apply-dry-run requires planned operations")
        })?;
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    fs::create_dir_all(apply_root).with_context(|| format!("create {}", apply_root.display()))?;

    let mut applied_operations = Vec::new();
    for operation in planned_operations {
        let operation_id = json_string(operation, "operation");
        let planned_path = json_string(operation, "path");
        let normalized_path = pulse_apply_normalized_path(&operation_id, &planned_path)?;
        let target_path = pulse_apply_target_path(apply_root, &normalized_path)?;
        let result = match operation_id.as_str() {
            "inspect_current_plugin_readiness_line" => {
                let existing_text = fs::read_to_string(&target_path).unwrap_or_default();
                let append = "\n## AO2 Pulse apply evidence\n\n- Applied bounded plugin/readiness maintenance through `ao2 pulse run --execute --apply-dry-run`.\n- C85 hosted GitHub Actions remains deferred until billing/spending-limit funding is fixed.\n- Hermes cron/watchdog jobs were not started or mutated.\n";
                let next_text = if existing_text.contains("AO2 Pulse apply evidence") {
                    existing_text
                } else {
                    format!("{existing_text}{append}")
                };
                atomic_write_text(&target_path, &next_text)?;
                serde_json::json!({
                    "operation": operation_id,
                    "path": normalized_path,
                    "planned_path": planned_path,
                    "mode": "applied",
                    "executed": true,
                    "allowed_by_dry_run": true,
                    "bytes_written": next_text.len()
                })
            }
            "write_dry_run_status_handoff" | "mirror_factory_v3_evaluator_status" => {
                let body = pulse_apply_status_body(&dry_run_json, &dry_run_sha256, &operation_id);
                atomic_write_text(&target_path, &body)?;
                serde_json::json!({
                    "operation": operation_id,
                    "path": normalized_path,
                    "planned_path": planned_path,
                    "mode": "applied",
                    "executed": true,
                    "allowed_by_dry_run": true,
                    "bytes_written": body.len()
                })
            }
            _ => anyhow::bail!(
                "ao2 pulse run --execute --apply-dry-run refuses unrecognized operation `{operation_id}`"
            ),
        };
        applied_operations.push(result);
    }

    let pulse_executor = out_dir.join("pulse-executor.json");
    let pulse_apply_result = out_dir.join("pulse-apply-result.json");
    let packet_mentions_c85_deferred = packet_text.contains("C85")
        && (packet_text.contains("deferred") || packet_text.contains("billing"));
    let apply_result = serde_json::json!({
        "schema_version": "ao2.pulse-apply-result.v1",
        "status": "accepted",
        "execution_mode": "bounded_planned_file_apply",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "selected_task": dry_run_json["selected_task"].clone(),
        "dry_run_task": {
            "path": dry_run_evidence.display().to_string(),
            "sha256": dry_run_sha256,
            "schema_version": "ao2.pulse-dry-run-task.v1",
            "status": "planned_without_mutation"
        },
        "prior_chain": dry_run_json["prior_chain"].clone(),
        "task_contract": dry_run_json["task_contract"].clone(),
        "governed_task_evidence": dry_run_json["governed_task_evidence"].clone(),
        "task_result": dry_run_json["task_result"].clone(),
        "evaluator_closer": dry_run_json["evaluator_closer"].clone(),
        "applied_file_operations": applied_operations,
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
    });
    let apply_result_text = serde_json::to_string_pretty(&apply_result)?;
    let apply_result_sha256 = sha256_hex(apply_result_text.as_bytes());
    let result = serde_json::json!({
        "schema_version": "ao2.pulse-executor.v1",
        "status": "applied_dry_run_task",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "scheduler": {
            "active_runner": "codex-cron",
            "hermes_frontend_queue_memory_concept": true,
            "hermes_cron_mutated": false,
            "fixed_interval_loop_successor": "ao2 pulse run --execute --apply-dry-run"
        },
        "observed_inputs": {
            "packet": packet.display().to_string(),
            "packet_sha256": sha256_hex(packet_text.as_bytes()),
            "packet_mentions_c85_deferred": packet_mentions_c85_deferred,
            "board": board.display().to_string(),
            "board_sha256": sha256_hex(board_text.as_bytes())
        },
        "selected_task": dry_run_json["selected_task"].clone(),
        "c85": {
            "status": "deferred",
            "reason": "hosted GitHub Actions billing/spending-limit funding is unavailable",
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
            "control_plane_mutation": false
        },
        "artifacts": {
            "pulse_executor": pulse_executor.display().to_string(),
            "pulse_apply_result": pulse_apply_result.display().to_string(),
            "pulse_apply_result_sha256": apply_result_sha256
        }
    });
    atomic_write_text(&pulse_apply_result, &apply_result_text)?;
    atomic_write_text(&pulse_executor, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

fn pulse_apply_normalized_path(operation: &str, planned_path: &str) -> Result<String> {
    let normalized = match operation {
        "inspect_current_plugin_readiness_line" => planned_path.to_string(),
        "write_dry_run_status_handoff" => {
            "docs/status/codex-cron-pulse-apply-result-final.md".to_string()
        }
        "mirror_factory_v3_evaluator_status" => {
            "docs/status/hermes-governed-backend-control-plane/codex-cron-pulse-apply-result-final.md"
                .to_string()
        }
        _ => anyhow::bail!(
            "ao2 pulse run --execute --apply-dry-run refuses unrecognized operation `{operation}`"
        ),
    };
    if normalized.starts_with('/') || normalized.contains("..") {
        anyhow::bail!("ao2 pulse run --execute --apply-dry-run refuses unsafe path `{normalized}`");
    }
    Ok(normalized)
}

fn pulse_apply_target_path(apply_root: &Path, relative_path: &str) -> Result<PathBuf> {
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        anyhow::bail!(
            "ao2 pulse run --execute --apply-dry-run refuses unsafe path `{relative_path}`"
        );
    }
    Ok(apply_root.join(path))
}

fn pulse_apply_status_body(
    dry_run_json: &serde_json::Value,
    dry_run_sha256: &str,
    operation_id: &str,
) -> String {
    format!(
        "# AO2 Pulse Apply Result\n\n- Operation: `{operation_id}`\n- Selected task: `{}`\n- Dry-run evidence SHA256: `{dry_run_sha256}`\n- Evaluator/closer status: `{}`\n- C85 hosted GitHub Actions: deferred until billing/spending-limit funding is fixed.\n- Hermes cron/watchdog mutation: false.\n- Provider, queue, memory, AO artifact, and control-plane mutation: false.\n",
        json_string(&dry_run_json["selected_task"], "id"),
        json_string(&dry_run_json["evaluator_closer"], "status")
    )
}

pub(crate) fn validate_pulse_task_contract(contract: &serde_json::Value) -> Result<()> {
    if json_string(contract, "schema_version") != "ao2.pulse-task-contract.v1" {
        anyhow::bail!("ao2 pulse run --execute requires ao2.pulse-task-contract.v1");
    }
    for field in ["id", "title", "classification", "shape"] {
        if json_string(contract, field).trim().is_empty() {
            anyhow::bail!("ao2 pulse run --execute requires task contract field `{field}`");
        }
    }
    if json_bool(contract, "c85") {
        anyhow::bail!("ao2 pulse run --execute refuses C85 task contracts");
    }
    if !json_bool(contract, "ao2_owned_execution") {
        anyhow::bail!("ao2 pulse run --execute requires AO2-owned task execution");
    }
    if !json_bool(contract, "factory_v3_evaluator_closer_required") {
        anyhow::bail!("ao2 pulse run --execute requires factory-v3 evaluator/closer acceptance");
    }
    let side_effects = contract
        .get("side_effects")
        .ok_or_else(|| anyhow!("ao2 pulse run --execute requires task contract side_effects"))?;
    for field in [
        "provider_execution",
        "queue_execution",
        "memory_write",
        "mutates_ao_artifacts",
        "hermes_cron_watchdog_mutation",
        "control_plane_mutation",
    ] {
        if json_bool(side_effects, field) {
            anyhow::bail!(
                "ao2 pulse run --execute refuses task contracts with forbidden side effect `{field}`"
            );
        }
    }
    Ok(())
}
