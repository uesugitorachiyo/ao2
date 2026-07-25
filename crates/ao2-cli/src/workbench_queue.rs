use super::*;
use crate::workbench_app::workbench_provider_pilot_json;

#[derive(Clone)]
pub(super) struct WorkbenchQueue {
    state: Arc<WorkbenchQueueState>,
    sender: mpsc::Sender<WorkbenchJobRequest>,
    active_children: Arc<Mutex<HashMap<String, Child>>>,
    support_signing: Option<WorkbenchSupportSigning>,
}

#[derive(Clone)]
pub(crate) struct WorkbenchSupportSigning {
    pub(crate) key_path: PathBuf,
    pub(crate) signer_id: String,
}

struct WorkbenchQueueState {
    path: PathBuf,
    retention_limit: usize,
    jobs: Mutex<Vec<WorkbenchJob>>,
}

#[derive(Clone, Deserialize, Serialize)]
struct WorkbenchJob {
    job_id: String,
    #[serde(default = "default_workbench_job_kind")]
    job_kind: String,
    run_id: String,
    template: String,
    provider: String,
    provider_prompt_file: String,
    #[serde(default)]
    repair_evidence_pack: String,
    #[serde(default)]
    repair_source_run_id: String,
    max_repair_attempts: usize,
    #[serde(default)]
    max_budget_usd: Option<f64>,
    retry_of: String,
    status: String,
    evidence_pack: String,
    cockpit: String,
    #[serde(default)]
    stdout_log: String,
    #[serde(default)]
    stderr_log: String,
    #[serde(default)]
    queued_at_ms: u64,
    #[serde(default)]
    started_at_ms: u64,
    #[serde(default)]
    finished_at_ms: u64,
    #[serde(default)]
    queue_wait_ms: u64,
    #[serde(default)]
    duration_ms: u64,
    #[serde(default = "default_workbench_exit_code")]
    exit_code: i32,
    #[serde(default)]
    retry_count: usize,
    error: String,
}

struct WorkbenchJobRequest {
    job_id: String,
    job_kind: String,
    target: PathBuf,
    template: String,
    provider: String,
    run_id: String,
    provider_prompt_file: Option<PathBuf>,
    repair_evidence_pack: Option<PathBuf>,
    max_repair_attempts: usize,
    max_budget_usd: Option<f64>,
}

struct WorkbenchJobExecutionResult {
    status: String,
    evidence_pack: String,
    cockpit: String,
    error: String,
    exit_code: i32,
}

#[derive(Debug)]
pub(super) struct WorkbenchMinimumScoreError {
    pub(super) run_id: String,
    pub(super) minimum_score: u64,
    pub(super) score: u64,
    pub(super) verdict: String,
}

impl std::fmt::Display for WorkbenchMinimumScoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "minimum_provider_score_not_met")
    }
}

impl std::error::Error for WorkbenchMinimumScoreError {}

#[derive(Deserialize, Serialize)]
struct WorkbenchQueueFile {
    schema_version: String,
    jobs: Vec<WorkbenchJob>,
}

pub(super) fn start_workbench_queue(
    target: &Path,
    retention_limit: usize,
    support_signing: Option<WorkbenchSupportSigning>,
) -> Result<WorkbenchQueue> {
    let state = Arc::new(load_workbench_queue_state(target, retention_limit)?);
    let (sender, receiver) = mpsc::channel::<WorkbenchJobRequest>();
    let worker_state = state.clone();
    let active_children = Arc::new(Mutex::new(HashMap::<String, Child>::new()));
    let worker_children = active_children.clone();
    thread::spawn(move || {
        while let Ok(request) = receiver.recv() {
            if !mark_workbench_job_running(&worker_state, &request.job_id) {
                continue;
            }
            let result = execute_workbench_job(&request, &worker_state, &worker_children);
            let _ = update_workbench_job(&worker_state, &request.job_id, |job| {
                if job.status == "cancelled" {
                    return;
                }
                let finished_at_ms = now_unix_ms();
                job.finished_at_ms = finished_at_ms;
                job.duration_ms = finished_at_ms.saturating_sub(job.started_at_ms);
                match result {
                    Ok(result) => {
                        job.status = result.status;
                        job.evidence_pack = result.evidence_pack;
                        job.cockpit = result.cockpit;
                        job.error = result.error;
                        job.exit_code = result.exit_code;
                    }
                    Err(error) => {
                        job.status = "failed".to_string();
                        job.error = error.to_string();
                        job.exit_code = 1;
                    }
                }
            });
            let _ = prune_and_persist_workbench_queue(&worker_state);
        }
    });
    Ok(WorkbenchQueue {
        state,
        sender,
        active_children,
        support_signing,
    })
}

impl WorkbenchQueue {
    pub(super) fn enqueue(
        &self,
        target: &Path,
        form: &std::collections::BTreeMap<String, String>,
    ) -> Result<serde_json::Value> {
        let template = form_value(form, "template", "bug-fix");
        if !TASK_TEMPLATES.iter().any(|entry| entry.name == template) {
            anyhow::bail!("unknown template: {template}");
        }
        let provider = form_value(form, "provider", "scripted");
        if !provider_profiles()
            .iter()
            .any(|entry| entry.name == provider)
        {
            anyhow::bail!("unknown provider: {provider}");
        }
        let provider_warnings = provider_warning_strings(provider)?;
        let run_id = form_value_owned(form, "run_id")
            .unwrap_or_else(|| format!("workbench-{}", generate_api_token()));
        let provider_prompt_file =
            form_value_owned(form, "provider_prompt_file").map(PathBuf::from);
        let max_repair_attempts = form
            .get("max_repair_attempts")
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(1);
        let max_budget_usd = parse_optional_budget_form(form, "max_budget_usd")?;
        let minimum_score = form
            .get("minimum_score")
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let scorecard = validate_minimum_provider_score(target, &run_id, minimum_score)?;
        let job_id = format!("job-{}", generate_api_token());
        let (stdout_log, stderr_log) = workbench_job_log_paths(target, &job_id);
        let queued_at_ms = now_unix_ms();
        let job = WorkbenchJob {
            job_id: job_id.clone(),
            job_kind: default_workbench_job_kind(),
            run_id: run_id.clone(),
            template: template.to_string(),
            provider: provider.to_string(),
            provider_prompt_file: provider_prompt_file
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            repair_evidence_pack: String::new(),
            repair_source_run_id: String::new(),
            max_repair_attempts,
            max_budget_usd,
            retry_of: String::new(),
            status: "queued".to_string(),
            evidence_pack: String::new(),
            cockpit: String::new(),
            stdout_log: stdout_log.display().to_string(),
            stderr_log: stderr_log.display().to_string(),
            queued_at_ms,
            started_at_ms: 0,
            finished_at_ms: 0,
            queue_wait_ms: 0,
            duration_ms: 0,
            exit_code: default_workbench_exit_code(),
            retry_count: 0,
            error: String::new(),
        };
        {
            let mut jobs = self.state.jobs.lock().expect("queue lock");
            jobs.push(job.clone());
            prune_workbench_jobs(&mut jobs, self.state.retention_limit);
        }
        persist_workbench_queue(&self.state)?;
        self.sender
            .send(WorkbenchJobRequest {
                job_id: job_id.clone(),
                job_kind: default_workbench_job_kind(),
                target: target.to_path_buf(),
                template: template.to_string(),
                provider: provider.to_string(),
                run_id: run_id.clone(),
                provider_prompt_file,
                repair_evidence_pack: None,
                max_repair_attempts,
                max_budget_usd,
            })
            .context("enqueue workbench job")?;
        append_workbench_audit_event(
            &self.state,
            serde_json::json!({
                "schema_version": "ao2.workbench-audit-event.v1",
                "timestamp_ms": queued_at_ms,
                "action": "start",
                "job_id": job_id,
                "run_id": run_id
            }),
        )?;
        Ok(serde_json::json!({
            "schema_version": "ao2.workbench-queue-start.v1",
            "status": "queued",
            "job_id": job_id,
            "run_id": run_id,
            "provider_warnings": provider_warnings,
            "minimum_score": minimum_score,
            "provider_score": scorecard,
            "retry_of": job.retry_of
        }))
    }

    pub(super) fn enqueue_repair_resume(
        &self,
        target: &Path,
        form: &std::collections::BTreeMap<String, String>,
    ) -> Result<serde_json::Value> {
        let template = form_value(form, "template", "bug-fix");
        if !TASK_TEMPLATES.iter().any(|entry| entry.name == template) {
            anyhow::bail!("unknown template: {template}");
        }
        let provider = form_value(form, "provider", "scripted");
        if !provider_profiles()
            .iter()
            .any(|entry| entry.name == provider)
        {
            anyhow::bail!("unknown provider: {provider}");
        }
        let evidence_pack = PathBuf::from(
            form_value_owned(form, "evidence_pack").context("evidence_pack is required")?,
        );
        let repair_source = repair_source_context_from_evidence_pack(&evidence_pack)?;
        let source_run_id = repair_source.source_run_id;
        let run_id = form_value_owned(form, "run_id")
            .unwrap_or_else(|| format!("workbench-repair-{}", generate_api_token()));
        let provider_prompt_file =
            form_value_owned(form, "provider_prompt_file").map(PathBuf::from);
        let max_repair_attempts = form
            .get("max_repair_attempts")
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(1);
        let max_budget_usd = parse_optional_budget_form(form, "max_budget_usd")?;
        let provider_warnings = provider_warning_strings(provider)?;
        let job_id = format!("job-{}", generate_api_token());
        let (stdout_log, stderr_log) = workbench_job_log_paths(target, &job_id);
        let queued_at_ms = now_unix_ms();
        let job = WorkbenchJob {
            job_id: job_id.clone(),
            job_kind: "repair_resume".to_string(),
            run_id: run_id.clone(),
            template: template.to_string(),
            provider: provider.to_string(),
            provider_prompt_file: provider_prompt_file
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            repair_evidence_pack: evidence_pack.display().to_string(),
            repair_source_run_id: source_run_id.clone(),
            max_repair_attempts,
            max_budget_usd,
            retry_of: String::new(),
            status: "queued".to_string(),
            evidence_pack: String::new(),
            cockpit: String::new(),
            stdout_log: stdout_log.display().to_string(),
            stderr_log: stderr_log.display().to_string(),
            queued_at_ms,
            started_at_ms: 0,
            finished_at_ms: 0,
            queue_wait_ms: 0,
            duration_ms: 0,
            exit_code: default_workbench_exit_code(),
            retry_count: 0,
            error: String::new(),
        };
        {
            let mut jobs = self.state.jobs.lock().expect("queue lock");
            jobs.push(job.clone());
            prune_workbench_jobs(&mut jobs, self.state.retention_limit);
        }
        persist_workbench_queue(&self.state)?;
        self.sender
            .send(WorkbenchJobRequest {
                job_id: job_id.clone(),
                job_kind: "repair_resume".to_string(),
                target: target.to_path_buf(),
                template: template.to_string(),
                provider: provider.to_string(),
                run_id: run_id.clone(),
                provider_prompt_file,
                repair_evidence_pack: Some(evidence_pack),
                max_repair_attempts,
                max_budget_usd,
            })
            .context("enqueue repair resume workbench job")?;
        append_workbench_audit_event(
            &self.state,
            serde_json::json!({
                "schema_version": "ao2.workbench-audit-event.v1",
                "timestamp_ms": queued_at_ms,
                "action": "repair_resume_start",
                "job_id": job_id,
                "run_id": run_id,
                "source_run_id": source_run_id
            }),
        )?;
        Ok(serde_json::json!({
            "schema_version": "ao2.workbench-repair-resume-start.v1",
            "status": "queued",
            "job_id": job_id,
            "run_id": run_id,
            "source_run_id": source_run_id,
            "repair_evidence_pack": job.repair_evidence_pack,
            "provider_warnings": provider_warnings,
            "retry_of": job.retry_of
        }))
    }

    pub(super) fn enqueue_provider_pilot(
        &self,
        target: &Path,
        form: &std::collections::BTreeMap<String, String>,
    ) -> Result<serde_json::Value> {
        let pilot = workbench_provider_pilot_json(target, form)?;
        if json_string(&pilot, "status") != "ready" {
            return Ok(pilot);
        }
        let approval = provider_pilot_approval_from_form(&pilot, form)?;
        if json_string(&approval, "status") != "approved_exact_action_digest" {
            return Ok(approval);
        }

        let template = json_string(&pilot, "template");
        let provider = json_string(&pilot, "provider");
        let run_id = json_string(&pilot, "run_id");
        let provider_prompt_file = PathBuf::from(json_string(&pilot, "provider_prompt_file"));
        let max_repair_attempts =
            usize::try_from(json_u64(&pilot, "max_repair_attempts")).unwrap_or(1);
        let max_budget_usd = pilot
            .get("max_budget_usd")
            .and_then(serde_json::Value::as_f64);
        let provider_warnings = provider_warning_strings(&provider)?;
        let job_id = format!("job-{}", generate_api_token());
        let (stdout_log, stderr_log) = workbench_job_log_paths(target, &job_id);
        let queued_at_ms = now_unix_ms();
        let job = WorkbenchJob {
            job_id: job_id.clone(),
            job_kind: default_workbench_job_kind(),
            run_id: run_id.clone(),
            template: template.clone(),
            provider: provider.clone(),
            provider_prompt_file: provider_prompt_file.display().to_string(),
            repair_evidence_pack: String::new(),
            repair_source_run_id: String::new(),
            max_repair_attempts,
            max_budget_usd,
            retry_of: String::new(),
            status: "queued".to_string(),
            evidence_pack: String::new(),
            cockpit: String::new(),
            stdout_log: stdout_log.display().to_string(),
            stderr_log: stderr_log.display().to_string(),
            queued_at_ms,
            started_at_ms: 0,
            finished_at_ms: 0,
            queue_wait_ms: 0,
            duration_ms: 0,
            exit_code: default_workbench_exit_code(),
            retry_count: 0,
            error: String::new(),
        };
        {
            let mut jobs = self.state.jobs.lock().expect("queue lock");
            jobs.push(job.clone());
            prune_workbench_jobs(&mut jobs, self.state.retention_limit);
        }
        persist_workbench_queue(&self.state)?;
        self.sender
            .send(WorkbenchJobRequest {
                job_id: job_id.clone(),
                job_kind: default_workbench_job_kind(),
                target: target.to_path_buf(),
                template,
                provider,
                run_id: run_id.clone(),
                provider_prompt_file: Some(provider_prompt_file),
                repair_evidence_pack: None,
                max_repair_attempts,
                max_budget_usd,
            })
            .context("enqueue provider pilot job")?;
        append_workbench_audit_event(
            &self.state,
            serde_json::json!({
                "schema_version": "ao2.workbench-audit-event.v1",
                "timestamp_ms": queued_at_ms,
                "action": "provider_pilot_start",
                "job_id": job_id,
                "run_id": run_id
            }),
        )?;
        Ok(serde_json::json!({
            "schema_version": "ao2.workbench-provider-pilot-start.v1",
            "status": "queued",
            "job_id": job_id,
            "run_id": run_id,
            "max_budget_usd": max_budget_usd,
            "provider_warnings": provider_warnings,
            "pilot": pilot,
            "approval": approval,
            "retry_of": job.retry_of
        }))
    }

    pub(super) fn cancel(
        &self,
        form: &std::collections::BTreeMap<String, String>,
    ) -> Result<serde_json::Value> {
        let job_id = form_value_owned(form, "job_id").context("job_id is required")?;
        let killed_child = {
            let mut children = self.active_children.lock().expect("active child lock");
            if let Some(child) = children.get_mut(&job_id) {
                terminate_workbench_child(child);
                true
            } else {
                false
            }
        };
        let (status, cancel_applied) = {
            let mut jobs = self.state.jobs.lock().expect("queue lock");
            let job = jobs
                .iter_mut()
                .find(|job| job.job_id == job_id)
                .with_context(|| format!("unknown job_id: {job_id}"))?;
            if !matches!(job.status.as_str(), "queued" | "running") {
                (job.status.clone(), false)
            } else {
                job.status = "cancelled".to_string();
                let finished_at_ms = now_unix_ms();
                job.finished_at_ms = finished_at_ms;
                if job.started_at_ms == 0 {
                    job.queue_wait_ms = finished_at_ms.saturating_sub(job.queued_at_ms);
                } else {
                    job.duration_ms = finished_at_ms.saturating_sub(job.started_at_ms);
                }
                job.exit_code = -1;
                job.error = if killed_child {
                    "running child process was terminated by operator".to_string()
                } else {
                    "job was cancelled by operator".to_string()
                };
                (job.status.clone(), true)
            }
        };
        persist_workbench_queue(&self.state)?;
        append_workbench_audit_event(
            &self.state,
            serde_json::json!({
                "schema_version": "ao2.workbench-audit-event.v1",
                "timestamp_ms": now_unix_ms(),
                "action": "cancel",
                "job_id": job_id,
                "killed_child": killed_child,
                "cancel_applied": cancel_applied,
                "status": status
            }),
        )?;
        Ok(serde_json::json!({
            "schema_version": "ao2.workbench-queue-cancel.v1",
            "job_id": job_id,
            "status": status,
            "cancel_applied": cancel_applied
        }))
    }

    pub(super) fn retry(
        &self,
        target: &Path,
        form: &std::collections::BTreeMap<String, String>,
    ) -> Result<serde_json::Value> {
        let original_job_id = form_value_owned(form, "job_id").context("job_id is required")?;
        let original = self
            .state
            .jobs
            .lock()
            .expect("queue lock")
            .iter()
            .find(|job| job.job_id == original_job_id)
            .cloned()
            .with_context(|| format!("unknown job_id: {original_job_id}"))?;
        let job_id = format!("job-{}", generate_api_token());
        let run_id = format!("{}-retry-{}", original.run_id, generate_api_token());
        let (stdout_log, stderr_log) = workbench_job_log_paths(target, &job_id);
        let queued_at_ms = now_unix_ms();
        let provider_prompt_file = if original.provider_prompt_file.is_empty() {
            None
        } else {
            Some(PathBuf::from(&original.provider_prompt_file))
        };
        let repair_evidence_pack = if original.repair_evidence_pack.is_empty() {
            None
        } else {
            Some(PathBuf::from(&original.repair_evidence_pack))
        };
        let job = WorkbenchJob {
            job_id: job_id.clone(),
            job_kind: original.job_kind.clone(),
            run_id: run_id.clone(),
            template: original.template.clone(),
            provider: original.provider.clone(),
            provider_prompt_file: original.provider_prompt_file.clone(),
            repair_evidence_pack: original.repair_evidence_pack.clone(),
            repair_source_run_id: original.repair_source_run_id.clone(),
            max_repair_attempts: original.max_repair_attempts,
            max_budget_usd: original.max_budget_usd,
            retry_of: original_job_id.clone(),
            status: "queued".to_string(),
            evidence_pack: String::new(),
            cockpit: String::new(),
            stdout_log: stdout_log.display().to_string(),
            stderr_log: stderr_log.display().to_string(),
            queued_at_ms,
            started_at_ms: 0,
            finished_at_ms: 0,
            queue_wait_ms: 0,
            duration_ms: 0,
            exit_code: default_workbench_exit_code(),
            retry_count: original.retry_count + 1,
            error: String::new(),
        };
        {
            let mut jobs = self.state.jobs.lock().expect("queue lock");
            jobs.push(job.clone());
            prune_workbench_jobs(&mut jobs, self.state.retention_limit);
        }
        persist_workbench_queue(&self.state)?;
        self.sender
            .send(WorkbenchJobRequest {
                job_id: job_id.clone(),
                job_kind: job.job_kind.clone(),
                target: target.to_path_buf(),
                template: job.template.clone(),
                provider: job.provider.clone(),
                run_id: run_id.clone(),
                provider_prompt_file,
                repair_evidence_pack,
                max_repair_attempts: job.max_repair_attempts,
                max_budget_usd: job.max_budget_usd,
            })
            .context("enqueue retried workbench job")?;
        append_workbench_audit_event(
            &self.state,
            serde_json::json!({
                "schema_version": "ao2.workbench-audit-event.v1",
                "timestamp_ms": queued_at_ms,
                "action": "retry",
                "job_id": job_id,
                "run_id": run_id,
                "retry_of": original_job_id
            }),
        )?;
        Ok(serde_json::json!({
            "schema_version": "ao2.workbench-queue-start.v1",
            "status": "queued",
            "job_id": job_id,
            "run_id": run_id,
            "retry_of": original_job_id
        }))
    }

    pub(super) fn job_detail(&self, query: &str) -> Result<serde_json::Value> {
        let job_id = query_value_owned(query, "job_id").context("job_id is required")?;
        let job = self
            .state
            .jobs
            .lock()
            .expect("queue lock")
            .iter()
            .find(|job| job.job_id == job_id)
            .cloned()
            .with_context(|| format!("unknown job_id: {job_id}"))?;
        let stdout = read_optional_text(&job.stdout_log)?;
        let stderr = read_optional_text(&job.stderr_log)?;
        let diagnosis = workbench_job_diagnosis(&job, &stdout, &stderr);
        Ok(serde_json::json!({
            "schema_version": "ao2.workbench-queue-job.v1",
            "job": workbench_job_json(&job),
            "stdout": stdout,
            "stderr": stderr,
            "diagnosis": diagnosis
        }))
    }

    pub(super) fn job_logs(&self, query: &str) -> Result<serde_json::Value> {
        let job_id = query_value_owned(query, "job_id").context("job_id is required")?;
        let tail_bytes = workbench_log_tail_bytes(query);
        let job = self
            .state
            .jobs
            .lock()
            .expect("queue lock")
            .iter()
            .find(|job| job.job_id == job_id)
            .cloned()
            .with_context(|| format!("unknown job_id: {job_id}"))?;
        Ok(serde_json::json!({
            "schema_version": "ao2.workbench-queue-job-logs.v1",
            "job": workbench_job_json(&job),
            "stdout": read_log_tail(&job.stdout_log, tail_bytes)?,
            "stderr": read_log_tail(&job.stderr_log, tail_bytes)?
        }))
    }

    pub(super) fn job_detail_page(&self, query: &str) -> Result<String> {
        let detail = self.job_detail(query)?;
        Ok(render_workbench_job_detail_page(&detail))
    }

    pub(super) fn audit_json(&self, query: &str) -> Result<serde_json::Value> {
        let action_filter = query_value_owned(query, "action");
        let job_id_filter = query_value_owned(query, "job_id");
        let events = read_workbench_audit_events(&workbench_audit_path(&self.state))?
            .into_iter()
            .filter(|event| {
                action_filter.as_deref().is_none_or(|action| {
                    event.get("action").and_then(serde_json::Value::as_str) == Some(action)
                }) && job_id_filter.as_deref().is_none_or(|job_id| {
                    event.get("job_id").and_then(serde_json::Value::as_str) == Some(job_id)
                })
            })
            .collect::<Vec<_>>();
        Ok(serde_json::json!({
            "schema_version": "ao2.workbench-audit.v1",
            "filters": {
                "action": action_filter,
                "job_id": job_id_filter
            },
            "events": events
        }))
    }

    pub(super) fn export_support_bundle(&self, target: &Path) -> Result<serde_json::Value> {
        let generated_at_ms = now_unix_ms();
        let bundle = self.support_bundle_payload(target, generated_at_ms, true)?;
        let bundle_path = workbench_support_bundle_path(target, generated_at_ms);
        if let Some(parent) = bundle_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        atomic_write_text(&bundle_path, &serde_json::to_string_pretty(&bundle)?)?;
        let support_metadata = if let Some(signing) = &self.support_signing {
            write_workbench_support_metadata(
                target,
                &bundle,
                &bundle_path,
                generated_at_ms,
                signing,
            )?
        } else {
            serde_json::json!({
                "present": false,
                "signature_verified": false
            })
        };
        Ok(serde_json::json!({
            "schema_version": "ao2.workbench-support-bundle.v1",
            "bundle_path": bundle_path,
            "support_metadata": support_metadata,
            "bundle": bundle
        }))
    }

    pub(super) fn preview_support_bundle(&self, target: &Path) -> Result<serde_json::Value> {
        let generated_at_ms = now_unix_ms();
        let bundle = self.support_bundle_payload(target, generated_at_ms, false)?;
        let queue_job_count = bundle["queue"]["jobs"].as_array().map_or(0, Vec::len);
        let audit_event_count = bundle["audit_events"].as_array().map_or(0, Vec::len);
        let job_log_count = bundle["job_logs"].as_array().map_or(0, Vec::len);
        let evidence_export_count = bundle["evidence_exports"].as_array().map_or(0, Vec::len);
        Ok(serde_json::json!({
            "schema_version": "ao2.workbench-support-bundle-preview.v1",
            "generated_at_ms": generated_at_ms,
            "would_write_bundle": false,
            "support_signing_enabled": self.support_signing.is_some(),
            "queue_job_count": queue_job_count,
            "audit_event_count": audit_event_count,
            "job_log_count": job_log_count,
            "evidence_export_count": evidence_export_count,
            "redaction_audit": bundle["redaction_audit"].clone(),
            "redaction_preview": workbench_support_bundle_redaction_preview(&bundle)
        }))
    }

    fn support_bundle_payload(
        &self,
        target: &Path,
        generated_at_ms: u64,
        redact_logs: bool,
    ) -> Result<serde_json::Value> {
        let queue = self.to_json("");
        let audit_events = read_workbench_audit_events(&workbench_audit_path(&self.state))?;
        let jobs = self.state.jobs.lock().expect("queue lock").clone();
        let mut raw_job_logs = Vec::new();
        let mut job_logs = Vec::new();
        for job in &jobs {
            let stdout = read_optional_text(&job.stdout_log)?;
            let stderr = read_optional_text(&job.stderr_log)?;
            raw_job_logs.push(serde_json::json!({
                "job": workbench_job_json(job),
                "stdout": stdout.clone(),
                "stderr": stderr.clone(),
                "diagnosis": workbench_job_diagnosis(job, &stdout, &stderr)
            }));
            let redacted_stdout = if redact_logs {
                redact_secrets(&stdout)
            } else {
                stdout
            };
            let redacted_stderr = if redact_logs {
                redact_secrets(&stderr)
            } else {
                stderr
            };
            job_logs.push(serde_json::json!({
                    "job": workbench_job_json(job),
                "stdout": redacted_stdout,
                "stderr": redacted_stderr,
                "diagnosis": workbench_job_diagnosis(job, &redacted_stdout, &redacted_stderr)
            }));
        }
        let evidence_exports = workbench_evidence_exports_for_support_bundle(target)?;
        let hermes_project_start_flow_contract =
            embedded_project_start_hermes_flow_contract_json(target)?;
        let redaction_audit_source = serde_json::json!({
            "job_logs": raw_job_logs
        });
        let redaction_audit = workbench_support_bundle_redaction_audit(&redaction_audit_source);
        let bundle = serde_json::json!({
            "schema_version": "ao2.workbench-support-bundle.v1",
            "generated_at_ms": generated_at_ms,
            "target": target,
            "queue": queue,
            "audit_events": audit_events,
            "job_logs": job_logs,
            "hermes_project_start_flow_contract": hermes_project_start_flow_contract,
            "evidence_exports": evidence_exports,
            "redaction_audit": redaction_audit
        });
        Ok(bundle)
    }

    pub(super) fn to_json(&self, query: &str) -> serde_json::Value {
        let status_filter = query_value_owned(query, "status");
        let template_filter = query_value_owned(query, "template");
        let jobs = self
            .state
            .jobs
            .lock()
            .expect("queue lock")
            .iter()
            .filter(|job| {
                status_filter
                    .as_deref()
                    .is_none_or(|status| job.status == status)
                    && template_filter
                        .as_deref()
                        .is_none_or(|template| job.template == template)
            })
            .map(workbench_job_json)
            .collect::<Vec<_>>();
        serde_json::json!({
            "schema_version": "ao2.workbench-queue.v1",
            "execution_enabled": true,
            "filters": {
                "status": status_filter,
                "template": template_filter
            },
            "jobs": jobs
        })
    }
}

pub(super) fn disabled_queue_json() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.workbench-queue.v1",
        "execution_enabled": false,
        "jobs": []
    })
}

fn load_workbench_queue_state(
    target: &Path,
    retention_limit: usize,
) -> Result<WorkbenchQueueState> {
    let path = workbench_queue_path(target);
    let mut jobs = if path.exists() {
        let content =
            fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str::<WorkbenchQueueFile>(&content)
            .with_context(|| format!("parse {}", path.display()))?
            .jobs
    } else {
        Vec::new()
    };
    let mut changed = false;
    for job in &mut jobs {
        if matches!(
            job.status.as_str(),
            "queued" | "running" | "cancel_requested"
        ) {
            job.status = "interrupted".to_string();
            job.error = "workbench server restarted before job completed".to_string();
            changed = true;
        }
    }
    changed |= prune_workbench_jobs(&mut jobs, retention_limit);
    let state = WorkbenchQueueState {
        path,
        retention_limit,
        jobs: Mutex::new(jobs),
    };
    if changed {
        persist_workbench_queue(&state)?;
    }
    Ok(state)
}

fn workbench_queue_path(target: &Path) -> PathBuf {
    target.join(".ao2").join("workbench").join("queue.json")
}

pub(super) fn read_workbench_queue_file(target: &Path) -> Result<serde_json::Value> {
    let path = workbench_queue_path(target);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(serde_json::json!({
                "schema_version": "ao2.workbench-queue-file.v1",
                "jobs": []
            }));
        }
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))
}

fn provider_pilot_approval_from_form(
    pilot: &serde_json::Value,
    form: &std::collections::BTreeMap<String, String>,
) -> Result<serde_json::Value> {
    let Some(packet) = pilot.get("approval_packet") else {
        anyhow::bail!("provider pilot approval packet missing");
    };
    let expected_digest = json_string(packet, "action_digest");
    if expected_digest.is_empty() {
        anyhow::bail!("provider pilot approval action digest missing");
    }
    let submitted_digest = form_value_owned(form, "approval_action_digest").unwrap_or_default();
    if submitted_digest != expected_digest {
        let mut required = packet.clone();
        required["status"] = serde_json::Value::String(if submitted_digest.is_empty() {
            "approval_required".to_string()
        } else {
            "approval_digest_mismatch".to_string()
        });
        required["pilot"] = pilot.clone();
        return Ok(required);
    }
    Ok(serde_json::json!({
        "schema_version": "ao2.provider-pilot-approval.v1",
        "status": "approved_exact_action_digest",
        "approval_mode": json_string(packet, "approval_mode"),
        "action_digest": expected_digest,
        "provider": json_string(pilot, "provider"),
        "run_id": json_string(pilot, "run_id")
    }))
}

fn workbench_job_log_paths(target: &Path, job_id: &str) -> (PathBuf, PathBuf) {
    let dir = target
        .join(".ao2")
        .join("workbench")
        .join("jobs")
        .join(job_id);
    (dir.join("stdout.txt"), dir.join("stderr.txt"))
}

fn workbench_audit_path(state: &WorkbenchQueueState) -> PathBuf {
    state
        .path
        .parent()
        .map(|parent| parent.join("audit.jsonl"))
        .unwrap_or_else(|| PathBuf::from("audit.jsonl"))
}

pub(super) fn workbench_audit_path_for_target(target: &Path) -> PathBuf {
    target.join(".ao2").join("workbench").join("audit.jsonl")
}

pub(super) fn read_workbench_audit_events(path: &Path) -> Result<Vec<serde_json::Value>> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let mut events = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event = serde_json::from_str::<serde_json::Value>(line)
            .with_context(|| format!("parse {} line {}", path.display(), index + 1))?;
        events.push(event);
    }
    Ok(events)
}

fn append_workbench_audit_event(
    state: &WorkbenchQueueState,
    event: serde_json::Value,
) -> Result<()> {
    let path = workbench_audit_path(state);
    append_workbench_audit_event_at_path(&path, event)
}

pub(super) fn append_workbench_audit_event_for_target(
    target: &Path,
    event: serde_json::Value,
) -> Result<()> {
    let path = workbench_audit_path_for_target(target);
    append_workbench_audit_event_at_path(&path, event)
}

fn append_workbench_audit_event_at_path(path: &Path, event: serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&event)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn default_workbench_exit_code() -> i32 {
    -1
}

fn default_workbench_job_kind() -> String {
    "run".to_string()
}

pub(crate) fn now_unix_ms() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    unix_ms_from_duration(duration)
}

pub(super) fn unix_ms_from_duration(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn prune_workbench_jobs(jobs: &mut Vec<WorkbenchJob>, retention_limit: usize) -> bool {
    let original_len = jobs.len();
    while jobs.len() > retention_limit {
        let Some(index) = jobs
            .iter()
            .position(|job| !matches!(job.status.as_str(), "queued" | "running"))
        else {
            break;
        };
        jobs.remove(index);
    }
    jobs.len() != original_len
}

fn prune_and_persist_workbench_queue(state: &WorkbenchQueueState) -> Result<()> {
    let mut jobs = state.jobs.lock().expect("queue lock");
    prune_workbench_jobs(&mut jobs, state.retention_limit);
    persist_workbench_queue_locked(state, &jobs)
}

fn persist_workbench_queue(state: &WorkbenchQueueState) -> Result<()> {
    let jobs = state.jobs.lock().expect("queue lock");
    persist_workbench_queue_locked(state, &jobs)
}

fn persist_workbench_queue_locked(
    state: &WorkbenchQueueState,
    jobs: &[WorkbenchJob],
) -> Result<()> {
    if let Some(parent) = state.path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let file = WorkbenchQueueFile {
        schema_version: "ao2.workbench-queue-file.v1".to_string(),
        jobs: jobs.to_vec(),
    };
    let content = serde_json::to_string_pretty(&file)?;
    atomic_write_text(&state.path, &content)?;
    Ok(())
}

pub(crate) fn atomic_write_text(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    // Route through the shared ao2-core durable writer: temp file + write_all +
    // sync_all + atomic rename, with the temp cleaned up on any error. This is
    // the same write discipline the AO2 evidence boundary depends on, so a crash
    // or power loss can never truncate the destination to a zero-length file or
    // strew half-written temporaries beside it.
    ao2_core::atomic_write(path, content)
        .with_context(|| format!("atomic write {}", path.display()))?;
    Ok(())
}

fn read_optional_text(path: &str) -> Result<String> {
    if path.is_empty() {
        return Ok(String::new());
    }
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("read {path}")),
    }
}

const WORKBENCH_LOG_TAIL_DEFAULT_BYTES: usize = 32 * 1024;
const WORKBENCH_LOG_TAIL_MAX_BYTES: usize = 256 * 1024;

fn workbench_log_tail_bytes(query: &str) -> usize {
    query_value_owned(query, "tail_bytes")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(WORKBENCH_LOG_TAIL_DEFAULT_BYTES)
        .min(WORKBENCH_LOG_TAIL_MAX_BYTES)
}

fn read_log_tail(path: &str, tail_bytes: usize) -> Result<serde_json::Value> {
    if path.is_empty() {
        return Ok(serde_json::json!({
            "text": "",
            "bytes": 0,
            "truncated": false
        }));
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error).with_context(|| format!("read {path}")),
    };
    let total_bytes = bytes.len();
    let truncated = total_bytes > tail_bytes;
    let start = if truncated {
        total_bytes.saturating_sub(tail_bytes)
    } else {
        0
    };
    let text = String::from_utf8_lossy(&bytes[start..]).to_string();
    Ok(serde_json::json!({
        "text": text,
        "bytes": total_bytes,
        "truncated": truncated
    }))
}

fn form_value<'a>(
    form: &'a std::collections::BTreeMap<String, String>,
    key: &str,
    default: &'a str,
) -> &'a str {
    form.get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
}

pub(crate) fn form_value_owned(
    form: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> Option<String> {
    form.get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn parse_optional_budget_form(
    form: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> Result<Option<f64>> {
    let Some(value) = form_value_owned(form, key) else {
        return Ok(None);
    };
    let budget = value
        .parse::<f64>()
        .with_context(|| format!("{key} must be a number"))?;
    if !budget.is_finite() || budget <= 0.0 {
        anyhow::bail!("{key} must be a positive finite number");
    }
    Ok(Some(budget))
}

pub(super) fn validate_minimum_provider_score(
    target: &Path,
    run_id: &str,
    minimum_score: u64,
) -> Result<serde_json::Value> {
    if minimum_score == 0 {
        return Ok(serde_json::Value::Null);
    }
    let scorecard =
        provider_score_json(target, run_id).map_err(|_| WorkbenchMinimumScoreError {
            run_id: run_id.to_string(),
            minimum_score,
            score: 0,
            verdict: "missing".to_string(),
        })?;
    let score = json_u64(&scorecard, "score");
    if score < minimum_score {
        return Err(WorkbenchMinimumScoreError {
            run_id: run_id.to_string(),
            minimum_score,
            score,
            verdict: json_string(&scorecard, "verdict"),
        }
        .into());
    }
    Ok(scorecard)
}

fn mark_workbench_job_running(state: &WorkbenchQueueState, job_id: &str) -> bool {
    let mut should_run = false;
    let result = update_workbench_job(state, job_id, |job| {
        if job.status == "queued" {
            let started_at_ms = now_unix_ms();
            job.status = "running".to_string();
            job.started_at_ms = started_at_ms;
            job.queue_wait_ms = started_at_ms.saturating_sub(job.queued_at_ms);
            should_run = true;
        }
    });
    result.is_ok() && should_run
}

fn update_workbench_job<F>(state: &WorkbenchQueueState, job_id: &str, update: F) -> Result<()>
where
    F: FnOnce(&mut WorkbenchJob),
{
    {
        let mut jobs = state.jobs.lock().expect("queue lock");
        if let Some(job) = jobs.iter_mut().find(|job| job.job_id == job_id) {
            update(job);
        }
    }
    persist_workbench_queue(state)
}

fn workbench_job_status(state: &WorkbenchQueueState, job_id: &str) -> Option<String> {
    state
        .jobs
        .lock()
        .expect("queue lock")
        .iter()
        .find(|job| job.job_id == job_id)
        .map(|job| job.status.clone())
}

fn execute_workbench_job(
    request: &WorkbenchJobRequest,
    state: &WorkbenchQueueState,
    active_children: &Arc<Mutex<HashMap<String, Child>>>,
) -> Result<WorkbenchJobExecutionResult> {
    let mut command = ProcessCommand::new(std::env::current_exe().context("resolve ao2 binary")?);
    match request.job_kind.as_str() {
        "repair_resume" => {
            let evidence_pack = request
                .repair_evidence_pack
                .as_ref()
                .context("repair resume job missing evidence_pack")?;
            command
                .arg("repair")
                .arg("resume")
                .arg("--evidence-pack")
                .arg(evidence_pack)
                .arg("--template")
                .arg(&request.template)
                .arg("--target")
                .arg(&request.target)
                .arg("--run-id")
                .arg(&request.run_id)
                .arg("--provider")
                .arg(&request.provider)
                .arg("--max-repair-attempts")
                .arg(request.max_repair_attempts.to_string());
            if let Some(prompt_file) = &request.provider_prompt_file {
                command.arg("--provider-prompt-file").arg(prompt_file);
            }
            if let Some(max_budget_usd) = request.max_budget_usd {
                command
                    .arg("--provider-max-budget-usd")
                    .arg(format_budget_usd(max_budget_usd)?);
            }
        }
        _ => {
            command
                .arg("run")
                .arg("--template")
                .arg(&request.template)
                .arg("--target")
                .arg(&request.target)
                .arg("--run-id")
                .arg(&request.run_id);
            if let Some(prompt_file) = &request.provider_prompt_file {
                command
                    .arg("--provider")
                    .arg(&request.provider)
                    .arg("--provider-prompt-file")
                    .arg(prompt_file)
                    .arg("--max-repair-attempts")
                    .arg(request.max_repair_attempts.to_string());
                if let Some(max_budget_usd) = request.max_budget_usd {
                    command
                        .arg("--provider-max-budget-usd")
                        .arg(format_budget_usd(max_budget_usd)?);
                }
            }
        }
    }
    command
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().context("spawn queued ao2 run")?;
    let (stdout_log, stderr_log) = workbench_job_log_paths(&request.target, &request.job_id);
    if let Some(parent) = stdout_log.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&stdout_log, b"").with_context(|| format!("write {}", stdout_log.display()))?;
    fs::write(&stderr_log, b"").with_context(|| format!("write {}", stderr_log.display()))?;
    let stdout = child.stdout.take().context("capture queued ao2 stdout")?;
    let stderr = child.stderr.take().context("capture queued ao2 stderr")?;
    let stdout_thread = spawn_workbench_log_writer(stdout, stdout_log.clone());
    let stderr_thread = spawn_workbench_log_writer(stderr, stderr_log.clone());
    active_children
        .lock()
        .expect("active child lock")
        .insert(request.job_id.clone(), child);
    if workbench_job_status(state, &request.job_id).as_deref() == Some("cancelled") {
        let child = active_children
            .lock()
            .expect("active child lock")
            .remove(&request.job_id);
        detach_cancelled_workbench_child(child, stdout_thread, stderr_thread);
        return Ok(WorkbenchJobExecutionResult {
            status: "cancelled".to_string(),
            evidence_pack: String::new(),
            cockpit: String::new(),
            error: "job was cancelled by operator".to_string(),
            exit_code: -1,
        });
    }
    loop {
        if workbench_job_status(state, &request.job_id).as_deref() == Some("cancelled") {
            let child = active_children
                .lock()
                .expect("active child lock")
                .remove(&request.job_id);
            detach_cancelled_workbench_child(child, stdout_thread, stderr_thread);
            return Ok(WorkbenchJobExecutionResult {
                status: "cancelled".to_string(),
                evidence_pack: String::new(),
                cockpit: String::new(),
                error: "job was cancelled by operator".to_string(),
                exit_code: -1,
            });
        }
        thread::sleep(Duration::from_millis(100));
        let status = {
            let mut children = active_children.lock().expect("active child lock");
            let Some(child) = children.get_mut(&request.job_id) else {
                return Ok(WorkbenchJobExecutionResult {
                    status: "cancelled".to_string(),
                    evidence_pack: String::new(),
                    cockpit: String::new(),
                    error: "job was cancelled by operator".to_string(),
                    exit_code: -1,
                });
            };
            child.try_wait().context("poll queued ao2 run")?
        };
        if status.is_some() {
            let mut child = active_children
                .lock()
                .expect("active child lock")
                .remove(&request.job_id)
                .context("remove completed queued ao2 run")?;
            let output_status = child.wait().context("wait queued ao2 run")?;
            join_workbench_log_writer(stdout_thread)?;
            join_workbench_log_writer(stderr_thread)?;
            if workbench_job_status(state, &request.job_id).as_deref() == Some("cancelled") {
                return Ok(WorkbenchJobExecutionResult {
                    status: "cancelled".to_string(),
                    evidence_pack: String::new(),
                    cockpit: String::new(),
                    error: "job was cancelled by operator".to_string(),
                    exit_code: output_status.code().unwrap_or(-1),
                });
            }
            if !output_status.success() {
                let stderr = read_optional_text(&stderr_log.display().to_string())?
                    .trim()
                    .to_string();
                let stdout = read_optional_text(&stdout_log.display().to_string())?
                    .trim()
                    .to_string();
                let detail = if !stderr.is_empty() {
                    stderr
                } else if !stdout.is_empty() {
                    stdout
                } else {
                    output_status.to_string()
                };
                return Ok(WorkbenchJobExecutionResult {
                    status: "failed".to_string(),
                    evidence_pack: String::new(),
                    cockpit: String::new(),
                    error: format!("queued ao2 run failed: {detail}"),
                    exit_code: output_status.code().unwrap_or(1),
                });
            }
            break;
        }
    }
    let _ = approve_and_resume_persisted_sandbox_patches(
        &request.target,
        &request.run_id,
        "human:workbench-operator",
    )?;
    let (html, evidence_pack) = render_report_for_run(&request.target, &request.run_id)?;
    let cockpit = run_dir(&request.target, &request.run_id)
        .join("cockpit")
        .join("index.html");
    if let Some(parent) = cockpit.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(&cockpit, html).with_context(|| format!("write {}", cockpit.display()))?;
    let run = run_summary_json(&request.target, &request.run_id)?;
    Ok(WorkbenchJobExecutionResult {
        status: json_string(&run, "status"),
        evidence_pack: evidence_pack.display().to_string(),
        cockpit: cockpit.display().to_string(),
        error: String::new(),
        exit_code: 0,
    })
}

pub(super) fn terminate_workbench_child(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = format!("-{}", child.id());
        let _ = ProcessCommand::new("kill")
            .arg("-TERM")
            .arg(&process_group)
            .status();
        thread::sleep(Duration::from_millis(50));
    }
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = ProcessCommand::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
}

fn spawn_workbench_log_writer<R>(mut reader: R, path: PathBuf) -> thread::JoinHandle<Result<()>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut file =
            fs::File::create(&path).with_context(|| format!("write {}", path.display()))?;
        std::io::copy(&mut reader, &mut file)
            .with_context(|| format!("stream {}", path.display()))?;
        Ok(())
    })
}

fn join_workbench_log_writer(handle: thread::JoinHandle<Result<()>>) -> Result<()> {
    handle
        .join()
        .map_err(|_| anyhow!("workbench log writer thread panicked"))?
}

fn detach_cancelled_workbench_child(
    child: Option<Child>,
    stdout_thread: thread::JoinHandle<Result<()>>,
    stderr_thread: thread::JoinHandle<Result<()>>,
) {
    thread::spawn(move || {
        if let Some(mut child) = child {
            terminate_workbench_child(&mut child);
            let _ = child.wait();
        }
        let _ = join_workbench_log_writer(stdout_thread);
        let _ = join_workbench_log_writer(stderr_thread);
    });
}

fn workbench_job_json(job: &WorkbenchJob) -> serde_json::Value {
    serde_json::json!({
        "job_id": job.job_id,
        "job_kind": job.job_kind,
        "run_id": job.run_id,
        "template": job.template,
        "provider": job.provider,
        "provider_prompt_file": job.provider_prompt_file,
        "repair_evidence_pack": job.repair_evidence_pack,
        "repair_source_run_id": job.repair_source_run_id,
        "max_repair_attempts": job.max_repair_attempts,
        "max_budget_usd": job.max_budget_usd,
        "retry_of": job.retry_of,
        "status": job.status,
        "evidence_pack": job.evidence_pack,
        "cockpit": job.cockpit,
        "stdout_log": job.stdout_log,
        "stderr_log": job.stderr_log,
        "queued_at_ms": job.queued_at_ms,
        "started_at_ms": job.started_at_ms,
        "finished_at_ms": job.finished_at_ms,
        "queue_wait_ms": job.queue_wait_ms,
        "duration_ms": job.duration_ms,
        "exit_code": job.exit_code,
        "retry_count": job.retry_count,
        "error": job.error,
        "diagnosis": workbench_job_diagnosis(job, "", "")
    })
}

fn workbench_job_diagnosis(job: &WorkbenchJob, stdout: &str, stderr: &str) -> serde_json::Value {
    let combined = format!("{}\n{}\n{}", job.error, stdout, stderr);
    let combined_lower = combined.to_ascii_lowercase();
    let timed_out = combined_lower.contains("timed_out: true")
        || combined_lower.contains("timeout")
        || combined_lower.contains("timed out");
    let failure_kind = if job.status == "cancelled" {
        "cancelled"
    } else if job.status == "interrupted" {
        "interrupted"
    } else if timed_out {
        "timeout"
    } else if job.exit_code != 0 && job.exit_code != default_workbench_exit_code() {
        "non_zero_exit"
    } else if !job.error.is_empty() {
        "error"
    } else {
        "none"
    };

    let mut recovery_actions = Vec::new();
    match failure_kind {
        "timeout" => {
            recovery_actions
                .push("Review stderr and stdout for the stalled provider step.".to_string());
            recovery_actions.push(
                "Reduce prompt scope or rerun after confirming the local provider CLI is responsive."
                    .to_string(),
            );
        }
        "non_zero_exit" => {
            recovery_actions.push(
                "Review stderr first, then stdout and provider transcript artifacts if they exist."
                    .to_string(),
            );
            recovery_actions.push(
                "Retry after fixing the prompt file, verifier failure, or local provider auth."
                    .to_string(),
            );
        }
        "cancelled" => {
            recovery_actions
                .push("Retry the job when the operator is ready to run it again.".to_string());
        }
        "interrupted" => {
            recovery_actions
                .push("Retry the job because the Workbench server stopped mid-run.".to_string());
        }
        "error" => {
            recovery_actions.push(
                "Review the job error and logs, then retry after correcting the cause.".to_string(),
            );
        }
        _ => {}
    }
    if combined_lower.contains("auth")
        || combined_lower.contains("oauth")
        || combined_lower.contains("login")
        || combined_lower.contains("credential")
    {
        recovery_actions.push(
            "Refresh local provider auth with the provider CLI OAuth login before retrying."
                .to_string(),
        );
    }
    if combined_lower.contains("no such file")
        || combined_lower.contains("missing")
        || combined_lower.contains("not found")
    {
        recovery_actions.push(
            "Confirm the provider prompt file path exists and is readable from this machine."
                .to_string(),
        );
    }

    let stderr_excerpt_source = if stderr.trim().is_empty() {
        job.error.as_str()
    } else {
        stderr
    };

    serde_json::json!({
        "schema_version": "ao2.workbench-job-diagnosis.v1",
        "run_id": job.run_id,
        "job_id": job.job_id,
        "provider": job.provider,
        "status": job.status,
        "failure_kind": failure_kind,
        "exit_code": job.exit_code,
        "timed_out": timed_out,
        "primary_error": job.error,
        "stdout_excerpt": workbench_log_excerpt(stdout),
        "stderr_excerpt": workbench_log_excerpt(stderr_excerpt_source),
        "recovery_actions": recovery_actions
    })
}

pub(super) fn workbench_log_excerpt(input: &str) -> String {
    let trimmed = input.trim();
    const LIMIT: usize = 2_000;
    if trimmed.len() <= LIMIT {
        trimmed.to_string()
    } else {
        let mut start = trimmed.len().saturating_sub(LIMIT);
        // `len - LIMIT` may land inside a multibyte char; slicing there panics.
        // Walk back to the nearest char boundary so the suffix stays whole. This
        // keeps the byte budget (at most a few extra bytes) rather than counting
        // characters, which a `chars().rev().take()` approach would blow past.
        while start > 0 && !trimmed.is_char_boundary(start) {
            start -= 1;
        }
        format!("...{}", &trimmed[start..])
    }
}
