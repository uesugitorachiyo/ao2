use std::path::PathBuf;

use ao2_adapters::{doctor_provider, AdapterRunRequest, LocalCliAdapter, ProviderKind};

#[test]
fn local_cli_adapter_captures_transcript_and_exit_status() {
    let adapter = LocalCliAdapter::new(ProviderKind::Scripted);
    let current_test_binary = std::env::current_exe().unwrap();

    let result = adapter
        .run(AdapterRunRequest {
            role_id: "adapter-test".to_string(),
            command: current_test_binary,
            args: vec!["--list".to_string()],
            working_dir: PathBuf::from("."),
            stdin: None,
            timeout_ms: None,
        })
        .unwrap();

    assert_eq!(result.provider, ProviderKind::Scripted);
    assert_eq!(result.role_id, "adapter-test");
    assert!(result.exit_code.unwrap_or_default() == 0);
    assert!(result.transcript.contains("local_cli_adapter"));
    assert!(result.blocker.is_none());
}

#[test]
fn adapter_doctor_reports_scripted_provider_without_external_binary() {
    let report = doctor_provider(ProviderKind::Scripted).unwrap();

    assert_eq!(report.provider, ProviderKind::Scripted);
    assert!(report.available);
    assert!(report.version.contains("built-in"));
    assert!(report.blocker.is_none());
}

#[test]
fn local_cli_adapter_times_out_slow_commands() {
    let adapter = LocalCliAdapter::new(ProviderKind::Scripted);
    let (command, args) = if cfg!(windows) {
        (
            PathBuf::from("powershell"),
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Start-Sleep -Seconds 5; Write-Output done".to_string(),
            ],
        )
    } else {
        (PathBuf::from("sleep"), vec!["5".to_string()])
    };

    let result = adapter
        .run(AdapterRunRequest {
            role_id: "timeout-test".to_string(),
            command,
            args,
            working_dir: PathBuf::from("."),
            stdin: None,
            timeout_ms: Some(100),
        })
        .unwrap();

    let blocker = result.blocker.expect("timeout should produce a blocker");
    assert_eq!(blocker.kind, "timeout");
    assert!(blocker.message.contains("100ms"));
    assert!(result.transcript.contains("timed_out: true"));
}

#[test]
fn local_cli_adapter_redacts_sensitive_material_from_persisted_outputs() {
    let adapter = LocalCliAdapter::new(ProviderKind::Scripted);
    let (command, args) = if cfg!(windows) {
        (
            PathBuf::from("powershell"),
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Write-Output 'OPENAI_API_KEY=sk-test-secret'; Write-Output '/api/runs?token=query-secret'; Write-Error 'Authorization: Bearer cp-secret'; Write-Error '--operator-token viewer:viewer:operator-secret'".to_string(),
            ],
        )
    } else {
        (
            PathBuf::from("sh"),
            vec![
                "-c".to_string(),
                "printf 'OPENAI_API_KEY=sk-test-secret\n'; printf '/api/runs?token=query-secret\n'; printf 'Authorization: Bearer cp-secret\n'; printf -- '--operator-token viewer:viewer:operator-secret\n' >&2".to_string(),
            ],
        )
    };

    let result = adapter
        .run(AdapterRunRequest {
            role_id: "redaction-test".to_string(),
            command,
            args,
            working_dir: PathBuf::from("."),
            stdin: None,
            timeout_ms: None,
        })
        .unwrap();

    let persisted = format!("{}\n{}\n{}", result.command, result.stdout, result.stderr);
    assert!(persisted.contains("[redacted sensitive line]"));
    assert!(result.transcript.contains("[redacted sensitive line]"));
    assert!(!persisted.contains("sk-test-secret"));
    assert!(!persisted.contains("cp-secret"));
    assert!(!persisted.contains("query-secret"));
    assert!(!persisted.contains("operator-secret"));
    assert!(!result.transcript.contains("sk-test-secret"));
    assert!(!result.transcript.contains("cp-secret"));
    assert!(!result.transcript.contains("query-secret"));
    assert!(!result.transcript.contains("operator-secret"));
}

#[test]
fn local_cli_adapter_redacts_operator_token_from_shell_diagnostic_continuation() {
    let adapter = LocalCliAdapter::new(ProviderKind::Scripted);
    let (command, args) = if cfg!(windows) {
        (
            PathBuf::from("powershell"),
            vec![
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "Write-Error '+ ... viewer:viewer:operator-secret ...'".to_string(),
            ],
        )
    } else {
        (
            PathBuf::from("sh"),
            vec![
                "-c".to_string(),
                "printf '+ ... viewer:viewer:operator-secret ...\\n' >&2".to_string(),
            ],
        )
    };

    let result = adapter
        .run(AdapterRunRequest {
            role_id: "redaction-diagnostic-test".to_string(),
            command,
            args,
            working_dir: PathBuf::from("."),
            stdin: None,
            timeout_ms: None,
        })
        .unwrap();

    let persisted = format!("{}\n{}\n{}", result.command, result.stdout, result.stderr);
    assert!(persisted.contains("[redacted sensitive line]"));
    assert!(result.transcript.contains("[redacted sensitive line]"));
    assert!(!persisted.contains("operator-secret"));
    assert!(!result.transcript.contains("operator-secret"));
}
