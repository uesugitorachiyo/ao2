//! Deterministic validator for `ao2.sdd-plan.v1`.
//!
//! Rules V1..V11 from README §5.1. Returns a structured
//! [`ValidationReport`] and never panics: any unexpected condition
//! becomes an error variant.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::Serialize;
use thiserror::Error;

use crate::schema::{Plan, SurfaceMap, SCHEMA_VERSION};

pub const ALLOWED_SHELLS: &[&str] = &[
    "cargo", "npm", "pnpm", "pytest", "python3", "bash", "sh", "node", "git", "gh", "ao2", "ao",
];

pub const ACCEPTANCE_VERBS: &[&str] = &[
    "accept",
    "add",
    "annotate",
    "append",
    "apply",
    "assert",
    "audit",
    "block",
    "build",
    "bump",
    "cancel",
    "capture",
    "change",
    "check",
    "clear",
    "close",
    "collect",
    "commit",
    "compose",
    "compute",
    "confirm",
    "configure",
    "connect",
    "construct",
    "copy",
    "create",
    "declare",
    "decode",
    "define",
    "delete",
    "deliver",
    "deploy",
    "derive",
    "describe",
    "deserialize",
    "detect",
    "diff",
    "dispatch",
    "document",
    "drop",
    "dump",
    "emit",
    "enable",
    "encode",
    "enforce",
    "ensure",
    "establish",
    "exit",
    "expand",
    "expect",
    "expose",
    "extend",
    "extract",
    "fail",
    "fetch",
    "find",
    "finish",
    "fix",
    "flag",
    "flush",
    "format",
    "gate",
    "generate",
    "get",
    "halt",
    "handle",
    "hash",
    "hide",
    "ignore",
    "implement",
    "import",
    "include",
    "increment",
    "index",
    "init",
    "initialize",
    "insert",
    "inspect",
    "install",
    "invoke",
    "issue",
    "land",
    "lint",
    "list",
    "load",
    "lock",
    "log",
    "make",
    "map",
    "mark",
    "match",
    "merge",
    "mirror",
    "mock",
    "move",
    "name",
    "normalize",
    "open",
    "order",
    "output",
    "package",
    "parse",
    "pass",
    "persist",
    "pin",
    "populate",
    "post",
    "preserve",
    "print",
    "produce",
    "propagate",
    "prove",
    "publish",
    "pull",
    "push",
    "query",
    "read",
    "rebase",
    "record",
    "redact",
    "refuse",
    "register",
    "reject",
    "release",
    "remove",
    "rename",
    "render",
    "replace",
    "report",
    "request",
    "require",
    "reset",
    "resolve",
    "respond",
    "restart",
    "restore",
    "retry",
    "return",
    "review",
    "rotate",
    "route",
    "run",
    "sanitize",
    "save",
    "scan",
    "schedule",
    "select",
    "send",
    "serialize",
    "serve",
    "set",
    "ship",
    "show",
    "sign",
    "skip",
    "sort",
    "split",
    "stamp",
    "start",
    "stop",
    "store",
    "stream",
    "stub",
    "submit",
    "succeed",
    "support",
    "surface",
    "swap",
    "sync",
    "tag",
    "target",
    "test",
    "throw",
    "throw",
    "tokenize",
    "trace",
    "track",
    "transform",
    "translate",
    "trigger",
    "trim",
    "truncate",
    "tune",
    "unblock",
    "uninstall",
    "unlock",
    "unregister",
    "update",
    "upgrade",
    "upload",
    "use",
    "validate",
    "verify",
    "wait",
    "warn",
    "watch",
    "wrap",
    "write",
    "yield",
    "zero",
    "cite",
    "demonstrate",
    "denote",
    "discover",
    "explain",
    "identify",
    "illustrate",
    "indicate",
    "locate",
    "maintain",
    "mention",
    "note",
    "observe",
    "place",
    "point",
    "recognize",
    "reexport",
    "reference",
];

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub errors: Vec<ValidationError>,
    /// Parsed plan when the candidate at least deserialized — present
    /// even when rule errors are non-empty, so callers can inspect it.
    /// `None` only on V1 Shape failure.
    pub plan: Option<Plan>,
}

impl ValidationReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// `§6` retry-protocol alias for [`Self::ok`].
    pub fn is_pass(&self) -> bool {
        self.ok()
    }

    /// One human-readable line per error, suitable for injection into
    /// `prior_errors[]` on the next provider attempt (README §6).
    pub fn errors_for_provider_feedback(&self) -> Vec<String> {
        self.errors.iter().map(|e| e.to_string()).collect()
    }

    /// Render the full report as a multi-line text artifact for
    /// `target/sdd-planner/<plan_id>/validation-errors.txt` (README §6).
    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.errors.is_empty() {
            out.push_str("PASS\n");
            return out;
        }
        out.push_str(&format!("FAIL: {} error(s)\n", self.errors.len()));
        for (i, e) in self.errors.iter().enumerate() {
            out.push_str(&format!("[{i:02}] {e}\n"));
        }
        out
    }
}

#[derive(Debug, Clone, Error, Serialize, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ValidationError {
    #[error("V1: candidate failed to deserialize into Plan: {message}")]
    Shape { message: String },

    #[error("V1: schema_version must be {expected} (got {actual})")]
    BadSchemaVersion { expected: String, actual: String },

    #[error("V2: step '{step_id}' references unknown path '{path}'")]
    UnknownPath { step_id: String, path: String },

    #[error("V3: step '{step_id}' has empty acceptance list")]
    EmptyAcceptance { step_id: String },

    #[error(
        "V3: step '{step_id}' acceptance entry {index} does not start with a verb (got '{token}')"
    )]
    AcceptanceVerbMissing {
        step_id: String,
        index: usize,
        token: String,
    },

    #[error("V4: dependency cycle involving steps {0:?}")]
    Cycle(BTreeSet<String>),

    #[error("V5: trust_boundary.mutates_ao_artifacts must be literal false")]
    MutatingPlanner,

    #[error("V6: provenance.attempts must be in [1, 3] (got {0})")]
    ExcessAttempts(u32),

    #[error("V7: plan.steps.len() must be in [1, 25] (got {0})")]
    BadStepCount(usize),

    #[error("V8: exit_criteria.{location}[{index}] shell '{token}' is not allow-listed")]
    DisallowedShell {
        location: &'static str,
        index: usize,
        token: String,
    },

    #[error("V9: step.id '{0}' does not match /^step_[a-z0-9_]+$/")]
    BadStepId(String),

    #[error("V10: plan.title length {0} exceeds 80")]
    OverlongTitle(usize),

    #[error("V4: step '{step_id}' depends on unknown step '{missing}'")]
    UnknownDependency { step_id: String, missing: String },
}

pub fn validate(plan_json: &str, surface_map: Option<&SurfaceMap>) -> ValidationReport {
    let mut errors: Vec<ValidationError> = Vec::new();

    let plan: Plan = match serde_json::from_str(plan_json) {
        Ok(p) => p,
        Err(e) => {
            errors.push(ValidationError::Shape {
                message: e.to_string(),
            });
            return ValidationReport { errors, plan: None };
        }
    };

    if plan.schema_version != SCHEMA_VERSION {
        errors.push(ValidationError::BadSchemaVersion {
            expected: SCHEMA_VERSION.to_string(),
            actual: plan.schema_version.clone(),
        });
    }

    check_v5_trust_boundary(&plan, &mut errors);
    check_v6_attempts(&plan, &mut errors);
    check_v7_step_count(&plan, &mut errors);
    check_v8_shell_allowlist(&plan, &mut errors);
    check_v9_step_ids(&plan, &mut errors);
    check_v10_title(&plan, &mut errors);
    check_v3_acceptance(&plan, &mut errors);
    check_v4_cycles(&plan, &mut errors);
    if let Some(sm) = surface_map {
        check_v2_paths(&plan, sm, &mut errors);
    }

    ValidationReport {
        errors,
        plan: Some(plan),
    }
}

fn check_v5_trust_boundary(plan: &Plan, errors: &mut Vec<ValidationError>) {
    if plan.trust_boundary.mutates_ao_artifacts {
        errors.push(ValidationError::MutatingPlanner);
    }
}

fn check_v6_attempts(plan: &Plan, errors: &mut Vec<ValidationError>) {
    let a = plan.provenance.attempts;
    if !(1..=3).contains(&a) {
        errors.push(ValidationError::ExcessAttempts(a));
    }
}

fn check_v7_step_count(plan: &Plan, errors: &mut Vec<ValidationError>) {
    let n = plan.plan.steps.len();
    if !(1..=25).contains(&n) {
        errors.push(ValidationError::BadStepCount(n));
    }
}

fn check_v8_shell_allowlist(plan: &Plan, errors: &mut Vec<ValidationError>) {
    let allowed: HashSet<&&str> = ALLOWED_SHELLS.iter().collect();
    let check = |location: &'static str, list: &[String], errors: &mut Vec<ValidationError>| {
        for (i, cmd) in list.iter().enumerate() {
            let token = cmd.split_whitespace().next().unwrap_or("").to_string();
            if !allowed.contains(&token.as_str()) {
                errors.push(ValidationError::DisallowedShell {
                    location,
                    index: i,
                    token,
                });
            }
        }
    };
    check("tests", &plan.plan.exit_criteria.tests, errors);
    check("gates", &plan.plan.exit_criteria.gates, errors);
}

fn check_v9_step_ids(plan: &Plan, errors: &mut Vec<ValidationError>) {
    for step in &plan.plan.steps {
        if !is_valid_step_id(&step.id) {
            errors.push(ValidationError::BadStepId(step.id.clone()));
        }
    }
}

fn is_valid_step_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("step_") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn check_v10_title(plan: &Plan, errors: &mut Vec<ValidationError>) {
    let len = plan.plan.title.chars().count();
    if len > 80 {
        errors.push(ValidationError::OverlongTitle(len));
    }
}

fn check_v3_acceptance(plan: &Plan, errors: &mut Vec<ValidationError>) {
    let verbs: HashSet<&&str> = ACCEPTANCE_VERBS.iter().collect();
    for step in &plan.plan.steps {
        if step.acceptance.is_empty() {
            errors.push(ValidationError::EmptyAcceptance {
                step_id: step.id.clone(),
            });
            continue;
        }
        for (i, line) in step.acceptance.iter().enumerate() {
            let raw_token = line
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            let token: String = raw_token.chars().filter(|c| *c != '-').collect();
            if !verbs.contains(&token.as_str()) {
                errors.push(ValidationError::AcceptanceVerbMissing {
                    step_id: step.id.clone(),
                    index: i,
                    token,
                });
            }
        }
    }
}

fn check_v4_cycles(plan: &Plan, errors: &mut Vec<ValidationError>) {
    let ids: HashSet<&str> = plan.plan.steps.iter().map(|s| s.id.as_str()).collect();
    let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
    for step in &plan.plan.steps {
        let mut edges: Vec<&str> = Vec::new();
        for dep in &step.depends_on {
            if !ids.contains(dep.as_str()) {
                errors.push(ValidationError::UnknownDependency {
                    step_id: step.id.clone(),
                    missing: dep.clone(),
                });
                continue;
            }
            edges.push(dep.as_str());
        }
        graph.insert(step.id.as_str(), edges);
    }

    enum Mark {
        Visiting,
        Done,
    }
    let mut state: HashMap<&str, Mark> = HashMap::new();
    let mut cycle_nodes: BTreeSet<String> = BTreeSet::new();

    fn dfs<'a>(
        node: &'a str,
        graph: &HashMap<&'a str, Vec<&'a str>>,
        state: &mut HashMap<&'a str, Mark>,
        stack: &mut Vec<&'a str>,
        cycle: &mut BTreeSet<String>,
    ) {
        match state.get(node) {
            Some(Mark::Done) => return,
            Some(Mark::Visiting) => {
                let start = stack.iter().position(|n| n == &node).unwrap_or(0);
                for n in &stack[start..] {
                    cycle.insert((*n).to_string());
                }
                cycle.insert(node.to_string());
                return;
            }
            None => {}
        }
        state.insert(node, Mark::Visiting);
        stack.push(node);
        if let Some(neighbors) = graph.get(node) {
            for next in neighbors {
                dfs(next, graph, state, stack, cycle);
            }
        }
        stack.pop();
        state.insert(node, Mark::Done);
    }

    for &node in graph.keys() {
        let mut stack: Vec<&str> = Vec::new();
        dfs(node, &graph, &mut state, &mut stack, &mut cycle_nodes);
    }

    if !cycle_nodes.is_empty() {
        errors.push(ValidationError::Cycle(cycle_nodes));
    }
}

fn check_v2_paths(plan: &Plan, surface_map: &SurfaceMap, errors: &mut Vec<ValidationError>) {
    let known: HashSet<&str> = surface_map.files.iter().map(|f| f.path.as_str()).collect();
    for step in &plan.plan.steps {
        for path in &step.paths {
            if !known.contains(path.as_str()) {
                errors.push(ValidationError::UnknownPath {
                    step_id: step.id.clone(),
                    path: path.clone(),
                });
            }
        }
    }
}
