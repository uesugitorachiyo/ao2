use std::fs;
use std::path::Path;

#[test]
fn cli_signature_helpers_use_native_crypto_without_openssl_shellouts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let main_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/main.rs")).expect("cli source exists");
    let contract_gate_signing_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/contract_gate_signing.rs"))
            .expect("contract gate signing source exists");
    let factory_app_run_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_app_run.rs"))
            .expect("factory app-run source exists");
    let factory_evaluator_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_evaluator.rs"))
            .expect("factory evaluator source exists");
    let factory_evidence_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_evidence.rs"))
            .expect("factory evidence source exists");
    let factory_project_execution_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_project_execution.rs"))
            .expect("factory project execution source exists");
    let factory_project_planning_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_project_planning.rs"))
            .expect("factory project planning source exists");
    let factory_project_start_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_project_start.rs"))
            .expect("factory project start source exists");
    let factory_queue_execution_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_queue_execution.rs"))
            .expect("factory queue execution source exists");
    let factory_queue_project_start_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_queue_project_start.rs"))
            .expect("factory queue project-start source exists");
    let factory_governance_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_governance.rs"))
            .expect("factory governance source exists");
    let factory_run_execution_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_run_execution.rs"))
            .expect("factory run execution source exists");
    let factory_queue_recovery_release_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_queue_recovery_release.rs"))
            .expect("factory queue recovery release source exists");
    let release_crypto_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/release_crypto.rs"))
            .expect("release crypto source exists");
    let release_provenance_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/release_provenance.rs"))
            .expect("release provenance source exists");
    let pulse_run_source = fs::read_to_string(root.join("crates/ao2-cli/src/pulse_run.rs"))
        .expect("pulse run source exists");
    let pulse_eval_loop_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/pulse_eval_loop.rs"))
            .expect("pulse eval-loop source exists");
    let skill_contract_manifest_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/skill_contract_manifest.rs"))
            .expect("skill contract manifest source exists");
    let run_resume_source = fs::read_to_string(root.join("crates/ao2-cli/src/run_resume.rs"))
        .expect("run resume source exists")
        .replace("\r\n", "\n");
    let provider_ops_source = fs::read_to_string(root.join("crates/ao2-cli/src/provider_ops.rs"))
        .expect("provider operations source exists")
        .replace("\r\n", "\n");
    let run_execution_source = fs::read_to_string(root.join("crates/ao2-cli/src/run_execution.rs"))
        .expect("run execution source exists")
        .replace("\r\n", "\n");
    let cli_source = fs::read_to_string(root.join("crates/ao2-cli/src/cli.rs"))
        .expect("CLI declaration source exists")
        .replace("\r\n", "\n");
    let cli_util_source = fs::read_to_string(root.join("crates/ao2-cli/src/cli_util.rs"))
        .expect("CLI utility source exists")
        .replace("\r\n", "\n");
    let factory_dispatch_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/factory_dispatch.rs"))
            .expect("factory dispatch source exists")
            .replace("\r\n", "\n");
    let release_dispatch_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/release_dispatch.rs"))
            .expect("release dispatch source exists")
            .replace("\r\n", "\n");
    let provider_run_repair_tests =
        fs::read_to_string(root.join("crates/ao2-cli/tests/cli_provider_run_repair.rs"))
            .expect("cli provider run/repair tests exist");
    for function_name in [
        "factory_closer_decision_json",
        "factory_closer_decision_verify_json",
    ] {
        assert!(
            factory_governance_source.contains(&format!("fn {function_name}(")),
            "{function_name} must be owned by factory_governance"
        );
        assert!(
            !main_source.contains(&format!("fn {function_name}(")),
            "{function_name} must not remain in main"
        );
    }
    for function_name in [
        "pulse_artifact_key",
        "pulse_run_execute_json",
        "pulse_run_apply_dry_run_json",
        "pulse_apply_normalized_path",
        "pulse_apply_target_path",
        "pulse_apply_status_body",
        "validate_pulse_task_contract",
    ] {
        assert!(
            pulse_run_source.contains(&format!("fn {function_name}(")),
            "{function_name} must be owned by pulse_run"
        );
        assert!(
            !main_source.contains(&format!("fn {function_name}(")),
            "{function_name} must not remain in main"
        );
    }
    assert!(
        pulse_eval_loop_source.contains("use crate::pulse_run::validate_pulse_task_contract;"),
        "pulse_eval_loop must import task-contract validation from pulse_run"
    );
    assert!(
        pulse_run_source
            .contains("use crate::cli_util::{atomic_write_text, json_bool, json_string};"),
        "pulse_run must import shared helpers directly from cli_util"
    );
    assert!(
        skill_contract_manifest_source.contains("enum SkillContractManifestCommand"),
        "skill contract manifest command must be owned by skill_contract_manifest"
    );
    assert!(
        !main_source.contains("enum SkillContractManifestCommand"),
        "skill contract manifest command must not remain in main"
    );
    for declaration_name in [
        "SKILL_CONTRACT_REQUIRED_INVENTORY",
        "SkillContractManifestEntrySpec",
    ] {
        assert!(
            skill_contract_manifest_source.contains(declaration_name),
            "{declaration_name} must be owned by skill_contract_manifest"
        );
        assert!(
            !main_source.contains(declaration_name),
            "{declaration_name} must not remain in main"
        );
    }
    for direct_import in [
        "crate::artifact_safety::factory_app_run_bundle_reject_secret_markers",
        "crate::cli_util::{",
        "crate::plugin_distribution::{",
    ] {
        assert!(
            skill_contract_manifest_source.contains(direct_import),
            "skill_contract_manifest must retain direct import edge {direct_import}"
        );
    }
    for function_name in [
        "skill_contract_manifest",
        "skill_contract_manifest_generate",
        "skill_contract_manifest_verify",
        "skill_contract_manifest_entry",
        "validate_skill_contract_manifest",
    ] {
        assert!(
            skill_contract_manifest_source.contains(&format!("fn {function_name}(")),
            "{function_name} must be owned by skill_contract_manifest"
        );
        assert!(
            !main_source.contains(&format!("fn {function_name}(")),
            "{function_name} must not remain in main"
        );
    }
    assert!(
        run_resume_source.contains("struct ApprovalRecoveryContext"),
        "approval recovery context must be owned by run_resume"
    );
    assert!(
        !main_source.contains("struct ApprovalRecoveryContext"),
        "approval recovery context must not remain in main"
    );
    for function_name in [
        "read_approval_recovery_context",
        "approval_recovery_context_by_ticket",
        "pending_approval_recovery_context",
        "print_approval_recovery_context",
        "approve",
        "replay",
    ] {
        assert!(
            run_resume_source.contains(&format!("fn {function_name}(")),
            "{function_name} must be owned by run_resume"
        );
        assert!(
            !main_source.contains(&format!("fn {function_name}(")),
            "{function_name} must not remain in main"
        );
    }
    assert!(
        run_execution_source.contains(
            "use crate::run_resume::{\n    approve_and_resume_persisted_sandbox_patches, pending_approval_recovery_context,\n    print_approval_recovery_context,\n};"
        ),
        "run_execution must import approval recovery helpers directly from run_resume"
    );
    assert!(
        main_source.contains("use run_resume::{approve, repair, replay};"),
        "main must import approval, repair, and replay commands directly from run_resume"
    );
    assert!(
        run_resume_source.contains("fn read_approval_recovery_context(")
            && !run_resume_source.contains("pub(crate) fn read_approval_recovery_context(")
            && run_resume_source.contains("fn approval_recovery_context_by_ticket(")
            && !run_resume_source.contains("pub(crate) fn approval_recovery_context_by_ticket("),
        "run_resume lookup helpers must remain private"
    );
    assert!(
        cli_util_source.contains("pub(crate) fn is_sha256_hex(")
            && !main_source.contains("fn is_sha256_hex("),
        "is_sha256_hex must be owned by cli_util"
    );
    for function_name in [
        "repair",
        "repair_source_context_from_evidence_pack",
        "unresolved_concerns_from_closures",
        "string_values",
        "latest_artifact_content",
    ] {
        assert!(
            run_resume_source.contains(&format!("fn {function_name}(")),
            "{function_name} must be owned by run_resume"
        );
        assert!(
            !main_source.contains(&format!("fn {function_name}(")),
            "{function_name} must not remain in main"
        );
    }
    assert!(
        main_source.contains("use run_resume::{approve, repair, replay};")
            && !main_source.contains("pub(crate) use run_resume::"),
        "main must import repair dispatch directly without re-exporting it"
    );
    let workbench_queue_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/workbench_queue.rs"))
            .expect("workbench queue source exists")
            .replace("\r\n", "\n");
    assert!(
        workbench_queue_source
            .contains("use crate::run_resume::repair_source_context_from_evidence_pack;"),
        "workbench_queue must import repair evidence parsing directly from run_resume"
    );
    let run_reporting_source = fs::read_to_string(root.join("crates/ao2-cli/src/run_reporting.rs"))
        .expect("run reporting source exists")
        .replace("\r\n", "\n");
    assert!(
        run_reporting_source.contains("pub(crate) fn print_run_summary(")
            && !run_execution_source.contains("fn print_run_summary(")
            && !run_resume_source.contains("use crate::run_execution::"),
        "shared run summary rendering must live in run_reporting without a resume/execution cycle"
    );
    assert!(
        run_execution_source.contains("crate::run_reporting::print_run_summary(&summary);")
            && run_resume_source.contains("use crate::run_reporting::print_run_summary;"),
        "run execution and repair resume must depend directly on run_reporting"
    );
    assert!(
        run_resume_source.contains("fn unresolved_concerns_from_closures(")
            && !run_resume_source.contains("pub(crate) fn unresolved_concerns_from_closures(")
            && run_resume_source.contains("fn string_values(")
            && !run_resume_source.contains("pub(crate) fn string_values(")
            && run_resume_source.contains("fn latest_artifact_content(")
            && !run_resume_source.contains("pub(crate) fn latest_artifact_content("),
        "repair evidence parsing details must remain private"
    );
    assert!(
        run_resume_source.lines().count() <= 5_000,
        "run_resume.rs must remain within the production-file hard ceiling"
    );
    {
        use sha2::{Digest, Sha256};

        let repair_start = run_resume_source
            .find("pub(crate) fn repair(")
            .expect("repair resume block exists");
        let normalized_body = format!("{}\n", run_resume_source[repair_start..].trim_end())
            .replace("pub(crate) fn ", "fn ");
        assert_eq!(
            format!("{:x}", Sha256::digest(normalized_body.as_bytes())),
            "416172855993ece88b18d9c54cbbb0d6b0ef9e009779c1929e913b4afeacde7e",
            "repair resume bodies must remain byte-identical to the parent extraction"
        );
    }
    for function_name in ["adapter", "adapter_patch", "split_tab_args"] {
        assert!(
            provider_ops_source.contains(&format!("fn {function_name}(")),
            "{function_name} must be owned by provider_ops"
        );
        assert!(
            !main_source.contains(&format!("fn {function_name}(")),
            "{function_name} must not remain in main"
        );
    }
    assert!(
        main_source.contains("use provider_ops::{")
            && main_source.contains("adapter, provider,")
            && !main_source.contains("pub(crate) use provider_ops::"),
        "main must import adapter dispatch directly from provider_ops without re-exporting it"
    );
    assert!(
        provider_ops_source
            .contains("use crate::cli::{AdapterCommand, AdapterPatchCommand, ProviderCommand};"),
        "provider_ops must import adapter command types directly from cli"
    );
    assert!(
        provider_ops_source.contains("fn adapter_patch(")
            && !provider_ops_source.contains("pub(crate) fn adapter_patch(")
            && provider_ops_source.contains("fn split_tab_args(")
            && !provider_ops_source.contains("pub(crate) fn split_tab_args("),
        "adapter implementation details must remain private"
    );
    assert!(
        provider_ops_source.lines().count() <= 5_000,
        "provider_ops.rs must remain within the production-file hard ceiling"
    );
    {
        use sha2::{Digest, Sha256};

        let adapter_start = provider_ops_source
            .find("pub(crate) fn adapter(")
            .expect("adapter dispatch block exists");
        let normalized_body = format!("{}\n", provider_ops_source[adapter_start..].trim_end())
            .replace("pub(crate) fn ", "fn ");
        assert_eq!(
            format!("{:x}", Sha256::digest(normalized_body.as_bytes())),
            "7ef9c5b835129040b43ffcee439980007f7d5a1477d1555db509edb1f3ef0b3b",
            "adapter dispatch bodies must remain byte-identical to the parent extraction"
        );
    }
    assert!(
        release_dispatch_source.contains("pub(crate) fn release("),
        "release command dispatch must be owned by release_dispatch"
    );
    assert!(
        !main_source.contains("fn release("),
        "release command dispatch must not remain in main"
    );
    assert!(
        main_source.contains("use release_dispatch::release;")
            && !main_source.contains("pub(crate) use release_dispatch::release;"),
        "main must import release dispatch directly without re-exporting it"
    );
    assert!(
        release_dispatch_source.contains("use crate::cli::ReleaseCommand;")
            && !release_dispatch_source.contains("use crate::{")
            && !release_dispatch_source.contains("super::*"),
        "release_dispatch must use direct owner imports without root glob dependencies"
    );
    assert!(
        release_dispatch_source.lines().count() <= 5_000,
        "release_dispatch.rs must remain within the production-file hard ceiling"
    );
    {
        use sha2::{Digest, Sha256};

        let release_start = release_dispatch_source
            .find("pub(crate) fn release(")
            .expect("release dispatch block exists");
        let normalized_body = format!("{}\n", release_dispatch_source[release_start..].trim_end())
            .replace("pub(crate) fn ", "fn ");
        assert_eq!(
            format!("{:x}", Sha256::digest(normalized_body.as_bytes())),
            "b6c829a66f249bacf6816cb2e72b9495b259d89923695ceacb4f2340119ae250",
            "release dispatch body must remain byte-identical to the parent extraction"
        );
    }
    assert!(
        cli_source.contains("pub(crate) struct Cli"),
        "top-level CLI parser must be owned by cli"
    );
    assert!(
        !main_source.contains("struct Cli"),
        "top-level CLI parser must not remain in main"
    );
    for declaration_name in [
        "Command",
        "CpCommand",
        "ReportCommand",
        "RepairCommand",
        "RunsCommand",
        "CockpitCommand",
        "PulseCommand",
        "PulseEvalLoopCommand",
        "WorkbenchCommand",
        "ControlPlaneCommand",
        "ControlPlaneSourcesCommand",
        "ControlPlaneHistoryCommand",
        "ContractCommand",
        "GitCommand",
        "IssueCommand",
        "FactoryCommand",
        "GreenfieldCommand",
        "ReleaseCommand",
        "TemplateCommand",
        "ProviderCommand",
        "PluginCommand",
        "AdapterCommand",
        "AdapterPatchCommand",
    ] {
        assert!(
            cli_source.contains(&format!("enum {declaration_name}")),
            "{declaration_name} must be owned by cli"
        );
        assert!(
            !main_source.contains(&format!("enum {declaration_name}")),
            "{declaration_name} must not remain in main"
        );
    }
    assert!(
        cli_source.contains("fn parse_bool(") && !main_source.contains("fn parse_bool("),
        "the Clap boolean parser must move with CLI declarations"
    );
    assert!(
        cli_source.contains("#[allow(clippy::large_enum_variant)]\npub(crate) enum PluginCommand"),
        "PluginCommand must retain its large-enum allowance"
    );
    assert!(
        !main_source.contains("pub(crate) use cli::")
            && !main_source.contains("pub(crate) use cli::{"),
        "CLI command types must not be re-exported through the crate root"
    );
    assert!(
        cli_source.lines().count() <= 5_000,
        "cli.rs must remain within the production-file hard ceiling"
    );
    for (relative_path, direct_import) in [
        (
            "crates/ao2-cli/src/control_plane_snapshot.rs",
            "use crate::cli::CpCommand;",
        ),
        (
            "crates/ao2-cli/src/control_plane_ops.rs",
            "use crate::cli::{",
        ),
        (
            "crates/ao2-cli/src/git_cmd.rs",
            "use crate::cli::GitCommand;",
        ),
        (
            "crates/ao2-cli/src/github_issue_intake.rs",
            "use crate::cli::IssueCommand;",
        ),
        ("crates/ao2-cli/src/provider_ops.rs", "use crate::cli::{"),
        (
            "crates/ao2-cli/src/plugin_cli.rs",
            "use crate::cli::PluginCommand;",
        ),
    ] {
        let source =
            fs::read_to_string(root.join(relative_path)).expect("CLI command consumer exists");
        assert!(
            source.contains(direct_import),
            "{relative_path} must import its command type directly from cli"
        );
    }
    assert!(
        factory_dispatch_source.contains("pub(crate) fn factory("),
        "Factory command dispatch must be owned by factory_dispatch"
    );
    assert!(
        !main_source.contains("fn factory("),
        "Factory command dispatch must not remain in main"
    );
    assert!(
        factory_dispatch_source.lines().count() <= 5_000,
        "factory_dispatch.rs must remain within the production-file hard ceiling"
    );
    for direct_import in [
        "use crate::cli::FactoryCommand;",
        "use crate::cli_util::{atomic_write_text, json_string, json_u64, sha256_file};",
        "use crate::release_crypto::{",
    ] {
        assert!(
            factory_dispatch_source.contains(direct_import),
            "factory_dispatch must retain direct import edge {direct_import}"
        );
    }
    for forbidden_import in ["use crate::{", "super::*", "ProcessCommand"] {
        assert!(
            !factory_dispatch_source.contains(forbidden_import),
            "factory_dispatch must not contain {forbidden_import}"
        );
    }
    assert!(
        factory_dispatch_source.contains("factory_bridge::"),
        "factory bridge calls must remain qualified"
    );
    assert!(
        main_source.contains("use factory_dispatch::factory;")
            && !main_source.contains("pub(crate) use factory_dispatch::factory;"),
        "main must import factory dispatch directly without re-exporting it"
    );
    {
        use sha2::{Digest, Sha256};

        let dispatch_start = factory_dispatch_source
            .find("pub(crate) fn factory(")
            .expect("factory dispatch body exists");
        let normalized_body = format!("{}\n", factory_dispatch_source[dispatch_start..].trim_end());
        assert_eq!(
            format!("{:x}", Sha256::digest(normalized_body.as_bytes())),
            "edfec7071595a02b27a626ac399b6c03ba52f3da65bcd4f30c6f440d546ffb39",
            "factory dispatch body must remain byte-identical to the parent extraction"
        );
    }
    for function_name in [
        "verify_release_archive_signature",
        "derive_public_key_from_private_key",
        "sign_file_with_private_key",
        "verify_file_signature",
    ] {
        let function_source = function_body_source(&release_crypto_source, function_name);
        assert!(
            !function_source.contains("ProcessCommand::new(\"openssl\")"),
            "{function_name} must not shell out to openssl"
        );
    }
    let function_source = function_body_source(
        &release_provenance_source,
        "verify_release_provenance_signature",
    );
    assert!(
        !function_source.contains("ProcessCommand::new(\"openssl\")"),
        "verify_release_provenance_signature must not shell out to openssl"
    );
    for source in [
        &main_source,
        &contract_gate_signing_source,
        &factory_app_run_source,
        &factory_evaluator_source,
        &factory_evidence_source,
        &factory_project_execution_source,
        &factory_project_planning_source,
        &factory_project_start_source,
        &factory_queue_execution_source,
        &factory_queue_project_start_source,
        &factory_governance_source,
        &factory_run_execution_source,
        &factory_queue_recovery_release_source,
        &factory_dispatch_source,
        &release_crypto_source,
        &release_provenance_source,
    ] {
        assert!(
            !source.contains("ProcessCommand::new(\"openssl\")"),
            "CLI release signing sources must not shell out to openssl"
        );
    }
    assert!(release_crypto_source.contains("RsaPrivateKey"));
    assert!(release_crypto_source.contains("RsaPublicKey"));
    assert!(
        !provider_run_repair_tests.contains("Command::new(\"openssl\")"),
        "integration tests must generate signing keys through native AO2 helpers"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let main_source = fs::read_to_string(root.join("crates/ao2-cli/src/main.rs"))
        .expect("CLI source exists")
        .replace("\r\n", "\n");
    let cli_util_source = fs::read_to_string(root.join("crates/ao2-cli/src/cli_util.rs"))
        .expect("CLI utility source exists")
        .replace("\r\n", "\n");
    let workbench_render_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/workbench_render.rs"))
            .expect("Workbench render source exists")
            .replace("\r\n", "\n");

    for function_name in [
        "query_value_owned",
        "form_value_owned",
        "shell_quote",
        "format_budget_usd",
        "http_html_response",
        "http_json_response",
        "http_text_response",
        "percent_decode",
        "percent_encode",
        "generate_api_token",
        "resolve_api_token",
    ] {
        assert!(
            cli_util_source.contains(&format!("fn {function_name}(")),
            "{function_name} must be owned by cli_util"
        );
        assert!(
            !main_source.contains(&format!("fn {function_name}(")),
            "{function_name} must not remain in main"
        );
    }
    for function_name in ["render_workbench_job_detail_page", "workbench_file_anchor"] {
        assert!(
            workbench_render_source.contains(&format!("fn {function_name}(")),
            "{function_name} must be owned by workbench_render"
        );
        assert!(
            !main_source.contains(&format!("fn {function_name}(")),
            "{function_name} must not remain in main"
        );
    }
    assert!(
        !main_source.contains("pub(crate) use cli_util::")
            && !main_source.contains("pub(crate) use workbench_render::"),
        "shared helpers must not be re-exported through the crate root"
    );
    assert!(
        workbench_render_source.contains("fn workbench_file_anchor(")
            && !workbench_render_source.contains("pub(super) fn workbench_file_anchor(")
            && !workbench_render_source.contains("pub(crate) fn workbench_file_anchor("),
        "the Workbench file-anchor helper must remain private"
    );
    for (relative_path, required_helpers) in [
        (
            "crates/ao2-cli/src/control_plane_ops.rs",
            &[
                "query_value_owned",
                "http_html_response",
                "http_json_response",
                "http_text_response",
                "generate_api_token",
            ][..],
        ),
        (
            "crates/ao2-cli/src/provider_ops.rs",
            &[
                "shell_quote",
                "format_budget_usd",
                "generate_api_token",
                "resolve_api_token",
            ][..],
        ),
        (
            "crates/ao2-cli/src/control_plane_snapshot.rs",
            &["resolve_api_token"][..],
        ),
        (
            "crates/ao2-cli/src/evidence_publish.rs",
            &["resolve_api_token"][..],
        ),
        (
            "crates/ao2-cli/src/phase1_promotion.rs",
            &["resolve_api_token"][..],
        ),
        (
            "crates/ao2-cli/src/release_dispatch.rs",
            &["resolve_api_token"][..],
        ),
        (
            "crates/ao2-cli/src/workbench_app.rs",
            &[
                "form_value_owned",
                "shell_quote",
                "percent_decode",
                "percent_encode",
                "generate_api_token",
            ][..],
        ),
        (
            "crates/ao2-cli/src/workbench_evidence_delivery.rs",
            &["form_value_owned"][..],
        ),
        (
            "crates/ao2-cli/src/workbench_factory_api.rs",
            &["query_value_owned", "form_value_owned", "percent_decode"][..],
        ),
        (
            "crates/ao2-cli/src/workbench_memory.rs",
            &["query_value_owned"][..],
        ),
        (
            "crates/ao2-cli/src/workbench_provider_pilot.rs",
            &["query_value_owned", "form_value_owned"][..],
        ),
        (
            "crates/ao2-cli/src/workbench_queue.rs",
            &[
                "query_value_owned",
                "form_value_owned",
                "format_budget_usd",
                "generate_api_token",
            ][..],
        ),
        (
            "crates/ao2-cli/src/workbench_release.rs",
            &["query_value_owned", "form_value_owned"][..],
        ),
        (
            "crates/ao2-cli/src/workbench_release_latest.rs",
            &["query_value_owned"][..],
        ),
        (
            "crates/ao2-cli/src/workbench_run_evidence.rs",
            &["query_value_owned"][..],
        ),
        (
            "crates/ao2-cli/src/workbench_server.rs",
            &[
                "query_value_owned",
                "form_value_owned",
                "http_html_response",
                "http_json_response",
                "http_text_response",
                "generate_api_token",
            ][..],
        ),
    ] {
        let source =
            fs::read_to_string(root.join(relative_path)).expect("helper consumer source exists");
        let direct_imports = source
            .match_indices("use crate::cli_util::")
            .filter_map(|(start, _)| {
                source[start..]
                    .find(';')
                    .map(|end| &source[start..=start + end])
            })
            .collect::<Vec<_>>()
            .join("\n");
        for helper in required_helpers {
            assert!(
                direct_imports.contains(helper),
                "{relative_path} must import {helper} directly from cli_util"
            );
        }
    }
    let root_cli_util_import_start = main_source
        .find("use cli_util::{")
        .expect("root cli_util import exists");
    let root_cli_util_import_end = main_source[root_cli_util_import_start..]
        .find("};")
        .expect("root cli_util import terminates");
    let root_cli_util_import = &main_source
        [root_cli_util_import_start..root_cli_util_import_start + root_cli_util_import_end];
    for helper in [
        "query_value_owned",
        "form_value_owned",
        "shell_quote",
        "format_budget_usd",
        "http_html_response",
        "http_json_response",
        "http_text_response",
        "percent_decode",
        "percent_encode",
        "generate_api_token",
        "resolve_api_token",
    ] {
        assert!(
            !root_cli_util_import.contains(helper),
            "{helper} must not be imported into the crate root"
        );
    }
    let workbench_queue_source =
        fs::read_to_string(root.join("crates/ao2-cli/src/workbench_queue.rs"))
            .expect("Workbench queue source exists");
    assert!(
        workbench_queue_source
            .contains("use crate::workbench_render::render_workbench_job_detail_page;"),
        "workbench_queue must import job-detail rendering directly from workbench_render"
    );
    {
        use sha2::{Digest, Sha256};

        let protocol_start = cli_util_source
            .find("pub(crate) fn query_value_owned(")
            .expect("protocol helper block exists");
        let encoding_start = cli_util_source
            .find("pub(crate) fn percent_decode(")
            .expect("encoding helper block exists");
        let encoding_end = cli_util_source[encoding_start..]
            .find("#[cfg(test)]")
            .map(|offset| encoding_start + offset)
            .expect("CLI utility tests follow production helpers");
        let protocol_block = format!(
            "{}\n",
            cli_util_source[protocol_start..encoding_start].trim_end()
        )
        .replace("pub(crate) fn ", "fn ");
        let encoding_block = format!(
            "{}\n",
            cli_util_source[encoding_start..encoding_end].trim_end()
        )
        .replace("pub(crate) fn ", "fn ");
        let render_start = workbench_render_source
            .find("pub(super) fn render_workbench_job_detail_page(")
            .expect("job-detail render block exists");
        let render_block = format!("{}\n", workbench_render_source[render_start..].trim_end())
            .replace("pub(super) fn ", "fn ");
        for (label, body, expected_sha256) in [
            (
                "protocol",
                protocol_block,
                "f15d90c928d0b0b47ef7aac1feec71d3c11e0feb62d3d2a72ba9cd5eda30f0cb",
            ),
            (
                "render",
                render_block,
                "a2907a145b66983541068f9a8e1aaea6f5e25e94305694fc1b71cc74b15a7de7",
            ),
            (
                "encoding",
                encoding_block,
                "dee2bd3b9e356adb2ee86f0870a689d5f936b7a7521448a39e621bbed376932d",
            ),
        ] {
            assert_eq!(
                format!("{:x}", Sha256::digest(body.as_bytes())),
                expected_sha256,
                "{label} helper bodies must remain byte-identical to the parent extraction"
            );
        }
    }
}

fn function_body_source<'a>(source: &'a str, function_name: &str) -> &'a str {
    let start = source
        .find(&format!("fn {function_name}"))
        .unwrap_or_else(|| panic!("{function_name} exists"));
    let tail = &source[start + 1..];
    let end = tail.find("\nfn ").unwrap_or(tail.len()) + 1;
    &source[start..start + end]
}
