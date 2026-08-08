use anyhow::{anyhow, Result};
use ao2_core::{new_id, sha256_hex, ApprovalTicket, PolicyDecision, PolicyIntegrityBinding};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const POLICY_VERSION: &str = "ao2.local.deny-by-default.v1";
pub const POLICY_IDENTITY: &str = "ao2-policy.local";

pub fn integrity_binding(decision: &PolicyDecision) -> PolicyIntegrityBinding {
    PolicyIntegrityBinding {
        policy_identity: POLICY_IDENTITY.to_string(),
        policy_version: decision.policy_version.clone(),
        policy_digest: sha256_hex(serde_json::to_vec(decision).unwrap_or_default()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequest {
    pub principal: String,
    pub tool: String,
    pub operation: String,
    pub resource: String,
    pub args: Vec<String>,
    pub expected_side_effects: Vec<String>,
}

impl ToolRequest {
    pub fn action_digest(&self) -> String {
        if self.tool == "sandbox" && self.operation == "apply" && self.args.len() > 1 {
            self.args[1].clone()
        } else {
            sha256_hex(serde_json::to_vec(self).unwrap_or_default())
        }
    }
}

pub fn fail_on_forbidden_provider_api_keys() -> Result<()> {
    let forbidden = ["OPENAI_API_KEY", "ANTHROPIC_API_KEY"];
    for key in forbidden {
        if std::env::var_os(key).is_some() {
            return Err(anyhow!(
                "forbidden provider API key present in environment: {key}"
            ));
        }
    }
    Ok(())
}

pub fn redact_secrets(input: &str) -> String {
    input
        .lines()
        .map(redact_secret_line)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn secret_redaction_count(input: &str) -> usize {
    secret_redaction_class_counts(input).values().sum()
}

pub fn secret_redaction_class_counts(input: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for line in input.lines() {
        add_line_secret_redaction_class_counts(line, &mut counts);
    }
    counts
}

fn redact_secret_line(line: &str) -> String {
    if let Some(redacted) = redact_assignment_line(line) {
        return redacted;
    }
    if let Some(redacted) = redact_header_line(line) {
        return redacted;
    }
    let redacted = redact_bearer_tokens(line);
    let redacted = redact_query_secret_values(&redacted);
    let redacted = redact_inline_secret_assignments(&redacted);
    redact_standalone_secret_tokens(&redacted)
}

fn redact_bearer_tokens(line: &str) -> String {
    let mut redacted = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(index) = find_ascii_case_insensitive(rest, "Bearer ") {
        redacted.push_str(&rest[..index]);
        redacted.push_str("Bearer [REDACTED]");
        let value_start = index + "Bearer ".len();
        rest = &rest[inline_secret_value_end(rest, value_start)..];
    }
    redacted.push_str(rest);
    redacted
}

fn add_line_secret_redaction_class_counts(line: &str, counts: &mut BTreeMap<String, usize>) {
    if let Some(class) = assignment_secret_class(line) {
        increment_secret_class(counts, class);
        return;
    }
    if let Some(class) = header_secret_class(line) {
        increment_secret_class(counts, class);
        return;
    }
    if find_ascii_case_insensitive(line, "Bearer ").is_some() {
        increment_secret_class(counts, "bearer_authorization");
    }
    add_query_secret_class_counts(line, counts);
    add_inline_secret_class_counts(line, counts);
}

fn redact_assignment_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let (export_prefix, assignment) = trimmed
        .strip_prefix("export ")
        .map_or(("", trimmed), |assignment| ("export ", assignment));
    let (key, _value) = assignment.split_once('=')?;
    let key = key.trim();
    if !is_assignment_secret_key(key) {
        return None;
    }
    Some(format!("{indent}{export_prefix}{key}=[REDACTED]"))
}

fn assignment_secret_class(line: &str) -> Option<&'static str> {
    let trimmed = line.trim_start();
    let assignment = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (key, _value) = assignment.split_once('=')?;
    let key = key.trim();
    if !is_assignment_key_shape(key) {
        return None;
    }
    sensitive_key_class(key)
}

fn redact_header_line(line: &str) -> Option<String> {
    let (name, value) = line.split_once(':')?;
    let normalized = name.trim().to_ascii_lowercase();
    let value = value.trim_start();
    if normalized == "authorization" && value.starts_with("Bearer ") {
        return Some(format!("{}: Bearer [REDACTED]", name.trim_end()));
    }
    if normalized == "x-api-key" || normalized == "api-key" || normalized.contains("password") {
        return Some(format!("{}: [REDACTED]", name.trim_end()));
    }
    None
}

fn header_secret_class(line: &str) -> Option<&'static str> {
    let (name, value) = line.split_once(':')?;
    let normalized = name.trim().to_ascii_lowercase();
    let value = value.trim_start();
    if normalized == "authorization" && value.starts_with("Bearer ") {
        return Some("bearer_authorization");
    }
    if normalized == "x-api-key" || normalized == "api-key" {
        return Some("api_key_header");
    }
    if normalized.contains("password") {
        return Some("password");
    }
    None
}

fn redact_query_secret_values(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut redacted = String::with_capacity(line.len());
    let mut index = 0;
    while index < line.len() {
        let at_param_start = index == 0 || matches!(bytes[index - 1], b'?' | b'&' | b';');
        if at_param_start {
            if let Some(eq_offset) = line[index..].find('=') {
                let eq = index + eq_offset;
                let key = &line[index..eq];
                if is_sensitive_query_key(key) {
                    redacted.push_str(key);
                    redacted.push_str("=[REDACTED]");
                    index = query_value_end(line, eq + 1);
                    continue;
                }
            }
        }
        let ch = line[index..].chars().next().expect("valid char boundary");
        redacted.push(ch);
        index += ch.len_utf8();
    }
    redacted
}

fn redact_inline_secret_assignments(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut redacted = String::with_capacity(line.len());
    let mut index = 0;
    while index < line.len() {
        if is_inline_assignment_boundary(bytes, index) {
            if let Some(eq_offset) = line[index..].find('=') {
                let eq = index + eq_offset;
                let key = &line[index..eq];
                if key
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
                    && is_sensitive_key(key)
                {
                    redacted.push_str(key);
                    redacted.push_str("=[REDACTED]");
                    index = inline_secret_value_end(line, eq + 1);
                    continue;
                }
            }
        }
        let ch = line[index..].chars().next().expect("valid char boundary");
        redacted.push(ch);
        index += ch.len_utf8();
    }
    redacted
}

fn redact_standalone_secret_tokens(line: &str) -> String {
    let mut redacted = String::with_capacity(line.len());
    let mut index = 0;
    while index < line.len() {
        let rest = &line[index..];
        if rest.starts_with("sk-") || rest.starts_with("ghp_") || rest.starts_with("github_pat_") {
            redacted.push_str("[REDACTED]");
            index = inline_secret_value_end(line, index);
            continue;
        }
        let ch = rest.chars().next().expect("valid char boundary");
        redacted.push(ch);
        index += ch.len_utf8();
    }
    redacted
}

fn add_inline_secret_class_counts(line: &str, counts: &mut BTreeMap<String, usize>) {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < line.len() {
        if is_inline_assignment_boundary(bytes, index) {
            if let Some(eq_offset) = line[index..].find('=') {
                let eq = index + eq_offset;
                let key = &line[index..eq];
                if key
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
                    && is_sensitive_key(key)
                {
                    if let Some(class) = sensitive_key_class(key) {
                        increment_secret_class(counts, class);
                    }
                    index = inline_secret_value_end(line, eq + 1);
                    continue;
                }
            }
        }
        let ch = line[index..].chars().next().expect("valid char boundary");
        index += ch.len_utf8();
    }
    if line.contains("sk-") || line.contains("ghp_") || line.contains("github_pat_") {
        increment_secret_class(counts, "inline_secret_token");
    }
}

fn is_inline_assignment_boundary(bytes: &[u8], index: usize) -> bool {
    index == 0
        || matches!(
            bytes[index - 1],
            b' ' | b'\t' | b'(' | b'[' | b'{' | b'"' | b'\''
        )
}

fn inline_secret_value_end(line: &str, value_start: usize) -> usize {
    line[value_start..]
        .find(['&', ';', '#', '"', '\'', '<', '>', ' ', '\t', '\n', '\r'])
        .map_or(line.len(), |offset| value_start + offset)
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

fn add_query_secret_class_counts(line: &str, counts: &mut BTreeMap<String, usize>) {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < line.len() {
        let at_param_start = index == 0 || matches!(bytes[index - 1], b'?' | b'&' | b';');
        if at_param_start {
            if let Some(eq_offset) = line[index..].find('=') {
                let eq = index + eq_offset;
                if let Some(class) = sensitive_query_key_class(&line[index..eq]) {
                    increment_secret_class(counts, class);
                    index = query_value_end(line, eq + 1);
                    continue;
                }
            }
        }
        let ch = line[index..].chars().next().expect("valid char boundary");
        index += ch.len_utf8();
    }
}

fn query_value_end(line: &str, value_start: usize) -> usize {
    line[value_start..]
        .find(['&', ';', '#', '"', '\'', '<', '>', ' ', '\t'])
        .map_or(line.len(), |offset| value_start + offset)
}

fn is_sensitive_query_key(key: &str) -> bool {
    sensitive_query_key_class(key).is_some()
}

fn sensitive_query_key_class(key: &str) -> Option<&'static str> {
    let key = key.trim().to_ascii_lowercase();
    match key.as_str() {
        "token" | "access_token" | "refresh_token" | "id_token" | "auth_token" => {
            Some("query_token")
        }
        "api_key" | "apikey" => Some("query_api_key"),
        "signature" | "sig" => Some("query_signature"),
        "secret" => Some("query_secret"),
        "password" => Some("query_password"),
        _ => None,
    }
}

fn sensitive_key_class(key: &str) -> Option<&'static str> {
    let key = key.to_ascii_uppercase();
    if key.ends_with("_API_KEY") {
        Some("provider_api_key")
    } else if key.ends_with("_AUTH_TOKEN") {
        Some("auth_token")
    } else if key.ends_with("_SERVICE_ROLE_KEY") {
        Some("service_role_key")
    } else if key.ends_with("_INTAKE_KEY") {
        Some("intake_key")
    } else if key.ends_with("_PRIVATE_KEY") {
        Some("private_key")
    } else if key.ends_with("_ACCESS_KEY") {
        Some("access_key")
    } else if key.ends_with("_TOKEN") {
        Some("token")
    } else if key.ends_with("_SECRET") {
        Some("secret")
    } else if key.ends_with("_PASSWORD") || key == "PASSWORD" {
        Some("password")
    } else {
        None
    }
}

fn increment_secret_class(counts: &mut BTreeMap<String, usize>, class: &str) {
    *counts.entry(class.to_string()).or_default() += 1;
}

fn is_sensitive_key(key: &str) -> bool {
    sensitive_key_class(key).is_some()
}

fn is_assignment_secret_key(key: &str) -> bool {
    is_assignment_key_shape(key) && is_sensitive_key(key)
}

fn is_assignment_key_shape(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub fn evaluate(request: &ToolRequest) -> PolicyDecision {
    let risky = is_risky(request);
    PolicyDecision {
        decision_id: new_id("pol"),
        principal: request.principal.clone(),
        action: format!("{}:{}", request.tool, request.operation),
        resource: request.resource.clone(),
        request_digest: request.action_digest(),
        decision: if risky { "requires_approval" } else { "allow" }.to_string(),
        reason: if risky {
            "risky side effect requires exact-digest human approval".to_string()
        } else {
            "allowed by local MVP policy".to_string()
        },
        policy_version: POLICY_VERSION.to_string(),
        approval_ticket_id: None,
        created_at: Utc::now(),
    }
}

pub fn deny(request: &ToolRequest, reason: &str) -> PolicyDecision {
    PolicyDecision {
        decision_id: new_id("pol"),
        principal: request.principal.clone(),
        action: format!("{}:{}", request.tool, request.operation),
        resource: request.resource.clone(),
        request_digest: request.action_digest(),
        decision: "deny".to_string(),
        reason: reason.to_string(),
        policy_version: POLICY_VERSION.to_string(),
        approval_ticket_id: None,
        created_at: Utc::now(),
    }
}

pub fn create_approval_ticket(run_id: &str, request: &ToolRequest) -> ApprovalTicket {
    ApprovalTicket {
        ticket_id: new_id("approval"),
        run_id: run_id.to_string(),
        requested_action: format!("{}:{}", request.tool, request.operation),
        action_digest: request.action_digest(),
        risk_class: "external_write".to_string(),
        requester: request.principal.clone(),
        approver: None,
        status: "pending".to_string(),
        scope: request.resource.clone(),
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::minutes(30),
    }
}

pub fn grant_exact(
    ticket: &ApprovalTicket,
    approver: &str,
    request: &ToolRequest,
) -> Result<ApprovalTicket> {
    if ticket.action_digest != request.action_digest() {
        return Err(anyhow!(
            "approval digest mismatch; modified action requires new approval"
        ));
    }
    let mut granted = ticket.clone();
    granted.approver = Some(approver.to_string());
    granted.status = "approved".to_string();
    Ok(granted)
}

fn is_risky(request: &ToolRequest) -> bool {
    let joined = request.args.join(" ");
    request.tool == "git" && request.operation == "push"
        || request.tool == "network"
        || request.tool == "package_manager"
        || request.tool == "secret"
        || request.operation == "write_tree"
        || request.operation.contains("delete")
        || joined.contains("rm -rf")
        || joined.contains("..")
        || request.expected_side_effects.iter().any(|v| {
            matches!(
                v.as_str(),
                "external_write"
                    | "network_egress"
                    | "destructive"
                    | "package_install"
                    | "broad_write"
                    | "raw_secret_access"
            )
        })
}
