use std::fs;
use std::path::Path;
use std::sync::Mutex;

use ao2_runtime::{run_risky_pr_provider_free, RunOptions, RunStatus};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn copy_fixture(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[test]
fn risky_pr_run_rejects_once_then_accepts_with_exported_evidence() {
    let _guard = ENV_LOCK.lock().unwrap();
    let old_openai = std::env::var_os("OPENAI_API_KEY");
    let old_anthropic = std::env::var_os("ANTHROPIC_API_KEY");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("ANTHROPIC_API_KEY");

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let summary = run_risky_pr_provider_free(RunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("test-run".to_string()),
    })
    .unwrap();

    assert_eq!(summary.status, RunStatus::Accepted);
    assert_eq!(summary.rejection_count, 1);
    assert_eq!(summary.denied_actions.len(), 1);
    assert_eq!(summary.approvals.len(), 1);
    assert!(summary.evidence_pack_path.exists());
    assert!(summary.report_path.exists());
    assert!(summary.run_record_path.exists());

    let evidence = fs::read_to_string(&summary.evidence_pack_path).unwrap();
    let evidence_json: serde_json::Value = serde_json::from_str(&evidence).unwrap();
    assert_eq!(evidence_json["verdict"], "accepted");
    assert!(evidence.contains("policy_denied_git_push"));
    assert!(evidence.contains("review_missing_tests"));
    assert!(evidence.contains("adapter_transcript"));
    assert!(evidence_json["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|artifact| artifact["artifact_type"] == "adapter_transcript"));

    let report = fs::read_to_string(&summary.report_path).unwrap();
    let run_record_path = summary.run_record_path.display().to_string();
    let evidence_pack_path = summary.evidence_pack_path.display().to_string();
    assert!(report.contains("Local Run Record"));
    assert!(report.contains(&run_record_path));
    assert!(report.contains("Static Export Evidence"));
    assert!(report.contains(&evidence_pack_path));
    assert!(report.contains("Evaluator Closure Evidence"));
    assert!(report.contains("rejected"));
    assert!(report.contains("accepted"));
    assert!(report.contains("Replay Evidence"));
    assert!(report.contains("events.jsonl"));
    assert!(report.contains("Objective"));
    assert!(report.contains("Add input validation to calculate_discount and update tests."));
    assert!(report.contains("Roles"));
    assert!(report.contains("planner"));
    assert!(report.contains("implementer"));
    assert!(report.contains("reviewer"));
    assert!(report.contains("test-engineer"));
    assert!(report.contains("evaluator-closer"));
    assert!(report.contains("Verifier Command"));
    assert!(report.contains("python -m pytest"));
    assert!(report.contains("Acceptance Criteria"));
    assert!(report.contains("risky git push was denied before execution"));
    assert!(report.contains("Run Health"));
    assert!(report.contains("Next Operator Action"));
    assert!(report.contains("No operator action required;"));
    assert!(report.contains("Policy Decisions"));
    assert!(report.contains("git:push"));
    assert!(report.contains("origin main"));
    assert!(report.contains("git push is an external write and is denied in local MVP"));
    assert!(report.contains("Approval Tickets"));
    assert!(report.contains("approved"));
    assert!(report.contains("digest="));
    assert!(report.contains("Artifacts"));
    assert!(report.contains("adapter_transcript"));

    let run_record = fs::read_to_string(&summary.run_record_path).unwrap();
    let run_record_json: serde_json::Value = serde_json::from_str(&run_record).unwrap();
    assert_eq!(run_record_json["closure"]["verdict"], "accepted");
    assert_eq!(
        run_record_json["evidence_pack"].as_str().unwrap(),
        evidence_pack_path
    );
    assert_eq!(
        run_record_json["report"].as_str().unwrap(),
        summary.report_path.display().to_string()
    );

    let events = fs::read_to_string(repo.join(".ao2/runs/test-run/events.jsonl")).unwrap();
    assert!(events.contains("\"event_type\":\"tool.denied\""));
    assert!(events.contains("\"event_type\":\"approval.granted\""));
    assert!(events.contains("\"event_type\":\"adapter.completed\""));
    assert!(events.contains("\"event_type\":\"closure.rejected\""));
    assert!(events.contains("\"event_type\":\"closure.accepted\""));

    restore_env("OPENAI_API_KEY", old_openai);
    restore_env("ANTHROPIC_API_KEY", old_anthropic);
}

#[test]
fn risky_pr_run_fails_closed_when_forbidden_provider_api_key_is_present() {
    let _guard = ENV_LOCK.lock().unwrap();
    let old_openai = std::env::var_os("OPENAI_API_KEY");
    let old_anthropic = std::env::var_os("ANTHROPIC_API_KEY");
    std::env::remove_var("ANTHROPIC_API_KEY");

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    std::env::set_var("OPENAI_API_KEY", "forbidden");
    let result = run_risky_pr_provider_free(RunOptions {
        target_repo: repo,
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("blocked-run".to_string()),
    });
    std::env::remove_var("OPENAI_API_KEY");

    let err = result.unwrap_err().to_string();
    assert!(err.contains("forbidden provider API key"));

    restore_env("OPENAI_API_KEY", old_openai);
    restore_env("ANTHROPIC_API_KEY", old_anthropic);
}

#[test]
fn risky_pr_run_uses_workflow_metadata_from_template_file() {
    let _guard = ENV_LOCK.lock().unwrap();
    let old_openai = std::env::var_os("OPENAI_API_KEY");
    let old_anthropic = std::env::var_os("ANTHROPIC_API_KEY");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("ANTHROPIC_API_KEY");

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let workflow = temp.path().join("custom-bug-fix.yaml");
    fs::write(
        &workflow,
        r#"id: bug-fix
version: 0.1.0
objective: Fix a failing production bug with a minimal patch and regression test.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: python -c "from pathlib import Path; Path('verifier-ran.txt').write_text('yes')"
policy:
  deny_by_default: true
  approval_mode: exact_action_digest
"#,
    )
    .unwrap();

    let summary = run_risky_pr_provider_free(RunOptions {
        target_repo: repo.clone(),
        workflow_path: workflow,
        run_id: Some("template-metadata-run".to_string()),
    })
    .unwrap();
    assert_eq!(summary.status, RunStatus::Accepted);

    let evidence = fs::read_to_string(&summary.evidence_pack_path).unwrap();
    let evidence_json: serde_json::Value = serde_json::from_str(&evidence).unwrap();
    assert_eq!(evidence_json["workflow_id"], "bug-fix@0.1.0");
    assert_eq!(
        evidence_json["objective"],
        "Fix a failing production bug with a minimal patch and regression test."
    );

    let events =
        fs::read_to_string(repo.join(".ao2/runs/template-metadata-run/events.jsonl")).unwrap();
    assert!(events.contains("\"workflow_id\":\"bug-fix@0.1.0\""));
    assert!(events.contains("Fix a failing production bug"));
    assert_eq!(
        fs::read_to_string(repo.join("verifier-ran.txt")).unwrap(),
        "yes"
    );

    restore_env("OPENAI_API_KEY", old_openai);
    restore_env("ANTHROPIC_API_KEY", old_anthropic);
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}
