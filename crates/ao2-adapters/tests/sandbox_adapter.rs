use std::fs;
use std::path::Path;
use std::process::Command;

use ao2_adapters::{
    apply_sandbox_patch, copy_dir_recursive, preview_sandbox_patch, AdapterRunRequest,
    LocalCliAdapter, ProviderKind, SandboxFileKind, SandboxFileState, SandboxPatchApplyRequest,
    SandboxPatchApprovalSubject, SandboxPatchOperation, SandboxPatchOperationKind,
    SandboxRunRequest,
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
        expected_subject: preview.approval_subject.clone(),
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
        expected_subject: preview.approval_subject.clone(),
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
        expected_subject: preview.approval_subject.clone(),
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

#[test]
fn patch_digest_binds_content_base_repository_and_operation_kind() {
    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(temp.path(), &[("value.txt", b"before\n")]);
    let sandbox = sandbox_copy(temp.path(), &target);
    fs::write(sandbox.join("value.txt"), "after-one\n").unwrap();

    let first = preview_sandbox_patch(&target, &sandbox).unwrap();
    assert_eq!(
        first.approval_subject.operations[0].kind,
        SandboxPatchOperationKind::Modified
    );

    fs::write(sandbox.join("value.txt"), "after-two\n").unwrap();
    let different_content = preview_sandbox_patch(&target, &sandbox).unwrap();
    assert_ne!(first.action_digest, different_content.action_digest);
    assert_ne!(
        first.approval_subject.operations[0].after,
        different_content.approval_subject.operations[0].after
    );

    fs::write(target.join("unrelated.txt"), "new base\n").unwrap();
    commit_all(&target, "advance base");
    fs::write(sandbox.join("unrelated.txt"), "new base\n").unwrap();
    fs::write(sandbox.join("value.txt"), "after-one\n").unwrap();
    let different_base = preview_sandbox_patch(&target, &sandbox).unwrap();
    assert_ne!(first.action_digest, different_base.action_digest);
    assert_ne!(
        first.approval_subject.base_commit,
        different_base.approval_subject.base_commit
    );

    let other_root = temp.path().join("other");
    fs::create_dir_all(&other_root).unwrap();
    let other_target = init_git_target(&other_root, &[("value.txt", b"before\n")]);
    let other_sandbox = sandbox_copy(&other_root, &other_target);
    fs::write(other_sandbox.join("value.txt"), "after-one\n").unwrap();
    let different_repo = preview_sandbox_patch(&other_target, &other_sandbox).unwrap();
    assert_ne!(
        first.approval_subject.repository_identity,
        different_repo.approval_subject.repository_identity
    );
    assert_ne!(first.action_digest, different_repo.action_digest);
}

#[test]
fn patch_preview_rejects_non_git_target() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("plain-target");
    let sandbox = temp.path().join("plain-sandbox");
    fs::create_dir_all(&target).unwrap();
    fs::create_dir_all(&sandbox).unwrap();
    fs::write(target.join("value.txt"), "before\n").unwrap();
    fs::write(sandbox.join("value.txt"), "after\n").unwrap();

    let error = preview_sandbox_patch(&target, &sandbox).unwrap_err();
    assert!(error.to_string().contains("Git"), "{error:#}");
}

#[test]
fn preview_emits_sorted_contiguous_operations() {
    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(temp.path(), &[("z.txt", b"z\n"), ("m.txt", b"m\n")]);
    let sandbox = sandbox_copy(temp.path(), &target);
    fs::write(sandbox.join("a.txt"), "a\n").unwrap();
    fs::write(sandbox.join("z.txt"), "changed\n").unwrap();
    fs::remove_file(sandbox.join("m.txt")).unwrap();

    let preview = preview_sandbox_patch(&target, &sandbox).unwrap();
    let paths = preview
        .approval_subject
        .operations
        .iter()
        .map(|operation| operation.path.as_str())
        .collect::<Vec<_>>();
    let orders = preview
        .approval_subject
        .operations
        .iter()
        .map(|operation| operation.order)
        .collect::<Vec<_>>();
    let kinds = preview
        .approval_subject
        .operations
        .iter()
        .map(|operation| operation.kind.clone())
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["a.txt", "m.txt", "z.txt"]);
    assert_eq!(orders, vec![0, 1, 2]);
    assert_eq!(
        kinds,
        vec![
            SandboxPatchOperationKind::Added,
            SandboxPatchOperationKind::Deleted,
            SandboxPatchOperationKind::Modified,
        ]
    );
}

#[cfg(unix)]
#[test]
fn patch_digest_binds_symlink_target_and_executable_mode() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(
        temp.path(),
        &[("tool.sh", b"#!/bin/sh\n"), ("other.sh", b"#!/bin/sh\n")],
    );
    symlink("tool.sh", target.join("tool-link")).unwrap();
    commit_all(&target, "add link");
    let sandbox = sandbox_copy(temp.path(), &target);

    fs::remove_file(sandbox.join("tool-link")).unwrap();
    symlink("other.sh", sandbox.join("tool-link")).unwrap();
    let link_change = preview_sandbox_patch(&target, &sandbox).unwrap();
    let link_operation = link_change
        .approval_subject
        .operations
        .iter()
        .find(|operation| operation.path == "tool-link")
        .unwrap();
    assert_eq!(
        link_operation.after.as_ref().unwrap().kind,
        SandboxFileKind::Symlink
    );

    fs::remove_file(sandbox.join("tool-link")).unwrap();
    symlink("tool.sh", sandbox.join("tool-link")).unwrap();
    let mut permissions = fs::metadata(sandbox.join("tool.sh")).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(sandbox.join("tool.sh"), permissions).unwrap();
    let mode_change = preview_sandbox_patch(&target, &sandbox).unwrap();
    let mode_operation = mode_change
        .approval_subject
        .operations
        .iter()
        .find(|operation| operation.path == "tool.sh")
        .unwrap();
    assert_eq!(
        mode_operation.after.as_ref().unwrap().unix_mode,
        Some(0o755)
    );
    assert_ne!(link_change.action_digest, mode_change.action_digest);
}

#[test]
fn apply_rejects_target_content_changed_after_preview_before_any_write() {
    assert_drift_rejected(|target, _sandbox| {
        fs::write(target.join("a.txt"), "target drift\n").unwrap();
    });
}

#[test]
fn apply_rejects_sandbox_content_changed_after_preview_before_any_write() {
    assert_drift_rejected(|_target, sandbox| {
        fs::write(sandbox.join("a.txt"), "sandbox drift\n").unwrap();
    });
}

#[test]
fn apply_rejects_target_head_changed_after_preview_before_any_write() {
    assert_drift_rejected(|target, _sandbox| {
        fs::write(target.join("unrelated.txt"), "new base\n").unwrap();
        commit_all(target, "advance target head");
    });
}

#[cfg(unix)]
#[test]
fn sandbox_patch_apply_preserves_symlink_and_reports_subject() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(
        temp.path(),
        &[("tool.sh", b"#!/bin/sh\n"), ("other.sh", b"#!/bin/sh\n")],
    );
    symlink("tool.sh", target.join("tool-link")).unwrap();
    commit_all(&target, "add link");
    let sandbox = sandbox_copy(temp.path(), &target);
    fs::remove_file(sandbox.join("tool-link")).unwrap();
    symlink("other.sh", sandbox.join("tool-link")).unwrap();
    let preview = preview_sandbox_patch(&target, &sandbox).unwrap();

    let applied = apply_sandbox_patch(SandboxPatchApplyRequest {
        target_repo: target.clone(),
        sandbox_path: sandbox,
        expected_subject: preview.approval_subject.clone(),
        expected_digest: preview.action_digest.clone(),
        approver: "human:test".to_string(),
    })
    .unwrap();

    assert_eq!(
        fs::read_link(target.join("tool-link")).unwrap(),
        Path::new("other.sh")
    );
    assert_eq!(applied.approval_subject, preview.approval_subject);
    assert_eq!(applied.action_digest, preview.action_digest);
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

fn commit_all(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "--quiet", "-m", message]);
}

fn sandbox_copy(root: &Path, target: &Path) -> std::path::PathBuf {
    let sandbox = root.join("sandbox");
    copy_dir_recursive(target, &sandbox).unwrap();
    sandbox
}

fn assert_drift_rejected(drift: impl FnOnce(&Path, &Path)) {
    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(
        temp.path(),
        &[("a.txt", b"before-a\n"), ("b.txt", b"before-b\n")],
    );
    let sandbox = sandbox_copy(temp.path(), &target);
    fs::write(sandbox.join("a.txt"), "approved-a\n").unwrap();
    fs::write(sandbox.join("b.txt"), "approved-b\n").unwrap();
    let preview = preview_sandbox_patch(&target, &sandbox).unwrap();

    drift(&target, &sandbox);
    let target_a_before_apply = fs::read(target.join("a.txt")).unwrap();
    let target_b_before_apply = fs::read(target.join("b.txt")).unwrap();
    let error = apply_sandbox_patch(SandboxPatchApplyRequest {
        target_repo: target.clone(),
        sandbox_path: sandbox,
        expected_subject: preview.approval_subject,
        expected_digest: preview.action_digest,
        approver: "human:test".to_string(),
    })
    .unwrap_err();

    assert!(
        error.to_string().contains("approval subject mismatch"),
        "{error:#}"
    );
    assert_eq!(
        fs::read(target.join("a.txt")).unwrap(),
        target_a_before_apply
    );
    assert_eq!(
        fs::read(target.join("b.txt")).unwrap(),
        target_b_before_apply
    );
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
