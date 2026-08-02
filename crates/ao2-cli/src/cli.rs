use std::path::PathBuf;

use ao2_adapters::DEFAULT_PROVIDER_TIMEOUT_SECONDS;
use clap::{Parser, Subcommand};

use crate::evidence_publish::EvidenceCommand;
use crate::install_cmd::InstallCommand;
use crate::memory_store::MemoryCommand;
use crate::skill_contract_manifest::SkillContractManifestCommand;
use crate::upgrade_cmd::UpgradeCommand;
use crate::{
    github_issue_draft, github_issue_intake::github_issue_publish, sdd_cmd, support_bundle,
};

#[derive(Debug, Parser)]
#[command(name = "ao2")]
#[command(about = "AO2 local governed software-delivery runner")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Init {
        #[arg(long, default_value = ".")]
        target: PathBuf,
    },
    Run {
        workflow: Option<PathBuf>,
        #[arg(long)]
        spec: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        target: Option<PathBuf>,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        pause_for_approval: bool,
        #[arg(long)]
        resume: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long)]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
    },
    Repair {
        #[command(subcommand)]
        command: RepairCommand,
    },
    Support {
        #[command(subcommand)]
        command: support_bundle::SupportCommand,
    },
    Status {
        run_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
    },
    Approve {
        ticket_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long, default_value = "human:local-user")]
        approver: String,
    },
    Adapter {
        #[command(subcommand)]
        command: AdapterCommand,
    },
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    Plugin {
        #[command(subcommand)]
        command: Box<PluginCommand>,
    },
    SkillContractManifest {
        #[command(subcommand)]
        command: SkillContractManifestCommand,
    },
    Template {
        #[command(subcommand)]
        command: TemplateCommand,
    },
    Replay {
        run_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
    },
    Report {
        #[command(subcommand)]
        command: Option<ReportCommand>,
        run_id: Option<String>,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
    },
    Runs {
        #[command(subcommand)]
        command: RunsCommand,
    },
    Cockpit {
        #[command(subcommand)]
        command: CockpitCommand,
    },
    Pulse {
        #[command(subcommand)]
        command: PulseCommand,
    },
    Workbench {
        #[command(subcommand)]
        command: WorkbenchCommand,
    },
    ControlPlane {
        #[command(subcommand)]
        command: ControlPlaneCommand,
    },
    Contract {
        #[command(subcommand)]
        command: ContractCommand,
    },
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommand,
    },
    Git {
        #[command(subcommand)]
        command: GitCommand,
    },
    Factory {
        #[command(subcommand)]
        command: FactoryCommand,
    },
    Greenfield {
        #[command(subcommand)]
        command: GreenfieldCommand,
    },
    Sdd {
        #[command(subcommand)]
        command: sdd_cmd::SddCommand,
    },
    Issue {
        #[command(subcommand)]
        command: IssueCommand,
    },
    Export {
        run_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
    },
    Version {
        #[arg(long)]
        json: bool,
    },
    Doctor {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        install_dir: Option<PathBuf>,
        #[arg(long, default_value = "dist-provenance")]
        provenance_dir: PathBuf,
        #[arg(long)]
        release: Option<String>,
        #[arg(long)]
        release_asset_dir: Option<PathBuf>,
        #[arg(long, default_value = "uesugitorachiyo/ao2")]
        release_repo: String,
    },
    Upgrade {
        #[command(subcommand)]
        command: UpgradeCommand,
    },
    Install {
        #[command(subcommand)]
        command: InstallCommand,
    },
    Release {
        #[command(subcommand)]
        command: ReleaseCommand,
    },
    Cp {
        #[command(subcommand)]
        command: CpCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum CpCommand {
    ProbeExtended {
        #[arg(long, default_value = "http://127.0.0.1:18745")]
        cp_url: String,
        #[arg(long)]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long = "write-json")]
        write_json: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Fetch the release-evidence endpoints from a control-plane and
    /// emit a canonical `ao2.cp-release-snapshot.v1` bundle (per-endpoint
    /// status, schema, body length, body SHA256). Read-only; never
    /// mutates AO artifacts.
    ReleaseSnapshot {
        #[arg(long, default_value = "http://127.0.0.1:18745")]
        cp_url: String,
        #[arg(long)]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long = "write-json")]
        write_json: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ReportCommand {
    Verify {
        #[arg(long)]
        run_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long)]
        index: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum RepairCommand {
    Resume {
        #[arg(long)]
        evidence_pack: PathBuf,
        #[arg(long)]
        workflow: Option<PathBuf>,
        #[arg(long)]
        template: Option<String>,
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long)]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum RunsCommand {
    List {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Show {
        run_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum CockpitCommand {
    Index {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
    },
    Serve {
        run_id: Option<String>,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8732)]
        port: u16,
        #[arg(long)]
        index: bool,
        #[arg(long)]
        once: bool,
    },
}

fn parse_bool(s: &str) -> std::result::Result<bool, String> {
    match s.to_lowercase().trim() {
        "true" | "1" | "yes" | "y" | "" => Ok(true),
        "false" | "0" | "no" | "n" => Ok(false),
        other => Err(format!("invalid boolean value: '{}'", other)),
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum PulseCommand {
    Run {
        #[arg(long)]
        once: bool,
        #[arg(long)]
        chain: bool,
        #[arg(long)]
        execute: bool,
        #[arg(long = "once-evidence")]
        once_evidence: Option<PathBuf>,
        #[arg(long = "chain-evidence")]
        chain_evidence: Option<PathBuf>,
        #[arg(long = "task-contract")]
        task_contract: Option<PathBuf>,
        #[arg(long = "dry-run-task")]
        dry_run_task: bool,
        #[arg(long = "apply-dry-run")]
        apply_dry_run: bool,
        #[arg(long = "dry-run-evidence")]
        dry_run_evidence: Option<PathBuf>,
        #[arg(long = "dry-run-sha256")]
        dry_run_sha256: Option<String>,
        #[arg(long = "apply-root", default_value = ".")]
        apply_root: PathBuf,
        #[arg(
            long,
            default_value = "../factory-v3/docs/status/hermes-governed-backend-control-plane/prompt.txt"
        )]
        packet: PathBuf,
        #[arg(
            long,
            default_value = "../factory-v3/docs/status/multi-agent-coordination/BOARD.md"
        )]
        board: PathBuf,
        #[arg(long, default_value = "target/ao2-pulse-once")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    EvalLoop {
        #[command(subcommand)]
        command: PulseEvalLoopCommand,
    },
    RunLoop {
        #[arg(long)]
        command: String,
        #[arg(long = "decision-file")]
        decision_file: Option<PathBuf>,
        #[arg(long = "max-chain-runs", default_value_t = 3)]
        max_chain_runs: u32,
        #[arg(long = "max-runtime-seconds", default_value_t = 2700)]
        max_runtime_seconds: u64,
        #[arg(long = "out-dir", default_value = "target/ao2-pulse-run-loop")]
        out_dir: PathBuf,
        #[arg(long = "stdout-fallback")]
        stdout_fallback: bool,
        #[arg(long = "apply-root", default_value = ".")]
        apply_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    AutoAdvance {
        #[arg(
            long,
            env = "AO2_PULSE_RESUME_JSON",
            default_value = ".ao2-local/pulse/latest/resume.json"
        )]
        resume_json: PathBuf,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_ROOT",
            default_value = "target/pulse-auto-advance/latest"
        )]
        out_dir: PathBuf,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_LEDGER",
            default_value = ".ao2-local/pulse/pulse-auto-advance-ledger.jsonl"
        )]
        ledger: PathBuf,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_STOP_FILE",
            default_value = ".ao2-local/pulse/STOP"
        )]
        stop_file: PathBuf,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_MAX_ITERATIONS")]
        max_iterations: Option<u32>,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_ALLOW_DUPLICATE", action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = parse_bool, default_value = "false")]
        allow_duplicate: bool,
        #[arg(long)]
        forever: bool,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_SLEEP_SECONDS",
            default_value = "30"
        )]
        sleep_seconds: u64,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT",
            default_value = "1"
        )]
        generate_next: u32,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT_SLEEP_SECONDS")]
        generate_next_sleep_seconds: Option<u64>,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE", default_value = "1")]
        pr_ci_gate: u32,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_STATE",
            default_value = ".ao2-local/pulse/pr-ci-gate.json"
        )]
        pr_ci_gate_state: PathBuf,
        #[arg(
            long,
            env = "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_UPDATE",
            default_value = "1"
        )]
        pr_ci_gate_update: u32,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED", action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = parse_bool, default_value = "false")]
        local_only_while_pr_blocked: bool,
        #[arg(long, env = "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH", action = clap::ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = parse_bool, default_value = "false")]
        direct_main_publish: bool,
        #[arg(long, default_value = ".")]
        apply_root: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum PulseEvalLoopCommand {
    Run {
        #[arg(long)]
        once: bool,
        #[arg(long)]
        chain: bool,
        #[arg(long = "executor-evidence")]
        executor_evidence: Option<PathBuf>,
        #[arg(long = "executor-sha256")]
        executor_sha256: Option<String>,
        #[arg(long = "eval-loop-evidence")]
        eval_loop_evidence: Option<PathBuf>,
        #[arg(long = "eval-loop-sha256")]
        eval_loop_sha256: Option<String>,
        #[arg(long = "verification-command")]
        verification_command: String,
        #[arg(long = "verification-status")]
        verification_status: String,
        #[arg(long)]
        packet: PathBuf,
        #[arg(long)]
        board: PathBuf,
        #[arg(long, default_value = "target/ao2-pulse-eval-loop")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Handoff {
        #[arg(long = "eval-loop-evidence")]
        eval_loop_evidence: PathBuf,
        #[arg(long = "eval-loop-sha256")]
        eval_loop_sha256: String,
        #[arg(
            long,
            default_value = "../factory-v3/docs/status/hermes-governed-backend-control-plane/prompt.txt"
        )]
        packet: PathBuf,
        #[arg(
            long,
            default_value = "../factory-v3/docs/status/multi-agent-coordination/BOARD.md"
        )]
        board: PathBuf,
        #[arg(long, default_value = "target/ao2-pulse-eval-loop-handoff")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkbenchCommand {
    Export {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
        #[arg(long, default_value = "dist-provenance")]
        provenance_dir: PathBuf,
    },
    Serve {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8732)]
        port: u16,
        #[arg(long)]
        once: bool,
        #[arg(long, default_value = "dist-provenance")]
        provenance_dir: PathBuf,
        #[arg(long)]
        api_token: Option<String>,
        #[arg(long = "operator-token")]
        operator_tokens: Vec<String>,
        #[arg(long)]
        enable_execution: bool,
        #[arg(long, default_value_t = 100)]
        queue_retention: usize,
        #[arg(long = "control-plane-url")]
        control_plane_url: Option<String>,
        #[arg(long)]
        support_signing_key: Option<PathBuf>,
        #[arg(long, default_value = "ao2-workbench")]
        support_signer_id: String,
    },
    SupportVerify {
        #[arg(long)]
        bundle_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    SupportImport {
        #[arg(long)]
        bundle_dir: PathBuf,
        #[arg(long, default_value = "workbench-support-cases")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    SupportInspect {
        #[arg(long)]
        bundle_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    SupportKeygen {
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 2048)]
        bits: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ControlPlaneCommand {
    Ingest {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Export {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        snapshot: Option<PathBuf>,
        #[arg(long)]
        fleet: Option<PathBuf>,
        #[arg(long)]
        health_history: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
    },
    Serve {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        snapshot: Option<PathBuf>,
        #[arg(long)]
        fleet: Option<PathBuf>,
        #[arg(long)]
        health_history: Option<PathBuf>,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8733)]
        port: u16,
        #[arg(long)]
        once: bool,
        #[arg(long)]
        api_token: Option<String>,
    },
    Index {
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        #[arg(long = "snapshot")]
        snapshots: Vec<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Refresh {
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        #[arg(long)]
        sources: Option<PathBuf>,
        #[arg(long)]
        history: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Health {
        #[arg(long)]
        fleet: PathBuf,
        #[arg(long)]
        history: Option<PathBuf>,
        #[arg(long)]
        record: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    HealthTrend {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        json: bool,
    },
    HealthExport {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
        #[arg(long)]
        json: bool,
    },
    HealthPrune {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        keep: usize,
        #[arg(long)]
        json: bool,
    },
    Sources {
        #[command(subcommand)]
        command: ControlPlaneSourcesCommand,
    },
    History {
        #[command(subcommand)]
        command: ControlPlaneHistoryCommand,
    },
    Bundle {
        #[arg(long)]
        fleet: PathBuf,
        #[arg(long)]
        health_history: Option<PathBuf>,
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long)]
        signing_key: Option<PathBuf>,
        #[arg(long, default_value = "local-operator")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    BundleVerify {
        #[arg(long)]
        bundle_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    BundleImport {
        #[arg(long)]
        archive: Option<PathBuf>,
        #[arg(long)]
        bundle_dir: Option<PathBuf>,
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    BundleInspect {
        #[arg(long)]
        archive: Option<PathBuf>,
        #[arg(long)]
        bundle_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ControlPlaneSourcesCommand {
    Save {
        #[arg(long = "target")]
        targets: Vec<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ControlPlaneHistoryCommand {
    Diff {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        from_index: Option<usize>,
        #[arg(long)]
        to_index: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    Prune {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        keep: usize,
        #[arg(long)]
        json: bool,
    },
    Export {
        #[arg(long)]
        history: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        open: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ContractCommand {
    Extract {
        #[arg(long)]
        spec: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Check {
        #[arg(long)]
        ledger: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Gate {
        #[arg(long)]
        ledger: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        stage: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
        /// When supplied, also emit an `ao2.workbench-evidence-export.v1`
        /// wrapper alongside the raw gate, sign it with the PEM private key
        /// (RSA/SHA-256), and drop the public key sidecar next to the
        /// wrapper. The wrapper, `.json.sig`, and
        /// `workbench-evidence-signing-public.pem` allow downstream observers
        /// to verify the gate via `ao2 contract verify-obligation-gate-signing`
        /// without operating a workbench HTTP serve loop. Default off
        /// preserves the unsigned legacy path.
        #[arg(long = "support-signing-key")]
        support_signing_key: Option<PathBuf>,
        /// Identifier recorded inside the signed wrapper's audit_event when
        /// `--support-signing-key` is in effect. Defaults to
        /// `ao2-contract-gate`.
        #[arg(long = "support-signer-id", default_value = "ao2-contract-gate")]
        support_signer_id: String,
        /// Operator role recorded inside the signed wrapper's audit_event.
        /// Must be non-empty for the verifier to flag the wrapper as
        /// `ao2_owned`. Defaults to `operator`.
        #[arg(long = "support-operator-role", default_value = "operator")]
        support_operator_role: String,
        /// Run identifier recorded inside the signed wrapper's audit_event.
        /// Defaults to `ao2-contract-gate-cli` (the wrapper is not tied to a
        /// workbench run; this field exists for observability parity with
        /// workbench-emitted wrappers).
        #[arg(long = "support-run-id", default_value = "ao2-contract-gate-cli")]
        support_run_id: String,
        /// Directory the signed wrapper + sidecars are written to when
        /// `--support-signing-key` is in effect. Defaults to the same
        /// directory as `--out` so the verifier finds them via the gate's
        /// parent-dir fallback.
        #[arg(long = "exports-dir")]
        exports_dir: Option<PathBuf>,
        /// Escape valve for legacy producers that have not yet provisioned
        /// an AO2 signing key. As of slice 18, `ao2 contract gate` defaults
        /// to fail-closed when `--support-signing-key` is not provided, so
        /// downstream `ao2 release gate` (slice 11, default-on signing
        /// required) and `/api/release-gate` (slice 17, HTTP default-on)
        /// never accept a silently-unsigned gate. Pass this flag to opt out
        /// and emit an unsigned raw gate only — the downstream release gate
        /// will still reject it unless `--allow-unsigned-obligation-gates`
        /// is also set there. Hidden because the principled path is to
        /// provision a signing key.
        #[arg(long = "allow-unsigned-obligation-gates", hide = true)]
        allow_unsigned_obligation_gates: bool,
    },
    /// Sign an existing raw `ao2.obligation-gate.v1` artifact by emitting the
    /// AO2 workbench evidence-export wrapper and signature sidecars expected
    /// by release gates and control-plane observers.
    SignObligationGate {
        #[arg(long = "gate")]
        gate: PathBuf,
        #[arg(long = "support-signing-key")]
        support_signing_key: PathBuf,
        #[arg(long = "support-signer-id", default_value = "ao2-contract-gate")]
        support_signer_id: String,
        #[arg(long = "support-operator-role", default_value = "operator")]
        support_operator_role: String,
        #[arg(long = "support-run-id", default_value = "ao2-contract-gate-cli")]
        support_run_id: String,
        #[arg(long = "exports-dir")]
        exports_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Annotate {
        #[arg(long)]
        ledger: PathBuf,
        #[arg(long = "obligation-id")]
        obligation_id: String,
        #[arg(long = "evidence-path")]
        evidence_path: Option<String>,
        #[arg(long = "evidence-line")]
        evidence_line: Option<usize>,
        #[arg(long)]
        detail: Option<String>,
        #[arg(long)]
        waiver: Option<String>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Audit whether a raw `obligation-gate-<stage>.json` file was AO2-signed
    /// via the workbench evidence-export signing path.
    ///
    /// The closure verdict (and release verification) consumes raw obligation
    /// gate files. AO2 is the only producer that can sign these gates — the
    /// signing wrapper is `ao2.workbench-evidence-export.v1` with a sidecar
    /// `.json.sig` + directory-shared `workbench-evidence-signing-public.pem`.
    /// This subcommand searches `--evidence-exports-dir` for a wrapper whose
    /// embedded gate equals the supplied raw gate, then verifies the
    /// wrapper's RSA/SHA-256 signature. Emits a per-gate verdict observers
    /// (CI, release-gate hooks, control-plane displays) can fail closed on.
    ///
    /// Possible signing_status values:
    ///   - `signed-and-verified`: a matching wrapper exists and its signature
    ///     verifies against the on-disk public key
    ///   - `wrapper-not-found`: no wrapper in the directory embeds a gate
    ///     equal to the supplied raw gate (the gate is unsigned)
    ///   - `signature-missing`: matching wrapper exists but `.json.sig` or
    ///     public key is absent on disk
    ///   - `signature-invalid`: matching wrapper exists with sidecars but
    ///     RSA/SHA-256 verify fails (tampered wrapper, sidecar, or key)
    ///
    /// All factory-v3 paths are excluded — AO2 is the only legitimate signer
    /// for obligation-gate evidence exports; surfacing this is the point of
    /// the audit.
    VerifyObligationGateSigning {
        #[arg(long = "gate")]
        gate: PathBuf,
        #[arg(long = "evidence-exports-dir")]
        evidence_exports_dir: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Inventory every obligation gate observers would consume and audit
    /// each via `contract verify-obligation-gate-signing`. Two scan modes:
    ///
    /// - `--target <PATH>`: walk every raw `obligation-gate-*.json` under
    ///   `<target>/.ao2/runs/*/evidence-pack/` (the in-run gate layout).
    /// - `--summary <PATH>`: walk every `obligation_gates.gates[].path`
    ///   entry inside the supplied release summary JSON (the
    ///   release-gate-consumed gate layout, which may live outside any
    ///   `.ao2/runs/` tree — e.g., free-standing files referenced by a
    ///   factory-v3 nightly enriched summary).
    ///
    /// At least one of `--target` or `--summary` is required. When both
    /// are given, gates seen by both scans are deduplicated by literal
    /// path and the `source` field lists every scan that observed the
    /// gate.
    ///
    /// This is the fleet-migration on-ramp for
    /// `ao2 release gate --require-obligation-gate-signing` (which fails
    /// closed unless every release-summary-embedded gate is
    /// `signed-and-verified`). Operators run this survey first, remediate
    /// the unsigned gates by re-emitting them via
    /// `ao2 workbench obligation-gate --support-signing-key <PEM>`, then
    /// re-run the survey to confirm before flipping the release gate to
    /// signing-required.
    ///
    /// Read-only; exits 0 even when unsigned gates are found so it can be
    /// used in a non-failing inventory cron.
    ObligationGateSigningSurvey {
        #[arg(long)]
        target: Option<PathBuf>,
        #[arg(long)]
        summary: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum GitCommand {
    Status {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Diff {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        stat: bool,
        #[arg(long)]
        json: bool,
    },
    Commit {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        message: String,
        #[arg(long = "path")]
        paths: Vec<PathBuf>,
        #[arg(long)]
        approve_action_digest: Option<String>,
        #[arg(long, default_value = "human:local-operator")]
        approver: String,
        #[arg(long)]
        json: bool,
    },
    Tag {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        tag: String,
        #[arg(long)]
        message: Option<String>,
        #[arg(long)]
        approve_action_digest: Option<String>,
        #[arg(long, default_value = "human:local-operator")]
        approver: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum IssueCommand {
    /// Canonicalize and classify one GitHub issue URL without network or GitHub writes.
    Intake {
        #[arg(long)]
        url: String,
        #[arg(long)]
        json: bool,
    },
    /// Plan isolated repository acquisition for one validated GitHub issue.
    Acquire {
        #[arg(long)]
        url: String,
        #[arg(long = "upstream-url")]
        upstream_url: String,
        #[arg(long = "default-branch", default_value = "main")]
        default_branch: String,
        #[arg(long = "target-commit")]
        target_commit: String,
        #[arg(long)]
        json: bool,
    },
    /// Produce bounded, offline GitHub issue discovery evidence from a sanitized page envelope.
    Discover {
        #[arg(long = "page-envelope")]
        page_envelope: PathBuf,
        #[arg(long)]
        url: String,
        #[arg(long)]
        repository: String,
        #[arg(long = "default-branch")]
        default_branch: String,
        #[arg(long = "head-sha")]
        head_sha: String,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "completed-at")]
        completed_at: String,
        #[arg(long = "snapshot-limit", default_value_t = 50)]
        snapshot_limit: usize,
        #[arg(long = "candidate-limit", default_value_t = 10)]
        candidate_limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Validate a sanitized historical repair pack without executing work.
    RepairPack {
        #[command(subcommand)]
        command: RepairPackCommand,
    },
    /// Build, verify, or exercise a bounded local draft pull request action.
    DraftPr {
        #[command(subcommand)]
        command: github_issue_draft::DraftPrCommand,
    },
    /// Verify or apply exact digest-bound GitHub repair publication actions.
    Publish {
        #[command(subcommand)]
        command: github_issue_publish::PublishCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum RepairPackCommand {
    /// Verify a strict manifest and its referenced artifacts without following links.
    Validate {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum FactoryCommand {
    Plan {
        #[arg(long = "request")]
        request: PathBuf,
        #[arg(long = "profile")]
        profile: Option<PathBuf>,
        #[arg(long = "runspec")]
        runspec: Option<PathBuf>,
        #[arg(long = "role-contract")]
        role_contracts: Vec<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-planner")]
        signer_id: String,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Run {
        #[arg(long = "plan")]
        plan: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-runner")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    ReplacementSmoke {
        #[arg(long = "request")]
        request: PathBuf,
        #[arg(long = "profile")]
        profile: Option<PathBuf>,
        #[arg(long = "runspec")]
        runspec: PathBuf,
        #[arg(long = "role-contract")]
        role_contracts: Vec<PathBuf>,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-replacement-smoke")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    GovernedRun {
        #[arg(long = "request")]
        request: PathBuf,
        #[arg(long = "profile")]
        profile: Option<PathBuf>,
        #[arg(long = "runspec")]
        runspec: PathBuf,
        #[arg(long = "role-contract")]
        role_contracts: Vec<PathBuf>,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-governed-run")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    GreenfieldRun {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-greenfield-run")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    GreenfieldSpecIngest {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        json: bool,
    },
    GreenfieldSpecIngestSubmit {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "provider-prompt-dir")]
        provider_prompt_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "approve-action-digest")]
        approve_action_digest: Option<String>,
        #[arg(long)]
        json: bool,
    },
    AppRun {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-app-run")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    AppRunBundle {
        #[arg(long = "app-run")]
        app_run: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectPlan {
        #[arg(long = "project-spec")]
        project_spec: PathBuf,
        #[arg(long = "project-root")]
        project_root: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "provider-prompt-dir")]
        provider_prompt_dir: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-acceptance-rubric")]
        signer_id: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectPlanValidate {
        #[arg(long = "project-plan")]
        project_plan: PathBuf,
        #[arg(long = "project-root")]
        project_root: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    EvaluatorRubric {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-native-evaluator-rubric")]
        signer_id: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    CloserDecision {
        #[arg(long = "rubric")]
        rubric: PathBuf,
        #[arg(long = "rubric-sha256")]
        rubric_sha256: String,
        #[arg(long = "evidence")]
        evidence: PathBuf,
        #[arg(long = "evidence-sha256")]
        evidence_sha256: String,
        #[arg(long = "skill-contract-manifest")]
        skill_contract_manifest: PathBuf,
        #[arg(long = "skill-contract-manifest-sha256")]
        skill_contract_manifest_sha256: String,
        #[arg(long = "signing-key")]
        signing_key: PathBuf,
        #[arg(long = "signer-id", default_value = "ao2-native-closer-decision")]
        signer_id: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    CloserDecisionVerify {
        #[arg(long = "decision")]
        decision: PathBuf,
        #[arg(long = "decision-sha256")]
        decision_sha256: String,
        #[arg(long)]
        json: bool,
    },
    ProjectStart {
        #[arg(long = "project-spec")]
        project_spec: PathBuf,
        #[arg(long = "project-root")]
        project_root: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "provider-prompt-dir")]
        provider_prompt_dir: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-project-start")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "handoff-bundle-out")]
        handoff_bundle_out: Option<PathBuf>,
        #[arg(long = "handoff-bundle-report")]
        handoff_bundle_report: Option<PathBuf>,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartHermesFlowContract {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartHermesContext {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartBundle {
        #[arg(long = "project-start")]
        project_start: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartBundleVerify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartSummary {
        #[arg(long = "project-start")]
        project_start: PathBuf,
        #[arg(long = "bundle-verification")]
        bundle_verification: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        markdown: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartClosure {
        #[arg(long = "queue-status")]
        queue_status: PathBuf,
        #[arg(long = "latest-queue-status")]
        latest_queue_status: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectStartClosureVerify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ReplacementPacket {
        #[arg(long = "queue-status")]
        queue_status: PathBuf,
        #[arg(long = "latest-queue-status")]
        latest_queue_status: PathBuf,
        #[arg(long)]
        closure: PathBuf,
        #[arg(long = "closure-verification")]
        closure_verification: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long = "cross-os-readback")]
        cross_os_readbacks: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    ReplacementPacketVerify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectRun {
        #[arg(long = "project-spec")]
        project_spec: PathBuf,
        #[arg(long = "project-plan")]
        project_plan: Option<PathBuf>,
        #[arg(long = "resume-from")]
        resume_from: Option<PathBuf>,
        #[arg(long = "app-run")]
        app_runs: Vec<PathBuf>,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-project-run")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ProjectAcceptanceReview {
        #[arg(long = "project-run")]
        project_run: PathBuf,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(
            long = "signer-id",
            default_value = "ao2-factory-project-acceptance-review"
        )]
        signer_id: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ReplacementSmokeGate {
        #[arg(long = "smoke")]
        smokes: Vec<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    ReplacementParityStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "governed-run")]
        governed_run: PathBuf,
        #[arg(long = "governed-run-sha256")]
        governed_run_sha256: String,
        #[arg(long = "three-os-gate")]
        three_os_gate: PathBuf,
        #[arg(long = "three-os-gate-sha256")]
        three_os_gate_sha256: String,
        #[arg(long)]
        json: bool,
    },
    VerifyHandoff {
        #[arg(long = "handoff")]
        handoff: PathBuf,
        #[arg(long)]
        json: bool,
    },
    VerifyRunResult {
        #[arg(long = "run-result")]
        run_result: PathBuf,
        #[arg(long)]
        json: bool,
    },
    VerifyPlanningEvidence {
        #[arg(long = "evidence")]
        evidence: PathBuf,
        #[arg(long = "signed-payload")]
        signed_payload: Option<PathBuf>,
        #[arg(long = "signature")]
        signature: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    VerifyEvaluatorDecision {
        #[arg(long = "decision")]
        decision: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Evaluate {
        #[arg(long = "evidence-pack")]
        evidence_pack: PathBuf,
        #[arg(long = "report")]
        report: Option<PathBuf>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-native-evaluator-closer")]
        signer_id: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    QueueSubmit {
        #[arg(long = "plan")]
        plan: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    QueueSubmitProjectStart {
        #[arg(long = "project-spec")]
        project_spec: PathBuf,
        #[arg(long = "project-root")]
        project_root: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "provider-prompt-dir")]
        provider_prompt_dir: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-project-start-queue")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: Option<PathBuf>,
        #[arg(long = "handoff-bundle-out")]
        handoff_bundle_out: Option<PathBuf>,
        #[arg(long = "handoff-bundle-report")]
        handoff_bundle_report: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartComplete {
        #[arg(long = "project-spec")]
        project_spec: PathBuf,
        #[arg(long = "project-root")]
        project_root: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long = "provider-prompt-dir")]
        provider_prompt_dir: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(
            long = "signer-id",
            default_value = "ao2-factory-project-start-queue-complete"
        )]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long = "handoff-bundle-out")]
        handoff_bundle_out: Option<PathBuf>,
        #[arg(long = "handoff-bundle-report")]
        handoff_bundle_report: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartCompleteStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartCompletionSummary {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartCompletionSummaryMemory {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "approve-action-digest")]
        approve_action_digest: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartCompletionSummaryMemoryStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecovery {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartLatestRecovery {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryAction {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeReceipt {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeCheckpoint {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "approve-action-digest")]
        approve_action_digest: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeCheckpointStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeContinuity {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePlan {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeClaim {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "approve-plan-sha256")]
        approve_plan_sha256: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeClaimStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeContinuationContract {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeContinue {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "approve-claim-status-sha256")]
        approve_claim_status_sha256: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumeContinuationStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationAction {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationExecute {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long = "approve-continuation-status-sha256")]
        approve_continuation_status_sha256: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationExecutionStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationNextAction {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long = "post-continuation-execution-status-sha256")]
        post_continuation_execution_status_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationClosure {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long = "post-continuation-execution-status-sha256")]
        post_continuation_execution_status_sha256: String,
        #[arg(long = "post-continuation-next-action-sha256")]
        post_continuation_next_action_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationEvaluatorDecision {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "queue-sha256")]
        queue_sha256: String,
        #[arg(long = "recovery-packet-sha256")]
        recovery_packet_sha256: String,
        #[arg(long = "plan-sha256")]
        plan_sha256: String,
        #[arg(long = "claim-status-sha256")]
        claim_status_sha256: String,
        #[arg(long = "continuation-status-sha256")]
        continuation_status_sha256: String,
        #[arg(long = "post-continuation-execution-status-sha256")]
        post_continuation_execution_status_sha256: String,
        #[arg(long = "post-continuation-next-action-sha256")]
        post_continuation_next_action_sha256: String,
        #[arg(long = "closure-sha256")]
        closure_sha256: String,
        #[arg(long = "signing-key")]
        signing_key: PathBuf,
        #[arg(long = "signer-id", default_value = "ao2-recovery-closure-evaluator")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleaseHandoff {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        decision: PathBuf,
        #[arg(long = "signed-payload")]
        signed_payload: PathBuf,
        #[arg(long)]
        signature: PathBuf,
        #[arg(long = "public-key")]
        public_key: PathBuf,
        #[arg(long = "closure-sha256")]
        closure_sha256: String,
        #[arg(long = "decision-sha256")]
        decision_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleaseHandoffStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long = "closure-sha256")]
        closure_sha256: String,
        #[arg(long = "decision-sha256")]
        decision_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleaseHandoffStatusSummary {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        status: PathBuf,
        #[arg(long = "status-sha256")]
        status_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleaseHandoffStatusSummaryExport {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleasePublicationReadiness {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        export: PathBuf,
        #[arg(long = "export-sha256")]
        export_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleasePublicationDispatchPlan {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        readiness: PathBuf,
        #[arg(long = "readiness-sha256")]
        readiness_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleasePublicationReadback {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "dispatch-plan")]
        dispatch_plan: PathBuf,
        #[arg(long = "dispatch-plan-sha256")]
        dispatch_plan_sha256: String,
        #[arg(long)]
        observation: PathBuf,
        #[arg(long = "observation-sha256")]
        observation_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartRecoveryResumePostContinuationReleasePublicationClosure {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        readback: PathBuf,
        #[arg(long = "readback-sha256")]
        readback_sha256: String,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartNextAction {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        contract: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueProjectStartPublishOperatorRecord {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        contract: PathBuf,
        #[arg(long = "record-out")]
        record_out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueList {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    QueueStatus {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "latest-completed-project-start")]
        latest_completed_project_start: bool,
        #[arg(long)]
        json: bool,
    },
    QueueCompletionContract {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "latest-completed-project-start")]
        latest_completed_project_start: bool,
        #[arg(long)]
        json: bool,
    },
    QueueCompletionContractConsume {
        #[arg(long)]
        contract: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Emit a factory-v3/ao2-watchdog-no-active-ao2-runs-attestation/v1
    /// payload from a captured `ao2 factory queue-list --json` snapshot.
    /// AO2-native producer for Phase 2 exit-gate item #5 (cancel
    /// semantics owned by AO2).
    CancelAuthority {
        #[arg(long = "queue-list-json")]
        queue_list_json: PathBuf,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long = "produced-at-ms")]
        produced_at_ms: Option<i64>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Emit an `ao2.factory-v3-compat-workbench-queue-transition.v1`
    /// payload that binds a recorded cancellation in an `ao2 factory
    /// queue-list --json` snapshot to a specific pid the watchdog
    /// terminated. AO2-native producer for the second half of Phase 2
    /// exit-gate item #5 (cancel-transition counterpart to
    /// `cancel-authority`).
    CancelTransition {
        #[arg(long = "queue-list-json")]
        queue_list_json: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "terminated-pid")]
        terminated_pid: i64,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long = "produced-at-ms")]
        produced_at_ms: Option<i64>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    QueueCancel {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueRetry {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        json: bool,
    },
    QueueRunNext {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-queue-runner")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    PackEvidence {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-pack-evidence-signer")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Phase 2 exit-gate single-command bridge: canonicalize every role id in
    /// an AO Operator RunSpec via the deterministic AO Operator -> AO2
    /// provider-contract mapping, then emit an AO2-native bridge-evidence
    /// JSON. Mapping-only dry-run; no ao2 invocation is shelled out. The
    /// `factory-v3` Python bridge can defer to this subcommand via its
    /// `--ao2-native-passthrough` mode (Phase 2 exit-gate items #1 and #2,
    /// AO2-native producer).
    Bridge {
        #[arg(long = "runspec")]
        runspec: PathBuf,
        #[arg(long = "work-request")]
        work_request: Option<PathBuf>,
        #[arg(long = "profile")]
        profile: Option<PathBuf>,
        #[arg(long = "role-contracts-dir")]
        role_contracts_dir: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-factory-bridge")]
        signer_id: String,
        #[arg(
            long = "now-ms",
            help = "override timestamp (millis) for deterministic tests"
        )]
        now_ms: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Emit the canonicalized AO Operator -> AO2 provider-contract mapping
    /// table or its sha256 digest. Useful for parity checks against the
    /// factory-v3 Python module that originally owned this mapping.
    BridgeMapping {
        #[arg(long)]
        digest: bool,
    },
    /// Verify a signed AO2-native bridge evidence file end-to-end.
    ///
    /// Resolves the signed-payload, signature, and public-key sidecars from
    /// the `signature` block embedded in the evidence body by default; any of
    /// the three can be overridden via the matching `--signed-payload`,
    /// `--signature`, or `--public-key` flags. Runs four independent integrity
    /// checks before emitting a verdict:
    ///   1. every sha256 recorded in the body's `signature` block matches the
    ///      bytes on disk for that file;
    ///   2. the canonical body minus the `signature` field equals the signed
    ///      payload bytes (catches body tampering even when only the sidecar
    ///      is forged);
    ///   3. RSA/SHA-256 cryptographic verify of the signature against the
    ///      signed-payload bytes with the supplied public key;
    ///   4. the body's trust boundary still names AO2 as the bridge owner.
    ///
    /// All four must hold for `status: accepted`. AO2-owned verifier surface;
    /// factory-v3 and other observers can shell out to this to confirm
    /// bridge evidence without re-implementing the signature pattern.
    VerifyBridgeEvidence {
        #[arg(long = "evidence")]
        evidence: PathBuf,
        #[arg(long = "signed-payload")]
        signed_payload: Option<PathBuf>,
        #[arg(long = "signature")]
        signature: Option<PathBuf>,
        #[arg(long = "public-key")]
        public_key: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum GreenfieldCommand {
    Ingest {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: Option<String>,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-greenfield-ingest")]
        signer_id: String,
        #[arg(long = "out-dir")]
        out_dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    GovernedRun {
        #[arg(long = "spec")]
        spec: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "run-id")]
        run_id: String,
        #[arg(long = "verifier-command", default_value = "npm run verify")]
        verifier_command: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        provider_prompt: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: Option<PathBuf>,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long = "factory-decision")]
        factory_decision: Option<PathBuf>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-greenfield-governed-run")]
        signer_id: String,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    ThreeOsSmokeGate {
        #[arg(long = "smoke")]
        smokes: Vec<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ReleaseCommand {
    Package {
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
        version: String,
        #[arg(long)]
        binary: Option<PathBuf>,
        #[arg(long)]
        target_label: Option<String>,
    },
    SmokeSummary {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long)]
        require_native_windows: bool,
    },
    SummaryEnrich {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long = "obligation-gate")]
        obligation_gates: Vec<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Gate {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long)]
        provenance_dir: PathBuf,
        #[arg(long)]
        macos_archive: Option<PathBuf>,
        #[arg(long)]
        linux_archive: PathBuf,
        #[arg(long = "linux-x86-64-archive")]
        linux_x86_64_archive: PathBuf,
        #[arg(long)]
        windows_archive: PathBuf,
        #[arg(long)]
        require_native_windows: bool,
        #[arg(long = "replacement-smoke-gate")]
        replacement_smoke_gate: Option<PathBuf>,
        #[arg(long = "greenfield-three-os-smoke-gate")]
        greenfield_three_os_smoke_gate: Option<PathBuf>,
        #[arg(long = "governed-run-evidence")]
        governed_run_evidence: Vec<PathBuf>,
        #[arg(long = "factory-project-run-summary")]
        factory_project_run_summaries: Vec<PathBuf>,
        /// Opt out of the default-on obligation-gate signing requirement.
        /// When set, the release gate skips the AO2 workbench
        /// evidence-export wrapper audit and omits the
        /// `obligation_gate_signing` block from the report. Intended for
        /// legacy callers that have not yet provisioned a signing key.
        #[arg(long = "allow-unsigned-obligation-gates")]
        allow_unsigned_obligation_gates: bool,
        /// Back-compat: accepted but no-op now that obligation-gate
        /// signing is required by default. Kept so existing scripts
        /// that explicitly opt in continue to parse.
        #[arg(long = "require-obligation-gate-signing", hide = true)]
        require_obligation_gate_signing: bool,
    },
    Compare {
        #[arg(long, default_value = "target/release-download")]
        release_download_dir: PathBuf,
        #[arg(long, default_value = "target/release-comparison-bundles")]
        out_dir: PathBuf,
        #[arg(long)]
        signing_key: Option<PathBuf>,
        #[arg(long, default_value = "ao2-release-operator")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    CompareVerify {
        #[arg(long)]
        bundle_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    SupportBundleBuild {
        #[arg(long = "release-assembly")]
        release_assembly: PathBuf,
        #[arg(long)]
        readiness: PathBuf,
        #[arg(long)]
        handoff: PathBuf,
        #[arg(long)]
        cockpit: PathBuf,
        #[arg(long = "evaluator-decision")]
        evaluator_decision: PathBuf,
        #[arg(long = "storage-support")]
        storage_support: PathBuf,
        #[arg(long)]
        replay: PathBuf,
        #[arg(long = "report-contract-verification")]
        report_contract_verification: Option<PathBuf>,
        #[arg(long = "install-verification")]
        install_verification: PathBuf,
        #[arg(long = "hosted-release-smoke")]
        hosted_release_smoke: PathBuf,
        #[arg(long = "report-target")]
        report_target: Option<PathBuf>,
        #[arg(long = "report-run-id")]
        report_run_id: Option<String>,
        #[arg(long = "report")]
        report: Option<PathBuf>,
        #[arg(long = "report-index")]
        report_index: Option<PathBuf>,
        #[arg(long = "operator-evidence")]
        operator_evidence: PathBuf,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    SupportBundleVerify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        checksums: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    EvidenceBundle {
        #[arg(long)]
        out_dir: PathBuf,
        /// Artifact to include, in `<label>=<path>` form. Repeat for
        /// readiness, handoff, evaluator-decision, three-OS smoke,
        /// provider-acceptance, or other locally produced release evidence.
        #[arg(long = "artifact")]
        artifacts: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    EvidenceBundleVerify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Phase1DecisionBuild {
        #[arg(long = "release-gate")]
        release_gate: PathBuf,
        #[arg(long = "replacement-smoke-gate")]
        replacement_smoke_gate: Option<PathBuf>,
        #[arg(long = "governed-run-evidence")]
        governed_run_evidence: Vec<PathBuf>,
        #[arg(long = "factory-project-run-summary")]
        factory_project_run_summaries: Vec<PathBuf>,
        #[arg(long = "provider-acceptance-preservation")]
        provider_acceptance_preservation: Option<PathBuf>,
        #[arg(long)]
        operator: String,
        #[arg(long)]
        rationale: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long = "checklist-out")]
        checklist_out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Phase1DecisionPublish {
        #[arg(long)]
        decision: PathBuf,
        #[arg(long = "signing-key")]
        signing_key: PathBuf,
        #[arg(long = "signer-id", default_value = "ao2-phase1-release")]
        signer_id: String,
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Phase1ThreeOsSmokeBuild {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long)]
        provenance: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Phase1ThreeOsSmokePublish {
        #[arg(long)]
        smoke: PathBuf,
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Phase1HistoryFetch {
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Phase1PromotionStatus {
        #[arg(long)]
        root: PathBuf,
        #[arg(long = "evidence-bundle")]
        evidence_bundle: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Phase1PromotionInputsVerify {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long, default_value = "preflight")]
        mode: String,
        #[arg(long)]
        json: bool,
    },
    Phase1PromotionInputsPublish {
        #[arg(long)]
        verification: PathBuf,
        #[arg(long = "control-plane-url")]
        control_plane_url: String,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// AO2-native producer of `factory-v3/ao2-release-evaluator-decision/v1`.
    /// Designed for byte-equal parity (under canonical JSON serialisation)
    /// with `factory-v3/scripts/ao2_release_evaluator_decision.py`. The
    /// factory-v3 Python remains in a read-only audit role; AO2 is the
    /// canonical producer.
    EvaluatorDecisionBuild {
        #[arg(long)]
        readiness: PathBuf,
        #[arg(long = "handoff-checklist")]
        handoff_checklist: PathBuf,
        #[arg(long = "support-bundle-status")]
        support_bundle_status: PathBuf,
        #[arg(long = "write-json")]
        write_json: Option<PathBuf>,
        #[arg(long = "write-md")]
        write_md: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// AO2-native producer of `factory-v3/ao2-release-handoff-checklist/v1`.
    /// Byte-equal parity (under canonical JSON sort) with
    /// `factory-v3/scripts/ao2_release_handoff_checklist.py`. factory-v3
    /// remains in a read-only audit role.
    HandoffChecklistBuild {
        #[arg(long)]
        handoff: PathBuf,
        #[arg(long = "write-json")]
        write_json: Option<PathBuf>,
        #[arg(long = "write-md")]
        write_md: Option<PathBuf>,
        /// Expected repository HEAD in `<repo>=<head>` form. Repeat to
        /// pin multiple repositories.
        #[arg(long = "expected-repo-head")]
        expected_repo_head: Vec<String>,
        /// Allow input that does not contain an AO2 release-candidate
        /// handoff to produce a `planned`/`skipped` checklist instead of
        /// failing.
        #[arg(long = "allow-skipped")]
        allow_skipped: bool,
        #[arg(long)]
        json: bool,
    },
    SignProvenance {
        #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
        version: String,
        #[arg(long)]
        macos_archive: Option<PathBuf>,
        #[arg(long)]
        linux_archive: PathBuf,
        #[arg(long = "linux-x86-64-archive")]
        linux_x86_64_archive: PathBuf,
        #[arg(long)]
        windows_archive: PathBuf,
        #[arg(long, default_value = "dist-provenance")]
        provenance_dir: PathBuf,
        #[arg(long, default_value = ".release-signing/ao2-release-signing-key.pem")]
        private_key: PathBuf,
        #[arg(long)]
        release_tag: Option<String>,
        #[arg(long)]
        json: bool,
    },
    VerifyProvenance {
        #[arg(long)]
        macos_archive: Option<PathBuf>,
        #[arg(long)]
        linux_archive: PathBuf,
        #[arg(long = "linux-x86-64-archive")]
        linux_x86_64_archive: PathBuf,
        #[arg(long)]
        windows_archive: PathBuf,
        #[arg(long, default_value = "dist-provenance")]
        provenance_dir: PathBuf,
        #[arg(long)]
        public_key: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum TemplateCommand {
    List,
    Show { name: String },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProviderCommand {
    List,
    Registry {
        #[arg(long = "control-plane-url")]
        control_plane_url: Option<String>,
        #[arg(long = "api-token")]
        api_token: Option<String>,
        #[arg(long = "api-token-env")]
        api_token_env: Option<String>,
        #[arg(long = "signing-key")]
        signing_key: Option<PathBuf>,
        #[arg(long = "signer-id", default_value = "ao2-provider-registry")]
        signer_id: String,
        #[arg(long)]
        json: bool,
    },
    Doctor {
        #[arg(long, default_value = "scripted")]
        provider: String,
    },
    Matrix {
        #[arg(long)]
        json: bool,
    },
    Contract {
        #[arg(long, default_value = "scripted")]
        provider: String,
        #[arg(long)]
        verify: bool,
        #[arg(long = "require")]
        require: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    SmokeAll {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 90)]
        minimum_score: u64,
        #[arg(long = "live-provider")]
        live_provider: Vec<String>,
    },
    Gate {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long = "require")]
        require: Vec<String>,
        #[arg(long, default_value_t = 90)]
        minimum_score: u64,
        #[arg(long)]
        json: bool,
    },
    Pilot {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        provider: String,
        #[arg(long, default_value = "bug-fix")]
        template: String,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long = "provider-prompt-file")]
        provider_prompt_file: PathBuf,
        #[arg(long, default_value_t = 1)]
        max_repair_attempts: usize,
        #[arg(long = "provider-max-budget-usd")]
        provider_max_budget_usd: Option<f64>,
        #[arg(long, default_value_t = 90)]
        minimum_score: u64,
        #[arg(long)]
        json: bool,
    },
    CostLedger {
        #[arg(long, default_value = "target/provider-pilot-acceptance")]
        acceptance_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    CostTrend {
        #[arg(long, default_value = "target/provider-pilot-acceptance")]
        acceptance_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Score {
        #[arg(long, default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        run_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum PluginCommand {
    /// Emit the Codex/Claude wrapper readiness contract for AO2 production
    /// plugin integrations. This command is read-only and produces no provider
    /// calls, queue writes, memory writes, or AO artifact mutations.
    Readiness {
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Package a deterministic Codex/Claude plugin manifest with wrapper
    /// schema examples and local OAuth CLI-only smoke fixtures.
    Manifest {
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a packaged Codex/Claude plugin manifest by exact digest, file
    /// SHA256 metadata, local OAuth posture, and observer-only trust boundary.
    ManifestVerify {
        #[arg(long = "manifest-dir")]
        manifest_dir: PathBuf,
        #[arg(long = "manifest-sha256")]
        manifest_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Dry-run Codex/Claude plugin setup from a digest-pinned manifest
    /// verification artifact without executing providers or mutating queues.
    InstallSmoke {
        #[arg(long = "manifest-dir")]
        manifest_dir: PathBuf,
        #[arg(long)]
        verification: PathBuf,
        #[arg(long = "verification-sha256")]
        verification_sha256: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Bundle a verified Codex/Claude plugin manifest and install-smoke proof
    /// into one digest-addressed package for local wrapper distribution.
    Package {
        #[arg(long = "manifest-dir")]
        manifest_dir: PathBuf,
        #[arg(long = "manifest-verification")]
        manifest_verification: PathBuf,
        #[arg(long = "manifest-verification-sha256")]
        manifest_verification_sha256: String,
        #[arg(long = "install-smoke")]
        install_smoke: PathBuf,
        #[arg(long = "install-smoke-sha256")]
        install_smoke_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Codex/Claude plugin package summary and archive
    /// after distribution without executing providers or mutating queues.
    PackageVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Install a verified plugin package into clean Codex/Claude wrapper
    /// fixture directories and run the consumer lifecycle from installed paths.
    DistributionRehearsal {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify clean Codex/Claude wrapper sandboxes from installed package and
    /// adapter paths without provider execution or mutable side effects.
    ConsumerLifecycle {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "adapter-scaffold")]
        adapter_scaffold: PathBuf,
        #[arg(long = "adapter-scaffold-sha256")]
        adapter_scaffold_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Write a digest-pinned Windows PowerShell runner for the consumer
    /// lifecycle proof so recovery needs one command session after transfer.
    ConsumerLifecycleWindowsRecovery {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "adapter-scaffold")]
        adapter_scaffold: PathBuf,
        #[arg(long = "adapter-scaffold-sha256")]
        adapter_scaffold_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows consumer-lifecycle proofs into one
    /// digest-bound K37 read-only observer bundle.
    ConsumerLifecycleObserverBundle {
        #[arg(long = "macos-lifecycle")]
        macos_lifecycle: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-lifecycle")]
        ubuntu_lifecycle: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-lifecycle")]
        windows_lifecycle: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned consumer-lifecycle observer bundle after
    /// distribution without executing providers or mutating queues.
    ConsumerLifecycleObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Aggregate digest-pinned plugin shipment evidence into one local release
    /// candidate proof for factory-v3 parity audit and read-only observation.
    ReleaseCandidate {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "distribution-rehearsal")]
        distribution_rehearsal: PathBuf,
        #[arg(long = "distribution-rehearsal-sha256")]
        distribution_rehearsal_sha256: String,
        #[arg(long = "adapter-observer-bundle")]
        adapter_observer_bundle: PathBuf,
        #[arg(long = "adapter-observer-bundle-sha256")]
        adapter_observer_bundle_sha256: String,
        #[arg(long = "adapter-observer-archive")]
        adapter_observer_archive: PathBuf,
        #[arg(long = "adapter-observer-archive-sha256")]
        adapter_observer_archive_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-bundle")]
        adapter_install_smoke_observer_bundle: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-bundle-sha256")]
        adapter_install_smoke_observer_bundle_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-archive")]
        adapter_install_smoke_observer_archive: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-archive-sha256")]
        adapter_install_smoke_observer_archive_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-bundle")]
        consumer_lifecycle_observer_bundle: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-bundle-sha256")]
        consumer_lifecycle_observer_bundle_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-archive")]
        consumer_lifecycle_observer_archive: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-archive-sha256")]
        consumer_lifecycle_observer_archive_sha256: String,
        #[arg(long = "release-gate-with-replacement-observer-bundle")]
        release_gate_with_replacement_observer_bundle: PathBuf,
        #[arg(long = "release-gate-with-replacement-observer-bundle-sha256")]
        release_gate_with_replacement_observer_bundle_sha256: String,
        #[arg(long = "release-gate-with-replacement-observer-archive")]
        release_gate_with_replacement_observer_archive: PathBuf,
        #[arg(long = "release-gate-with-replacement-observer-archive-sha256")]
        release_gate_with_replacement_observer_archive_sha256: String,
        #[arg(long = "control-plane-fixture-handoff-verification")]
        control_plane_fixture_handoff_verification: PathBuf,
        #[arg(long = "control-plane-fixture-handoff-verification-sha256")]
        control_plane_fixture_handoff_verification_sha256: String,
        #[arg(long = "control-plane-readback-commit")]
        control_plane_readback_commit: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned plugin release-candidate summary after
    /// distribution without executing providers or mutating queues.
    ReleaseCandidateVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Write a digest-pinned Windows PowerShell runner for release-candidate
    /// proof recovery so the next attempt needs one command session.
    ReleaseCandidateWindowsRecovery {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "distribution-rehearsal")]
        distribution_rehearsal: PathBuf,
        #[arg(long = "distribution-rehearsal-sha256")]
        distribution_rehearsal_sha256: String,
        #[arg(long = "adapter-observer-bundle")]
        adapter_observer_bundle: PathBuf,
        #[arg(long = "adapter-observer-bundle-sha256")]
        adapter_observer_bundle_sha256: String,
        #[arg(long = "adapter-observer-archive")]
        adapter_observer_archive: PathBuf,
        #[arg(long = "adapter-observer-archive-sha256")]
        adapter_observer_archive_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-bundle")]
        adapter_install_smoke_observer_bundle: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-bundle-sha256")]
        adapter_install_smoke_observer_bundle_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-archive")]
        adapter_install_smoke_observer_archive: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-archive-sha256")]
        adapter_install_smoke_observer_archive_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-bundle")]
        consumer_lifecycle_observer_bundle: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-bundle-sha256")]
        consumer_lifecycle_observer_bundle_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-archive")]
        consumer_lifecycle_observer_archive: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-archive-sha256")]
        consumer_lifecycle_observer_archive_sha256: String,
        #[arg(long = "release-gate-with-replacement-observer-bundle")]
        release_gate_with_replacement_observer_bundle: PathBuf,
        #[arg(long = "release-gate-with-replacement-observer-bundle-sha256")]
        release_gate_with_replacement_observer_bundle_sha256: String,
        #[arg(long = "release-gate-with-replacement-observer-archive")]
        release_gate_with_replacement_observer_archive: PathBuf,
        #[arg(long = "release-gate-with-replacement-observer-archive-sha256")]
        release_gate_with_replacement_observer_archive_sha256: String,
        #[arg(long = "control-plane-fixture-handoff-verification")]
        control_plane_fixture_handoff_verification: PathBuf,
        #[arg(long = "control-plane-fixture-handoff-verification-sha256")]
        control_plane_fixture_handoff_verification_sha256: String,
        #[arg(long = "control-plane-readback-commit")]
        control_plane_readback_commit: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Windows release-candidate recovery package before
    /// transferring it to an unreliable Windows SSH host.
    ReleaseCandidateWindowsRecoveryVerify {
        #[arg(long)]
        recovery: PathBuf,
        #[arg(long = "recovery-sha256")]
        recovery_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package the verified Windows recovery runner and AO2 source archive
    /// into one digest-bound transfer bundle for a single Windows session.
    ReleaseCandidateWindowsTransferBundle {
        #[arg(long = "ao2-source-archive")]
        ao2_source_archive: PathBuf,
        #[arg(long = "ao2-source-archive-sha256")]
        ao2_source_archive_sha256: String,
        #[arg(long = "recovery-dir")]
        recovery_dir: PathBuf,
        #[arg(long)]
        recovery: PathBuf,
        #[arg(long = "recovery-sha256")]
        recovery_sha256: String,
        #[arg(long = "recovery-verification")]
        recovery_verification: PathBuf,
        #[arg(long = "recovery-verification-sha256")]
        recovery_verification_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows release-candidate verification
    /// proofs into one digest-bound K37 read-only observer bundle.
    ReleaseCandidateObserverBundle {
        #[arg(long = "macos-verification")]
        macos_verification: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-verification")]
        ubuntu_verification: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-verification")]
        windows_verification: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned release-candidate observer bundle after
    /// distribution without executing providers or mutating queues.
    ReleaseCandidateObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package Pulse apply-result proofs into one digest-bound K37 read-only
    /// observer bundle.
    PulseApplyObserverBundle {
        #[arg(long = "macos-apply-result")]
        macos_apply_result: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-apply-result")]
        ubuntu_apply_result: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-apply-result")]
        windows_apply_result: Option<PathBuf>,
        #[arg(long = "windows-sha256")]
        windows_sha256: Option<String>,
        #[arg(long = "windows-unavailable-reason")]
        windows_unavailable_reason: Option<String>,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Pulse apply-result observer bundle after
    /// distribution without executing providers or mutating queues.
    PulseApplyObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package Pulse once-mode proofs into one digest-bound K37 read-only
    /// observer bundle.
    PulseOnceObserverBundle {
        #[arg(long = "macos-once")]
        macos_once: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-once")]
        ubuntu_once: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-once")]
        windows_once: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Pulse once-mode observer bundle after
    /// distribution without executing providers or mutating queues.
    PulseOnceObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package Pulse chain-mode proofs into one digest-bound K37 read-only
    /// observer bundle.
    PulseChainObserverBundle {
        #[arg(long = "macos-chain")]
        macos_chain: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-chain")]
        ubuntu_chain: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-chain")]
        windows_chain: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Pulse chain-mode observer bundle after
    /// distribution without executing providers or mutating queues.
    PulseChainObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package Pulse eval-loop proofs into one digest-bound K37 read-only
    /// observer bundle.
    PulseEvalLoopObserverBundle {
        #[arg(long = "macos-eval-loop")]
        macos_eval_loop: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-eval-loop")]
        ubuntu_eval_loop: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-eval-loop")]
        windows_eval_loop: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Pulse eval-loop observer bundle after
    /// distribution without executing providers or mutating queues.
    PulseEvalLoopObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package Pulse executor, governed-task, and task-result proofs into one
    /// digest-bound K37 read-only observer bundle.
    PulseExecutorObserverBundle {
        #[arg(long = "macos-executor")]
        macos_executor: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-executor")]
        ubuntu_executor: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-executor")]
        windows_executor: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Pulse executor observer bundle after
    /// distribution without executing providers or mutating queues.
    PulseExecutorObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Prepare a digest-pinned Windows PowerShell runner for Pulse apply-result
    /// and K37 observer-bundle proof recovery.
    PulseApplyWindowsRecovery {
        #[arg(long = "apply-result")]
        apply_result: PathBuf,
        #[arg(long = "apply-result-sha256")]
        apply_result_sha256: String,
        #[arg(long = "observer-bundle")]
        observer_bundle: PathBuf,
        #[arg(long = "observer-bundle-sha256")]
        observer_bundle_sha256: String,
        #[arg(long = "observer-archive")]
        observer_archive: PathBuf,
        #[arg(long = "observer-archive-sha256")]
        observer_archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Prepare a digest-pinned release-candidate fixture handoff for
    /// ao2-control-plane read-only observation without editing control-plane
    /// source files.
    ReleaseCandidateControlPlaneFixtureHandoff {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned release-candidate control-plane fixture handoff
    /// after distribution without editing or mutating control-plane files.
    ReleaseCandidateControlPlaneFixtureHandoffVerify {
        #[arg(long)]
        handoff: PathBuf,
        #[arg(long = "handoff-sha256")]
        handoff_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Produce a final digest-pinned Codex/Claude install transcript from a
    /// release-candidate observer bundle without executing providers.
    FinalInstallTranscript {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows Codex/Claude final install
    /// transcripts into one digest-bound K37 read-only observer bundle.
    FinalInstallTranscriptObserverBundle {
        #[arg(long = "macos-codex-transcript")]
        macos_codex_transcript: PathBuf,
        #[arg(long = "macos-codex-sha256")]
        macos_codex_sha256: String,
        #[arg(long = "macos-claude-transcript")]
        macos_claude_transcript: PathBuf,
        #[arg(long = "macos-claude-sha256")]
        macos_claude_sha256: String,
        #[arg(long = "ubuntu-codex-transcript")]
        ubuntu_codex_transcript: PathBuf,
        #[arg(long = "ubuntu-codex-sha256")]
        ubuntu_codex_sha256: String,
        #[arg(long = "ubuntu-claude-transcript")]
        ubuntu_claude_transcript: PathBuf,
        #[arg(long = "ubuntu-claude-sha256")]
        ubuntu_claude_sha256: String,
        #[arg(long = "windows-codex-transcript")]
        windows_codex_transcript: PathBuf,
        #[arg(long = "windows-codex-sha256")]
        windows_codex_sha256: String,
        #[arg(long = "windows-claude-transcript")]
        windows_claude_transcript: PathBuf,
        #[arg(long = "windows-claude-sha256")]
        windows_claude_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Aggregate digest-pinned plugin shipment evidence into one final
    /// operator-facing readiness handoff for Codex/Claude plugin release.
    ShipmentReadiness {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "adapter-observer-bundle")]
        adapter_observer_bundle: PathBuf,
        #[arg(long = "adapter-observer-bundle-sha256")]
        adapter_observer_bundle_sha256: String,
        #[arg(long = "adapter-observer-archive")]
        adapter_observer_archive: PathBuf,
        #[arg(long = "adapter-observer-archive-sha256")]
        adapter_observer_archive_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-bundle")]
        adapter_install_smoke_observer_bundle: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-bundle-sha256")]
        adapter_install_smoke_observer_bundle_sha256: String,
        #[arg(long = "adapter-install-smoke-observer-archive")]
        adapter_install_smoke_observer_archive: PathBuf,
        #[arg(long = "adapter-install-smoke-observer-archive-sha256")]
        adapter_install_smoke_observer_archive_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-bundle")]
        consumer_lifecycle_observer_bundle: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-bundle-sha256")]
        consumer_lifecycle_observer_bundle_sha256: String,
        #[arg(long = "consumer-lifecycle-observer-archive")]
        consumer_lifecycle_observer_archive: PathBuf,
        #[arg(long = "consumer-lifecycle-observer-archive-sha256")]
        consumer_lifecycle_observer_archive_sha256: String,
        #[arg(long = "release-candidate-observer-bundle")]
        release_candidate_observer_bundle: PathBuf,
        #[arg(long = "release-candidate-observer-bundle-sha256")]
        release_candidate_observer_bundle_sha256: String,
        #[arg(long = "release-candidate-observer-archive")]
        release_candidate_observer_archive: PathBuf,
        #[arg(long = "release-candidate-observer-archive-sha256")]
        release_candidate_observer_archive_sha256: String,
        #[arg(long = "final-install-transcript-observer-bundle")]
        final_install_transcript_observer_bundle: PathBuf,
        #[arg(long = "final-install-transcript-observer-bundle-sha256")]
        final_install_transcript_observer_bundle_sha256: String,
        #[arg(long = "final-install-transcript-observer-archive")]
        final_install_transcript_observer_archive: PathBuf,
        #[arg(long = "final-install-transcript-observer-archive-sha256")]
        final_install_transcript_observer_archive_sha256: String,
        #[arg(long = "control-plane-readback-commit")]
        control_plane_readback_commit: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Prepare a digest-pinned fixture handoff for ao2-control-plane read-only
    /// observation without editing control-plane source files.
    ControlPlaneFixtureHandoff {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned ao2-control-plane fixture handoff after
    /// distribution without editing or mutating control-plane files.
    ControlPlaneFixtureHandoffVerify {
        #[arg(long)]
        handoff: PathBuf,
        #[arg(long = "handoff-sha256")]
        handoff_sha256: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows K37 observer inputs into one
    /// digest-bound bundle for read-only control-plane observation.
    DistributionObserverBundle {
        #[arg(long = "macos-observer")]
        macos_observer: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-observer")]
        ubuntu_observer: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-observer")]
        windows_observer: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows clean packaged-plugin rehearsal
    /// proofs into one digest-bound K37 read-only operator index.
    CleanPackageOperatorIndex {
        #[arg(long = "macos-rehearsal")]
        macos_rehearsal: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-rehearsal")]
        ubuntu_rehearsal: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-rehearsal")]
        windows_rehearsal: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows packaged-replacement hardening
    /// proofs into one digest-bound K37 read-only observer bundle.
    PackagedReplacementObserverBundle {
        #[arg(long = "macos-proof")]
        macos_proof: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-proof")]
        ubuntu_proof: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-proof")]
        windows_proof: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned packaged-replacement observer bundle after
    /// distribution without executing providers or mutating queues.
    PackagedReplacementObserverBundleVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        archive: PathBuf,
        #[arg(long = "archive-sha256")]
        archive_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows release-gate-with-replacement
    /// rollups into one digest-bound K37 read-only observer bundle.
    ReleaseGateWithReplacementObserverBundle {
        #[arg(long = "macos-rollup")]
        macos_rollup: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-rollup")]
        ubuntu_rollup: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-rollup")]
        windows_rollup: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Generate Codex/Claude local OAuth wrapper adapter scaffolds from a
    /// digest-pinned plugin package and K37 observer bundle.
    AdapterScaffold {
        #[arg(long = "package-summary")]
        package_summary: PathBuf,
        #[arg(long = "package-summary-sha256")]
        package_summary_sha256: String,
        #[arg(long = "package-archive")]
        package_archive: PathBuf,
        #[arg(long = "package-archive-sha256")]
        package_archive_sha256: String,
        #[arg(long = "k37-bundle")]
        k37_bundle: PathBuf,
        #[arg(long = "k37-bundle-sha256")]
        k37_bundle_sha256: String,
        #[arg(long = "k37-archive")]
        k37_archive: PathBuf,
        #[arg(long = "k37-archive-sha256")]
        k37_archive_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify a generated Codex/Claude adapter scaffold by exact digest,
    /// adapter file SHA256s, local OAuth posture, and observer-only boundaries.
    AdapterScaffoldVerify {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Dry-run Codex/Claude adapter installation from a digest-pinned scaffold
    /// without executing providers, mutating queues, or writing memory.
    AdapterInstallSmoke {
        #[arg(long)]
        summary: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Verify a digest-pinned Codex/Claude adapter install-smoke artifact
    /// after distribution without executing providers or mutating queues.
    AdapterInstallSmokeVerify {
        #[arg(long)]
        smoke: PathBuf,
        #[arg(long = "smoke-sha256")]
        smoke_sha256: String,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows adapter install-smoke verification
    /// artifacts into one digest-bound K37 read-only observer bundle.
    AdapterInstallSmokeObserverBundle {
        #[arg(long = "macos-verification")]
        macos_verification: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-verification")]
        ubuntu_verification: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-verification")]
        windows_verification: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Package macOS, Ubuntu, and Windows adapter scaffold verification
    /// artifacts into one digest-bound K37 read-only observer bundle.
    AdapterObserverBundle {
        #[arg(long = "macos-verification")]
        macos_verification: PathBuf,
        #[arg(long = "macos-sha256")]
        macos_sha256: String,
        #[arg(long = "ubuntu-verification")]
        ubuntu_verification: PathBuf,
        #[arg(long = "ubuntu-sha256")]
        ubuntu_sha256: String,
        #[arg(long = "windows-verification")]
        windows_verification: PathBuf,
        #[arg(long = "windows-sha256")]
        windows_sha256: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Execute a digest-pinned AO2 factory app-run/project-run through the
    /// plugin wrapper contract and persist token-safe evidence for audit.
    WrapperHarness {
        #[arg(long)]
        readiness: PathBuf,
        #[arg(long = "readiness-sha256")]
        readiness_sha256: String,
        #[arg(long = "args-file")]
        args_file: PathBuf,
        #[arg(long = "args-sha256")]
        args_sha256: String,
        #[arg(long = "run-kind")]
        run_kind: String,
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Verify an existing plugin wrapper-harness evidence directory by digest.
    WrapperHarnessVerify {
        #[arg(long = "evidence-dir")]
        evidence_dir: PathBuf,
        #[arg(long = "summary-sha256")]
        summary_sha256: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum AdapterCommand {
    Doctor {
        #[arg(long, default_value = "scripted")]
        provider: String,
    },
    Run {
        #[arg(long, default_value = "scripted")]
        provider: String,
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        command: PathBuf,
        #[arg(long, default_value = "", allow_hyphen_values = true)]
        args: String,
        #[arg(long, default_value = "adapter-cli")]
        role_id: String,
        #[arg(long)]
        keep_sandbox: bool,
        #[arg(long, default_value_t = DEFAULT_PROVIDER_TIMEOUT_SECONDS)]
        timeout_seconds: u64,
    },
    Prompt {
        #[arg(long, default_value = "scripted")]
        provider: String,
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        prompt_file: Option<PathBuf>,
        #[arg(long, default_value = "adapter-prompt")]
        role_id: String,
        #[arg(long)]
        keep_sandbox: bool,
        #[arg(long, default_value_t = DEFAULT_PROVIDER_TIMEOUT_SECONDS)]
        timeout_seconds: u64,
        #[arg(long = "max-budget-usd")]
        max_budget_usd: Option<f64>,
    },
    Patch {
        #[command(subcommand)]
        command: AdapterPatchCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum AdapterPatchCommand {
    Preview {
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        sandbox: PathBuf,
    },
    Apply {
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        sandbox: PathBuf,
        #[arg(long)]
        digest: String,
        #[arg(long, default_value = "human:local-user")]
        approver: String,
    },
}
