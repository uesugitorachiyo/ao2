use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write as IoWrite};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::cli_util::{create_tar_gz, escape_html, json_array, json_string, json_u64, sha256_file};
use crate::provider_ops::read_provider_smoke_history;
use crate::release_crypto::{
    copy_dir_recursive, derive_public_key_from_private_key, extract_tar_gz,
    sign_file_with_private_key, verify_file_signature,
};
use crate::release_provenance::ensure_rsa_private_key;
use crate::workbench_support::empty_workbench_redaction_audit;
use crate::{
    atomic_write_text, generate_api_token, http_html_response, http_json_response,
    http_text_response, now_unix_ms, open_report_target, parse_http_request_line,
    query_value_owned, read_workbench_audit_events, read_workbench_queue_file, runs_list_json,
    runtime_git_commit, runtime_target_label, split_path_query, workbench_audit_path_for_target,
    ControlPlaneCommand, ControlPlaneHistoryCommand, ControlPlaneSourcesCommand,
    WorkbenchSupportSigning,
};

pub(crate) fn workbench_support_keygen(out: PathBuf, bits: usize, json: bool) -> Result<()> {
    ensure_rsa_private_key(&out, bits)?;
    let public_key = out.with_extension("public.pem");
    derive_public_key_from_private_key(&out, &public_key)?;
    let report = serde_json::json!({
        "schema": "ao2.workbench-support-keygen.v1",
        "private_key": out,
        "public_key": public_key,
        "bits": bits,
        "status": "passed"
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "support_private_key={}",
            json_string(&report, "private_key")
        );
        println!("support_public_key={}", json_string(&report, "public_key"));
        println!("workbench_support_keygen=passed");
    }
    Ok(())
}

pub(crate) fn control_plane(command: ControlPlaneCommand) -> Result<()> {
    match command {
        ControlPlaneCommand::Ingest { target, out, json } => {
            control_plane_ingest(target, out, json)
        }
        ControlPlaneCommand::Export {
            target,
            snapshot,
            fleet,
            health_history,
            out,
            open,
        } => control_plane_export(target, snapshot, fleet, health_history, out, open),
        ControlPlaneCommand::Serve {
            target,
            snapshot,
            fleet,
            health_history,
            host,
            port,
            once,
            api_token,
        } => control_plane_serve(ControlPlaneServeOptions {
            target,
            snapshot,
            fleet,
            health_history,
            host,
            port,
            once,
            api_token,
        }),
        ControlPlaneCommand::Index {
            targets,
            snapshots,
            out,
            json,
        } => control_plane_index(targets, snapshots, out, json),
        ControlPlaneCommand::Refresh {
            targets,
            sources,
            history,
            out,
            json,
        } => control_plane_refresh(targets, sources, history, out, json),
        ControlPlaneCommand::Health {
            fleet,
            history,
            record,
            json,
        } => control_plane_health(fleet, history, record, json),
        ControlPlaneCommand::HealthTrend { history, json } => {
            control_plane_health_trend(history, json)
        }
        ControlPlaneCommand::HealthExport {
            history,
            out,
            open,
            json,
        } => control_plane_health_export(history, out, open, json),
        ControlPlaneCommand::HealthPrune {
            history,
            keep,
            json,
        } => control_plane_health_prune(history, keep, json),
        ControlPlaneCommand::Sources { command } => match command {
            ControlPlaneSourcesCommand::Save { targets, out, json } => {
                control_plane_sources_save(targets, out, json)
            }
        },
        ControlPlaneCommand::History { command } => match command {
            ControlPlaneHistoryCommand::Diff {
                history,
                from_index,
                to_index,
                json,
            } => control_plane_history_diff(history, from_index, to_index, json),
            ControlPlaneHistoryCommand::Prune {
                history,
                keep,
                json,
            } => control_plane_history_prune(history, keep, json),
            ControlPlaneHistoryCommand::Export {
                history,
                out,
                open,
                json,
            } => control_plane_history_export(history, out, open, json),
        },
        ControlPlaneCommand::Bundle {
            fleet,
            health_history,
            out_dir,
            signing_key,
            signer_id,
            json,
        } => control_plane_bundle(fleet, health_history, out_dir, signing_key, signer_id, json),
        ControlPlaneCommand::BundleVerify { bundle_dir, json } => {
            control_plane_bundle_verify(bundle_dir, json)
        }
        ControlPlaneCommand::BundleImport {
            archive,
            bundle_dir,
            out_dir,
            json,
        } => control_plane_bundle_import(archive, bundle_dir, out_dir, json),
        ControlPlaneCommand::BundleInspect {
            archive,
            bundle_dir,
            json,
        } => control_plane_bundle_inspect(archive, bundle_dir, json),
    }
}

fn control_plane_ingest(target: PathBuf, out: Option<PathBuf>, json: bool) -> Result<()> {
    let snapshot_path = out.unwrap_or_else(|| control_plane_snapshot_path(&target));
    let snapshot = control_plane_snapshot_json(&target, &snapshot_path)?;
    if let Some(parent) = snapshot_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    atomic_write_text(&snapshot_path, &serde_json::to_string_pretty(&snapshot)?)?;
    let result = serde_json::json!({
        "schema_version": "ao2.control-plane-ingest.v1",
        "target": target,
        "snapshot_path": snapshot_path,
        "run_count": json_array(&snapshot["runs"], "runs").len(),
        "queue_job_count": json_array(&snapshot["queue"], "jobs").len(),
        "audit_event_count": snapshot["audit_events"].as_array().map(Vec::len).unwrap_or_default()
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "snapshot={}",
            result["snapshot_path"].as_str().unwrap_or("")
        );
        println!("runs={}", result["run_count"].as_u64().unwrap_or_default());
        println!(
            "queue_jobs={}",
            result["queue_job_count"].as_u64().unwrap_or_default()
        );
        println!(
            "audit_events={}",
            result["audit_event_count"].as_u64().unwrap_or_default()
        );
    }
    Ok(())
}

fn control_plane_index(
    targets: Vec<PathBuf>,
    snapshots: Vec<PathBuf>,
    out: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    if targets.is_empty() && snapshots.is_empty() {
        return Err(anyhow!(
            "at least one --target or --snapshot is required for control-plane index"
        ));
    }
    let mut repositories = Vec::new();
    for target in targets {
        let snapshot_path = control_plane_snapshot_path(&target);
        let snapshot = read_control_plane_snapshot(&snapshot_path)?;
        repositories.push(control_plane_repository_index_json(
            Some(target),
            snapshot_path,
            snapshot,
        ));
    }
    for snapshot_path in snapshots {
        let snapshot = read_control_plane_snapshot(&snapshot_path)?;
        repositories.push(control_plane_repository_index_json(
            None,
            snapshot_path,
            snapshot,
        ));
    }
    let totals = control_plane_fleet_totals(&repositories);
    let fleet = serde_json::json!({
        "schema_version": "ao2.control-plane-fleet-snapshot.v1",
        "generated_at_ms": now_unix_ms(),
        "repositories": repositories,
        "totals": totals
    });
    let output_path = out.unwrap_or_else(|| {
        PathBuf::from(".")
            .join(".ao2")
            .join("control-plane")
            .join("fleet-snapshot.json")
    });
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    atomic_write_text(&output_path, &serde_json::to_string_pretty(&fleet)?)?;
    let result = serde_json::json!({
        "schema_version": "ao2.control-plane-index.v1",
        "fleet_path": output_path,
        "repository_count": fleet["totals"]["repository_count"],
        "run_count": fleet["totals"]["run_count"],
        "queue_job_count": fleet["totals"]["queue_job_count"],
        "audit_event_count": fleet["totals"]["audit_event_count"],
        "evidence_pack_count": fleet["totals"]["evidence_pack_count"]
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("fleet={}", result["fleet_path"].as_str().unwrap_or(""));
        println!(
            "repositories={}",
            result["repository_count"].as_u64().unwrap_or_default()
        );
        println!("runs={}", result["run_count"].as_u64().unwrap_or_default());
    }
    Ok(())
}

fn control_plane_refresh(
    mut targets: Vec<PathBuf>,
    sources: Option<PathBuf>,
    history: Option<PathBuf>,
    out: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    if let Some(source_path) = sources {
        targets.extend(read_control_plane_sources(&source_path)?);
    }
    if targets.is_empty() {
        return Err(anyhow!(
            "at least one --target is required for control-plane refresh"
        ));
    }
    let mut repositories = Vec::new();
    for target in targets {
        let snapshot_path = control_plane_snapshot_path(&target);
        let snapshot = control_plane_snapshot_json(&target, &snapshot_path)?;
        if let Some(parent) = snapshot_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        atomic_write_text(&snapshot_path, &serde_json::to_string_pretty(&snapshot)?)?;
        repositories.push(control_plane_repository_index_json(
            Some(target),
            snapshot_path,
            snapshot,
        ));
    }
    let totals = control_plane_fleet_totals(&repositories);
    let fleet = serde_json::json!({
        "schema_version": "ao2.control-plane-fleet-snapshot.v1",
        "generated_at_ms": now_unix_ms(),
        "repositories": repositories,
        "totals": totals
    });
    let output_path = out.unwrap_or_else(|| {
        PathBuf::from(".")
            .join(".ao2")
            .join("control-plane")
            .join("fleet-snapshot.json")
    });
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    atomic_write_text(&output_path, &serde_json::to_string_pretty(&fleet)?)?;
    let mut result = serde_json::json!({
        "schema_version": "ao2.control-plane-refresh.v1",
        "fleet_path": output_path,
        "refreshed_repository_count": fleet["totals"]["repository_count"],
        "run_count": fleet["totals"]["run_count"],
        "queue_job_count": fleet["totals"]["queue_job_count"],
        "audit_event_count": fleet["totals"]["audit_event_count"],
        "evidence_pack_count": fleet["totals"]["evidence_pack_count"]
    });
    if let Some(history_dir) = history {
        let history_result = record_control_plane_history(&history_dir, &output_path, &fleet)?;
        result["history_path"] = history_result["history_path"].clone();
        result["history_entry_path"] = history_result["history_entry_path"].clone();
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("fleet={}", result["fleet_path"].as_str().unwrap_or(""));
        println!(
            "refreshed_repositories={}",
            result["refreshed_repository_count"]
                .as_u64()
                .unwrap_or_default()
        );
        println!("runs={}", result["run_count"].as_u64().unwrap_or_default());
    }
    Ok(())
}

fn control_plane_health(
    fleet: PathBuf,
    history: Option<PathBuf>,
    record: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let fleet_snapshot = read_control_plane_snapshot(&fleet)?;
    let mut result = control_plane_health_json(&fleet, &fleet_snapshot, history.as_deref())?;
    if let Some(record_dir) = record {
        let record_result = record_control_plane_health_history(&record_dir, &result)?;
        result["health_history_path"] = record_result["health_history_path"].clone();
        result["health_entry_path"] = record_result["health_entry_path"].clone();
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("health={}", json_string(&result, "status"));
        println!(
            "alerts={}",
            result["alert_count"].as_u64().unwrap_or_default()
        );
        if !result["health_history_path"].is_null() {
            println!(
                "health_history={}",
                result["health_history_path"].as_str().unwrap_or("")
            );
        }
    }
    Ok(())
}

pub(crate) fn control_plane_health_json(
    fleet_path: &Path,
    fleet: &serde_json::Value,
    history_dir: Option<&Path>,
) -> Result<serde_json::Value> {
    if json_string(fleet, "schema_version") != "ao2.control-plane-fleet-snapshot.v1" {
        return Err(anyhow!(
            "control-plane health requires an ao2.control-plane-fleet-snapshot.v1 file"
        ));
    }
    let mut alerts = Vec::new();
    let repositories = json_array(fleet, "repositories");
    if repositories.is_empty() {
        alerts.push(control_plane_health_alert(
            "warning",
            "empty_fleet",
            "",
            "",
            "",
            "Fleet snapshot has no repositories",
        ));
    }
    for repo in repositories {
        let repository = json_string(repo, "target");
        let runs = json_array(&repo["snapshot"]["runs"], "runs");
        if runs.is_empty() {
            alerts.push(control_plane_health_alert(
                "warning",
                "repo_has_no_runs",
                &repository,
                "",
                "",
                &format!("Repository {repository} has no runs"),
            ));
        }
        for run in runs {
            let run_id = json_string(run, "run_id");
            let status = json_string(run, "status").to_lowercase();
            if control_plane_status_is_unhealthy(&status) {
                alerts.push(control_plane_health_alert(
                    "warning",
                    "run_not_accepted",
                    &repository,
                    &run_id,
                    "",
                    &format!("Run {run_id} status is {status}"),
                ));
            }
            let digest_failures = run
                .get("digest_failures")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            if digest_failures > 0 {
                alerts.push(control_plane_health_alert(
                    "warning",
                    "run_digest_failures",
                    &repository,
                    &run_id,
                    "",
                    &format!("Run {run_id} has {digest_failures} digest failure(s)"),
                ));
            }
            let evidence_pack = json_string(run, "evidence_pack");
            if evidence_pack.is_empty() || !Path::new(&evidence_pack).is_file() {
                alerts.push(control_plane_health_alert(
                    "warning",
                    "missing_evidence_pack",
                    &repository,
                    &run_id,
                    "",
                    &format!("Run {run_id} evidence pack is missing"),
                ));
            }
        }
        for job in json_array(&repo["snapshot"]["queue"], "jobs") {
            let job_id = json_string(job, "job_id");
            let run_id = json_string(job, "run_id");
            let status = json_string(job, "status").to_lowercase();
            if control_plane_status_is_unhealthy(&status) {
                alerts.push(control_plane_health_alert(
                    "warning",
                    "queue_job_not_accepted",
                    &repository,
                    &run_id,
                    &job_id,
                    &format!("Queue job {job_id} status is {status}"),
                ));
            }
        }
    }
    let provider_readiness = control_plane_provider_readiness_rollup(repositories, &mut alerts);
    let mut history_path = serde_json::Value::Null;
    let mut history_entry_count = serde_json::Value::Null;
    if let Some(history_dir) = history_dir {
        let history = read_control_plane_history(history_dir)?;
        let entries = json_array(&history, "entries");
        history_path = serde_json::json!(history_dir.join("history.json"));
        history_entry_count = serde_json::json!(entries.len());
        if entries.is_empty() {
            alerts.push(control_plane_health_alert(
                "warning",
                "empty_history",
                "",
                "",
                "",
                "Fleet history has no retained entries",
            ));
        }
    }
    let status = if alerts.is_empty() { "ok" } else { "warn" };
    Ok(serde_json::json!({
        "schema_version": "ao2.control-plane-health.v1",
        "status": status,
        "fleet_path": fleet_path,
        "history_path": history_path,
        "history_entry_count": history_entry_count,
        "repository_count": json_u64(&fleet["totals"], "repository_count"),
        "run_count": json_u64(&fleet["totals"], "run_count"),
        "queue_job_count": json_u64(&fleet["totals"], "queue_job_count"),
        "audit_event_count": json_u64(&fleet["totals"], "audit_event_count"),
        "evidence_pack_count": json_u64(&fleet["totals"], "evidence_pack_count"),
        "provider_readiness": provider_readiness,
        "alert_count": alerts.len(),
        "alerts": alerts
    }))
}

fn control_plane_provider_readiness_rollup(
    repositories: &[serde_json::Value],
    alerts: &mut Vec<serde_json::Value>,
) -> serde_json::Value {
    let mut provider_counts: HashMap<String, (u64, u64, u64, u64)> = HashMap::new();
    let mut missing_history_count = 0_u64;
    let mut ready_repository_count = 0_u64;
    let mut not_ready_repository_count = 0_u64;

    for repo in repositories {
        let repository = json_string(repo, "target");
        let history = &repo["snapshot"]["provider_smoke_history"];
        let latest = &history["latest"];
        if latest.is_null() || json_array(history, "entries").is_empty() {
            missing_history_count += 1;
            not_ready_repository_count += 1;
            alerts.push(control_plane_health_alert(
                "warning",
                "provider_smoke_missing",
                &repository,
                "",
                "",
                &format!("Repository {repository} has no provider smoke history"),
            ));
            continue;
        }

        let mut scripted_ready = false;
        for provider in json_array(latest, "providers") {
            let name = json_string(provider, "provider");
            let verdict = json_string(provider, "verdict");
            let entry = provider_counts.entry(name.clone()).or_insert((0, 0, 0, 0));
            match verdict.as_str() {
                "ready" => entry.0 += 1,
                "unavailable" => entry.3 += 1,
                "warn" | "not_run" => entry.1 += 1,
                _ => entry.2 += 1,
            }
            if name == "scripted" && verdict == "ready" {
                scripted_ready = true;
            }
        }

        if scripted_ready {
            ready_repository_count += 1;
        } else {
            not_ready_repository_count += 1;
            alerts.push(control_plane_health_alert(
                "warning",
                "provider_smoke_not_ready",
                &repository,
                "",
                "",
                &format!("Repository {repository} scripted provider smoke is not ready"),
            ));
        }
    }

    let mut providers = serde_json::Map::new();
    for (provider, (ready, warn, fail, unavailable)) in provider_counts {
        providers.insert(
            provider,
            serde_json::json!({
                "ready_count": ready,
                "warn_count": warn,
                "fail_count": fail,
                "unavailable_count": unavailable
            }),
        );
    }

    serde_json::json!({
        "schema": "ao2.provider-readiness-rollup.v1",
        "repository_count": repositories.len(),
        "missing_history_count": missing_history_count,
        "ready_repository_count": ready_repository_count,
        "not_ready_repository_count": not_ready_repository_count,
        "providers": providers
    })
}

fn control_plane_health_alert(
    severity: &str,
    code: &str,
    repository: &str,
    run_id: &str,
    job_id: &str,
    message: &str,
) -> serde_json::Value {
    serde_json::json!({
        "severity": severity,
        "code": code,
        "repository": repository,
        "run_id": run_id,
        "job_id": job_id,
        "message": message
    })
}

fn control_plane_status_is_unhealthy(status: &str) -> bool {
    matches!(
        status,
        "rejected" | "failed" | "cancelled" | "canceled" | "interrupted"
    )
}

fn record_control_plane_health_history(
    history_dir: &Path,
    health: &serde_json::Value,
) -> Result<serde_json::Value> {
    fs::create_dir_all(history_dir).with_context(|| format!("create {}", history_dir.display()))?;
    let generated_at_ms = now_unix_ms();
    let history_path = history_dir.join("health-history.json");
    let mut history = if history_path.is_file() {
        read_control_plane_health_history(history_dir)?
    } else {
        serde_json::json!({
            "schema_version": "ao2.control-plane-health-history.v1",
            "generated_at_ms": generated_at_ms,
            "entries": []
        })
    };
    let entry_path = unique_control_plane_health_entry_path(history_dir, generated_at_ms);
    atomic_write_text(&entry_path, &serde_json::to_string_pretty(health)?)?;
    let entry_sha = sha256_file(&entry_path)?;
    let entry = serde_json::json!({
        "generated_at_ms": generated_at_ms,
        "health_path": entry_path,
        "health_sha256": entry_sha,
        "status": json_string(health, "status"),
        "alert_count": json_u64(health, "alert_count"),
        "repository_count": json_u64(health, "repository_count"),
        "run_count": json_u64(health, "run_count"),
        "queue_job_count": json_u64(health, "queue_job_count")
    });
    let entries = history
        .get_mut("entries")
        .and_then(serde_json::Value::as_array_mut)
        .context("control-plane health history missing entries array")?;
    entries.push(entry.clone());
    history["generated_at_ms"] = serde_json::json!(generated_at_ms);
    atomic_write_text(&history_path, &serde_json::to_string_pretty(&history)?)?;
    Ok(serde_json::json!({
        "health_history_path": history_path,
        "health_entry_path": entry_path,
        "entry": entry
    }))
}

fn unique_control_plane_health_entry_path(history_dir: &Path, generated_at_ms: u64) -> PathBuf {
    let mut suffix = 0_u32;
    loop {
        let filename = if suffix == 0 {
            format!("{generated_at_ms}-health.json")
        } else {
            format!("{generated_at_ms}-{suffix}-health.json")
        };
        let candidate = history_dir.join(filename);
        if !candidate.exists() {
            return candidate;
        }
        suffix += 1;
    }
}

fn read_control_plane_health_history(history_dir: &Path) -> Result<serde_json::Value> {
    let history_path = history_dir.join("health-history.json");
    let history = read_control_plane_snapshot(&history_path)?;
    if json_string(&history, "schema_version") != "ao2.control-plane-health-history.v1" {
        return Err(anyhow!(
            "control-plane health history file must use schema ao2.control-plane-health-history.v1"
        ));
    }
    if history
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .is_none()
    {
        return Err(anyhow!(
            "control-plane health history missing entries array"
        ));
    }
    Ok(history)
}

fn control_plane_health_trend(history_dir: PathBuf, json: bool) -> Result<()> {
    let result = control_plane_health_trend_json(&history_dir)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "health_history={}",
            result["history_path"].as_str().unwrap_or("")
        );
        println!(
            "entries={}",
            result["entry_count"].as_u64().unwrap_or_default()
        );
        println!("trend={}", json_string(&result, "trend"));
        println!(
            "alert_delta={}",
            result["alert_count_delta"].as_i64().unwrap_or_default()
        );
    }
    Ok(())
}

fn control_plane_health_trend_json(history_dir: &Path) -> Result<serde_json::Value> {
    let history = read_control_plane_health_history(history_dir)?;
    let entries = json_array(&history, "entries");
    let history_path = history_dir.join("health-history.json");
    if entries.is_empty() {
        return Ok(serde_json::json!({
            "schema_version": "ao2.control-plane-health-trend.v1",
            "history_path": history_path,
            "entry_count": 0,
            "latest_status": "",
            "latest_alert_count": 0,
            "previous_alert_count": 0,
            "alert_count_delta": 0,
            "trend": "empty"
        }));
    }
    let latest = entries
        .last()
        .context("control-plane health history missing latest entry")?;
    let latest_alert_count = json_u64(latest, "alert_count");
    let (previous_alert_count, delta, trend) = if entries.len() < 2 {
        (0, latest_alert_count as i64, "insufficient_data")
    } else {
        let previous = &entries[entries.len() - 2];
        let previous_alert_count = json_u64(previous, "alert_count");
        let delta = latest_alert_count as i64 - previous_alert_count as i64;
        let trend = if delta < 0 {
            "improving"
        } else if delta > 0 {
            "worsening"
        } else {
            "stable"
        };
        (previous_alert_count, delta, trend)
    };
    Ok(serde_json::json!({
        "schema_version": "ao2.control-plane-health-trend.v1",
        "history_path": history_path,
        "entry_count": entries.len(),
        "latest_status": json_string(latest, "status"),
        "latest_alert_count": latest_alert_count,
        "previous_alert_count": previous_alert_count,
        "alert_count_delta": delta,
        "trend": trend,
        "latest_health_path": json_string(latest, "health_path")
    }))
}

fn control_plane_health_export(
    history_dir: PathBuf,
    out: Option<PathBuf>,
    open: bool,
    json: bool,
) -> Result<()> {
    let history = read_control_plane_health_history(&history_dir)?;
    let trend = control_plane_health_trend_json(&history_dir)?;
    let html = render_control_plane_health_trend_dashboard(&history, &trend)?;
    let path = out.unwrap_or_else(|| history_dir.join("health-index.html"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    atomic_write_text(&path, &html)?;
    let result = serde_json::json!({
        "schema_version": "ao2.control-plane-health-export.v1",
        "history_path": history_dir.join("health-history.json"),
        "health_dashboard_path": path,
        "entry_count": json_array(&history, "entries").len(),
        "trend": json_string(&trend, "trend")
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "health_dashboard={}",
            result["health_dashboard_path"].as_str().unwrap_or("")
        );
    }
    if open {
        let path = PathBuf::from(
            result["health_dashboard_path"]
                .as_str()
                .context("health dashboard path is a string")?,
        );
        open_report_target(&path)?;
        println!("open_target={}", path.display());
    }
    Ok(())
}

fn control_plane_health_prune(history_dir: PathBuf, keep: usize, json: bool) -> Result<()> {
    if keep == 0 {
        return Err(anyhow!(
            "control-plane health-prune --keep must be greater than 0"
        ));
    }
    let mut history = read_control_plane_health_history(&history_dir)?;
    let entries = json_array(&history, "entries");
    let mut indexed_entries = entries
        .iter()
        .cloned()
        .enumerate()
        .collect::<Vec<(usize, serde_json::Value)>>();
    indexed_entries.sort_by_key(|(index, entry)| (json_u64(entry, "generated_at_ms"), *index));
    let remove_count = indexed_entries.len().saturating_sub(keep);
    let removed = indexed_entries
        .iter()
        .take(remove_count)
        .map(|(_, entry)| entry.clone())
        .collect::<Vec<_>>();
    let mut kept = indexed_entries
        .into_iter()
        .skip(remove_count)
        .collect::<Vec<(usize, serde_json::Value)>>();
    kept.sort_by_key(|(index, _)| *index);
    let kept_entries = kept.into_iter().map(|(_, entry)| entry).collect::<Vec<_>>();
    let mut removed_paths = Vec::new();
    for entry in &removed {
        let path_text = json_string(entry, "health_path");
        if path_text.is_empty() {
            continue;
        }
        let path = PathBuf::from(&path_text);
        if path.exists() {
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        }
        removed_paths.push(path_text);
    }
    history["entries"] = serde_json::Value::Array(kept_entries);
    history["generated_at_ms"] = serde_json::json!(now_unix_ms());
    let history_path = history_dir.join("health-history.json");
    atomic_write_text(&history_path, &serde_json::to_string_pretty(&history)?)?;
    let result = serde_json::json!({
        "schema_version": "ao2.control-plane-health-prune.v1",
        "history_path": history_path,
        "kept_count": json_array(&history, "entries").len(),
        "removed_count": removed.len(),
        "removed_paths": removed_paths
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "health_history={}",
            result["history_path"].as_str().unwrap_or("")
        );
        println!("kept={}", result["kept_count"].as_u64().unwrap_or_default());
        println!(
            "removed={}",
            result["removed_count"].as_u64().unwrap_or_default()
        );
    }
    Ok(())
}

fn record_control_plane_history(
    history_dir: &Path,
    fleet_path: &Path,
    fleet: &serde_json::Value,
) -> Result<serde_json::Value> {
    fs::create_dir_all(history_dir).with_context(|| format!("create {}", history_dir.display()))?;
    let generated_at_ms = now_unix_ms();
    let history_path = history_dir.join("history.json");
    let mut history = if history_path.is_file() {
        read_control_plane_snapshot(&history_path)?
    } else {
        serde_json::json!({
            "schema_version": "ao2.control-plane-fleet-history.v1",
            "generated_at_ms": generated_at_ms,
            "entries": []
        })
    };
    if json_string(&history, "schema_version") != "ao2.control-plane-fleet-history.v1" {
        return Err(anyhow!(
            "control-plane history file must use schema ao2.control-plane-fleet-history.v1"
        ));
    }
    let entry_path = unique_control_plane_history_entry_path(history_dir, generated_at_ms);
    fs::copy(fleet_path, &entry_path)
        .with_context(|| format!("copy {} to {}", fleet_path.display(), entry_path.display()))?;
    let entry_sha = sha256_file(&entry_path)?;
    let entry = serde_json::json!({
        "generated_at_ms": generated_at_ms,
        "fleet_snapshot_path": entry_path,
        "fleet_snapshot_sha256": entry_sha,
        "repository_count": json_u64(&fleet["totals"], "repository_count"),
        "run_count": json_u64(&fleet["totals"], "run_count"),
        "queue_job_count": json_u64(&fleet["totals"], "queue_job_count"),
        "audit_event_count": json_u64(&fleet["totals"], "audit_event_count"),
        "evidence_pack_count": json_u64(&fleet["totals"], "evidence_pack_count")
    });
    let entries = history
        .get_mut("entries")
        .and_then(serde_json::Value::as_array_mut)
        .context("control-plane history missing entries array")?;
    entries.push(entry.clone());
    history["generated_at_ms"] = serde_json::json!(generated_at_ms);
    atomic_write_text(&history_path, &serde_json::to_string_pretty(&history)?)?;
    Ok(serde_json::json!({
        "history_path": history_path,
        "history_entry_path": entry_path,
        "entry": entry
    }))
}

fn unique_control_plane_history_entry_path(history_dir: &Path, generated_at_ms: u64) -> PathBuf {
    let mut suffix = 0_u32;
    loop {
        let filename = if suffix == 0 {
            format!("{generated_at_ms}-fleet-snapshot.json")
        } else {
            format!("{generated_at_ms}-{suffix}-fleet-snapshot.json")
        };
        let candidate = history_dir.join(filename);
        if !candidate.exists() {
            return candidate;
        }
        suffix += 1;
    }
}

fn read_control_plane_history(history_dir: &Path) -> Result<serde_json::Value> {
    let history_path = history_dir.join("history.json");
    let history = read_control_plane_snapshot(&history_path)?;
    if json_string(&history, "schema_version") != "ao2.control-plane-fleet-history.v1" {
        return Err(anyhow!(
            "control-plane history file must use schema ao2.control-plane-fleet-history.v1"
        ));
    }
    if history
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .is_none()
    {
        return Err(anyhow!("control-plane history missing entries array"));
    }
    Ok(history)
}

fn control_plane_history_diff(
    history_dir: PathBuf,
    from_index: Option<usize>,
    to_index: Option<usize>,
    json: bool,
) -> Result<()> {
    let history = read_control_plane_history(&history_dir)?;
    let entries = json_array(&history, "entries");
    if entries.len() < 2 {
        return Err(anyhow!(
            "at least two history entries are required for control-plane history diff"
        ));
    }
    let default_from = entries.len() - 2;
    let default_to = entries.len() - 1;
    let from_index = from_index.unwrap_or(default_from);
    let to_index = to_index.unwrap_or(default_to);
    let from_entry = entries
        .get(from_index)
        .with_context(|| format!("history --from-index {from_index} is out of range"))?;
    let to_entry = entries
        .get(to_index)
        .with_context(|| format!("history --to-index {to_index} is out of range"))?;
    let from_path = PathBuf::from(json_string(from_entry, "fleet_snapshot_path"));
    let to_path = PathBuf::from(json_string(to_entry, "fleet_snapshot_path"));
    let from_snapshot = read_control_plane_snapshot(&from_path)?;
    let to_snapshot = read_control_plane_snapshot(&to_path)?;
    let from_run_ids = control_plane_fleet_run_ids(&from_snapshot);
    let to_run_ids = control_plane_fleet_run_ids(&to_snapshot);
    let added_run_ids = to_run_ids
        .difference(&from_run_ids)
        .cloned()
        .collect::<Vec<_>>();
    let removed_run_ids = from_run_ids
        .difference(&to_run_ids)
        .cloned()
        .collect::<Vec<_>>();
    let result = serde_json::json!({
        "schema_version": "ao2.control-plane-history-diff.v1",
        "history_path": history_dir.join("history.json"),
        "from_index": from_index,
        "to_index": to_index,
        "from_snapshot_path": from_path,
        "to_snapshot_path": to_path,
        "from_repository_count": json_u64(&from_snapshot["totals"], "repository_count"),
        "to_repository_count": json_u64(&to_snapshot["totals"], "repository_count"),
        "repository_count_delta": json_u64(&to_snapshot["totals"], "repository_count") as i64
            - json_u64(&from_snapshot["totals"], "repository_count") as i64,
        "from_run_count": json_u64(&from_snapshot["totals"], "run_count"),
        "to_run_count": json_u64(&to_snapshot["totals"], "run_count"),
        "run_count_delta": json_u64(&to_snapshot["totals"], "run_count") as i64
            - json_u64(&from_snapshot["totals"], "run_count") as i64,
        "added_run_ids": added_run_ids,
        "removed_run_ids": removed_run_ids
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("history={}", result["history_path"].as_str().unwrap_or(""));
        println!("from_index={from_index}");
        println!("to_index={to_index}");
        println!(
            "repository_delta={}",
            result["repository_count_delta"]
                .as_i64()
                .unwrap_or_default()
        );
        println!(
            "run_delta={}",
            result["run_count_delta"].as_i64().unwrap_or_default()
        );
    }
    Ok(())
}

fn control_plane_history_prune(history_dir: PathBuf, keep: usize, json: bool) -> Result<()> {
    if keep == 0 {
        return Err(anyhow!(
            "control-plane history prune --keep must be greater than 0"
        ));
    }
    let mut history = read_control_plane_history(&history_dir)?;
    let entries = json_array(&history, "entries");
    let mut indexed_entries = entries
        .iter()
        .cloned()
        .enumerate()
        .collect::<Vec<(usize, serde_json::Value)>>();
    indexed_entries.sort_by_key(|(index, entry)| (json_u64(entry, "generated_at_ms"), *index));
    let remove_count = indexed_entries.len().saturating_sub(keep);
    let removed = indexed_entries
        .iter()
        .take(remove_count)
        .map(|(_, entry)| entry.clone())
        .collect::<Vec<_>>();
    let mut kept = indexed_entries
        .into_iter()
        .skip(remove_count)
        .collect::<Vec<(usize, serde_json::Value)>>();
    kept.sort_by_key(|(index, _)| *index);
    let kept_entries = kept.into_iter().map(|(_, entry)| entry).collect::<Vec<_>>();
    let mut removed_paths = Vec::new();
    for entry in &removed {
        let path_text = json_string(entry, "fleet_snapshot_path");
        if path_text.is_empty() {
            continue;
        }
        let path = PathBuf::from(&path_text);
        if path.exists() {
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        }
        removed_paths.push(path_text);
    }
    history["entries"] = serde_json::Value::Array(kept_entries);
    history["generated_at_ms"] = serde_json::json!(now_unix_ms());
    let history_path = history_dir.join("history.json");
    atomic_write_text(&history_path, &serde_json::to_string_pretty(&history)?)?;
    let result = serde_json::json!({
        "schema_version": "ao2.control-plane-history-prune.v1",
        "history_path": history_path,
        "kept_count": json_array(&history, "entries").len(),
        "removed_count": removed.len(),
        "removed_snapshot_paths": removed_paths
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("history={}", result["history_path"].as_str().unwrap_or(""));
        println!("kept={}", result["kept_count"].as_u64().unwrap_or_default());
        println!(
            "removed={}",
            result["removed_count"].as_u64().unwrap_or_default()
        );
    }
    Ok(())
}

fn control_plane_history_export(
    history_dir: PathBuf,
    out: Option<PathBuf>,
    open: bool,
    json: bool,
) -> Result<()> {
    let history = read_control_plane_history(&history_dir)?;
    let html = render_control_plane_history_dashboard(&history)?;
    let path = out.unwrap_or_else(|| history_dir.join("index.html"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, html).with_context(|| format!("write {}", path.display()))?;
    let result = serde_json::json!({
        "schema_version": "ao2.control-plane-history-export.v1",
        "history_path": history_dir.join("history.json"),
        "history_dashboard_path": path,
        "entry_count": json_array(&history, "entries").len()
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!(
            "history_dashboard={}",
            result["history_dashboard_path"].as_str().unwrap_or("")
        );
    }
    if open {
        let path = PathBuf::from(
            result["history_dashboard_path"]
                .as_str()
                .context("history dashboard path is a string")?,
        );
        open_report_target(&path)?;
        println!("open_target={}", path.display());
    }
    Ok(())
}

fn control_plane_fleet_run_ids(fleet: &serde_json::Value) -> BTreeSet<String> {
    let mut run_ids = BTreeSet::new();
    for repo in json_array(fleet, "repositories") {
        for run in json_array(&repo["snapshot"]["runs"], "runs") {
            let run_id = json_string(run, "run_id");
            if !run_id.is_empty() {
                run_ids.insert(run_id);
            }
        }
    }
    run_ids
}

fn control_plane_sources_save(
    targets: Vec<PathBuf>,
    out: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    if targets.is_empty() {
        return Err(anyhow!(
            "at least one --target is required for control-plane sources save"
        ));
    }
    let output_path = out.unwrap_or_else(|| {
        PathBuf::from(".")
            .join(".ao2")
            .join("control-plane")
            .join("sources.json")
    });
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let sources = serde_json::json!({
        "schema_version": "ao2.control-plane-sources.v1",
        "generated_at_ms": now_unix_ms(),
        "targets": targets
    });
    atomic_write_text(&output_path, &serde_json::to_string_pretty(&sources)?)?;
    let result = serde_json::json!({
        "schema_version": "ao2.control-plane-sources.v1",
        "sources_path": output_path,
        "target_count": sources["targets"].as_array().map(Vec::len).unwrap_or_default()
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("sources={}", result["sources_path"].as_str().unwrap_or(""));
        println!(
            "targets={}",
            result["target_count"].as_u64().unwrap_or_default()
        );
    }
    Ok(())
}

fn read_control_plane_sources(path: &Path) -> Result<Vec<PathBuf>> {
    let sources = read_control_plane_snapshot(path)?;
    if json_string(&sources, "schema_version") != "ao2.control-plane-sources.v1" {
        return Err(anyhow!(
            "control-plane sources file must use schema ao2.control-plane-sources.v1"
        ));
    }
    let Some(targets) = sources.get("targets").and_then(serde_json::Value::as_array) else {
        return Ok(Vec::new());
    };
    Ok(targets
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(PathBuf::from)
        .collect())
}

fn control_plane_bundle(
    fleet: PathBuf,
    health_history: Option<PathBuf>,
    out_dir: PathBuf,
    signing_key: Option<PathBuf>,
    signer_id: String,
    json: bool,
) -> Result<()> {
    let fleet_snapshot = read_control_plane_snapshot(&fleet)?;
    if json_string(&fleet_snapshot, "schema_version") != "ao2.control-plane-fleet-snapshot.v1" {
        return Err(anyhow!(
            "control-plane bundle requires an ao2.control-plane-fleet-snapshot.v1 file"
        ));
    }
    fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let generated_at_ms = now_unix_ms();
    let stage_dir = out_dir.join(format!("fleet-bundle-{generated_at_ms}"));
    fs::create_dir_all(&stage_dir).with_context(|| format!("create {}", stage_dir.display()))?;

    let snapshot_path = stage_dir.join("fleet-snapshot.json");
    let bundle_path = stage_dir.join("fleet-bundle.json");
    let sha256_path = stage_dir.join("SHA256SUMS");
    let archive_path = out_dir.join(format!("fleet-bundle-{generated_at_ms}.tar.gz"));

    atomic_write_text(
        &snapshot_path,
        &serde_json::to_string_pretty(&fleet_snapshot)?,
    )?;
    let mut files = vec![
        serde_json::json!({ "path": "fleet-snapshot.json", "role": "source_snapshot" }),
        serde_json::json!({ "path": "fleet-bundle.json", "role": "portable_bundle" }),
        serde_json::json!({ "path": "SHA256SUMS", "role": "checksum_manifest" }),
    ];
    let mut health_history_json = serde_json::Value::Null;
    let mut health_trend_json = serde_json::Value::Null;
    let mut health_history_entry_count = 0_usize;
    if let Some(health_history_dir) = &health_history {
        health_history_json = read_control_plane_health_history(health_history_dir)?;
        health_trend_json = control_plane_health_trend_json(health_history_dir)?;
        let staged_health_history_path = stage_dir.join("health-history.json");
        let staged_health_trend_path = stage_dir.join("health-trend.json");
        let staged_health_dashboard_path = stage_dir.join("health-trend.html");
        atomic_write_text(
            &staged_health_history_path,
            &serde_json::to_string_pretty(&health_history_json)?,
        )?;
        atomic_write_text(
            &staged_health_trend_path,
            &serde_json::to_string_pretty(&health_trend_json)?,
        )?;
        atomic_write_text(
            &staged_health_dashboard_path,
            &render_control_plane_health_trend_dashboard(&health_history_json, &health_trend_json)?,
        )?;
        files.push(serde_json::json!({ "path": "health-history.json", "role": "health_history" }));
        files.push(serde_json::json!({ "path": "health-trend.json", "role": "health_trend" }));
        files.push(serde_json::json!({ "path": "health-trend.html", "role": "health_dashboard" }));
        let entries_dir = stage_dir.join("health-entries");
        fs::create_dir_all(&entries_dir)
            .with_context(|| format!("create {}", entries_dir.display()))?;
        for (index, entry) in json_array(&health_history_json, "entries")
            .iter()
            .enumerate()
        {
            let source_path = PathBuf::from(json_string(entry, "health_path"));
            let filename = source_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("health-entry-{index}.json"));
            let relative_path = format!("health-entries/{filename}");
            fs::copy(&source_path, stage_dir.join(&relative_path)).with_context(|| {
                format!(
                    "copy health entry {} to {}",
                    source_path.display(),
                    relative_path
                )
            })?;
            files.push(serde_json::json!({
                "path": relative_path,
                "role": "health_entry",
                "source_path": source_path
            }));
            health_history_entry_count += 1;
        }
    }
    let support_metadata_signed = signing_key.is_some();
    if support_metadata_signed {
        files.push(serde_json::json!({
            "path": "support-bundle-metadata.json",
            "role": "support_metadata"
        }));
        files.push(serde_json::json!({
            "path": "support-bundle-metadata.json.sig",
            "role": "support_metadata_signature"
        }));
        files.push(serde_json::json!({
            "path": "support-bundle-signing-public.pem",
            "role": "support_metadata_public_key"
        }));
    }
    let bundle_json = serde_json::json!({
        "schema_version": "ao2.control-plane-fleet-bundle.v1",
        "generated_at_ms": generated_at_ms,
        "source_fleet_path": fleet,
        "source_health_history_path": health_history,
        "fleet_snapshot": fleet_snapshot,
        "health_history": health_history_json,
        "health_trend": health_trend_json,
        "files": files
    });
    atomic_write_text(&bundle_path, &serde_json::to_string_pretty(&bundle_json)?)?;

    let mut support_metadata_path = serde_json::Value::Null;
    let mut support_metadata_signature_path = serde_json::Value::Null;
    let mut support_metadata_public_key_path = serde_json::Value::Null;
    if let Some(signing_key_path) = &signing_key {
        let metadata_path = stage_dir.join("support-bundle-metadata.json");
        let signature_path = stage_dir.join("support-bundle-metadata.json.sig");
        let public_key_path = stage_dir.join("support-bundle-signing-public.pem");
        derive_public_key_from_private_key(signing_key_path, &public_key_path)?;
        let metadata = serde_json::json!({
            "schema_version": "ao2.control-plane-support-metadata.v1",
            "generated_at_ms": generated_at_ms,
            "signer_id": signer_id,
            "signature_algorithm": "RSA/SHA-256",
            "producer": {
                "package": env!("CARGO_PKG_NAME"),
                "version": env!("CARGO_PKG_VERSION"),
                "git_commit": runtime_git_commit(),
                "target": runtime_target_label()
            },
            "source_fleet_path": fleet,
            "source_health_history_path": health_history,
            "fleet_bundle_sha256": sha256_file(&bundle_path)?,
            "fleet_snapshot_sha256": sha256_file(&snapshot_path)?,
            "public_key_sha256": sha256_file(&public_key_path)?,
            "repository_count": json_u64(&bundle_json["fleet_snapshot"]["totals"], "repository_count"),
            "run_count": json_u64(&bundle_json["fleet_snapshot"]["totals"], "run_count"),
            "health_history_entry_count": health_history_entry_count,
            "bundle_files": json_array(&bundle_json, "files")
        });
        atomic_write_text(&metadata_path, &serde_json::to_string_pretty(&metadata)?)?;
        sign_file_with_private_key(signing_key_path, &metadata_path, &signature_path)?;
        support_metadata_path = serde_json::json!(metadata_path);
        support_metadata_signature_path = serde_json::json!(signature_path);
        support_metadata_public_key_path = serde_json::json!(public_key_path);
    }

    let manifest = json_array(&bundle_json, "files")
        .iter()
        .filter_map(|file| {
            let relative_path = json_string(file, "path");
            if relative_path == "SHA256SUMS" {
                None
            } else {
                Some(relative_path)
            }
        })
        .map(|relative_path| {
            let digest = sha256_file(&stage_dir.join(&relative_path))?;
            Ok(format!("{digest}  {relative_path}\n"))
        })
        .collect::<Result<String>>()?;
    atomic_write_text(&sha256_path, &manifest)?;
    create_tar_gz(&stage_dir, &archive_path)?;

    let result = serde_json::json!({
        "schema_version": "ao2.control-plane-bundle.v1",
        "bundle_path": bundle_path,
        "archive_path": archive_path,
        "sha256_path": sha256_path,
        "repository_count": json_u64(&bundle_json["fleet_snapshot"]["totals"], "repository_count"),
        "run_count": json_u64(&bundle_json["fleet_snapshot"]["totals"], "run_count"),
        "health_history_entry_count": health_history_entry_count,
        "support_metadata_signed": support_metadata_signed,
        "support_metadata_path": support_metadata_path,
        "support_metadata_signature_path": support_metadata_signature_path,
        "support_metadata_public_key_path": support_metadata_public_key_path
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("bundle={}", result["bundle_path"].as_str().unwrap_or(""));
        println!("archive={}", result["archive_path"].as_str().unwrap_or(""));
        println!("sha256={}", result["sha256_path"].as_str().unwrap_or(""));
    }
    Ok(())
}

fn control_plane_bundle_verify(bundle_dir: PathBuf, json: bool) -> Result<()> {
    let result = control_plane_bundle_verify_json(&bundle_dir)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("verified=true");
        println!("bundle={}", result["bundle_path"].as_str().unwrap_or(""));
        println!(
            "files={}",
            result["file_count"].as_u64().unwrap_or_default()
        );
    }
    Ok(())
}

fn control_plane_bundle_verify_json(bundle_dir: &Path) -> Result<serde_json::Value> {
    let bundle_path = bundle_dir.join("fleet-bundle.json");
    let sha256_path = bundle_dir.join("SHA256SUMS");
    let bundle = read_control_plane_snapshot(&bundle_path)?;
    if json_string(&bundle, "schema_version") != "ao2.control-plane-fleet-bundle.v1" {
        return Err(anyhow!(
            "control-plane bundle verify requires an ao2.control-plane-fleet-bundle.v1 file"
        ));
    }
    let manifest = fs::read_to_string(&sha256_path)
        .with_context(|| format!("read {}", sha256_path.display()))?;
    let mut verified_files = Vec::new();
    for (line_number, line) in manifest.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let expected = parts
            .next()
            .with_context(|| format!("missing checksum on SHA256SUMS line {}", line_number + 1))?;
        let relative_path = parts
            .next()
            .with_context(|| format!("missing path on SHA256SUMS line {}", line_number + 1))?;
        if parts.next().is_some() {
            return Err(anyhow!(
                "invalid SHA256SUMS line {}: expected '<sha256>  <path>'",
                line_number + 1
            ));
        }
        let file_path = bundle_dir.join(relative_path);
        let actual = sha256_file(&file_path)?;
        if actual != expected {
            return Err(anyhow!(
                "checksum mismatch for {}: expected {}, got {}",
                relative_path,
                expected,
                actual
            ));
        }
        verified_files.push(serde_json::json!({
            "path": relative_path,
            "sha256": actual
        }));
    }
    Ok(serde_json::json!({
        "schema_version": "ao2.control-plane-bundle-verify.v1",
        "verified": true,
        "bundle_path": bundle_path,
        "sha256_path": sha256_path,
        "file_count": verified_files.len(),
        "files": verified_files,
        "support_metadata": support_bundle_metadata_verification_json(bundle_dir)?,
        "repository_count": json_u64(&bundle["fleet_snapshot"]["totals"], "repository_count"),
        "run_count": json_u64(&bundle["fleet_snapshot"]["totals"], "run_count")
    }))
}

fn support_bundle_metadata_verification_json(bundle_dir: &Path) -> Result<serde_json::Value> {
    let metadata_path = bundle_dir.join("support-bundle-metadata.json");
    let signature_path = bundle_dir.join("support-bundle-metadata.json.sig");
    let public_key_path = bundle_dir.join("support-bundle-signing-public.pem");
    if !metadata_path.exists() && !signature_path.exists() && !public_key_path.exists() {
        return Ok(serde_json::json!({
            "present": false,
            "signature_verified": false
        }));
    }
    if !metadata_path.is_file() || !signature_path.is_file() || !public_key_path.is_file() {
        return Err(anyhow!(
            "support bundle metadata is incomplete; metadata, signature, and public key are all required"
        ));
    }
    let metadata = read_control_plane_snapshot(&metadata_path)?;
    if json_string(&metadata, "schema_version") != "ao2.control-plane-support-metadata.v1" {
        return Err(anyhow!(
            "support bundle metadata must use schema ao2.control-plane-support-metadata.v1"
        ));
    }
    let signature_verified =
        verify_file_signature(&metadata_path, &signature_path, &public_key_path)?;
    if !signature_verified {
        return Err(anyhow!(
            "support bundle metadata signature verification failed"
        ));
    }
    Ok(serde_json::json!({
        "present": true,
        "signature_verified": signature_verified,
        "metadata_path": metadata_path,
        "signature_path": signature_path,
        "public_key_path": public_key_path,
        "metadata_sha256": sha256_file(&metadata_path)?,
        "signature_sha256": sha256_file(&signature_path)?,
        "public_key_sha256": sha256_file(&public_key_path)?,
        "signer_id": json_string(&metadata, "signer_id"),
        "signature_algorithm": json_string(&metadata, "signature_algorithm"),
        "metadata": metadata
    }))
}

pub(crate) fn write_workbench_support_metadata(
    target: &Path,
    bundle: &serde_json::Value,
    bundle_path: &Path,
    generated_at_ms: u64,
    signing: &WorkbenchSupportSigning,
) -> Result<serde_json::Value> {
    let bundle_dir = bundle_path
        .parent()
        .with_context(|| format!("resolve bundle directory for {}", bundle_path.display()))?;
    let metadata_path = bundle_dir.join("support-bundle-metadata.json");
    let signature_path = bundle_dir.join("support-bundle-metadata.json.sig");
    let public_key_path = bundle_dir.join("support-bundle-signing-public.pem");
    derive_public_key_from_private_key(&signing.key_path, &public_key_path)?;
    let metadata = serde_json::json!({
        "schema_version": "ao2.workbench-support-metadata.v1",
        "generated_at_ms": generated_at_ms,
        "signer_id": signing.signer_id,
        "signature_algorithm": "RSA/SHA-256",
        "producer": {
            "package": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "git_commit": runtime_git_commit(),
            "target": runtime_target_label()
        },
        "target": target,
        "workbench_support_bundle_path": bundle_path,
        "workbench_support_bundle_sha256": sha256_file(bundle_path)?,
        "public_key_sha256": sha256_file(&public_key_path)?,
        "queue_job_count": json_array(&bundle["queue"], "jobs").len(),
        "audit_event_count": json_array(bundle, "audit_events").len(),
        "job_log_count": json_array(bundle, "job_logs").len(),
        "evidence_export_count": json_array(bundle, "evidence_exports").len(),
        "redaction_count": json_u64(&bundle["redaction_audit"], "redaction_count"),
        "redaction_classes": bundle["redaction_audit"]["secret_classes"].clone()
    });
    atomic_write_text(&metadata_path, &serde_json::to_string_pretty(&metadata)?)?;
    sign_file_with_private_key(&signing.key_path, &metadata_path, &signature_path)?;
    workbench_support_metadata_verification_json(bundle_dir)
}

fn workbench_support_metadata_verification_json(bundle_dir: &Path) -> Result<serde_json::Value> {
    let metadata_path = bundle_dir.join("support-bundle-metadata.json");
    let signature_path = bundle_dir.join("support-bundle-metadata.json.sig");
    let public_key_path = bundle_dir.join("support-bundle-signing-public.pem");
    if !metadata_path.exists() && !signature_path.exists() && !public_key_path.exists() {
        return Ok(serde_json::json!({
            "present": false,
            "signature_verified": false
        }));
    }
    if !metadata_path.is_file() || !signature_path.is_file() || !public_key_path.is_file() {
        return Err(anyhow!(
            "workbench support bundle metadata is incomplete; metadata, signature, and public key are all required"
        ));
    }
    let metadata: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&metadata_path)
            .with_context(|| format!("read {}", metadata_path.display()))?,
    )
    .with_context(|| format!("parse {}", metadata_path.display()))?;
    if json_string(&metadata, "schema_version") != "ao2.workbench-support-metadata.v1" {
        return Err(anyhow!(
            "workbench support bundle metadata must use schema ao2.workbench-support-metadata.v1"
        ));
    }
    let signature_verified =
        verify_file_signature(&metadata_path, &signature_path, &public_key_path)?;
    if !signature_verified {
        return Err(anyhow!(
            "workbench support bundle metadata signature verification failed"
        ));
    }
    Ok(serde_json::json!({
        "present": true,
        "signature_verified": signature_verified,
        "metadata_path": metadata_path,
        "signature_path": signature_path,
        "public_key_path": public_key_path,
        "metadata_sha256": sha256_file(&metadata_path)?,
        "signature_sha256": sha256_file(&signature_path)?,
        "public_key_sha256": sha256_file(&public_key_path)?,
        "signer_id": json_string(&metadata, "signer_id"),
        "signature_algorithm": json_string(&metadata, "signature_algorithm"),
        "metadata": metadata
    }))
}

pub(crate) fn workbench_support_bundle_verify(bundle_dir: PathBuf, json: bool) -> Result<()> {
    let result = workbench_support_bundle_verify_json(&bundle_dir)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("verified=true");
        println!("bundle={}", result["bundle_path"].as_str().unwrap_or(""));
        println!(
            "queue_jobs={}",
            result["queue_job_count"].as_u64().unwrap_or_default()
        );
        println!(
            "audit_events={}",
            result["audit_event_count"].as_u64().unwrap_or_default()
        );
        println!(
            "evidence_exports={}",
            result["evidence_export_count"].as_u64().unwrap_or_default()
        );
        let support_metadata = &result["support_metadata"];
        println!(
            "support_metadata={}",
            support_metadata_status_text(support_metadata)
        );
        if support_metadata
            .get("present")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            println!("signer_id={}", json_string(support_metadata, "signer_id"));
        }
    }
    Ok(())
}

pub(crate) fn workbench_support_bundle_verify_json(bundle_dir: &Path) -> Result<serde_json::Value> {
    let bundle_path = bundle_dir.join("support-bundle.json");
    if !bundle_path.is_file() {
        return Err(anyhow!(
            "workbench support bundle requires {}",
            bundle_path.display()
        ));
    }
    let bundle: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&bundle_path)
            .with_context(|| format!("read {}", bundle_path.display()))?,
    )
    .with_context(|| format!("parse {}", bundle_path.display()))?;
    if json_string(&bundle, "schema_version") != "ao2.workbench-support-bundle.v1" {
        return Err(anyhow!(
            "workbench support bundle must use schema ao2.workbench-support-bundle.v1"
        ));
    }
    if json_string(&bundle["queue"], "schema_version") != "ao2.workbench-queue.v1" {
        return Err(anyhow!(
            "workbench support bundle queue must use schema ao2.workbench-queue.v1"
        ));
    }
    let support_metadata = workbench_support_metadata_verification_json(bundle_dir)?;
    let queue_job_count = json_array(&bundle["queue"], "jobs").len();
    let audit_event_count = json_array(&bundle, "audit_events").len();
    let job_log_count = json_array(&bundle, "job_logs").len();
    let queue_job_diagnoses = workbench_support_queue_job_diagnoses(&bundle);
    let queue_job_diagnosis_count = queue_job_diagnoses.len();
    let evidence_exports = workbench_support_evidence_export_summaries(&bundle);
    let evidence_export_count = json_array(&bundle, "evidence_exports").len();
    let hermes_project_start_flow_contract =
        workbench_support_hermes_flow_contract_summary(&bundle);
    let redaction_audit = bundle
        .get("redaction_audit")
        .cloned()
        .unwrap_or_else(empty_workbench_redaction_audit);
    if support_metadata
        .get("present")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        let metadata = &support_metadata["metadata"];
        let expected_bundle_sha = json_string(metadata, "workbench_support_bundle_sha256");
        let actual_bundle_sha = sha256_file(&bundle_path)?;
        if expected_bundle_sha != actual_bundle_sha {
            return Err(anyhow!(
                "workbench support bundle digest mismatch in signed metadata"
            ));
        }
        if metadata
            .get("queue_job_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as usize
            != queue_job_count
        {
            return Err(anyhow!(
                "workbench support bundle queue job count mismatch in signed metadata"
            ));
        }
        if metadata
            .get("audit_event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as usize
            != audit_event_count
        {
            return Err(anyhow!(
                "workbench support bundle audit event count mismatch in signed metadata"
            ));
        }
        if metadata
            .get("job_log_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as usize
            != job_log_count
        {
            return Err(anyhow!(
                "workbench support bundle job log count mismatch in signed metadata"
            ));
        }
        if metadata
            .get("evidence_export_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as usize
            != evidence_export_count
        {
            return Err(anyhow!(
                "workbench support bundle evidence export count mismatch in signed metadata"
            ));
        }
        if metadata.get("redaction_count").is_some() {
            if metadata
                .get("redaction_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default()
                != json_u64(&redaction_audit, "redaction_count")
            {
                return Err(anyhow!(
                    "workbench support bundle redaction count mismatch in signed metadata"
                ));
            }
            if metadata
                .get("redaction_classes")
                .unwrap_or(&serde_json::Value::Null)
                != redaction_audit
                    .get("secret_classes")
                    .unwrap_or(&serde_json::Value::Null)
            {
                return Err(anyhow!(
                    "workbench support bundle redaction classes mismatch in signed metadata"
                ));
            }
        }
    }

    let mut files = vec![serde_json::json!({
        "path": "support-bundle.json",
        "sha256": sha256_file(&bundle_path)?
    })];
    for path in [
        "support-bundle-metadata.json",
        "support-bundle-metadata.json.sig",
        "support-bundle-signing-public.pem",
    ] {
        let file_path = bundle_dir.join(path);
        if file_path.is_file() {
            files.push(serde_json::json!({
                "path": path,
                "sha256": sha256_file(&file_path)?
            }));
        }
    }

    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-support-bundle-verify.v1",
        "verified": true,
        "bundle_dir": bundle_dir,
        "bundle_path": bundle_path,
        "bundle_sha256": sha256_file(&bundle_path)?,
        "queue_job_count": queue_job_count,
        "queue_job_diagnosis_count": queue_job_diagnosis_count,
        "queue_job_diagnoses": queue_job_diagnoses,
        "audit_event_count": audit_event_count,
        "job_log_count": job_log_count,
        "evidence_export_count": evidence_export_count,
        "evidence_exports": evidence_exports,
        "hermes_project_start_flow_contract": hermes_project_start_flow_contract,
        "redaction_audit": redaction_audit,
        "support_metadata": support_metadata,
        "files": files
    }))
}

fn workbench_support_hermes_flow_contract_summary(bundle: &serde_json::Value) -> serde_json::Value {
    let contract = &bundle["hermes_project_start_flow_contract"];
    if !contract.is_object() {
        return serde_json::json!({ "present": false });
    }
    let workflow = &contract["workflow"];
    let hermes = &contract["hermes_contract"];
    let side_effects = &contract["side_effects"];
    let trust_boundary = &contract["trust_boundary"];
    serde_json::json!({
        "present": true,
        "schema_version": json_string(contract, "schema_version"),
        "contract_sha256": json_string(contract, "contract_sha256"),
        "preview_role": json_string(&workflow["preview"], "minimum_role"),
        "publish_role": json_string(&workflow["publish"], "minimum_role"),
        "raw_queue_json_scrape_required": hermes["raw_queue_json_scrape_required"]
            .as_bool()
            .unwrap_or(false),
        "would_execute_queue": side_effects["would_execute_queue"].as_bool().unwrap_or(false),
        "would_submit_queue_entry": side_effects["would_submit_queue_entry"]
            .as_bool()
            .unwrap_or(false),
        "would_rebuild_wrappers": side_effects["would_rebuild_wrappers"].as_bool().unwrap_or(false),
        "would_mutate_control_plane": side_effects["would_mutate_control_plane"]
            .as_bool()
            .unwrap_or(false),
        "release_acceptance_owner": json_string(trust_boundary, "release_acceptance_owner"),
        "control_plane_approves_release": trust_boundary["control_plane_approves_release"]
            .as_bool()
            .unwrap_or(false),
        "mutates_ao_artifacts": trust_boundary["mutates_ao_artifacts"]
            .as_bool()
            .unwrap_or(false)
    })
}

fn workbench_support_queue_job_diagnoses(bundle: &serde_json::Value) -> Vec<serde_json::Value> {
    json_array(bundle, "job_logs")
        .iter()
        .filter_map(|job_log| {
            let job = &job_log["job"];
            let diagnosis = if job_log.get("diagnosis").is_some() {
                &job_log["diagnosis"]
            } else {
                &job["diagnosis"]
            };
            let failure_kind = json_string(diagnosis, "failure_kind");
            if failure_kind.is_empty() || failure_kind == "none" {
                return None;
            }
            Some(serde_json::json!({
                "job_id": json_string(job, "job_id"),
                "run_id": json_string(job, "run_id"),
                "provider": json_string(job, "provider"),
                "status": json_string(job, "status"),
                "failure_kind": failure_kind,
                "exit_code": diagnosis
                    .get("exit_code")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or_default(),
                "timed_out": diagnosis
                    .get("timed_out")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                "primary_error": json_string(diagnosis, "primary_error"),
                "stderr_excerpt": json_string(diagnosis, "stderr_excerpt"),
                "stdout_excerpt": json_string(diagnosis, "stdout_excerpt"),
                "recovery_actions": json_array(diagnosis, "recovery_actions")
            }))
        })
        .collect()
}

fn workbench_support_evidence_export_summaries(
    bundle: &serde_json::Value,
) -> Vec<serde_json::Value> {
    json_array(bundle, "evidence_exports")
        .iter()
        .map(|evidence_export| {
            let content = &evidence_export["content"];
            let body = &content["export"];
            let kind = json_string(evidence_export, "kind");
            let summary = &body["summary"];
            let diff = &body["diff"];
            let changes = &body["changes"];
            let release_history = &body["release_history"];
            let release_comparison_verification = &body["release_comparison_verification"];
            let provider_pilot_acceptance = &body["provider_pilot_acceptance"];
            let operator_packet = &body["operator_packet"];
            serde_json::json!({
                "path": evidence_export.get("path").cloned().unwrap_or(serde_json::Value::Null),
                "sha256": json_string(evidence_export, "sha256"),
                "kind": kind,
                "schema_version": json_string(content, "schema_version"),
                "generated_at_ms": evidence_export
                    .get("generated_at_ms")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default(),
                "run_id": json_string(summary, "run_id"),
                "left_run_id": json_string(diff, "left_run_id"),
                "right_run_id": json_string(diff, "right_run_id"),
                "baseline_run_id": json_string(&changes["baseline"], "run_id"),
                "selected_run_id": json_string(&changes["selected"], "run_id"),
                "latest_release_tag": json_string(&release_history["trend"], "latest_release_tag"),
                "release_entry_count": json_u64(&release_history["trend"], "entry_count"),
                "release_comparison_bundle_dir": json_string(body, "release_comparison_bundle_dir"),
                "release_comparison_latest_release_tag": json_string(
                    release_comparison_verification,
                    "latest_release_tag"
                ),
                "release_comparison_release_count": json_u64(
                    release_comparison_verification,
                    "release_count"
                ),
                "release_comparison_regression_count": json_u64(
                    release_comparison_verification,
                    "regression_count"
                ),
                "release_comparison_manifest_verified": release_comparison_verification
                    .get("manifest_verified")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                "release_comparison_signature_verified": release_comparison_verification
                    .get("signature_verified")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                "provider_pilot_acceptance_bundle": json_string(body, "provider_pilot_acceptance_bundle"),
                "provider_pilot_schema_version": json_string(provider_pilot_acceptance, "schema_version"),
                "provider_pilot_status": json_string(provider_pilot_acceptance, "status"),
                "provider_pilot_provider": json_string(provider_pilot_acceptance, "provider"),
                "provider_pilot_run_id": json_string(provider_pilot_acceptance, "run_id"),
                "provider_pilot_score": json_u64(&provider_pilot_acceptance["score"], "score"),
                "provider_pilot_verdict": json_string(&provider_pilot_acceptance["score"], "verdict"),
                "provider_pilot_replay_status": json_string(&provider_pilot_acceptance["replay"], "status"),
                "provider_pilot_digest_failure_count": json_array(&provider_pilot_acceptance["replay"], "digest_failures").len(),
                "provider_pilot_evidence_pack": json_string(provider_pilot_acceptance, "evidence_pack"),
                "provider_pilot_cockpit": json_string(provider_pilot_acceptance, "cockpit"),
                "operator_packet_schema_version": json_string(operator_packet, "schema_version"),
                "operator_packet_run_id": json_string(operator_packet, "run_id"),
                "operator_packet_closure_verdict": json_string(&operator_packet["evaluator_closure"], "verdict"),
                "operator_packet_replay_status": json_string(&operator_packet["replay"], "status"),
                "operator_packet_provider_score_present": operator_packet["provider_scorecard"]["present"]
                    .as_bool()
                    .unwrap_or(false),
                "operator_packet_provider_score": json_u64(&operator_packet["provider_scorecard"], "score"),
                "operator_packet_static_report_present": !json_string(
                    &operator_packet["artifacts"]["static_report"],
                    "sha256"
                )
                .is_empty(),
                "operator_packet_run_record_sha256": json_string(
                    &operator_packet["artifacts"]["run_record"],
                    "sha256"
                ),
                "operator_packet_evidence_pack_sha256": json_string(
                    &operator_packet["artifacts"]["evidence_pack"],
                    "sha256"
                )
            })
        })
        .collect()
}

fn workbench_support_evidence_export_text(summary: &serde_json::Value) -> String {
    let kind = json_string(summary, "kind");
    let run_id = json_string(summary, "run_id");
    let selected_run_id = json_string(summary, "selected_run_id");
    let baseline_run_id = json_string(summary, "baseline_run_id");
    let left_run_id = json_string(summary, "left_run_id");
    let right_run_id = json_string(summary, "right_run_id");
    let sha256 = json_string(summary, "sha256");
    let subject = if !run_id.is_empty() {
        run_id
    } else if !selected_run_id.is_empty() || !baseline_run_id.is_empty() {
        format!("{baseline_run_id}->{selected_run_id}")
    } else if !left_run_id.is_empty() || !right_run_id.is_empty() {
        format!("{left_run_id}->{right_run_id}")
    } else if !json_string(summary, "latest_release_tag").is_empty() {
        format!(
            "{} entries={}",
            json_string(summary, "latest_release_tag"),
            json_u64(summary, "release_entry_count")
        )
    } else if !json_string(summary, "release_comparison_latest_release_tag").is_empty() {
        format!(
            "{} releases={} regressions={}",
            json_string(summary, "release_comparison_latest_release_tag"),
            json_u64(summary, "release_comparison_release_count"),
            json_u64(summary, "release_comparison_regression_count")
        )
    } else if !json_string(summary, "provider_pilot_run_id").is_empty() {
        format!(
            "{} {} score={} replay={} digest_failures={}",
            json_string(summary, "provider_pilot_provider"),
            json_string(summary, "provider_pilot_run_id"),
            json_u64(summary, "provider_pilot_score"),
            json_string(summary, "provider_pilot_replay_status"),
            json_u64(summary, "provider_pilot_digest_failure_count")
        )
    } else if !json_string(summary, "operator_packet_run_id").is_empty() {
        format!(
            "{} closure={} replay={} score={} sha256={}",
            json_string(summary, "operator_packet_run_id"),
            json_string(summary, "operator_packet_closure_verdict"),
            json_string(summary, "operator_packet_replay_status"),
            json_u64(summary, "operator_packet_provider_score"),
            json_string(summary, "operator_packet_run_record_sha256")
        )
    } else {
        String::from("unknown")
    };
    format!("{kind} {subject} sha256={sha256}")
}

fn workbench_support_queue_job_diagnosis_text(diagnosis: &serde_json::Value) -> String {
    let mut text = format!(
        "{} {} exit={} timed_out={}",
        json_string(diagnosis, "run_id"),
        json_string(diagnosis, "failure_kind"),
        diagnosis
            .get("exit_code")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default(),
        diagnosis
            .get("timed_out")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    );
    let primary_error = json_string(diagnosis, "primary_error");
    if !primary_error.is_empty() {
        text.push_str(" error=");
        text.push_str(&primary_error);
    }
    let recovery = workbench_support_queue_job_recovery_text(diagnosis);
    if !recovery.is_empty() {
        text.push_str(" recovery=");
        text.push_str(&recovery);
    }
    text
}

fn workbench_support_queue_job_recovery_text(diagnosis: &serde_json::Value) -> String {
    json_array(diagnosis, "recovery_actions")
        .first()
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string()
}

pub(crate) fn render_workbench_queue_failure_diagnostics_table(
    diagnoses: &[serde_json::Value],
    empty_colspan: usize,
) -> String {
    if diagnoses.is_empty() {
        return format!(
            "<tr><td colspan=\"{empty_colspan}\" class=\"muted\">No queue failure diagnostics.</td></tr>"
        );
    }
    diagnoses
        .iter()
        .map(|diagnosis| {
            format!(
                "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td></tr>",
                escape_html(&json_string(diagnosis, "run_id")),
                escape_html(&json_string(diagnosis, "failure_kind")),
                diagnosis
                    .get("exit_code")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or_default(),
                diagnosis
                    .get("timed_out")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                escape_html(&json_string(diagnosis, "primary_error")),
                escape_html(&workbench_support_queue_job_recovery_text(diagnosis)),
                escape_html(&json_string(diagnosis, "stderr_excerpt"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn workbench_support_bundle_import(
    bundle_dir: PathBuf,
    out_dir: PathBuf,
    json: bool,
) -> Result<()> {
    workbench_support_bundle_verify_json(&bundle_dir)?;
    fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let generated_at_ms = now_unix_ms();
    let case_dir = out_dir.join(format!("workbench-support-import-{generated_at_ms}"));
    let imported_bundle_dir = case_dir.join("bundle");
    fs::create_dir_all(&imported_bundle_dir)
        .with_context(|| format!("create {}", imported_bundle_dir.display()))?;
    copy_dir_recursive(&bundle_dir, &imported_bundle_dir)?;
    let verify = workbench_support_bundle_verify_json(&imported_bundle_dir)?;
    let summary_path = case_dir.join("import-summary.json");
    let index_path = case_dir.join("index.html");
    let summary = serde_json::json!({
        "schema_version": "ao2.workbench-support-bundle-import.v1",
        "generated_at_ms": generated_at_ms,
        "verified": true,
        "input_kind": "directory",
        "input_path": bundle_dir,
        "case_dir": case_dir,
        "bundle_dir": imported_bundle_dir,
        "summary_path": summary_path,
        "index_path": index_path,
        "queue_job_count": json_u64(&verify, "queue_job_count"),
        "queue_job_diagnosis_count": json_u64(&verify, "queue_job_diagnosis_count"),
        "queue_job_diagnoses": json_array(&verify, "queue_job_diagnoses"),
        "audit_event_count": json_u64(&verify, "audit_event_count"),
        "job_log_count": json_u64(&verify, "job_log_count"),
        "evidence_export_count": json_u64(&verify, "evidence_export_count"),
        "evidence_exports": json_array(&verify, "evidence_exports"),
        "redaction_audit": verify["redaction_audit"].clone(),
        "support_metadata": verify["support_metadata"].clone(),
        "verify": verify
    });
    atomic_write_text(&summary_path, &serde_json::to_string_pretty(&summary)?)?;
    atomic_write_text(
        &index_path,
        &render_workbench_support_bundle_import_html(&summary)?,
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!("verified=true");
        println!("case={}", summary["case_dir"].as_str().unwrap_or(""));
        println!("summary={}", summary["summary_path"].as_str().unwrap_or(""));
        println!("index={}", summary["index_path"].as_str().unwrap_or(""));
        println!(
            "evidence_exports={}",
            summary["evidence_export_count"]
                .as_u64()
                .unwrap_or_default()
        );
        println!(
            "queue_job_diagnoses={}",
            summary["queue_job_diagnosis_count"]
                .as_u64()
                .unwrap_or_default()
        );
        println!(
            "redactions={}",
            json_u64(&summary["redaction_audit"], "redaction_count")
        );
    }
    Ok(())
}

pub(crate) fn workbench_support_bundle_inspect(bundle_dir: PathBuf, json: bool) -> Result<()> {
    let summary = workbench_support_bundle_inspect_json(bundle_dir)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!("verified=true");
        println!("bundle={}", summary["bundle_path"].as_str().unwrap_or(""));
        println!(
            "queue_jobs={}",
            summary["queue_job_count"].as_u64().unwrap_or_default()
        );
        println!(
            "audit_events={}",
            summary["audit_event_count"].as_u64().unwrap_or_default()
        );
        println!(
            "evidence_exports={}",
            summary["evidence_export_count"]
                .as_u64()
                .unwrap_or_default()
        );
        println!(
            "redactions={}",
            json_u64(&summary["redaction_audit"], "redaction_count")
        );
        for (index, diagnosis) in json_array(&summary, "queue_job_diagnoses")
            .iter()
            .enumerate()
        {
            println!(
                "queue_job_diagnosis_{}={}",
                index + 1,
                workbench_support_queue_job_diagnosis_text(diagnosis)
            );
        }
        for (index, evidence_export) in json_array(&summary, "evidence_exports").iter().enumerate()
        {
            println!(
                "evidence_export_{}={}",
                index + 1,
                workbench_support_evidence_export_text(evidence_export)
            );
        }
        let support_metadata = &summary["support_metadata"];
        println!(
            "support_metadata={}",
            support_metadata_status_text(support_metadata)
        );
        if support_metadata
            .get("present")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            println!("signer_id={}", json_string(support_metadata, "signer_id"));
        }
    }
    Ok(())
}

pub(crate) fn workbench_support_bundle_inspect_json(
    bundle_dir: PathBuf,
) -> Result<serde_json::Value> {
    let verify = workbench_support_bundle_verify_json(&bundle_dir)?;
    Ok(serde_json::json!({
        "schema_version": "ao2.workbench-support-bundle-inspect.v1",
        "generated_at_ms": now_unix_ms(),
        "verified": true,
        "input_kind": "directory",
        "input_path": bundle_dir,
        "bundle_dir": verify["bundle_dir"].clone(),
        "bundle_path": verify["bundle_path"].clone(),
        "bundle_sha256": verify["bundle_sha256"].clone(),
        "queue_job_count": json_u64(&verify, "queue_job_count"),
        "queue_job_diagnosis_count": json_u64(&verify, "queue_job_diagnosis_count"),
        "queue_job_diagnoses": json_array(&verify, "queue_job_diagnoses"),
        "audit_event_count": json_u64(&verify, "audit_event_count"),
        "job_log_count": json_u64(&verify, "job_log_count"),
        "evidence_export_count": json_u64(&verify, "evidence_export_count"),
        "evidence_exports": json_array(&verify, "evidence_exports"),
        "redaction_audit": verify["redaction_audit"].clone(),
        "support_metadata": verify["support_metadata"].clone(),
        "files": json_array(&verify, "files"),
        "verify": verify
    }))
}

fn render_workbench_support_bundle_import_html(summary: &serde_json::Value) -> Result<String> {
    let trust = render_support_metadata_trust_html(&summary["support_metadata"]);
    let files = json_array(&summary["verify"], "files")
        .iter()
        .map(|file| {
            format!(
                "<tr><td><code>{}</code></td><td><code>{}</code></td></tr>",
                escape_html(&json_string(file, "path")),
                escape_html(&json_string(file, "sha256"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let evidence_exports = json_array(summary, "evidence_exports")
        .iter()
        .map(|evidence_export| {
            let subject = workbench_support_evidence_export_subject(evidence_export);
            format!(
                "<tr><td>{}</td><td><code>{}</code></td><td><code>{}</code></td><td><code>{}</code></td></tr>",
                escape_html(&json_string(evidence_export, "kind")),
                escape_html(&subject),
                escape_html(&json_string(evidence_export, "sha256")),
                escape_html(&json_string(evidence_export, "path"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let evidence_exports_section = if evidence_exports.is_empty() {
        String::from(
            "<section>\n<h2>Evidence Exports</h2>\n<p>No attached evidence exports.</p>\n</section>",
        )
    } else {
        format!(
            r#"<section>
<h2>Evidence Exports</h2>
<table><thead><tr><th>Kind</th><th>Run</th><th>SHA256</th><th>Path</th></tr></thead><tbody>
{evidence_exports}
</tbody></table>
</section>"#
        )
    };
    let queue_diagnosis_entries = json_array(summary, "queue_job_diagnoses");
    let queue_diagnoses =
        render_workbench_queue_failure_diagnostics_table(queue_diagnosis_entries, 7);
    let redaction_audit_section =
        render_workbench_redaction_audit_section(&summary["redaction_audit"]);
    let queue_diagnoses_section = format!(
        r#"<section>
<h2>Queue Failure Diagnostics</h2>
<table><thead><tr><th>Run</th><th>Failure</th><th>Exit</th><th>Timed Out</th><th>Primary Error</th><th>Recovery</th><th>Stderr</th></tr></thead><tbody>
{queue_diagnoses}
</tbody></table>
</section>"#
    );
    Ok(format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Workbench Support Bundle</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; margin: 32px; color: #172026; background: #f7f8fa; }}
main {{ max-width: 1080px; margin: 0 auto; }}
section {{ background: #fff; border: 1px solid #d9dee5; border-radius: 8px; padding: 20px; margin: 16px 0; }}
.metrics {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; }}
.metric {{ border: 1px solid #e1e6ed; border-radius: 6px; padding: 12px; }}
.label {{ color: #5b6573; font-size: 12px; text-transform: uppercase; }}
.value {{ font-size: 22px; font-weight: 700; margin-top: 4px; }}
table {{ border-collapse: collapse; width: 100%; }}
td, th {{ border-top: 1px solid #e1e6ed; padding: 8px; text-align: left; }}
code {{ word-break: break-all; }}
</style>
</head>
<body>
<main>
<h1>Workbench Support Bundle</h1>
<section class="metrics">
  <div class="metric"><div class="label">Verified</div><div class="value">{verified}</div></div>
  <div class="metric"><div class="label">Queue Jobs</div><div class="value">{queue_jobs}</div></div>
  <div class="metric"><div class="label">Audit Events</div><div class="value">{audit_events}</div></div>
  <div class="metric"><div class="label">Job Logs</div><div class="value">{job_logs}</div></div>
  <div class="metric"><div class="label">Evidence Exports</div><div class="value">{evidence_export_count}</div></div>
  <div class="metric"><div class="label">Redactions</div><div class="value">{redaction_count}</div></div>
</section>
{redaction_audit_section}
{trust}
{queue_diagnoses_section}
{evidence_exports_section}
<section>
<h2>Imported Evidence</h2>
<p>Bundle directory: <code>{bundle_dir}</code></p>
</section>
<section>
<h2>Verified Files</h2>
<table><thead><tr><th>Path</th><th>SHA256</th></tr></thead><tbody>
{files}
</tbody></table>
</section>
</main>
</body>
</html>
"#,
        verified = escape_html(&json_string(summary, "verified")),
        queue_jobs = json_u64(summary, "queue_job_count"),
        audit_events = json_u64(summary, "audit_event_count"),
        job_logs = json_u64(summary, "job_log_count"),
        evidence_export_count = json_u64(summary, "evidence_export_count"),
        redaction_count = json_u64(&summary["redaction_audit"], "redaction_count"),
        bundle_dir = escape_html(&json_string(summary, "bundle_dir")),
        redaction_audit_section = redaction_audit_section,
        trust = trust,
        queue_diagnoses_section = queue_diagnoses_section,
        evidence_exports_section = evidence_exports_section,
        files = files
    ))
}

pub(crate) fn workbench_support_evidence_export_subject(summary: &serde_json::Value) -> String {
    let run_id = json_string(summary, "run_id");
    if !run_id.is_empty() {
        return run_id;
    }
    let selected_run_id = json_string(summary, "selected_run_id");
    let baseline_run_id = json_string(summary, "baseline_run_id");
    if !selected_run_id.is_empty() || !baseline_run_id.is_empty() {
        return format!("{baseline_run_id} -> {selected_run_id}");
    }
    let left_run_id = json_string(summary, "left_run_id");
    let right_run_id = json_string(summary, "right_run_id");
    if !left_run_id.is_empty() || !right_run_id.is_empty() {
        return format!("{left_run_id} -> {right_run_id}");
    }
    let latest_release_tag = json_string(summary, "latest_release_tag");
    if !latest_release_tag.is_empty() {
        return format!(
            "{} entries={}",
            latest_release_tag,
            json_u64(summary, "release_entry_count")
        );
    }
    let provider_pilot_run_id = json_string(summary, "provider_pilot_run_id");
    if !provider_pilot_run_id.is_empty() {
        return format!(
            "{} {} score={} replay={} digest_failures={}",
            json_string(summary, "provider_pilot_provider"),
            provider_pilot_run_id,
            json_u64(summary, "provider_pilot_score"),
            json_string(summary, "provider_pilot_replay_status"),
            json_u64(summary, "provider_pilot_digest_failure_count")
        );
    }
    let operator_packet_run_id = json_string(summary, "operator_packet_run_id");
    if !operator_packet_run_id.is_empty() {
        return format!(
            "{} closure={} replay={} score={}",
            operator_packet_run_id,
            json_string(summary, "operator_packet_closure_verdict"),
            json_string(summary, "operator_packet_replay_status"),
            json_u64(summary, "operator_packet_provider_score")
        );
    }
    String::from("unknown")
}

pub(crate) fn render_workbench_redaction_audit_section(audit: &serde_json::Value) -> String {
    let rows = audit
        .get("secret_classes")
        .and_then(serde_json::Value::as_object)
        .map(|classes| {
            classes
                .iter()
                .map(|(class, count)| {
                    format!(
                        "<tr><td><code>{}</code></td><td>{}</td></tr>",
                        escape_html(class),
                        count.as_u64().unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let rows = if rows.is_empty() {
        String::from("<tr><td colspan=\"2\" class=\"muted\">No redactions recorded.</td></tr>")
    } else {
        rows
    };
    format!(
        r#"<section>
<h2>Redaction Audit</h2>
<p>Total redactions: <strong>{}</strong></p>
<table><thead><tr><th>Secret Class</th><th>Count</th></tr></thead><tbody>
{}
</tbody></table>
</section>"#,
        json_u64(audit, "redaction_count"),
        rows
    )
}

fn control_plane_bundle_import(
    archive: Option<PathBuf>,
    bundle_dir: Option<PathBuf>,
    out_dir: PathBuf,
    json: bool,
) -> Result<()> {
    let input_count = usize::from(archive.is_some()) + usize::from(bundle_dir.is_some());
    if input_count != 1 {
        return Err(anyhow!(
            "control-plane bundle-import requires exactly one of --archive or --bundle-dir"
        ));
    }
    let generated_at_ms = now_unix_ms();
    let (input_kind, input_path, verified_source_dir, temp_verify_dir) = if let Some(archive_path) =
        archive
    {
        let temp_dir =
            std::env::temp_dir().join(format!("ao2-bundle-import-verify-{generated_at_ms}"));
        fs::create_dir_all(&temp_dir).with_context(|| format!("create {}", temp_dir.display()))?;
        extract_tar_gz(&archive_path, &temp_dir)?;
        control_plane_bundle_verify_json(&temp_dir)?;
        ("archive", archive_path, temp_dir.clone(), Some(temp_dir))
    } else {
        let source_dir = bundle_dir.expect("bundle_dir is present after input_count validation");
        control_plane_bundle_verify_json(&source_dir)?;
        ("directory", source_dir.clone(), source_dir, None)
    };

    fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let case_dir = out_dir.join(format!("control-plane-import-{generated_at_ms}"));
    let imported_bundle_dir = case_dir.join("bundle");
    fs::create_dir_all(&imported_bundle_dir)
        .with_context(|| format!("create {}", imported_bundle_dir.display()))?;
    copy_dir_recursive(&verified_source_dir, &imported_bundle_dir)?;
    if let Some(temp_dir) = temp_verify_dir {
        let _ = fs::remove_dir_all(temp_dir);
    }

    let verify = control_plane_bundle_verify_json(&imported_bundle_dir)?;
    let bundle_path = imported_bundle_dir.join("fleet-bundle.json");
    let bundle = read_control_plane_snapshot(&bundle_path)?;
    let health_history_entry_count = json_array(&bundle["health_history"], "entries").len();
    let summary_path = case_dir.join("import-summary.json");
    let index_path = case_dir.join("index.html");
    let summary = serde_json::json!({
        "schema_version": "ao2.control-plane-bundle-import.v1",
        "generated_at_ms": generated_at_ms,
        "verified": true,
        "input_kind": input_kind,
        "input_path": input_path,
        "case_dir": case_dir,
        "bundle_dir": imported_bundle_dir,
        "summary_path": summary_path,
        "index_path": index_path,
        "repository_count": json_u64(&bundle["fleet_snapshot"]["totals"], "repository_count"),
        "run_count": json_u64(&bundle["fleet_snapshot"]["totals"], "run_count"),
        "health_history_entry_count": health_history_entry_count,
        "health_trend": bundle["health_trend"].clone(),
        "support_metadata": verify["support_metadata"].clone(),
        "verify": verify
    });
    atomic_write_text(&summary_path, &serde_json::to_string_pretty(&summary)?)?;
    atomic_write_text(
        &index_path,
        &render_control_plane_bundle_import_html(&summary)?,
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!("verified=true");
        println!("case={}", summary["case_dir"].as_str().unwrap_or(""));
        println!("summary={}", summary["summary_path"].as_str().unwrap_or(""));
        println!("index={}", summary["index_path"].as_str().unwrap_or(""));
    }
    Ok(())
}

fn control_plane_bundle_inspect(
    archive: Option<PathBuf>,
    bundle_dir: Option<PathBuf>,
    json: bool,
) -> Result<()> {
    let input_count = usize::from(archive.is_some()) + usize::from(bundle_dir.is_some());
    if input_count != 1 {
        return Err(anyhow!(
            "control-plane bundle-inspect requires exactly one of --archive or --bundle-dir"
        ));
    }

    let (input_kind, input_path, inspected_bundle_dir) = if let Some(archive_path) = archive {
        let inspect_dir =
            std::env::temp_dir().join(format!("ao2-bundle-inspect-{}", now_unix_ms()));
        fs::create_dir_all(&inspect_dir)
            .with_context(|| format!("create {}", inspect_dir.display()))?;
        extract_tar_gz(&archive_path, &inspect_dir)?;
        ("archive", archive_path, inspect_dir)
    } else {
        let bundle_path = bundle_dir.expect("bundle_dir is present after input_count validation");
        ("directory", bundle_path.clone(), bundle_path)
    };

    let summary = control_plane_bundle_inspect_json(input_kind, input_path, inspected_bundle_dir)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!("verified=true");
        println!(
            "input_kind={}",
            summary["input_kind"].as_str().unwrap_or("")
        );
        println!(
            "repositories={}",
            summary["repository_count"].as_u64().unwrap_or_default()
        );
        println!("runs={}", summary["run_count"].as_u64().unwrap_or_default());
        println!(
            "health_trend={}",
            json_string(&summary["health_trend"], "trend")
        );
        let support_metadata = &summary["support_metadata"];
        println!(
            "support_metadata={}",
            support_metadata_status_text(support_metadata)
        );
        if support_metadata
            .get("present")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            println!("signer_id={}", json_string(support_metadata, "signer_id"));
        }
    }
    Ok(())
}

fn control_plane_bundle_inspect_json(
    input_kind: &str,
    input_path: PathBuf,
    bundle_dir: PathBuf,
) -> Result<serde_json::Value> {
    let verify = control_plane_bundle_verify_json(&bundle_dir)?;
    let bundle_path = bundle_dir.join("fleet-bundle.json");
    let bundle = read_control_plane_snapshot(&bundle_path)?;
    let health_history_entry_count = json_array(&bundle["health_history"], "entries").len();
    Ok(serde_json::json!({
        "schema_version": "ao2.control-plane-bundle-inspect.v1",
        "generated_at_ms": now_unix_ms(),
        "verified": true,
        "input_kind": input_kind,
        "input_path": input_path,
        "bundle_dir": bundle_dir,
        "repository_count": json_u64(&bundle["fleet_snapshot"]["totals"], "repository_count"),
        "run_count": json_u64(&bundle["fleet_snapshot"]["totals"], "run_count"),
        "health_history_entry_count": health_history_entry_count,
        "health_trend": bundle["health_trend"].clone(),
        "support_metadata": verify["support_metadata"].clone(),
        "files": json_array(&verify, "files"),
        "verify": verify
    }))
}

fn render_control_plane_bundle_import_html(summary: &serde_json::Value) -> Result<String> {
    let trend = json_string(&summary["health_trend"], "trend");
    let trend_text = if trend.is_empty() {
        "not recorded".to_string()
    } else {
        trend
    };
    let trust = render_support_metadata_trust_html(&summary["support_metadata"]);
    let files = json_array(&summary["verify"], "files")
        .iter()
        .map(|file| {
            format!(
                "<tr><td><code>{}</code></td><td><code>{}</code></td></tr>",
                escape_html(&json_string(file, "path")),
                escape_html(&json_string(file, "sha256"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Control Plane Support Bundle</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; margin: 32px; color: #172026; background: #f7f8fa; }}
main {{ max-width: 1080px; margin: 0 auto; }}
section {{ background: #fff; border: 1px solid #d9dee5; border-radius: 8px; padding: 20px; margin: 16px 0; }}
.metrics {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; }}
.metric {{ border: 1px solid #e1e6ed; border-radius: 6px; padding: 12px; }}
.label {{ color: #5b6573; font-size: 12px; text-transform: uppercase; }}
.value {{ font-size: 22px; font-weight: 700; margin-top: 4px; }}
table {{ border-collapse: collapse; width: 100%; }}
td, th {{ border-top: 1px solid #e1e6ed; padding: 8px; text-align: left; }}
code {{ word-break: break-all; }}
</style>
</head>
<body>
<main>
<h1>Control Plane Support Bundle</h1>
<section class="metrics">
  <div class="metric"><div class="label">Verified</div><div class="value">{verified}</div></div>
  <div class="metric"><div class="label">Repositories</div><div class="value">{repositories}</div></div>
  <div class="metric"><div class="label">Runs</div><div class="value">{runs}</div></div>
  <div class="metric"><div class="label">Health Trend</div><div class="value">{trend}</div></div>
</section>
{trust}
<section>
<h2>Imported Evidence</h2>
<p>Health entries: <strong>{health_entries}</strong>. Health dashboard: <code>bundle/health-trend.html</code>.</p>
<p>Bundle directory: <code>{bundle_dir}</code></p>
</section>
<section>
<h2>Verified Files</h2>
<table><thead><tr><th>Path</th><th>SHA256</th></tr></thead><tbody>
{files}
</tbody></table>
</section>
</main>
</body>
</html>
"#,
        verified = escape_html(&json_string(summary, "verified")),
        repositories = json_u64(summary, "repository_count"),
        runs = json_u64(summary, "run_count"),
        trend = escape_html(&trend_text),
        health_entries = json_u64(summary, "health_history_entry_count"),
        bundle_dir = escape_html(&json_string(summary, "bundle_dir")),
        trust = trust,
        files = files
    ))
}

fn support_metadata_status_text(support_metadata: &serde_json::Value) -> &'static str {
    if !support_metadata
        .get("present")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return "unsigned";
    }
    if support_metadata
        .get("signature_verified")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        "signature_verified"
    } else {
        "signature_unverified"
    }
}

fn render_support_metadata_trust_html(support_metadata: &serde_json::Value) -> String {
    let present = support_metadata
        .get("present")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !present {
        return r#"<section>
<h2>Support Bundle Trust</h2>
<p>Status: <strong>Unsigned</strong></p>
</section>"#
            .to_string();
    }

    let status = if support_metadata
        .get("signature_verified")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        "Signature verified"
    } else {
        "Signature not verified"
    };
    format!(
        r#"<section>
<h2>Support Bundle Trust</h2>
<div class="metrics">
  <div class="metric"><div class="label">Status</div><div class="value">{status}</div></div>
  <div class="metric"><div class="label">Signer</div><div class="value">{signer_id}</div></div>
</div>
<p>Metadata SHA256: <code>{metadata_sha256}</code></p>
<p>Public key SHA256: <code>{public_key_sha256}</code></p>
</section>"#,
        status = escape_html(status),
        signer_id = escape_html(&json_string(support_metadata, "signer_id")),
        metadata_sha256 = escape_html(&json_string(support_metadata, "metadata_sha256")),
        public_key_sha256 = escape_html(&json_string(support_metadata, "public_key_sha256"))
    )
}

fn control_plane_export(
    target: PathBuf,
    snapshot: Option<PathBuf>,
    fleet: Option<PathBuf>,
    health_history: Option<PathBuf>,
    out: Option<PathBuf>,
    open: bool,
) -> Result<()> {
    let html = if let Some(fleet_path) = fleet {
        let fleet_json = read_control_plane_snapshot(&fleet_path)?;
        render_control_plane_document(&fleet_json, None, health_history.as_deref())?
    } else {
        let snapshot_path = snapshot.unwrap_or_else(|| control_plane_snapshot_path(&target));
        let snapshot_json = read_control_plane_snapshot(&snapshot_path)?;
        render_control_plane_document(&snapshot_json, None, None)?
    };
    let path = out.unwrap_or_else(|| target.join(".ao2").join("control-plane").join("index.html"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&path, html).with_context(|| format!("write {}", path.display()))?;
    println!("control_plane={}", path.display());
    if open {
        open_report_target(&path)?;
        println!("open_target={}", path.display());
    }
    Ok(())
}

struct ControlPlaneServeOptions {
    target: PathBuf,
    snapshot: Option<PathBuf>,
    fleet: Option<PathBuf>,
    health_history: Option<PathBuf>,
    host: String,
    port: u16,
    once: bool,
    api_token: Option<String>,
}

fn control_plane_serve(options: ControlPlaneServeOptions) -> Result<()> {
    let ControlPlaneServeOptions {
        target,
        snapshot,
        fleet,
        health_history,
        host,
        port,
        once,
        api_token,
    } = options;
    let snapshot_path = match fleet {
        Some(path) => path,
        None => snapshot.unwrap_or_else(|| control_plane_snapshot_path(&target)),
    };
    let api_token = api_token.unwrap_or_else(generate_api_token);
    let listener = TcpListener::bind((host.as_str(), port))
        .with_context(|| format!("bind control plane server on {host}:{port}"))?;
    let address = listener
        .local_addr()
        .context("read control plane server address")?;
    println!("url=http://{}:{}/", address.ip(), address.port());
    eprintln!("api_token_redacted=true");
    std::io::stdout()
        .flush()
        .context("flush control plane server url")?;

    for stream in listener.incoming() {
        let mut stream = stream.context("accept control plane connection")?;
        let mut request_buffer = [0_u8; 8192];
        let bytes_read = stream.read(&mut request_buffer).unwrap_or(0);
        let request = String::from_utf8_lossy(&request_buffer[..bytes_read]).to_string();
        let response = handle_control_plane_request(
            &request,
            &snapshot_path,
            health_history.as_deref(),
            &api_token,
        )?;
        stream
            .write_all(response.as_bytes())
            .context("write control plane response")?;
        if once {
            break;
        }
    }
    Ok(())
}

fn handle_control_plane_request(
    request: &str,
    snapshot_path: &Path,
    health_history: Option<&Path>,
    api_token: &str,
) -> Result<String> {
    let Some(request_line) = request.lines().next() else {
        return Ok(http_text_response(400, "Bad Request", "empty request"));
    };
    let (method, raw_path) = parse_http_request_line(request_line);
    let (path, query) = split_path_query(raw_path);

    if query_value_owned(query, "token").as_deref() != Some(api_token) {
        return http_json_response(
            403,
            serde_json::json!({
                "schema_version": "ao2.control-plane-error.v1",
                "error": "invalid_api_token"
            }),
        );
    }

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            let snapshot = read_control_plane_snapshot(snapshot_path)?;
            Ok(http_html_response(render_control_plane_document(
                &snapshot,
                Some(api_token),
                health_history,
            )?))
        }
        ("GET", "/api/control-plane/snapshot") => {
            http_json_response(200, read_control_plane_snapshot(snapshot_path)?)
        }
        ("GET", "/api/control-plane/health") => {
            let snapshot = read_control_plane_snapshot(snapshot_path)?;
            http_json_response(
                200,
                control_plane_health_json(snapshot_path, &snapshot, None)?,
            )
        }
        ("GET", "/api/control-plane/health-trend") => {
            let Some(health_history) = health_history else {
                return http_json_response(
                    404,
                    serde_json::json!({
                        "schema_version": "ao2.control-plane-error.v1",
                        "error": "health_history_not_configured"
                    }),
                );
            };
            http_json_response(200, control_plane_health_trend_json(health_history)?)
        }
        _ => Ok(http_text_response(404, "Not Found", "not found")),
    }
}

pub(crate) fn control_plane_snapshot_path(target: &Path) -> PathBuf {
    target
        .join(".ao2")
        .join("control-plane")
        .join("snapshot.json")
}

fn read_control_plane_snapshot(path: &Path) -> Result<serde_json::Value> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))
}

pub(crate) fn control_plane_snapshot_json(
    target: &Path,
    snapshot_path: &Path,
) -> Result<serde_json::Value> {
    let runs = runs_list_json(target)?;
    let queue = read_workbench_queue_file(target)?;
    let audit_events = read_workbench_audit_events(&workbench_audit_path_for_target(target))?;
    let provider_smoke_history = read_provider_smoke_history(target)?;
    let evidence_packs = json_array(&runs, "runs")
        .iter()
        .filter_map(|run| run.get("evidence_pack").and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "schema_version": "ao2.control-plane-snapshot.v1",
        "generated_at_ms": now_unix_ms(),
        "target": target,
        "snapshot_path": snapshot_path,
        "runs": runs,
        "queue": queue,
        "audit_events": audit_events,
        "provider_smoke_history": provider_smoke_history,
        "evidence_packs": evidence_packs
    }))
}

fn control_plane_repository_index_json(
    target: Option<PathBuf>,
    snapshot_path: PathBuf,
    snapshot: serde_json::Value,
) -> serde_json::Value {
    let target = target
        .or_else(|| {
            snapshot
                .get("target")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
        })
        .unwrap_or_default();
    serde_json::json!({
        "target": target,
        "snapshot_path": snapshot_path,
        "run_count": json_array(&snapshot["runs"], "runs").len(),
        "queue_job_count": json_array(&snapshot["queue"], "jobs").len(),
        "audit_event_count": json_array(&snapshot, "audit_events").len(),
        "evidence_pack_count": json_array(&snapshot, "evidence_packs").len(),
        "snapshot": snapshot
    })
}

fn control_plane_fleet_totals(repositories: &[serde_json::Value]) -> serde_json::Value {
    let run_count = repositories
        .iter()
        .map(|repo| json_u64(repo, "run_count"))
        .sum::<u64>();
    let queue_job_count = repositories
        .iter()
        .map(|repo| json_u64(repo, "queue_job_count"))
        .sum::<u64>();
    let audit_event_count = repositories
        .iter()
        .map(|repo| json_u64(repo, "audit_event_count"))
        .sum::<u64>();
    let evidence_pack_count = repositories
        .iter()
        .map(|repo| json_u64(repo, "evidence_pack_count"))
        .sum::<u64>();
    serde_json::json!({
        "repository_count": repositories.len(),
        "run_count": run_count,
        "queue_job_count": queue_job_count,
        "audit_event_count": audit_event_count,
        "evidence_pack_count": evidence_pack_count
    })
}

fn render_control_plane_document(
    snapshot: &serde_json::Value,
    api_token: Option<&str>,
    health_history: Option<&Path>,
) -> Result<String> {
    if json_string(snapshot, "schema_version") == "ao2.control-plane-fleet-snapshot.v1" {
        render_control_plane_fleet_dashboard(snapshot, api_token, health_history)
    } else {
        render_control_plane_dashboard(snapshot, api_token)
    }
}

fn render_control_plane_dashboard(
    snapshot: &serde_json::Value,
    api_token: Option<&str>,
) -> Result<String> {
    let runs = json_array(&snapshot["runs"], "runs");
    let queue_jobs = json_array(&snapshot["queue"], "jobs");
    let audit_events = json_array(snapshot, "audit_events");
    let evidence_packs = json_array(snapshot, "evidence_packs");
    let mut html = String::new();
    write!(
        html,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AO2 Control Plane</title>
<style>
:root {{
  color-scheme: light;
  --bg: #f5f7f9;
  --panel: #ffffff;
  --ink: #17202a;
  --muted: #5f6b7a;
  --line: #d9dee7;
  --accent: #176b87;
  --ok: #16794c;
  --warn: #9b5a00;
}}
* {{ box-sizing: border-box; }}
body {{ margin: 0; background: var(--bg); color: var(--ink); font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
main {{ max-width: 1220px; margin: 0 auto; padding: 28px; }}
h1 {{ margin: 0 0 6px; font-size: 30px; letter-spacing: 0; }}
h2 {{ margin: 0 0 14px; font-size: 18px; letter-spacing: 0; }}
p {{ margin: 0; }}
.muted {{ color: var(--muted); }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; margin: 22px 0; }}
.metric, section {{ background: var(--panel); border: 1px solid var(--line); border-radius: 8px; }}
.metric {{ padding: 14px; min-height: 86px; }}
.label {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
.value {{ margin-top: 6px; font-size: 18px; font-weight: 700; overflow-wrap: anywhere; }}
section {{ margin: 16px 0; padding: 18px; }}
table {{ width: 100%; border-collapse: collapse; }}
th, td {{ padding: 9px 8px; border-top: 1px solid var(--line); text-align: left; vertical-align: top; }}
th {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
code {{ background: #eef2f6; border-radius: 4px; padding: 2px 4px; overflow-wrap: anywhere; }}
a {{ color: var(--accent); text-decoration: none; }}
.ok {{ color: var(--ok); }}
.warn {{ color: var(--warn); }}
</style>
</head>
<body>
<main data-api-token="{api_token}">
<h1>AO2 Control Plane</h1>
<p class="muted">Read-only local fleet view derived from an AO2 control-plane snapshot.</p>
<div class="grid">
  <div class="metric"><div class="label">Snapshot</div><div class="value">{schema}</div></div>
  <div class="metric"><div class="label">Target</div><div class="value">{target}</div></div>
  <div class="metric"><div class="label">Runs</div><div class="value">{run_count}</div></div>
  <div class="metric"><div class="label">Queue Jobs</div><div class="value">{queue_count}</div></div>
  <div class="metric"><div class="label">Audit Events</div><div class="value">{audit_count}</div></div>
  <div class="metric"><div class="label">Evidence Packs</div><div class="value">{evidence_count}</div></div>
</div>
"#,
        api_token = escape_html(api_token.unwrap_or("")),
        schema = escape_html(&json_string(snapshot, "schema_version")),
        target = escape_html(&json_string(snapshot, "target")),
        run_count = runs.len(),
        queue_count = queue_jobs.len(),
        audit_count = audit_events.len(),
        evidence_count = evidence_packs.len()
    )?;

    html.push_str("<section>\n<h2>Runs</h2>\n<table><thead><tr><th>Run</th><th>Status</th><th>Workflow</th><th>Digest Failures</th><th>Evidence</th></tr></thead><tbody>\n");
    if runs.is_empty() {
        html.push_str(
            "<tr><td colspan=\"5\" class=\"muted\">No runs indexed in this snapshot.</td></tr>\n",
        );
    }
    for run in runs {
        writeln!(
            html,
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape_html(&json_string(run, "run_id")),
            escape_html(&json_string(run, "status")),
            escape_html(&json_string(run, "workflow_id")),
            run.get("digest_failures")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            escape_html(&json_string(run, "evidence_pack"))
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");

    html.push_str("<section>\n<h2>Queue Jobs</h2>\n<table><thead><tr><th>Job</th><th>Run</th><th>Status</th><th>Template</th><th>Duration</th></tr></thead><tbody>\n");
    if queue_jobs.is_empty() {
        html.push_str("<tr><td colspan=\"5\" class=\"muted\">No queue jobs indexed in this snapshot.</td></tr>\n");
    }
    for job in queue_jobs {
        writeln!(
            html,
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{} ms</td></tr>",
            escape_html(&json_string(job, "job_id")),
            escape_html(&json_string(job, "run_id")),
            escape_html(&json_string(job, "status")),
            escape_html(&json_string(job, "template")),
            job.get("duration_ms")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");
    html.push_str("</main>\n</body>\n</html>\n");
    Ok(html)
}

fn render_control_plane_fleet_dashboard(
    fleet: &serde_json::Value,
    api_token: Option<&str>,
    health_history: Option<&Path>,
) -> Result<String> {
    let repositories = json_array(fleet, "repositories");
    let totals = &fleet["totals"];
    let health = control_plane_health_json(Path::new("fleet-snapshot.json"), fleet, None)?;
    let health_trend = match health_history {
        Some(path) => Some(control_plane_health_trend_json(path)?),
        None => None,
    };
    let mut html = String::new();
    write!(
        html,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AO2 Control Plane</title>
<style>
:root {{
  color-scheme: light;
  --bg: #f5f7f9;
  --panel: #ffffff;
  --ink: #17202a;
  --muted: #5f6b7a;
  --line: #d9dee7;
  --accent: #176b87;
}}
* {{ box-sizing: border-box; }}
body {{ margin: 0; background: var(--bg); color: var(--ink); font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
main {{ max-width: 1220px; margin: 0 auto; padding: 28px; }}
h1 {{ margin: 0 0 6px; font-size: 30px; letter-spacing: 0; }}
h2 {{ margin: 0 0 14px; font-size: 18px; letter-spacing: 0; }}
p {{ margin: 0; }}
.muted {{ color: var(--muted); }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; margin: 22px 0; }}
.metric, section {{ background: var(--panel); border: 1px solid var(--line); border-radius: 8px; }}
.metric {{ padding: 14px; min-height: 86px; }}
.label {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
.value {{ margin-top: 6px; font-size: 18px; font-weight: 700; overflow-wrap: anywhere; }}
section {{ margin: 16px 0; padding: 18px; }}
table {{ width: 100%; border-collapse: collapse; }}
th, td {{ padding: 9px 8px; border-top: 1px solid var(--line); text-align: left; vertical-align: top; }}
th {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
code {{ background: #eef2f6; border-radius: 4px; padding: 2px 4px; overflow-wrap: anywhere; }}
a {{ color: var(--accent); text-decoration: none; }}
.ok {{ color: #16794c; }}
.warn {{ color: #9b5a00; }}
.filters {{ display: flex; flex-wrap: wrap; gap: 10px; margin: 18px 0; align-items: end; }}
.filters label {{ display: grid; gap: 4px; color: var(--muted); font-size: 12px; text-transform: uppercase; }}
.filters input, .filters select {{ min-width: 220px; border: 1px solid var(--line); border-radius: 6px; padding: 8px; background: #fff; color: var(--ink); font: inherit; text-transform: none; }}
.filters select {{ min-width: 150px; }}
</style>
</head>
<body>
<main data-api-token="{api_token}">
<h1>AO2 Control Plane</h1>
<p class="muted">Read-only local fleet view derived from AO2 control-plane snapshots.</p>
<div class="grid">
  <div class="metric"><div class="label">Repositories</div><div class="value">{repository_count}</div></div>
  <div class="metric"><div class="label">Total Runs</div><div class="value">{run_count}</div></div>
  <div class="metric"><div class="label">Queue Jobs</div><div class="value">{queue_job_count}</div></div>
  <div class="metric"><div class="label">Audit Events</div><div class="value">{audit_event_count}</div></div>
  <div class="metric"><div class="label">Evidence Packs</div><div class="value">{evidence_pack_count}</div></div>
</div>
<div class="filters">
  <label for="fleet-search">Search<input id="fleet-search" type="search" placeholder="Repository, run, workflow, evidence"></label>
  <label for="fleet-status-filter">Status<select id="fleet-status-filter"><option value="">All</option><option value="accepted">Accepted</option><option value="rejected">Rejected</option><option value="failed">Failed</option><option value="running">Running</option><option value="canceled">Canceled</option></select></label>
</div>
"#,
        api_token = escape_html(api_token.unwrap_or("")),
        repository_count = json_u64(totals, "repository_count"),
        run_count = json_u64(totals, "run_count"),
        queue_job_count = json_u64(totals, "queue_job_count"),
        audit_event_count = json_u64(totals, "audit_event_count"),
        evidence_pack_count = json_u64(totals, "evidence_pack_count")
    )?;

    html.push_str("<section>\n<h2>Fleet Health</h2>\n");
    writeln!(
        html,
        "<div class=\"grid\"><div class=\"metric\"><div class=\"label\">Status</div><div class=\"value {}\">{}</div></div><div class=\"metric\"><div class=\"label\">Alerts</div><div class=\"value\">{}</div></div></div>",
        if json_string(&health, "status") == "ok" {
            "ok"
        } else {
            "warn"
        },
        escape_html(&json_string(&health, "status")),
        json_u64(&health, "alert_count")
    )?;
    html.push_str("<table><thead><tr><th>Severity</th><th>Code</th><th>Repository</th><th>Run</th><th>Job</th><th>Message</th></tr></thead><tbody>\n");
    let alerts = json_array(&health, "alerts");
    if alerts.is_empty() {
        html.push_str("<tr><td colspan=\"6\" class=\"muted\">No fleet health alerts.</td></tr>\n");
    }
    for alert in alerts {
        writeln!(
            html,
            "<tr><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape_html(&json_string(alert, "severity")),
            escape_html(&json_string(alert, "code")),
            escape_html(&json_string(alert, "repository")),
            escape_html(&json_string(alert, "run_id")),
            escape_html(&json_string(alert, "job_id")),
            escape_html(&json_string(alert, "message"))
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");

    let provider_readiness = &health["provider_readiness"];
    html.push_str("<section>\n<h2>Provider Readiness</h2>\n");
    writeln!(
        html,
        "<div class=\"grid\"><div class=\"metric\"><div class=\"label\">Ready Repositories</div><div class=\"value ok\">{}</div></div><div class=\"metric\"><div class=\"label\">Not Ready</div><div class=\"value warn\">{}</div></div><div class=\"metric\"><div class=\"label\">Missing History</div><div class=\"value warn\">{}</div></div></div>",
        json_u64(provider_readiness, "ready_repository_count"),
        json_u64(provider_readiness, "not_ready_repository_count"),
        json_u64(provider_readiness, "missing_history_count")
    )?;
    html.push_str("<table><thead><tr><th>Provider</th><th>Ready</th><th>Warn</th><th>Fail</th><th>Unavailable</th></tr></thead><tbody>\n");
    if let Some(providers) = provider_readiness
        .get("providers")
        .and_then(serde_json::Value::as_object)
    {
        if providers.is_empty() {
            html.push_str("<tr><td colspan=\"5\" class=\"muted\">No provider smoke evidence found.</td></tr>\n");
        }
        for (provider, counts) in providers {
            writeln!(
                html,
                "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(provider),
                json_u64(counts, "ready_count"),
                json_u64(counts, "warn_count"),
                json_u64(counts, "fail_count"),
                json_u64(counts, "unavailable_count")
            )?;
        }
    }
    html.push_str("</tbody></table>\n</section>\n");

    if let Some(trend) = &health_trend {
        html.push_str("<section>\n<h2>Fleet Health Trend</h2>\n");
        writeln!(
            html,
            "<div class=\"grid\"><div class=\"metric\"><div class=\"label\">Trend</div><div class=\"value\">{}</div></div><div class=\"metric\"><div class=\"label\">Entries</div><div class=\"value\">{}</div></div><div class=\"metric\"><div class=\"label\">Latest Alerts</div><div class=\"value\">{}</div></div><div class=\"metric\"><div class=\"label\">Alert Delta</div><div class=\"value\">{}</div></div><div class=\"metric\"><div class=\"label\">History</div><div class=\"value\"><code>health-history.json</code></div></div></div>",
            escape_html(&json_string(trend, "trend")),
            json_u64(trend, "entry_count"),
            json_u64(trend, "latest_alert_count"),
            trend["alert_count_delta"].as_i64().unwrap_or_default()
        )?;
        html.push_str("</section>\n");
    }

    html.push_str("<section>\n<h2>Repositories</h2>\n<table><thead><tr><th>Target</th><th>Runs</th><th>Queue Jobs</th><th>Audit Events</th><th>Snapshot</th></tr></thead><tbody>\n");
    if repositories.is_empty() {
        html.push_str("<tr><td colspan=\"5\" class=\"muted\">No repositories indexed in this fleet snapshot.</td></tr>\n");
    }
    for repo in repositories {
        let target = json_string(repo, "target");
        let snapshot_path = json_string(repo, "snapshot_path");
        let search_text = format!("{target} {snapshot_path}");
        writeln!(
            html,
            "<tr data-fleet-row data-search=\"{}\"><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            escape_html(&search_text),
            escape_html(&target),
            json_u64(repo, "run_count"),
            json_u64(repo, "queue_job_count"),
            json_u64(repo, "audit_event_count"),
            escape_html(&snapshot_path)
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n");

    html.push_str("<section>\n<h2>Runs</h2>\n<table><thead><tr><th>Repository</th><th>Run</th><th>Status</th><th>Workflow</th><th>Evidence</th></tr></thead><tbody>\n");
    let mut rendered_runs = 0_usize;
    for repo in repositories {
        let target = json_string(repo, "target");
        for run in json_array(&repo["snapshot"]["runs"], "runs") {
            rendered_runs += 1;
            let run_id = json_string(run, "run_id");
            let status = json_string(run, "status");
            let workflow = json_string(run, "workflow_id");
            let evidence = json_string(run, "evidence_pack");
            let search_text = format!("{target} {run_id} {status} {workflow} {evidence}");
            writeln!(
                html,
                "<tr data-fleet-row data-status=\"{}\" data-search=\"{}\"><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&status),
                escape_html(&search_text),
                escape_html(&target),
                escape_html(&run_id),
                escape_html(&status),
                escape_html(&workflow),
                escape_html(&evidence)
            )?;
        }
    }
    if rendered_runs == 0 {
        html.push_str("<tr><td colspan=\"5\" class=\"muted\">No runs indexed in this fleet snapshot.</td></tr>\n");
    }
    html.push_str(
        r#"</tbody></table>
</section>
<script>
(() => {
  const search = document.getElementById('fleet-search');
  const status = document.getElementById('fleet-status-filter');
  const applyFilters = () => {
    const query = (search?.value || '').trim().toLowerCase();
    const selectedStatus = (status?.value || '').toLowerCase();
    document.querySelectorAll('[data-fleet-row]').forEach((row) => {
      const haystack = (row.getAttribute('data-search') || '').toLowerCase();
      const rowStatus = (row.getAttribute('data-status') || '').toLowerCase();
      const searchMatch = !query || haystack.includes(query);
      const statusMatch = !selectedStatus || !rowStatus || rowStatus === selectedStatus;
      row.hidden = !(searchMatch && statusMatch);
    });
  };
  search?.addEventListener('input', applyFilters);
  status?.addEventListener('change', applyFilters);
})();
</script>
</main>
</body>
</html>
"#,
    );
    Ok(html)
}

fn render_control_plane_history_dashboard(history: &serde_json::Value) -> Result<String> {
    let entries = json_array(history, "entries");
    let mut html = String::new();
    write!(
        html,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AO2 Fleet History</title>
<style>
:root {{
  color-scheme: light;
  --bg: #f5f7f9;
  --panel: #ffffff;
  --ink: #17202a;
  --muted: #5f6b7a;
  --line: #d9dee7;
  --accent: #176b87;
}}
* {{ box-sizing: border-box; }}
body {{ margin: 0; background: var(--bg); color: var(--ink); font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
main {{ max-width: 1220px; margin: 0 auto; padding: 28px; }}
h1 {{ margin: 0 0 6px; font-size: 30px; letter-spacing: 0; }}
h2 {{ margin: 0 0 14px; font-size: 18px; letter-spacing: 0; }}
p {{ margin: 0; }}
.muted {{ color: var(--muted); }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; margin: 22px 0; }}
.metric, section {{ background: var(--panel); border: 1px solid var(--line); border-radius: 8px; }}
.metric {{ padding: 14px; min-height: 86px; }}
.label {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
.value {{ margin-top: 6px; font-size: 18px; font-weight: 700; overflow-wrap: anywhere; }}
section {{ margin: 16px 0; padding: 18px; }}
table {{ width: 100%; border-collapse: collapse; }}
th, td {{ padding: 9px 8px; border-top: 1px solid var(--line); text-align: left; vertical-align: top; }}
th {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
code {{ background: #eef2f6; border-radius: 4px; padding: 2px 4px; overflow-wrap: anywhere; }}
a {{ color: var(--accent); text-decoration: none; }}
</style>
</head>
<body>
<main>
<h1>AO2 Fleet History</h1>
<p class="muted">Read-only local timeline of retained control-plane fleet snapshots.</p>
<div class="grid">
  <div class="metric"><div class="label">Schema</div><div class="value">{schema}</div></div>
  <div class="metric"><div class="label">History Entries</div><div class="value">{entry_count}</div></div>
  <div class="metric"><div class="label">Updated</div><div class="value">{generated_at_ms}</div></div>
</div>
"#,
        schema = escape_html(&json_string(history, "schema_version")),
        entry_count = entries.len(),
        generated_at_ms = json_u64(history, "generated_at_ms")
    )?;

    html.push_str("<section>\n<h2>History Entries</h2>\n<table><thead><tr><th>Index</th><th>Generated</th><th>Repositories</th><th>Runs</th><th>Queue Jobs</th><th>SHA256</th><th>Snapshot</th><th>Run IDs</th></tr></thead><tbody>\n");
    if entries.is_empty() {
        html.push_str(
            "<tr><td colspan=\"8\" class=\"muted\">No fleet history entries recorded.</td></tr>\n",
        );
    }
    for (index, entry) in entries.iter().enumerate() {
        let snapshot_path = json_string(entry, "fleet_snapshot_path");
        let run_ids = if snapshot_path.is_empty() {
            Vec::new()
        } else {
            read_control_plane_snapshot(Path::new(&snapshot_path))
                .map(|snapshot| control_plane_fleet_run_ids(&snapshot).into_iter().collect())
                .unwrap_or_default()
        };
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr>",
            index,
            json_u64(entry, "generated_at_ms"),
            json_u64(entry, "repository_count"),
            json_u64(entry, "run_count"),
            json_u64(entry, "queue_job_count"),
            escape_html(&json_string(entry, "fleet_snapshot_sha256")),
            escape_html(&snapshot_path),
            escape_html(&run_ids.join(", "))
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n</main>\n</body>\n</html>\n");
    Ok(html)
}

fn render_control_plane_health_trend_dashboard(
    history: &serde_json::Value,
    trend: &serde_json::Value,
) -> Result<String> {
    let entries = json_array(history, "entries");
    let mut html = String::new();
    write!(
        html,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AO2 Fleet Health Trend</title>
<style>
:root {{
  color-scheme: light;
  --bg: #f5f7f9;
  --panel: #ffffff;
  --ink: #17202a;
  --muted: #5f6b7a;
  --line: #d9dee7;
  --accent: #176b87;
}}
* {{ box-sizing: border-box; }}
body {{ margin: 0; background: var(--bg); color: var(--ink); font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
main {{ max-width: 1220px; margin: 0 auto; padding: 28px; }}
h1 {{ margin: 0 0 6px; font-size: 30px; letter-spacing: 0; }}
h2 {{ margin: 0 0 14px; font-size: 18px; letter-spacing: 0; }}
p {{ margin: 0; }}
.muted {{ color: var(--muted); }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; margin: 22px 0; }}
.metric, section {{ background: var(--panel); border: 1px solid var(--line); border-radius: 8px; }}
.metric {{ padding: 14px; min-height: 86px; }}
.label {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
.value {{ margin-top: 6px; font-size: 18px; font-weight: 700; overflow-wrap: anywhere; }}
section {{ margin: 16px 0; padding: 18px; }}
table {{ width: 100%; border-collapse: collapse; }}
th, td {{ padding: 9px 8px; border-top: 1px solid var(--line); text-align: left; vertical-align: top; }}
th {{ color: var(--muted); font-size: 12px; text-transform: uppercase; }}
code {{ background: #eef2f6; border-radius: 4px; padding: 2px 4px; overflow-wrap: anywhere; }}
a {{ color: var(--accent); text-decoration: none; }}
</style>
</head>
<body>
<main>
<h1>AO2 Fleet Health Trend</h1>
<p class="muted">Read-only local trend view derived from retained control-plane health checks in health-history.json.</p>
<div class="grid">
  <div class="metric"><div class="label">Trend</div><div class="value">{trend_label}</div></div>
  <div class="metric"><div class="label">Entries</div><div class="value">{entry_count}</div></div>
  <div class="metric"><div class="label">Latest Alerts</div><div class="value">{latest_alerts}</div></div>
  <div class="metric"><div class="label">Alert Delta</div><div class="value">{alert_delta}</div></div>
  <div class="metric"><div class="label">History</div><div class="value"><code>health-history.json</code></div></div>
</div>
"#,
        trend_label = escape_html(&json_string(trend, "trend")),
        entry_count = entries.len(),
        latest_alerts = json_u64(trend, "latest_alert_count"),
        alert_delta = trend["alert_count_delta"].as_i64().unwrap_or_default()
    )?;

    html.push_str("<section>\n<h2>Health Entries</h2>\n<table><thead><tr><th>Index</th><th>Generated</th><th>Status</th><th>Alerts</th><th>Repositories</th><th>Runs</th><th>Queue Jobs</th><th>SHA256</th><th>Health File</th></tr></thead><tbody>\n");
    if entries.is_empty() {
        html.push_str(
            "<tr><td colspan=\"9\" class=\"muted\">No fleet health entries recorded.</td></tr>\n",
        );
    }
    for (index, entry) in entries.iter().enumerate() {
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td><td><code>{}</code></td></tr>",
            index,
            json_u64(entry, "generated_at_ms"),
            escape_html(&json_string(entry, "status")),
            json_u64(entry, "alert_count"),
            json_u64(entry, "repository_count"),
            json_u64(entry, "run_count"),
            json_u64(entry, "queue_job_count"),
            escape_html(&json_string(entry, "health_sha256")),
            escape_html(&json_string(entry, "health_path"))
        )?;
    }
    html.push_str("</tbody></table>\n</section>\n</main>\n</body>\n</html>\n");
    Ok(html)
}
