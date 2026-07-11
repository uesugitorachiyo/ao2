use std::fs;
use std::path::Path;
use std::sync::Mutex;

use ao2_runtime::{
    approve_risky_pr_ticket, replay_run, resume_risky_pr_provider_free,
    start_risky_pr_provider_free, ApprovalOptions, ReplayOptions, ResumeOptions, RunOptions,
    RunStatus,
};

mod support;

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
    support::commit_fixture(dst);
}

#[test]
fn risky_pr_run_pauses_for_exact_approval_then_resumes_to_acceptance() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let waiting = start_risky_pr_provider_free(RunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("interactive-run".to_string()),
    })
    .unwrap();

    assert_eq!(waiting.status, RunStatus::WaitingForApproval);
    assert_eq!(waiting.approval_ticket.status, "pending");
    assert!(waiting.run_record_path.exists());

    let unapproved_resume = resume_risky_pr_provider_free(ResumeOptions {
        target_repo: repo.clone(),
        run_id: "interactive-run".to_string(),
    });
    assert!(unapproved_resume
        .unwrap_err()
        .to_string()
        .contains("waiting for approval"));

    let approved = approve_risky_pr_ticket(ApprovalOptions {
        target_repo: repo.clone(),
        ticket_id: waiting.approval_ticket.ticket_id.clone(),
        approver: "human:test-operator".to_string(),
    })
    .unwrap();
    assert_eq!(approved.status, "approved");
    assert_eq!(approved.approver.as_deref(), Some("human:test-operator"));

    let accepted = resume_risky_pr_provider_free(ResumeOptions {
        target_repo: repo.clone(),
        run_id: "interactive-run".to_string(),
    })
    .unwrap();

    assert_eq!(accepted.status, RunStatus::Accepted);
    assert_eq!(accepted.rejection_count, 1);
    assert!(accepted.evidence_pack_path.exists());
    assert!(accepted.report_path.exists());

    let run_record =
        fs::read_to_string(repo.join(".ao2/runs/interactive-run/run-record.json")).unwrap();
    let run_record: serde_json::Value = serde_json::from_str(&run_record).unwrap();
    assert_eq!(run_record["status"], "accepted");
    assert_eq!(run_record["approval_tickets"][0]["status"], "approved");
    let closures = run_record["closures"].as_array().unwrap();
    assert_eq!(closures.len(), 2);
    assert_eq!(closures[0]["verdict"], "rejected");
    assert_eq!(closures[1]["verdict"], "accepted");
    assert_eq!(run_record["closure"]["verdict"], "accepted");

    env.restore();
}

#[test]
fn replay_reconstructs_state_and_rejects_tampered_artifacts() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let waiting = start_risky_pr_provider_free(RunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("replay-run".to_string()),
    })
    .unwrap();
    approve_risky_pr_ticket(ApprovalOptions {
        target_repo: repo.clone(),
        ticket_id: waiting.approval_ticket.ticket_id,
        approver: "human:test-operator".to_string(),
    })
    .unwrap();
    resume_risky_pr_provider_free(ResumeOptions {
        target_repo: repo.clone(),
        run_id: "replay-run".to_string(),
    })
    .unwrap();

    let replay = replay_run(ReplayOptions {
        target_repo: repo.clone(),
        run_id: "replay-run".to_string(),
    })
    .unwrap();
    assert_eq!(replay.status, RunStatus::Accepted);
    assert!(replay.event_count >= 12);
    assert!(replay.artifact_count >= 7);
    assert!(replay.event_types.contains(&"approval.granted".to_string()));
    assert!(replay.digest_failures.is_empty());

    let run_record = fs::read_to_string(repo.join(".ao2/runs/replay-run/run-record.json")).unwrap();
    let run_record: serde_json::Value = serde_json::from_str(&run_record).unwrap();
    let artifact_uri = run_record["artifacts"][0]["uri"].as_str().unwrap();
    fs::write(artifact_uri, "tampered").unwrap();

    let tampered = replay_run(ReplayOptions {
        target_repo: repo,
        run_id: "replay-run".to_string(),
    });
    assert!(tampered
        .unwrap_err()
        .to_string()
        .contains("digest mismatch"));

    env.restore();
}

#[test]
fn replay_rejects_run_record_with_changed_policy_request_digest() {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let waiting = start_risky_pr_provider_free(RunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("policy-binding-replay-run".to_string()),
    })
    .unwrap();
    approve_risky_pr_ticket(ApprovalOptions {
        target_repo: repo.clone(),
        ticket_id: waiting.approval_ticket.ticket_id,
        approver: "human:test-operator".to_string(),
    })
    .unwrap();
    resume_risky_pr_provider_free(ResumeOptions {
        target_repo: repo.clone(),
        run_id: "policy-binding-replay-run".to_string(),
    })
    .unwrap();

    let record_path = repo.join(".ao2/runs/policy-binding-replay-run/run-record.json");
    let mut record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&record_path).unwrap()).unwrap();
    let policy = record["policy_decisions"].as_array_mut().unwrap();
    assert!(!policy.is_empty(), "fixture must record a policy decision");
    policy[0]["request_digest"] = serde_json::json!("sha256:changed-policy-request-digest");
    fs::write(&record_path, serde_json::to_string_pretty(&record).unwrap()).unwrap();

    let result = replay_run(ReplayOptions {
        target_repo: repo,
        run_id: "policy-binding-replay-run".to_string(),
    });
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("run record integrity mismatch"),
        "changed policy request digest must be rejected during replay, got: {err}"
    );

    env.restore();
}

#[test]
fn replay_rejects_run_record_with_reused_approval_identifier() {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let waiting = start_risky_pr_provider_free(RunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("approval-binding-replay-run".to_string()),
    })
    .unwrap();
    approve_risky_pr_ticket(ApprovalOptions {
        target_repo: repo.clone(),
        ticket_id: waiting.approval_ticket.ticket_id,
        approver: "human:test-operator".to_string(),
    })
    .unwrap();
    resume_risky_pr_provider_free(ResumeOptions {
        target_repo: repo.clone(),
        run_id: "approval-binding-replay-run".to_string(),
    })
    .unwrap();

    let record_path = repo.join(".ao2/runs/approval-binding-replay-run/run-record.json");
    let mut record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&record_path).unwrap()).unwrap();
    let approvals = record["approval_tickets"].as_array_mut().unwrap();
    assert!(
        !approvals.is_empty(),
        "fixture must record an approval ticket"
    );
    approvals[0]["ticket_id"] = serde_json::json!("approval-id-reused-from-another-run");
    fs::write(&record_path, serde_json::to_string_pretty(&record).unwrap()).unwrap();

    let result = replay_run(ReplayOptions {
        target_repo: repo,
        run_id: "approval-binding-replay-run".to_string(),
    });
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("run record integrity mismatch"),
        "reused approval identifier must be rejected during replay, got: {err}"
    );

    env.restore();
}

#[test]
fn approve_rejects_tampered_approval_request() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let waiting = start_risky_pr_provider_free(RunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("tamper-run".to_string()),
    })
    .unwrap();
    let ticket_id = waiting.approval_ticket.ticket_id.clone();

    // Tamper the persisted request (not the ticket): the stored ticket's
    // action_digest was computed over the ORIGINAL request, so the approve
    // path's grant_exact() must detect the mismatch and refuse to approve.
    let approval_path = repo.join(format!(".ao2/runs/tamper-run/approvals/{ticket_id}.json"));
    let mut stored: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&approval_path).unwrap()).unwrap();
    stored["request"]["args"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("--force"));
    fs::write(
        &approval_path,
        serde_json::to_string_pretty(&stored).unwrap(),
    )
    .unwrap();

    let result = approve_risky_pr_ticket(ApprovalOptions {
        target_repo: repo.clone(),
        ticket_id,
        approver: "human:test-operator".to_string(),
    });
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("approval digest mismatch"),
        "tampered approval request must be rejected, got: {err}"
    );

    env.restore();
}

#[test]
fn approve_risky_pr_ticket_takes_run_lock_sentinel() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let waiting = start_risky_pr_provider_free(RunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("lock-sentinel-run".to_string()),
    })
    .unwrap();

    // Starting a run does not take the run lock, so no sentinel exists yet.
    let lock_path = repo.join(".ao2/runs/lock-sentinel-run/.lock");
    assert!(
        !lock_path.exists(),
        "no run-lock sentinel should exist before approval"
    );

    approve_risky_pr_ticket(ApprovalOptions {
        target_repo: repo.clone(),
        ticket_id: waiting.approval_ticket.ticket_id,
        approver: "human:test-operator".to_string(),
    })
    .unwrap();

    // The approval read-modify-write runs under the run lock, which materializes the
    // `<run_dir>/.lock` sentinel that serializes concurrent `ao2` processes against
    // the same run.
    assert!(
        lock_path.exists(),
        "approve_risky_pr_ticket must acquire the run lock (creating <run_dir>/.lock)"
    );

    env.restore();
}

#[test]
fn replay_rejects_corrupted_events_file() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);

    start_risky_pr_provider_free(RunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("corrupt-run".to_string()),
    })
    .unwrap();

    // Corrupt the append-only event log with a non-JSON line. Replay parses
    // every line and must surface a clear, located parse error rather than
    // silently skipping or panicking.
    let events_path = repo.join(".ao2/runs/corrupt-run/events.jsonl");
    let mut events = fs::read_to_string(&events_path).unwrap();
    events.push_str("this is not valid json\n");
    fs::write(&events_path, events).unwrap();

    let result = replay_run(ReplayOptions {
        target_repo: repo,
        run_id: "corrupt-run".to_string(),
    });
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("parse event line"),
        "corrupted event log must produce a located parse error, got: {err}"
    );

    env.restore();
}

#[test]
fn replay_rejects_changed_policy_integrity_binding_in_event() {
    let _guard = ENV_LOCK.lock().unwrap();
    let env = EnvSnapshot::clear_for_runtime();

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_fixture(Path::new("../../fixtures/discount-service"), &repo);
    start_risky_pr_provider_free(RunOptions {
        target_repo: repo.clone(),
        workflow_path: Path::new("../../examples/risky-pr-run/risky-pr.yaml").to_path_buf(),
        run_id: Some("policy-event-integrity-run".to_string()),
    })
    .unwrap();

    let events_path = repo.join(".ao2/runs/policy-event-integrity-run/events.jsonl");
    let events = fs::read_to_string(&events_path).unwrap();
    let mut event_values = events
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let event = event_values
        .iter_mut()
        .find(|event| event["policy_integrity"].is_object())
        .expect("policy-bound event must be recorded");
    event["policy_integrity"]["policy_digest"] = serde_json::json!("0".repeat(64));
    fs::write(
        &events_path,
        event_values
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n")
            + "\n",
    )
    .unwrap();

    let replay = replay_run(ReplayOptions {
        target_repo: repo,
        run_id: "policy-event-integrity-run".to_string(),
    });
    assert!(replay
        .unwrap_err()
        .to_string()
        .contains("payload digest mismatch"));

    env.restore();
}

struct EnvSnapshot {
    openai: Option<std::ffi::OsString>,
    anthropic: Option<std::ffi::OsString>,
}

impl EnvSnapshot {
    fn clear_for_runtime() -> Self {
        let snapshot = Self {
            openai: std::env::var_os("OPENAI_API_KEY"),
            anthropic: std::env::var_os("ANTHROPIC_API_KEY"),
        };
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");
        snapshot
    }

    fn restore(self) {
        restore_env("OPENAI_API_KEY", self.openai);
        restore_env("ANTHROPIC_API_KEY", self.anthropic);
    }
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}
