use std::fs;
use std::path::Path;

const REQUIRED_TEMPLATES: &[(&str, &str)] = &[
    ("bug-fix", "examples/task-templates/bug-fix.yaml"),
    (
        "small-refactor",
        "examples/task-templates/small-refactor.yaml",
    ),
    (
        "dependency-upgrade",
        "examples/task-templates/dependency-upgrade.yaml",
    ),
    (
        "test-generation",
        "examples/task-templates/test-generation.yaml",
    ),
];

const REQUIRED_SCHEMAS: &[&str] = &[
    "workflow.schema.json",
    "role.schema.json",
    "task.schema.json",
    "run-record.schema.json",
    "event.schema.json",
    "artifact.schema.json",
    "policy-decision.schema.json",
    "approval-ticket.schema.json",
    "tool-request.schema.json",
    "tool-result.schema.json",
    "context-bundle.schema.json",
    "closure.schema.json",
    "evidence-pack.schema.json",
    "obligation-ledger.schema.json",
    "report-contract.schema.json",
];

#[test]
fn canonical_schema_files_are_present_and_valid_json() {
    let root = workspace_root();
    for schema in REQUIRED_SCHEMAS {
        let path = root.join("schemas").join(schema);
        let content = fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!("read {}: {err}", path.display());
        });
        let json: serde_json::Value = serde_json::from_str(&content).unwrap_or_else(|err| {
            panic!("parse {}: {err}", path.display());
        });
        assert_eq!(
            json["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert!(
            json["title"].is_string(),
            "{} missing title",
            path.display()
        );
        assert_eq!(
            json["type"],
            "object",
            "{} must define object",
            path.display()
        );
    }
}

#[test]
fn risky_pr_example_declares_expected_evidence_contract() {
    let root = workspace_root();
    let expected_events = read_json(root.join("examples/risky-pr-run/expected-events.json"));
    let expected_pack = read_json(root.join("examples/risky-pr-run/expected-evidence-pack.json"));

    assert!(expected_events["required_event_types"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event == "tool.denied"));
    assert_eq!(expected_pack["verdict"], "accepted");
    assert_eq!(expected_pack["workflow_id"], "risky-pr-run@0.1.0");
}

#[test]
fn task_templates_exist_and_keep_governance_contract() {
    let root = workspace_root();
    for (id, rel_path) in REQUIRED_TEMPLATES {
        let content = fs::read_to_string(root.join(rel_path)).unwrap_or_else(|err| {
            panic!("read {rel_path}: {err}");
        });

        assert!(content.contains(&format!("id: {id}")), "{rel_path} id");
        assert!(content.contains("version: 0.1.0"), "{rel_path} version");
        assert!(content.contains("objective:"), "{rel_path} objective");
        assert!(content.contains("verifier:"), "{rel_path} verifier");
        assert!(content.contains("command:"), "{rel_path} verifier command");
        assert!(
            content.contains("approval_mode: exact_action_digest"),
            "{rel_path} exact approval"
        );
        assert!(
            content.contains("deny_by_default: true"),
            "{rel_path} deny by default"
        );
        assert!(
            content.contains("evidence_cockpit: required"),
            "{rel_path} cockpit requirement"
        );
    }
}

fn read_json(path: impl AsRef<Path>) -> serde_json::Value {
    let path = path.as_ref();
    let content = fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("read {}: {err}", path.display());
    });
    serde_json::from_str(&content).unwrap_or_else(|err| {
        panic!("parse {}: {err}", path.display());
    })
}

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}
