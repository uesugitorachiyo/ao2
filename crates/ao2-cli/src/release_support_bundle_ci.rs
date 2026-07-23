pub(crate) fn release_support_bundle_ci_evidence_index() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.cp-ci-evidence-index.v1",
        "status": "indexed",
        "control_plane_role": "read-only-observer",
        "mutates_ao_artifacts": false,
        "mutates_observer_storage": false,
        "control_plane_approves_release": false,
        "auth": {
            "required": true,
            "scheme": "bearer",
            "credential_material_included": false,
            "credential_material_in_urls": false
        },
        "endpoints": {
            "html": "/api/v1/ci/evidence-index",
            "json": "/api/v1/ci/evidence-index.json"
        },
        "evidence_families": [
            release_support_bundle_ci_family(
                "risky-pr-golden-bridge-smoke",
                "ao2-control-plane-risky-pr-golden-bridge-<target>",
                &["ao2.cp-risky-pr-golden-bridge-smoke.v1"],
                &["risky-pr-golden-bridge-smoke (<target>)"],
                &["ao2-control-plane-risky-pr-golden-bridge-ubuntu-x86_64"],
                "summary.json carries schema/status and artifact digests",
            ),
            release_support_bundle_ci_family(
                "release-train-bridge-smoke",
                "ao2-control-plane-release-train-bridge-<target>",
                &[
                    "ao2.cp-release-train-bridge-smoke.v1",
                    "ao2.cp-release-train-readback.v1",
                    "ao2.public-release-train-drill.v1",
                ],
                &["release-train-bridge-smoke (<target>)"],
                &["ao2-control-plane-release-train-bridge-ubuntu-x86_64"],
                "summary.json carries schema/status and release-train readback captures",
            ),
            release_support_bundle_ci_family(
                "ingest-smoke",
                "ao2-control-plane-ingest-smoke-<target>",
                &["ao2.cp-ingest-smoke.v1"],
                &["ingest-smoke (<target>)"],
                &["ao2-control-plane-ingest-smoke-ubuntu-x86_64"],
                "summary.json carries schema/status and artifact digests",
            ),
            release_support_bundle_ci_family(
                "release-archive-smoke",
                "ao2-control-plane-smoke-<target>",
                &["ao2.cp-release-archive-smoke.v1"],
                &["release-archive-smoke (<target>)"],
                &["ao2-control-plane-smoke-ubuntu-x86_64"],
                "summary.json carries schema/status and artifact digests",
            ),
            release_support_bundle_ci_family(
                "backup-restore-drill",
                "ao2-control-plane-dr-restore",
                &["ao2.cp-dr-restore-drill.v1"],
                &["backup-restore-drill (<target>)"],
                &["ao2-control-plane-dr-restore"],
                "summary.json carries schema/status and artifact digests",
            ),
            release_support_bundle_ci_family(
                "stable-promotion-evidence-readback",
                "ao2-control-plane-ao2-stable-promotion-evidence-index-readback",
                &[
                    "ao2.cp-ao2-stable-promotion-evidence-index-readback.v1",
                    "ao2.cp-stable-promotion-evidence-readback.v1",
                    "ao2.stable-promotion-evidence-index.v1",
                ],
                &["AO2 stable promotion evidence index readback"],
                &["ao2-control-plane-ao2-stable-promotion-evidence-index-readback"],
                "summary.json carries schema/status plus stable promotion evidence readiness",
            ),
        ]
    })
}

fn release_support_bundle_ci_family(
    id: &str,
    artifact_name_pattern: &str,
    schema_versions: &[&str],
    job_names: &[&str],
    artifact_names: &[&str],
    digest_reference: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "artifact_name_pattern": artifact_name_pattern,
        "schema_versions": schema_versions,
        "operator_action": "download-ci-artifact",
        "ci_artifact_provenance": {
            "provider": "github-actions",
            "workflow_file": ".github/workflows/ci.yml",
            "workflow_name": "CI",
            "run_id_source": "github_actions_run_id",
            "run_url_template": "https://github.com/uesugitorachiyo/ao2-control-plane/actions/runs/<run_id>",
            "artifact_download_url_template": "https://github.com/uesugitorachiyo/ao2-control-plane/actions/runs/<run_id>/artifacts",
            "job_names": job_names,
            "artifact_names": artifact_names,
            "digest_reference": digest_reference,
            "token_free": true
        },
        "trust_boundary": {
            "read_only": true,
            "approves_release": false,
            "mutates_ao_artifacts": false
        }
    })
}
