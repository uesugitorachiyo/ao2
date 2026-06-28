use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use ao2_core::sha256_hex;
use chrono::{SecondsFormat, Utc};

use crate::{atomic_write_text, json_string};

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
