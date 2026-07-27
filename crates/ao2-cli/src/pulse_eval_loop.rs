use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use ao2_core::sha256_hex;
use chrono::{SecondsFormat, Utc};

use crate::cli::PulseEvalLoopCommand;
use crate::pulse_run::validate_pulse_task_contract;
use crate::{atomic_write_text, json_bool, json_string, json_u64, sha256_file};

pub(crate) fn pulse_eval_loop_run_once_json(
    executor_evidence: &Path,
    expected_executor_sha256: &str,
    verification_command: &str,
    verification_status: &str,
    packet: &Path,
    board: &Path,
    out_dir: &Path,
) -> Result<serde_json::Value> {
    let executor_text = fs::read_to_string(executor_evidence)
        .with_context(|| format!("read executor evidence {}", executor_evidence.display()))?;
    let executor_sha256 = sha256_hex(executor_text.as_bytes());
    if executor_sha256 != expected_executor_sha256 {
        anyhow::bail!("ao2 pulse eval-loop run --once executor SHA256 mismatch");
    }
    let executor_json: serde_json::Value =
        serde_json::from_str(&executor_text).context("parse ao2 pulse executor evidence")?;
    if json_string(&executor_json, "schema_version") != "ao2.pulse-executor.v1" {
        anyhow::bail!("ao2 pulse eval-loop run --once requires ao2.pulse-executor.v1 evidence");
    }

    let verification_status = verification_status.to_ascii_lowercase();
    if !matches!(
        verification_status.as_str(),
        "passed" | "failed" | "blocked"
    ) {
        anyhow::bail!(
            "ao2 pulse eval-loop run --once requires verification status passed, failed, or blocked"
        );
    }

    let packet_text =
        fs::read_to_string(packet).with_context(|| format!("read packet {}", packet.display()))?;
    let board_text =
        fs::read_to_string(board).with_context(|| format!("read board {}", board.display()))?;
    let pulse_eval_loop = out_dir.join("pulse-eval-loop.json");
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let verification_passed = verification_status == "passed";
    let selected_task = executor_json
        .get("selected_task")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let recommended_next_task = serde_json::json!({
        "id": "ao2-pulse-eval-loop-next-task",
        "title": "Advance AO2 Pulse from recommendation-only eval loop evidence",
        "classification": "COMPLEX",
        "shape": "governed_eval_loop",
        "status": if verification_passed { "recommended" } else { "blocked" },
        "requires_operator_or_follow_on": true,
        "reason": if verification_passed {
            "Existing Pulse executor evidence and local verification passed; the next loop may plan the next bounded AO2 Pulse task."
        } else {
            "Local verification did not pass, so the eval loop stops without recommending execution."
        },
        "recommended_command": "ao2 pulse eval-loop run --once --executor-evidence <pulse-executor.json> --executor-sha256 <sha256> --verification-command <cmd> --verification-status passed --packet <packet> --board <board> --out-dir <evidence-dir> --json"
    });
    let result = serde_json::json!({
        "schema_version": "ao2.pulse-eval-loop.v1",
        "status": if verification_passed { "ready_for_next_pulse_task" } else { "blocked_by_verification" },
        "mode": "recommendation_only",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "loop": {
            "bounded": true,
            "max_iterations": 1,
            "terminal": true,
            "chain_depth": 0,
            "continues_automatically": false,
            "fixed_interval_loop_successor": "ao2 pulse eval-loop run --once"
        },
        "observed_inputs": {
            "packet": packet.display().to_string(),
            "packet_sha256": sha256_hex(packet_text.as_bytes()),
            "board": board.display().to_string(),
            "board_sha256": sha256_hex(board_text.as_bytes())
        },
        "prior_executor": {
            "path": executor_evidence.display().to_string(),
            "sha256": executor_sha256,
            "schema_version": "ao2.pulse-executor.v1",
            "status": json_string(&executor_json, "status"),
            "selected_task": selected_task
        },
        "verification": {
            "command": verification_command,
            "status": verification_status,
            "required_for_recommendation": true
        },
        "evaluator": {
            "decision": if verification_passed { "recommend_next_task" } else { "block_next_task" },
            "verification_status": verification_status,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "evidence_digest_required": true,
            "reason": if verification_passed {
                "The eval loop may recommend exactly one next task because the supplied Pulse executor evidence digest matches and local verification passed."
            } else {
                "The eval loop stops because local verification did not pass."
            }
        },
        "recommended_next_task": recommended_next_task,
        "c85": executor_json
            .get("c85")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({
                "status": "unknown",
                "hosted_github_actions_checked": false,
                "rerun_allowed_without_user_billing_fix": false
            })),
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
            "pulse_eval_loop": pulse_eval_loop.display().to_string()
        }
    });
    atomic_write_text(&pulse_eval_loop, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn pulse_eval_loop_run_chain_json(
    eval_loop_evidence: &Path,
    expected_eval_loop_sha256: &str,
    verification_command: &str,
    verification_status: &str,
    packet: &Path,
    board: &Path,
    out_dir: &Path,
) -> Result<serde_json::Value> {
    let prior_text = fs::read_to_string(eval_loop_evidence)
        .with_context(|| format!("read eval-loop evidence {}", eval_loop_evidence.display()))?;
    let prior_sha256 = sha256_hex(prior_text.as_bytes());
    if prior_sha256 != expected_eval_loop_sha256 {
        anyhow::bail!("ao2 pulse eval-loop run --chain eval-loop SHA256 mismatch");
    }
    let prior_json: serde_json::Value =
        serde_json::from_str(&prior_text).context("parse ao2 pulse eval-loop evidence")?;
    if json_string(&prior_json, "schema_version") != "ao2.pulse-eval-loop.v1" {
        anyhow::bail!("ao2 pulse eval-loop run --chain requires ao2.pulse-eval-loop.v1 evidence");
    }
    if !json_bool(&prior_json["loop"], "terminal") {
        anyhow::bail!("ao2 pulse eval-loop run --chain requires terminal eval-loop evidence");
    }
    if json_bool(&prior_json["side_effects"], "repo_apply") {
        anyhow::bail!("ao2 pulse eval-loop run --chain refuses prior repo-apply evidence");
    }

    let verification_status = verification_status.to_ascii_lowercase();
    if !matches!(
        verification_status.as_str(),
        "passed" | "failed" | "blocked"
    ) {
        anyhow::bail!(
            "ao2 pulse eval-loop run --chain requires verification status passed, failed, or blocked"
        );
    }
    let verification_passed = verification_status == "passed";
    let prior_depth = prior_json["loop"]
        .get("chain_depth")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let chain_depth = prior_depth + 1;

    let packet_text =
        fs::read_to_string(packet).with_context(|| format!("read packet {}", packet.display()))?;
    let board_text =
        fs::read_to_string(board).with_context(|| format!("read board {}", board.display()))?;
    let pulse_eval_loop = out_dir.join("pulse-eval-loop.json");
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;

    let recommended_next_task = serde_json::json!({
        "id": "ao2-pulse-eval-loop-chain-next-task",
        "title": "Advance AO2 Pulse eval-loop chain toward the next bounded feature",
        "classification": "COMPLEX",
        "shape": "governed_eval_loop_chain",
        "status": if verification_passed { "recommended" } else { "blocked" },
        "requires_operator_or_follow_on": true,
        "reason": if verification_passed {
            "Prior eval-loop evidence was terminal and local verification passed; the chain may recommend exactly one next Pulse task."
        } else {
            "Local verification did not pass, so chain mode stops without recommending execution."
        },
        "recommended_command": "ao2 pulse eval-loop run --chain --eval-loop-evidence <pulse-eval-loop.json> --eval-loop-sha256 <sha256> --verification-command <cmd> --verification-status passed --packet <packet> --board <board> --out-dir <evidence-dir> --json"
    });
    let result = serde_json::json!({
        "schema_version": "ao2.pulse-eval-loop.v1",
        "status": if verification_passed { "ready_for_next_pulse_task" } else { "blocked_by_verification" },
        "mode": "recommendation_only",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "loop": {
            "bounded": true,
            "max_iterations": 1,
            "terminal": true,
            "chain_depth": chain_depth,
            "continues_automatically": false,
            "fixed_interval_loop_successor": "ao2 pulse eval-loop run --chain"
        },
        "observed_inputs": {
            "packet": packet.display().to_string(),
            "packet_sha256": sha256_hex(packet_text.as_bytes()),
            "board": board.display().to_string(),
            "board_sha256": sha256_hex(board_text.as_bytes())
        },
        "prior_eval_loop": {
            "path": eval_loop_evidence.display().to_string(),
            "sha256": prior_sha256,
            "schema_version": "ao2.pulse-eval-loop.v1",
            "status": json_string(&prior_json, "status"),
            "mode": json_string(&prior_json, "mode"),
            "terminal": json_bool(&prior_json["loop"], "terminal"),
            "chain_depth": prior_depth,
            "recommended_next_task": prior_json
                .get("recommended_next_task")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}))
        },
        "verification": {
            "command": verification_command,
            "status": verification_status,
            "required_for_recommendation": true
        },
        "evaluator": {
            "decision": if verification_passed { "recommend_next_task" } else { "block_next_task" },
            "verification_status": verification_status,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "evidence_digest_required": true,
            "reason": if verification_passed {
                "The eval-loop chain may recommend exactly one next task because prior evidence is terminal, digest verified, and local verification passed."
            } else {
                "The eval-loop chain stops because local verification did not pass."
            }
        },
        "recommended_next_task": recommended_next_task,
        "c85": prior_json
            .get("c85")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({
                "status": "unknown",
                "hosted_github_actions_checked": false,
                "rerun_allowed_without_user_billing_fix": false
            })),
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
            "pulse_eval_loop": pulse_eval_loop.display().to_string()
        }
    });
    atomic_write_text(&pulse_eval_loop, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn pulse_eval_loop_handoff_json(
    eval_loop_evidence: &Path,
    expected_eval_loop_sha256: &str,
    packet: &Path,
    board: &Path,
    out_dir: &Path,
) -> Result<serde_json::Value> {
    let eval_loop_text = fs::read_to_string(eval_loop_evidence)
        .with_context(|| format!("read eval-loop evidence {}", eval_loop_evidence.display()))?;
    let eval_loop_sha256 = sha256_hex(eval_loop_text.as_bytes());
    if eval_loop_sha256 != expected_eval_loop_sha256 {
        anyhow::bail!("ao2 pulse eval-loop handoff eval-loop SHA256 mismatch");
    }
    let eval_loop_json: serde_json::Value =
        serde_json::from_str(&eval_loop_text).context("parse ao2 pulse eval-loop evidence")?;
    if json_string(&eval_loop_json, "schema_version") != "ao2.pulse-eval-loop.v1" {
        anyhow::bail!("ao2 pulse eval-loop handoff requires ao2.pulse-eval-loop.v1 evidence");
    }
    if json_string(&eval_loop_json, "status") != "ready_for_next_pulse_task"
        || json_string(&eval_loop_json, "mode") != "recommendation_only"
        || !json_bool(&eval_loop_json["loop"], "terminal")
        || json_bool(&eval_loop_json["loop"], "continues_automatically")
    {
        anyhow::bail!("ao2 pulse eval-loop handoff requires terminal ready eval-loop evidence");
    }
    if json_bool(&eval_loop_json["side_effects"], "repo_apply") {
        anyhow::bail!("ao2 pulse eval-loop handoff refuses repo-apply evidence");
    }

    let recommended = eval_loop_json
        .get("recommended_next_task")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let task_id = json_string(&recommended, "id");
    if task_id.trim().is_empty() {
        anyhow::bail!("ao2 pulse eval-loop handoff requires a recommended task id");
    }
    let task_title = {
        let title = json_string(&recommended, "title");
        if title.trim().is_empty() {
            task_id.clone()
        } else {
            title
        }
    };
    let task_classification = {
        let classification = json_string(&recommended, "classification");
        if classification.trim().is_empty() {
            "COMPLEX".to_string()
        } else {
            classification
        }
    };
    let task_shape = {
        let shape = json_string(&recommended, "shape");
        if shape.trim().is_empty() {
            "governed_eval_loop_chain".to_string()
        } else {
            shape
        }
    };

    let packet_text =
        fs::read_to_string(packet).with_context(|| format!("read packet {}", packet.display()))?;
    let board_text =
        fs::read_to_string(board).with_context(|| format!("read board {}", board.display()))?;
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let task_contract_path = out_dir.join("pulse-task-contract.json");
    let handoff_path = out_dir.join("pulse-task-contract-handoff.json");

    let side_effects = serde_json::json!({
        "provider_execution": false,
        "queue_execution": false,
        "memory_write": false,
        "mutates_ao_artifacts": false,
        "hermes_cron_watchdog_mutation": false,
        "control_plane_mutation": false,
        "repo_apply": false
    });
    let task_contract = serde_json::json!({
        "schema_version": "ao2.pulse-task-contract.v1",
        "id": task_id,
        "title": task_title,
        "classification": task_classification,
        "shape": task_shape,
        "c85": false,
        "ao2_owned_execution": true,
        "factory_v3_evaluator_closer_required": true,
        "evaluator_acceptance": "accept_non_c85_governed_task",
        "closer_acceptance": "accepted",
        "source_eval_loop": {
            "path": eval_loop_evidence.display().to_string(),
            "sha256": eval_loop_sha256,
            "schema_version": "ao2.pulse-eval-loop.v1",
            "status": json_string(&eval_loop_json, "status"),
            "chain_depth": json_u64(&eval_loop_json["loop"], "chain_depth")
        },
        "side_effects": side_effects
    });
    validate_pulse_task_contract(&task_contract)?;
    atomic_write_text(
        &task_contract_path,
        &serde_json::to_string_pretty(&task_contract)?,
    )?;
    let task_contract_sha256 = sha256_file(&task_contract_path)?;

    let result = serde_json::json!({
        "schema_version": "ao2.pulse-task-contract-handoff.v1",
        "status": "task_contract_ready",
        "mode": "contract_only",
        "generated_at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "prior_eval_loop": {
            "path": eval_loop_evidence.display().to_string(),
            "sha256": eval_loop_sha256,
            "schema_version": "ao2.pulse-eval-loop.v1",
            "status": json_string(&eval_loop_json, "status"),
            "chain_depth": json_u64(&eval_loop_json["loop"], "chain_depth")
        },
        "observed_inputs": {
            "packet": packet.display().to_string(),
            "packet_sha256": sha256_hex(packet_text.as_bytes()),
            "board": board.display().to_string(),
            "board_sha256": sha256_hex(board_text.as_bytes())
        },
        "selected_task": task_contract
            .as_object()
            .map(|contract| serde_json::json!({
                "id": contract.get("id").cloned().unwrap_or_else(|| serde_json::json!("")),
                "title": contract.get("title").cloned().unwrap_or_else(|| serde_json::json!("")),
                "classification": contract.get("classification").cloned().unwrap_or_else(|| serde_json::json!("")),
                "shape": contract.get("shape").cloned().unwrap_or_else(|| serde_json::json!("")),
                "c85": false
            }))
            .unwrap_or_else(|| serde_json::json!({})),
        "evaluator_closer": {
            "status": "contract_ready",
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "evidence_digest_required": true,
            "executes_task": false,
            "applies_repo_changes": false
        },
        "side_effects": side_effects,
        "artifacts": {
            "task_contract": task_contract_path.display().to_string(),
            "task_contract_sha256": task_contract_sha256,
            "handoff": handoff_path.display().to_string()
        }
    });
    atomic_write_text(&handoff_path, &serde_json::to_string_pretty(&result)?)?;
    Ok(result)
}

pub(crate) fn pulse_eval_loop(command: PulseEvalLoopCommand) -> Result<()> {
    match command {
        PulseEvalLoopCommand::Run {
            once,
            chain,
            executor_evidence,
            executor_sha256,
            eval_loop_evidence,
            eval_loop_sha256,
            verification_command,
            verification_status,
            packet,
            board,
            out_dir,
            json,
        } => {
            if [once, chain].into_iter().filter(|enabled| *enabled).count() != 1 {
                anyhow::bail!("ao2 pulse eval-loop run requires exactly one of --once or --chain");
            }
            let result = if once {
                let executor_evidence = executor_evidence.as_deref().ok_or_else(|| {
                    anyhow!("ao2 pulse eval-loop run --once requires --executor-evidence")
                })?;
                let executor_sha256 = executor_sha256.as_deref().ok_or_else(|| {
                    anyhow!("ao2 pulse eval-loop run --once requires --executor-sha256")
                })?;
                if eval_loop_evidence.is_some() || eval_loop_sha256.is_some() {
                    anyhow::bail!("--eval-loop-evidence is only valid with --chain");
                }
                pulse_eval_loop_run_once_json(
                    executor_evidence,
                    executor_sha256,
                    &verification_command,
                    &verification_status,
                    &packet,
                    &board,
                    &out_dir,
                )?
            } else {
                let eval_loop_evidence = eval_loop_evidence.as_deref().ok_or_else(|| {
                    anyhow!("ao2 pulse eval-loop run --chain requires --eval-loop-evidence")
                })?;
                let eval_loop_sha256 = eval_loop_sha256.as_deref().ok_or_else(|| {
                    anyhow!("ao2 pulse eval-loop run --chain requires --eval-loop-sha256")
                })?;
                if executor_evidence.is_some() || executor_sha256.is_some() {
                    anyhow::bail!("--executor-evidence is only valid with --once");
                }
                pulse_eval_loop_run_chain_json(
                    eval_loop_evidence,
                    eval_loop_sha256,
                    &verification_command,
                    &verification_status,
                    &packet,
                    &board,
                    &out_dir,
                )?
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "recommended_next_task={}",
                    json_string(&result["recommended_next_task"], "id")
                );
                println!(
                    "artifact={}",
                    json_string(&result["artifacts"], "pulse_eval_loop")
                );
            }
            Ok(())
        }
        PulseEvalLoopCommand::Handoff {
            eval_loop_evidence,
            eval_loop_sha256,
            packet,
            board,
            out_dir,
            json,
        } => {
            let result = pulse_eval_loop_handoff_json(
                &eval_loop_evidence,
                &eval_loop_sha256,
                &packet,
                &board,
                &out_dir,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("status={}", json_string(&result, "status"));
                println!(
                    "task_contract={}",
                    json_string(&result["artifacts"], "task_contract")
                );
                println!(
                    "task_contract_sha256={}",
                    json_string(&result["artifacts"], "task_contract_sha256")
                );
            }
            Ok(())
        }
    }
}
