use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{anyhow, Context, Result};
use ao2_core::sha256_hex;
use ao2_policy::redact_secrets;

use super::json_string;
use crate::cli::GitCommand;

pub(super) fn git(command: GitCommand) -> Result<()> {
    match command {
        GitCommand::Status { target, json } => {
            let result = git_evidence_json(&target, "status", &["status", "--short"])?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print!("{}", json_string(&result, "stdout"));
            }
            Ok(())
        }
        GitCommand::Diff { target, stat, json } => {
            let args: Vec<&str> = if stat {
                vec!["diff", "--stat"]
            } else {
                vec!["diff"]
            };
            let mut result = git_evidence_json(&target, "diff", &args)?;
            result["mode"] = serde_json::json!(if stat { "stat" } else { "patch" });
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print!("{}", json_string(&result, "stdout"));
            }
            Ok(())
        }
        GitCommand::Commit {
            target,
            message,
            paths,
            approve_action_digest,
            approver,
            json,
        } => git_commit_command(
            &target,
            &message,
            &paths,
            approve_action_digest.as_deref(),
            &approver,
            json,
        ),
        GitCommand::Tag {
            target,
            tag,
            message,
            approve_action_digest,
            approver,
            json,
        } => git_tag_command(
            &target,
            &tag,
            message.as_deref(),
            approve_action_digest.as_deref(),
            &approver,
            json,
        ),
    }
}

fn git_evidence_json(target: &Path, operation: &str, args: &[&str]) -> Result<serde_json::Value> {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(target)
        .args(args)
        .output()
        .with_context(|| format!("run git {operation} under {}", target.display()))?;
    let stdout = redact_secrets(&String::from_utf8_lossy(&output.stdout));
    let stderr = redact_secrets(&String::from_utf8_lossy(&output.stderr));
    let exit_code = output.status.code().unwrap_or(-1);
    let schema_version = format!("ao2.git-{operation}.v1");
    let argv = std::iter::once("git".to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "schema_version": schema_version,
        "operation": operation,
        "target": target,
        "argv": argv,
        "exit_code": exit_code,
        "success": output.status.success(),
        "stdout": stdout,
        "stderr": stderr,
        "is_dirty": operation == "status" && !stdout.trim().is_empty(),
        "evidence_sha256": sha256_hex(format!("{operation}\n{stdout}\n{stderr}")),
        "trust_boundary": {
            "mode": "read_only_git_evidence",
            "write_commands_allowed": false,
            "external_writes_allowed": false
        }
    }))
}

fn git_commit_command(
    target: &Path,
    message: &str,
    paths: &[PathBuf],
    approved_digest: Option<&str>,
    approver: &str,
    json: bool,
) -> Result<()> {
    if paths.is_empty() {
        return Err(anyhow!("ao2 git commit requires at least one --path"));
    }
    let request =
        git_write_request(
            target,
            "commit",
            std::iter::once("commit".to_string())
                .chain(["-m".to_string(), message.to_string()])
                .chain(paths.iter().flat_map(|path| {
                    vec!["--path".to_string(), path.to_string_lossy().to_string()]
                }))
                .collect(),
        );
    let digest = request.action_digest();
    require_git_write_approval("commit", &digest, approved_digest, json)?;

    let path_args = paths
        .iter()
        .map(|path| path.as_os_str().to_os_string())
        .collect::<Vec<_>>();
    let add = run_git_os(target, ["add"].map(OsString::from), &path_args)?;
    if !add.status.success() {
        return git_write_failure("commit", target, &digest, "git add", add, json);
    }
    let commit = run_git(target, &["commit", "-m", message])?;
    if !commit.status.success() {
        return git_write_failure("commit", target, &digest, "git commit", commit, json);
    }
    let commit_sha = git_stdout(target, &["rev-parse", "HEAD"])?;
    let result = serde_json::json!({
        "schema_version": "ao2.git-commit.v1",
        "operation": "commit",
        "target": target,
        "success": true,
        "commit_sha": commit_sha.trim(),
        "message": message,
        "paths": paths.iter().map(|path| path.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "stdout": redact_secrets(&String::from_utf8_lossy(&commit.stdout)),
        "stderr": redact_secrets(&String::from_utf8_lossy(&commit.stderr)),
        "approval": {
            "mode": "exact_action_digest",
            "action_digest": digest,
            "approver": approver,
        },
        "trust_boundary": git_write_trust_boundary(),
    });
    print_git_result(&result, json, "commit_sha");
    Ok(())
}

fn git_tag_command(
    target: &Path,
    tag: &str,
    message: Option<&str>,
    approved_digest: Option<&str>,
    approver: &str,
    json: bool,
) -> Result<()> {
    let tag_message = message.unwrap_or(tag);
    let request = git_write_request(
        target,
        "tag",
        vec![
            "tag".to_string(),
            "-a".to_string(),
            tag.to_string(),
            "-m".to_string(),
            tag_message.to_string(),
        ],
    );
    let digest = request.action_digest();
    require_git_write_approval("tag", &digest, approved_digest, json)?;

    let tag_output = run_git(target, &["tag", "-a", tag, "-m", tag_message])?;
    if !tag_output.status.success() {
        return git_write_failure("tag", target, &digest, "git tag", tag_output, json);
    }
    let tag_sha = git_stdout(target, &["rev-parse", tag])?;
    let result = serde_json::json!({
        "schema_version": "ao2.git-tag.v1",
        "operation": "tag",
        "target": target,
        "success": true,
        "tag": tag,
        "tag_sha": tag_sha.trim(),
        "message": tag_message,
        "stdout": redact_secrets(&String::from_utf8_lossy(&tag_output.stdout)),
        "stderr": redact_secrets(&String::from_utf8_lossy(&tag_output.stderr)),
        "approval": {
            "mode": "exact_action_digest",
            "action_digest": digest,
            "approver": approver,
        },
        "trust_boundary": git_write_trust_boundary(),
    });
    print_git_result(&result, json, "tag");
    Ok(())
}

fn git_write_request(target: &Path, operation: &str, args: Vec<String>) -> ao2_policy::ToolRequest {
    ao2_policy::ToolRequest {
        principal: "role:operator".to_string(),
        tool: "git".to_string(),
        operation: operation.to_string(),
        resource: target.to_string_lossy().to_string(),
        args,
        expected_side_effects: vec!["repo_write".to_string()],
    }
}

fn require_git_write_approval(
    operation: &str,
    digest: &str,
    approved_digest: Option<&str>,
    json: bool,
) -> Result<()> {
    if approved_digest == Some(digest) {
        return Ok(());
    }
    let status = if approved_digest.is_some() {
        "approval_digest_mismatch"
    } else {
        "approval_required"
    };
    let result = serde_json::json!({
        "schema_version": format!("ao2.git-{operation}-approval.v1"),
        "operation": operation,
        "status": status,
        "action_digest": digest,
        "approval_mode": "exact_action_digest",
        "required_flag": "--approve-action-digest",
        "trust_boundary": git_write_trust_boundary(),
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("status={status}");
        println!("action_digest={digest}");
    }
    Err(anyhow!(
        "git {operation} requires exact action digest approval"
    ))
}

fn git_write_failure(
    operation: &str,
    target: &Path,
    digest: &str,
    command: &str,
    output: std::process::Output,
    json: bool,
) -> Result<()> {
    let result = serde_json::json!({
        "schema_version": format!("ao2.git-{operation}.v1"),
        "operation": operation,
        "target": target,
        "success": false,
        "failed_command": command,
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": redact_secrets(&String::from_utf8_lossy(&output.stdout)),
        "stderr": redact_secrets(&String::from_utf8_lossy(&output.stderr)),
        "approval": {
            "mode": "exact_action_digest",
            "action_digest": digest,
        },
        "trust_boundary": git_write_trust_boundary(),
    });
    print_git_result(&result, json, "success");
    Err(anyhow!("git {operation} failed at {command}"))
}

fn git_write_trust_boundary() -> serde_json::Value {
    serde_json::json!({
        "mode": "exact_digest_approved_git_write",
        "external_writes_allowed": false,
        "push_allowed": false,
        "broad_stage_allowed": false,
    })
}

fn print_git_result(result: &serde_json::Value, json: bool, plain_key: &str) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        println!("{plain_key}={}", json_string(result, plain_key));
    }
}

fn run_git(target: &Path, args: &[&str]) -> Result<std::process::Output> {
    ProcessCommand::new("git")
        .arg("-C")
        .arg(target)
        .args(args)
        .output()
        .with_context(|| format!("run git {} under {}", args.join(" "), target.display()))
}

fn run_git_os<I>(target: &Path, fixed: I, rest: &[OsString]) -> Result<std::process::Output>
where
    I: IntoIterator<Item = OsString>,
{
    ProcessCommand::new("git")
        .arg("-C")
        .arg(target)
        .args(fixed)
        .arg("--")
        .args(rest)
        .output()
        .with_context(|| format!("run git command under {}", target.display()))
}

fn git_stdout(target: &Path, args: &[&str]) -> Result<String> {
    let output = run_git(target, args)?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
