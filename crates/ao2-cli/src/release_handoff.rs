use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command as ProcessCommand;

use anyhow::{anyhow, Context, Result};

// ----------------------------------------------------------------------------
// factory-v3/ao2-release-handoff-checklist/v1 — AO2-native producer
// ----------------------------------------------------------------------------
//
// Phase 2 W2 P1: Rust translation of
// factory-v3/scripts/ao2_release_handoff_checklist.py. Byte-equal output
// (under canonical sort+separators) so factory-v3 Python remains in a
// read-only audit role.

const HANDOFF_CHECKLIST_SCHEMA: &str = "factory-v3/ao2-release-handoff-checklist/v1";
const HANDOFF_SOURCE_SCHEMA: &str = "ao2.cp-release-candidate-handoff.v1";

fn handoff_extract(payload: &serde_json::Value) -> Option<&serde_json::Value> {
    if payload.get("schema_version").and_then(|v| v.as_str()) == Some(HANDOFF_SOURCE_SCHEMA) {
        return Some(payload);
    }
    if let Some(snapshot) = payload.get("handoff_snapshot") {
        if snapshot.is_object()
            && snapshot.get("schema_version").and_then(|v| v.as_str())
                == Some(HANDOFF_SOURCE_SCHEMA)
        {
            return Some(snapshot);
        }
    }
    None
}

fn handoff_nested_str(value: &serde_json::Value, keys: &[&str]) -> String {
    let mut current = value;
    for key in keys {
        match current.get(*key) {
            Some(next) => current = next,
            None => return "missing".to_string(),
        }
    }
    match current.as_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "missing".to_string(),
    }
}

fn handoff_nested_bool(value: &serde_json::Value, keys: &[&str], default: bool) -> bool {
    let mut current = value;
    for key in keys {
        match current.get(*key) {
            Some(next) => current = next,
            None => return default,
        }
    }
    current.as_bool().unwrap_or(default)
}

fn handoff_check(check_id: &str, label: &str, observed: &str, expected: &str) -> serde_json::Value {
    let status = if observed == expected {
        "passed"
    } else {
        "blocked"
    };
    serde_json::json!({
        "id": check_id,
        "label": label,
        "observed": observed,
        "expected": expected,
        "status": status,
    })
}

fn handoff_provider_acceptance_state(handoff: &serde_json::Value, provider: &str) -> String {
    let status = handoff_nested_str(handoff, &["acceptance", provider, "status"]);
    let source = handoff_nested_str(handoff, &["acceptance", provider, "source_class"]);
    if status == "passed" && source == "live" {
        "passed/live".to_string()
    } else {
        format!("{status}/{source}")
    }
}

fn handoff_parse_expected_repo_heads(values: &[String]) -> Result<BTreeMap<String, String>> {
    let mut expected: BTreeMap<String, String> = BTreeMap::new();
    for value in values {
        let (name, head) = value
            .split_once('=')
            .ok_or_else(|| anyhow!("--expected-repo-head must be formatted as <repo>=<head>"))?;
        if name.is_empty() || head.is_empty() {
            return Err(anyhow!(
                "--expected-repo-head must be formatted as <repo>=<head>"
            ));
        }
        if name.chars().any(char::is_whitespace) || head.chars().any(char::is_whitespace) {
            return Err(anyhow!("--expected-repo-head cannot contain whitespace"));
        }
        expected.insert(name.to_string(), head.to_string());
    }
    Ok(expected)
}

fn handoff_release_publication_metadata_refresh(
    handoff: &serde_json::Value,
    repo: &str,
    observed: &str,
    expected: &str,
) -> Option<(String, Vec<String>, String)> {
    // Returns (status_override, metadata_refresh_paths, reason) when an
    // ao2 HEAD drift is solely a release-candidate metadata refresh.
    if repo != "ao2" || observed == expected {
        return None;
    }
    let root = handoff_nested_str(handoff, &["release", "repositories", repo, "path"]);
    if root == "missing" {
        return None;
    }
    let merge_base_ok = ProcessCommand::new("git")
        .args([
            "-C",
            &root,
            "merge-base",
            "--is-ancestor",
            observed,
            expected,
        ])
        .output()
        .ok()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !merge_base_ok {
        return None;
    }
    let diff = ProcessCommand::new("git")
        .args([
            "-C",
            &root,
            "diff",
            "--name-only",
            &format!("{observed}..{expected}"),
        ])
        .output();
    let diff = match diff {
        Ok(o) if o.status.success() => o,
        _ => return None,
    };
    let text = String::from_utf8_lossy(&diff.stdout).to_string();
    let changed_paths: Vec<String> = text
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if changed_paths.is_empty() {
        return None;
    }
    let allowed_prefixes = ["docs/status/release-candidates/"];
    if changed_paths
        .iter()
        .any(|p| !allowed_prefixes.iter().any(|prefix| p.starts_with(prefix)))
    {
        return None;
    }
    Some((
        "passed_with_metadata_refresh".to_string(),
        changed_paths,
        "ao2 HEAD advanced only by release-candidate metadata refresh files".to_string(),
    ))
}

fn handoff_repo_head_checks(
    handoff: &serde_json::Value,
    expected_repo_heads: &BTreeMap<String, String>,
) -> Vec<serde_json::Value> {
    let mut checks: Vec<serde_json::Value> = Vec::new();
    for (repo, expected_head) in expected_repo_heads.iter() {
        let observed = handoff_nested_str(handoff, &["release", "repositories", repo, "head"]);
        let mut item = handoff_check(
            &format!("repo_head_{repo}"),
            &format!("Repo head {repo}"),
            &observed,
            expected_head,
        );
        if let Some((status, paths, reason)) =
            handoff_release_publication_metadata_refresh(handoff, repo, &observed, expected_head)
        {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("status".to_string(), serde_json::Value::String(status));
                obj.insert(
                    "metadata_refresh_paths".to_string(),
                    serde_json::Value::Array(
                        paths.into_iter().map(serde_json::Value::String).collect(),
                    ),
                );
                obj.insert(
                    "metadata_refresh_reason".to_string(),
                    serde_json::Value::String(reason),
                );
            }
        }
        checks.push(item);
    }
    checks
}

fn handoff_build_checklist(
    handoff: &serde_json::Value,
    expected_repo_heads: &BTreeMap<String, String>,
) -> serde_json::Value {
    let mutates = handoff_nested_bool(handoff, &["operator_handoff", "mutates_ao_artifacts"], true);
    let control_plane_role =
        handoff_nested_str(handoff, &["operator_handoff", "control_plane_role"]);
    let release_acceptance_owner =
        handoff_nested_str(handoff, &["operator_handoff", "release_acceptance_owner"]);
    let trust_state = if control_plane_role == "read_only_observer"
        && !mutates
        && release_acceptance_owner == "factory-v3 evaluator-closer"
    {
        "read_only_evaluator_owned"
    } else {
        "attention"
    };
    let mut checks: Vec<serde_json::Value> = vec![
        handoff_check(
            "handoff_schema",
            "Handoff schema",
            &handoff_nested_str(handoff, &["schema_version"]),
            HANDOFF_SOURCE_SCHEMA,
        ),
        handoff_check(
            "handoff_status",
            "Handoff status",
            &handoff_nested_str(handoff, &["status"]),
            "ready",
        ),
        handoff_check(
            "release_cockpit",
            "Release cockpit",
            &handoff_nested_str(handoff, &["gates", "release_cockpit"]),
            "ready",
        ),
        handoff_check(
            "phase1_promotion",
            "Phase 1 promotion",
            &handoff_nested_str(handoff, &["gates", "phase1_promotion"]),
            "observed",
        ),
        handoff_check(
            "decision_signature",
            "Decision signature",
            &handoff_nested_str(handoff, &["gates", "decision_signature"]),
            "present",
        ),
        handoff_check(
            "provider_acceptance",
            "Provider acceptance",
            &handoff_nested_str(handoff, &["gates", "provider_acceptance"]),
            "live_complete",
        ),
        handoff_check(
            "codex_acceptance",
            "Codex acceptance",
            &handoff_provider_acceptance_state(handoff, "codex"),
            "passed/live",
        ),
        handoff_check(
            "claude_acceptance",
            "Claude acceptance",
            &handoff_provider_acceptance_state(handoff, "claude"),
            "passed/live",
        ),
        handoff_check(
            "trust_boundary",
            "Trust boundary",
            trust_state,
            "read_only_evaluator_owned",
        ),
    ];
    checks.extend(handoff_repo_head_checks(handoff, expected_repo_heads));

    let blockers: Vec<String> = checks
        .iter()
        .filter(|item| {
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            !status.starts_with("passed")
        })
        .map(|item| {
            format!(
                "{}: expected {}, observed {}",
                item.get("id").and_then(|v| v.as_str()).unwrap_or_default(),
                item.get("expected")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
                item.get("observed")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            )
        })
        .collect();

    let status = if blockers.is_empty() {
        "ready_for_evaluator_closer"
    } else {
        "blocked"
    };
    let release = handoff
        .get("release")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let links = handoff
        .get("links")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let next_action = if blockers.is_empty() {
        "factory-v3 evaluator-closer may review and accept or reject the release-line decision"
    } else {
        "resolve blockers before evaluator-closer release-line review"
    };
    serde_json::json!({
        "schema": HANDOFF_CHECKLIST_SCHEMA,
        "status": status,
        "release": release,
        "checks": checks,
        "blockers": blockers,
        "operator_decision": {
            "factory_v3_evaluator_closer_required": true,
            "control_plane_approves_release": false,
            "next_action": next_action,
        },
        "trust_boundary": {
            "frontend": "Hermes front end / queue / memory surface",
            "governed_backend": "factory-v3 / AO Operator evaluator-closer",
            "trusted_execution": "ao2 signed evidence boundary",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
        },
        "links": links,
    })
}

fn handoff_build_unavailable_checklist(
    source: &serde_json::Value,
    status: &str,
) -> serde_json::Value {
    let reason = source
        .get("reason")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("AO2 release-candidate handoff is not available")
        .to_string();
    let blockers: Vec<String> = if status == "planned" {
        Vec::new()
    } else {
        vec![format!("handoff_available: {reason}")]
    };
    let links = match source.get("links") {
        Some(v) if v.is_object() => v.clone(),
        _ => serde_json::json!({}),
    };
    serde_json::json!({
        "schema": HANDOFF_CHECKLIST_SCHEMA,
        "status": status,
        "release": serde_json::json!({}),
        "checks": [
            handoff_check("handoff_available", "Handoff available", status, "ready"),
        ],
        "blockers": blockers,
        "operator_decision": {
            "factory_v3_evaluator_closer_required": true,
            "control_plane_approves_release": false,
            "next_action": "fetch AO2 release-candidate handoff before evaluator-closer release-line review",
        },
        "trust_boundary": {
            "frontend": "Hermes front end / queue / memory surface",
            "governed_backend": "factory-v3 / AO Operator evaluator-closer",
            "trusted_execution": "ao2 signed evidence boundary",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
        },
        "links": links,
    })
}

pub(crate) fn release_handoff_checklist_build(
    handoff_path: &Path,
    expected_repo_head: &[String],
    allow_skipped: bool,
) -> Result<serde_json::Value> {
    let expected_repo_heads = handoff_parse_expected_repo_heads(expected_repo_head)?;
    let source = evaluator_read_json(handoff_path)?;
    let payload = match handoff_extract(&source) {
        Some(handoff) => handoff_build_checklist(handoff, &expected_repo_heads),
        None => {
            if !allow_skipped {
                return Err(anyhow!(
                    "input does not contain an AO2 release-candidate handoff"
                ));
            }
            let source_status = source
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("skipped");
            let status = if source_status == "planned" {
                "planned"
            } else {
                "skipped"
            };
            handoff_build_unavailable_checklist(&source, status)
        }
    };
    Ok(payload)
}

pub(crate) fn release_handoff_checklist_markdown(payload: &serde_json::Value) -> String {
    let release = payload
        .get("release")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let release_tag = if release.is_object() {
        release
            .get("release_tag")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("missing")
            .to_string()
    } else {
        "missing".to_string()
    };
    let operator_decision = payload
        .get("operator_decision")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let evaluator_required = operator_decision
        .get("factory_v3_evaluator_closer_required")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "false".to_string());
    let cp_approves = operator_decision
        .get("control_plane_approves_release")
        .map(|v| {
            if v.as_bool() == Some(true) {
                "True".to_string()
            } else if v.as_bool() == Some(false) {
                "False".to_string()
            } else {
                v.to_string()
            }
        })
        .unwrap_or_else(|| "False".to_string());
    let evaluator_required_pretty = if evaluator_required == "true" {
        "True".to_string()
    } else if evaluator_required == "false" {
        "False".to_string()
    } else {
        evaluator_required
    };
    let mut lines: Vec<String> = vec![
        "# AO2 Release Handoff Checklist".to_string(),
        "".to_string(),
        format!(
            "- status: `{}`",
            payload
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("missing")
        ),
        format!("- release_tag: `{release_tag}`"),
        format!("- evaluator_closer_required: `{evaluator_required_pretty}`"),
        format!("- control_plane_approves_release: `{cp_approves}`"),
        "".to_string(),
        "## Checks".to_string(),
        "".to_string(),
        "| Check | Status | Observed | Expected |".to_string(),
        "| --- | --- | --- | --- |".to_string(),
    ];
    if let Some(items) = payload.get("checks").and_then(|v| v.as_array()) {
        for item in items {
            lines.push(format!(
                "| {} | `{}` | `{}` | `{}` |",
                item.get("label").and_then(|v| v.as_str()).unwrap_or(""),
                item.get("status").and_then(|v| v.as_str()).unwrap_or(""),
                item.get("observed").and_then(|v| v.as_str()).unwrap_or(""),
                item.get("expected").and_then(|v| v.as_str()).unwrap_or(""),
            ));
        }
    }
    lines.push("".to_string());
    lines.push("## Blockers".to_string());
    lines.push("".to_string());
    let blockers = payload.get("blockers").and_then(|v| v.as_array());
    match blockers {
        Some(items) if !items.is_empty() => {
            for blocker in items {
                let text = blocker
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| blocker.to_string());
                lines.push(format!("- {text}"));
            }
        }
        _ => lines.push("- none".to_string()),
    }
    lines.push("".to_string());
    lines.push("## Trust Boundary".to_string());
    lines.push("".to_string());
    lines.push("- Hermes remains front end, queue, cron, and memory surface.".to_string());
    lines.push("- ao2 remains the trusted signed evidence boundary.".to_string());
    lines.push("- ao2-control-plane remains read-only and does not approve releases.".to_string());
    lines.push("- factory-v3 evaluator-closer owns release acceptance.".to_string());
    lines.push("".to_string());
    lines.join("\n")
}

// ----------------------------------------------------------------------------
// factory-v3/ao2-release-evaluator-decision/v1 — AO2-native producer
// ----------------------------------------------------------------------------
//
// Phase 2 W2 P0: this is the Rust translation of
// factory-v3/scripts/ao2_release_evaluator_decision.py. Output is designed to
// be byte-equal (under canonical sort+separator serialisation) to the Python
// producer so the factory-v3 parity oracle can audit AO2 as the canonical
// source of `factory-v3/ao2-release-evaluator-decision/v1`.

const EVALUATOR_DECISION_SCHEMA: &str = "factory-v3/ao2-release-evaluator-decision/v1";
const EVALUATOR_READINESS_BRIDGE_SCHEMA: &str = "factory-v3/hermes-ao-bridge/v1";
const EVALUATOR_READINESS_SCHEMA: &str = "ao2.cp-release-readiness.v1";
const EVALUATOR_HANDOFF_CHECKLIST_SCHEMA: &str = "factory-v3/ao2-release-handoff-checklist/v1";
const EVALUATOR_SUPPORT_BUNDLE_BRIDGE_ACTION: &str = "release-support-bundle-status";
const EVALUATOR_SUPPORT_BUNDLE_SCHEMA: &str = "ao2.cp-release-support-bundle.v1";
const EVALUATOR_RELEASE_ASSEMBLY_SCHEMA: &str = "ao2.cp-release-assembly.v1";

fn evaluator_read_json(path: &Path) -> Result<serde_json::Value> {
    let content =
        fs::read_to_string(path).with_context(|| format!("missing input: {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("invalid json in {}", path.display()))?;
    if !value.is_object() {
        return Err(anyhow!("expected json object in {}", path.display()));
    }
    Ok(value)
}

fn evaluator_as_str(value: Option<&serde_json::Value>) -> String {
    evaluator_as_str_default(value, "missing")
}

fn evaluator_as_str_default(value: Option<&serde_json::Value>, default: &str) -> String {
    match value {
        Some(serde_json::Value::String(text)) if !text.is_empty() => text.clone(),
        _ => default.to_string(),
    }
}

fn evaluator_as_bool(value: Option<&serde_json::Value>, default: bool) -> bool {
    match value {
        Some(serde_json::Value::Bool(b)) => *b,
        _ => default,
    }
}

fn evaluator_nested<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for key in keys {
        current = current.get(*key)?;
    }
    Some(current)
}

fn evaluator_extract_readiness(payload: &serde_json::Value) -> Option<&serde_json::Value> {
    if payload.get("schema_version").and_then(|v| v.as_str()) == Some(EVALUATOR_READINESS_SCHEMA) {
        return Some(payload);
    }
    if payload.get("schema").and_then(|v| v.as_str()) == Some(EVALUATOR_READINESS_BRIDGE_SCHEMA)
        && payload.get("action").and_then(|v| v.as_str()) == Some("release-readiness-status")
    {
        let snapshot = payload.get("readiness_snapshot")?;
        if snapshot.is_object()
            && snapshot.get("schema_version").and_then(|v| v.as_str())
                == Some(EVALUATOR_READINESS_SCHEMA)
        {
            return Some(snapshot);
        }
    }
    None
}

fn evaluator_extract_support_bundle(payload: &serde_json::Value) -> Option<&serde_json::Value> {
    if payload.get("schema_version").and_then(|v| v.as_str())
        == Some(EVALUATOR_SUPPORT_BUNDLE_SCHEMA)
    {
        return Some(payload);
    }
    if payload.get("schema").and_then(|v| v.as_str()) == Some(EVALUATOR_READINESS_BRIDGE_SCHEMA)
        && payload.get("action").and_then(|v| v.as_str())
            == Some(EVALUATOR_SUPPORT_BUNDLE_BRIDGE_ACTION)
    {
        let snapshot = payload.get("support_bundle_snapshot")?;
        if snapshot.is_object()
            && snapshot.get("schema_version").and_then(|v| v.as_str())
                == Some(EVALUATOR_SUPPORT_BUNDLE_SCHEMA)
        {
            return Some(snapshot);
        }
    }
    None
}

fn evaluator_release_from(
    readiness: Option<&serde_json::Value>,
    checklist: &serde_json::Value,
) -> serde_json::Value {
    if let Some(r) = readiness {
        if let Some(release) = r.get("release") {
            if release.is_object() && !release.as_object().map(|m| m.is_empty()).unwrap_or(true) {
                return release.clone();
            }
        }
    }
    match checklist.get("release") {
        Some(release) if release.is_object() => release.clone(),
        _ => serde_json::json!({}),
    }
}

fn evaluator_check(
    check_id: &str,
    label: &str,
    observed: &str,
    expected: &str,
    blockers: &mut Vec<String>,
    allow_pending_self_reference: bool,
) -> serde_json::Value {
    let mut status = if observed == expected {
        "passed"
    } else {
        "blocked"
    };
    if status == "blocked" && allow_pending_self_reference {
        status = "passed_pending_self_reference";
    }
    if status == "blocked" {
        blockers.push(format!(
            "{check_id}: expected {expected}, observed {observed}"
        ));
    }
    serde_json::json!({
        "id": check_id,
        "label": label,
        "observed": observed,
        "expected": expected,
        "status": status,
    })
}

fn evaluator_is_missing_evaluator_blocker(value: &serde_json::Value) -> bool {
    let text = match value.as_str() {
        Some(s) => s,
        None => return false,
    };
    matches!(
        text,
        "release_evaluator_decision: expected accepted, observed missing"
            | "candidate_correlation: expected matched, observed mismatched"
            | "handoff_status: expected ready, observed attention"
    ) || text.starts_with("release_evaluator_version unknown does not match ")
        || text.starts_with("release_evaluator_tag unknown does not match ")
}

fn evaluator_only_missing_evaluator_blockers(values: Option<&serde_json::Value>) -> bool {
    let array = match values.and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => a,
        _ => return false,
    };
    array.iter().all(evaluator_is_missing_evaluator_blocker)
}

fn evaluator_candidate_correlation_is_only_missing_evaluator(
    release_assembly: &serde_json::Value,
    support_frontend: &serde_json::Value,
    release: &serde_json::Value,
) -> bool {
    let candidate_correlation = evaluator_as_str(
        release_assembly
            .get("candidate_correlation")
            .or_else(|| support_frontend.get("candidate_correlation")),
    );
    if candidate_correlation != "mismatched" {
        return false;
    }
    let detail = match release_assembly.get("candidate_correlation_detail") {
        Some(d) if d.is_object() => d,
        _ => return false,
    };
    if !evaluator_only_missing_evaluator_blockers(detail.get("blockers")) {
        return false;
    }
    let release_version = evaluator_as_str(release.get("version"));
    let release_tag = evaluator_as_str(release.get("release_tag"));
    evaluator_as_str(detail.get("release_version")) == release_version
        && evaluator_as_str(detail.get("release_tag")) == release_tag
        && evaluator_as_str(detail.get("codex_acceptance_version")) == release_version
        && evaluator_as_str(detail.get("claude_acceptance_version")) == release_version
        && evaluator_as_str(detail.get("three_os_version")) == release_version
        && evaluator_as_str(detail.get("release_evaluator_version")) == "unknown"
        && evaluator_as_str(detail.get("release_evaluator_tag")) == "unknown"
}

#[allow(clippy::too_many_arguments)]
fn evaluator_should_apply_missing_evaluator_self_reference_exception(
    readiness: Option<&serde_json::Value>,
    readiness_frontend: &serde_json::Value,
    readiness_blockers: Option<&serde_json::Value>,
    checklist: &serde_json::Value,
    checklist_blockers: Option<&serde_json::Value>,
    support_frontend: &serde_json::Value,
    release_assembly: &serde_json::Value,
    release: &serde_json::Value,
) -> bool {
    let readiness_status = evaluator_as_str(
        readiness
            .and_then(|r| r.get("status"))
            .or_else(|| readiness_frontend.get("status")),
    );
    let handoff_status = evaluator_as_str(checklist.get("status"));
    let assembly_status = evaluator_as_str(
        release_assembly
            .get("status")
            .or_else(|| support_frontend.get("status")),
    );
    let missing_count = match support_frontend.get("missing_artifact_count") {
        Some(v) => match v {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "None".to_string(),
            _ => v.to_string(),
        },
        None => "missing".to_string(),
    };
    readiness_status == "attention"
        && handoff_status == "blocked"
        && assembly_status == "attention"
        && missing_count == "0"
        && evaluator_only_missing_evaluator_blockers(readiness_blockers)
        && evaluator_only_missing_evaluator_blockers(checklist_blockers)
        && evaluator_candidate_correlation_is_only_missing_evaluator(
            release_assembly,
            support_frontend,
            release,
        )
}

pub(crate) fn release_evaluator_decision_build(
    readiness_path: &Path,
    checklist_path: &Path,
    support_bundle_path: &Path,
) -> Result<serde_json::Value> {
    let readiness_source = evaluator_read_json(readiness_path)?;
    let checklist = evaluator_read_json(checklist_path)?;
    let support_bundle_source = evaluator_read_json(support_bundle_path)?;
    Ok(release_evaluator_decision_payload(
        readiness_path,
        &readiness_source,
        checklist_path,
        &checklist,
        support_bundle_path,
        &support_bundle_source,
    ))
}

fn release_evaluator_decision_payload(
    readiness_path: &Path,
    readiness_source: &serde_json::Value,
    checklist_path: &Path,
    checklist: &serde_json::Value,
    support_bundle_path: &Path,
    support_bundle_source: &serde_json::Value,
) -> serde_json::Value {
    let mut blockers: Vec<String> = Vec::new();
    let readiness = evaluator_extract_readiness(readiness_source);
    let empty = serde_json::json!({});
    let readiness_frontend = match readiness_source.get("frontend_status") {
        Some(v) if v.is_object() => v,
        _ => &empty,
    };
    let support_bundle = evaluator_extract_support_bundle(support_bundle_source);
    let support_frontend = match support_bundle_source.get("frontend_status") {
        Some(v) if v.is_object() => v,
        _ => &empty,
    };
    let release_assembly = support_bundle
        .and_then(|b| b.get("release_assembly"))
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let release = evaluator_release_from(readiness, checklist);
    let readiness_blockers = readiness.and_then(|r| r.get("blockers"));
    let checklist_blockers = checklist.get("blockers");
    let self_reference_exception_applied =
        evaluator_should_apply_missing_evaluator_self_reference_exception(
            readiness,
            readiness_frontend,
            readiness_blockers,
            checklist,
            checklist_blockers,
            support_frontend,
            &release_assembly,
            &release,
        );

    let mut checks: Vec<serde_json::Value> = vec![
        evaluator_check(
            "readiness_schema",
            "Readiness schema",
            &evaluator_as_str(readiness.and_then(|r| r.get("schema_version"))),
            EVALUATOR_READINESS_SCHEMA,
            &mut blockers,
            false,
        ),
        evaluator_check(
            "readiness_status",
            "Release readiness status",
            &evaluator_as_str(
                readiness
                    .and_then(|r| r.get("status"))
                    .or_else(|| readiness_frontend.get("status")),
            ),
            "ready",
            &mut blockers,
            self_reference_exception_applied,
        ),
        evaluator_check(
            "handoff_checklist_schema",
            "Handoff checklist schema",
            &evaluator_as_str(checklist.get("schema")),
            EVALUATOR_HANDOFF_CHECKLIST_SCHEMA,
            &mut blockers,
            false,
        ),
        evaluator_check(
            "handoff_checklist_status",
            "Handoff checklist status",
            &evaluator_as_str(checklist.get("status")),
            "ready_for_evaluator_closer",
            &mut blockers,
            self_reference_exception_applied,
        ),
        evaluator_check(
            "support_bundle_schema",
            "Support bundle schema",
            &evaluator_as_str(support_bundle.and_then(|s| s.get("schema_version"))),
            EVALUATOR_SUPPORT_BUNDLE_SCHEMA,
            &mut blockers,
            false,
        ),
        evaluator_check(
            "release_assembly_schema",
            "Release assembly schema",
            &evaluator_as_str(release_assembly.get("schema_version")),
            EVALUATOR_RELEASE_ASSEMBLY_SCHEMA,
            &mut blockers,
            false,
        ),
        evaluator_check(
            "release_assembly_status",
            "Release assembly status",
            &evaluator_as_str(
                release_assembly
                    .get("status")
                    .or_else(|| support_frontend.get("status")),
            ),
            "assembled",
            &mut blockers,
            self_reference_exception_applied,
        ),
        evaluator_check(
            "release_assembly_candidate_correlation",
            "Release assembly candidate correlation",
            &evaluator_as_str(
                release_assembly
                    .get("candidate_correlation")
                    .or_else(|| support_frontend.get("candidate_correlation")),
            ),
            "matched",
            &mut blockers,
            self_reference_exception_applied,
        ),
    ];
    let missing_count = match support_frontend.get("missing_artifact_count") {
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        Some(serde_json::Value::Null) => "None".to_string(),
        Some(other) => other.to_string(),
        None => "missing".to_string(),
    };
    checks.push(evaluator_check(
        "release_assembly_missing_artifacts",
        "Release assembly missing artifacts",
        &missing_count,
        "0",
        &mut blockers,
        false,
    ));

    if let Some(serde_json::Value::Array(items)) = readiness_blockers {
        for blocker in items {
            if self_reference_exception_applied && evaluator_is_missing_evaluator_blocker(blocker) {
                continue;
            }
            let text = match blocker.as_str() {
                Some(s) => s.to_string(),
                None => blocker.to_string(),
            };
            blockers.push(format!("readiness_blocker: {text}"));
        }
    }
    if let Some(serde_json::Value::Array(items)) = checklist_blockers {
        for blocker in items {
            if self_reference_exception_applied && evaluator_is_missing_evaluator_blocker(blocker) {
                continue;
            }
            let text = match blocker.as_str() {
                Some(s) => s.to_string(),
                None => blocker.to_string(),
            };
            blockers.push(format!("handoff_checklist_blocker: {text}"));
        }
    }

    let readiness_control_plane_approves = evaluator_as_bool(
        readiness.and_then(|r| {
            evaluator_nested(r, &["operator_decision", "control_plane_approves_release"])
        }),
        evaluator_as_bool(
            readiness_frontend.get("control_plane_approves_release"),
            false,
        ),
    );
    let checklist_control_plane_approves = evaluator_as_bool(
        evaluator_nested(
            checklist,
            &["operator_decision", "control_plane_approves_release"],
        ),
        false,
    );
    let support_control_plane_approves = evaluator_as_bool(
        release_assembly.get("control_plane_approves_release"),
        evaluator_as_bool(
            support_frontend.get("control_plane_approves_release"),
            false,
        ),
    );
    if readiness_control_plane_approves
        || checklist_control_plane_approves
        || support_control_plane_approves
    {
        blockers.push("trust_boundary: control plane must not approve release".to_string());
    }

    let release_version = evaluator_as_str(release.get("version"));
    let assembly_version = evaluator_as_str(
        release_assembly
            .get("release_candidate_version")
            .or_else(|| support_frontend.get("release_candidate_version")),
    );
    if release_version != "missing" && assembly_version != release_version {
        blockers.push(format!(
            "release_assembly_version: expected {release_version}, observed {assembly_version}"
        ));
    }
    let accepted = blockers.is_empty();
    let decision = if accepted {
        "accept_phase1_release_candidate"
    } else {
        "reject_phase1_release_candidate"
    };
    let self_reference_status = if self_reference_exception_applied {
        "applied"
    } else {
        "not_applicable"
    };
    let self_reference_reason = if self_reference_exception_applied {
        "control-plane readiness was waiting only for the factory-v3 evaluator decision currently being produced"
    } else {
        "no evaluator self-reference gap was detected"
    };
    let next_action = if accepted {
        "release candidate is accepted by factory-v3 evaluator-closer for release-line handoff"
    } else {
        "resolve blockers before release-line handoff"
    };
    serde_json::json!({
        "schema": EVALUATOR_DECISION_SCHEMA,
        "status": if accepted { "accepted" } else { "rejected" },
        "decision": decision,
        "release": release,
        "checks": checks,
        "blockers": blockers,
        "self_reference_exception": {
            "status": self_reference_status,
            "reason": self_reference_reason,
        },
        "evidence": {
            "release_readiness_status": readiness_path.display().to_string(),
            "release_handoff_checklist": checklist_path.display().to_string(),
            "release_support_bundle_status": support_bundle_path.display().to_string(),
        },
        "trust_boundary": {
            "frontend": "Hermes front end / queue / memory surface",
            "governed_backend": "factory-v3 / AO Operator evaluator-closer",
            "trusted_execution": "ao2 signed evidence boundary",
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": false,
            "control_plane_approves_release": false,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
        },
        "next_action": next_action,
    })
}

pub(crate) fn release_evaluator_decision_markdown(payload: &serde_json::Value) -> String {
    let release = payload
        .get("release")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let release_tag = if release.is_object() {
        evaluator_as_str(release.get("release_tag"))
    } else {
        "missing".to_string()
    };
    let mut lines: Vec<String> = vec![
        "# AO2 Release Evaluator Decision".to_string(),
        "".to_string(),
        format!(
            "- status: `{}`",
            evaluator_as_str_default(payload.get("status"), "missing")
        ),
        format!(
            "- decision: `{}`",
            evaluator_as_str_default(payload.get("decision"), "missing")
        ),
        format!("- release_tag: `{release_tag}`"),
        "- release_acceptance_owner: `factory-v3 evaluator-closer`".to_string(),
        "- control_plane_approves_release: `False`".to_string(),
        "".to_string(),
        "## Checks".to_string(),
        "".to_string(),
        "| Check | Status | Observed | Expected |".to_string(),
        "| --- | --- | --- | --- |".to_string(),
    ];
    if let Some(items) = payload.get("checks").and_then(|v| v.as_array()) {
        for item in items {
            lines.push(format!(
                "| {} | `{}` | `{}` | `{}` |",
                evaluator_as_str_default(item.get("label"), ""),
                evaluator_as_str_default(item.get("status"), ""),
                evaluator_as_str_default(item.get("observed"), ""),
                evaluator_as_str_default(item.get("expected"), ""),
            ));
        }
    }
    lines.push("".to_string());
    lines.push("## Blockers".to_string());
    lines.push("".to_string());
    let blockers = payload.get("blockers").and_then(|v| v.as_array());
    match blockers {
        Some(items) if !items.is_empty() => {
            for blocker in items {
                let text = blocker
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| blocker.to_string());
                lines.push(format!("- {text}"));
            }
        }
        _ => lines.push("- none".to_string()),
    }
    if let Some(exception) = payload.get("self_reference_exception") {
        if exception.get("status").and_then(|v| v.as_str()) == Some("applied") {
            lines.push("".to_string());
            lines.push("## Self Reference Exception".to_string());
            lines.push("".to_string());
            lines.push(format!(
                "- self_reference_exception: `{}`",
                evaluator_as_str_default(exception.get("status"), "")
            ));
            lines.push(format!(
                "- reason: `{}`",
                evaluator_as_str_default(exception.get("reason"), "")
            ));
        }
    }
    lines.push("".to_string());
    lines.push("## Evidence".to_string());
    lines.push("".to_string());
    let evidence = payload
        .get("evidence")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    lines.push(format!(
        "- release_readiness_status: `{}`",
        evaluator_as_str_default(evidence.get("release_readiness_status"), "")
    ));
    lines.push(format!(
        "- release_handoff_checklist: `{}`",
        evaluator_as_str_default(evidence.get("release_handoff_checklist"), "")
    ));
    lines.push(format!(
        "- release_support_bundle_status: `{}`",
        evaluator_as_str_default(evidence.get("release_support_bundle_status"), "")
    ));
    lines.push("".to_string());
    lines.join("\n")
}
