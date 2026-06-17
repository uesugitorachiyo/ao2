use std::fs;
use std::path::Path;
use tempfile::tempdir;

use ao2_runtime::pulse_event_loop::{
    parse_event_loop_decision, run_pulse_event_loop, PulseEventLoopAction,
};

#[test]
fn test_parse_decisions() {
    // Test parsing CODEX_CRON schema
    let codex_continue = r#"{"schema_version":"codex-cron.event-loop-decision.v1","event_loop":{"action":"continue","reason":"more tasks","next_task_id":"t2"}}"#;
    let d1 = parse_event_loop_decision(codex_continue).unwrap();
    assert_eq!(d1.action, PulseEventLoopAction::Continue);
    assert_eq!(d1.reason.as_deref(), Some("more tasks"));
    assert_eq!(d1.next_task_id.as_deref(), Some("t2"));

    let codex_stop = r#"{"schema_version":"codex-cron.event-loop-decision.v1","event_loop":{"action":"stop","reason":"done"}}"#;
    let d2 = parse_event_loop_decision(codex_stop).unwrap();
    assert_eq!(d2.action, PulseEventLoopAction::Stop);
    assert_eq!(d2.reason.as_deref(), Some("done"));

    // Test parsing AO2_PULSE schema
    let ao2_backoff = r#"{"schema_version":"ao2.pulse-event-loop-decision.v1","event_loop":{"action":"backoff","reason":"no change"}}"#;
    let d3 = parse_event_loop_decision(ao2_backoff).unwrap();
    assert_eq!(d3.action, PulseEventLoopAction::Backoff);
    assert_eq!(d3.reason.as_deref(), Some("no change"));

    // Test parsing malformed JSON
    let malformed = r#"{"schema_version":"codex-cron.event-loop-decision.v1","event_loop":{"action":"invalid"}}"#;
    assert!(parse_event_loop_decision(malformed).is_err());
}

#[test]
fn test_malformed_decision_fails_closed() {
    let tmp = tempdir().unwrap();
    let apply_root = tmp.path();
    let out_dir = apply_root.join("out");
    let decision_file = apply_root.join("decision.json");

    fs::write(&decision_file, "{invalid json}").unwrap();

    let summary = run_pulse_event_loop(
        "cargo --version",
        Some(&decision_file),
        3,
        10,
        &out_dir,
        false,
        apply_root,
    )
    .unwrap();

    assert_eq!(summary.status, "failed");
    assert_eq!(summary.iterations, 1);
    assert!(summary.reasons[0].contains("malformed decision"));
}

#[test]
fn test_missing_decision_fails_closed() {
    let tmp = tempdir().unwrap();
    let apply_root = tmp.path();
    let out_dir = apply_root.join("out");
    let decision_file = apply_root.join("missing_decision.json");

    // File doesn't exist, stdout fallback disabled
    let summary = run_pulse_event_loop(
        "cargo --version",
        Some(&decision_file),
        3,
        10,
        &out_dir,
        false,
        apply_root,
    )
    .unwrap();

    assert_eq!(summary.status, "failed");
    assert_eq!(summary.iterations, 1);
    assert!(summary.reasons[0].contains("decision file missing"));
}

#[test]
fn test_max_chain_runs_stops_loop() {
    let tmp = tempdir().unwrap();
    let apply_root = tmp.path();
    let out_dir = apply_root.join("out");
    let decision_file = apply_root.join("decision.json");

    let decision_json = r#"{"schema_version":"ao2.pulse-event-loop-decision.v1","event_loop":{"action":"continue","reason":"keep going","next_task_id":"next"}}"#;
    fs::write(&decision_file, decision_json).unwrap();

    let summary = run_pulse_event_loop(
        "cargo --version",
        Some(&decision_file),
        3, // max chain runs
        10,
        &out_dir,
        false,
        apply_root,
    )
    .unwrap();

    assert_eq!(summary.status, "max_chain_reached");
    assert_eq!(summary.iterations, 3);
}

#[test]
fn test_max_runtime_seconds_stops_loop() {
    let tmp = tempdir().unwrap();
    let apply_root = tmp.path();
    let out_dir = apply_root.join("out");
    let decision_file = apply_root.join("decision.json");

    let decision_json = r#"{"schema_version":"ao2.pulse-event-loop-decision.v1","event_loop":{"action":"continue","reason":"keep going","next_task_id":"next"}}"#;
    fs::write(&decision_file, decision_json).unwrap();

    // Check if python3 is available, else fallback to python
    let python_cmd = if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok()
    {
        "python3"
    } else {
        "python"
    };

    let cmd_str = format!("{} -c \"import time; time.sleep(1.2)\"", python_cmd);

    let summary = run_pulse_event_loop(
        &cmd_str,
        Some(&decision_file),
        5,
        1, // max runtime seconds
        &out_dir,
        false,
        apply_root,
    )
    .unwrap();

    assert_eq!(summary.status, "max_runtime_reached");
    assert!(summary.iterations >= 1);
}

#[test]
fn test_path_resolution_relative_and_absolute() {
    let tmp = tempdir().unwrap();
    let apply_root = tmp.path();
    let out_dir = apply_root.join("out");

    let decision_json = r#"{"schema_version":"ao2.pulse-event-loop-decision.v1","event_loop":{"action":"stop","reason":"relative works"}}"#;

    // Test relative path
    let rel_decision = Path::new("nested/decision.json");
    let rel_decision_abs = apply_root.join(rel_decision);
    fs::create_dir_all(rel_decision_abs.parent().unwrap()).unwrap();
    fs::write(&rel_decision_abs, decision_json).unwrap();

    let summary1 = run_pulse_event_loop(
        "cargo --version",
        Some(rel_decision),
        3,
        10,
        &out_dir,
        false,
        apply_root,
    )
    .unwrap();

    assert_eq!(summary1.status, "stopped");
    assert_eq!(summary1.decision_source, "file");

    // Test absolute path
    let abs_decision = apply_root.join("absolute_decision.json");
    fs::write(&abs_decision, decision_json).unwrap();

    let summary2 = run_pulse_event_loop(
        "cargo --version",
        Some(&abs_decision),
        3,
        10,
        &out_dir,
        false,
        apply_root,
    )
    .unwrap();

    assert_eq!(summary2.status, "stopped");
    assert_eq!(summary2.decision_source, "file");
}

#[test]
fn test_no_provider_api_key_requirement() {
    let old_openai = std::env::var_os("OPENAI_API_KEY");
    let old_anthropic = std::env::var_os("ANTHROPIC_API_KEY");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("ANTHROPIC_API_KEY");

    let tmp = tempdir().unwrap();
    let apply_root = tmp.path();
    let out_dir = apply_root.join("out");
    let decision_file = apply_root.join("decision.json");

    let decision_json = r#"{"schema_version":"ao2.pulse-event-loop-decision.v1","event_loop":{"action":"stop","reason":"no keys needed"}}"#;
    fs::write(&decision_file, decision_json).unwrap();

    let summary = run_pulse_event_loop(
        "cargo --version",
        Some(&decision_file),
        3,
        10,
        &out_dir,
        false,
        apply_root,
    )
    .unwrap();

    assert_eq!(summary.status, "stopped");

    if let Some(val) = old_openai {
        std::env::set_var("OPENAI_API_KEY", val);
    }
    if let Some(val) = old_anthropic {
        std::env::set_var("ANTHROPIC_API_KEY", val);
    }
}
