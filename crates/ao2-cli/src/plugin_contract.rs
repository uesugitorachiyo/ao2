use anyhow::{Context, Result};

use super::cli_util::{canonical_json_sha256, json_u64};
use super::plugin_distribution::{
    validate_plugin_observer_trust_boundary, validate_plugin_provider_auth,
};
use super::{is_sha256_hex, json_bool, json_string};

pub(super) fn validate_plugin_consumer_lifecycle_observer_bundle_summary(
    summary: &serde_json::Value,
    actual_archive_sha256: &str,
) -> Result<()> {
    if json_string(summary, "schema_version")
        != "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1"
    {
        anyhow::bail!(
            "consumer lifecycle observer bundle requires ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1, got {}",
            json_string(summary, "schema_version")
        );
    }
    if json_string(summary, "status") != "ready_for_k37_observation" {
        anyhow::bail!(
            "consumer lifecycle observer bundle status must be ready_for_k37_observation"
        );
    }
    if json_string(summary, "producer") != "ao2" {
        anyhow::bail!("consumer lifecycle observer bundle producer must be ao2");
    }
    if summary.get("platforms") != Some(&serde_json::json!(["macos", "ubuntu", "windows"]))
        || json_u64(summary, "platform_count") != 3
    {
        anyhow::bail!("consumer lifecycle observer bundle must cover macos, ubuntu, and windows");
    }
    if summary.get("observed_evidence_scope")
        != Some(&serde_json::json!(["ao2.plugin-consumer-lifecycle.v1"]))
    {
        anyhow::bail!("consumer lifecycle observer bundle observed evidence scope is invalid");
    }
    if json_string(summary, "archive_sha256") != actual_archive_sha256 {
        anyhow::bail!(
            "consumer lifecycle observer bundle summary archive sha256 mismatch: summary {}, actual {}",
            json_string(summary, "archive_sha256"),
            actual_archive_sha256
        );
    }
    let platform_lifecycles = summary
        .get("platform_lifecycles")
        .context("consumer lifecycle observer bundle missing platform_lifecycles")?;
    if json_string(summary, "platform_lifecycles_sha256")
        != canonical_json_sha256(platform_lifecycles)
    {
        anyhow::bail!("consumer lifecycle observer bundle platform lifecycle digest mismatch");
    }
    validate_plugin_observer_trust_boundary(
        summary
            .get("trust_boundary")
            .context("consumer lifecycle observer bundle missing trust_boundary")?,
        "consumer lifecycle observer bundle",
    )?;
    validate_plugin_control_plane_observation(
        summary
            .get("control_plane_observation")
            .context("consumer lifecycle observer bundle missing control_plane_observation")?,
        "consumer lifecycle observer bundle",
    )?;
    validate_plugin_side_effects_false(
        summary
            .get("side_effects")
            .context("consumer lifecycle observer bundle missing side_effects")?,
        "consumer lifecycle observer bundle",
    )?;
    if !json_bool(summary, "token_safe_output_verified") {
        anyhow::bail!("consumer lifecycle observer bundle token_safe_output_verified must be true");
    }
    if json_string(summary, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("consumer lifecycle observer bundle factory_v3_role must be parity_auditor");
    }
    Ok(())
}

pub(super) fn validate_plugin_consumer_lifecycle_contract(
    lifecycle: &serde_json::Value,
    platform: &str,
) -> Result<()> {
    if json_string(lifecycle, "schema_version") != "ao2.plugin-consumer-lifecycle.v1" {
        anyhow::bail!(
            "{platform} consumer lifecycle requires ao2.plugin-consumer-lifecycle.v1, got {}",
            json_string(lifecycle, "schema_version")
        );
    }
    if json_string(lifecycle, "status") != "passed" {
        anyhow::bail!("{platform} consumer lifecycle status must be passed");
    }
    if lifecycle.get("targets") != Some(&serde_json::json!(["codex", "claude"])) {
        anyhow::bail!("{platform} consumer lifecycle must target codex and claude");
    }
    validate_plugin_provider_auth(
        lifecycle
            .get("provider_auth")
            .context("plugin consumer lifecycle missing provider_auth")?,
        &format!("{platform} consumer lifecycle"),
    )?;
    validate_plugin_observer_trust_boundary(
        lifecycle
            .get("trust_boundary")
            .context("plugin consumer lifecycle missing trust_boundary")?,
        &format!("{platform} consumer lifecycle"),
    )?;
    validate_plugin_control_plane_observation(
        lifecycle
            .get("control_plane_observation")
            .context("plugin consumer lifecycle missing control_plane_observation")?,
        &format!("{platform} consumer lifecycle"),
    )?;
    validate_plugin_side_effects_false(
        lifecycle
            .get("side_effects")
            .context("plugin consumer lifecycle missing side_effects")?,
        &format!("{platform} consumer lifecycle"),
    )?;
    if !json_bool(lifecycle, "token_safe_output_verified") {
        anyhow::bail!("{platform} consumer lifecycle token_safe_output_verified must be true");
    }
    if json_string(lifecycle, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("{platform} consumer lifecycle factory_v3_role must be parity_auditor");
    }

    let package = lifecycle
        .get("package")
        .context("plugin consumer lifecycle missing package")?;
    for field in ["summary_sha256", "archive_sha256"] {
        if !is_sha256_hex(&json_string(package, field)) {
            anyhow::bail!("{platform} consumer lifecycle package.{field} must be a digest");
        }
    }
    let adapter_scaffold = lifecycle
        .get("adapter_scaffold")
        .context("plugin consumer lifecycle missing adapter_scaffold")?;
    if !is_sha256_hex(&json_string(adapter_scaffold, "summary_sha256")) {
        anyhow::bail!(
            "{platform} consumer lifecycle adapter_scaffold.summary_sha256 must be a digest"
        );
    }

    let target_results = lifecycle
        .get("target_results")
        .and_then(serde_json::Value::as_object)
        .context("plugin consumer lifecycle missing target_results")?;
    for target in ["codex", "claude"] {
        let result = target_results
            .get(target)
            .with_context(|| format!("{platform} consumer lifecycle missing {target} result"))?;
        if json_string(result, "status") != "passed" {
            anyhow::bail!("{platform} consumer lifecycle {target} result must pass");
        }
        if !json_bool(result, "installed_package_paths_only") {
            anyhow::bail!(
                "{platform} consumer lifecycle {target} must use installed package paths only"
            );
        }
        for field in [
            "provider_execution_started",
            "queue_mutated",
            "memory_written",
            "control_plane_mutated",
            "ao_artifacts_mutated",
            "release_approved",
        ] {
            if json_bool(result, field) {
                anyhow::bail!(
                    "{platform} consumer lifecycle {target} side-effect field must be false: {field}"
                );
            }
        }
    }
    Ok(())
}

pub(super) fn validate_plugin_control_plane_fixture_handoff(
    handoff: &serde_json::Value,
) -> Result<()> {
    if json_string(handoff, "schema_version") != "ao2.control-plane-fixture-handoff.v1" {
        anyhow::bail!(
            "control-plane fixture handoff requires ao2.control-plane-fixture-handoff.v1, got {}",
            json_string(handoff, "schema_version")
        );
    }
    if json_string(handoff, "status") != "ready_for_control_plane_readback" {
        anyhow::bail!(
            "control-plane fixture handoff status must be ready_for_control_plane_readback"
        );
    }
    if json_string(handoff, "producer") != "ao2" {
        anyhow::bail!("control-plane fixture handoff producer must be ao2");
    }
    if json_string(handoff, "source_schema_version")
        != "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1"
        || json_string(handoff, "expected_schema_version")
            != "ao2.k37-plugin-consumer-lifecycle-observer-bundle.v1"
        || json_string(handoff, "expected_status") != "ready_for_k37_observation"
    {
        anyhow::bail!("control-plane fixture handoff source contract is invalid");
    }
    if handoff.get("expected_platforms") != Some(&serde_json::json!(["macos", "ubuntu", "windows"]))
        || json_u64(handoff, "expected_platform_count") != 3
    {
        anyhow::bail!("control-plane fixture handoff must expect macos, ubuntu, and windows");
    }
    if handoff.get("expected_observed_evidence_scope")
        != Some(&serde_json::json!(["ao2.plugin-consumer-lifecycle.v1"]))
    {
        anyhow::bail!("control-plane fixture handoff observed evidence scope is invalid");
    }
    if json_string(handoff, "recommended_control_plane_fixture_path")
        != "crates/ao2-cp-server/tests/fixtures/k37-plugin-observer/consumer-lifecycle-observer-bundle.json"
        || json_string(handoff, "recommended_control_plane_test_name")
            != "consumer_lifecycle_observer_bundle_is_read_only_three_platform_evidence"
    {
        anyhow::bail!("control-plane fixture handoff recommendation metadata is invalid");
    }
    validate_plugin_provider_auth(
        handoff
            .get("provider_auth")
            .context("control-plane fixture handoff missing provider_auth")?,
        "control-plane fixture handoff",
    )?;
    validate_plugin_observer_trust_boundary(
        handoff
            .get("trust_boundary")
            .context("control-plane fixture handoff missing trust_boundary")?,
        "control-plane fixture handoff",
    )?;
    validate_plugin_control_plane_observation(
        handoff
            .get("control_plane_observation")
            .context("control-plane fixture handoff missing control_plane_observation")?,
        "control-plane fixture handoff",
    )?;
    let side_effects = handoff
        .get("side_effects")
        .context("control-plane fixture handoff missing side_effects")?;
    for field in [
        "would_execute_provider",
        "would_execute_queue",
        "would_write_memory",
        "would_mutate_control_plane",
        "would_mutate_ao_artifacts",
        "would_approve_release",
    ] {
        if json_bool(side_effects, field) {
            anyhow::bail!(
                "control-plane fixture handoff side_effects field must be false: {field}"
            );
        }
    }
    if !json_bool(handoff, "token_safe_output_verified") {
        anyhow::bail!("control-plane fixture handoff token_safe_output_verified must be true");
    }
    if json_string(handoff, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("control-plane fixture handoff factory_v3_role must be parity_auditor");
    }
    Ok(())
}

pub(super) fn validate_plugin_adapter_scaffold_summary(summary: &serde_json::Value) -> Result<()> {
    if json_string(summary, "schema_version") != "ao2.plugin-adapter-scaffold.v1" {
        anyhow::bail!(
            "plugin adapter scaffold requires ao2.plugin-adapter-scaffold.v1, got {}",
            json_string(summary, "schema_version")
        );
    }
    if json_string(summary, "status") != "ready_for_local_oauth_wrapper_integration" {
        anyhow::bail!("plugin adapter scaffold must be ready_for_local_oauth_wrapper_integration");
    }
    if summary.get("targets") != Some(&serde_json::json!(["codex", "claude"])) {
        anyhow::bail!("plugin adapter scaffold must target codex and claude");
    }
    validate_plugin_provider_auth(
        summary
            .get("provider_auth")
            .context("plugin adapter scaffold missing provider_auth")?,
        "plugin adapter scaffold",
    )?;
    validate_plugin_observer_trust_boundary(
        summary
            .get("trust_boundary")
            .context("plugin adapter scaffold missing trust_boundary")?,
        "plugin adapter scaffold",
    )?;
    validate_plugin_control_plane_observation(
        summary
            .get("control_plane_observation")
            .context("plugin adapter scaffold missing control_plane_observation")?,
        "plugin adapter scaffold",
    )?;
    validate_plugin_side_effects_false(
        summary
            .get("side_effects")
            .context("plugin adapter scaffold missing side_effects")?,
        "plugin adapter scaffold",
    )?;
    let digest_gates = summary
        .get("digest_gates")
        .context("plugin adapter scaffold missing digest_gates")?;
    for field in [
        "package_summary_sha256_verified",
        "package_archive_sha256_verified",
        "k37_bundle_sha256_verified",
        "k37_archive_sha256_verified",
        "wrapper_inputs_must_be_sha256_pinned",
    ] {
        if !json_bool(digest_gates, field) {
            anyhow::bail!("plugin adapter scaffold digest gate is incomplete: {field}");
        }
    }
    for (object_name, fields) in [
        (
            "package",
            ["summary_sha256", "archive_sha256", "schema_version"],
        ),
        (
            "k37_observer_bundle",
            ["summary_sha256", "archive_sha256", "schema_version"],
        ),
    ] {
        let object = summary
            .get(object_name)
            .with_context(|| format!("plugin adapter scaffold missing {object_name}"))?;
        for field in fields {
            if field.ends_with("sha256") && !is_sha256_hex(&json_string(object, field)) {
                anyhow::bail!("plugin adapter scaffold {object_name}.{field} must be a digest");
            }
        }
    }
    if json_string(
        summary
            .get("package")
            .context("plugin adapter scaffold missing package")?,
        "schema_version",
    ) != "ao2.plugin-package.v1"
    {
        anyhow::bail!(
            "plugin adapter scaffold package schema_version must be ao2.plugin-package.v1"
        );
    }
    if json_string(
        summary
            .get("k37_observer_bundle")
            .context("plugin adapter scaffold missing k37_observer_bundle")?,
        "schema_version",
    ) != "ao2.k37-plugin-observer-bundle.v1"
    {
        anyhow::bail!(
            "plugin adapter scaffold K37 bundle schema_version must be ao2.k37-plugin-observer-bundle.v1"
        );
    }
    if !json_bool(summary, "token_safe_output_verified") {
        anyhow::bail!("plugin adapter scaffold token_safe_output_verified must be true");
    }
    if json_string(summary, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("plugin adapter scaffold factory_v3_role must be parity_auditor");
    }
    Ok(())
}

pub(super) fn validate_plugin_adapter_scaffold_verification(
    verification: &serde_json::Value,
    platform: &str,
) -> Result<()> {
    if json_string(verification, "schema_version") != "ao2.plugin-adapter-scaffold-verification.v1"
    {
        anyhow::bail!(
            "{platform} adapter verification requires ao2.plugin-adapter-scaffold-verification.v1, got {}",
            json_string(verification, "schema_version")
        );
    }
    if json_string(verification, "status") != "passed" {
        anyhow::bail!("{platform} adapter verification status must be passed");
    }
    if verification.get("targets") != Some(&serde_json::json!(["codex", "claude"])) {
        anyhow::bail!("{platform} adapter verification must target codex and claude");
    }
    if !is_sha256_hex(&json_string(verification, "summary_sha256")) {
        anyhow::bail!("{platform} adapter verification summary_sha256 must be a digest");
    }
    if !json_bool(verification, "adapter_files_verified")
        || !json_bool(verification, "digest_gates_verified")
        || !json_bool(verification, "token_safe_output_verified")
    {
        anyhow::bail!("{platform} adapter verification gates are incomplete");
    }
    validate_plugin_provider_auth(
        verification
            .get("provider_auth")
            .context("plugin adapter verification missing provider_auth")?,
        &format!("{platform} adapter verification"),
    )?;
    validate_plugin_observer_trust_boundary(
        verification
            .get("trust_boundary")
            .context("plugin adapter verification missing trust_boundary")?,
        &format!("{platform} adapter verification"),
    )?;
    validate_plugin_control_plane_observation(
        verification
            .get("control_plane_observation")
            .context("plugin adapter verification missing control_plane_observation")?,
        &format!("{platform} adapter verification"),
    )?;
    validate_plugin_side_effects_false(
        verification
            .get("side_effects")
            .context("plugin adapter verification missing side_effects")?,
        &format!("{platform} adapter verification"),
    )?;
    if json_string(verification, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("{platform} adapter verification factory_v3_role must be parity_auditor");
    }
    Ok(())
}

pub(super) fn validate_plugin_adapter_install_smoke_contract(
    smoke: &serde_json::Value,
) -> Result<()> {
    if json_string(smoke, "schema_version") != "ao2.plugin-adapter-install-smoke.v1" {
        anyhow::bail!(
            "plugin adapter install smoke requires ao2.plugin-adapter-install-smoke.v1, got {}",
            json_string(smoke, "schema_version")
        );
    }
    if json_string(smoke, "status") != "passed" {
        anyhow::bail!("plugin adapter install smoke status must be passed");
    }
    if smoke.get("targets") != Some(&serde_json::json!(["codex", "claude"])) {
        anyhow::bail!("plugin adapter install smoke must target codex and claude");
    }
    for field in [
        "adapter_files_verified",
        "digest_gates_verified",
        "command_surface_verified",
        "token_safe_output_verified",
    ] {
        if !json_bool(smoke, field) {
            anyhow::bail!("plugin adapter install smoke gate is incomplete: {field}");
        }
    }
    if !is_sha256_hex(&json_string(smoke, "summary_sha256")) {
        anyhow::bail!("plugin adapter install smoke summary_sha256 must be a digest");
    }
    validate_plugin_provider_auth(
        smoke
            .get("provider_auth")
            .context("plugin adapter install smoke missing provider_auth")?,
        "plugin adapter install smoke",
    )?;
    validate_plugin_observer_trust_boundary(
        smoke
            .get("trust_boundary")
            .context("plugin adapter install smoke missing trust_boundary")?,
        "plugin adapter install smoke",
    )?;
    validate_plugin_control_plane_observation(
        smoke
            .get("control_plane_observation")
            .context("plugin adapter install smoke missing control_plane_observation")?,
        "plugin adapter install smoke",
    )?;
    validate_plugin_side_effects_false(
        smoke
            .get("side_effects")
            .context("plugin adapter install smoke missing side_effects")?,
        "plugin adapter install smoke",
    )?;
    if json_string(smoke, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("plugin adapter install smoke factory_v3_role must be parity_auditor");
    }

    let target_results = smoke
        .get("target_results")
        .and_then(serde_json::Value::as_object)
        .context("plugin adapter install smoke missing target_results")?;
    for target in ["codex", "claude"] {
        let result = target_results
            .get(target)
            .with_context(|| format!("plugin adapter install smoke missing {target} result"))?;
        if json_string(result, "status") != "passed" {
            anyhow::bail!("plugin adapter install smoke {target} result must pass");
        }
        if json_string(result, "adapter_schema_version") != "ao2.plugin-adapter.v1" {
            anyhow::bail!("plugin adapter install smoke {target} result schema mismatch");
        }
        if !is_sha256_hex(&json_string(result, "adapter_sha256")) {
            anyhow::bail!("plugin adapter install smoke {target} adapter_sha256 must be a digest");
        }
    }
    Ok(())
}

pub(super) fn validate_plugin_adapter_install_smoke_verification(
    verification: &serde_json::Value,
    platform: &str,
) -> Result<()> {
    if json_string(verification, "schema_version")
        != "ao2.plugin-adapter-install-smoke-verification.v1"
    {
        anyhow::bail!(
            "{platform} adapter install-smoke verification requires ao2.plugin-adapter-install-smoke-verification.v1, got {}",
            json_string(verification, "schema_version")
        );
    }
    if json_string(verification, "status") != "passed" {
        anyhow::bail!("{platform} adapter install-smoke verification status must be passed");
    }
    if verification.get("targets") != Some(&serde_json::json!(["codex", "claude"])) {
        anyhow::bail!("{platform} adapter install-smoke verification must target codex and claude");
    }
    if json_string(verification, "adapter_install_smoke_schema_version")
        != "ao2.plugin-adapter-install-smoke.v1"
    {
        anyhow::bail!("{platform} adapter install-smoke verification input schema mismatch");
    }
    for field in [
        "adapter_files_verified",
        "digest_gates_verified",
        "command_surface_verified",
        "token_safe_output_verified",
    ] {
        if !json_bool(verification, field) {
            anyhow::bail!(
                "{platform} adapter install-smoke verification gate is incomplete: {field}"
            );
        }
    }
    if !is_sha256_hex(&json_string(verification, "smoke_sha256")) {
        anyhow::bail!(
            "{platform} adapter install-smoke verification smoke_sha256 must be a digest"
        );
    }
    validate_plugin_provider_auth(
        verification
            .get("provider_auth")
            .context("plugin adapter install-smoke verification missing provider_auth")?,
        &format!("{platform} adapter install-smoke verification"),
    )?;
    validate_plugin_observer_trust_boundary(
        verification
            .get("trust_boundary")
            .context("plugin adapter install-smoke verification missing trust_boundary")?,
        &format!("{platform} adapter install-smoke verification"),
    )?;
    validate_plugin_control_plane_observation(
        verification.get("control_plane_observation").context(
            "plugin adapter install-smoke verification missing control_plane_observation",
        )?,
        &format!("{platform} adapter install-smoke verification"),
    )?;
    validate_plugin_side_effects_false(
        verification
            .get("side_effects")
            .context("plugin adapter install-smoke verification missing side_effects")?,
        &format!("{platform} adapter install-smoke verification"),
    )?;
    if json_string(verification, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!(
            "{platform} adapter install-smoke verification factory_v3_role must be parity_auditor"
        );
    }
    Ok(())
}

pub(super) fn validate_plugin_adapter_file(
    adapter: &serde_json::Value,
    target: &str,
    summary: &serde_json::Value,
) -> Result<()> {
    if json_string(adapter, "schema_version") != "ao2.plugin-adapter.v1" {
        anyhow::bail!(
            "plugin adapter {target} requires ao2.plugin-adapter.v1, got {}",
            json_string(adapter, "schema_version")
        );
    }
    if json_string(adapter, "status") != "ready_for_local_oauth_wrapper_integration" {
        anyhow::bail!("plugin adapter {target} must be ready_for_local_oauth_wrapper_integration");
    }
    if json_string(adapter, "target") != target {
        anyhow::bail!("plugin adapter {target} target field mismatch");
    }
    validate_plugin_provider_auth(
        adapter
            .get("provider_auth")
            .with_context(|| format!("plugin adapter {target} missing provider_auth"))?,
        &format!("plugin adapter {target}"),
    )?;
    validate_plugin_observer_trust_boundary(
        adapter
            .get("trust_boundary")
            .with_context(|| format!("plugin adapter {target} missing trust_boundary"))?,
        &format!("plugin adapter {target}"),
    )?;
    validate_plugin_control_plane_observation(
        adapter.get("control_plane_observation").with_context(|| {
            format!("plugin adapter {target} missing control_plane_observation")
        })?,
        &format!("plugin adapter {target}"),
    )?;
    validate_plugin_side_effects_false(
        adapter
            .get("side_effects")
            .with_context(|| format!("plugin adapter {target} missing side_effects"))?,
        &format!("plugin adapter {target}"),
    )?;
    let digest_gates = adapter
        .get("digest_gates")
        .with_context(|| format!("plugin adapter {target} missing digest_gates"))?;
    for field in [
        "package_summary_sha256_verified",
        "package_archive_sha256_verified",
        "k37_bundle_sha256_verified",
        "k37_archive_sha256_verified",
        "wrapper_inputs_must_be_sha256_pinned",
    ] {
        if !json_bool(digest_gates, field) {
            anyhow::bail!("plugin adapter {target} digest gate is incomplete: {field}");
        }
    }
    let inputs = adapter
        .get("inputs")
        .with_context(|| format!("plugin adapter {target} missing inputs"))?;
    let package = summary
        .get("package")
        .context("plugin adapter scaffold missing package")?;
    let k37_bundle = summary
        .get("k37_observer_bundle")
        .context("plugin adapter scaffold missing k37_observer_bundle")?;
    for (input_field, expected) in [
        (
            "package_summary_sha256",
            json_string(package, "summary_sha256"),
        ),
        (
            "package_archive_sha256",
            json_string(package, "archive_sha256"),
        ),
        (
            "k37_bundle_sha256",
            json_string(k37_bundle, "summary_sha256"),
        ),
        (
            "k37_archive_sha256",
            json_string(k37_bundle, "archive_sha256"),
        ),
    ] {
        if json_string(inputs, input_field) != expected {
            anyhow::bail!("plugin adapter {target} {input_field} does not match scaffold summary");
        }
    }
    let commands = adapter
        .get("commands")
        .with_context(|| format!("plugin adapter {target} missing commands"))?;
    for command in [
        "readiness",
        "package_verify",
        "distribution_observer_bundle",
        "consumer_lifecycle_observer_bundle",
        "consumer_lifecycle_observer_bundle_verify",
        "control_plane_fixture_handoff",
        "control_plane_fixture_handoff_verify",
        "release_candidate",
        "release_candidate_verify",
        "release_candidate_windows_recovery",
        "release_candidate_windows_recovery_verify",
        "release_candidate_windows_transfer_bundle",
        "release_candidate_observer_bundle",
        "release_candidate_observer_bundle_verify",
        "release_candidate_control_plane_fixture_handoff",
        "release_candidate_control_plane_fixture_handoff_verify",
        "final_install_transcript",
        "final_install_transcript_observer_bundle",
        "closer_decision",
        "closer_decision_verify",
        "shipment_readiness",
        "adapter_install_smoke_verify",
        "adapter_install_smoke_observer_bundle",
        "wrapper_harness",
        "wrapper_harness_verify",
    ] {
        if json_string(commands, command).is_empty() {
            anyhow::bail!("plugin adapter {target} missing command {command}");
        }
    }
    let token_safe_output = adapter
        .get("token_safe_output")
        .with_context(|| format!("plugin adapter {target} missing token_safe_output"))?;
    if json_string(token_safe_output, "redaction_policy") != "paths_status_and_digests_only"
        || json_bool(token_safe_output, "bearer_tokens_serialized")
        || json_bool(token_safe_output, "cookies_serialized")
        || json_bool(token_safe_output, "private_keys_serialized")
    {
        anyhow::bail!("plugin adapter {target} token_safe_output is not safe");
    }
    if json_string(adapter, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("plugin adapter {target} factory_v3_role must be parity_auditor");
    }
    Ok(())
}

pub(super) fn validate_plugin_control_plane_observation(
    observation: &serde_json::Value,
    context: &str,
) -> Result<()> {
    if json_string(observation, "role") != "read_only_observer"
        || json_bool(observation, "may_mutate_evidence")
        || json_bool(observation, "may_approve_release")
    {
        anyhow::bail!("{context} control-plane observation is not read-only");
    }
    Ok(())
}

pub(super) fn validate_plugin_side_effects_false(
    side_effects: &serde_json::Value,
    context: &str,
) -> Result<()> {
    for field in [
        "provider_execution_started",
        "queue_mutated",
        "memory_written",
        "ao_artifacts_mutated",
        "control_plane_mutated",
        "release_approved",
    ] {
        if json_bool(side_effects, field) {
            anyhow::bail!("{context} side_effects field must be false: {field}");
        }
    }
    Ok(())
}

pub(super) fn validate_k37_plugin_observer_bundle(bundle: &serde_json::Value) -> Result<()> {
    if json_string(bundle, "schema_version") != "ao2.k37-plugin-observer-bundle.v1" {
        anyhow::bail!(
            "K37 observer bundle requires ao2.k37-plugin-observer-bundle.v1, got {}",
            json_string(bundle, "schema_version")
        );
    }
    if json_string(bundle, "status") != "ready_for_k37_observation" {
        anyhow::bail!("K37 observer bundle must be ready_for_k37_observation");
    }
    if json_string(bundle, "producer") != "ao2" {
        anyhow::bail!("K37 observer bundle producer must be ao2");
    }
    if json_u64(bundle, "platform_count") != 3 {
        anyhow::bail!("K37 observer bundle platform_count must be 3");
    }
    if bundle.get("platforms") != Some(&serde_json::json!(["macos", "ubuntu", "windows"])) {
        anyhow::bail!("K37 observer bundle platforms must be macos, ubuntu, windows");
    }
    let archive_sha256 = json_string(bundle, "archive_sha256");
    if archive_sha256.len() != 64 || !archive_sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        anyhow::bail!("K37 observer bundle archive_sha256 must be a sha256 hex digest");
    }
    validate_plugin_observer_trust_boundary(
        bundle
            .get("trust_boundary")
            .context("K37 observer bundle missing trust_boundary")?,
        "K37 observer bundle",
    )?;
    let observation = bundle
        .get("control_plane_observation")
        .context("K37 observer bundle missing control_plane_observation")?;
    if json_string(observation, "role") != "read_only_observer"
        || json_bool(observation, "may_mutate_evidence")
        || json_bool(observation, "may_approve_release")
    {
        anyhow::bail!("K37 observer bundle control-plane observation is not read-only");
    }
    let side_effects = bundle
        .get("side_effects")
        .context("K37 observer bundle missing side_effects")?;
    for field in [
        "would_execute_provider",
        "would_execute_queue",
        "would_write_memory",
        "would_mutate_control_plane",
        "would_mutate_ao_artifacts",
        "would_approve_release",
    ] {
        if json_bool(side_effects, field) {
            anyhow::bail!("K37 observer bundle side_effects field must be false: {field}");
        }
    }
    if !json_bool(bundle, "token_safe_output_verified") {
        anyhow::bail!("K37 observer bundle token_safe_output_verified must be true");
    }
    if json_string(bundle, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("K37 observer bundle factory_v3_role must be parity_auditor");
    }
    Ok(())
}

pub(super) fn validate_k37_plugin_observer_input(
    input: &serde_json::Value,
    platform: &str,
) -> Result<()> {
    if json_string(input, "schema_version") != "ao2.k37-plugin-observer-input.v1" {
        anyhow::bail!(
            "{platform} observer input requires ao2.k37-plugin-observer-input.v1, got {}",
            json_string(input, "schema_version")
        );
    }
    if json_string(input, "status") != "ready_for_k37_observation" {
        anyhow::bail!("{platform} observer input must be ready_for_k37_observation");
    }
    if json_string(input, "producer") != "ao2" {
        anyhow::bail!("{platform} observer input producer must be ao2");
    }
    for field in ["package_summary_sha256", "package_archive_sha256"] {
        let digest = json_string(input, field);
        if digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
            anyhow::bail!("{platform} observer input {field} must be a sha256 hex digest");
        }
    }
    validate_plugin_observer_trust_boundary(
        input
            .get("trust_boundary")
            .context("k37 plugin observer input missing trust_boundary")?,
        &format!("{platform} observer input"),
    )?;
    let observation = input
        .get("control_plane_observation")
        .context("k37 plugin observer input missing control_plane_observation")?;
    if json_string(observation, "role") != "read_only_observer"
        || json_bool(observation, "may_mutate_evidence")
        || json_bool(observation, "may_approve_release")
    {
        anyhow::bail!("{platform} observer input control-plane observation is not read-only");
    }
    if json_string(input, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("{platform} observer input factory_v3_role must be parity_auditor");
    }
    let target_results = input
        .get("target_results")
        .and_then(serde_json::Value::as_object)
        .context("k37 plugin observer input missing target_results")?;
    for target in ["codex", "claude"] {
        let result = target_results
            .get(target)
            .with_context(|| format!("{platform} observer input missing {target} result"))?;
        if json_string(result, "status") != "passed" {
            anyhow::bail!("{platform} observer input {target} result must be passed");
        }
    }
    Ok(())
}

pub(super) fn validate_plugin_distribution_rehearsal_summary(
    rehearsal: &serde_json::Value,
    platform: &str,
) -> Result<()> {
    if json_string(rehearsal, "schema_version") != "ao2.plugin-distribution-rehearsal.v1" {
        anyhow::bail!(
            "{platform} clean package rehearsal requires ao2.plugin-distribution-rehearsal.v1, got {}",
            json_string(rehearsal, "schema_version")
        );
    }
    if json_string(rehearsal, "status") != "passed" {
        anyhow::bail!("{platform} clean package rehearsal must be passed");
    }
    if !json_bool(rehearsal, "package_verified_before_install") {
        anyhow::bail!("{platform} clean package rehearsal must verify package before install");
    }
    for field in ["summary_sha256", "archive_sha256"] {
        let digest = json_string(rehearsal, field);
        if digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
            anyhow::bail!("{platform} clean package rehearsal {field} must be a sha256 hex digest");
        }
    }
    let observer_input = rehearsal
        .get("observer_input")
        .context("clean package rehearsal missing observer_input")?;
    let observer_input_sha256 = json_string(observer_input, "sha256");
    if observer_input_sha256.len() != 64
        || !observer_input_sha256
            .chars()
            .all(|ch| ch.is_ascii_hexdigit())
    {
        anyhow::bail!(
            "{platform} clean package rehearsal observer_input.sha256 must be a sha256 hex digest"
        );
    }
    if json_string(observer_input, "path").trim().is_empty() {
        anyhow::bail!("{platform} clean package rehearsal observer_input.path must be non-empty");
    }
    validate_plugin_provider_auth(
        rehearsal
            .get("provider_auth")
            .context("clean package rehearsal missing provider_auth")?,
        &format!("{platform} clean package rehearsal"),
    )?;
    validate_plugin_observer_trust_boundary(
        rehearsal
            .get("trust_boundary")
            .context("clean package rehearsal missing trust_boundary")?,
        &format!("{platform} clean package rehearsal"),
    )?;
    let observation = rehearsal
        .get("control_plane_observation")
        .context("clean package rehearsal missing control_plane_observation")?;
    if json_string(observation, "role") != "read_only_observer"
        || json_bool(observation, "may_mutate_evidence")
        || json_bool(observation, "may_approve_release")
    {
        anyhow::bail!(
            "{platform} clean package rehearsal control-plane observation is not read-only"
        );
    }
    if json_string(rehearsal, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!("{platform} clean package rehearsal factory_v3_role must be parity_auditor");
    }
    if !json_bool(rehearsal, "token_safe_output_verified") {
        anyhow::bail!("{platform} clean package rehearsal token_safe_output_verified must be true");
    }
    let targets = rehearsal
        .get("targets")
        .and_then(serde_json::Value::as_array)
        .context("clean package rehearsal missing targets")?;
    for target in ["codex", "claude"] {
        if !targets.iter().any(|value| value.as_str() == Some(target)) {
            anyhow::bail!("{platform} clean package rehearsal missing target {target}");
        }
    }
    let target_results = rehearsal
        .get("target_results")
        .and_then(serde_json::Value::as_object)
        .context("clean package rehearsal missing target_results")?;
    for target in ["codex", "claude"] {
        let result = target_results.get(target).with_context(|| {
            format!("{platform} clean package rehearsal missing {target} result")
        })?;
        if json_string(result, "status") != "passed" {
            anyhow::bail!("{platform} clean package rehearsal {target} result must be passed");
        }
    }
    Ok(())
}

pub(super) fn validate_plugin_packaged_replacement_hardening_proof(
    proof: &serde_json::Value,
    platform: &str,
) -> Result<()> {
    if json_string(proof, "schema_version") != "ao2.packaged-replacement-hardening.v1" {
        anyhow::bail!(
            "{platform} packaged replacement proof requires ao2.packaged-replacement-hardening.v1, got {}",
            json_string(proof, "schema_version")
        );
    }
    if json_string(proof, "status") != "passed" {
        anyhow::bail!("{platform} packaged replacement proof must be passed");
    }
    if json_string(proof, "platform") != platform {
        anyhow::bail!("{platform} packaged replacement proof platform mismatch");
    }
    let package = proof
        .get("package")
        .context("packaged replacement proof missing package")?;
    for field in ["summary_sha256", "archive_sha256", "package_verify_sha256"] {
        let digest = json_string(package, field);
        if !is_sha256_hex(&digest) {
            anyhow::bail!(
                "{platform} packaged replacement proof package.{field} must be a sha256 digest"
            );
        }
    }
    let replacement = proof
        .get("factory_replacement")
        .context("packaged replacement proof missing factory_replacement")?;
    for field in [
        "app_run_sha256",
        "app_run_bundle_sha256",
        "project_plan_sha256",
        "project_run_sha256",
        "release_review_package_sha256",
        "rubric_sha256",
        "project_acceptance_rubric_sha256",
    ] {
        let digest = json_string(replacement, field);
        if !is_sha256_hex(&digest) {
            anyhow::bail!(
                "{platform} packaged replacement proof factory_replacement.{field} must be a sha256 digest"
            );
        }
    }
    let Some(closer) = proof.get("closer_decision") else {
        anyhow::bail!("{platform} packaged replacement proof missing closer_decision");
    };
    if json_string(closer, "schema_version") != "ao2.factory-closer-decision.v1" {
        anyhow::bail!(
            "{platform} packaged replacement proof closer_decision.schema_version must be ao2.factory-closer-decision.v1"
        );
    }
    if json_string(closer, "verification_schema_version")
        != "ao2.factory-closer-decision-verification.v1"
    {
        anyhow::bail!(
            "{platform} packaged replacement proof closer_decision.verification_schema_version must be ao2.factory-closer-decision-verification.v1"
        );
    }
    for field in [
        "decision_sha256",
        "decision_verification_sha256",
        "rubric_sha256",
    ] {
        let digest = json_string(closer, field);
        if !is_sha256_hex(&digest) {
            anyhow::bail!(
                "{platform} packaged replacement proof closer_decision.{field} must be a sha256 digest"
            );
        }
    }
    if json_string(closer, "rubric_sha256") != json_string(replacement, "rubric_sha256") {
        anyhow::bail!(
            "{platform} packaged replacement proof closer_decision.rubric_sha256 must match factory_replacement.rubric_sha256"
        );
    }
    validate_plugin_provider_auth(
        proof
            .get("provider_auth")
            .context("packaged replacement proof missing provider_auth")?,
        &format!("{platform} packaged replacement proof"),
    )?;
    validate_plugin_observer_trust_boundary(
        proof
            .get("trust_boundary")
            .context("packaged replacement proof missing trust_boundary")?,
        &format!("{platform} packaged replacement proof"),
    )?;
    if let Some(observation) = proof.get("control_plane_observation") {
        validate_plugin_control_plane_observation(
            observation,
            &format!("{platform} packaged replacement proof"),
        )?;
    }
    validate_plugin_packaged_replacement_side_effects_false(
        proof
            .get("side_effects")
            .context("packaged replacement proof missing side_effects")?,
        &format!("{platform} packaged replacement proof"),
    )?;
    let token_safe_output = proof
        .get("token_safe_output")
        .context("packaged replacement proof missing token_safe_output")?;
    if json_bool(token_safe_output, "bearer_tokens_serialized")
        || json_bool(token_safe_output, "cookies_serialized")
        || json_bool(token_safe_output, "private_keys_serialized")
        || json_string(token_safe_output, "redaction_policy") != "paths_status_and_digests_only"
    {
        anyhow::bail!("{platform} packaged replacement proof token_safe_output is not safe");
    }
    Ok(())
}

pub(super) fn validate_plugin_packaged_replacement_observer_bundle_summary(
    summary: &serde_json::Value,
    archive_sha256: &str,
) -> Result<()> {
    if json_string(summary, "schema_version")
        != "ao2.k37-packaged-replacement-hardening-observer-bundle.v1"
    {
        anyhow::bail!(
            "packaged replacement observer bundle requires ao2.k37-packaged-replacement-hardening-observer-bundle.v1, got {}",
            json_string(summary, "schema_version")
        );
    }
    if json_string(summary, "status") != "ready_for_k37_observation" {
        anyhow::bail!(
            "packaged replacement observer bundle status must be ready_for_k37_observation"
        );
    }
    if json_string(summary, "producer") != "ao2" {
        anyhow::bail!("packaged replacement observer bundle producer must be ao2");
    }
    if json_string(summary, "archive_sha256") != archive_sha256 {
        anyhow::bail!("packaged replacement observer bundle archive sha256 does not match");
    }
    if json_u64(summary, "platform_count") != 3
        || summary.get("platforms") != Some(&serde_json::json!(["macos", "ubuntu", "windows"]))
    {
        anyhow::bail!("packaged replacement observer bundle must cover macos/ubuntu/windows");
    }
    if summary.get("observed_evidence_scope")
        != Some(&serde_json::json!([
            "ao2.packaged-replacement-hardening.v1",
            "ao2.factory-closer-decision.v1",
            "ao2.factory-closer-decision-verification.v1"
        ]))
    {
        anyhow::bail!("packaged replacement observer bundle observed evidence scope is invalid");
    }
    let platform_proofs = summary
        .get("platform_proofs")
        .context("packaged replacement observer bundle missing platform_proofs")?;
    if json_string(summary, "platform_proofs_sha256") != canonical_json_sha256(platform_proofs) {
        anyhow::bail!("packaged replacement observer bundle platform digest mismatch");
    }
    for platform in ["macos", "ubuntu", "windows"] {
        if platform_proofs.get(platform).is_none() {
            anyhow::bail!("packaged replacement observer bundle missing {platform}");
        }
    }
    validate_plugin_provider_auth(
        summary
            .get("provider_auth")
            .context("packaged replacement observer bundle missing provider_auth")?,
        "packaged replacement observer bundle",
    )?;
    validate_plugin_observer_trust_boundary(
        summary
            .get("trust_boundary")
            .context("packaged replacement observer bundle missing trust_boundary")?,
        "packaged replacement observer bundle",
    )?;
    validate_plugin_control_plane_observation(
        summary
            .get("control_plane_observation")
            .context("packaged replacement observer bundle missing control_plane_observation")?,
        "packaged replacement observer bundle",
    )?;
    validate_plugin_side_effects_false(
        summary
            .get("side_effects")
            .context("packaged replacement observer bundle missing side_effects")?,
        "packaged replacement observer bundle",
    )?;
    if !json_bool(summary, "token_safe_output_verified") {
        anyhow::bail!(
            "packaged replacement observer bundle token_safe_output_verified must be true"
        );
    }
    if json_string(summary, "factory_v3_role") != "parity_auditor" {
        anyhow::bail!(
            "packaged replacement observer bundle factory_v3_role must be parity_auditor"
        );
    }
    Ok(())
}

fn validate_plugin_packaged_replacement_side_effects_false(
    side_effects: &serde_json::Value,
    label: &str,
) -> Result<()> {
    for key in [
        "provider_execution",
        "queue_mutation",
        "memory_write",
        "control_plane_mutation",
        "ao_artifact_mutation",
        "release_approval",
    ] {
        if json_bool(side_effects, key) {
            anyhow::bail!("{label} side effect must be false: {key}");
        }
    }
    Ok(())
}
