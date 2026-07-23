use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use ao2_core::sha256_hex;
use ao2_policy::redact_secrets;

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn factory_ensure_target_repo(target: &Path) -> Result<()> {
    if !target.exists() {
        return Err(anyhow!("target repo does not exist: {}", target.display()));
    }
    Ok(())
}

pub(crate) fn read_factory_compat_value(path: &Path) -> Result<serde_json::Value> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let content = content.trim_start_matches('\u{feff}');
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("json") {
        return serde_json::from_str(content)
            .with_context(|| format!("parse json {}", path.display()));
    }
    if extension.eq_ignore_ascii_case("toml") {
        let value: toml::Value =
            toml::from_str(content).with_context(|| format!("parse toml {}", path.display()))?;
        return serde_json::to_value(value)
            .with_context(|| format!("convert toml {} to json value", path.display()));
    }
    serde_yaml::from_str(content).with_context(|| format!("parse yaml {}", path.display()))
}

pub(crate) fn reject_factory_provider_api_key_auth(
    source: &str,
    value: &serde_json::Value,
) -> Result<()> {
    let mut path = Vec::new();
    if factory_value_requests_api_key_auth(value, &mut path) {
        return Err(anyhow!(
            "provider API-key authentication is forbidden in factory-v3 compatibility {source}; use local OAuth CLI provider auth only"
        ));
    }
    Ok(())
}

fn factory_value_requests_api_key_auth(value: &serde_json::Value, path: &mut Vec<String>) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                path.push(key.to_ascii_lowercase());
                let key_is_auth_boundary = matches!(
                    key.to_ascii_lowercase().as_str(),
                    "auth"
                        | "authentication"
                        | "provider_auth"
                        | "provider_authentication"
                        | "env"
                        | "environment"
                        | "kind"
                        | "type"
                );
                if key_is_auth_boundary && factory_auth_value_is_forbidden(child) {
                    return true;
                }
                if factory_value_requests_api_key_auth(child, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                path.push(index.to_string());
                if factory_value_requests_api_key_auth(child, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        serde_json::Value::String(text) => {
            let under_auth_boundary = path.iter().any(|part| {
                matches!(
                    part.as_str(),
                    "auth"
                        | "authentication"
                        | "provider_auth"
                        | "provider_authentication"
                        | "env"
                        | "environment"
                        | "kind"
                        | "type"
                )
            });
            under_auth_boundary && factory_auth_string_is_forbidden(text)
        }
        _ => false,
    }
}

fn factory_auth_value_is_forbidden(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => factory_auth_string_is_forbidden(text),
        serde_json::Value::Array(items) => items.iter().any(factory_auth_value_is_forbidden),
        serde_json::Value::Object(map) => map.values().any(factory_auth_value_is_forbidden),
        _ => false,
    }
}

fn factory_auth_string_is_forbidden(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "api_key" | "apikey" | "openai_api_key" | "anthropic_api_key"
    ) || normalized.contains("bearer token")
}

pub(crate) fn factory_input_ref(kind: &str, path: &Path) -> Result<serde_json::Value> {
    let bytes = fs::read(path).with_context(|| format!("read {kind} {}", path.display()))?;
    Ok(serde_json::json!({
        "kind": kind,
        "path": path.display().to_string(),
        "sha256": sha256_hex(bytes)
    }))
}

pub(crate) struct FactoryStructuredClassification {
    pub(crate) size: String,
    pub(crate) shape: String,
    pub(crate) source: &'static str,
}

fn normalize_factory_request_size(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "small" | "trivial" => Some("small".to_string()),
        "medium" | "moderate" => Some("medium".to_string()),
        "large" | "complex" => Some("large".to_string()),
        _ => None,
    }
}

fn normalize_factory_request_shape(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "greenfield" | "bug-fix" | "refactor" => Some(normalized),
        _ => None,
    }
}

pub(crate) fn factory_structured_classification_override(
    request_value: &serde_json::Value,
) -> Option<FactoryStructuredClassification> {
    let size = factory_request_string(request_value, "size")
        .or_else(|| factory_request_string(request_value, "classification"))
        .and_then(|value| normalize_factory_request_size(&value));
    let shape = factory_request_string(request_value, "shape")
        .and_then(|value| normalize_factory_request_shape(&value));
    match (size, shape) {
        (Some(size), Some(shape)) => Some(FactoryStructuredClassification {
            size,
            shape,
            source: "structured_work_request",
        }),
        _ => None,
    }
}

pub(crate) fn classify_factory_shape(text: &str) -> &'static str {
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
    .any(|needle| text.contains(needle))
    {
        "bug-fix"
    } else if [
        "refactor",
        "rename",
        "restructure",
        "migrate",
        "compatibility",
        "parity",
    ]
    .iter()
    .any(|needle| text.contains(needle))
    {
        "refactor"
    } else {
        "greenfield"
    }
}

pub(crate) fn classify_factory_size(
    text: &str,
    has_profile: bool,
    has_runspec: bool,
    role_contract_count: usize,
) -> &'static str {
    if role_contract_count >= 4
        || [
            "windows",
            "ubuntu",
            "macos",
            "cross-platform",
            "release",
            "provider",
            "migration",
            "governed execution",
        ]
        .iter()
        .any(|needle| text.contains(needle))
    {
        "large"
    } else if role_contract_count >= 2 || has_profile || has_runspec || text.len() > 1200 {
        "medium"
    } else {
        "small"
    }
}

pub(crate) fn factory_classification_signals(text: &str) -> Vec<&'static str> {
    let mut signals = Vec::new();
    for (needle, signal) in [
        ("bug", "bug_language"),
        ("refactor", "refactor_language"),
        ("parity", "replacement_parity"),
        ("provider", "provider_orchestration"),
        ("windows", "three_os_or_windows"),
        ("ubuntu", "three_os_or_ubuntu"),
        ("macos", "three_os_or_macos"),
        ("release", "release_gate"),
        ("governed", "governed_execution"),
    ] {
        if text.contains(needle) {
            signals.push(signal);
        }
    }
    if signals.is_empty() {
        signals.push("default_intake");
    }
    signals
}

fn factory_default_roles() -> Vec<String> {
    [
        "planner",
        "implementer",
        "reviewer",
        "test-engineer",
        "evaluator-closer",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub(crate) fn factory_compat_roles(
    role_contract_values: &[serde_json::Value],
    runspec_value: Option<&serde_json::Value>,
    profile_value: Option<&serde_json::Value>,
) -> Vec<serde_json::Value> {
    if !role_contract_values.is_empty() {
        return role_contract_values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let path = json_string(value, "path");
                let contract = &value["contract"];
                let role_id = factory_request_string(contract, "name")
                    .or_else(|| factory_request_string(contract, "id"))
                    .unwrap_or_else(|| {
                        Path::new(&path)
                            .file_stem()
                            .and_then(|stem| stem.to_str())
                            .map(|stem| stem.replace([' ', '_'], "-"))
                            .filter(|stem| !stem.trim().is_empty())
                            .unwrap_or_else(|| format!("factory-role-{}", index + 1))
                    });
                serde_json::json!({
                    "role_id": role_id,
                    "source": "factory-v3-role-contract",
                    "contract_path": path,
                    "contract_sha256": json_string(value, "digest"),
                    "status_required": contract.get("status_required").and_then(|value| value.as_bool()).unwrap_or(true),
                    "inputs": contract.get("inputs").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "outputs": factory_normalized_role_outputs(contract.get("outputs"))
                })
            })
            .collect();
    }

    let profile_roles = factory_profile_roles(profile_value);
    if !profile_roles.is_empty() {
        return profile_roles
            .into_iter()
            .map(|role| {
                let id = role.get("id").and_then(|id| id.as_str()).unwrap_or("");
                serde_json::json!({
                    "role_id": id,
                    "role_name": role.get("role").and_then(|value| value.as_str()).unwrap_or(id),
                    "source": "factory-v3-profile-role",
                    "provider_profile": role.get("provider_key").and_then(|value| value.as_str()).unwrap_or("scripted"),
                    "deps": factory_json_string_array(role.get("deps")),
                    "reads": role.get("reads").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "writes": role.get("writes").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "instructions": role.get("instructions").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "status_required": true,
                    "outputs": factory_required_role_outputs()
                })
            })
            .collect();
    }

    let runspec_roles = factory_runspec_role_ids(runspec_value);
    if !runspec_roles.is_empty() {
        let role_source = if !factory_runspec_tasks(runspec_value).is_empty() {
            "factory-v3-runspec-task"
        } else {
            "factory-v3-legacy-runspec-role"
        };
        return runspec_roles
            .into_iter()
            .map(|role| {
                serde_json::json!({
                    "role_id": role,
                    "source": role_source,
                    "status_required": true,
                    "outputs": factory_required_role_outputs()
                })
            })
            .collect();
    }

    factory_default_roles()
        .into_iter()
        .map(|role| {
            serde_json::json!({
                "role_id": role,
                "source": "ao2-default-factory-compatible-contract",
                "status_required": true,
                "outputs": factory_required_role_outputs()
            })
        })
        .collect()
}

pub(crate) fn factory_role_contract_gate(roles: &[serde_json::Value]) -> serde_json::Value {
    let required_outputs = factory_required_role_outputs();
    let mut matrix = Vec::new();
    let mut missing_obligations = Vec::new();

    for role in roles {
        let role_id = role
            .get("role_id")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown-role");
        let status_required = role
            .get("status_required")
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        let outputs = role
            .get("outputs")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let missing_for_role = required_outputs
            .iter()
            .filter(|required| {
                !outputs
                    .iter()
                    .any(|output| output.as_str() == Some(**required))
            })
            .map(|required| (*required).to_string())
            .collect::<Vec<_>>();
        if !missing_for_role.is_empty() {
            missing_obligations.push(serde_json::json!({
                "role_id": role_id,
                "missing_outputs": missing_for_role
            }));
        }
        matrix.push(serde_json::json!({
            "role_id": role_id,
            "source": role.get("source").cloned().unwrap_or_else(|| serde_json::json!("unknown")),
            "status_required": status_required,
            "outputs": outputs,
            "satisfied": missing_for_role.is_empty()
        }));
    }

    serde_json::json!({
        "owner": "ao2-native-evaluator-closer",
        "factory_v3_role": "parity_oracle_only",
        "status": if missing_obligations.is_empty() { "satisfied_at_plan_time" } else { "blocked_missing_role_obligations" },
        "required_outputs": required_outputs,
        "role_count": roles.len(),
        "missing_obligations": missing_obligations,
        "matrix": matrix
    })
}

fn factory_required_role_outputs() -> Vec<&'static str> {
    vec![
        "evidence",
        "concerns",
        "blockers",
        "changed_files",
        "sandbox",
        "secret_redaction",
    ]
}

fn factory_normalized_role_outputs(outputs: Option<&serde_json::Value>) -> Vec<serde_json::Value> {
    let mut normalized = outputs
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    for required in factory_required_role_outputs() {
        if !normalized
            .iter()
            .any(|item| item.as_str() == Some(required))
        {
            normalized.push(serde_json::Value::String(required.to_string()));
        }
    }
    normalized
}

fn factory_runspec_tasks(runspec_value: Option<&serde_json::Value>) -> Vec<&serde_json::Value> {
    runspec_value
        .and_then(|runspec| runspec.get("spec"))
        .and_then(|spec| spec.get("tasks"))
        .and_then(|tasks| tasks.as_array())
        .map(|tasks| tasks.iter().collect::<Vec<_>>())
        .unwrap_or_default()
}

fn factory_legacy_runspec_roles(
    runspec_value: Option<&serde_json::Value>,
) -> Vec<&serde_json::Value> {
    runspec_value
        .and_then(|runspec| runspec.get("roles"))
        .and_then(|roles| roles.as_array())
        .map(|roles| roles.iter().collect::<Vec<_>>())
        .unwrap_or_default()
}

fn factory_profile_roles(profile_value: Option<&serde_json::Value>) -> Vec<&serde_json::Value> {
    profile_value
        .and_then(|profile| profile.get("roles"))
        .and_then(|roles| roles.as_array())
        .map(|roles| roles.iter().collect::<Vec<_>>())
        .unwrap_or_default()
}

pub(crate) fn validate_factory_runspec_graph(runspec_value: &serde_json::Value) -> Result<()> {
    let tasks = factory_runspec_tasks(Some(runspec_value));
    if !tasks.is_empty() {
        validate_factory_runspec_items("task", &tasks)?;
        return Ok(());
    }

    let legacy_roles = factory_legacy_runspec_roles(Some(runspec_value));
    if !legacy_roles.is_empty() {
        validate_factory_runspec_items("role", &legacy_roles)?;
    }
    Ok(())
}

pub(crate) fn validate_factory_profile_graph(profile_value: &serde_json::Value) -> Result<()> {
    let profile_roles = factory_profile_roles(Some(profile_value));
    if profile_roles.is_empty() {
        return Ok(());
    }

    let mut known_ids = BTreeSet::new();
    for role in &profile_roles {
        let Some(id) = role
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            // Legacy string-form roles (e.g. `- planner`) lack an `id` field
            // by design; tolerate them the same way the rest of the planner
            // does (see factory_provider_profiles and factory_role_dependencies).
            continue;
        };
        if !known_ids.insert(id.to_string()) {
            return Err(anyhow!(
                "factory profile contains duplicate role id {}",
                redact_secrets(id)
            ));
        }
    }

    if known_ids.is_empty() {
        return Ok(());
    }

    for role in &profile_roles {
        let Some(id) = role
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        for dep in factory_json_string_array(role.get("deps")) {
            if !known_ids.contains(dep.as_str()) {
                return Err(anyhow!(
                    "factory profile dependency {} for role {} does not reference a known role",
                    redact_secrets(&dep),
                    redact_secrets(id)
                ));
            }
        }
    }

    validate_factory_dependency_graph_is_acyclic("factory profile", "role", &profile_roles)
}

fn validate_factory_runspec_items(kind: &str, items: &[&serde_json::Value]) -> Result<()> {
    let mut known_ids = BTreeSet::new();
    for item in items {
        let Some(id) = item
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            return Err(anyhow!(
                "RunSpec {kind} is missing a non-empty id; AO2 refuses to materialize an ambiguous factory-v3 graph"
            ));
        };
        if !known_ids.insert(id.to_string()) {
            return Err(anyhow!(
                "RunSpec contains duplicate task or role id {}",
                redact_secrets(id)
            ));
        }
    }

    for item in items {
        let id = item
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::trim)
            .unwrap_or("");
        for dep in factory_json_string_array(item.get("deps")) {
            if !known_ids.contains(dep.as_str()) {
                return Err(anyhow!(
                    "RunSpec dependency {} for task {} does not reference a known task",
                    redact_secrets(&dep),
                    redact_secrets(id)
                ));
            }
        }
    }
    validate_factory_dependency_graph_is_acyclic("RunSpec", kind, items)?;
    Ok(())
}

fn validate_factory_dependency_graph_is_acyclic(
    graph_label: &str,
    item_kind: &str,
    items: &[&serde_json::Value],
) -> Result<()> {
    let mut indegree: BTreeMap<String, usize> = BTreeMap::new();
    let mut outgoing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in items {
        let Some(id) = item
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        indegree.entry(id.to_string()).or_insert(0);
    }

    for item in items {
        let Some(id) = item
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        for dep in factory_json_string_array(item.get("deps")) {
            outgoing.entry(dep).or_default().push(id.to_string());
            *indegree.entry(id.to_string()).or_insert(0) += 1;
        }
    }

    let mut ready = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let mut visited = 0usize;
    while let Some(id) = ready.pop() {
        visited += 1;
        if let Some(children) = outgoing.get(&id) {
            for child in children {
                if let Some(degree) = indegree.get_mut(child) {
                    *degree = degree.saturating_sub(1);
                    if *degree == 0 {
                        ready.push(child.clone());
                    }
                }
            }
        }
    }

    if visited != indegree.len() {
        let cycle_nodes = indegree
            .iter()
            .filter(|(_, degree)| **degree > 0)
            .map(|(id, _)| redact_secrets(id))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(anyhow!(
            "{graph_label} contains a dependency cycle involving {item_kind} id(s): {cycle_nodes}; AO2 refuses to materialize a non-DAG factory-v3 graph"
        ));
    }

    Ok(())
}

pub(crate) fn factory_runspec_role_ids(runspec_value: Option<&serde_json::Value>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut roles = Vec::new();
    let tasks = factory_runspec_tasks(runspec_value);
    if !tasks.is_empty() {
        for task in tasks {
            let kind = task
                .get("kind")
                .and_then(|kind| kind.as_str())
                .unwrap_or("agent");
            if kind != "agent" {
                continue;
            }
            let Some(id) = task
                .get("id")
                .and_then(|id| id.as_str())
                .map(str::trim)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            if seen.insert(id.to_string()) {
                roles.push(id.to_string());
            }
        }
        return roles;
    }

    for role in factory_legacy_runspec_roles(runspec_value) {
        let Some(id) = role
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        if seen.insert(id.to_string()) {
            roles.push(id.to_string());
        }
    }
    roles
}

pub(crate) fn factory_provider_profiles(
    runspec_value: Option<&serde_json::Value>,
    profile_value: Option<&serde_json::Value>,
) -> Vec<String> {
    let mut providers = BTreeSet::new();
    for role in factory_profile_roles(profile_value) {
        if let Some(provider_key) = role
            .get("provider_key")
            .and_then(|provider_key| provider_key.as_str())
            .map(str::trim)
            .filter(|provider_key| !provider_key.is_empty())
        {
            providers.insert(provider_key.to_string());
        }
    }
    let tasks = factory_runspec_tasks(runspec_value);
    if !tasks.is_empty() {
        for task in tasks {
            if let Some(provider) = task
                .get("spec")
                .and_then(|spec| spec.get("provider"))
                .and_then(|provider| provider.as_str())
                .map(str::trim)
                .filter(|provider| !provider.is_empty())
            {
                providers.insert(provider.to_string());
            }
        }
    } else {
        for role in factory_legacy_runspec_roles(runspec_value) {
            if let Some(provider_key) = role
                .get("provider_key")
                .and_then(|provider_key| provider_key.as_str())
                .map(str::trim)
                .filter(|provider_key| !provider_key.is_empty())
            {
                providers.insert(provider_key.to_string());
            }
        }
    }
    if providers.is_empty() {
        return vec![
            "scripted".to_string(),
            "codex".to_string(),
            "claude".to_string(),
        ];
    }
    providers.into_iter().collect()
}

pub(crate) fn factory_runspec_translation(
    runspec_value: Option<&serde_json::Value>,
    profile_value: Option<&serde_json::Value>,
    roles: &[serde_json::Value],
) -> serde_json::Value {
    let tasks = factory_runspec_tasks(runspec_value);
    if tasks.is_empty() {
        let profile_roles = factory_profile_roles(profile_value);
        if !profile_roles.is_empty() {
            let mut translated_roles = Vec::new();
            let mut dependency_edges = Vec::new();
            for role in profile_roles {
                let id = role.get("id").and_then(|id| id.as_str()).unwrap_or("");
                let deps = factory_json_string_array(role.get("deps"));
                for dep in &deps {
                    dependency_edges.push(serde_json::json!([dep, id]));
                }
                translated_roles.push(serde_json::json!({
                    "id": id,
                    "kind": "agent",
                    "deps": deps,
                    "provider_profile": role.get("provider_key").and_then(|value| value.as_str()).unwrap_or(""),
                    "reads": role.get("reads").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "writes": role.get("writes").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "instructions": role.get("instructions").cloned().unwrap_or_else(|| serde_json::json!([]))
                }));
            }
            return serde_json::json!({
                "source": "factory-v3-profile",
                "task_count": translated_roles.len(),
                "task_graph": translated_roles,
                "direct_dependency_edges": dependency_edges,
                "role_ids": roles.iter().filter_map(|role| role.get("role_id").and_then(|value| value.as_str())).collect::<Vec<_>>(),
                "providers": factory_provider_profiles(runspec_value, profile_value)
            });
        }

        let legacy_roles = factory_legacy_runspec_roles(runspec_value);
        if legacy_roles.is_empty() {
            return serde_json::json!({
                "source": "ao2-default-factory-compatible-workflow",
                "task_count": 0,
                "task_graph": [],
                "direct_dependency_edges": [],
                "role_ids": roles.iter().filter_map(|role| role.get("role_id").and_then(|value| value.as_str())).collect::<Vec<_>>(),
                "providers": factory_provider_profiles(None, None)
            });
        }

        let mut translated_roles = Vec::new();
        let mut dependency_edges = Vec::new();
        for role in legacy_roles {
            let id = role.get("id").and_then(|id| id.as_str()).unwrap_or("");
            let deps = factory_json_string_array(role.get("deps"));
            for dep in &deps {
                dependency_edges.push(serde_json::json!([dep, id]));
            }
            translated_roles.push(serde_json::json!({
                "id": id,
                "kind": "agent",
                "deps": deps,
                "provider_profile": role.get("provider_key").and_then(|value| value.as_str()).unwrap_or(""),
                "host_tags": role.get("host_tag").cloned().unwrap_or_else(|| serde_json::json!([])),
                "reads": role.get("reads").cloned().unwrap_or_else(|| serde_json::json!([])),
                "writes": role.get("writes").cloned().unwrap_or_else(|| serde_json::json!([]))
            }));
        }
        return serde_json::json!({
            "source": "factory-v3-legacy-roles-runspec",
            "task_count": translated_roles.len(),
            "task_graph": translated_roles,
            "direct_dependency_edges": dependency_edges,
            "role_ids": roles.iter().filter_map(|role| role.get("role_id").and_then(|value| value.as_str())).collect::<Vec<_>>(),
            "providers": factory_provider_profiles(runspec_value, profile_value)
        });
    }

    let mut translated_tasks = Vec::new();
    let mut dependency_edges = Vec::new();
    for task in tasks {
        let id = task.get("id").and_then(|id| id.as_str()).unwrap_or("");
        let deps = factory_json_string_array(task.get("deps"));
        for dep in &deps {
            dependency_edges.push(serde_json::json!([dep, id]));
        }
        let spec = &task["spec"];
        translated_tasks.push(serde_json::json!({
            "id": id,
            "kind": task.get("kind").and_then(|kind| kind.as_str()).unwrap_or("agent"),
            "deps": deps,
            "provider": spec.get("provider").and_then(|value| value.as_str()).unwrap_or(""),
            "agent": spec.get("agent").and_then(|value| value.as_str()).unwrap_or(""),
            "prompt_file": spec.get("promptFile").and_then(|value| value.as_str()).unwrap_or(""),
            "policy_profile": spec.get("policyProfile").and_then(|value| value.as_str()).unwrap_or("")
        }));
    }

    serde_json::json!({
        "source": "factory-v3-runspec",
        "task_count": translated_tasks.len(),
        "task_graph": translated_tasks,
        "direct_dependency_edges": dependency_edges,
        "role_ids": roles.iter().filter_map(|role| role.get("role_id").and_then(|value| value.as_str())).collect::<Vec<_>>(),
        "providers": factory_provider_profiles(runspec_value, profile_value)
    })
}

fn factory_workflow_tasks(
    runspec_value: Option<&serde_json::Value>,
    profile_value: Option<&serde_json::Value>,
    role_ids: &[String],
) -> Vec<serde_json::Value> {
    let tasks = factory_runspec_tasks(runspec_value);
    if tasks.is_empty() {
        let profile_roles = factory_profile_roles(profile_value);
        if !profile_roles.is_empty() {
            return profile_roles
                .into_iter()
                .filter_map(|role| {
                    let id = role.get("id")?.as_str()?.trim();
                    if id.is_empty() {
                        return None;
                    }
                    Some(serde_json::json!({
                        "id": id,
                        "role": id,
                        "kind": "agent",
                        "provider_profile": role.get("provider_key").and_then(|value| value.as_str()).unwrap_or("scripted"),
                        "reads": role.get("reads").cloned().unwrap_or_else(|| serde_json::json!([])),
                        "writes": role.get("writes").cloned().unwrap_or_else(|| serde_json::json!([])),
                        "instructions": role.get("instructions").cloned().unwrap_or_else(|| serde_json::json!([])),
                        "policy_profile": "factory-v3-profile-role"
                    }))
                })
                .collect();
        }
        let legacy_roles = factory_legacy_runspec_roles(runspec_value);
        if legacy_roles.is_empty() {
            return role_ids
                .iter()
                .map(|role| {
                    serde_json::json!({
                        "id": role,
                        "role": role,
                        "kind": "agent",
                        "provider": "scripted",
                        "prompt_file": null,
                        "policy_profile": "ao2-default-local"
                    })
                })
                .collect();
        }
        return legacy_roles
            .into_iter()
            .filter_map(|role| {
                let id = role.get("id")?.as_str()?.trim();
                if id.is_empty() {
                    return None;
                }
                Some(serde_json::json!({
                    "id": id,
                    "role": id,
                    "kind": "agent",
                    "provider_profile": role.get("provider_key").and_then(|value| value.as_str()).unwrap_or(""),
                    "host_tags": role.get("host_tag").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "reads": role.get("reads").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "writes": role.get("writes").cloned().unwrap_or_else(|| serde_json::json!([])),
                    "policy_profile": "factory-v3-legacy-runspec-role"
                }))
            })
            .collect();
    }
    tasks
        .into_iter()
        .filter_map(|task| {
            let id = task.get("id")?.as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            let kind = task
                .get("kind")
                .and_then(|kind| kind.as_str())
                .unwrap_or("agent");
            let spec = &task["spec"];
            Some(serde_json::json!({
                "id": id,
                "role": id,
                "kind": kind,
                "provider": spec.get("provider").and_then(|value| value.as_str()).unwrap_or("scripted"),
                "agent": spec.get("agent").and_then(|value| value.as_str()).unwrap_or(""),
                "prompt_file": spec.get("promptFile").and_then(|value| value.as_str()).unwrap_or(""),
                "policy_profile": spec.get("policyProfile").and_then(|value| value.as_str()).unwrap_or("ao2-default-local")
            }))
        })
        .collect()
}

fn factory_workflow_dependencies(
    runspec_value: Option<&serde_json::Value>,
    profile_value: Option<&serde_json::Value>,
) -> Vec<serde_json::Value> {
    let mut dependencies = Vec::new();
    let tasks = factory_runspec_tasks(runspec_value);
    if !tasks.is_empty() {
        for task in tasks {
            let Some(id) = task.get("id").and_then(|id| id.as_str()) else {
                continue;
            };
            for dep in factory_json_string_array(task.get("deps")) {
                dependencies.push(serde_json::json!({
                    "from": dep,
                    "to": id,
                    "source": "factory-v3-runspec-deps"
                }));
            }
        }
        return dependencies;
    }

    for role in factory_profile_roles(profile_value) {
        let Some(id) = role.get("id").and_then(|id| id.as_str()) else {
            continue;
        };
        for dep in factory_json_string_array(role.get("deps")) {
            dependencies.push(serde_json::json!({
                "from": dep,
                "to": id,
                "source": "factory-v3-profile-role-deps"
            }));
        }
    }
    if !dependencies.is_empty() {
        return dependencies;
    }

    for role in factory_legacy_runspec_roles(runspec_value) {
        let Some(id) = role.get("id").and_then(|id| id.as_str()) else {
            continue;
        };
        for dep in factory_json_string_array(role.get("deps")) {
            dependencies.push(serde_json::json!({
                "from": dep,
                "to": id,
                "source": "factory-v3-legacy-runspec-role-deps"
            }));
        }
    }
    dependencies
}

pub(crate) fn factory_compat_workflow_value(
    workflow_id: &str,
    request_value: &serde_json::Value,
    runspec_value: Option<&serde_json::Value>,
    profile_value: Option<&serde_json::Value>,
    roles: &[serde_json::Value],
) -> serde_json::Value {
    let role_ids = roles
        .iter()
        .filter_map(|role| role.get("role_id").and_then(|value| value.as_str()))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut acceptance = factory_request_redacted_string_array(request_value, "acceptance");
    if acceptance.is_empty() {
        acceptance = vec![
            "AO2 executes the generated workflow without factory-v3 driving the run.".to_string(),
            "Midpoint and closure evidence include concerns, blockers, changed files, sandbox, and redaction status.".to_string(),
            "Replay has zero digest failures.".to_string(),
        ];
    }
    serde_json::json!({
        "id": workflow_id,
        "name": workflow_id,
        "description": "AO2-native workflow materialized from factory-v3-compatible request and RunSpec inputs.",
        "version": "0.1.0",
        "template_kind": "real_project",
        "objective": factory_request_redacted_string(request_value, "objective")
            .or_else(|| factory_request_redacted_string(request_value, "title"))
            .unwrap_or_else(|| "Execute factory-v3-compatible governed work through AO2.".to_string()),
        "roles": if role_ids.is_empty() { factory_default_roles() } else { role_ids.clone() },
        "tasks": factory_workflow_tasks(runspec_value, profile_value, &role_ids),
        "dependencies": factory_workflow_dependencies(runspec_value, profile_value),
        "inputs": [],
        "budgets": {
            "max_repair_attempts": 1,
            "provider_budget_owner": "ao2"
        },
        "tool_scopes": {
            "target_repo": "governed_write_after_policy_approval",
            "network": "provider_cli_oauth_only",
            "secrets": "redacted_and_never_logged"
        },
        "approval_rules": {
            "mode": "exact_action_digest",
            "risky_actions_require_approval": true
        },
        "evaluator": {
            "role": "evaluator-closer",
            "owner": "ao2-native-evaluator-closer",
            "factory_v3_role": "parity_oracle_only"
        },
        "verifier": {
            "command": factory_runspec_verifier_command(runspec_value)
        },
        "policy": {
            "deny_by_default": true,
            "approval_mode": "exact_action_digest",
            "risky_actions": ["git_push", "destructive_delete", "package_install", "network_egress", "secret_read"],
            "profile_policy_posture": profile_value
                .and_then(|value| value.get("policy_posture").cloned())
                .unwrap_or_else(|| serde_json::json!({}))
        },
        "evidence": {
            "evidence_cockpit": "required",
            "replay": "required",
            "required_artifacts": ["planning_evidence", "plan", "provider_output", "concern_report", "test_log", "closure_report"]
        },
        "acceptance": acceptance,
        "factory_v3_compatibility": {
            "source": "factory-v3-style request/profile/runspec/role-contract inputs",
            "factory_v3_role": "parity_oracle_only",
            "ao2_execution_owner": true,
            "legacy_roles_runspec": !factory_legacy_runspec_roles(runspec_value).is_empty()
        }
    })
}

fn factory_request_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|candidate| candidate.as_str())
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .map(str::to_string)
}

fn factory_request_redacted_string(value: &serde_json::Value, key: &str) -> Option<String> {
    factory_request_string(value, key).map(|candidate| redact_secrets(&candidate))
}

fn factory_request_string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    factory_json_string_array(value.get(key))
}

fn factory_request_redacted_string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    factory_request_string_array(value, key)
        .into_iter()
        .map(|candidate| redact_secrets(&candidate))
        .collect()
}

fn factory_json_string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|candidate| candidate.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn factory_runspec_verifier_command(runspec_value: Option<&serde_json::Value>) -> String {
    runspec_value
        .and_then(|runspec| {
            runspec
                .get("verifier")
                .and_then(|verifier| {
                    verifier.as_str().map(str::to_string).or_else(|| {
                        verifier
                            .get("command")
                            .and_then(|command| command.as_str())
                            .map(str::to_string)
                    })
                })
                .or_else(|| {
                    runspec
                        .get("verify")
                        .and_then(|verify| verify.as_str())
                        .map(str::to_string)
                })
        })
        .map(|command| command.trim().to_string())
        .filter(|command| !command.is_empty())
        .unwrap_or_else(|| "npm run verify".to_string())
}
