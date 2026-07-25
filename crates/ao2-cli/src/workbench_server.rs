use super::*;
use crate::control_plane_http::parse_http_endpoint;
use crate::workbench_app::{
    build_workbench_operators, parse_form_body, templates_json,
    workbench_evidence_control_plane_dashboard_json, workbench_launch_json,
    workbench_provider_pilot_json, workbench_provider_pilot_preflight_json,
    WorkbenchMinimumScoreError,
};
use crate::workbench_contract::{
    WorkbenchOperator, WorkbenchOperatorRole, WorkbenchSupportSigning,
};
use crate::workbench_obligation::{
    workbench_obligation_annotation_json, workbench_obligation_gate_json,
};
use crate::workbench_provider_pilot::{
    workbench_export_latest_provider_pilot_acceptance_json,
    workbench_latest_provider_pilot_acceptance_json, workbench_provider_pilot_cost_ledger_json,
    workbench_provider_pilot_cost_trend_json,
};
use crate::workbench_queue::{start_workbench_queue, WorkbenchQueue};
use crate::workbench_render::{
    latest_workbench_support_packet_json, render_workbench, workbench_provider_contracts_json,
    WorkbenchRenderOptions,
};

pub(super) struct ServeWorkbenchOptions {
    pub(super) target: PathBuf,
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) once: bool,
    pub(super) provenance_dir: PathBuf,
    pub(super) api_token: Option<String>,
    pub(super) operator_tokens: Vec<String>,
    pub(super) enable_execution: bool,
    pub(super) queue_retention: usize,
    pub(super) control_plane_url: Option<String>,
    pub(super) support_signing_key: Option<PathBuf>,
    pub(super) support_signer_id: String,
}

struct WorkbenchAppOptions {
    host: String,
    port: u16,
    once: bool,
    target: PathBuf,
    provenance_dir: PathBuf,
    operators: Vec<WorkbenchOperator>,
    support_signing: Option<WorkbenchSupportSigning>,
    queue: Option<WorkbenchQueue>,
    control_plane_url: Option<String>,
}

pub(super) fn serve_workbench(options: ServeWorkbenchOptions) -> Result<()> {
    let ServeWorkbenchOptions {
        target,
        host,
        port,
        once,
        provenance_dir,
        api_token,
        operator_tokens,
        enable_execution,
        queue_retention,
        control_plane_url,
        support_signing_key,
        support_signer_id,
    } = options;
    let api_token = api_token.unwrap_or_else(generate_api_token);
    let control_plane_url = normalize_workbench_control_plane_url(control_plane_url)?;
    let operators = build_workbench_operators(api_token.clone(), operator_tokens)?;
    let support_signing = support_signing_key.map(|key_path| WorkbenchSupportSigning {
        key_path,
        signer_id: support_signer_id,
    });
    let queue = if enable_execution {
        Some(start_workbench_queue(
            &target,
            queue_retention,
            support_signing.clone(),
        )?)
    } else {
        None
    };
    serve_workbench_app(WorkbenchAppOptions {
        host,
        port,
        once,
        target,
        provenance_dir,
        operators,
        support_signing,
        queue,
        control_plane_url,
    })
}

fn normalize_workbench_control_plane_url(value: Option<String>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    parse_http_endpoint(trimmed)
        .with_context(|| format!("validate --control-plane-url {trimmed}"))?;
    Ok(Some(trimmed.trim_end_matches('/').to_string()))
}

pub(super) fn serve_cockpit(
    target: PathBuf,
    run_id: Option<String>,
    host: String,
    port: u16,
    index: bool,
    once: bool,
) -> Result<()> {
    let html = if index {
        render_cockpit_index(&target)?
    } else {
        let run_id = run_id.context("run_id is required unless --index is used")?;
        render_report_for_run(&target, &run_id)?.0
    };
    serve_html(host, port, once, html)
}

fn serve_html(host: String, port: u16, once: bool, html: String) -> Result<()> {
    let listener = TcpListener::bind((host.as_str(), port))
        .with_context(|| format!("bind local server on {host}:{port}"))?;
    let address = listener.local_addr().context("read local server address")?;
    println!("url=http://{}:{}/", address.ip(), address.port());
    std::io::stdout()
        .flush()
        .context("flush local server url")?;

    for stream in listener.incoming() {
        let mut stream = stream.context("accept local server connection")?;
        let mut request_buffer = [0_u8; 1024];
        let _ = stream.read(&mut request_buffer);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(),
            html
        );
        stream
            .write_all(response.as_bytes())
            .context("write local server response")?;
        if once {
            break;
        }
    }
    Ok(())
}

fn serve_workbench_app(options: WorkbenchAppOptions) -> Result<()> {
    let WorkbenchAppOptions {
        host,
        port,
        once,
        target,
        provenance_dir,
        operators,
        support_signing,
        queue,
        control_plane_url,
    } = options;
    let listener = TcpListener::bind((host.as_str(), port))
        .with_context(|| format!("bind workbench server on {host}:{port}"))?;
    let address = listener
        .local_addr()
        .context("read workbench server address")?;
    println!("url=http://{}:{}/", address.ip(), address.port());
    if let Some(admin) = operators.first() {
        eprintln!("api_token_redacted=true");
        eprintln!("admin_operator_id={}", admin.id);
    }
    eprintln!("operators={}", operators.len());
    std::io::stdout()
        .flush()
        .context("flush workbench server url")?;

    for stream in listener.incoming() {
        let mut stream = stream.context("accept workbench connection")?;
        let mut request_buffer = [0_u8; 8192];
        let bytes_read = stream.read(&mut request_buffer).unwrap_or(0);
        let request = String::from_utf8_lossy(&request_buffer[..bytes_read]).to_string();
        let response = handle_workbench_request(
            &request,
            &target,
            &provenance_dir,
            &operators,
            support_signing.as_ref(),
            queue.as_ref(),
            control_plane_url.as_deref(),
        )?;
        stream
            .write_all(response.as_bytes())
            .context("write workbench response")?;
        if once {
            break;
        }
    }
    Ok(())
}

fn handle_workbench_request(
    request: &str,
    target: &Path,
    provenance_dir: &Path,
    operators: &[WorkbenchOperator],
    support_signing: Option<&WorkbenchSupportSigning>,
    queue: Option<&WorkbenchQueue>,
    control_plane_url: Option<&str>,
) -> Result<String> {
    let Some(request_line) = request.lines().next() else {
        return Ok(http_text_response(400, "Bad Request", "empty request"));
    };
    let (method, raw_path) = parse_http_request_line(request_line);
    let (path, query) = split_path_query(raw_path);

    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            let Some(operator) = workbench_operator_for_html(query, operators) else {
                return Ok(http_text_response(403, "Forbidden", "invalid api token"));
            };
            let html = render_workbench(
                target,
                provenance_dir,
                WorkbenchRenderOptions {
                    operator: Some(operator),
                    execution_enabled: queue.is_some(),
                    can_operate: operator.role.can(WorkbenchOperatorRole::Operator),
                    release_comparison_signing_enabled: support_signing.is_some(),
                    control_plane_url,
                    release_gate_artifact_path: query_value_owned(query, "release_gate_artifact")
                        .as_deref(),
                },
            )?;
            Ok(http_html_response(html))
        }
        ("GET", "/api/runs") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            http_json_response(200, runs_list_json(target)?)
        }
        ("GET", "/api/factory/project-start/next-action") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_next_action_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/completion-summary") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_completion_summary_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/completion-summary/memory-checkpoint") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_completion_summary_memory_status_json(
                target, query,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/latest") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_latest_recovery_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/action") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_action_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-receipt") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_receipt_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/factory/project-start/recovery/resume-checkpoint") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_factory_project_start_recovery_resume_checkpoint_json(target, &form) {
                Ok(json) if json_string(&json, "status") == "recorded" => {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-checkpoint/status") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_checkpoint_status_json(
                target, query,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-continuity") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_continuity_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-plan") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_plan_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/factory/project-start/recovery/resume-claim") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_factory_project_start_recovery_resume_claim_json(target, &form) {
                Ok(json) if json_string(&json, "status") == "claimed" => {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-claim/status") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_claim_status_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-continuation-contract") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_continuation_contract_json(
                target, query,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/factory/project-start/recovery/resume-continue") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_factory_project_start_recovery_resume_continue_json(target, &form) {
                Ok(json) if json_string(&json, "status") == "continued" => {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-continuation/status") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_continuation_status_json(
                target, query,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-post-continuation/action") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_action_json(
                target, query,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/factory/project-start/recovery/resume-post-continuation/execute") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_factory_project_start_recovery_resume_post_continuation_execute_json(
                target, &form,
            ) {
                Ok(json) if json_string(&json, "status") == "executed" => {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        (
            "GET",
            "/api/factory/project-start/recovery/resume-post-continuation/execution-status",
        ) => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_execution_status_json(
                target, query,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-post-continuation/next-action") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_next_action_json(
                target, query,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-post-continuation/closure") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_closure_json(
                target, query,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        (
            "GET",
            "/api/factory/project-start/recovery/resume-post-continuation/evaluator-decision",
        ) => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_evaluator_decision_json(
                target,
                query,
                support_signing,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/recovery/resume-post-continuation/release-handoff") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_json(
                target, query,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        (
            "GET",
            "/api/factory/project-start/recovery/resume-post-continuation/release-handoff-status",
        ) => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_status_json(
                target, query,
            ) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        (
            "GET",
            "/api/factory/project-start/recovery/resume-post-continuation/release-handoff-status-summary",
        ) => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_status_summary_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        (
            "GET",
            "/api/factory/project-start/recovery/resume-post-continuation/release-handoff-status-summary-export",
        ) => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_release_handoff_status_summary_export_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        (
            "GET",
            "/api/factory/project-start/recovery/resume-post-continuation/release-publication-readiness",
        ) => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_release_publication_readiness_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        (
            "GET",
            "/api/factory/project-start/recovery/resume-post-continuation/release-publication-dispatch-plan",
        ) => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_release_publication_dispatch_plan_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        (
            "GET",
            "/api/factory/project-start/recovery/resume-post-continuation/release-publication-readback",
        ) => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_release_publication_readback_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        (
            "GET",
            "/api/factory/project-start/recovery/resume-post-continuation/release-publication-closure",
        ) => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_recovery_resume_post_continuation_release_publication_closure_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/replacement-parity-status") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_replacement_parity_status_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/factory/compat-plan") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_factory_compat_plan_json(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/factory/project-start/completion-summary/memory-checkpoint") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_factory_project_start_completion_summary_memory_json(target, &form) {
                Ok(json) if json_string(&json, "status") == "recorded" => {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/project-start/hermes-flow-contract") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_project_start_hermes_flow_contract_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/factory/greenfield-spec-ingest") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_factory_greenfield_spec_ingest_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/factory/greenfield-spec-ingest/submit") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_factory_greenfield_spec_ingest_submit_json(target, &form) {
                Ok(json) if json_string(&json, "status") == "queued" => {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/factory/project-start/run-next") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_factory_project_start_run_next_json(target, &form) {
                Ok(json)
                    if json_string(&json, "approval_status") == "approved_exact_action_digest" =>
                {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/factory/project-start/operator-record") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_factory_project_start_operator_record_json(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/memory/search") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_memory_search_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/memory/recent") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_memory_recent_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/memory/export") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_memory_export_json(target, &form, support_signing) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/memory/publish-latest") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_memory_publish_latest_json(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/memory/control-plane-dashboard") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_memory_control_plane_dashboard_json(&form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/memory/link-run") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_memory_link_run_json(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/runs/evidence") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_run_evidence_summary_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/runs/evidence/diff") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_run_evidence_diff_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/runs/evidence/changes") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_run_evidence_changes_json(target, query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/support/latest") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match latest_workbench_support_packet_json(target) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/runs/evidence/export") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_evidence_export_json(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/runs/evidence/publish") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_evidence_publish_json(target, &form, support_signing) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/runs/evidence/detail") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_evidence_control_plane_detail_json(&form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/runs/evidence/dashboard") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_evidence_control_plane_dashboard_json(&form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/obligations/annotate") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let Some(operator) = workbench_operator_from_query(query, operators) else {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "invalid_api_token"
                    }),
                );
            };
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_obligation_annotation_json(target, &form, operator, support_signing) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/obligations/gate") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let Some(operator) = workbench_operator_from_query(query, operators) else {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "invalid_api_token"
                    }),
                );
            };
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_obligation_gate_json(target, &form, operator, support_signing) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/templates") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            http_json_response(200, templates_json())
        }
        ("GET", "/api/doctor") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            http_json_response(
                200,
                doctor_report_json(
                    None,
                    provenance_dir.to_path_buf(),
                    None,
                    None,
                    "uesugitorachiyo/ao2".to_string(),
                )?,
            )
        }
        ("GET", "/api/release-health") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_release_health_json(query, provenance_dir) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/release-history") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_release_history_json(query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/release-gate/artifact") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_release_gate_artifact_json(query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/release-comparison") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let Some(support_signing) = support_signing else {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "release comparison signing requires --support-signing-key"
                    }),
                );
            };
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_release_comparison_json(&form, support_signing) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/release-summary/enrich") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_release_summary_enrich_json(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/release-gate") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_release_gate_json(&form) {
                Ok(json) if json_string(&json, "status") == "verified" => {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/release-comparison/verify") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_release_comparison_verification_json(query) {
                Ok(json) if json_string(&json["verification"], "status") == "verified" => {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/release-comparison/latest") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_latest_release_comparison_json(query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/release-retention/prune") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_release_retention_prune_json(&form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/provider-matrix") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            http_json_response(200, provider_matrix_json()?)
        }
        ("GET", "/api/provider-contracts") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            http_json_response(200, workbench_provider_contracts_json())
        }
        ("GET", "/api/provider-pilot/acceptance/latest") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_latest_provider_pilot_acceptance_json(query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/provider-pilot/cost-ledger") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_provider_pilot_cost_ledger_json(query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/provider-pilot/cost-trend") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            match workbench_provider_pilot_cost_trend_json(query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/provider-pilot/acceptance/export-latest") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_export_latest_provider_pilot_acceptance_json(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/provider-smoke") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            if queue.is_none() {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "execution_disabled"
                    }),
                );
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            let minimum_score = form
                .get("minimum_score")
                .and_then(|value| value.trim().parse::<u64>().ok())
                .unwrap_or(90);
            let live_providers = form_value_owned(&form, "live_provider")
                .map(|provider| vec![provider])
                .unwrap_or_default();
            match provider_smoke_all_json(target, minimum_score, &live_providers) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/provider-pilot") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_provider_pilot_json(target, &form) {
                Ok(json) if json_string(&json, "status") == "ready" => {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/provider-pilot/preflight") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_provider_pilot_preflight_json(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/provider-pilot/start") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "execution_disabled"
                    }),
                );
            };
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match queue.enqueue_provider_pilot(target, &form) {
                Ok(json) if json_string(&json, "status") == "queued" => {
                    http_json_response(200, json)
                }
                Ok(json) => http_json_response(400, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/repair/resume/start") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "execution_disabled"
                    }),
                );
            };
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match queue.enqueue_repair_resume(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/queue") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(200, disabled_queue_json());
            };
            http_json_response(200, queue.to_json(query))
        }
        ("GET", "/api/queue/job") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(200, disabled_queue_json());
            };
            match queue.job_detail(query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/queue/job/logs") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(200, disabled_queue_json());
            };
            match queue.job_logs(query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/api/queue/audit") => {
            if let Some(response) =
                workbench_api_authorization_failure(query, operators, WorkbenchOperatorRole::Viewer)
            {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(200, disabled_queue_json());
            };
            match queue.audit_json(query) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("GET", "/queue/job") => {
            let Some(operator) = workbench_operator_from_query(query, operators) else {
                return Ok(http_text_response(403, "Forbidden", "invalid api token"));
            };
            if !operator.role.can(WorkbenchOperatorRole::Viewer) {
                return Ok(http_text_response(
                    403,
                    "Forbidden",
                    "insufficient operator role",
                ));
            }
            let Some(queue) = queue else {
                return Ok(http_text_response(403, "Forbidden", "execution disabled"));
            };
            match queue.job_detail_page(query) {
                Ok(html) => Ok(http_html_response(html)),
                Err(error) => Ok(http_text_response(400, "Bad Request", &error.to_string())),
            }
        }
        ("POST", "/api/launch") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match workbench_launch_json(target, &form, support_signing) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/queue/start") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "execution_disabled"
                    }),
                );
            };
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match queue.enqueue(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/queue/cancel") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "execution_disabled"
                    }),
                );
            };
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match queue.cancel(&form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/queue/export") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "execution_disabled"
                    }),
                );
            };
            match queue.export_support_bundle(target) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/queue/export-preview") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "execution_disabled"
                    }),
                );
            };
            match queue.preview_support_bundle(target) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        ("POST", "/api/queue/retry") => {
            if let Some(response) = workbench_api_authorization_failure(
                query,
                operators,
                WorkbenchOperatorRole::Operator,
            ) {
                return response;
            }
            let Some(queue) = queue else {
                return http_json_response(
                    403,
                    serde_json::json!({
                        "schema_version": "ao2.workbench-error.v1",
                        "error": "execution_disabled"
                    }),
                );
            };
            let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
            let form = parse_form_body(body);
            match queue.retry(target, &form) {
                Ok(json) => http_json_response(200, json),
                Err(error) => http_json_response(400, workbench_error_json(&error)),
            }
        }
        _ => Ok(http_text_response(404, "Not Found", "not found")),
    }
}

pub(super) fn parse_http_request_line(request_line: &str) -> (&str, &str) {
    let line = request_line.trim_end_matches('\r');
    let Some((method, rest)) = line.split_once(' ') else {
        return ("", "/");
    };
    let rest = rest.trim_start();
    let raw_path = match rest.rfind(" HTTP/") {
        Some(version_start) => &rest[..version_start],
        None => rest.split_whitespace().next().unwrap_or("/"),
    }
    .trim_end();
    (method, if raw_path.is_empty() { "/" } else { raw_path })
}

pub(super) fn split_path_query(raw_path: &str) -> (&str, &str) {
    raw_path
        .split_once('?')
        .map_or((raw_path, ""), |(path, query)| (path, query))
}

fn workbench_operator_for_html<'a>(
    query: &str,
    operators: &'a [WorkbenchOperator],
) -> Option<&'a WorkbenchOperator> {
    if let Some(operator) = workbench_operator_from_query(query, operators) {
        return Some(operator);
    }
    if query_value_owned(query, "token").is_none() && operators.len() == 1 {
        return operators.first();
    }
    None
}

fn workbench_operator_from_query<'a>(
    query: &str,
    operators: &'a [WorkbenchOperator],
) -> Option<&'a WorkbenchOperator> {
    let token = query_value_owned(query, "token")?;
    operators.iter().find(|operator| operator.token == token)
}

fn workbench_api_authorization_failure(
    query: &str,
    operators: &[WorkbenchOperator],
    required_role: WorkbenchOperatorRole,
) -> Option<Result<String>> {
    let Some(operator) = workbench_operator_from_query(query, operators) else {
        return Some(http_json_response(
            403,
            serde_json::json!({
                "schema_version": "ao2.workbench-error.v1",
                "error": "invalid_api_token"
            }),
        ));
    };
    if operator.role.can(required_role) {
        return None;
    }
    Some(http_json_response(
        403,
        serde_json::json!({
            "schema_version": "ao2.workbench-error.v1",
            "error": "insufficient_operator_role",
            "operator_id": operator.id,
            "operator_role": operator.role.as_str(),
            "required_role": required_role.as_str()
        }),
    ))
}

fn workbench_error_json(error: &anyhow::Error) -> serde_json::Value {
    if let Some(score_error) = error.downcast_ref::<WorkbenchMinimumScoreError>() {
        return serde_json::json!({
            "schema_version": "ao2.workbench-error.v1",
            "error": "minimum_provider_score_not_met",
            "run_id": score_error.run_id,
            "minimum_score": score_error.minimum_score,
            "score": score_error.score,
            "verdict": score_error.verdict
        });
    }
    serde_json::json!({
        "schema_version": "ao2.workbench-error.v1",
        "error": error.to_string()
    })
}
