use std::fs;
use std::path::Path;

use ao2_adapters::{
    apply_sandbox_patch, preview_sandbox_patch, AdapterRunRequest, LocalCliAdapter, ProviderKind,
    SandboxPatchApplyRequest, SandboxRunRequest,
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
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("value.txt"), "before\n").unwrap();

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
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("keep.txt"), "keep\n").unwrap();
    fs::write(target.join("remove.txt"), "doomed\n").unwrap();

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

fn shell_command() -> std::path::PathBuf {
    if cfg!(windows) {
        "powershell".into()
    } else {
        "sh".into()
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
