use anyhow::{Context, Result};
use ao2_policy::redact_secrets;
use globset::GlobBuilder;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::manifest::{LevelContract, LoadedManifest, StepContract};
use super::snapshot::{git_state, QualitySnapshot};
use super::{QualityCheckResult, QualityLevel};

const MAX_CAPTURE_BYTES: usize = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const CAPTURE_GRACE: Duration = Duration::from_millis(250);
const SCRUBBED_ENVIRONMENT: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "AO2_PROVIDER",
    "CLAUDE_API_KEY",
    "CODEX_API_KEY",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "OPENAI_API_KEY",
];

#[derive(Debug, Serialize)]
pub(super) struct SelectedStep {
    pub id: String,
    pub reason: &'static str,
    pub matched_triggers: Vec<String>,
    pub matched_paths: Vec<String>,
    pub argv_sha256: String,
}

#[derive(Debug, Serialize)]
pub(super) struct StepResult {
    pub id: String,
    pub status: &'static str,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub failure_code: Option<String>,
    pub descendant_processes_terminated: bool,
    pub duration_ms: u64,
    pub stdout_redacted_sha256: String,
    pub stdout_bytes: u64,
    pub stdout_truncated: bool,
    pub stdout_complete: bool,
    pub stderr_redacted_sha256: String,
    pub stderr_bytes: u64,
    pub stderr_truncated: bool,
    pub stderr_complete: bool,
}

struct Capture {
    bytes: u64,
    truncated: bool,
    complete: bool,
    retained: Vec<u8>,
}

pub(super) fn execute(
    target: &Path,
    level: QualityLevel,
    loaded: LoadedManifest,
    snapshot: QualitySnapshot,
) -> Result<QualityCheckResult> {
    let started = Instant::now();
    let contract = level_contract(&loaded, level);
    let selected_steps = select_steps(level, contract, &snapshot)?;
    let before =
        git_state(target).context("[SOURCE_STATE_READ_FAILED] capture pre-gate Git state")?;
    let mut steps = Vec::new();
    let mut failure_codes = Vec::new();

    for selected in &selected_steps {
        let step = contract
            .steps
            .iter()
            .find(|step| step.id == selected.id)
            .context("[STEP_SELECTION_INVALID] selected step is absent from manifest")?;
        let remaining = Duration::from_secs(contract.maximum_duration_seconds)
            .saturating_sub(started.elapsed());
        if remaining.is_zero() {
            failure_codes.push("LEVEL_TIMEOUT".to_string());
            break;
        }
        let timeout = Duration::from_secs(step.timeout_seconds).min(remaining);
        let result = run_step(target, level, contract.network_allowed, step, timeout)?;
        let passed = result.status == "passed";
        if let Some(code) = &result.failure_code {
            failure_codes.push(code.clone());
        }
        steps.push(result);
        if !passed {
            break;
        }
    }

    let after =
        git_state(target).context("[SOURCE_STATE_READ_FAILED] capture post-gate Git state")?;
    let source_mutation_detected = before != after;
    if source_mutation_detected {
        failure_codes.push("SOURCE_MUTATION_DETECTED".to_string());
    }
    if started.elapsed() > Duration::from_secs(contract.maximum_duration_seconds) {
        failure_codes.push("LEVEL_TIMEOUT".to_string());
    }
    failure_codes.sort();
    failure_codes.dedup();
    let status = if failure_codes.is_empty() {
        "passed"
    } else {
        "failed"
    };
    Ok(QualityCheckResult {
        schema_version: "ao2.quality-check-result.v1",
        status,
        repository: loaded.manifest.repository,
        level: level.as_str(),
        manifest_path: "ao-quality-gates.json",
        manifest_sha256: loaded.sha256,
        source_head: snapshot.head_sha.clone(),
        snapshot,
        selection_status: if selected_steps.is_empty() {
            "not_applicable"
        } else {
            "selected"
        },
        selected_steps,
        steps,
        duration_ms: milliseconds(started.elapsed()),
        source_mutation_detected,
        provider_calls: 0,
        credential_environment_scrubbed: true,
        failure_codes,
    })
}

fn level_contract(loaded: &LoadedManifest, level: QualityLevel) -> &LevelContract {
    match level {
        QualityLevel::Commit => &loaded.manifest.levels.commit,
        QualityLevel::Push => &loaded.manifest.levels.push,
        QualityLevel::Full => &loaded.manifest.levels.full,
    }
}

fn select_steps(
    level: QualityLevel,
    contract: &LevelContract,
    snapshot: &QualitySnapshot,
) -> Result<Vec<SelectedStep>> {
    let mut selected = Vec::new();
    for step in &contract.steps {
        let argv = serde_json::to_vec(&step.argv).context("encode quality step argv")?;
        if matches!(level, QualityLevel::Full) {
            selected.push(SelectedStep {
                id: step.id.clone(),
                reason: "full_source_head",
                matched_triggers: step.path_triggers.clone(),
                matched_paths: Vec::new(),
                argv_sha256: format!("sha256:{:x}", Sha256::digest(argv)),
            });
            continue;
        }
        let mut matched_triggers = Vec::new();
        let mut matched_paths = Vec::new();
        for pattern in &step.path_triggers {
            let matcher = GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .with_context(|| format!("[PATH_PATTERN_INVALID] compile {pattern:?}"))?
                .compile_matcher();
            let paths: Vec<String> = snapshot
                .changed_paths
                .iter()
                .filter(|path| matcher.is_match(path))
                .cloned()
                .collect();
            if !paths.is_empty() {
                matched_triggers.push(pattern.clone());
                matched_paths.extend(paths);
            }
        }
        if !matched_triggers.is_empty() {
            matched_paths.sort();
            matched_paths.dedup();
            selected.push(SelectedStep {
                id: step.id.clone(),
                reason: "changed_path_match",
                matched_triggers,
                matched_paths,
                argv_sha256: format!("sha256:{:x}", Sha256::digest(argv)),
            });
        }
    }
    Ok(selected)
}

fn run_step(
    target: &Path,
    level: QualityLevel,
    network_allowed: bool,
    step: &StepContract,
    timeout: Duration,
) -> Result<StepResult> {
    let started = Instant::now();
    let mut command = Command::new(&step.argv[0]);
    command
        .args(&step.argv[1..])
        .current_dir(target)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("AO2_QUALITY_GATE", "1")
        .env("AO2_QUALITY_LEVEL", level.as_str())
        .env("GIT_TERMINAL_PROMPT", "0");
    for variable in SCRUBBED_ENVIRONMENT {
        command.env_remove(variable);
    }
    if !network_allowed {
        command
            .env("CARGO_NET_OFFLINE", "true")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GOPROXY", "off")
            .env("npm_config_offline", "true")
            .env("PIP_NO_INDEX", "1");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    let process_job = {
        let mut limits = win32job::ExtendedLimitInfo::new();
        limits.limit_kill_on_job_close();
        win32job::Job::create_with_limit_info(&limits)
            .context("[STEP_JOB_FAILED] create kill-on-close process job")?
    };
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return Ok(failed_without_process(
                step,
                "STEP_START_FAILED",
                started.elapsed(),
            ));
        }
    };
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        if process_job
            .assign_process(child.as_raw_handle() as isize)
            .is_err()
        {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(failed_without_process(
                step,
                "STEP_JOB_FAILED",
                started.elapsed(),
            ));
        }
    }
    let stdout = child.stdout.take().context("capture quality step stdout")?;
    let stderr = child.stderr.take().context("capture quality step stderr")?;
    let (stdout_sender, stdout_receiver) = mpsc::sync_channel(1);
    let (stderr_sender, stderr_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = stdout_sender.send(capture(stdout));
    });
    thread::spawn(move || {
        let _ = stderr_sender.send(capture(stderr));
    });

    let deadline = Instant::now() + timeout;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait().context("poll quality step")? {
            break (status, false);
        }
        if Instant::now() >= deadline {
            #[cfg(unix)]
            let _ = terminate_process_tree(child.id());
            child.kill().context("kill timed-out quality step")?;
            break (
                child.wait().context("wait for timed-out quality step")?,
                true,
            );
        }
        thread::sleep(POLL_INTERVAL);
    };
    #[cfg(unix)]
    let descendant_processes_terminated = terminate_process_tree(child.id());
    #[cfg(windows)]
    let descendant_processes_terminated = false;
    #[cfg(windows)]
    drop(process_job);
    let capture_deadline = Instant::now() + CAPTURE_GRACE;
    let stdout = bounded_capture_result(
        stdout_receiver,
        capture_deadline.saturating_duration_since(Instant::now()),
    );
    let stderr = bounded_capture_result(
        stderr_receiver,
        capture_deadline.saturating_duration_since(Instant::now()),
    );
    let failure_code = if timed_out {
        Some("STEP_TIMEOUT".to_string())
    } else if descendant_processes_terminated {
        Some("STEP_DESCENDANT_TERMINATED".to_string())
    } else if !stdout.complete || !stderr.complete {
        Some("STEP_CAPTURE_INCOMPLETE".to_string())
    } else if stdout.truncated || stderr.truncated {
        Some("STEP_OUTPUT_LIMIT".to_string())
    } else if !status.success() {
        Some("STEP_EXIT_NONZERO".to_string())
    } else {
        None
    };
    Ok(StepResult {
        id: step.id.clone(),
        status: if failure_code.is_none() {
            "passed"
        } else {
            "failed"
        },
        exit_code: status.code(),
        timed_out,
        failure_code,
        descendant_processes_terminated,
        duration_ms: milliseconds(started.elapsed()),
        stdout_redacted_sha256: redacted_sha256(&stdout.retained),
        stdout_bytes: stdout.bytes,
        stdout_truncated: stdout.truncated,
        stdout_complete: stdout.complete,
        stderr_redacted_sha256: redacted_sha256(&stderr.retained),
        stderr_bytes: stderr.bytes,
        stderr_truncated: stderr.truncated,
        stderr_complete: stderr.complete,
    })
}

fn capture(mut reader: impl Read) -> Result<Capture> {
    let mut buffer = [0_u8; 8192];
    let mut bytes = 0_u64;
    let mut retained = Vec::new();
    loop {
        let count = reader
            .read(&mut buffer)
            .context("read quality step output")?;
        if count == 0 {
            break;
        }
        bytes = bytes.saturating_add(count as u64);
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..count.min(remaining)]);
    }
    Ok(Capture {
        bytes,
        truncated: bytes > MAX_CAPTURE_BYTES as u64,
        complete: true,
        retained,
    })
}

fn bounded_capture_result(receiver: mpsc::Receiver<Result<Capture>>, timeout: Duration) -> Capture {
    match receiver.recv_timeout(timeout) {
        Ok(Ok(capture)) => capture,
        Ok(Err(_)) | Err(_) => Capture {
            bytes: 0,
            truncated: false,
            complete: false,
            retained: Vec::new(),
        },
    }
}

fn failed_without_process(step: &StepContract, code: &str, duration: Duration) -> StepResult {
    let empty_sha256 = format!("sha256:{:x}", Sha256::digest([]));
    StepResult {
        id: step.id.clone(),
        status: "failed",
        exit_code: None,
        timed_out: false,
        failure_code: Some(code.to_string()),
        descendant_processes_terminated: false,
        duration_ms: milliseconds(duration),
        stdout_redacted_sha256: empty_sha256.clone(),
        stdout_bytes: 0,
        stdout_truncated: false,
        stdout_complete: true,
        stderr_redacted_sha256: empty_sha256,
        stderr_bytes: 0,
        stderr_truncated: false,
        stderr_complete: true,
    }
}

fn redacted_sha256(bytes: &[u8]) -> String {
    let redacted = redact_secrets(&String::from_utf8_lossy(bytes));
    format!("sha256:{:x}", Sha256::digest(redacted.as_bytes()))
}

#[cfg(unix)]
fn terminate_process_tree(pid: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn milliseconds(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}
