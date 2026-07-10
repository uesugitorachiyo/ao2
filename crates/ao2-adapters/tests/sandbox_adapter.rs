use std::fs;
use std::path::Path;
use std::process::Command;

use ao2_adapters::{
    apply_sandbox_patch, preview_sandbox_patch, AdapterRunRequest, LocalCliAdapter, ProviderKind,
    SandboxFileKind, SandboxFileState, SandboxPatchApplyRequest, SandboxPatchApprovalSubject,
    SandboxPatchOperation, SandboxPatchOperationKind, SandboxRunRequest,
};

#[test]
fn sandbox_run_captures_diff_without_mutating_target_repo() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("src")).unwrap();
    fs::write(target.join("src/value.txt"), "before\n").unwrap();

    let script = shell_script(
        "printf 'after\\n' > src/value.txt && printf 'created\\n' > src/new.txt",
        "Set-Content -NoNewline -Path 'src/value.txt' -Value \"after`n\"; Set-Content -NoNewline -Path 'src/new.txt' -Value \"created`n\"",
    );
    let request = AdapterRunRequest {
        role_id: "sandbox-test".to_string(),
        command: shell_command(),
        args: shell_args(&script),
        working_dir: Path::new(".").to_path_buf(),
        stdin: None,
        timeout_ms: None,
    };

    let result = LocalCliAdapter::new(ProviderKind::Scripted)
        .run_in_sandbox(SandboxRunRequest {
            target_repo: target.clone(),
            request,
            keep_sandbox: false,
        })
        .unwrap();

    assert!(result.adapter.blocker.is_none());
    assert_eq!(
        fs::read_to_string(target.join("src/value.txt")).unwrap(),
        "before\n"
    );
    assert!(!target.join("src/new.txt").exists());
    assert!(result.changed_files.contains(&"src/value.txt".to_string()));
    assert!(result.changed_files.contains(&"src/new.txt".to_string()));
    assert!(result.diff_summary.contains("modified: src/value.txt"));
    assert!(result.diff_summary.contains("added: src/new.txt"));
    assert!(!result.sandbox_path.exists());
}

#[test]
fn sandbox_run_can_keep_sandbox_for_manual_inspection() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("value.txt"), "before\n").unwrap();

    let request = AdapterRunRequest {
        role_id: "sandbox-keep-test".to_string(),
        command: shell_command(),
        args: shell_args(&shell_script(
            "printf 'after\\n' > value.txt",
            "Set-Content -NoNewline -Path 'value.txt' -Value \"after`n\"",
        )),
        working_dir: Path::new(".").to_path_buf(),
        stdin: None,
        timeout_ms: None,
    };

    let result = LocalCliAdapter::new(ProviderKind::Scripted)
        .run_in_sandbox(SandboxRunRequest {
            target_repo: target,
            request,
            keep_sandbox: true,
        })
        .unwrap();

    assert!(result.sandbox_path.exists());
    assert_eq!(
        fs::read_to_string(result.sandbox_path.join("value.txt")).unwrap(),
        "after\n"
    );
}

#[test]
fn sandbox_patch_apply_requires_exact_digest_and_then_promotes_changes() {
    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(temp.path(), &[("value.txt", b"before\n")]);

    let request = AdapterRunRequest {
        role_id: "sandbox-apply-test".to_string(),
        command: shell_command(),
        args: shell_args(&shell_script(
            "printf 'after\\n' > value.txt && printf 'new\\n' > new.txt",
            "Set-Content -NoNewline -Path 'value.txt' -Value \"after`n\"; Set-Content -NoNewline -Path 'new.txt' -Value \"new`n\"",
        )),
        working_dir: Path::new(".").to_path_buf(),
        stdin: None,
        timeout_ms: None,
    };

    let sandbox = LocalCliAdapter::new(ProviderKind::Scripted)
        .run_in_sandbox(SandboxRunRequest {
            target_repo: target.clone(),
            request,
            keep_sandbox: true,
        })
        .unwrap();

    let preview = preview_sandbox_patch(&target, &sandbox.sandbox_path).unwrap();
    assert!(preview.changed_files.contains(&"value.txt".to_string()));
    assert!(preview.changed_files.contains(&"new.txt".to_string()));

    let wrong = apply_sandbox_patch(SandboxPatchApplyRequest {
        target_repo: target.clone(),
        sandbox_path: sandbox.sandbox_path.clone(),
        expected_digest: "wrong-digest".to_string(),
        approver: "human:test".to_string(),
    });
    assert!(wrong.unwrap_err().to_string().contains("digest mismatch"));
    assert_eq!(
        fs::read_to_string(target.join("value.txt")).unwrap(),
        "before\n"
    );

    let applied = apply_sandbox_patch(SandboxPatchApplyRequest {
        target_repo: target.clone(),
        sandbox_path: sandbox.sandbox_path,
        expected_digest: preview.action_digest,
        approver: "human:test".to_string(),
    })
    .unwrap();

    assert_eq!(applied.applied_files, vec!["new.txt", "value.txt"]);
    assert_eq!(applied.approver, "human:test");
    assert_eq!(
        fs::read_to_string(target.join("value.txt")).unwrap(),
        "after\n"
    );
    assert_eq!(fs::read_to_string(target.join("new.txt")).unwrap(), "new\n");
}

#[test]
fn sandbox_run_rejects_working_dir_that_escapes_sandbox() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("value.txt"), "before\n").unwrap();

    let request = AdapterRunRequest {
        role_id: "sandbox-escape-test".to_string(),
        command: shell_command(),
        args: shell_args(&shell_script(
            "printf 'mutated\\n' > value.txt",
            "Set-Content -NoNewline -Path 'value.txt' -Value \"mutated`n\"",
        )),
        working_dir: target.clone(),
        stdin: None,
        timeout_ms: None,
    };

    let error = LocalCliAdapter::new(ProviderKind::Scripted)
        .run_in_sandbox(SandboxRunRequest {
            target_repo: target.clone(),
            request,
            keep_sandbox: false,
        })
        .unwrap_err();

    assert!(error.to_string().contains("escapes sandbox"));
    assert_eq!(
        fs::read_to_string(target.join("value.txt")).unwrap(),
        "before\n"
    );
}

#[test]
fn sandbox_patch_applies_file_deletions() {
    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(
        temp.path(),
        &[("keep.txt", b"keep\n"), ("remove.txt", b"doomed\n")],
    );

    let request = AdapterRunRequest {
        role_id: "sandbox-delete-test".to_string(),
        command: shell_command(),
        args: shell_args(&shell_script(
            "rm remove.txt",
            "Remove-Item -Path 'remove.txt' -Force",
        )),
        working_dir: Path::new(".").to_path_buf(),
        stdin: None,
        timeout_ms: None,
    };

    let sandbox = LocalCliAdapter::new(ProviderKind::Scripted)
        .run_in_sandbox(SandboxRunRequest {
            target_repo: target.clone(),
            request,
            keep_sandbox: true,
        })
        .unwrap();

    // The sandbox run leaves the target untouched and reports the deletion.
    assert!(target.join("remove.txt").exists());
    assert!(sandbox.changed_files.contains(&"remove.txt".to_string()));
    assert!(sandbox.diff_summary.contains("deleted: remove.txt"));

    let preview = preview_sandbox_patch(&target, &sandbox.sandbox_path).unwrap();
    assert!(preview.changed_files.contains(&"remove.txt".to_string()));

    let applied = apply_sandbox_patch(SandboxPatchApplyRequest {
        target_repo: target.clone(),
        sandbox_path: sandbox.sandbox_path,
        expected_digest: preview.action_digest,
        approver: "human:test".to_string(),
    })
    .unwrap();

    // The deletion is promoted to the target: the file is gone, siblings stay.
    assert!(applied.applied_files.contains(&"remove.txt".to_string()));
    assert!(!target.join("remove.txt").exists());
    assert_eq!(
        fs::read_to_string(target.join("keep.txt")).unwrap(),
        "keep\n"
    );
}

#[test]
fn approval_subject_digest_binds_every_contract_field() {
    let subject = sample_approval_subject();
    let original = subject.action_digest().unwrap();

    let mut different_repo = subject.clone();
    different_repo.repository_identity = format!("sha256:{}", "2".repeat(64));
    assert_ne!(different_repo.action_digest().unwrap(), original);

    let mut different_base = subject.clone();
    different_base.base_commit = "b".repeat(40);
    assert_ne!(different_base.action_digest().unwrap(), original);

    let mut different_operation = subject.clone();
    different_operation.operations[0].kind = SandboxPatchOperationKind::Deleted;
    assert_ne!(different_operation.action_digest().unwrap(), original);

    let mut different_before = subject.clone();
    different_before.operations[0].before = Some(sample_file_state('6'));
    assert_ne!(different_before.action_digest().unwrap(), original);

    let mut reordered = subject.clone();
    reordered.operations.swap(0, 1);
    assert_ne!(reordered.action_digest().unwrap(), original);
}

fn shell_command() -> std::path::PathBuf {
    if cfg!(windows) {
        "powershell".into()
    } else {
        "sh".into()
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_git_target(root: &Path, files: &[(&str, &[u8])]) -> std::path::PathBuf {
    let target = root.join("target");
    fs::create_dir_all(&target).unwrap();
    git(&target, &["init", "--quiet"]);
    git(&target, &["config", "user.name", "AO2 Test"]);
    git(
        &target,
        &["config", "user.email", "ao2-test@example.invalid"],
    );
    for (path, bytes) in files {
        let path = target.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    git(&target, &["add", "-A"]);
    git(&target, &["commit", "--quiet", "-m", "fixture"]);
    target
}

fn sample_file_state(digit: char) -> SandboxFileState {
    SandboxFileState {
        kind: SandboxFileKind::RegularFile,
        content_sha256: Some(format!("sha256:{}", digit.to_string().repeat(64))),
        symlink_target_sha256: None,
        unix_mode: Some(0o644),
    }
}

fn sample_approval_subject() -> SandboxPatchApprovalSubject {
    SandboxPatchApprovalSubject {
        schema_version: "ao2.sandbox-patch-approval-subject.v1".to_string(),
        repository_identity: format!("sha256:{}", "1".repeat(64)),
        base_commit: "a".repeat(40),
        operation_type: "sandbox_patch_apply".to_string(),
        operations: vec![
            SandboxPatchOperation {
                order: 0,
                path: "a.txt".to_string(),
                kind: SandboxPatchOperationKind::Modified,
                before: Some(sample_file_state('3')),
                after: Some(sample_file_state('4')),
            },
            SandboxPatchOperation {
                order: 1,
                path: "b.txt".to_string(),
                kind: SandboxPatchOperationKind::Added,
                before: None,
                after: Some(sample_file_state('5')),
            },
        ],
    }
}

fn shell_args(script: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            "-NoProfile".to_string(),
            "-Command".to_string(),
            script.to_string(),
        ]
    } else {
        vec!["-c".to_string(), script.to_string()]
    }
}

fn shell_script(unix: &str, powershell: &str) -> String {
    if cfg!(windows) {
        powershell.to_string()
    } else {
        unix.to_string()
    }
}
