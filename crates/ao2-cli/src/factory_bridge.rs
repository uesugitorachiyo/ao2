//! AO2-native `ao2 factory bridge` subcommand.
//!
//! The bridge is the Phase 2 exit-gate single-command surface for migration
//! roadmap items #1 (AO Operator -> AO2 run bridge) and #2 (deterministic
//! AO Operator role -> AO2 provider-contract mapping). It mirrors the
//! factory-v3 Python bridge at `scripts/start_ao2_run_from_role_runspec.py`
//! and `scripts/ao_operator_ao2_provider_contract.py`, but the evidence is
//! AO2-native: the digest, mapping table, and trust boundary are all signed
//! and owned by AO2 instead of by a factory-v3 Python producer.
//!
//! The canonical roles, alias map, and provider-contract entries are kept
//! byte-for-byte in sync with the Python mapping module so that
//! `factory_bridge::mapping_digest()` equals `ao_operator_ao2_provider_contract.mapping_digest()`.
//! The Rust integration test pins the digest against the value emitted by the
//! Python module on the same git revision; any drift fails the test before
//! it can leak through to a bridge invocation.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use ao2_core::sha256_hex;
use serde_json::{json, Value};

pub const BRIDGE_SCHEMA: &str = "ao2.factory-bridge.v1";
pub const BRIDGE_ACTION: &str = "factory-bridge";

pub const MAPPING_SCHEMA: &str = "factory-v3/ao-operator-ao2-provider-contract/v1";
pub const MAPPING_VERSION: &str = "1.0.0";

pub const REDACTED_ENV_SUBSTRINGS: &[&str] = &["API_KEY", "TOKEN", "SECRET", "PASSWORD", "AUTH"];

pub const CANONICAL_ROLES: &[&str] = &[
    "intake",
    "planner",
    "plan_hardener",
    "factory_manager",
    "implementer",
    "reviewer",
    "integrator",
    "evaluator_closer",
];

pub const ROLE_ALIASES: &[(&str, &str)] = &[
    ("intake", "intake"),
    ("planner-intake", "intake"),
    ("planner_intake", "intake"),
    ("planner", "planner"),
    ("agent-os-planner", "planner"),
    ("agent_os_planner", "planner"),
    ("plan-hardener", "plan_hardener"),
    ("plan_hardener", "plan_hardener"),
    ("agent-os-plan-hardener", "plan_hardener"),
    ("agent_os_plan_hardener", "plan_hardener"),
    ("factory-manager", "factory_manager"),
    ("factory_manager", "factory_manager"),
    ("agent-os-factory-manager", "factory_manager"),
    ("agent_os_factory_manager", "factory_manager"),
    ("implementer", "implementer"),
    ("implementer-slice", "implementer"),
    ("implementer_slice", "implementer"),
    ("agent-os-implementer", "implementer"),
    ("agent_os_implementer", "implementer"),
    ("reviewer", "reviewer"),
    ("reviewer-slice", "reviewer"),
    ("reviewer_slice", "reviewer"),
    ("slice-reviewer", "reviewer"),
    ("slice_reviewer", "reviewer"),
    ("agent-os-slice-reviewer", "reviewer"),
    ("agent_os_slice_reviewer", "reviewer"),
    ("integrator", "integrator"),
    ("agent-os-integrator", "integrator"),
    ("agent_os_integrator", "integrator"),
    ("evaluator-closer", "evaluator_closer"),
    ("evaluator_closer", "evaluator_closer"),
    ("agent-os-evaluator-closer", "evaluator_closer"),
    ("agent_os_evaluator_closer", "evaluator_closer"),
];

pub struct ProviderContract {
    pub slug: &'static str,
    pub sandbox: &'static str,
    pub evidence_obligation: &'static str,
    pub closure_owner: &'static str,
}

pub const AO2_PROVIDER_CONTRACTS: &[(&str, ProviderContract)] = &[
    (
        "intake",
        ProviderContract {
            slug: "ao2.provider-contract.intake.v1",
            sandbox: "read_only_brief_summarization",
            evidence_obligation: "intake_summary_with_brief_digest",
            closure_owner: "ao2_native_evaluator_closer",
        },
    ),
    (
        "planner",
        ProviderContract {
            slug: "ao2.provider-contract.planner.v1",
            sandbox: "read_only_planning_with_scoped_writes",
            evidence_obligation: "plan_artifact_with_role_contract_refs",
            closure_owner: "ao2_native_evaluator_closer",
        },
    ),
    (
        "plan_hardener",
        ProviderContract {
            slug: "ao2.provider-contract.plan-hardener.v1",
            sandbox: "read_only_planning_with_scoped_writes",
            evidence_obligation: "hardened_plan_with_threat_model_refs",
            closure_owner: "ao2_native_evaluator_closer",
        },
    ),
    (
        "factory_manager",
        ProviderContract {
            slug: "ao2.provider-contract.factory-manager.v1",
            sandbox: "read_only_orchestration",
            evidence_obligation: "factory_manager_dispatch_decisions",
            closure_owner: "ao2_native_evaluator_closer",
        },
    ),
    (
        "implementer",
        ProviderContract {
            slug: "ao2.provider-contract.implementer.v1",
            sandbox: "scoped_write_with_digest_patch_and_repair_budget",
            evidence_obligation: "implementation_digest_patch_and_test_evidence",
            closure_owner: "ao2_native_evaluator_closer",
        },
    ),
    (
        "reviewer",
        ProviderContract {
            slug: "ao2.provider-contract.reviewer.v1",
            sandbox: "read_only_review",
            evidence_obligation: "review_artifact_with_diff_and_test_refs",
            closure_owner: "ao2_native_evaluator_closer",
        },
    ),
    (
        "integrator",
        ProviderContract {
            slug: "ao2.provider-contract.integrator.v1",
            sandbox: "scoped_write_with_merge_and_repair_budget",
            evidence_obligation: "integration_evidence_with_merge_refs",
            closure_owner: "ao2_native_evaluator_closer",
        },
    ),
    (
        "evaluator_closer",
        ProviderContract {
            slug: "ao2.provider-contract.evaluator-closer.v1",
            sandbox: "read_only_evaluation_with_signed_decision",
            evidence_obligation: "evaluator_decision_signed_with_trust_boundary",
            closure_owner: "ao2_native_evaluator_closer",
        },
    ),
];

/// Mapping-module trust boundary. Must match the Python module byte-for-byte
/// so the digest is identical.
pub const MAPPING_TRUST_BOUNDARY: &[(&str, &str)] = &[
    (
        "factory_v3_role",
        "ao_operator_role_canonicalization_and_mapping_source",
    ),
    ("ao2_role", "provider_contract_owner_and_closure_authority"),
    (
        "control_plane_role",
        "read_only_observer_for_signed_evidence_and_memory_exports",
    ),
    (
        "mapping_owner",
        "factory_v3_to_ao2_provider_contract_mapping_module",
    ),
];

/// AO2-native bridge-evidence trust boundary. AO2 owns the evidence; the
/// factory-v3 Python bridge becomes a `parity_oracle_only` consumer once
/// passthrough mode is enabled there.
pub const BRIDGE_TRUST_BOUNDARY: &[(&str, &str)] = &[
    ("factory_v3_role", "parity_oracle_only"),
    ("ao2_role", "ao2_native_bridge_evidence_owner"),
    (
        "control_plane_role",
        "read_only_observer_after_signed_evidence",
    ),
    ("bridge_owner", "ao2_factory_bridge_subcommand"),
];

fn provider_contract(canonical: &str) -> &'static ProviderContract {
    AO2_PROVIDER_CONTRACTS
        .iter()
        .find(|(role, _)| *role == canonical)
        .map(|(_, contract)| contract)
        .expect("canonical role must have a provider contract entry")
}

fn normalize(role_id: &str) -> String {
    role_id.trim().to_ascii_lowercase().replace('_', "-")
}

/// Canonicalize a raw role id, mirroring the Python `canonical_role` lookup.
pub fn canonical_role(role_id: &str) -> Result<&'static str> {
    let normalized = normalize(role_id);
    if let Some((_, target)) = ROLE_ALIASES.iter().find(|(alias, _)| *alias == normalized) {
        return Ok(target);
    }
    let underscore = normalized.replace('-', "_");
    if let Some(role) = CANONICAL_ROLES
        .iter()
        .find(|candidate| **candidate == underscore)
    {
        return Ok(*role);
    }
    if let Some(stripped) = strip_numeric_fan_out_suffix(&normalized) {
        if let Some((_, target)) = ROLE_ALIASES.iter().find(|(alias, _)| *alias == stripped) {
            return Ok(target);
        }
        let stripped_underscore = stripped.replace('-', "_");
        if let Some(role) = CANONICAL_ROLES
            .iter()
            .find(|candidate| **candidate == stripped_underscore)
        {
            return Ok(*role);
        }
    }
    Err(anyhow!(
        "AO Operator role id {:?} has no AO2 provider-contract mapping; either add an alias to ROLE_ALIASES or a canonical role + contract entry.",
        role_id
    ))
}

/// Strip a trailing `-N` (where `N` is one or more ASCII digits) from
/// `normalized`. Used to canonicalize numbered fan-out role ids like
/// `implementer-slice-1` to `implementer-slice` so they map to their parent
/// canonical role through `ROLE_ALIASES`. Returns `None` when the input
/// does not end with `-` + ASCII digits. Byte-identical to the Python
/// implementation; does not affect `mapping_digest()` (the digest is
/// computed over the static tables, not this function body).
fn strip_numeric_fan_out_suffix(normalized: &str) -> Option<String> {
    let bytes = normalized.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_digit() {
        end -= 1;
    }
    if end == bytes.len() || end == 0 || bytes[end - 1] != b'-' {
        return None;
    }
    Some(normalized[..end - 1].to_string())
}

/// Resolve a single role id into its provider-contract record.
pub fn resolve_role(role_id: &str) -> Result<Value> {
    let canonical = canonical_role(role_id)?;
    let contract = provider_contract(canonical);
    let mut entry: BTreeMap<String, Value> = BTreeMap::new();
    entry.insert("role_id".to_string(), json!(role_id));
    entry.insert("canonical_role".to_string(), json!(canonical));
    entry.insert(
        "ao2_provider_contract_slug".to_string(),
        json!(contract.slug),
    );
    entry.insert("sandbox".to_string(), json!(contract.sandbox));
    entry.insert(
        "evidence_obligation".to_string(),
        json!(contract.evidence_obligation),
    );
    entry.insert("closure_owner".to_string(), json!(contract.closure_owner));
    Ok(Value::Object(entry.into_iter().collect()))
}

#[derive(Clone, Debug)]
pub struct RunSpecRoleTask {
    pub role_id: String,
    pub kind: String,
    pub depends_on: Vec<String>,
    pub provider: Option<String>,
    pub agent: Option<String>,
    pub prompt_file: Option<String>,
    pub workspace: Option<String>,
    pub policy_profile: Option<String>,
    pub dispatch_authorized: Option<bool>,
}

fn json_string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .map(|field| field.to_string())
}

fn json_string_array_field(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|field| field.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|item| item.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Pull the ordered role-task records out of a RunSpec.
///
/// Supports the factory-v3/runspec/v1 shape (top-level `roles:`) and the
/// ao.dev/v1 `Run` shape (`spec.tasks:` with `kind: agent`). The ao.dev/v1
/// path also preserves Factory-owned provider/profile fields so AO2 can
/// materialize a governed plan without factory-v3 rehydrating the RunSpec.
pub fn extract_role_tasks(runspec: &Value) -> Vec<RunSpecRoleTask> {
    let mut tasks_out: Vec<RunSpecRoleTask> = Vec::new();
    if let Some(roles) = runspec.get("roles").and_then(|value| value.as_array()) {
        for entry in roles {
            if let Some(id) = entry.get("id").and_then(|value| value.as_str()) {
                tasks_out.push(RunSpecRoleTask {
                    role_id: id.to_string(),
                    kind: "agent".to_string(),
                    depends_on: Vec::new(),
                    provider: json_string_field(entry, "provider"),
                    agent: json_string_field(entry, "agent"),
                    prompt_file: json_string_field(entry, "promptFile")
                        .or_else(|| json_string_field(entry, "prompt_file")),
                    workspace: json_string_field(entry, "workspace"),
                    policy_profile: json_string_field(entry, "policyProfile")
                        .or_else(|| json_string_field(entry, "policy_profile")),
                    dispatch_authorized: entry
                        .get("dispatchAuthorized")
                        .or_else(|| entry.get("dispatch_authorized"))
                        .and_then(|value| value.as_bool()),
                });
            }
        }
        return tasks_out;
    }
    if let Some(tasks) = runspec
        .get("spec")
        .and_then(|spec| spec.get("tasks"))
        .and_then(|value| value.as_array())
    {
        for entry in tasks {
            let Some(object) = entry.as_object() else {
                continue;
            };
            let kind = object
                .get("kind")
                .and_then(|value| value.as_str())
                .unwrap_or("agent");
            if kind != "agent" {
                continue;
            }
            if let Some(id) = object.get("id").and_then(|value| value.as_str()) {
                let spec = object.get("spec").unwrap_or(entry);
                tasks_out.push(RunSpecRoleTask {
                    role_id: id.to_string(),
                    kind: kind.to_string(),
                    depends_on: json_string_array_field(entry, "deps"),
                    provider: json_string_field(spec, "provider"),
                    agent: json_string_field(spec, "agent"),
                    prompt_file: json_string_field(spec, "promptFile")
                        .or_else(|| json_string_field(spec, "prompt_file")),
                    workspace: json_string_field(spec, "workspace"),
                    policy_profile: json_string_field(spec, "policyProfile")
                        .or_else(|| json_string_field(spec, "policy_profile")),
                    dispatch_authorized: spec
                        .get("dispatchAuthorized")
                        .or_else(|| spec.get("dispatch_authorized"))
                        .and_then(|value| value.as_bool()),
                });
            }
        }
    }
    tasks_out
}

/// Pull the ordered list of role ids out of a RunSpec.
#[allow(dead_code)]
pub fn extract_role_ids(runspec: &Value) -> Vec<String> {
    extract_role_tasks(runspec)
        .into_iter()
        .map(|task| task.role_id)
        .collect()
}

/// Build the canonicalized mapping table that the digest is computed over.
///
/// Every nested object is a `BTreeMap` so serde_json serializes keys in
/// alphabetical order, matching the Python `json.dumps(..., sort_keys=True)`
/// shape used to compute the source-of-truth digest.
pub fn mapping_table() -> Value {
    let mut role_aliases: BTreeMap<String, Value> = BTreeMap::new();
    for (alias, target) in ROLE_ALIASES {
        role_aliases.insert((*alias).to_string(), json!(target));
    }
    let mut provider_contracts: BTreeMap<String, Value> = BTreeMap::new();
    for (role, contract) in AO2_PROVIDER_CONTRACTS {
        let mut inner: BTreeMap<String, Value> = BTreeMap::new();
        inner.insert("slug".to_string(), json!(contract.slug));
        inner.insert("sandbox".to_string(), json!(contract.sandbox));
        inner.insert(
            "evidence_obligation".to_string(),
            json!(contract.evidence_obligation),
        );
        inner.insert("closure_owner".to_string(), json!(contract.closure_owner));
        provider_contracts.insert(
            (*role).to_string(),
            Value::Object(inner.into_iter().collect()),
        );
    }
    let mut trust_boundary: BTreeMap<String, Value> = BTreeMap::new();
    for (key, value) in MAPPING_TRUST_BOUNDARY {
        trust_boundary.insert((*key).to_string(), json!(value));
    }
    let mut table: BTreeMap<String, Value> = BTreeMap::new();
    table.insert("schema".to_string(), json!(MAPPING_SCHEMA));
    table.insert("mapping_version".to_string(), json!(MAPPING_VERSION));
    table.insert(
        "canonical_roles".to_string(),
        Value::Array(CANONICAL_ROLES.iter().map(|role| json!(role)).collect()),
    );
    table.insert(
        "role_aliases".to_string(),
        Value::Object(role_aliases.into_iter().collect()),
    );
    table.insert(
        "ao2_provider_contracts".to_string(),
        Value::Object(provider_contracts.into_iter().collect()),
    );
    table.insert(
        "trust_boundary".to_string(),
        Value::Object(trust_boundary.into_iter().collect()),
    );
    Value::Object(table.into_iter().collect())
}

/// Stable sha256 hex of the canonicalized mapping table.
///
/// Computed as `sha256(serde_json::to_string(mapping_table()))`, which
/// matches Python's `sha256(json.dumps(mapping_table(), sort_keys=True,
/// separators=(",", ":")))` because all nested objects are `BTreeMap` and
/// `serde_json::to_string` emits no whitespace.
pub fn mapping_digest() -> String {
    let serialized =
        serde_json::to_string(&mapping_table()).expect("mapping_table serializes to JSON");
    sha256_hex(serialized.as_bytes())
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes =
        fs::read(path).with_context(|| format!("read runspec for digest: {}", path.display()))?;
    Ok(sha256_hex(bytes))
}

/// Parse a RunSpec file. Accepts both YAML and JSON because YAML is a
/// superset of JSON and `serde_yaml::from_str` handles both.
pub fn load_runspec(path: &Path) -> Result<Value> {
    let text =
        fs::read_to_string(path).with_context(|| format!("read RunSpec {}", path.display()))?;
    let value: Value = serde_yaml::from_str(&text)
        .with_context(|| format!("parse RunSpec YAML {}", path.display()))?;
    if !value.is_object() {
        return Err(anyhow!(
            "RunSpec {} did not parse to a mapping",
            path.display()
        ));
    }
    Ok(value)
}

fn runspec_schema_field(runspec: &Value) -> Option<String> {
    if let Some(schema) = runspec.get("schema").and_then(|value| value.as_str()) {
        return Some(schema.to_string());
    }
    runspec
        .get("apiVersion")
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
}

fn runspec_name_field(runspec: &Value) -> Option<String> {
    if let Some(name) = runspec
        .get("metadata")
        .and_then(|meta| meta.get("name"))
        .and_then(|value| value.as_str())
    {
        return Some(name.to_string());
    }
    runspec
        .get("slug")
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
}

fn json_bool_field(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|field| field.as_bool())
}

fn sha256_text(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

fn is_env_key_name(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_uppercase() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_uppercase() || ch.is_ascii_digit())
}

fn ensure_no_secret_like_profile_text(field: &str, value: &str) -> Result<()> {
    let lowered = value.to_ascii_lowercase();
    let secret_markers = [
        "api_key=",
        "apikey=",
        "password=",
        "passwd=",
        "secret=",
        "token=",
        "bearer ",
        "authorization:",
        "ghp_",
        "sk-",
    ];
    if secret_markers.iter().any(|marker| lowered.contains(marker)) {
        return Err(anyhow!(
            "factory profile field {field} contains secret-like material and cannot be emitted as bridge evidence"
        ));
    }
    Ok(())
}

fn factory_profile_string_array_summary(role: &Value, field: &str) -> Result<Value> {
    let values = json_string_array_field(role, field);
    for value in &values {
        ensure_no_secret_like_profile_text(field, value)?;
    }
    Ok(json!({
        "count": values.len(),
        "sha256": sha256_text(&values.join("\n")),
    }))
}

fn factory_profile_role_ref(role: &Value, role_index: usize) -> Result<(String, Value)> {
    let id = role
        .get("id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!("factory profile role at index {role_index} is missing string id"))?
        .to_string();
    ensure_no_secret_like_profile_text("roles[].id", &id)?;
    let mut block: BTreeMap<String, Value> = BTreeMap::new();
    block.insert("owner".to_string(), json!("ao2"));
    block.insert("factory_v3_required_to_load".to_string(), json!(false));
    block.insert("id".to_string(), json!(id.clone()));
    if let Some(role_name) = json_string_field(role, "role") {
        ensure_no_secret_like_profile_text("roles[].role", &role_name)?;
        block.insert("role".to_string(), json!(role_name));
    }
    if let Some(provider_key) = json_string_field(role, "provider_key") {
        if !is_env_key_name(&provider_key) || !redacted_env_keys([provider_key.as_str()]).is_empty()
        {
            return Err(anyhow!(
                "factory profile role {id} provider_key must be a non-secret env-var name, not a provider API key or token value"
            ));
        }
        block.insert("provider_key".to_string(), json!(provider_key));
        block.insert("provider_key_value_exposed".to_string(), json!(false));
        block.insert(
            "provider_auth_contract".to_string(),
            json!("local_oauth_cli_only_no_provider_api_keys"),
        );
    }
    for field in ["deps", "reads", "writes", "skills"] {
        let values = json_string_array_field(role, field);
        for value in &values {
            ensure_no_secret_like_profile_text(field, value)?;
        }
        block.insert(
            field.to_string(),
            Value::Array(values.into_iter().map(|item| json!(item)).collect()),
        );
    }
    block.insert(
        "instructions_count".to_string(),
        json!(json_string_array_field(role, "instructions").len()),
    );
    if let Some(is_mutator) = json_bool_field(role, "is_mutator") {
        block.insert("is_mutator".to_string(), json!(is_mutator));
    }
    if let Some(deterministic) = json_bool_field(role, "deterministic") {
        block.insert("deterministic".to_string(), json!(deterministic));
        block.insert(
            "replay_command_summary".to_string(),
            factory_profile_string_array_summary(role, "replay_command")?,
        );
        block.insert(
            "replay_outputs_summary".to_string(),
            factory_profile_string_array_summary(role, "replay_outputs")?,
        );
    }
    Ok((id, Value::Object(block.into_iter().collect())))
}

fn profile_reference_block(path: &Path) -> Result<(Value, BTreeMap<String, Value>)> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read factory profile {}", path.display()))?;
    let profile: Value = serde_yaml::from_str(&text)
        .with_context(|| format!("parse factory profile {}", path.display()))?;
    if !profile.is_object() {
        return Err(anyhow!(
            "factory profile {} did not parse to a mapping",
            path.display()
        ));
    }
    let mut role_refs: BTreeMap<String, Value> = BTreeMap::new();
    if let Some(roles) = profile.get("roles").and_then(|value| value.as_array()) {
        for (role_index, role) in roles.iter().enumerate() {
            let (id, role_ref) = factory_profile_role_ref(role, role_index)?;
            if role_refs.insert(id.clone(), role_ref).is_some() {
                return Err(anyhow!(
                    "factory profile {} contains duplicate role id {id}",
                    path.display()
                ));
            }
        }
    }

    let mut block: BTreeMap<String, Value> = BTreeMap::new();
    block.insert("owner".to_string(), json!("ao2"));
    block.insert("path".to_string(), json!(path.display().to_string()));
    block.insert("sha256".to_string(), json!(sha256_file(path)?));
    block.insert("factory_v3_required_to_load".to_string(), json!(false));
    block.insert("provider_key_values_exposed".to_string(), json!(false));
    block.insert(
        "provider_auth_contract".to_string(),
        json!("local_oauth_cli_only_no_provider_api_keys"),
    );
    block.insert(
        "schema".to_string(),
        profile
            .get("schema")
            .and_then(|value| value.as_str())
            .map_or(Value::Null, |value| json!(value)),
    );
    block.insert(
        "version".to_string(),
        profile.get("version").cloned().unwrap_or(Value::Null),
    );
    if let Some(profile_name) = profile.get("profile").and_then(|value| value.as_str()) {
        ensure_no_secret_like_profile_text("profile", profile_name)?;
        block.insert("profile".to_string(), json!(profile_name));
    } else {
        block.insert("profile".to_string(), Value::Null);
    }
    if let Some(description) = profile.get("description").and_then(|value| value.as_str()) {
        ensure_no_secret_like_profile_text("description", description)?;
        block.insert("description_present".to_string(), json!(true));
        block.insert(
            "description_sha256".to_string(),
            json!(sha256_text(description)),
        );
    } else {
        block.insert("description_present".to_string(), json!(false));
    }
    block.insert(
        "common_instructions_count".to_string(),
        json!(json_string_array_field(&profile, "common_instructions").len()),
    );
    block.insert("role_count".to_string(), json!(role_refs.len()));
    block.insert(
        "role_ids".to_string(),
        Value::Array(role_refs.keys().map(|id| json!(id)).collect()),
    );
    block.insert(
        "profile_role_contracts".to_string(),
        Value::Object(role_refs.clone().into_iter().collect()),
    );
    Ok((Value::Object(block.into_iter().collect()), role_refs))
}

const VALID_CLASSIFICATIONS: &[&str] = &["TRIVIAL", "MODERATE", "COMPLEX"];
const VALID_SHAPES: &[&str] = &["greenfield", "bug-fix", "refactor"];

fn normalize_classification(value: &str) -> Option<String> {
    match value.trim().to_ascii_uppercase().as_str() {
        "TRIVIAL" | "SMALL" => Some("TRIVIAL".to_string()),
        "MODERATE" | "MEDIUM" => Some("MODERATE".to_string()),
        "COMPLEX" | "LARGE" => Some("COMPLEX".to_string()),
        _ => None,
    }
}

fn normalize_shape(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    if VALID_SHAPES.contains(&normalized.as_str()) {
        Some(normalized)
    } else {
        None
    }
}

fn first_known_token(text: &str, tokens: &[&str], case_sensitive: bool) -> Option<String> {
    if case_sensitive {
        for token in tokens {
            if text.contains(token) {
                return Some((*token).to_string());
            }
        }
        return None;
    }
    let lowered = text.to_ascii_lowercase();
    for token in tokens {
        if lowered.contains(&token.to_ascii_lowercase()) {
            return Some((*token).to_string());
        }
    }
    None
}

fn classification_from_json(value: &Value) -> (Option<String>, Option<String>) {
    let classification = value
        .get("classification")
        .or_else(|| value.get("size"))
        .and_then(|value| value.as_str())
        .and_then(normalize_classification);
    let shape = value
        .get("shape")
        .and_then(|value| value.as_str())
        .and_then(normalize_shape);
    (classification, shape)
}

fn classification_from_text(text: &str) -> (Option<String>, Option<String>) {
    let classification = first_known_token(text, VALID_CLASSIFICATIONS, true)
        .or_else(|| first_known_token(text, &["small", "medium", "large"], false))
        .and_then(|value| normalize_classification(&value))
        .or_else(|| Some(heuristic_classification(text)));
    let shape = first_known_token(text, VALID_SHAPES, false)
        .and_then(|value| normalize_shape(&value))
        .or_else(|| Some(heuristic_shape(text)));
    (classification, shape)
}

fn heuristic_shape(text: &str) -> String {
    let lowered = text.to_ascii_lowercase();
    if [
        "bug",
        "fix",
        "failure",
        "failing",
        "regression",
        "error",
        "broken",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "bug-fix".to_string()
    } else if [
        "refactor",
        "rename",
        "restructure",
        "migrate",
        "migration",
        "compatibility",
        "parity",
        "replace",
        "replacement",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "refactor".to_string()
    } else {
        "greenfield".to_string()
    }
}

fn heuristic_classification(text: &str) -> String {
    let lowered = text.to_ascii_lowercase();
    if [
        "windows",
        "ubuntu",
        "macos",
        "cross-platform",
        "three-os",
        "release",
        "provider",
        "governed execution",
        "replacement parity",
        "evaluator",
        "control-plane",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
        || text.len() > 1200
    {
        "COMPLEX".to_string()
    } else if [
        "runspec",
        "profile",
        "role contract",
        "workflow",
        "evidence",
        "queue",
        "resume",
        "repair",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
        || text.len() > 400
    {
        "MODERATE".to_string()
    } else {
        "TRIVIAL".to_string()
    }
}

fn classification_signals(text: &str) -> Vec<String> {
    let lowered = text.to_ascii_lowercase();
    let mut signals = Vec::new();
    for (needle, signal) in [
        ("bug", "bug_language"),
        ("fix", "fix_language"),
        ("refactor", "refactor_language"),
        ("parity", "replacement_parity"),
        ("replace", "replacement_language"),
        ("provider", "provider_orchestration"),
        ("windows", "three_os_or_windows"),
        ("ubuntu", "three_os_or_ubuntu"),
        ("macos", "three_os_or_macos"),
        ("release", "release_gate"),
        ("governed", "governed_execution"),
        ("runspec", "runspec_compatibility"),
        ("profile", "profile_compatibility"),
        ("evidence", "evidence_contract"),
    ] {
        if lowered.contains(needle) {
            signals.push(signal.to_string());
        }
    }
    if signals.is_empty() {
        signals.push("default_intake".to_string());
    }
    signals
}

fn work_request_block(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read work request {}", path.display()))?;
    let parsed_json = serde_json::from_str::<Value>(&text).ok();
    let (classification, shape) = match parsed_json.as_ref() {
        Some(value) if value.is_object() => classification_from_json(value),
        _ => classification_from_text(&text),
    };
    let classification_status = if classification.is_some() && shape.is_some() {
        "classified"
    } else {
        "blocked_missing_classification_or_shape"
    };

    let mut block: BTreeMap<String, Value> = BTreeMap::new();
    block.insert("path".to_string(), json!(path.display().to_string()));
    block.insert("sha256".to_string(), json!(sha256_file(path)?));
    block.insert(
        "classification".to_string(),
        classification.map_or(Value::Null, |value| json!(value)),
    );
    block.insert(
        "shape".to_string(),
        shape.map_or(Value::Null, |value| json!(value)),
    );
    block.insert(
        "classification_status".to_string(),
        json!(classification_status),
    );
    block.insert(
        "classification_owner".to_string(),
        json!("ao2-native-classifier"),
    );
    block.insert(
        "factory_v3_required_before_classification".to_string(),
        json!(false),
    );
    block.insert(
        "classification_signals".to_string(),
        Value::Array(
            classification_signals(&text)
                .into_iter()
                .map(|signal| json!(signal))
                .collect(),
        ),
    );
    Ok(Value::Object(block.into_iter().collect()))
}

fn provider_adapter_family(source_task: Option<&RunSpecRoleTask>) -> &'static str {
    let Some(provider) = source_task.and_then(|task| task.provider.as_deref()) else {
        return "local_scripted";
    };
    match provider.trim().to_ascii_lowercase().as_str() {
        "codex" | "openai-codex" => "codex_cli_oauth",
        "claude" | "claude-code" | "anthropic-claude" => "claude_code_oauth",
        "scripted" | "local" | "shell" | "script" => "local_scripted",
        _ => "declared_provider_adapter",
    }
}

fn provider_auth_contract(adapter_family: &str) -> &'static str {
    match adapter_family {
        "codex_cli_oauth" | "claude_code_oauth" => "local_oauth_cli_only_no_provider_api_keys",
        "local_scripted" => "local_scripted_no_secret_env_values",
        _ => "declared_provider_must_satisfy_ao2_secret_redaction_contract",
    }
}

fn provider_adapter_contract_block(role: &Value, source_task: Option<&RunSpecRoleTask>) -> Value {
    let adapter_family = provider_adapter_family(source_task);
    let mut block: BTreeMap<String, Value> = BTreeMap::new();
    block.insert("owner".to_string(), json!("ao2"));
    block.insert("adapter_family".to_string(), json!(adapter_family));
    block.insert(
        "auth_contract".to_string(),
        json!(provider_auth_contract(adapter_family)),
    );
    block.insert(
        "evidence_contract".to_string(),
        role["evidence_obligation"].clone(),
    );
    block.insert(
        "concern_contract".to_string(),
        json!("concerns_recorded_or_empty"),
    );
    block.insert(
        "blocker_contract".to_string(),
        json!("blockers_resolved_or_explicitly_blocked"),
    );
    block.insert(
        "changed_files_contract".to_string(),
        json!("changed_files_digest_recorded"),
    );
    block.insert("sandbox_contract".to_string(), role["sandbox"].clone());
    block.insert(
        "secret_redaction_contract".to_string(),
        json!("secret_redaction_summary_recorded"),
    );
    block.insert("provider_api_key_auth_allowed".to_string(), json!(false));
    block.insert("factory_v3_required_to_enforce".to_string(), json!(false));
    Value::Object(block.into_iter().collect())
}

fn factory_runspec_task_block(source_task: &RunSpecRoleTask) -> Value {
    let mut block: BTreeMap<String, Value> = BTreeMap::new();
    block.insert("kind".to_string(), json!(source_task.kind));
    if let Some(provider) = &source_task.provider {
        block.insert("provider".to_string(), json!(provider));
    }
    if let Some(agent) = &source_task.agent {
        block.insert("agent".to_string(), json!(agent));
    }
    if let Some(prompt_file) = &source_task.prompt_file {
        block.insert("prompt_file".to_string(), json!(prompt_file));
    }
    if let Some(workspace) = &source_task.workspace {
        block.insert("workspace".to_string(), json!(workspace));
    }
    if let Some(policy_profile) = &source_task.policy_profile {
        block.insert("policy_profile".to_string(), json!(policy_profile));
    }
    if let Some(dispatch_authorized) = source_task.dispatch_authorized {
        block.insert(
            "dispatch_authorized".to_string(),
            json!(dispatch_authorized),
        );
    }
    Value::Object(block.into_iter().collect())
}

fn profile_role_ref_for<'a>(
    profile_role_refs: &'a BTreeMap<String, Value>,
    source_task: &RunSpecRoleTask,
) -> Option<&'a Value> {
    if let Some(exact) = profile_role_refs.get(&source_task.role_id) {
        return Some(exact);
    }
    let source_canonical = canonical_role(&source_task.role_id).ok()?;
    profile_role_refs
        .iter()
        .find_map(|(profile_role_id, role_ref)| {
            (canonical_role(profile_role_id.as_str()).ok() == Some(source_canonical))
                .then_some(role_ref)
        })
}

fn governed_run_plan(
    resolved_roles: &[Value],
    source_tasks: &[RunSpecRoleTask],
    role_contract_refs: &BTreeMap<String, Value>,
    profile_role_refs: &BTreeMap<String, Value>,
) -> Value {
    let tasks: Vec<Value> = resolved_roles
        .iter()
        .enumerate()
        .map(|(index, role)| {
            let mut task: BTreeMap<String, Value> = BTreeMap::new();
            let source_task = source_tasks
                .get(index)
                .filter(|source_task| source_task.role_id == role["role_id"]);
            task.insert("sequence".to_string(), json!(index + 1));
            task.insert("role_id".to_string(), role["role_id"].clone());
            task.insert("canonical_role".to_string(), role["canonical_role"].clone());
            task.insert(
                "provider_contract".to_string(),
                role["ao2_provider_contract_slug"].clone(),
            );
            task.insert("sandbox".to_string(), role["sandbox"].clone());
            task.insert(
                "evidence_obligation".to_string(),
                role["evidence_obligation"].clone(),
            );
            task.insert("closure_owner".to_string(), role["closure_owner"].clone());
            task.insert(
                "provider_adapter_contract".to_string(),
                provider_adapter_contract_block(role, source_task),
            );
            task.insert(
                "depends_on".to_string(),
                source_task.map_or_else(
                    || Value::Array(Vec::new()),
                    |source_task| {
                        Value::Array(
                            source_task
                                .depends_on
                                .iter()
                                .map(|dep| json!(dep))
                                .collect(),
                        )
                    },
                ),
            );
            if let Some(source_task) = source_task {
                task.insert(
                    "factory_runspec_task".to_string(),
                    factory_runspec_task_block(source_task),
                );
                if let Some(role_contract_ref) = role_contract_refs.get(&source_task.role_id) {
                    task.insert("role_contract_ref".to_string(), role_contract_ref.clone());
                }
                if let Some(profile_role_ref) = profile_role_ref_for(profile_role_refs, source_task)
                {
                    task.insert("profile_role_ref".to_string(), profile_role_ref.clone());
                }
            }
            Value::Object(task.into_iter().collect())
        })
        .collect();

    let mut plan: BTreeMap<String, Value> = BTreeMap::new();
    plan.insert("schema".to_string(), json!("ao2.governed-run-plan.v1"));
    plan.insert("status".to_string(), json!("materialized_dry_run"));
    plan.insert(
        "decision_owner".to_string(),
        json!("ao2_native_evaluator_closer"),
    );
    plan.insert(
        "factory_v3_decision_owner".to_string(),
        json!("parity_oracle_only"),
    );
    plan.insert(
        "native_gates".to_string(),
        governed_run_native_gates(&tasks),
    );
    plan.insert("tasks".to_string(), Value::Array(tasks));
    Value::Object(plan.into_iter().collect())
}

fn governed_run_native_gates(tasks: &[Value]) -> Value {
    let task_ids: Vec<String> = tasks
        .iter()
        .filter_map(|task| task.get("role_id").and_then(|value| value.as_str()))
        .map(ToString::to_string)
        .collect();
    let canonical_roles: Vec<String> = tasks
        .iter()
        .filter_map(|task| task.get("canonical_role").and_then(|value| value.as_str()))
        .map(ToString::to_string)
        .collect();
    let reviewer_task_ids: Vec<String> = tasks
        .iter()
        .filter(|task| {
            task.get("canonical_role").and_then(|value| value.as_str()) == Some("reviewer")
        })
        .filter_map(|task| task.get("role_id").and_then(|value| value.as_str()))
        .map(ToString::to_string)
        .collect();

    let has_implementer = canonical_roles.iter().any(|role| role == "implementer");
    let has_reviewer = !reviewer_task_ids.is_empty();
    let has_evaluator_closer = canonical_roles
        .iter()
        .any(|role| role == "evaluator_closer");

    let midpoint_prerequisites = if has_implementer {
        vec!["implementation_digest_patch_and_test_evidence"]
    } else {
        Vec::new()
    };
    let closure_prerequisites = vec![
        "role_evidence_obligations_satisfied",
        "concerns_recorded_or_empty",
        "blockers_resolved_or_explicitly_blocked",
        "changed_files_digest_recorded",
        "secret_redaction_summary_recorded",
    ];

    let mut midpoint: BTreeMap<String, Value> = BTreeMap::new();
    midpoint.insert("stage".to_string(), json!("midpoint"));
    midpoint.insert("owner".to_string(), json!("ao2_native_evaluator_closer"));
    midpoint.insert(
        "status".to_string(),
        json!(if has_implementer && has_reviewer {
            "planned"
        } else {
            "not_applicable_for_runspec"
        }),
    );
    midpoint.insert("factory_v3_role".to_string(), json!("parity_oracle_only"));
    midpoint.insert(
        "required_before_roles".to_string(),
        Value::Array(
            reviewer_task_ids
                .into_iter()
                .map(|role_id| json!(role_id))
                .collect(),
        ),
    );
    midpoint.insert(
        "required_evidence".to_string(),
        Value::Array(
            midpoint_prerequisites
                .into_iter()
                .map(|value| json!(value))
                .collect(),
        ),
    );
    midpoint.insert(
        "decision_logic".to_string(),
        json!("continue_when_required_evidence_present_and_no_open_blockers_else_repair_or_block"),
    );
    midpoint.insert(
        "emits".to_string(),
        json!("ao2.obligation-gate.midpoint.v1"),
    );

    let mut closure: BTreeMap<String, Value> = BTreeMap::new();
    closure.insert("stage".to_string(), json!("closure"));
    closure.insert("owner".to_string(), json!("ao2_native_evaluator_closer"));
    closure.insert(
        "status".to_string(),
        json!(if has_evaluator_closer {
            "planned"
        } else {
            "blocked_missing_evaluator_closer_role"
        }),
    );
    closure.insert("factory_v3_role".to_string(), json!("parity_oracle_only"));
    closure.insert(
        "required_after_roles".to_string(),
        Value::Array(task_ids.into_iter().map(|role_id| json!(role_id)).collect()),
    );
    closure.insert(
        "required_evidence".to_string(),
        Value::Array(
            closure_prerequisites
                .into_iter()
                .map(|value| json!(value))
                .collect(),
        ),
    );
    closure.insert(
        "decision_logic".to_string(),
        json!(
            "accept_when_all_role_obligations_satisfied_and_no_open_blockers_else_repair_or_reject"
        ),
    );
    closure.insert(
        "emits".to_string(),
        json!("ao2.evaluator-closer-decision.v1"),
    );

    Value::Array(vec![
        Value::Object(midpoint.into_iter().collect()),
        Value::Object(closure.into_iter().collect()),
    ])
}

fn toml_string_array_field(value: &toml::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|field| field.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn role_contract_candidates(
    dir: &Path,
    source_task: &RunSpecRoleTask,
    canonical_role: &str,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for stem in [
        normalize(&source_task.role_id),
        canonical_role.replace('_', "-"),
        match canonical_role {
            "reviewer" => "slice-reviewer".to_string(),
            other => other.replace('_', "-"),
        },
    ] {
        let path = dir.join(format!("{stem}.toml"));
        if !candidates.contains(&path) {
            candidates.push(path);
        }
    }
    candidates
}

fn load_role_contract_ref(
    dir: &Path,
    source_task: &RunSpecRoleTask,
    canonical_role: &str,
) -> Result<Option<Value>> {
    let Some(path) = role_contract_candidates(dir, source_task, canonical_role)
        .into_iter()
        .find(|path| path.is_file())
    else {
        return Ok(None);
    };
    let text = fs::read_to_string(&path)
        .with_context(|| format!("read role contract {}", path.display()))?;
    let parsed: toml::Value = toml::from_str(&text)
        .with_context(|| format!("parse role contract TOML {}", path.display()))?;
    let mut block: BTreeMap<String, Value> = BTreeMap::new();
    block.insert("contract_status".to_string(), json!("loaded"));
    block.insert("path".to_string(), json!(path.display().to_string()));
    block.insert("sha256".to_string(), json!(sha256_file(&path)?));
    block.insert(
        "name".to_string(),
        parsed
            .get("name")
            .and_then(|field| field.as_str())
            .map_or(Value::Null, |name| json!(name)),
    );
    block.insert(
        "description".to_string(),
        parsed
            .get("description")
            .and_then(|field| field.as_str())
            .map_or(Value::Null, |description| json!(description)),
    );
    block.insert(
        "inputs".to_string(),
        Value::Array(
            toml_string_array_field(&parsed, "inputs")
                .into_iter()
                .map(|item| json!(item))
                .collect(),
        ),
    );
    block.insert(
        "outputs".to_string(),
        Value::Array(
            toml_string_array_field(&parsed, "outputs")
                .into_iter()
                .map(|item| json!(item))
                .collect(),
        ),
    );
    block.insert(
        "status_required".to_string(),
        parsed
            .get("status_required")
            .and_then(|field| field.as_bool())
            .map_or(Value::Null, |status_required| json!(status_required)),
    );
    block.insert("owner".to_string(), json!("ao2"));
    block.insert("factory_v3_required_to_load".to_string(), json!(false));
    Ok(Some(Value::Object(block.into_iter().collect())))
}

fn role_contract_refs_block(
    dir: Option<&Path>,
    source_tasks: &[RunSpecRoleTask],
    discovery: Option<&str>,
) -> Result<(BTreeMap<String, Value>, Option<Value>)> {
    let Some(dir) = dir else {
        return Ok((BTreeMap::new(), None));
    };
    let mut refs = BTreeMap::new();
    let mut missing_roles = Vec::new();
    for source_task in source_tasks {
        let canonical =
            canonical_role(&source_task.role_id).unwrap_or(source_task.role_id.as_str());
        if let Some(role_contract_ref) = load_role_contract_ref(dir, source_task, canonical)? {
            refs.insert(source_task.role_id.clone(), role_contract_ref);
        } else {
            missing_roles.push(source_task.role_id.clone());
        }
    }
    let mut block: BTreeMap<String, Value> = BTreeMap::new();
    block.insert("owner".to_string(), json!("ao2"));
    block.insert("path".to_string(), json!(dir.display().to_string()));
    block.insert("factory_v3_required_to_load".to_string(), json!(false));
    if let Some(discovery) = discovery {
        block.insert("discovery".to_string(), json!(discovery));
    }
    block.insert("loaded_count".to_string(), json!(refs.len()));
    block.insert(
        "missing_roles".to_string(),
        Value::Array(missing_roles.into_iter().map(|role| json!(role)).collect()),
    );
    Ok((refs, Some(Value::Object(block.into_iter().collect()))))
}

fn infer_ao_operator_role_contracts_dir(runspec_path: &Path) -> Option<PathBuf> {
    let runspecs_dir = runspec_path.parent()?;
    if runspecs_dir.file_name()?.to_str()? != "runspecs" {
        return None;
    }
    let ao_dir = runspecs_dir.parent()?;
    if ao_dir.file_name()?.to_str()? != "ao" {
        return None;
    }
    let repo_root = ao_dir.parent()?;
    let agents_dir = repo_root.join("agents");
    if agents_dir.is_dir() {
        Some(agents_dir)
    } else {
        None
    }
}

fn redacted_env_keys<I, S>(env_keys: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut matches: Vec<String> = Vec::new();
    for key in env_keys {
        let upper = key.as_ref().to_ascii_uppercase();
        if REDACTED_ENV_SUBSTRINGS
            .iter()
            .any(|substring| upper.contains(substring))
        {
            matches.push(key.as_ref().to_string());
        }
    }
    matches.sort();
    matches.dedup();
    matches
}

/// Read the process environment and return the subset of variable names
/// whose upper-cased form contains any of `REDACTED_ENV_SUBSTRINGS`. Values
/// are never read or returned.
pub fn redacted_env_keys_from_process() -> Vec<String> {
    redacted_env_keys(std::env::vars_os().filter_map(|(key, _value)| key.into_string().ok()))
}

pub struct BridgeOptions<'a> {
    pub runspec_path: &'a Path,
    pub work_request_path: Option<&'a Path>,
    pub profile_path: Option<&'a Path>,
    pub role_contracts_dir: Option<&'a Path>,
    pub now_ms: Option<i64>,
    pub env_keys_override: Option<Vec<String>>,
}

/// Build the AO2-native bridge-evidence JSON. Mapping-only dry-run; no
/// downstream `ao2 factory plan` invocation. The shape is alphabetized by
/// every nested `BTreeMap`, so serializing this value produces deterministic
/// output ready for hashing or signing downstream.
pub fn build_bridge_evidence(options: BridgeOptions<'_>) -> Result<Value> {
    let runspec_value = load_runspec(options.runspec_path)?;
    let runspec_sha = sha256_file(options.runspec_path)?;
    let source_tasks = extract_role_tasks(&runspec_value);
    let inferred_role_contracts_dir = if options.role_contracts_dir.is_none() {
        infer_ao_operator_role_contracts_dir(options.runspec_path)
    } else {
        None
    };
    let role_contracts_dir = options
        .role_contracts_dir
        .or(inferred_role_contracts_dir.as_deref());
    let role_contract_discovery = if options.role_contracts_dir.is_some() {
        Some("explicit_role_contracts_dir")
    } else if inferred_role_contracts_dir.is_some() {
        Some("auto_discovered_from_ao_runspec_layout")
    } else {
        None
    };
    let (role_contract_refs, role_contracts_block) =
        role_contract_refs_block(role_contracts_dir, &source_tasks, role_contract_discovery)?;

    let mut resolved: Vec<Value> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for source_task in &source_tasks {
        match resolve_role(&source_task.role_id) {
            Ok(record) => resolved.push(record),
            Err(_) => unknown.push(source_task.role_id.clone()),
        }
    }

    let status = if unknown.is_empty() {
        "mapping_resolved_dry_run"
    } else {
        "blocked_unknown_roles"
    };

    let env_keys = options
        .env_keys_override
        .unwrap_or_else(redacted_env_keys_from_process);

    let timestamp = options
        .now_ms
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let generated_at = generated_at_iso(timestamp);

    let mut input_runspec: BTreeMap<String, Value> = BTreeMap::new();
    input_runspec.insert(
        "path".to_string(),
        json!(options.runspec_path.display().to_string()),
    );
    input_runspec.insert("sha256".to_string(), json!(runspec_sha));
    input_runspec.insert(
        "schema".to_string(),
        match runspec_schema_field(&runspec_value) {
            Some(value) => json!(value),
            None => Value::Null,
        },
    );
    input_runspec.insert(
        "name".to_string(),
        match runspec_name_field(&runspec_value) {
            Some(value) => json!(value),
            None => Value::Null,
        },
    );

    let mut mapping_block: BTreeMap<String, Value> = BTreeMap::new();
    mapping_block.insert("schema".to_string(), json!(MAPPING_SCHEMA));
    mapping_block.insert("version".to_string(), json!(MAPPING_VERSION));
    mapping_block.insert("digest".to_string(), json!(mapping_digest()));

    let mut trust_boundary: BTreeMap<String, Value> = BTreeMap::new();
    for (key, value) in BRIDGE_TRUST_BOUNDARY {
        trust_boundary.insert((*key).to_string(), json!(value));
    }

    let mut evidence: BTreeMap<String, Value> = BTreeMap::new();
    evidence.insert("schema".to_string(), json!(BRIDGE_SCHEMA));
    evidence.insert("action".to_string(), json!(BRIDGE_ACTION));
    evidence.insert("generated_at".to_string(), json!(generated_at));
    evidence.insert("produced_at_ms".to_string(), json!(timestamp));
    evidence.insert("status".to_string(), json!(status));
    evidence.insert(
        "trust_boundary".to_string(),
        Value::Object(trust_boundary.into_iter().collect()),
    );
    evidence.insert(
        "input_runspec".to_string(),
        Value::Object(input_runspec.into_iter().collect()),
    );
    evidence.insert(
        "mapping".to_string(),
        Value::Object(mapping_block.into_iter().collect()),
    );
    if let Some(work_request_path) = options.work_request_path {
        evidence.insert(
            "work_request".to_string(),
            work_request_block(work_request_path)?,
        );
    }
    let profile_role_refs = if let Some(profile_path) = options.profile_path {
        let (profile_reference, profile_role_refs) = profile_reference_block(profile_path)?;
        evidence.insert("profile_reference".to_string(), profile_reference);
        profile_role_refs
    } else {
        BTreeMap::new()
    };
    evidence.insert(
        "governed_run_plan".to_string(),
        governed_run_plan(
            &resolved,
            &source_tasks,
            &role_contract_refs,
            &profile_role_refs,
        ),
    );
    if let Some(role_contracts_block) = role_contracts_block {
        evidence.insert("role_contracts".to_string(), role_contracts_block);
    }
    evidence.insert("resolved_roles".to_string(), Value::Array(resolved));
    evidence.insert(
        "unknown_roles".to_string(),
        Value::Array(unknown.into_iter().map(|id| json!(id)).collect()),
    );
    evidence.insert(
        "redacted_env_keys_observed".to_string(),
        Value::Array(env_keys.into_iter().map(|key| json!(key)).collect()),
    );

    Ok(Value::Object(evidence.into_iter().collect()))
}

fn generated_at_iso(now_ms: i64) -> String {
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms)
        .unwrap_or_else(chrono::Utc::now)
        .with_timezone(&chrono::Utc);
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Static-table fail-fast checks. Called from `tests` and from the CLI handler
/// to fail loudly if a canonical role lacks a contract or vice versa.
pub fn audit_static_tables() -> Result<()> {
    let mut missing: Vec<&str> = Vec::new();
    for role in CANONICAL_ROLES {
        if !AO2_PROVIDER_CONTRACTS
            .iter()
            .any(|(contract_role, _)| contract_role == role)
        {
            missing.push(*role);
        }
    }
    if !missing.is_empty() {
        return Err(anyhow!(
            "canonical roles missing AO2 provider contract: {}",
            {
                missing.sort();
                missing.join(", ")
            }
        ));
    }
    let mut extra: Vec<&str> = Vec::new();
    for (role, _) in AO2_PROVIDER_CONTRACTS {
        if !CANONICAL_ROLES.contains(role) {
            extra.push(*role);
        }
    }
    if !extra.is_empty() {
        return Err(anyhow!(
            "AO2 provider contracts not in CANONICAL_ROLES: {}",
            {
                extra.sort();
                extra.join(", ")
            }
        ));
    }
    let canonical_set: std::collections::HashSet<&&str> = CANONICAL_ROLES.iter().collect();
    let mut unknown_targets: Vec<&str> = Vec::new();
    for (_, target) in ROLE_ALIASES {
        if !canonical_set.contains(target) {
            unknown_targets.push(*target);
        }
    }
    if !unknown_targets.is_empty() {
        unknown_targets.sort();
        unknown_targets.dedup();
        return Err(anyhow!(
            "ROLE_ALIASES targets not in CANONICAL_ROLES: {}",
            unknown_targets.join(", ")
        ));
    }
    Ok(())
}

/// CLI entry: emit the mapping table as pretty JSON for tooling that wants to
/// pin the table without recomputing it. Used by the integration test that
/// proves Rust and Python emit identical mappings.
pub fn mapping_table_pretty() -> Result<String> {
    Ok(serde_json::to_string_pretty(&mapping_table())? + "\n")
}

/// Pretty-print bridge evidence ready to be written to disk. Matches the
/// Python bridge's `json.dumps(value, indent=2, sort_keys=True) + "\n"` so
/// observers can diff evidence files across the two producers byte-for-byte
/// modulo timestamp and runspec-path differences.
pub fn evidence_pretty(value: &Value) -> Result<String> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    Ok(text)
}

#[allow(dead_code)]
pub fn redacted_env_keys_for_test(keys: &[&str]) -> Vec<String> {
    redacted_env_keys(keys.iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_static_tables_pass() {
        audit_static_tables().expect("static tables must be self-consistent");
    }

    #[test]
    fn canonicalizes_case_insensitively_and_normalizes_underscore_dash() {
        assert_eq!(canonical_role("intake").unwrap(), "intake");
        assert_eq!(canonical_role("Planner-Intake").unwrap(), "intake");
        assert_eq!(
            canonical_role("EVALUATOR_CLOSER").unwrap(),
            "evaluator_closer"
        );
        assert_eq!(
            canonical_role("agent-os-slice-reviewer").unwrap(),
            "reviewer"
        );
    }

    #[test]
    fn unknown_role_returns_error() {
        let err = canonical_role("ghost-role").unwrap_err().to_string();
        assert!(err.contains("ghost-role"), "error message: {err}");
    }

    #[test]
    fn extracts_factory_v3_runspec_roles_in_order() {
        let runspec = json!({
            "schema": "factory-v3/runspec/v1",
            "slug": "bug-fix",
            "roles": [
                {"id": "intake"},
                {"id": "planner"},
                {"id": "implementer"},
                {"id": "reviewer"},
                {"id": "evaluator-closer"},
            ],
        });
        let ids = extract_role_ids(&runspec);
        assert_eq!(
            ids,
            vec![
                "intake",
                "planner",
                "implementer",
                "reviewer",
                "evaluator-closer"
            ]
        );
    }

    #[test]
    fn extracts_ao_dev_v1_runspec_agent_tasks() {
        let runspec = json!({
            "apiVersion": "ao.dev/v1",
            "kind": "Run",
            "metadata": {"name": "factory-v3-smoke"},
            "spec": {
                "tasks": [
                    {"id": "planner-intake", "kind": "agent"},
                    {"id": "plan-hardener", "kind": "agent"},
                    {"id": "non-agent", "kind": "tool"},
                    {"id": "implementer-slice", "kind": "agent"},
                ]
            }
        });
        let ids = extract_role_ids(&runspec);
        assert_eq!(
            ids,
            vec!["planner-intake", "plan-hardener", "implementer-slice"]
        );
    }

    #[test]
    fn resolve_role_returns_provider_contract_record() {
        let record = resolve_role("implementer-slice").unwrap();
        let object = record.as_object().unwrap();
        assert_eq!(object["role_id"], json!("implementer-slice"));
        assert_eq!(object["canonical_role"], json!("implementer"));
        assert_eq!(
            object["ao2_provider_contract_slug"],
            json!("ao2.provider-contract.implementer.v1")
        );
        assert_eq!(
            object["sandbox"],
            json!("scoped_write_with_digest_patch_and_repair_budget")
        );
        assert_eq!(
            object["evidence_obligation"],
            json!("implementation_digest_patch_and_test_evidence")
        );
        assert_eq!(
            object["closure_owner"],
            json!("ao2_native_evaluator_closer")
        );
    }

    #[test]
    fn redacted_env_keys_finds_substring_matches_and_sorts() {
        let keys = redacted_env_keys_for_test(&[
            "ANTHROPIC_API_KEY",
            "PATH",
            "MY_SECRET",
            "github_token",
            "PASSWORD_FILE",
            "AUTHORIZED_KEYS",
        ]);
        assert_eq!(
            keys,
            vec![
                "ANTHROPIC_API_KEY".to_string(),
                "AUTHORIZED_KEYS".to_string(),
                "MY_SECRET".to_string(),
                "PASSWORD_FILE".to_string(),
                "github_token".to_string(),
            ]
        );
    }

    #[test]
    fn mapping_digest_matches_python_module_reference_value() {
        // Pinned to the value emitted by
        // factory-v3/scripts/ao_operator_ao2_provider_contract.py:digest on
        // 2026-05-25 against canonical roles + aliases + contracts.
        // If the mapping changes intentionally, regenerate via:
        //   python3 scripts/ao_operator_ao2_provider_contract.py digest
        // and update both sides in the same commit.
        assert_eq!(
            mapping_digest(),
            "cda521f5bd1ae42f06ab2f44689161034fa8790163b020ba888719312635cd99"
        );
    }
}
