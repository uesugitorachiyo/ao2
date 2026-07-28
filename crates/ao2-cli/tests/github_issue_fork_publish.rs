use std::fs;
use std::process::{Command, Output};

use chrono::{Duration, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn ao2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(args)
        .output()
        .expect("run ao2")
}

fn verify(path: &std::path::Path, push: &str, draft: &str) -> Output {
    ao2(&[
        "issue",
        "publish",
        "verify",
        "--plan",
        path.to_str().unwrap(),
        "--expected-push-action-digest",
        push,
        "--expected-draft-action-digest",
        draft,
        "--json",
    ])
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        Value::Object(values) => {
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonicalize(&values[key])))
                    .collect(),
            )
        }
        value => value.clone(),
    }
}

fn action(operation: &str, approved_at: &str, expires_at: &str) -> Value {
    let title = "Fix bounded fixture";
    let body = "Repairs #101 with exact evidence.";
    let mut action = json!({
        "schema": "ao.architecture.autonomous-issue-repair.github-action-digest.v1",
        "run_id": "repair-run-cli-test",
        "repository": "fixture/repair",
        "issue_number": 101,
        "base_sha": "1111111111111111111111111111111111111111",
        "head_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "fork": "operator/repair",
        "branch": "codex/repair-101",
        "pr_title_digest": sha256(title.as_bytes()),
        "pr_body_digest": sha256(body.as_bytes()),
        "diff_digest": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "required_checks": [{
            "name": "test",
            "conclusion": "success",
            "head_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }],
        "action": operation,
        "approved_at": approved_at,
        "expires_at": expires_at,
        "run_envelope_digest": "2222222222222222222222222222222222222222222222222222222222222222",
        "candidate_decision_digest": "3333333333333333333333333333333333333333333333333333333333333333",
        "governance_decision_digest": "4444444444444444444444444444444444444444444444444444444444444444",
        "reviewer_independence_digest": "5555555555555555555555555555555555555555555555555555555555555555",
        "action_digest": ""
    });
    let mut subject = action.clone();
    subject.as_object_mut().unwrap().remove("action_digest");
    action["action_digest"] = Value::String(sha256(
        serde_json::to_string(&canonicalize(&subject))
            .unwrap()
            .as_bytes(),
    ));
    action
}

fn bind_document(mut value: Value, field: &str) -> Value {
    let digest = sha256(
        serde_json::to_string(&canonicalize(&value))
            .unwrap()
            .as_bytes(),
    );
    value[field] = Value::String(digest);
    value
}

fn refresh_action_digests(plan: &mut Value) -> (String, String) {
    for action_name in ["push_action", "draft_action"] {
        plan[action_name]["run_envelope_digest"] =
            plan["authority"]["run_envelope"]["canonical_digest"].clone();
        plan[action_name]["candidate_decision_digest"] =
            plan["authority"]["candidate_decision"]["decision_digest"].clone();
        plan[action_name]["governance_decision_digest"] =
            plan["authority"]["governance_decision"]["decision_digest"].clone();
        plan[action_name]["reviewer_independence_digest"] =
            plan["authority"]["reviewer_independence"]["review_digest"].clone();
        let mut subject = plan[action_name].clone();
        subject.as_object_mut().unwrap().remove("action_digest");
        plan[action_name]["action_digest"] = Value::String(sha256(
            serde_json::to_string(&canonicalize(&subject))
                .unwrap()
                .as_bytes(),
        ));
    }
    (
        plan["push_action"]["action_digest"]
            .as_str()
            .unwrap()
            .to_string(),
        plan["draft_action"]["action_digest"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}

fn publication_plan(now: chrono::DateTime<Utc>) -> (Value, String, String) {
    let approved_at = (now - Duration::minutes(5)).to_rfc3339();
    let expires_at = (now + Duration::minutes(55)).to_rfc3339();
    let authority = json!({
        "run_envelope": bind_document(json!({
            "schema": "ao.architecture.autonomous-issue-repair.run-envelope.v1",
            "run_id": "repair-run-cli-test",
            "loop": {
                "goal": "Repair one bounded issue.",
                "trigger": "Use the pinned repository.",
                "discovery": "Select one authentic bug.",
                "action": "Prepare one repair.",
                "verification": "Require exact checks.",
                "state": "Persist digest-bound state.",
                "human_gates": "Require exact action approval."
            },
            "trigger": {
                "mode": "issue_list",
                "canonical_url": "https://github.com/fixture/repair/issues",
                "repository": "fixture/repair",
                "default_branch": "main",
                "pinned_base_commit": "1111111111111111111111111111111111111111"
            },
            "discovery": {
                "snapshot_limit": 50,
                "candidate_limit": 10,
                "selected_limit": 1
            },
            "budgets": {
                "wall_clock_seconds": 4800,
                "clone_count": 1,
                "test_runs": 2,
                "retry_count": 1,
                "repair_count": 1,
                "publication_count": 1
            },
            "governance": {
                "ownership_class": "external",
                "allowed_actions": ["push_operator_fork", "open_upstream_draft_pr"],
                "denied_actions": [
                    "push_upstream", "open_ready_pr", "mark_ready", "approve_review",
                    "merge", "mutate_issue", "publish_release"
                ],
                "sole_control_auto_merge_opt_in": false
            },
            "routing": {
                "default_branch": "main",
                "pinned_base_commit": "1111111111111111111111111111111111111111",
                "fork_owner": "operator",
                "repair_branch": "codex/repair-101",
                "protected_path_classes": ["workflow"],
                "required_checks": ["test"]
            },
            "created_at": (now - Duration::minutes(20)).to_rfc3339(),
            "expires_at": (now + Duration::minutes(60)).to_rfc3339(),
            "predecessor_digest": null,
            "lineage": {
                "kind": "origin",
                "predecessor_run_id": null,
                "predecessor_digest": null
            },
            "stop_conditions": ["digest_mismatch"],
            "terminal_statuses": ["completed", "blocked"]
        }), "canonical_digest"),
        "candidate_decision": bind_document(json!({
            "schema": "ao.architecture.autonomous-issue-repair.candidate-decision.v1",
            "run_id": "repair-run-cli-test",
            "repository": "fixture/repair",
            "base_sha": "1111111111111111111111111111111111111111",
            "issue_number": 101,
            "rank": 1,
            "decision": "selected",
            "eligibility": {
                "open_bug": true,
                "target_in_repository": true,
                "no_existing_fix": true,
                "current_head_unfixed": true,
                "security_sensitive": false,
                "public_reproduction_feasible": true,
                "deterministic_local_reproduction": true,
                "expected_behavior_grounded": true,
                "bounded_policy_compatible": true
            },
            "reason_codes": ["eligible_all_predicates_passed"],
            "evidence_digests": [
                "6666666666666666666666666666666666666666666666666666666666666666"
            ],
            "expected_behavior_source": "tests",
            "decided_at": (now - Duration::minutes(8)).to_rfc3339()
        }), "decision_digest"),
        "governance_decision": bind_document(json!({
            "schema": "ao.architecture.autonomous-issue-repair.governance-decision.v1",
            "run_id": "repair-run-cli-test",
            "repository": "fixture/repair",
            "base_sha": "1111111111111111111111111111111111111111",
            "head_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "governance_class": "external",
            "classification_sources": ["repository_policy", "operator_envelope"],
            "push_target": "operator_owned_fork",
            "pull_request_mode": "upstream_draft_only",
            "merge": {
                "authorized": false,
                "mode": "never",
                "approval_kind": "none",
                "approval_head_sha": null,
                "auto_merge_opt_in": false,
                "branch_protection_bypassed": false
            },
            "protected_path_touched": false,
            "required_checks": [{
                "name": "test",
                "conclusion": "success",
                "head_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            }],
            "action_digest_required": true,
            "decided_at": (now - Duration::minutes(7)).to_rfc3339()
        }), "decision_digest"),
        "reviewer_independence": bind_document(json!({
            "schema": "ao.architecture.autonomous-issue-repair.reviewer-independence.v1",
            "run_id": "repair-run-cli-test",
            "subject_digest": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            "reviewer_id": "independent-reviewer",
            "status": "independent",
            "deterministic_tests_primary": true,
            "satisfies_team_merge_gate": false,
            "reviewed_at": (now - Duration::minutes(6)).to_rfc3339()
        }), "review_digest")
    });
    let mut push = action("push_operator_fork", &approved_at, &expires_at);
    let mut draft = action("open_upstream_draft_pr", &approved_at, &expires_at);
    for item in [&mut push, &mut draft] {
        item["run_envelope_digest"] = authority["run_envelope"]["canonical_digest"].clone();
        item["candidate_decision_digest"] =
            authority["candidate_decision"]["decision_digest"].clone();
        item["governance_decision_digest"] =
            authority["governance_decision"]["decision_digest"].clone();
        item["reviewer_independence_digest"] =
            authority["reviewer_independence"]["review_digest"].clone();
        let mut subject = item.clone();
        subject.as_object_mut().unwrap().remove("action_digest");
        item["action_digest"] = Value::String(sha256(
            serde_json::to_string(&canonicalize(&subject))
                .unwrap()
                .as_bytes(),
        ));
    }
    let push_digest = push["action_digest"].as_str().unwrap().to_string();
    let draft_digest = draft["action_digest"].as_str().unwrap().to_string();
    (
        json!({
            "schema_version": "ao2.github-repair-publication-plan.v1",
            "architecture_contract_commit": "8e6f247b800b60c520b4e967f7553974a20ec2f8",
            "authority": authority,
            "push_action": push,
            "draft_action": draft,
            "draft": {
                "title": "Fix bounded fixture",
                "body": "Repairs #101 with exact evidence."
            }
        }),
        push_digest,
        draft_digest,
    )
}

#[test]
fn issue_publish_command_is_registered_as_a_separate_bounded_surface() {
    let output = ao2(&["issue", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 help");
    assert!(stdout
        .lines()
        .any(|line| line.trim_start().starts_with("publish ")));
}

#[test]
fn verify_accepts_exact_live_actions_and_reports_zero_writes() {
    let now = Utc::now();
    let (plan, push_digest, draft_digest) = publication_plan(now);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("plan.json");
    fs::write(&path, serde_json::to_vec_pretty(&plan).unwrap()).unwrap();
    let output = verify(&path, &push_digest, &draft_digest);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(readback["status"], "passed");
    assert_eq!(readback["github_contacted"], false);
    assert_eq!(readback["git_write_performed"], false);
    assert_eq!(readback["draft_pr_write_performed"], false);
    assert_eq!(readback["merge_performed"], false);

    let rejected = verify(&path, &"0".repeat(64), &draft_digest);
    assert!(!rejected.status.success());
}

#[test]
fn verify_rejects_unknown_malformed_oversized_and_unsafe_plan_inputs() {
    let now = Utc::now();
    let (mut unknown, push_digest, draft_digest) = publication_plan(now);
    let temp = tempfile::tempdir().unwrap();

    unknown["execute_merge"] = json!(false);
    let unknown_path = temp.path().join("unknown.json");
    fs::write(&unknown_path, serde_json::to_vec(&unknown).unwrap()).unwrap();
    assert!(!verify(&unknown_path, &push_digest, &draft_digest)
        .status
        .success());

    let (mut incomplete, _, _) = publication_plan(now);
    incomplete["authority"]["run_envelope"] = bind_document(
        json!({
            "schema": "ao.architecture.autonomous-issue-repair.run-envelope.v1",
            "run_id": "repair-run-cli-test"
        }),
        "canonical_digest",
    );
    let (incomplete_push, incomplete_draft) = refresh_action_digests(&mut incomplete);
    let incomplete_path = temp.path().join("incomplete.json");
    fs::write(&incomplete_path, serde_json::to_vec(&incomplete).unwrap()).unwrap();
    assert!(
        !verify(&incomplete_path, &incomplete_push, &incomplete_draft)
            .status
            .success()
    );

    let (mut extra_authority, _, _) = publication_plan(now);
    let mut candidate = extra_authority["authority"]["candidate_decision"].clone();
    candidate.as_object_mut().unwrap().remove("decision_digest");
    candidate["execute_merge"] = json!(false);
    extra_authority["authority"]["candidate_decision"] =
        bind_document(candidate, "decision_digest");
    let (extra_push, extra_draft) = refresh_action_digests(&mut extra_authority);
    let extra_path = temp.path().join("extra-authority-field.json");
    fs::write(&extra_path, serde_json::to_vec(&extra_authority).unwrap()).unwrap();
    assert!(!verify(&extra_path, &extra_push, &extra_draft)
        .status
        .success());

    let malformed_path = temp.path().join("malformed.json");
    fs::write(&malformed_path, b"{").unwrap();
    assert!(!verify(&malformed_path, &push_digest, &draft_digest)
        .status
        .success());

    let oversized_path = temp.path().join("oversized.json");
    fs::write(&oversized_path, vec![b'x'; 65_537]).unwrap();
    assert!(!verify(&oversized_path, &push_digest, &draft_digest)
        .status
        .success());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let symlink_path = temp.path().join("symlink.json");
        symlink(&unknown_path, &symlink_path).unwrap();
        assert!(!verify(&symlink_path, &push_digest, &draft_digest)
            .status
            .success());
    }
}
