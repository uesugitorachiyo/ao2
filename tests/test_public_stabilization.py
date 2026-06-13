import json
import os
import re
import shlex
import stat
import subprocess
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (REPO_ROOT / path).read_text(encoding="utf-8")


def test_ci_runs_on_public_push_and_pull_request_while_release_gates_stay_manual():
    ci = read(".github/workflows/ci.yml")
    assert re.search(r"(?m)^\s*workflow_dispatch:\s*$", ci)
    assert re.search(r"(?m)^\s*pull_request:\s*$", ci)
    assert re.search(r"(?m)^\s*push:\s*$", ci)
    assert re.search(r"(?m)^\s*branches:\s*\[\s*main\s*\]\s*$", ci)
    assert re.search(r"(?m)^concurrency:\s*$", ci)
    assert "permissions:" in ci
    assert "  actions: read" in ci
    assert "  contents: read" in ci
    assert "  actions: write" not in ci
    assert "  contents: write" not in ci

    for workflow in (
        ".github/workflows/release-gate.yml",
        ".github/workflows/public-release-build.yml",
    ):
        text = read(workflow)
        assert re.search(r"(?m)^\s*workflow_dispatch:\s*$", text)
        assert not re.search(r"(?m)^\s*pull_request:\s*$", text)
        assert not re.search(r"(?m)^\s*push:\s*$", text)


def test_ci_matrix_entries_do_not_repeat_top_level_keys():
    ci = read(".github/workflows/ci.yml")
    duplicates = []
    entry_name = None
    seen_keys = {}

    for line_number, line in enumerate(ci.splitlines(), start=1):
        first_key = re.match(r"^ {10}- ([A-Za-z_][A-Za-z0-9_-]*):", line)
        if first_key:
            entry_name = f"entry starting on line {line_number}"
            seen_keys = {first_key.group(1): line_number}
            continue

        if entry_name is None:
            continue

        if line and not line.startswith(" " * 10):
            entry_name = None
            seen_keys = {}
            continue

        key = re.match(r"^ {12}([A-Za-z_][A-Za-z0-9_-]*):", line)
        if not key:
            continue

        key_name = key.group(1)
        if key_name in seen_keys:
            duplicates.append(
                f"{entry_name} repeats {key_name!r} on lines "
                f"{seen_keys[key_name]} and {line_number}"
            )
        seen_keys[key_name] = line_number

    assert not duplicates


def test_ci_non_approval_shards_are_split_for_mac_and_windows():
    ci = read(".github/workflows/ci.yml")
    shard_tests = {
        "test-cli-contract-gate-signing": [
            "contract_gate_support_signing",
            "contract_obligation_gate_signing_survey",
            "contract_verify_obligation_gate_signing",
        ],
        "test-cli-factory-control": [
            "cp_release_snapshot",
            "factory_bridge",
            "factory_cancel_authority",
            "factory_cancel_transition",
        ],
        "test-cli-release-readiness": [
            "release_evaluator_decision",
            "release_gate_obligation_gate_signing",
            "release_handoff_checklist",
            "release_support_bundle_verification",
        ],
        "test-cli-release-packaging-sdd": [
            "release_packaging",
            "sdd_subcommand",
        ],
    }

    assert "phase: test-cli-non-approval" not in ci
    for os_name in ["macos-latest", "windows-latest"]:
        for phase, tests in shard_tests.items():
            assert f"os: {os_name}\n            phase: {phase}" in ci
            for test_name in tests:
                assert f"--test {test_name}" in ci


def test_ci_release_support_verifier_runs_on_ubuntu():
    ci = read(".github/workflows/ci.yml")

    assert "os: ubuntu-latest\n            phase: test-cli-release-support" in ci
    assert "--test release_support_bundle_verification" in ci


def test_ci_uploads_risky_pr_golden_release_support_bundle_artifacts():
    ci = read(".github/workflows/ci.yml")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        "risky-pr-golden-artifacts:",
        "name: Risky PR golden release support bundle artifacts",
        "AO2_RISKY_PR_GOLDEN_ROOT=target/risky-pr-golden-ci",
        "npm run risky-pr:golden",
        "target/risky-pr-golden-ci/summary.json",
        "target/risky-pr-golden-ci/release-support-bundle-build.json",
        "target/risky-pr-golden-ci/release-support-bundle/release-support-bundle.json",
        "target/risky-pr-golden-ci/release-support-bundle/SHA256SUMS",
        "ao2-risky-pr-golden-release-support-bundle",
    ]:
        assert needle in ci
    assert "Risky PR golden release support bundle artifacts" in verification
    assert "ao2-risky-pr-golden-release-support-bundle" in verification


def test_ci_python_guard_documents_task_board_full_loop_selector():
    ci = read(".github/workflows/ci.yml")
    guard = read("scripts/ci-python-guard-artifacts.sh")

    assert "test_pulse_task_board_full_loop_generate_execute_validate_regenerate" in ci
    assert "tests/test_public_stabilization.py" in guard
    assert "tests/test_phase1_promote_wrapper.py" in guard


def test_risky_pr_golden_upload_artifacts_have_stable_manifest():
    script = read("scripts/risky-pr-golden-path.sh")
    ci = read(".github/workflows/ci.yml")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        'ARTIFACT_MANIFEST="$OUT_ROOT/artifact-manifest.json"',
        "ao2.risky-pr-golden-artifact-manifest.v1",
        '"artifact_manifest"',
        '"artifact_count"',
        '"sha256"',
        '"summary.json"',
        '"report-verify.json"',
        '"release-support-bundle-build.json"',
        '"release-support-bundle/release-support-bundle.json"',
        '"release-support-bundle/SHA256SUMS"',
        '"cockpit/index.report.json"',
    ]:
        assert needle in script
    assert "target/risky-pr-golden-ci/artifact-manifest.json" in ci
    assert "ao2.risky-pr-golden-artifact-manifest.v1" in verification
    assert "artifact-manifest.json" in verification


def test_risky_pr_golden_control_plane_bridge_materializes_manifest_for_cp(tmp_path):
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["risky-pr:control-plane-bridge"]
        == "node scripts/run-sh-script.js scripts/risky-pr-golden-control-plane-bridge.sh"
    )

    script = REPO_ROOT / "scripts" / "risky-pr-golden-control-plane-bridge.sh"
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "AO2_CP_RISKY_PR_GOLDEN_ARTIFACT_MANIFEST",
        "ao2.risky-pr-golden-control-plane-bridge.v1",
        "ao2.risky-pr-golden-artifact-manifest.v1",
        "ao2.cp-risky-pr-golden-artifact-manifest-observer.v1",
        "/api/v1/risky-pr/golden/artifact-manifest",
        "/api/v1/risky-pr/golden/artifact-manifest.json",
        "control_plane_role",
        "read-only-observer",
        "credential_material_included",
        "credential_material_in_urls",
        "env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY",
    ]:
        assert needle in text

    artifact_root = tmp_path / "golden"
    bundle_dir = artifact_root / "release-support-bundle"
    bundle_dir.mkdir(parents=True)
    (artifact_root / "summary.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.risky-pr-golden-path.v1",
                "status": "passed",
                "run_id": "fixture-run",
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (artifact_root / "report-verify.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.report-contract-verification.v1",
                "status": "passed",
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (bundle_dir / "release-support-bundle.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.cp-release-support-bundle.v1",
                "status": "ready",
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    artifacts = []
    for relative_path in [
        "summary.json",
        "report-verify.json",
        "release-support-bundle/release-support-bundle.json",
    ]:
        path = artifact_root / relative_path
        artifacts.append(
            {
                "relative_path": relative_path,
                "path": relative_path,
                "size_bytes": path.stat().st_size,
                "sha256": __import__("hashlib").sha256(path.read_bytes()).hexdigest(),
                "schema_version": json.loads(path.read_text(encoding="utf-8")).get(
                    "schema_version"
                ),
            }
        )
    manifest = {
        "schema_version": "ao2.risky-pr-golden-artifact-manifest.v1",
        "status": "indexed",
        "run_id": "fixture-run",
        "artifact_root": ".",
        "artifact_count": len(artifacts),
        "artifacts": artifacts,
    }
    (artifact_root / "artifact-manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    out_root = tmp_path / "bridge"
    cp_root = tmp_path / "ao2-control-plane"
    env = os.environ.copy()
    env["AO2_RISKY_PR_CP_BRIDGE_ROOT"] = str(out_root)
    result = subprocess.run(
        [
            "npm",
            "run",
            "risky-pr:control-plane-bridge",
            "--",
            "--artifact-root",
            str(artifact_root),
            "--control-plane-root",
            str(cp_root),
        ],
        cwd=REPO_ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr + result.stdout

    stable_manifest = out_root / "latest" / "artifact-manifest.json"
    summary_path = out_root / "latest" / "summary.json"
    env_file = out_root / "latest" / "control-plane.env"
    cp_manifest = (
        cp_root
        / "target"
        / "risky-pr-golden-control-plane-bridge"
        / "artifact-manifest.json"
    )
    assert stable_manifest.is_file()
    assert cp_manifest.is_file()
    assert stable_manifest.read_text(encoding="utf-8") == cp_manifest.read_text(
        encoding="utf-8"
    )
    assert env_file.is_file()

    summary = json.loads(summary_path.read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.risky-pr-golden-control-plane-bridge.v1"
    assert summary["status"] == "passed"
    assert summary["manifest"]["schema_version"] == manifest["schema_version"]
    assert summary["manifest"]["artifact_count"] == len(artifacts)
    assert summary["control_plane"]["configured_env"] == (
        "AO2_CP_RISKY_PR_GOLDEN_ARTIFACT_MANIFEST"
    )
    assert summary["control_plane"]["stable_manifest"] == str(stable_manifest)
    assert summary["control_plane"]["mirror_manifest"] == str(cp_manifest)
    assert summary["control_plane"]["role"] == "read-only-observer"
    assert summary["control_plane"]["credential_material_included"] is False
    assert summary["control_plane"]["credential_material_in_urls"] is False
    assert summary["trust_boundary"]["local_only"] is True
    assert summary["trust_boundary"]["control_plane_approves_release"] is False
    assert summary["trust_boundary"]["mutates_ao2_artifacts"] is False
    assert "Bearer" not in summary_path.read_text(encoding="utf-8")
    assert (
        f"AO2_CP_RISKY_PR_GOLDEN_ARTIFACT_MANIFEST={stable_manifest}"
        in env_file.read_text(encoding="utf-8")
    )


def test_ci_reports_legacy_non_approval_required_check_names():
    ci = read(".github/workflows/ci.yml")

    assert "phase: test-cli-non-approval" not in ci
    assert "non_approval_required_check_compat:" in ci
    assert "needs: verify" in ci
    assert "if: ${{ always() }}" in ci
    assert "name: Verify ${{ matrix.os }} / test-cli-non-approval" in ci
    for os_name in ["macos-latest", "windows-latest"]:
        assert f"os: {os_name}" in ci
    assert 'Split non-approval shards did not pass: ${{ needs.verify.result }}' in ci
    assert "Split non-approval shards passed; reporting legacy required check name." in ci


def test_github_owned_actions_use_node24_runtime_majors():
    workflows = [
        ".github/workflows/ci.yml",
        ".github/workflows/release-gate.yml",
        ".github/workflows/public-release-build.yml",
        ".github/workflows/windows-release-smoke.yml",
    ]
    combined = "\n".join(read(workflow) for workflow in workflows)

    assert "uses: actions/checkout@v6.0.3" in combined
    assert "uses: actions/setup-node@v6.4.0" in combined
    assert "uses: actions/upload-artifact@v7.0.1" in combined
    assert "uses: docker/setup-qemu-action@v4.1.0" in combined

    stale_actions = [
        "actions/checkout@v4",
        "actions/setup-node@v4",
        "actions/upload-artifact@v4",
        "docker/setup-qemu-action@v3",
    ]
    for stale_action in stale_actions:
        assert stale_action not in combined


def test_public_release_publication_contract_matches_signed_sidecars_and_x86_artifact():
    public_release_build = read(".github/workflows/public-release-build.yml")
    publication_readiness = read("scripts/release-asset-publication-readiness.sh")
    publish_simulation = read("scripts/release-artifact-publish-simulation.sh")
    blocker_closure = read("scripts/release-publish-blocker-closure-drill.sh")
    combined_contracts = "\n".join(
        [publication_readiness, publish_simulation, blocker_closure]
    )

    assert "dist-linux-x86_64/*.tar.gz" in public_release_build
    assert "dist-provenance/*" in public_release_build
    for needle in [
        "ao2-release-provenance.json",
        "ao2-release-provenance.json.sig",
        "ao2-release-signing-public.pem",
    ]:
        assert needle in combined_contracts

    assert "provenance.json.signature" not in combined_contracts


def test_release_asset_publication_readiness_uses_local_artifact_fixture():
    publication_readiness = read("scripts/release-asset-publication-readiness.sh")
    public_ship_dry_run = read("scripts/public-ship-dry-run.sh")
    public_ship_rehearsal = read("scripts/public-ship-rehearsal.sh")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        "release-artifact-fixture",
        "ao2-python-guard",
        "ao2.python-guard-ci-artifacts.v1",
        "AO2_PUBLIC_RELEASE_TRAIN_FIXTURE_DIR",
        "AO2_PUBLIC_SHIP_DRY_RUN_FIXTURE_DIR",
    ]:
        assert needle in publication_readiness

    assert "AO2_PUBLIC_SHIP_DRY_RUN_FIXTURE_DIR" in public_ship_dry_run
    assert "AO2_PUBLIC_SHIP_REHEARSAL_FIXTURE_DIR" in public_ship_rehearsal
    assert "AO2_PUBLIC_RELEASE_TRAIN_FIXTURE_DIR" in public_ship_rehearsal
    assert "release-artifact-fixture" in verification


def test_public_agent_coordination_doc_exists_and_matches_agents_contract():
    agents = read("AGENTS.md")
    assert "docs/AGENT-COORDINATION.md" in agents
    coordination = read("docs/AGENT-COORDINATION.md")
    assert "public-safe" in coordination
    assert "reserve" in coordination
    assert "release" in coordination
    assert "Do not record secrets" in coordination
    assert "target/" in coordination


def test_public_ci_docs_do_not_claim_manual_only_private_ci():
    readme = read("README.md")
    verification = read("docs/VERIFICATION.md")
    combined = readme + "\n" + verification
    assert "manual-only templates" not in combined
    assert "manual `workflow_dispatch` only" not in combined
    assert "disabled manually at the GitHub repository level" not in combined
    assert "pull request" in readme.lower()
    assert "release-gate.yml" in readme


def test_public_release_links_and_install_guide_track_current_prerelease():
    readme = read("README.md")
    install = read("docs/INSTALL.md")

    for needle in [
        "https://github.com/uesugitorachiyo/ao2/releases/tag/v0.4.80",
        "https://github.com/uesugitorachiyo/ao2/releases/download/v0.4.80",
        "img.shields.io/github/v/release/uesugitorachiyo/ao2",
        "gh release download v0.4.80 --repo uesugitorachiyo/ao2",
        "ao2-0.4.80-macos-aarch64.tar.gz",
        "ao2-0.4.80-linux-x86_64.tar.gz",
        "ao2-0.4.80-windows-x86_64.tar.gz",
        "SHA256SUMS",
    ]:
        assert needle in readme

    assert "The current stable public release line is `v0.4.80`." in install
    assert "v0.4.79" not in install


def test_windows_release_smoke_verifies_public_archive_checksum():
    workflow = read(".github/workflows/windows-release-smoke.yml")

    for needle in [
        "gh release download v0.4.80",
        '--pattern "ao2-0.4.80-windows-x86_64.tar.gz"',
        '--pattern "SHA256SUMS"',
        "Get-FileHash -Algorithm SHA256",
        "ao2-0.4.80-windows-x86_64.tar.gz",
        "Archive checksum mismatch",
    ]:
        assert needle in workflow


def test_public_release_download_verify_is_checksum_first_and_post_merge_canaried():
    verifier = read("scripts/release-download-verify.sh")
    canary = read("scripts/post-merge-canary.sh")
    install = read("docs/INSTALL.md")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        "SHA256SUMS",
        "shasum -a 256 -c SHA256SUMS",
        "release_checksum_verify=passed",
        "release_provenance_verify=skipped_missing_public_key",
        "release_provenance_status",
    ]:
        assert needle in verifier

    for needle in [
        "release_download_verify",
        "npm run release:download-verify",
        "AO2_RELEASE_DOWNLOAD_DIR",
        "PULSE_SOURCE=\"$OUT_ROOT/pulse-source\"",
        "AO2_PULSE_LOCAL_MIRROR_SOURCE=\"$PULSE_SOURCE\"",
        "pulse-eval-loop.json",
        '"release_download_verify": str(out_root / "release-download" / "release-rollback-summary.json")',
        '"pulse_resume": str(out_root / "pulse-resume" / "summary.json")',
    ]:
        assert needle in canary

    for needle in [
        "verifies every\nasset listed in `SHA256SUMS`",
        "verifies signed\nprovenance",
        "public release download checksum verification",
        "stable public release archives at v0.4.80",
    ]:
        assert needle in install + "\n" + verification


def test_release_asset_completeness_gate_covers_ao2_and_control_plane():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:asset-completeness"]
        == "node scripts/run-sh-script.js scripts/release-asset-completeness.sh"
    )

    script = REPO_ROOT / "scripts" / "release-asset-completeness.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.release-asset-completeness.v1",
        "uesugitorachiyo/ao2",
        "uesugitorachiyo/ao2-control-plane",
        "v0.4.80",
        "v0.1.13",
        "ao2-0.4.80-linux-aarch64.tar.gz",
        "ao2-0.4.80-linux-x86_64.tar.gz",
        "ao2-0.4.80-macos-aarch64.tar.gz",
        "ao2-0.4.80-windows-x86_64.tar.gz",
        "ao2-release-provenance.json",
        "ao2-release-provenance.json.sig",
        "ao2-release-signing-public.pem",
        "ao2-release-readiness-summary.json",
        "ao2-control-plane-0.1.13-linux-x86_64.tar.gz",
        "ao2-control-plane-0.1.13-macos-aarch64.tar.gz",
        "ao2-control-plane-0.1.13-windows-x86_64.tar.gz",
        "summary.json",
        "SHA256SUMS",
        "missing_assets",
        "missing_checksum_entries",
        "stable_release_present",
        "release_channel",
        "release_name",
        "dashboard.html",
        "Stable release absent",
        "Prerelease present",
        "gh release view",
        "gh release download",
    ]:
        assert needle in text

    canary = read("scripts/post-merge-canary.sh")
    for needle in [
        "release_asset_completeness",
        "npm run release:asset-completeness",
        '"release_asset_completeness": str(out_root / "release-asset-completeness" / "summary.json")',
    ]:
        assert needle in canary

    verification = read("docs/VERIFICATION.md")
    assert "npm run release:asset-completeness" in verification
    assert "ao2.release-asset-completeness.v1" in verification
    assert "dashboard.html" in verification


def test_stable_release_readiness_reports_prerelease_blockers():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:stable-readiness"]
        == "node scripts/run-sh-script.js scripts/release-stable-readiness.sh"
    )

    script = REPO_ROOT / "scripts" / "release-stable-readiness.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")

    for needle in [
        "ao2.stable-release-readiness.v1",
        "release:asset-completeness",
        "stable_release_absent",
        "current_channel_is_prerelease",
        "signed_provenance_public_key_missing",
        "stable_release_label_mentions_alpha",
        "release_name",
        "stable_release_ready",
        "promotion_blockers",
        "dashboard.html",
        "Stable Release Readiness",
        "Not ready for stable release",
    ]:
        assert needle in text

    verification = read("docs/VERIFICATION.md")
    assert "npm run release:stable-readiness" in verification
    assert "ao2.stable-release-readiness.v1" in verification


def test_release_metadata_drift_audit_is_exposed_and_documented():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:metadata-drift-audit"]
        == "node scripts/run-sh-script.js scripts/release-metadata-drift-audit.sh"
    )

    script = REPO_ROOT / "scripts" / "release-metadata-drift-audit.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")

    for needle in [
        "ao2.release-metadata-drift-audit.v1",
        "uesugitorachiyo/ao2",
        "uesugitorachiyo/ao2-control-plane",
        "v0.4.80",
        "v0.1.13",
        "AO2 v0.4.80 stable",
        "ao2-control-plane v0.1.13",
        "docs/release/PUBLIC-RELEASE-VERIFICATION.md",
        "docs/INSTALL.md",
        "gh release view",
        "release_name_drift",
        "release_channel_drift",
        "doc_channel_drift",
        "mutates_releases",
        "stores_credentials",
    ]:
        assert needle in text

    verification = read("docs/VERIFICATION.md")
    assert "npm run release:metadata-drift-audit" in verification
    assert "ao2.release-metadata-drift-audit.v1" in verification

    public_release_index = read("docs/release/PUBLIC-RELEASE-VERIFICATION.md")
    assert "AO2 control-plane stable release: `v0.1.13`" in public_release_index
    assert "AO2 control-plane prerelease" not in public_release_index


def test_public_release_pair_digest_audit_rejects_closure_release_asset_drift(tmp_path):
    package_json = json.loads(read("package.json"))
    assert package_json["scripts"]["release:public-pair-digest-audit"] == (
        "node scripts/run-sh-script.js scripts/public-release-pair-digest-audit.sh"
    )

    script = REPO_ROOT / "scripts" / "public-release-pair-digest-audit.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")

    for needle in [
        "ao2.public-release-pair-digest-audit.v1",
        "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON",
        "AO2_PUBLIC_PAIR_DIGEST_AUDIT_AO2_RELEASE_VIEW_JSON",
        "AO2_PUBLIC_PAIR_DIGEST_AUDIT_CONTROL_PLANE_RELEASE_VIEW_JSON",
        "uesugitorachiyo/ao2",
        "uesugitorachiyo/ao2-control-plane",
        "gh release view",
        "dual_repo_closure_digest_match",
        "published_asset_digest_present",
        "published_asset_size_match",
        "mutates_releases",
        "stores_credentials",
    ]:
        assert needle in text

    verification = read("docs/VERIFICATION.md")
    assert "npm run release:public-pair-digest-audit" in verification
    assert "ao2.public-release-pair-digest-audit.v1" in verification

    closure_index = tmp_path / "dual-repo-closure-index.json"
    ao2_release = tmp_path / "ao2-release.json"
    control_plane_release = tmp_path / "control-plane-release.json"
    ao2_assets = [
        ("ao2-0.4.80-linux-aarch64.tar.gz", "c" * 64, 3345601),
        ("ao2-0.4.80-linux-x86_64.tar.gz", "d" * 64, 3345603),
        ("ao2-0.4.80-macos-aarch64.tar.gz", "e" * 64, 3345605),
        ("ao2-0.4.80-windows-x86_64.tar.gz", "f" * 64, 3345607),
    ]
    control_plane_assets = [
        ("ao2-control-plane-0.1.13-linux-x86_64.tar.gz", "b" * 64, 4236805),
        ("ao2-control-plane-0.1.13-macos-aarch64.tar.gz", "1" * 64, 4236807),
        ("ao2-control-plane-0.1.13-windows-x86_64.tar.gz", "2" * 64, 4236809),
    ]

    closure_index.write_text(
        json.dumps(
            {
                "schema_version": "ao2.dual-repo-release-publication-closure-index.v1",
                "status": "passed",
                "ao2": {
                    "schema_version": "ao2.release-publication-dry-run-closure.v1",
                    "archive_assets": [
                        {"name": name, "sha256": digest, "size_bytes": size}
                        for name, digest, size in ao2_assets
                    ],
                },
                "control_plane": {
                    "schema_version": "ao2.cp-release-publication-closure.v1",
                    "checksum_verified": True,
                    "archive_assets": [
                        {
                            "name": name,
                            "sha256": ("a" * 64 if name.endswith("linux-x86_64.tar.gz") else digest),
                            "size_bytes": size,
                        }
                        for name, digest, size in control_plane_assets
                    ],
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    ao2_release.write_text(
        json.dumps(
            {
                "tagName": "v0.4.80",
                "name": "AO2 v0.4.80 stable",
                "isPrerelease": False,
                "publishedAt": "2026-06-10T18:45:16Z",
                "url": "https://github.com/uesugitorachiyo/ao2/releases/tag/v0.4.80",
                "assets": [
                    {"name": name, "digest": "sha256:" + digest, "size": size}
                    for name, digest, size in ao2_assets
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    control_plane_release.write_text(
        json.dumps(
            {
                "tagName": "v0.1.13",
                "name": "ao2-control-plane v0.1.13",
                "isPrerelease": False,
                "publishedAt": "2026-06-12T05:53:59Z",
                "url": (
                    "https://github.com/uesugitorachiyo/ao2-control-plane/"
                    "releases/tag/v0.1.13"
                ),
                "assets": [
                    {"name": name, "digest": "sha256:" + digest, "size": size}
                    for name, digest, size in control_plane_assets
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    out_root = tmp_path / "audit"
    result = subprocess.run(
        ["npm", "run", "release:public-pair-digest-audit"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_ROOT": str(out_root),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON": str(
                closure_index
            ),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_AO2_RELEASE_VIEW_JSON": str(ao2_release),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_CONTROL_PLANE_RELEASE_VIEW_JSON": str(
                control_plane_release
            ),
        },
        check=False,
    )

    assert result.returncode != 0
    assert "status=failed" in result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.public-release-pair-digest-audit.v1"
    assert summary["status"] == "failed"
    failed = [item for item in summary["checks"] if item["status"] == "failed"]
    assert any(item["code"] == "dual_repo_closure_digest_match" for item in failed)
    assert summary["trust_boundary"]["mutates_releases"] is False

    closure_index.write_text(
        json.dumps(
                {
                    "schema_version": "ao2.dual-repo-release-publication-closure-index.v1",
                    "status": "passed",
                    "ao2": {
                        "schema_version": "ao2.release-publication-dry-run-closure.v1",
                        "archive_assets": [
                            {"name": name, "sha256": digest, "size_bytes": size}
                            for name, digest, size in ao2_assets
                        ],
                    },
                    "control_plane": {
                        "schema_version": "ao2.cp-release-publication-closure.v1",
                        "checksum_verified": True,
                        "archive_assets": [
                            {"name": name, "sha256": digest, "size_bytes": size}
                            for name, digest, size in control_plane_assets
                        ],
                    },
                },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    passed_root = tmp_path / "audit-passed"
    passed_result = subprocess.run(
        ["npm", "run", "release:public-pair-digest-audit"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_ROOT": str(passed_root),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON": str(
                closure_index
            ),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_AO2_RELEASE_VIEW_JSON": str(ao2_release),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_CONTROL_PLANE_RELEASE_VIEW_JSON": str(
                control_plane_release
            ),
        },
        check=False,
    )

    assert passed_result.returncode == 0, passed_result.stderr + passed_result.stdout
    assert "status=passed" in passed_result.stdout


def test_public_release_pair_digest_audit_rejects_missing_or_mismatched_full_archive_parity(tmp_path):
    script = read("scripts/public-release-pair-digest-audit.sh")
    for needle in [
        "required_archive_presence",
        "full_archive_parity",
        "required_archive_names",
        "closure_archive_assets",
        "ao2-0.4.80-linux-aarch64.tar.gz",
        "ao2-0.4.80-linux-x86_64.tar.gz",
        "ao2-0.4.80-macos-aarch64.tar.gz",
        "ao2-0.4.80-windows-x86_64.tar.gz",
        "ao2-control-plane-0.1.13-linux-x86_64.tar.gz",
        "ao2-control-plane-0.1.13-macos-aarch64.tar.gz",
        "ao2-control-plane-0.1.13-windows-x86_64.tar.gz",
    ]:
        assert needle in script

    ao2_archives = {
        "ao2-0.4.80-linux-aarch64.tar.gz": ("a" * 64, 101),
        "ao2-0.4.80-linux-x86_64.tar.gz": ("b" * 64, 102),
        "ao2-0.4.80-macos-aarch64.tar.gz": ("c" * 64, 103),
        "ao2-0.4.80-windows-x86_64.tar.gz": ("d" * 64, 104),
    }
    cp_archives = {
        "ao2-control-plane-0.1.13-linux-x86_64.tar.gz": ("e" * 64, 201),
        "ao2-control-plane-0.1.13-macos-aarch64.tar.gz": ("f" * 64, 202),
        "ao2-control-plane-0.1.13-windows-x86_64.tar.gz": ("1" * 64, 203),
    }

    def release_fixture(path: Path, component: str, archives: dict[str, tuple[str, int]]):
        path.write_text(
            json.dumps(
                {
                    "tagName": "v0.4.80" if component == "ao2" else "v0.1.13",
                    "name": (
                        "AO2 v0.4.80 stable"
                        if component == "ao2"
                        else "ao2-control-plane v0.1.13"
                    ),
                    "isPrerelease": False,
                    "publishedAt": "2026-06-12T00:00:00Z",
                    "url": f"https://github.com/uesugitorachiyo/{component}/releases",
                    "assets": [
                        {"name": name, "digest": f"sha256:{digest}", "size": size}
                        for name, (digest, size) in archives.items()
                    ],
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    def closure_asset_records(archives: dict[str, tuple[str, int]]):
        return [
            {"name": name, "sha256": digest, "size_bytes": size}
            for name, (digest, size) in archives.items()
        ]

    def write_closure(path: Path, ao2: dict[str, tuple[str, int]], cp: dict[str, tuple[str, int]]):
        path.write_text(
            json.dumps(
                {
                    "schema_version": "ao2.dual-repo-release-publication-closure-index.v1",
                    "status": "passed",
                    "ao2": {
                        "schema_version": "ao2.release-publication-dry-run-closure.v1",
                        "archive_assets": closure_asset_records(ao2),
                    },
                    "control_plane": {
                        "schema_version": "ao2.cp-release-publication-closure.v1",
                        "checksum_verified": True,
                        "archive_assets": closure_asset_records(cp),
                    },
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    ao2_release = tmp_path / "ao2-release.json"
    cp_release = tmp_path / "control-plane-release.json"
    release_fixture(ao2_release, "ao2", ao2_archives)
    release_fixture(cp_release, "ao2-control-plane", cp_archives)

    missing_closure = tmp_path / "missing-closure.json"
    incomplete_ao2 = {
        name: value
        for name, value in ao2_archives.items()
        if not name.endswith("windows-x86_64.tar.gz")
    }
    write_closure(missing_closure, incomplete_ao2, cp_archives)
    missing_result = subprocess.run(
        ["npm", "run", "release:public-pair-digest-audit"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_ROOT": str(tmp_path / "missing-audit"),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON": str(
                missing_closure
            ),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_AO2_RELEASE_VIEW_JSON": str(ao2_release),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_CONTROL_PLANE_RELEASE_VIEW_JSON": str(
                cp_release
            ),
        },
        check=False,
    )
    assert missing_result.returncode != 0
    missing_summary = json.loads(
        (tmp_path / "missing-audit" / "summary.json").read_text(encoding="utf-8")
    )
    missing_failed = [
        item for item in missing_summary["checks"] if item["status"] == "failed"
    ]
    assert any(
        item["component"] == "ao2"
        and item["code"] == "required_archive_presence"
        and "ao2-0.4.80-windows-x86_64.tar.gz" in item["missing_assets"]
        for item in missing_failed
    )

    drift_closure = tmp_path / "drift-closure.json"
    drift_cp = dict(cp_archives)
    drift_cp["ao2-control-plane-0.1.13-windows-x86_64.tar.gz"] = ("1" * 64, 999)
    write_closure(drift_closure, ao2_archives, drift_cp)
    drift_result = subprocess.run(
        ["npm", "run", "release:public-pair-digest-audit"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_ROOT": str(tmp_path / "drift-audit"),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON": str(
                drift_closure
            ),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_AO2_RELEASE_VIEW_JSON": str(ao2_release),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_CONTROL_PLANE_RELEASE_VIEW_JSON": str(
                cp_release
            ),
        },
        check=False,
    )
    assert drift_result.returncode != 0
    drift_summary = json.loads(
        (tmp_path / "drift-audit" / "summary.json").read_text(encoding="utf-8")
    )
    assert any(
        item["component"] == "ao2-control-plane"
        and item["code"] == "dual_repo_closure_size_match"
        and item["status"] == "failed"
        for item in drift_summary["checks"]
    )

    extra_public_archives = dict(cp_archives)
    extra_public_archives[
        "ao2-control-plane-0.1.13-linux-riscv64.tar.gz"
    ] = ("2" * 64, 204)
    release_fixture(cp_release, "ao2-control-plane", extra_public_archives)
    extra_public_closure = tmp_path / "extra-public-closure.json"
    write_closure(extra_public_closure, ao2_archives, cp_archives)
    extra_public_result = subprocess.run(
        ["npm", "run", "release:public-pair-digest-audit"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_ROOT": str(tmp_path / "extra-public-audit"),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON": str(
                extra_public_closure
            ),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_AO2_RELEASE_VIEW_JSON": str(ao2_release),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_CONTROL_PLANE_RELEASE_VIEW_JSON": str(
                cp_release
            ),
        },
        check=False,
    )
    assert extra_public_result.returncode != 0
    extra_public_summary = json.loads(
        (tmp_path / "extra-public-audit" / "summary.json").read_text(
            encoding="utf-8"
        )
    )
    extra_public_name = "ao2-control-plane-0.1.13-linux-riscv64.tar.gz"
    assert any(
        item["component"] == "ao2-control-plane"
        and item["code"] == "public_archive_closure_parity"
        and item["status"] == "failed"
        and extra_public_name in item["published_without_closure_assets"]
        for item in extra_public_summary["checks"]
    )
    assert (
        extra_public_summary["archive_parity"]["components"]["ao2-control-plane"][
            "published_without_closure_assets"
        ]
        == [extra_public_name]
    )

    release_fixture(cp_release, "ao2-control-plane", cp_archives)
    passed_closure = tmp_path / "passed-closure.json"
    write_closure(passed_closure, ao2_archives, cp_archives)
    passed_result = subprocess.run(
        ["npm", "run", "release:public-pair-digest-audit"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_ROOT": str(tmp_path / "passed-audit"),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON": str(
                passed_closure
            ),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_AO2_RELEASE_VIEW_JSON": str(ao2_release),
            "AO2_PUBLIC_PAIR_DIGEST_AUDIT_CONTROL_PLANE_RELEASE_VIEW_JSON": str(
                cp_release
            ),
        },
        check=False,
    )
    assert passed_result.returncode == 0, passed_result.stderr + passed_result.stdout
    passed_summary = json.loads(
        (tmp_path / "passed-audit" / "summary.json").read_text(encoding="utf-8")
    )
    assert passed_summary["status"] == "passed"
    assert passed_summary["archive_parity"]["status"] == "passed"
    assert passed_summary["archive_parity"]["components"]["ao2"]["required_archive_count"] == 4
    assert (
        passed_summary["archive_parity"]["components"]["ao2-control-plane"][
            "required_archive_count"
        ]
        == 3
    )


def test_post_release_pair_digest_audit_workflow_is_manual_and_read_only():
    workflow = read(".github/workflows/post-release-pair-digest-audit.yml")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        "name: Post Release Pair Digest Audit",
        "workflow_dispatch:",
        "permissions:",
        "  contents: read",
        "  actions: read",
        "uses: actions/checkout@v6.0.3",
        "uses: actions/setup-node@v6.4.0",
        'node-version: "22"',
        "EXPECTED_HEAD_SHA: ${{ github.sha }}",
        "gh run list --repo uesugitorachiyo/ao2 --branch main --workflow CI --status success",
        "--json databaseId,headSha",
        'select(.headSha == \\"$expected_head_sha\\")',
        "missing successful AO2 main CI run for head sha",
        'gh run download "$run_id" --repo uesugitorachiyo/ao2',
        "--name ao2-dual-repo-release-publication-closure-index",
        "AO2_PUBLIC_PAIR_DIGEST_AUDIT_ROOT=target/post-release-pair-digest-audit",
        "AO2_PUBLIC_PAIR_DIGEST_AUDIT_DUAL_REPO_CLOSURE_INDEX_JSON=target/post-release-pair-digest-audit-input/summary.json",
        "npm run release:public-pair-digest-audit",
        "ao2.public-release-pair-digest-audit.v1",
        "target/post-release-pair-digest-audit/summary.json",
        "archive_parity",
        "mutates_releases",
        "stores_credentials",
        "uses: actions/upload-artifact@v7.0.1",
        "name: ao2-public-release-pair-digest-audit",
    ]:
        assert needle in workflow

    for forbidden in [
        "pull_request:",
        "push:",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "gh release edit",
        "gh release upload",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in workflow

    assert "Post Release Pair Digest Audit" in verification
    assert "ao2-public-release-pair-digest-audit" in verification
    assert "target/post-release-pair-digest-audit-input/summary.json" in verification


def test_release_sync_provenance_assets_is_guarded_and_documented():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:sync-provenance-assets"]
        == "node scripts/run-sh-script.js scripts/release-sync-provenance-assets.sh"
    )

    script = REPO_ROOT / "scripts" / "release-sync-provenance-assets.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")

    for needle in [
        "ao2.release-sync-provenance-assets.v1",
        "AO2_RELEASE_SYNC_CONFIRM",
        "sync-$AO2_RELEASE_TAG",
        "dry_run",
        "gh release view",
        "gh release upload",
        "ao2-release-provenance.json",
        "ao2-release-provenance.json.sig",
        "ao2-release-signing-public.pem",
        "mutates_releases",
        "stores_credentials",
    ]:
        assert needle in text

    verification = read("docs/VERIFICATION.md")
    assert "npm run release:sync-provenance-assets" in verification
    assert "ao2.release-sync-provenance-assets.v1" in verification


def test_release_publication_dry_run_closure_composes_no_publish_gates():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:publication-dry-run-closure"]
        == "node scripts/run-sh-script.js scripts/release-publication-dry-run-closure.sh"
    )

    script = REPO_ROOT / "scripts" / "release-publication-dry-run-closure.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")

    for needle in [
        "ao2.release-publication-dry-run-closure.v1",
        "release:asset-publication-readiness",
        "release:sync-provenance-assets",
        "release:stable-readiness",
        "AO2_RELEASE_PUBLICATION_DRY_RUN_CLOSURE_ROOT",
        "AO2_RELEASE_SYNC_CONFIRM=",
        "AO2_STABLE_PROMOTION_CONFIRM=",
        "ao2.release-asset-publication-readiness.v1",
        "ao2.release-sync-provenance-assets.v1",
        "ao2.stable-release-readiness.v1",
        "publication_ready",
        "stable_release_ready",
        "upload_status",
        "dry_run",
        "mutates_releases",
        "stores_credentials",
        "release_publish",
        "not executed",
    ]:
        assert needle in text

    for forbidden in [
        "gh release upload",
        "gh release edit",
        "git push origin",
        "npm publish",
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
    ]:
        assert forbidden not in text

    verification = read("docs/VERIFICATION.md")
    assert "npm run release:publication-dry-run-closure" in verification
    assert "ao2.release-publication-dry-run-closure.v1" in verification
    assert "release publication dry-run closure" in verification


def test_stable_promotion_workflow_is_guarded_and_documented():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:stable-promotion-workflow"]
        == "node scripts/run-sh-script.js scripts/release-stable-promotion-workflow.sh"
    )

    script = REPO_ROOT / "scripts" / "release-stable-promotion-workflow.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")

    for needle in [
        "ao2.stable-promotion-workflow.v1",
        "release:stable-readiness",
        "AO2_STABLE_PROMOTION_CONFIRM",
        "promote-stable-v0.4.80-v0.1.13",
        "gh release edit",
        "--prerelease=false",
        "stable_channel_only",
        "mutates_releases",
        "uesugitorachiyo/ao2",
        "uesugitorachiyo/ao2-control-plane",
        "AO2_STABLE_PROMOTION_SKIP_EVIDENCE_DOWNLOAD",
        "AO2_STABLE_PROMOTION_EVIDENCE_ROOT",
        "AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR",
        "Post Stable Release Verification",
        "Post Release Pair Digest Audit",
        "Post Release Verification",
        "post-stable-release-smoke-Linux",
        "post-stable-release-smoke-macOS",
        "post-stable-release-smoke-Windows",
        "ao2-dual-public-release-smoke",
        "ao2.dual-public-release-smoke.v1",
        "ao2-public-release-pair-digest-audit",
        "ao2.public-release-pair-digest-audit.v1",
        "post-release-pair-digest-audit/summary.json",
        "target/post-release-pair-digest-audit/summary.json",
        "archive_parity_status",
        "task_board_readback_schema",
        "dual-public-release-smoke",
        "public-pair-digest-audit",
        "auth_value_stored",
        "credential_material_in_urls",
        "ao2-control-plane-post-release-verification-ubuntu",
        "ao2-control-plane-post-release-verification-macos",
        "ao2-control-plane-post-release-verification-windows",
        "gh run list --repo \"$repo\" --branch main --workflow \"$workflow\" --status success",
        "gh run download \"$run_id\" --repo \"$repo\" --name \"$artifact\" --dir \"$dest\"",
        "ao2.stable-promotion-evidence-gate.v1",
        "stable_promotion_evidence_gate",
        "post_release_evidence_ready",
        "post_release_evidence_missing",
        "checksum_verified",
        "credential_material_included",
        "signature_verified",
    ]:
        assert needle in text

    verification = read("docs/VERIFICATION.md")
    assert "npm run release:stable-promotion-workflow" in verification
    assert "ao2.stable-promotion-workflow.v1" in verification
    assert "ao2.stable-promotion-evidence-gate.v1" in verification
    assert "post-release verification evidence gate" in verification
    assert "ao2-dual-public-release-smoke" in verification
    assert "ao2-public-release-pair-digest-audit" in verification
    assert "archive_parity.status=passed" in verification
    assert "post-release-pair-digest-audit/summary.json" in verification
    assert "dual public task-board readback schemas" in verification

    public_release_index = read("docs/release/PUBLIC-RELEASE-VERIFICATION.md")
    assert "Stable promotion evidence gate" in public_release_index
    assert "AO2_STABLE_PROMOTION_SKIP_EVIDENCE_DOWNLOAD=1" in public_release_index
    assert "ao2-dual-public-release-smoke" in public_release_index
    assert "ao2-public-release-pair-digest-audit" in public_release_index
    assert "archive_parity.status=passed" in public_release_index
    assert "post-release-pair-digest-audit/summary.json" in public_release_index
    assert "control_plane_approves_release=false" in public_release_index


def test_stable_promotion_workflow_requires_public_pair_digest_audit(tmp_path):
    fixture = tmp_path / "fixture"
    for name in ["ao2-linux", "ao2-macos", "ao2-windows"]:
        install_update = fixture / name / "smoke" / "install-update.json"
        install_update.parent.mkdir(parents=True)
        install_update.write_text(
            json.dumps(
                {
                    "status": "installed",
                    "signature_verified": True,
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    dual_public = fixture / "dual-public-release-smoke" / "latest"
    (dual_public / "smoke").mkdir(parents=True)
    (dual_public / "summary.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.dual-public-release-smoke.v1",
                "status": "passed",
                "trust_boundary": {
                    "auth_value_stored": False,
                    "credential_material_in_urls": False,
                    "credential_material_included": False,
                    "mutates_github_releases": False,
                    "control_plane_approves_release": False,
                },
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (dual_public / "smoke" / "task-board-readback.json").write_text(
        json.dumps(
            {"schema_version": "ao2.cp-ai-task-board-readback.v1"},
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (dual_public / "smoke" / "task-board-dashboard.json").write_text(
        json.dumps(
            {"schema_version": "ao2.cp-ai-task-board-dashboard.v1"},
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    for name in ["control-plane-ubuntu", "control-plane-macos", "control-plane-windows"]:
        summary = fixture / name / "summary.json"
        summary.parent.mkdir(parents=True)
        summary.write_text(
            json.dumps(
                {
                    "schema_version": "ao2.cp-release-publication-closure.v1",
                    "status": "passed",
                    "checksum_verified": True,
                    "trust_boundary": {
                        "credential_material_included": False,
                        "mutates_github_releases": False,
                    },
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "npm").write_text(
        """#!/usr/bin/env bash
set -euo pipefail
if [ "$1 $2" != "run release:stable-readiness" ]; then
  echo "unexpected npm command: $*" >&2
  exit 1
fi
mkdir -p "$AO2_STABLE_RELEASE_READINESS_ROOT"
cat > "$AO2_STABLE_RELEASE_READINESS_ROOT/summary.json" <<'JSON'
{
  "schema_version": "ao2.stable-release-readiness.v1",
  "stable_release_ready": false,
  "components": [
    {"name": "ao2", "repo": "uesugitorachiyo/ao2", "tag": "v0.4.80"},
    {"name": "ao2-control-plane", "repo": "uesugitorachiyo/ao2-control-plane", "tag": "v0.1.13"}
  ],
  "promotion_blockers": [
    {"component": "ao2", "code": "stable_release_absent", "severity": "blocking"},
    {"component": "ao2-control-plane", "code": "current_channel_is_prerelease", "severity": "blocking"}
  ]
}
JSON
""",
        encoding="utf-8",
    )
    (bin_dir / "npm").chmod(0o755)
    (bin_dir / "gh").write_text(
        "#!/usr/bin/env bash\nexit 1\n",
        encoding="utf-8",
    )
    (bin_dir / "gh").chmod(0o755)

    out_root = tmp_path / "stable-promotion"
    result = subprocess.run(
        ["node", "scripts/run-sh-script.js", "scripts/release-stable-promotion-workflow.sh"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
            "AO2_STABLE_PROMOTION_ROOT": str(out_root),
            "AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR": str(fixture),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr

    evidence_summary = json.loads(
        (out_root / "post-release-verification-evidence" / "summary.json").read_text(
            encoding="utf-8"
        )
    )
    public_pair_check = next(
        check
        for check in evidence_summary["checks"]
        if check["artifact"] == "ao2-public-release-pair-digest-audit"
    )
    assert public_pair_check["status"] == "missing"
    assert public_pair_check["missing"] == [
        "post-release-pair-digest-audit/summary.json",
        "target/post-release-pair-digest-audit/summary.json",
    ]
    assert evidence_summary["post_release_evidence_ready"] is False

    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "blocked"
    assert summary["post_release_evidence_ready"] is False
    assert any(
        blocker["code"] == "post_release_evidence_missing"
        for blocker in summary["blockers"]
    )


def test_stable_promotion_accepts_downloaded_public_pair_digest_layout(tmp_path):
    fixture = tmp_path / "fixture"
    for name in ["ao2-linux", "ao2-macos", "ao2-windows"]:
        install_update = fixture / name / "smoke" / "install-update.json"
        install_update.parent.mkdir(parents=True)
        install_update.write_text(
            json.dumps(
                {
                    "status": "installed",
                    "signature_verified": True,
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    dual_public = fixture / "dual-public-release-smoke" / "latest"
    (dual_public / "smoke").mkdir(parents=True)
    (dual_public / "summary.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.dual-public-release-smoke.v1",
                "status": "passed",
                "trust_boundary": {
                    "auth_value_stored": False,
                    "credential_material_in_urls": False,
                    "credential_material_included": False,
                    "mutates_github_releases": False,
                    "control_plane_approves_release": False,
                },
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (dual_public / "smoke" / "task-board-readback.json").write_text(
        json.dumps(
            {"schema_version": "ao2.cp-ai-task-board-readback.v1"},
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (dual_public / "smoke" / "task-board-dashboard.json").write_text(
        json.dumps(
            {"schema_version": "ao2.cp-ai-task-board-dashboard.v1"},
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    public_pair_digest = (
        fixture / "public-pair-digest-audit" / "post-release-pair-digest-audit"
    )
    public_pair_digest.mkdir(parents=True)
    (public_pair_digest / "summary.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.public-release-pair-digest-audit.v1",
                "status": "passed",
                "archive_parity": {"status": "passed"},
                "trust_boundary": {
                    "mutates_releases": False,
                    "stores_credentials": False,
                },
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    for name in ["control-plane-ubuntu", "control-plane-macos", "control-plane-windows"]:
        summary = fixture / name / "summary.json"
        summary.parent.mkdir(parents=True)
        summary.write_text(
            json.dumps(
                {
                    "schema_version": "ao2.cp-release-publication-closure.v1",
                    "status": "passed",
                    "checksum_verified": True,
                    "trust_boundary": {
                        "credential_material_included": False,
                        "mutates_github_releases": False,
                    },
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "npm").write_text(
        """#!/usr/bin/env bash
set -euo pipefail
if [ "$1 $2" != "run release:stable-readiness" ]; then
  echo "unexpected npm command: $*" >&2
  exit 1
fi
mkdir -p "$AO2_STABLE_RELEASE_READINESS_ROOT"
cat > "$AO2_STABLE_RELEASE_READINESS_ROOT/summary.json" <<'JSON'
{
  "schema_version": "ao2.stable-release-readiness.v1",
  "stable_release_ready": false,
  "components": [
    {"name": "ao2", "repo": "uesugitorachiyo/ao2", "tag": "v0.4.80"},
    {"name": "ao2-control-plane", "repo": "uesugitorachiyo/ao2-control-plane", "tag": "v0.1.13"}
  ],
  "promotion_blockers": [
    {"component": "ao2", "code": "stable_release_absent", "severity": "blocking"},
    {"component": "ao2-control-plane", "code": "current_channel_is_prerelease", "severity": "blocking"}
  ]
}
JSON
""",
        encoding="utf-8",
    )
    (bin_dir / "npm").chmod(0o755)
    (bin_dir / "gh").write_text(
        "#!/usr/bin/env bash\nexit 1\n",
        encoding="utf-8",
    )
    (bin_dir / "gh").chmod(0o755)

    out_root = tmp_path / "stable-promotion"
    result = subprocess.run(
        ["node", "scripts/run-sh-script.js", "scripts/release-stable-promotion-workflow.sh"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
            "AO2_STABLE_PROMOTION_ROOT": str(out_root),
            "AO2_STABLE_PROMOTION_EVIDENCE_FIXTURE_DIR": str(fixture),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr

    evidence_summary = json.loads(
        (out_root / "post-release-verification-evidence" / "summary.json").read_text(
            encoding="utf-8"
        )
    )
    public_pair_check = next(
        check
        for check in evidence_summary["checks"]
        if check["artifact"] == "ao2-public-release-pair-digest-audit"
    )
    assert public_pair_check["status"] == "passed"
    assert public_pair_check["summary"].endswith(
        "public-pair-digest-audit/post-release-pair-digest-audit/summary.json"
    )
    assert evidence_summary["post_release_evidence_ready"] is True

    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["post_release_evidence_ready"] is True
    assert not any(
        blocker["code"] == "post_release_evidence_missing"
        for blocker in summary["blockers"]
    )


def test_operator_release_evidence_bundle_downloads_and_verifies_cross_repo_artifacts(tmp_path):
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:operator-evidence-bundle"]
        == "node scripts/run-sh-script.js scripts/operator-release-evidence-bundle.sh"
    )

    workflow = read(".github/workflows/operator-release-evidence-audit.yml")
    for needle in [
        'assert len(summary["checks"]) == 9',
        'check["artifact"] == "ao2-public-release-pair-digest-audit"',
        'public_pair_digest["schema_version"] == "ao2.public-release-pair-digest-audit.v1"',
        'public_pair_digest["archive_parity_status"] == "passed"',
        'public_pair_digest["mutates_releases"] is False',
        'public_pair_digest["stores_credentials"] is False',
    ]:
        assert needle in workflow

    script = REPO_ROOT / "scripts" / "operator-release-evidence-bundle.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")

    for needle in [
        "ao2.operator-release-evidence-bundle.v1",
        "AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR",
        "AO2_OPERATOR_RELEASE_EVIDENCE_ROOT",
        "gh run list --repo \"$repo\" --branch main --workflow \"$workflow\" --status success",
        "gh run download \"$run_id\" --repo \"$repo\" --name \"$artifact\" --dir \"$dest\"",
        "ao2-dual-repo-release-publication-closure-index",
        "post-stable-release-smoke-Linux",
        "post-stable-release-smoke-macOS",
        "post-stable-release-smoke-Windows",
        "ao2-dual-public-release-smoke",
        "ao2.dual-public-release-smoke.v1",
        "Post Release Pair Digest Audit",
        "ao2-public-release-pair-digest-audit",
        "ao2.public-release-pair-digest-audit.v1",
        "public-pair-digest-audit",
        "archive_parity",
        "task_board_readback_schema",
        "auth_value_stored",
        "credential_material_in_urls",
        "control_plane_approves_release",
        "ao2-control-plane-post-release-verification-ubuntu",
        "ao2-control-plane-post-release-verification-macos",
        "ao2-control-plane-post-release-verification-windows",
        "ao2.dual-repo-release-publication-closure-index.v1",
        "ao2.cp-release-publication-closure.v1",
        "signature_verified",
        "checksum_verified",
        "credential_material_included",
        "mutates_github_releases",
        "operator_release_evidence_ready",
    ]:
        assert needle in text

    fixture = tmp_path / "fixture"
    (fixture / "ao2-dual-repo-release-publication-closure-index").mkdir(parents=True)
    (fixture / "ao2-dual-repo-release-publication-closure-index" / "summary.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.dual-repo-release-publication-closure-index.v1",
                "status": "passed",
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    dual_public = fixture / "dual-public-release-smoke" / "latest"
    (dual_public / "smoke").mkdir(parents=True)
    (dual_public / "summary.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.dual-public-release-smoke.v1",
                "status": "passed",
                "trust_boundary": {
                    "auth_value_stored": False,
                    "credential_material_in_urls": False,
                    "credential_material_included": False,
                    "mutates_github_releases": False,
                    "control_plane_approves_release": False,
                },
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (dual_public / "smoke" / "task-board-readback.json").write_text(
        json.dumps(
            {"schema_version": "ao2.cp-ai-task-board-readback.v1"},
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    (dual_public / "smoke" / "task-board-dashboard.json").write_text(
        json.dumps(
            {"schema_version": "ao2.cp-ai-task-board-dashboard.v1"},
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    public_pair_digest = (
        fixture / "public-pair-digest-audit" / "post-release-pair-digest-audit"
    )
    public_pair_digest.mkdir(parents=True)
    (public_pair_digest / "summary.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.public-release-pair-digest-audit.v1",
                "status": "passed",
                "archive_parity": {"status": "passed"},
                "trust_boundary": {
                    "mutates_releases": False,
                    "stores_credentials": False,
                },
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    for name in ["ao2-linux", "ao2-macos", "ao2-windows"]:
        install_update = fixture / name / "smoke" / "install-update.json"
        install_update.parent.mkdir(parents=True)
        install_update.write_text(
            json.dumps(
                {
                    "status": "installed",
                    "signature_verified": True,
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
    for name in ["control-plane-ubuntu", "control-plane-macos", "control-plane-windows"]:
        summary = fixture / name / "summary.json"
        summary.parent.mkdir(parents=True)
        summary.write_text(
            json.dumps(
                {
                    "schema_version": "ao2.cp-release-publication-closure.v1",
                    "status": "passed",
                    "checksum_verified": True,
                    "trust_boundary": {
                        "credential_material_included": False,
                        "mutates_github_releases": False,
                    },
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

    out_root = tmp_path / "bundle"
    result = subprocess.run(
        ["npm", "run", "release:operator-evidence-bundle"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_OPERATOR_RELEASE_EVIDENCE_ROOT": str(out_root),
            "AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR": str(fixture),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.operator-release-evidence-bundle.v1"
    assert summary["status"] == "passed"
    assert summary["operator_release_evidence_ready"] is True
    assert summary["trust_boundary"]["downloads_github_actions_artifacts"] is False
    assert summary["trust_boundary"]["mutates_releases"] is False
    assert summary["trust_boundary"]["stores_credentials"] is False
    dual_public_check = next(
        check
        for check in summary["checks"]
        if check["artifact"] == "ao2-dual-public-release-smoke"
    )
    assert dual_public_check["status"] == "passed"
    assert dual_public_check["task_board_readback_schema"] == "ao2.cp-ai-task-board-readback.v1"
    assert dual_public_check["task_board_dashboard_schema"] == "ao2.cp-ai-task-board-dashboard.v1"
    assert dual_public_check["auth_value_stored"] is False
    assert dual_public_check["credential_material_in_urls"] is False
    assert dual_public_check["control_plane_approves_release"] is False
    public_pair_digest_check = next(
        check
        for check in summary["checks"]
        if check["artifact"] == "ao2-public-release-pair-digest-audit"
    )
    assert public_pair_digest_check["status"] == "passed"
    assert (
        public_pair_digest_check["schema_version"]
        == "ao2.public-release-pair-digest-audit.v1"
    )
    assert public_pair_digest_check["summary_status"] == "passed"
    assert public_pair_digest_check["archive_parity_status"] == "passed"
    assert public_pair_digest_check["mutates_releases"] is False
    assert public_pair_digest_check["stores_credentials"] is False
    assert public_pair_digest_check["summary"].endswith(
        "public-pair-digest-audit/post-release-pair-digest-audit/summary.json"
    )

    verification = read("docs/VERIFICATION.md")
    assert "npm run release:operator-evidence-bundle" in verification
    assert "ao2.operator-release-evidence-bundle.v1" in verification
    assert "ao2-public-release-pair-digest-audit" in verification

    public_release_index = read("docs/release/PUBLIC-RELEASE-VERIFICATION.md")
    assert "Operator release evidence bundle" in public_release_index
    assert "release:operator-evidence-bundle" in public_release_index
    assert "ao2.public-release-pair-digest-audit.v1" in public_release_index


def test_stable_release_evidence_packet_combines_release_and_operator_baselines(tmp_path):
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:stable-evidence-packet"]
        == "node scripts/run-sh-script.js scripts/stable-release-evidence-packet.sh"
    )

    script = REPO_ROOT / "scripts" / "stable-release-evidence-packet.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.stable-release-evidence-packet.v1",
        "AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT",
        "AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY",
        "AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY",
        "ao2.stable-promotion-workflow.v1",
        "ao2.operator-release-evidence-bundle.v1",
        "post_release_evidence_ready",
        "operator_release_evidence_ready",
        "stable_release_evidence_ready",
        "dashboard.html",
        "mutates_releases",
        "stores_credentials",
    ]:
        assert needle in text

    stable_summary = tmp_path / "stable-promotion" / "summary.json"
    stable_summary.parent.mkdir(parents=True)
    stable_summary.write_text(
        json.dumps(
            {
                "schema_version": "ao2.stable-promotion-workflow.v1",
                "status": "already_stable",
                "post_release_evidence_ready": True,
                "evidence_gate_status": "passed",
                "blockers": [],
                "components": [
                    {"name": "ao2", "repo": "uesugitorachiyo/ao2", "tag": "v0.4.80"},
                    {
                        "name": "ao2-control-plane",
                        "repo": "uesugitorachiyo/ao2-control-plane",
                        "tag": "v0.1.13",
                    },
                ],
                "trust_boundary": {
                    "mutates_releases": False,
                    "stores_credentials": False,
                },
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    operator_summary = tmp_path / "operator-bundle" / "summary.json"
    operator_summary.parent.mkdir(parents=True)
    operator_summary.write_text(
        json.dumps(
            {
                "schema_version": "ao2.operator-release-evidence-bundle.v1",
                "status": "passed",
                "operator_release_evidence_ready": True,
                "checks": [
                    {
                        "component": "ao2",
                        "platform": "linux",
                        "artifact": "post-stable-release-smoke-Linux",
                        "status": "passed",
                    },
                    {
                        "component": "ao2-control-plane",
                        "platform": "windows",
                        "artifact": "ao2-control-plane-post-release-verification-windows",
                        "status": "passed",
                    },
                ],
                "trust_boundary": {
                    "mutates_releases": False,
                    "stores_credentials": False,
                },
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    out_root = tmp_path / "packet"
    result = subprocess.run(
        ["npm", "run", "release:stable-evidence-packet"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_STABLE_RELEASE_EVIDENCE_PACKET_ROOT": str(out_root),
            "AO2_STABLE_RELEASE_EVIDENCE_PACKET_STABLE_SUMMARY": str(stable_summary),
            "AO2_STABLE_RELEASE_EVIDENCE_PACKET_OPERATOR_SUMMARY": str(operator_summary),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr

    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.stable-release-evidence-packet.v1"
    assert summary["status"] == "passed"
    assert summary["stable_release_evidence_ready"] is True
    assert summary["stable_promotion"]["status"] == "already_stable"
    assert summary["stable_promotion"]["post_release_evidence_ready"] is True
    assert summary["stable_promotion"]["evidence_gate_status"] == "passed"
    assert summary["operator_evidence"]["status"] == "passed"
    assert summary["operator_evidence"]["operator_release_evidence_ready"] is True
    assert summary["operator_evidence"]["check_count"] == 2
    assert summary["operator_evidence"]["passed_check_count"] == 2
    assert summary["trust_boundary"]["mutates_releases"] is False
    assert summary["trust_boundary"]["stores_credentials"] is False
    assert summary["sources"]["stable_promotion_summary"] == str(stable_summary)
    assert summary["sources"]["operator_evidence_summary"] == str(operator_summary)
    assert not summary["blockers"]

    dashboard = (out_root / "dashboard.html").read_text(encoding="utf-8")
    assert "Stable Release Evidence Packet" in dashboard
    assert "ao2.stable-release-evidence-packet.v1" in dashboard
    assert "post-stable-release-smoke-Linux" in dashboard
    assert "ao2-control-plane-post-release-verification-windows" in dashboard

    verification = read("docs/VERIFICATION.md")
    assert "npm run release:stable-evidence-packet" in verification
    assert "ao2.stable-release-evidence-packet.v1" in verification

    public_release_index = read("docs/release/PUBLIC-RELEASE-VERIFICATION.md")
    assert "Stable release evidence packet" in public_release_index
    assert "release:stable-evidence-packet" in public_release_index


def test_evidence_control_plane_smoke_script_is_token_safe_and_exposed_by_npm():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["smoke:evidence-control-plane"]
        == "node scripts/run-sh-script.js scripts/smoke-evidence-pack-control-plane.sh"
    )

    script = REPO_ROOT / "scripts" / "smoke-evidence-pack-control-plane.sh"
    mode = script.stat().st_mode
    assert mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    required = [
        "env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY",
        "AO2_CP_API_TOKEN",
        "ao2-cp-server",
        "ao2 run",
        "ao2 evidence publish",
        "--api-token-env",
        "/api/v1/evidence-pack/dashboard.json",
        "/api/v1/evidence-pack/run/",
        "token_leak_detected",
        "ao2.evidence-pack-control-plane-smoke.v1",
        "ao2.evidence-pack-control-plane-publish.v1",
        "ao2.cp-ingest-receipt.v1",
        "ao2.cp-evidence-pack-dashboard.v1",
        "ao2.cp-evidence-pack-detail.v1",
        "read_only_observer_for_signed_evidence",
        "can_approve_runs",
        "can_mutate_ao2_evidence",
    ]
    for needle in required:
        assert needle in text
    assert "$AO2_CP_API_TOKEN\"" not in text


def test_risky_pr_golden_path_script_is_exposed_and_checks_uat_surface():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["risky-pr:golden"]
        == "node scripts/run-sh-script.js scripts/risky-pr-golden-path.sh"
    )

    script = REPO_ROOT / "scripts" / "risky-pr-golden-path.sh"
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    required = [
        "env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY",
        "AO2_BIN_EXPLICIT=\"${AO2_BIN:-}\"",
        "explicit AO2_BIN is not executable",
        "--pause-for-approval",
        "approval_ticket_id",
        "approve \"$TICKET\"",
        "run --resume",
        "ao2.risky-pr-golden-path.v1",
        "policy_denial_observed",
        "exact_approval_observed",
        "evaluator_rejection_observed",
        "evaluator_acceptance_observed",
        "acceptance_evidence_observed",
        "Local Run Record",
        "Static Export Evidence",
        "Policy Decisions",
        "Evaluator Closure Evidence",
        "Closure Reports",
        "Replay Evidence",
        "Run Markers",
        "RELEASE_SUPPORT_INPUTS=\"$OUT_ROOT/release-support-inputs\"",
        "release support-bundle-build",
        "--report-target \"$TARGET\"",
        "--report-run-id \"$RUN_ID\"",
        "--install-verification \"$RELEASE_SUPPORT_INPUTS/install-verification.json\"",
        "ao2.install-verification-evidence.v1",
        "release-support-bundle.json",
        "ao2.release-support-bundle-build.v1",
        "ao2.cp-release-support-bundle.v1",
        "release_support_bundle_verification_status",
    ]
    for needle in required:
        assert needle in text
    assert "OPENAI_API_KEY=" not in text
    assert "ANTHROPIC_API_KEY=" not in text


def test_risky_pr_product_readiness_gate_reuses_one_golden_run_for_product_evidence():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    assert (
        package_json["scripts"]["risky-pr:product-readiness"]
        == "node scripts/run-sh-script.js scripts/risky-pr-product-readiness-gate.sh"
    )

    script = REPO_ROOT / "scripts" / "risky-pr-product-readiness-gate.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.risky-pr-product-readiness-gate.v1",
        "AO2_RISKY_PR_GOLDEN_ROOT=\"$RISKY_ROOT\"",
        "npm run risky-pr:golden",
        "ao2.risky-pr-golden-path.v1",
        "ao2.evidence-pack.v1",
        "local_run_record",
        "static_report_export",
        "evaluator_closure_evidence",
        "manual_filesystem_archaeology_required",
        "stores_credentials",
    ]:
        assert needle in text
    assert text.count("npm run risky-pr:golden") == 1
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run risky-pr:product-readiness",
        "ao2.risky-pr-product-readiness-gate.v1",
        "local run record",
        "static report/export",
        "evaluator closure",
        "Local Run Record",
        "Evaluator Closure Evidence",
        "Replay Evidence",
    ]:
        assert needle in verification


def test_release_readiness_script_is_local_only_and_checks_repo_guardrails():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:readiness"]
        == "node scripts/run-sh-script.js scripts/release-readiness.sh"
    )
    assert (
        package_json["scripts"]["release:readiness:static"]
        == "node scripts/run-sh-script.js scripts/release-readiness.sh --static-only"
    )
    assert (
        package_json["scripts"]["release:readiness:full"]
        == "node scripts/run-sh-script.js scripts/release-readiness.sh --full"
    )

    script = REPO_ROOT / "scripts" / "release-readiness.sh"
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    required = [
        "ao2.release-readiness-local.v1",
        "branch_protection",
        "latest_main_ci",
        "risky-pr:golden",
        "smoke:evidence-control-plane",
        "verify:no-factory-v3",
        "release-gate.yml",
        "public-release-build.yml",
        "report.md",
        "report.html",
        "uesugitorachiyo/ao2",
        "uesugitorachiyo/ao2-control-plane",
        "release:metadata-drift-audit",
        "ao2.release-metadata-drift-audit.v1",
    ]
    for needle in required:
        assert needle in text
    assert "cat target/long-lived-control-plane/api-token" not in text


def test_release_readiness_static_gate_locks_cross_os_ci_contract(tmp_path):
    script = read("scripts/release-readiness.sh")
    for needle in [
        'required_ci_os = ["ubuntu-latest", "macos-latest", "windows-latest"]',
        'add_job_matrix_os_check("verify", required_ci_os)',
        'add_job_matrix_os_check("release-archive-hosted-smoke", required_ci_os)',
        'add_job_matrix_os_check("workbench-operator-packet-control-plane-smoke", required_ci_os)',
        'add_job_matrix_os_check("non_approval_required_check_compat", ["macos-latest", "windows-latest"])',
        "ci_workbench_operator_packet_smoke_index_requires_all_os",
        "ci_release_readiness_static_artifact_job",
        "ci_release_readiness_artifact_consumer_job",
        "ci_ai_task_board_control_plane_bridge_artifact_job",
        "ci_pulse_task_board_closure_packet_artifact_job",
        "ci_pulse_codex_cron_event_loop_smoke_artifact_job",
        "ci_dual_repo_installed_release_smoke_artifact_job",
        "ci_release_publication_closure_artifact_job",
        "ci_dual_repo_release_publication_closure_index_job",
        "release_metadata_drift_audit",
        "release_metadata_drift_audit_summary",
        "release_metadata_drift_audit_status",
        "release_public_pair_digest_audit_contract",
        "post_release_pair_digest_audit_workflow",
        "Post Release Pair Digest Audit",
        "ao2-public-release-pair-digest-audit",
        "required_archive_names",
        "required_archive_presence",
        "closure_archive_assets",
        "full_archive_parity",
        "release:public-pair-digest-audit",
        "target/post-release-pair-digest-audit/summary.json",
        "ao2.public-release-pair-digest-audit.v1",
        "ao2-0.4.80-linux-aarch64.tar.gz",
        "ao2-0.4.80-linux-x86_64.tar.gz",
        "ao2-0.4.80-macos-aarch64.tar.gz",
        "ao2-0.4.80-windows-x86_64.tar.gz",
        "ao2-control-plane-0.1.13-linux-x86_64.tar.gz",
        "ao2-control-plane-0.1.13-macos-aarch64.tar.gz",
        "ao2-control-plane-0.1.13-windows-x86_64.tar.gz",
        "target/release-readiness-consumer/ao2-release-readiness",
        "target/release-readiness-consumer/ao2-release-train-control-plane-bridge",
        "target/release-readiness-consumer/ao2-ai-task-board-control-plane-bridge",
        "target/release-readiness-consumer/ao2-pulse-task-board-closure-packet",
        "target/release-readiness-consumer/ao2-pulse-codex-cron-event-loop-smoke",
        "target/release-readiness-consumer/ao2-dual-repo-installed-release-smoke",
        "target/release-readiness-consumer/ao2-release-publication-closure",
        "target/release-readiness-consumer/ao2-dual-repo-release-publication-closure-index",
        "ao2.release-readiness-local.v1",
        "ao2.release-train-control-plane-bridge.v1",
        "ao2.ai-task-board-control-plane-bridge.v1",
        "ao2.pulse-task-board-closure-packet.v1",
        "ao2.pulse-codex-cron-event-loop-smoke.v1",
        "codex-cron.event-loop-decision.v1",
        "ao2.dual-repo-installed-release-smoke.v1",
        "ao2.release-publication-dry-run-closure.v1",
        "ao2.cp-release-publication-closure.v1",
        "ao2.dual-repo-release-publication-closure-index.v1",
        "ao2.release-metadata-drift-audit.v1",
        "ao2.public-release-pair-digest-audit.v1",
        "ao2.release-artifact-closure-index.v1",
        "ao2-control-plane-",
        ".tar.gz",
        "sha256",
        "size_bytes",
        "artifact-closure-index.json",
        "release_readiness_artifact_consumer",
        "release_train_control_plane_bridge",
        "ai_task_board_control_plane_bridge",
        "pulse_task_board_closure_packet",
        "pulse_codex_cron_event_loop_smoke",
        "dual_repo_installed_release_smoke",
        "release_publication_closure",
        "dual_repo_release_publication_closure_index",
        "release_metadata_drift_audit",
        "release_public_pair_digest_audit",
    ]:
        assert needle in script

    ci = read(".github/workflows/ci.yml")
    for needle in [
        "release-publication-closure-artifacts:",
        "name: Release publication closure artifacts",
        "dual-repo-release-publication-closure-index:",
        "name: Dual-repo release publication closure index",
        "--name ao2-control-plane-release-publication-closure",
        "gh run list --repo uesugitorachiyo/ao2-control-plane --branch main --workflow CI",
        "gh run download \"$candidate_run_id\" --repo uesugitorachiyo/ao2-control-plane",
        "ao2.dual-repo-release-publication-closure-index.v1",
        "ao2.cp-release-publication-closure.v1",
        "Download AO2 public archive assets for closure index",
        "target/dual-repo-release-publication-closure-index/ao2-release-archives",
        "ao2_archive_assets",
        "hashlib.sha256(path.read_bytes()).hexdigest()",
        "ao2-0.4.80-linux-aarch64.tar.gz",
        "ao2-0.4.80-linux-x86_64.tar.gz",
        "ao2-0.4.80-macos-aarch64.tar.gz",
        "ao2-0.4.80-windows-x86_64.tar.gz",
        "ao2-control-plane-",
        ".tar.gz",
        "sha256",
        "size_bytes",
        "ao2-dual-repo-release-publication-closure-index",
        "uses: dtolnay/rust-toolchain@stable",
        "Download published provenance sidecars",
        "gh release download",
        "AO2_RELEASE_PROVENANCE_DIR=target/release-publication-provenance",
        "AO2_RELEASE_ASSET_PUBLICATION_READINESS_CI_SAFE=1",
        "pulse-task-board-closure-packet-artifacts:",
        "name: Pulse task-board closure packet artifacts",
        "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_ROOT=target/pulse-task-board-closure-packet-ci",
        "npm run pulse:task-board-closure-packet",
        "ao2.pulse-task-board-closure-packet.v1",
        "ao2-pulse-task-board-closure-packet",
        "pulse-codex-cron-event-loop-smoke-artifacts:",
        "name: Pulse codex-cron event-loop smoke artifacts",
        "ao2-pulse-codex-cron-event-loop-smoke",
        "release-readiness-artifact-consumer:",
        "name: Release readiness artifact consumer",
        "needs: [release-readiness-artifacts, release-train-control-plane-bridge-artifacts, ai-task-board-control-plane-bridge-artifacts, pulse-task-board-closure-packet-artifacts, pulse-codex-cron-event-loop-smoke-artifacts, dual-repo-installed-release-smoke-artifacts, release-publication-closure-artifacts, dual-repo-release-publication-closure-index]",
        "uses: actions/checkout@v6.0.3",
        "uses: actions/download-artifact@v8.0.1",
        "name: ao2-release-readiness",
        "path: target/release-readiness-consumer/ao2-release-readiness",
        "name: ao2-release-train-control-plane-bridge",
        "path: target/release-readiness-consumer/ao2-release-train-control-plane-bridge",
        "name: ao2-ai-task-board-control-plane-bridge",
        "path: target/release-readiness-consumer/ao2-ai-task-board-control-plane-bridge",
        "name: ao2-pulse-task-board-closure-packet",
        "path: target/release-readiness-consumer/ao2-pulse-task-board-closure-packet",
        "name: ao2-pulse-codex-cron-event-loop-smoke",
        "path: target/release-readiness-consumer/ao2-pulse-codex-cron-event-loop-smoke",
        "name: ao2-dual-repo-installed-release-smoke",
        "path: target/release-readiness-consumer/ao2-dual-repo-installed-release-smoke",
        "name: ao2-release-publication-closure",
        "path: target/release-readiness-consumer/ao2-release-publication-closure",
        "name: ao2-dual-repo-release-publication-closure-index",
        "path: target/release-readiness-consumer/ao2-dual-repo-release-publication-closure-index",
        "AO2_RELEASE_PUBLICATION_DRY_RUN_CLOSURE_ROOT=target/release-publication-closure-ci",
        "npm run release:publication-dry-run-closure",
        "if: always()",
        "npm run release:readiness:artifact-consumer",
    ]:
        assert needle in ci

    asset_publication = read("scripts/release-asset-publication-readiness.sh")
    public_ship_dry_run = read("scripts/public-ship-dry-run.sh")
    public_ship_rehearsal = read("scripts/public-ship-rehearsal.sh")
    release_train = read("scripts/public-release-train-drill.sh")
    for text, needle in [
        (asset_publication, "AO2_RELEASE_ASSET_PUBLICATION_READINESS_CI_SAFE"),
        (asset_publication, "AO2_PUBLIC_SHIP_DRY_RUN_CI_SAFE"),
        (public_ship_dry_run, "AO2_PUBLIC_SHIP_DRY_RUN_CI_SAFE"),
        (public_ship_dry_run, "AO2_PUBLIC_SHIP_REHEARSAL_CI_SAFE"),
        (public_ship_rehearsal, "AO2_PUBLIC_SHIP_REHEARSAL_CI_SAFE"),
        (public_ship_rehearsal, "AO2_PUBLIC_RELEASE_TRAIN_CI_SAFE"),
        (release_train, "AO2_PUBLIC_RELEASE_TRAIN_CI_SAFE"),
        (release_train, "release_evidence_closure skipped in ci-safe mode"),
        (release_train, "post_merge_canary skipped in ci-safe mode"),
    ]:
        assert needle in text

    verification = read("docs/VERIFICATION.md")
    for needle in [
        "Release readiness artifact consumer",
        "ao2-release-readiness-consumer",
        "ao2.release-readiness-artifact-consumer.v1",
        "ao2-release-train-control-plane-bridge",
        "ao2.release-train-control-plane-bridge.v1",
        "ao2-pulse-task-board-closure-packet",
        "ao2.pulse-task-board-closure-packet.v1",
        "ao2-pulse-codex-cron-event-loop-smoke",
        "ao2.pulse-codex-cron-event-loop-smoke.v1",
        "ao2-release-publication-closure",
        "ao2.release-publication-dry-run-closure.v1",
        "ao2-control-plane-release-publication-closure",
        "ao2.cp-release-publication-closure.v1",
        "ao2-control-plane-",
        ".tar.gz",
        "sha256",
        "size_bytes",
        "ao2-dual-repo-release-publication-closure-index",
        "ao2.dual-repo-release-publication-closure-index.v1",
        "ao2.release-artifact-closure-index.v1",
        "artifact-closure-index.json",
        "required_archive_names",
        "required_archive_presence",
        "full archive parity",
        "ao2-0.4.80-linux-aarch64.tar.gz",
        "ao2-control-plane-0.1.13-windows-x86_64.tar.gz",
    ]:
        assert needle in verification

    out_root = tmp_path / "release-readiness"
    result = subprocess.run(
        ["bash", "scripts/release-readiness.sh", "--static-only"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_RELEASE_READINESS_ROOT": str(out_root),
        },
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.release-readiness-local.v1"
    assert summary["status"] == "passed"
    assert summary["artifact_closure_index"] == str(out_root / "artifact-closure-index.json")
    checks = {item["name"]: item for item in summary["checks"]}
    for name in [
        "ci_job_required_os:verify",
        "ci_job_required_os:release-archive-hosted-smoke",
        "ci_job_required_os:workbench-operator-packet-control-plane-smoke",
        "ci_job_required_os:non_approval_required_check_compat",
        "ci_workbench_operator_packet_smoke_index_requires_all_os",
        "ci_release_readiness_static_artifact_job",
        "ci_release_readiness_artifact_consumer_job",
        "ci_release_train_control_plane_bridge_artifact_job",
        "ci_ai_task_board_control_plane_bridge_artifact_job",
        "ci_pulse_task_board_closure_packet_artifact_job",
        "ci_pulse_codex_cron_event_loop_smoke_artifact_job",
        "ci_dual_repo_installed_release_smoke_artifact_job",
        "ci_release_publication_closure_artifact_job",
        "ci_dual_repo_release_publication_closure_index_job",
    ]:
        assert checks[name]["status"] == "passed"
    closure = json.loads(
        (out_root / "artifact-closure-index.json").read_text(encoding="utf-8")
    )
    assert closure["schema_version"] == "ao2.release-artifact-closure-index.v1"
    assert closure["status"] == "passed"
    artifacts = {artifact["id"]: artifact for artifact in closure["required_artifacts"]}
    assert artifacts["release_readiness"]["artifact_name"] == "ao2-release-readiness"
    assert artifacts["release_readiness"]["schema_versions"] == [
        "ao2.release-readiness-local.v1"
    ]
    assert artifacts["release_readiness_artifact_consumer"]["artifact_name"] == (
        "ao2-release-readiness-consumer"
    )
    assert artifacts["release_readiness_artifact_consumer"]["producer_job"] == (
        "release-readiness-artifact-consumer"
    )
    assert artifacts["release_readiness_artifact_consumer"]["schema_versions"] == [
        "ao2.release-readiness-artifact-consumer.v1"
    ]
    assert "ao2-pulse-codex-cron-event-loop-smoke" in artifacts[
        "release_readiness_artifact_consumer"
    ]["consumes"]
    assert artifacts["release_train_control_plane_bridge"]["artifact_name"] == (
        "ao2-release-train-control-plane-bridge"
    )
    assert artifacts["release_train_control_plane_bridge"]["producer_job"] == (
        "release-train-control-plane-bridge-artifacts"
    )
    assert artifacts["release_train_control_plane_bridge"]["schema_versions"] == [
        "ao2.release-train-control-plane-bridge.v1",
        "ao2.cp-release-train-bridge-smoke.v1",
    ]
    assert artifacts["ai_task_board_control_plane_bridge"]["artifact_name"] == (
        "ao2-ai-task-board-control-plane-bridge"
    )
    assert artifacts["ai_task_board_control_plane_bridge"]["producer_job"] == (
        "ai-task-board-control-plane-bridge-artifacts"
    )
    assert artifacts["ai_task_board_control_plane_bridge"]["schema_versions"] == [
        "ao2.ai-task-board-control-plane-bridge.v1",
        "ao2.ai-task-board-control-plane-bridge-smoke.v1",
        "ao2.cp-ai-task-board-readback.v1",
        "ao2.cp-ai-task-board-dashboard.v1",
    ]
    assert artifacts["pulse_task_board_closure_packet"]["artifact_name"] == (
        "ao2-pulse-task-board-closure-packet"
    )
    assert artifacts["pulse_task_board_closure_packet"]["producer_job"] == (
        "pulse-task-board-closure-packet-artifacts"
    )
    assert artifacts["pulse_task_board_closure_packet"]["schema_versions"] == [
        "ao2.pulse-task-board-closure-packet.v1",
        "ao2.pulse-next-actions.v1",
        "ao2.pulse-task-board-state.v1",
        "ao2.control-plane-fixture-consumer-smoke.v1",
    ]
    assert artifacts["pulse_codex_cron_event_loop_smoke"]["artifact_name"] == (
        "ao2-pulse-codex-cron-event-loop-smoke"
    )
    assert artifacts["pulse_codex_cron_event_loop_smoke"]["producer_job"] == (
        "pulse-codex-cron-event-loop-smoke-artifacts"
    )
    assert artifacts["pulse_codex_cron_event_loop_smoke"]["required_files"] == [
        "latest/summary.json",
        "latest/pulse-generate-next/summary.json",
        "latest/pulse-next-recommended-tasks/codex-cron-event-loop-decision.json",
        "latest/codex-cron-run-loop.stdout",
    ]
    assert artifacts["pulse_codex_cron_event_loop_smoke"]["schema_versions"] == [
        "ao2.pulse-codex-cron-event-loop-smoke.v1",
        "codex-cron.event-loop-decision.v1",
        "ao2.pulse-codex-cron-event-loop-decision.v1",
        "ao2.pulse-generate-next.v1",
    ]
    assert artifacts["pulse_codex_cron_event_loop_smoke"]["required_checks"] == [
        "ci_pulse_codex_cron_event_loop_smoke_artifact_job"
    ]
    assert artifacts["dual_repo_installed_release_smoke"]["artifact_name"] == (
        "ao2-dual-repo-installed-release-smoke"
    )
    assert artifacts["dual_repo_installed_release_smoke"]["producer_job"] == (
        "dual-repo-installed-release-smoke-artifacts"
    )
    assert artifacts["dual_repo_installed_release_smoke"]["schema_versions"] == [
        "ao2.dual-repo-installed-release-smoke.v1",
        "ao2.release-manifest.v1",
        "ao2-control-plane.release-manifest.v1",
        "ao2.cp-ai-task-board-readback.v1",
        "ao2.cp-ai-task-board-dashboard.v1",
    ]
    assert artifacts["release_publication_closure"]["artifact_name"] == (
        "ao2-release-publication-closure"
    )
    assert artifacts["release_publication_closure"]["producer_job"] == (
        "release-publication-closure-artifacts"
    )
    assert artifacts["release_publication_closure"]["schema_versions"] == [
        "ao2.release-publication-dry-run-closure.v1",
        "ao2.release-asset-publication-readiness.v1",
        "ao2.release-sync-provenance-assets.v1",
        "ao2.stable-release-readiness.v1",
    ]
    assert artifacts["dual_repo_release_publication_closure_index"]["artifact_name"] == (
        "ao2-dual-repo-release-publication-closure-index"
    )
    assert artifacts["dual_repo_release_publication_closure_index"]["producer_job"] == (
        "dual-repo-release-publication-closure-index"
    )
    assert artifacts["dual_repo_release_publication_closure_index"]["schema_versions"] == [
        "ao2.dual-repo-release-publication-closure-index.v1",
        "ao2.release-publication-dry-run-closure.v1",
        "ao2.cp-release-publication-closure.v1",
    ]
    assert artifacts["dual_repo_release_publication_closure_index"]["source_artifacts"] == [
        "ao2-release-publication-closure",
        "ao2-control-plane-release-publication-closure",
    ]
    assert closure["trust_boundary"]["local_only"] is True
    assert closure["trust_boundary"]["stores_credentials"] is False


def _write_release_readiness_consumer_json(root: Path, rel_path: str, payload):
    path = root / rel_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def _write_release_readiness_consumer_fixture(root: Path):
    required_checks = [
        "ci_job_required_os:verify",
        "ci_job_required_os:release-archive-hosted-smoke",
        "ci_job_required_os:workbench-operator-packet-control-plane-smoke",
        "ci_release_readiness_static_artifact_job",
        "ci_release_train_control_plane_bridge_artifact_job",
        "ci_ai_task_board_control_plane_bridge_artifact_job",
        "ci_pulse_task_board_closure_packet_artifact_job",
        "ci_pulse_codex_cron_event_loop_smoke_artifact_job",
        "ci_dual_repo_installed_release_smoke_artifact_job",
        "ci_release_publication_closure_artifact_job",
        "ci_dual_repo_release_publication_closure_index_job",
    ]
    _write_release_readiness_consumer_json(
        root,
        "ao2-release-readiness/summary.json",
        {
            "schema_version": "ao2.release-readiness-local.v1",
            "status": "passed",
            "checks": [{"name": name, "status": "passed"} for name in required_checks],
        },
    )
    _write_release_readiness_consumer_json(
        root,
        "ao2-release-train-control-plane-bridge/latest/summary.json",
        {
            "schema_version": "ao2.release-train-control-plane-bridge.v1",
            "status": "passed",
            "control_plane": {"smoke": "passed"},
        },
    )
    _write_release_readiness_consumer_json(
        root,
        "ao2-ai-task-board-control-plane-bridge/latest/summary.json",
        {
            "schema_version": "ao2.ai-task-board-control-plane-bridge.v1",
            "status": "passed",
            "control_plane": {"smoke": "passed"},
        },
    )
    _write_release_readiness_consumer_json(
        root,
        "ao2-ai-task-board-control-plane-bridge/latest/control-plane-smoke/summary.json",
        {
            "latest": {"schema_version": "ao2.cp-ai-task-board-readback.v1"},
            "dashboard": {"schema_version": "ao2.cp-ai-task-board-dashboard.v1"},
        },
    )
    _write_release_readiness_consumer_json(
        root,
        "ao2-pulse-task-board-closure-packet/latest/summary.json",
        {
            "schema_version": "ao2.pulse-task-board-closure-packet.v1",
            "status": "passed",
            "alignment": {"task_ids_match": True, "safety_fields_preserved": True},
            "checks": {
                "control_plane_fixture_consumer": {
                    "operator_task_board_view_status": "passed"
                }
            },
        },
    )
    _write_release_readiness_consumer_json(
        root,
        "ao2-pulse-codex-cron-event-loop-smoke/latest/summary.json",
        {
            "schema_version": "ao2.pulse-codex-cron-event-loop-smoke.v1",
            "status": "passed",
            "codex_cron": {"decision_source": "file"},
            "ao2": {
                "decision_schema": "codex-cron.event-loop-decision.v1",
                "ao2_decision_schema": "ao2.pulse-codex-cron-event-loop-decision.v1",
            },
            "trust_boundary": {"provider_execution": False},
        },
    )
    _write_release_readiness_consumer_json(
        root,
        "ao2-pulse-codex-cron-event-loop-smoke/latest/pulse-generate-next/summary.json",
        {"schema_version": "ao2.pulse-generate-next.v1", "status": "ready"},
    )
    _write_release_readiness_consumer_json(
        root,
        "ao2-pulse-codex-cron-event-loop-smoke/latest/pulse-next-recommended-tasks/codex-cron-event-loop-decision.json",
        {
            "schema_version": "codex-cron.event-loop-decision.v1",
            "ao2": {
                "schema_version": "ao2.pulse-codex-cron-event-loop-decision.v1"
            },
        },
    )
    stdout_path = (
        root
        / "ao2-pulse-codex-cron-event-loop-smoke/latest/codex-cron-run-loop.stdout"
    )
    stdout_path.parent.mkdir(parents=True, exist_ok=True)
    stdout_path.write_text("ok\n", encoding="utf-8")
    _write_release_readiness_consumer_json(
        root,
        "ao2-dual-repo-installed-release-smoke/latest/summary.json",
        {
            "schema_version": "ao2.dual-repo-installed-release-smoke.v1",
            "status": "passed",
            "archives": {
                "ao2": {"manifest_schema": "ao2.release-manifest.v1"},
                "ao2_control_plane": {
                    "manifest_schema": "ao2-control-plane.release-manifest.v1"
                },
            },
            "trust_boundary": {"auth_value_stored": False},
        },
    )
    _write_release_readiness_consumer_json(
        root,
        "ao2-release-publication-closure/summary.json",
        {
            "schema_version": "ao2.release-publication-dry-run-closure.v1",
            "status": "passed",
            "publication_ready": True,
            "stable_release_ready": True,
            "publication_state": {"dry_run": True, "upload_status": "not_attempted"},
            "trust_boundary": {"mutates_releases": False, "stores_credentials": False},
        },
    )
    _write_release_readiness_consumer_json(
        root,
        "ao2-dual-repo-release-publication-closure-index/summary.json",
        {
            "schema_version": "ao2.dual-repo-release-publication-closure-index.v1",
            "status": "passed",
            "ao2": {"schema_version": "ao2.release-publication-dry-run-closure.v1"},
            "control_plane": {
                "schema_version": "ao2.cp-release-publication-closure.v1",
                "checksum_verified": True,
                "assets": [
                    {"name": "SHA256SUMS"},
                    {
                        "name": "ao2-control-plane-0.1.13-linux-x86_64.tar.gz",
                        "sha256": "a" * 64,
                        "size_bytes": 4096,
                    },
                ],
            },
            "trust_boundary": {
                "mutates_releases": False,
                "mutates_github_releases": False,
            },
        },
    )


def _run_release_readiness_artifact_consumer(root: Path):
    return subprocess.run(
        ["npm", "run", "release:readiness:artifact-consumer"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={**os.environ, "AO2_RELEASE_READINESS_CONSUMER_ROOT": str(root)},
        check=False,
    )


def test_release_readiness_artifact_consumer_script_runs_against_fixture(tmp_path):
    package_json = json.loads(read("package.json"))
    assert package_json["scripts"]["release:readiness:artifact-consumer"] == (
        "node scripts/run-sh-script.js scripts/release-readiness-artifact-consumer.sh"
    )

    script_path = REPO_ROOT / "scripts" / "release-readiness-artifact-consumer.sh"
    assert script_path.is_file()
    assert script_path.stat().st_mode & stat.S_IXUSR
    script = script_path.read_text(encoding="utf-8")
    for needle in [
        "AO2_RELEASE_READINESS_CONSUMER_ROOT",
        "ao2.release-readiness-artifact-consumer.v1",
        "ao2.pulse-codex-cron-event-loop-smoke.v1",
        "codex-cron.event-loop-decision.v1",
        "ci_pulse_codex_cron_event_loop_smoke_artifact_job",
        "github_actions_artifact_download",
        "provider_execution",
        "ao2-control-plane-",
        ".tar.gz",
        "sha256",
        "size_bytes",
    ]:
        assert needle in script

    ci = read(".github/workflows/ci.yml")
    consumer_block = re.search(
        r"(?ms)^  release-readiness-artifact-consumer:\n(?P<body>.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)",
        ci,
    )
    assert consumer_block
    consumer_ci = consumer_block.group("body")
    assert "uses: actions/checkout@v6.0.3" in consumer_ci
    assert "npm run release:readiness:artifact-consumer" in consumer_ci
    assert "python3 - <<'PY'" not in consumer_ci

    root = tmp_path / "release-readiness-consumer"
    _write_release_readiness_consumer_fixture(root)

    result = _run_release_readiness_artifact_consumer(root)
    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.release-readiness-artifact-consumer.v1"
    assert summary["status"] == "passed"
    assert "ao2-pulse-codex-cron-event-loop-smoke" in summary["source_artifacts"]
    assert "ci_pulse_codex_cron_event_loop_smoke_artifact_job" in summary["required_checks"]
    assert summary["trust_boundary"]["stores_credentials"] is False


def test_release_readiness_artifact_consumer_rejects_bad_fixture_evidence(tmp_path):
    cases = [
        (
            "missing_codex_cron_decision",
            lambda root: (
                root
                / "ao2-pulse-codex-cron-event-loop-smoke/latest/pulse-next-recommended-tasks/codex-cron-event-loop-decision.json"
            ).unlink(),
            "missing Pulse codex-cron smoke file",
        ),
        (
            "wrong_pulse_generate_next_schema",
            lambda root: _write_release_readiness_consumer_json(
                root,
                "ao2-pulse-codex-cron-event-loop-smoke/latest/pulse-generate-next/summary.json",
                {"schema_version": "ao2.wrong-pulse-generate-next.v1"},
            ),
            "unexpected Pulse generate-next schema",
        ),
        (
            "wrong_codex_cron_decision_schema",
            lambda root: _write_release_readiness_consumer_json(
                root,
                "ao2-pulse-codex-cron-event-loop-smoke/latest/pulse-next-recommended-tasks/codex-cron-event-loop-decision.json",
                {
                    "schema_version": "codex-cron.wrong-event-loop-decision.v1",
                    "ao2": {
                        "schema_version": "ao2.pulse-codex-cron-event-loop-decision.v1"
                    },
                },
            ),
            "unexpected codex-cron decision file schema",
        ),
        (
            "blocked_pulse_generate_next_status",
            lambda root: _write_release_readiness_consumer_json(
                root,
                "ao2-pulse-codex-cron-event-loop-smoke/latest/pulse-generate-next/summary.json",
                {"schema_version": "ao2.pulse-generate-next.v1", "status": "blocked"},
            ),
            "Pulse generate-next was not ready",
        ),
        (
            "provider_execution_enabled",
            lambda root: _write_release_readiness_consumer_json(
                root,
                "ao2-pulse-codex-cron-event-loop-smoke/latest/summary.json",
                {
                    "schema_version": "ao2.pulse-codex-cron-event-loop-smoke.v1",
                    "status": "passed",
                    "codex_cron": {"decision_source": "file"},
                    "ao2": {
                        "decision_schema": "codex-cron.event-loop-decision.v1",
                        "ao2_decision_schema": (
                            "ao2.pulse-codex-cron-event-loop-decision.v1"
                        ),
                    },
                    "trust_boundary": {"provider_execution": True},
                },
            ),
            "Pulse codex-cron smoke must not execute providers",
        ),
        (
            "control_plane_checksum_valid_without_archive_asset",
            lambda root: _write_release_readiness_consumer_json(
                root,
                "ao2-dual-repo-release-publication-closure-index/summary.json",
                {
                    "schema_version": (
                        "ao2.dual-repo-release-publication-closure-index.v1"
                    ),
                    "status": "passed",
                    "ao2": {
                        "schema_version": (
                            "ao2.release-publication-dry-run-closure.v1"
                        )
                    },
                    "control_plane": {
                        "schema_version": "ao2.cp-release-publication-closure.v1",
                        "checksum_verified": True,
                        "assets": [{"name": "SHA256SUMS"}],
                    },
                    "trust_boundary": {
                        "mutates_releases": False,
                        "mutates_github_releases": False,
                    },
                },
            ),
            "control-plane publication closure missing release archive asset",
        ),
        (
            "control_plane_archive_asset_without_digest_evidence",
            lambda root: _write_release_readiness_consumer_json(
                root,
                "ao2-dual-repo-release-publication-closure-index/summary.json",
                {
                    "schema_version": (
                        "ao2.dual-repo-release-publication-closure-index.v1"
                    ),
                    "status": "passed",
                    "ao2": {
                        "schema_version": (
                            "ao2.release-publication-dry-run-closure.v1"
                        )
                    },
                    "control_plane": {
                        "schema_version": "ao2.cp-release-publication-closure.v1",
                        "checksum_verified": True,
                        "assets": [
                            {
                                "name": (
                                    "ao2-control-plane-0.1.13-linux-x86_64.tar.gz"
                                )
                            }
                        ],
                    },
                    "trust_boundary": {
                        "mutates_releases": False,
                        "mutates_github_releases": False,
                    },
                },
            ),
            "control-plane publication closure archive missing digest evidence",
        ),
        (
            "missing_required_check",
            lambda root: _write_release_readiness_consumer_json(
                root,
                "ao2-release-readiness/summary.json",
                {
                    "schema_version": "ao2.release-readiness-local.v1",
                    "status": "passed",
                    "checks": [],
                },
            ),
            "release-readiness artifact missing passed checks",
        ),
    ]

    for case_name, mutate, expected in cases:
        root = tmp_path / case_name
        _write_release_readiness_consumer_fixture(root)
        mutate(root)

        result = _run_release_readiness_artifact_consumer(root)

        assert result.returncode != 0, case_name
        assert expected in result.stderr + result.stdout


def test_dual_repo_installed_release_smoke_contract():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:dual-repo-installed-smoke"]
        == "node scripts/run-sh-script.js scripts/dual-repo-installed-release-smoke.sh"
    )

    script = REPO_ROOT / "scripts" / "dual-repo-installed-release-smoke.sh"
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.dual-repo-installed-release-smoke.v1",
        "ao2.release-manifest.v1",
        "ao2-control-plane.release-manifest.v1",
        "ao2.ai-task-board.v1",
        "ao2.cp-ai-task-board-readback.v1",
        "ao2.cp-ai-task-board-dashboard.v1",
        "AO2_DUAL_REPO_INSTALLED_SMOKE_ROOT",
        "AO2_DUAL_REPO_INSTALLED_SMOKE_BIND",
        "AO2_CONTROL_PLANE_ROOT",
        "Authorization: Bearer",
        "auth_value_stored",
        "credential_material_included",
        "control_plane_approves_release",
        "mutates_releases",
        "stores_credentials",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "release package",
        "scripts/package-local.sh",
    ]:
        assert needle in text

    for forbidden in [
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text


def test_dual_repo_installed_release_smoke_ci_and_release_note_contract():
    ci = read(".github/workflows/ci.yml")
    for needle in [
        "dual-repo-installed-release-smoke-artifacts:",
        "name: Dual-repo installed release smoke artifacts",
        "repository: uesugitorachiyo/ao2-control-plane",
        "AO2_DUAL_REPO_INSTALLED_SMOKE_ROOT=target/dual-repo-installed-release-smoke-ci",
        "npm run release:dual-repo-installed-smoke -- --control-plane-root ao2-control-plane",
        "ao2.dual-repo-installed-release-smoke.v1",
        "ao2.cp-ai-task-board-readback.v1",
        "ao2.cp-ai-task-board-dashboard.v1",
        "name: ao2-dual-repo-installed-release-smoke",
        "target/dual-repo-installed-release-smoke-ci/latest/summary.json",
        "target/release-readiness-consumer/ao2-dual-repo-installed-release-smoke",
    ]:
        assert needle in ci

    release_doc = read("docs/release/v0.4.81-ai-task-board-control-surface.md")
    for needle in [
        "npm run release:dual-repo-installed-smoke",
        "ao2.dual-repo-installed-release-smoke.v1",
        "installed AO2 archive",
        "installed ao2-control-plane archive",
        "ao2.cp-ai-task-board-readback.v1",
    ]:
        assert needle in release_doc


def test_pulse_codex_cron_event_loop_smoke_contract():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["pulse:codex-cron-event-loop-smoke"]
        == "node scripts/run-sh-script.js scripts/pulse-codex-cron-event-loop-smoke.sh"
    )

    script = REPO_ROOT / "scripts" / "pulse-codex-cron-event-loop-smoke.sh"
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.pulse-codex-cron-event-loop-smoke.v1",
        "AO2_PULSE_CODEX_CRON_SMOKE_ROOT",
        "AO2_CODEX_CRON_BIN",
        "AO2_CODEX_CRON_ROOT",
        "codex-cron-event-loop-decision.json",
        "codex-cron.event-loop-decision.v1",
        "ao2.pulse-codex-cron-event-loop-decision.v1",
        "--event-loop-decision-file",
        'generator_summary_path = root / pulse_generate_root_rel / "summary.json"',
        "decision_source",
        "file",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "stores_credentials",
        "provider_execution",
    ]:
        assert needle in text

    for forbidden in [
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        'generator_summary_path = root / "target/pulse-codex-cron-event-loop-smoke/latest/pulse-generate-next/summary.json"',
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text


def test_pulse_codex_cron_event_loop_smoke_ci_contract():
    ci = read(".github/workflows/ci.yml")
    for needle in [
        "pulse-codex-cron-event-loop-smoke-artifacts:",
        "name: Pulse codex-cron event-loop smoke artifacts",
        "repository: uesugitorachiyo/codex-cron",
        "AO2_PULSE_CODEX_CRON_SMOKE_ROOT=target/pulse-codex-cron-event-loop-smoke-ci",
        "npm run pulse:codex-cron-event-loop-smoke -- --codex-cron-root codex-cron",
        "ao2.pulse-codex-cron-event-loop-smoke.v1",
        "codex-cron.event-loop-decision.v1",
        "decision_source",
        "name: ao2-pulse-codex-cron-event-loop-smoke",
        "target/pulse-codex-cron-event-loop-smoke-ci/latest/summary.json",
    ]:
        assert needle in ci

    verification = read("docs/VERIFICATION.md")
    for needle in [
        "npm run pulse:codex-cron-event-loop-smoke",
        "ao2.pulse-codex-cron-event-loop-smoke.v1",
        "codex-cron-event-loop-decision.json",
        "decision_source=file",
    ]:
        assert needle in verification


def test_dual_public_release_smoke_contract():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:dual-public-smoke"]
        == "node scripts/run-sh-script.js scripts/dual-public-release-smoke.sh"
    )

    script = REPO_ROOT / "scripts" / "dual-public-release-smoke.sh"
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.dual-public-release-smoke.v1",
        "AO2_PUBLIC_RELEASE_SMOKE_ROOT",
        "AO2_PUBLIC_RELEASE_SMOKE_BIND",
        "AO2_RELEASE_TAG",
        "AO2_CP_RELEASE_TAG",
        "uesugitorachiyo/ao2",
        "uesugitorachiyo/ao2-control-plane",
        "gh release download",
        "ao2-0.4.80-linux-x86_64.tar.gz",
        "ao2-control-plane-0.1.13-linux-x86_64.tar.gz",
        "SHA256SUMS",
        "ao2.release-manifest.v1",
        "ao2-control-plane.release-manifest.v1",
        "ao2.ai-task-board.v1",
        "ao2.cp-ai-task-board-readback.v1",
        "ao2.cp-ai-task-board-dashboard.v1",
        "Authorization: Bearer",
        "downloads_public_release_archives",
        "auth_value_stored",
        "credential_material_included",
        "mutates_github_releases",
        "control_plane_approves_release",
    ]:
        assert needle in text

    for forbidden in [
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release upload",
        "gh release edit",
        "gh release create",
        "git push origin",
        "npm publish",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
    ]:
        assert forbidden not in text


def test_dual_public_release_smoke_workflow_and_docs_contract():
    workflow = read(".github/workflows/post-stable-release-verification.yml")
    release_doc = read("docs/release/PUBLIC-RELEASE-VERIFICATION.md")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        "dual-public-release-smoke:",
        "name: Dual public release smoke",
        "AO2_PUBLIC_RELEASE_SMOKE_ROOT=target/dual-public-release-smoke",
        "npm run release:dual-public-smoke",
        "ao2.dual-public-release-smoke.v1",
        "ao2.cp-ai-task-board-readback.v1",
        "ao2.cp-ai-task-board-dashboard.v1",
        "ao2-dual-public-release-smoke",
        "target/dual-public-release-smoke/latest/summary.json",
    ]:
        assert needle in workflow

    for needle in [
        "AO2 stable release: `v0.4.80`",
        "AO2 control-plane stable release: `v0.1.13`",
        "ao2-dual-public-release-smoke",
        "ao2.dual-public-release-smoke.v1",
        "published AO2 Linux x86_64 archive",
        "published control-plane Linux x86_64 archive",
        "read-only",
        "mutates_github_releases=false",
    ]:
        assert needle in release_doc

    for needle in [
        "release:dual-public-smoke",
        "ao2.dual-public-release-smoke.v1",
        "published AO2 and control-plane archives",
    ]:
        assert needle in verification


def test_ai_task_board_control_plane_bridge_script_contract():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["ai-task-board:control-plane-bridge"]
        == "node scripts/run-sh-script.js scripts/ai-task-board-control-plane-bridge.sh"
    )

    script = REPO_ROOT / "scripts" / "ai-task-board-control-plane-bridge.sh"
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.ai-task-board-control-plane-bridge.v1",
        "ao2.ai-task-board.v1",
        "ao2.cp-ingest-receipt.v1",
        "ao2.cp-ai-task-board-readback.v1",
        "ao2.cp-ai-task-board-dashboard.v1",
        "/api/v1/ai/task-board",
        "/api/v1/ai/task-board/latest",
        "/api/v1/ai/task-board/dashboard.json",
        "AO2_AI_TASK_BOARD_CP_BRIDGE_ROOT",
        "AO2_AI_TASK_BOARD_CP_BRIDGE_BIND",
        "AO2_CONTROL_PLANE_ROOT",
        "Authorization: Bearer",
        "auth_value_stored",
        "credential_material_included",
        "control_plane_approves_release",
        "mutates_releases",
        "stores_credentials",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
    ]:
        assert needle in text

    for forbidden in [
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text


def test_ai_task_board_control_plane_bridge_ci_and_release_readiness_contract():
    ci = read(".github/workflows/ci.yml")
    for needle in [
        "ai-task-board-control-plane-bridge-artifacts:",
        "name: AI task board control-plane bridge artifacts",
        "repository: uesugitorachiyo/ao2-control-plane",
        "AO2_AI_TASK_BOARD_CP_BRIDGE_ROOT=target/ai-task-board-control-plane-bridge-ci",
        "npm run ai-task-board:control-plane-bridge -- --control-plane-root ao2-control-plane",
        "ao2.ai-task-board-control-plane-bridge.v1",
        "ao2.cp-ai-task-board-readback.v1",
        "ao2.cp-ai-task-board-dashboard.v1",
        "name: ao2-ai-task-board-control-plane-bridge",
        "target/ai-task-board-control-plane-bridge-ci/latest/control-plane-smoke/summary.json",
        "target/release-readiness-consumer/ao2-ai-task-board-control-plane-bridge",
    ]:
        assert needle in ci

    script = read("scripts/release-readiness.sh")
    for needle in [
        "ai-task-board-control-plane-bridge-artifacts",
        "ao2-ai-task-board-control-plane-bridge",
        "ao2.ai-task-board-control-plane-bridge.v1",
        "ao2.cp-ai-task-board-readback.v1",
        "ci_ai_task_board_control_plane_bridge_artifact_job",
    ]:
        assert needle in script


def test_ai_task_board_control_plane_bridge_release_note_evidence():
    release_doc = read("docs/release/v0.4.81-ai-task-board-control-surface.md")
    for needle in [
        "npm run ai-task-board:control-plane-bridge",
        "ao2.ai-task-board-control-plane-bridge.v1",
        "/api/v1/ai/task-board",
        "ao2.cp-ai-task-board-readback.v1",
        "ao2.cp-ai-task-board-dashboard.v1",
    ]:
        assert needle in release_doc


def test_pulse_task_board_closure_packet_contract():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["pulse:task-board-closure-packet"]
        == "node scripts/run-sh-script.js scripts/pulse-task-board-closure-packet.sh"
    )

    script = REPO_ROOT / "scripts" / "pulse-task-board-closure-packet.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.pulse-task-board-closure-packet.v1",
        "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_ROOT",
        "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_VERIFY_ONLY",
        "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_TASK_BOARD",
        "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_NEXT_ACTIONS",
        "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_TASK_BOARD_STATE",
        "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_CONTROL_PLANE",
        "AO2_PULSE_GENERATE_NEXT_REGISTER=0",
        "AO2_PULSE_TASK_BOARD_ROOT",
        "AO2_PULSE_NEXT_ACTIONS_BOARD",
        "AO2_PULSE_TASK_BOARD_STATE_BOARD",
        "AO2_CP_FIXTURE_CONSUMER_TASK_BOARD",
        "npm run pulse:generate-next",
        "npm run pulse:next-actions",
        "npm run pulse:task-board-state",
        "npm run control-plane:fixture-consumer-smoke",
        "required_evidence",
        "stop_conditions",
        "blockers",
        "safety_fields_missing",
        "task_id_alignment_mismatch",
        "control_plane_readback_failed",
        "operator_task_board_view",
        "stores_credentials",
        "mutates_releases",
    ]:
        assert needle in text

    for forbidden in [
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text


def test_pulse_task_board_closure_packet_executes_with_safety_fields(tmp_path):
    out_root = tmp_path / "closure-packet"
    result = subprocess.run(
        ["npm", "run", "pulse:task-board-closure-packet"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_ROOT": str(out_root),
        },
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "latest" / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-task-board-closure-packet.v1"
    assert summary["status"] == "passed"
    assert summary["task_count"] >= 1
    assert summary["alignment"]["task_ids_match"] is True
    assert summary["alignment"]["safety_fields_preserved"] is True
    assert summary["checks"]["task_board"]["schema_version"] == "ao2.ai-task-board.v1"
    assert summary["checks"]["next_actions"]["schema_version"] == "ao2.pulse-next-actions.v1"
    assert summary["checks"]["task_board_state"]["schema_version"] == "ao2.pulse-task-board-state.v1"
    assert summary["checks"]["control_plane_fixture_consumer"]["schema_version"] == "ao2.control-plane-fixture-consumer-smoke.v1"
    assert summary["checks"]["control_plane_fixture_consumer"]["operator_task_board_view_status"] == "passed"
    assert summary["trust_boundary"]["local_only"] is True
    assert summary["trust_boundary"]["stores_credentials"] is False
    assert summary["trust_boundary"]["mutates_releases"] is False

    first_action = summary["next_actions"][0]
    assert first_action["required_evidence_count"] >= 1
    assert first_action["stop_conditions_count"] >= 1
    assert (out_root / "latest" / "closure-packet.md").is_file()


def _write_closure_packet_fixture(root: Path, *, case: str) -> dict:
    root.mkdir(parents=True, exist_ok=True)
    task_id = "ao2-test-task-g1"
    board = {
        "schema_version": "ao2.ai-task-board.v1",
        "release_objective": "Test closure packet fail-closed behavior.",
        "tasks": [
            {
                "task_id": task_id,
                "stable_task_id": "ao2-test-task",
                "title": "Test task",
                "status": "proposed",
                "next_action": "npm test",
                "required_evidence": ["ao2.test.evidence.v1"],
                "stop_conditions": ["Stop if evidence is missing."],
            }
        ],
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    next_actions = {
        "schema_version": "ao2.pulse-next-actions.v1",
        "status": "passed",
        "next_actions": [
            {
                "task_id": task_id,
                "stable_task_id": "ao2-test-task",
                "status": "proposed",
                "next_action": "npm test",
                "required_evidence": ["ao2.test.evidence.v1"],
                "stop_conditions": ["Stop if evidence is missing."],
            }
        ],
    }
    task_board_state = {
        "schema_version": "ao2.pulse-task-board-state.v1",
        "status": "passed",
        "next_actions": [
            {
                "task_id": task_id,
                "stable_task_id": "ao2-test-task",
                "status": "proposed",
                "next_action": "npm test",
            }
        ],
    }
    control_plane = {
        "schema_version": "ao2.control-plane-fixture-consumer-smoke.v1",
        "status": "passed",
        "task_board_readback": {"status": "passed"},
        "operator_task_board_view": {"status": "passed"},
    }
    if case == "missing_safety_fields":
        next_actions["next_actions"][0]["required_evidence"] = []
        next_actions["next_actions"][0]["stop_conditions"] = []
    elif case == "mismatched_task_ids":
        task_board_state["next_actions"][0]["task_id"] = "ao2-different-task-g1"
    elif case == "control_plane_failed":
        control_plane["status"] = "failed"
        control_plane["task_board_readback"] = {"status": "failed"}
        control_plane["operator_task_board_view"] = {"status": "skipped"}
    else:
        raise AssertionError(f"unknown fixture case: {case}")

    paths = {
        "task_board": root / "task-board.json",
        "next_actions": root / "next-actions.json",
        "task_board_state": root / "task-board-state.json",
        "control_plane": root / "control-plane.json",
    }
    payloads = {
        "task_board": board,
        "next_actions": next_actions,
        "task_board_state": task_board_state,
        "control_plane": control_plane,
    }
    for name, payload in payloads.items():
        paths[name].write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return paths


def _run_closure_packet_verify_only(tmp_path: Path, *, case: str) -> tuple[subprocess.CompletedProcess[str], dict]:
    fixture_root = tmp_path / "fixtures" / case
    out_root = tmp_path / "out" / case
    paths = _write_closure_packet_fixture(fixture_root, case=case)
    result = subprocess.run(
        ["npm", "run", "pulse:task-board-closure-packet"],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_ROOT": str(out_root),
            "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_VERIFY_ONLY": "1",
            "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_TASK_BOARD": str(paths["task_board"]),
            "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_NEXT_ACTIONS": str(paths["next_actions"]),
            "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_TASK_BOARD_STATE": str(paths["task_board_state"]),
            "AO2_PULSE_TASK_BOARD_CLOSURE_PACKET_CONTROL_PLANE": str(paths["control_plane"]),
        },
        check=False,
    )
    summary = json.loads((out_root / "latest" / "summary.json").read_text(encoding="utf-8"))
    return result, summary


def test_pulse_task_board_closure_packet_rejects_missing_safety_fields(tmp_path):
    result, summary = _run_closure_packet_verify_only(tmp_path, case="missing_safety_fields")

    assert result.returncode != 0
    assert summary["status"] == "failed"
    assert summary["alignment"]["safety_fields_preserved"] is False
    assert "safety_fields_missing" in summary["blockers"]


def test_pulse_task_board_closure_packet_rejects_mismatched_task_ids(tmp_path):
    result, summary = _run_closure_packet_verify_only(tmp_path, case="mismatched_task_ids")

    assert result.returncode != 0
    assert summary["status"] == "failed"
    assert summary["alignment"]["task_ids_match"] is False
    assert "task_id_alignment_mismatch" in summary["blockers"]


def test_pulse_task_board_closure_packet_rejects_failed_control_plane_readback(tmp_path):
    result, summary = _run_closure_packet_verify_only(tmp_path, case="control_plane_failed")

    assert result.returncode != 0
    assert summary["status"] == "failed"
    assert summary["checks"]["control_plane_fixture_consumer"]["status"] == "failed"
    assert "control_plane_readback_failed" in summary["blockers"]


def test_verification_docs_include_next_length_task_commands():
    verification = read("docs/VERIFICATION.md")
    for needle in [
        "npm run risky-pr:golden",
        "npm run smoke:evidence-control-plane",
        "npm run smoke:phase1-operator-golden",
        "npm run release:readiness",
        "ao2.risky-pr-golden-path.v1",
        "ao2.phase1-operator-golden-path-smoke.v1",
        "ao2.release-readiness-local.v1",
    ]:
        assert needle in verification


def test_ci_runs_python_guard_tests_and_pulse_docs_reference_persistent_local_mirror():
    ci = read(".github/workflows/ci.yml")
    verification = read("docs/VERIFICATION.md")

    assert "phase: python-guard-tests" in ci
    assert (
        "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest "
        "tests/test_public_stabilization.py "
        "tests/test_phase1_promote_wrapper.py -q"
    ) in ci
    assert ".ao2-local/pulse/" in verification
    assert "cargo clean" in verification
    assert "target/pulse-next-recommended-tasks" in verification


def test_pulse_local_mirror_script_is_exposed_and_ignored():
    package_json = json.loads(read("package.json"))
    gitignore = read(".gitignore")
    script = REPO_ROOT / "scripts" / "pulse-local-mirror.sh"

    assert (
        package_json["scripts"]["pulse:local-mirror"]
        == "node scripts/run-sh-script.js scripts/pulse-local-mirror.sh"
    )
    assert "/.ao2-local/" in gitignore
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    assert "target/pulse-next-recommended-tasks" in text
    assert ".ao2-local/pulse" in text
    assert "pulse-eval-loop.json" in text
    assert "executor-evidence.json" in text
    assert "shasum -a 256" in text
    assert 'SOURCE="$(cd "$SOURCE" && pwd -P)"' in text
    assert 'DEST="$(cd "$DEST" && pwd -P)"' in text
    assert "ao2.pulse-local-mirror.v1" in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text


def test_ci_uploads_guard_and_release_readiness_artifacts():
    package_json = json.loads(read("package.json"))
    ci = read(".github/workflows/ci.yml")

    assert (
        package_json["scripts"]["release:readiness:regression-gate"]
        == "node scripts/run-sh-script.js scripts/release-readiness-regression-gate.sh"
    )
    assert "scripts/ci-python-guard-artifacts.sh" in ci
    assert "Upload Python guard artifacts" in ci
    assert "ao2-python-guard-${{ matrix.os }}" in ci
    assert "target/ci-artifacts/python-guard-tests" in ci
    assert "release-readiness-artifacts" in ci
    assert "Upload release-readiness artifacts" in ci
    assert "ao2-release-readiness" in ci
    assert "target/release-readiness-ci" in ci

    for script_name in [
        "ci-python-guard-artifacts.sh",
        "release-readiness-regression-gate.sh",
    ]:
        script = REPO_ROOT / "scripts" / script_name
        assert script.is_file()
        assert script.stat().st_mode & stat.S_IXUSR


def test_ci_cargo_retry_wrapper_is_used_for_matrix_commands():
    ci = read(".github/workflows/ci.yml")
    script = REPO_ROOT / "scripts" / "ci-cargo-retry.sh"

    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    assert 'scripts/ci-cargo-retry.sh "${{ matrix.phase }}" <<\'AO2_CI_COMMAND\'' in ci
    assert "${{ matrix.command }}" in ci
    assert "run: ${{ matrix.command }}" not in ci

    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.ci-cargo-retry.v1",
        "AO2_CI_CARGO_RETRY_MAX_ATTEMPTS",
        "Connection reset by peer",
        "Broken pipe",
        "failed to get",
        "download of",
        "summary.json",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text


def test_ci_cargo_retry_wrapper_retries_transient_cargo_network_failures(tmp_path):
    attempts = tmp_path / "attempts"
    flaky = tmp_path / "flaky.sh"
    out_root = tmp_path / "retry-out"
    flaky.write_text(
        "#!/usr/bin/env bash\n"
        "set -euo pipefail\n"
        "count=0\n"
        "if [ -f \"$1\" ]; then count=$(cat \"$1\"); fi\n"
        "count=$((count + 1))\n"
        "printf '%s' \"$count\" > \"$1\"\n"
        "if [ \"$count\" -lt 2 ]; then\n"
        "  echo 'error: failed to get `wasm-bindgen` as a dependency' >&2\n"
        "  echo 'Caused by: [56] Failure when receiving data from the peer (Recv failure: Connection reset by peer)' >&2\n"
        "  exit 101\n"
        "fi\n",
        encoding="utf-8",
    )
    flaky.chmod(flaky.stat().st_mode | stat.S_IXUSR)

    result = subprocess.run(
        ["bash", "scripts/ci-cargo-retry.sh", "pytest-transient"],
        cwd=REPO_ROOT,
        input=f"bash {flaky} {attempts}\n",
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_CI_CARGO_RETRY_ROOT": str(out_root),
            "AO2_CI_CARGO_RETRY_SLEEP_SECONDS": "0",
        },
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    assert attempts.read_text(encoding="utf-8") == "2"
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.ci-cargo-retry.v1"
    assert summary["status"] == "passed"
    assert summary["attempts"] == 2
    assert summary["retried"] is True


def test_ci_cargo_retry_wrapper_does_not_retry_non_transient_failures(tmp_path):
    attempts = tmp_path / "attempts"
    out_root = tmp_path / "retry-out"

    result = subprocess.run(
        ["bash", "scripts/ci-cargo-retry.sh", "pytest-hard-failure"],
        cwd=REPO_ROOT,
        input=(
            f"count=0; if [ -f {attempts} ]; then count=$(cat {attempts}); fi; "
            f"count=$((count + 1)); printf '%s' \"$count\" > {attempts}; "
            "echo 'test assertion failed' >&2; exit 101\n"
        ),
        text=True,
        capture_output=True,
        env={
            **os.environ,
            "AO2_CI_CARGO_RETRY_ROOT": str(out_root),
            "AO2_CI_CARGO_RETRY_SLEEP_SECONDS": "0",
        },
        check=False,
    )

    assert result.returncode == 101
    assert attempts.read_text(encoding="utf-8") == "1"
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "failed"
    assert summary["attempts"] == 1
    assert summary["retried"] is False
    assert summary["transient_failure_detected"] is False


def test_phase1_operator_support_bundle_smoke_contract():
    readback_script = read("scripts/smoke-phase1-control-plane-readback.sh")
    golden_script = read("scripts/smoke-phase1-operator-golden-path.sh")

    for endpoint in [
        "/api/v1/phase1/promotion/operator-support-bundle.json",
        "/api/v1/phase1/promotion/operator-support-bundle/download",
        "/api/v1/phase1/promotion/operator-support-bundle/SHA256SUMS",
        "/api/v1/phase1/promotion/operator-support-bundle/verify",
        "/api/v1/phase1/promotion/operator-support-bundle/verify.json",
    ]:
        assert endpoint in readback_script

    for needle in [
        "operator_support_bundle",
        "ao2.cp-phase1-operator-support-bundle.v1",
        "ao2.cp-phase1-operator-support-bundle-verification.v1",
        "ao2.cp-phase1-operator-support-bundle-checksums.v1",
    ]:
        assert needle in readback_script
        assert needle in golden_script


def test_release_readiness_regression_gate_contract():
    package_json = json.loads(read("package.json"))
    script = REPO_ROOT / "scripts" / "release-readiness-regression-gate.sh"

    assert (
        package_json["scripts"]["release:readiness:regression-gate"]
        == "node scripts/run-sh-script.js scripts/release-readiness-regression-gate.sh"
    )
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR

    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.release-readiness-regression-gate.v1",
        "release:readiness:static",
        "smoke:phase1-operator-golden",
        "pulse:local-mirror",
        "../ao2-control-plane/scripts/smoke-long-lived-dev.sh",
        "control_plane_long_lived_smoke",
        "phase1_operator_golden",
        "pulse_local_mirror",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text


def test_pulse_local_mirror_resume_command_contract():
    text = read("scripts/pulse-local-mirror.sh")

    for needle in [
        "resume.json",
        "resume-command.sh",
        "pulse_eval_loop_sha256",
        "pulse_eval_loop_path",
        "ao2 pulse eval-loop run --chain",
        "ao2.pulse-local-mirror-resume.v1",
    ]:
        assert needle in text


def test_pulse_auto_advance_prompt_registration_contract():
    package_json = json.loads(read("package.json"))
    mirror = read("scripts/pulse-local-mirror.sh")
    resume = read("scripts/pulse-resume.sh")
    register = read("scripts/pulse-register-auto-advance.sh")
    generator = read("scripts/pulse-generate-next.sh")
    verification = read("docs/VERIFICATION.md")
    prompt = (
        "After each task batch, re-evaluate AO2 and ao2-control-plane at project level. "
        "Choose next tasks by highest long-term value, not similarity to last tasks. "
        "Prefer the Risky PR Run MVP product loop, local run record, static "
        "report/export, evaluator closure evidence, public reliability, "
        "Ubuntu/macOS/Windows correctness, CI confidence, evidence quality, "
        "security/safety boundaries, control-plane integration, release readiness, "
        "and developer/operator usability. Do not create new shell wrappers unless "
        "they directly unlock a product-slice or release-readiness bottleneck. "
        "Avoid narrow recursion or low-value daemon work unless it is the bottleneck. "
        "Generate next lengthy tasks with rationale, required evidence, and stop "
        "conditions, then register and continue through the AO2 event loop."
    )

    assert (
        package_json["scripts"]["pulse:register-auto-advance"]
        == "node scripts/run-sh-script.js scripts/pulse-register-auto-advance.sh"
    )
    register_path = REPO_ROOT / "scripts" / "pulse-register-auto-advance.sh"
    assert register_path.is_file()
    assert register_path.stat().st_mode & stat.S_IXUSR

    for text in [mirror, resume, register, generator]:
        assert prompt in text
        assert "OPENAI_API_KEY" not in text
        assert "ANTHROPIC_API_KEY" not in text
        assert "git push origin" not in text
        assert "gh release create" not in text

    for needle in [
        "AO2_PULSE_AUTO_ADVANCE_PROMPT",
        "operator-prompt.txt",
        "operator_prompt_sha256",
        "auto_advance",
        "registered_once",
        "continue_until_stopped",
        "stop_signal",
        "stores_credentials",
    ]:
        assert needle in mirror

    for needle in [
        "operator_prompt_sha256_matches",
        "operator_prompt_path",
        "auto_advance",
        "operator_prompt",
    ]:
        assert needle in resume

    for needle in [
        "ao2.pulse-auto-advance-registration.v1",
        "pulse:local-mirror",
        "resume.json",
        "auto_advance",
        "operator_prompt_sha256",
        "trust_boundary",
    ]:
        assert needle in register

    for needle in [
        "npm run pulse:register-auto-advance",
        "ao2.pulse-auto-advance-registration.v1",
        "target/pulse-auto-advance-registration/latest/summary.json",
    ]:
        assert needle in verification


def test_pulse_auto_advance_runner_restart_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    expected_scripts = {
        "pulse:auto-advance": "scripts/pulse-auto-advance.sh",
        "pulse:direct-main-publish": "scripts/pulse-direct-main-publish.sh",
        "pulse:direct-main-publish:contract": "scripts/pulse-direct-main-publish-contract.sh",
        "pulse:resume-workspace-cli-fallback": "scripts/pulse-resume-workspace-cli-fallback.sh",
        "pulse:terminal-eval-loop-schema-compatibility": "scripts/pulse-terminal-eval-loop-schema-compatibility.sh",
        "pulse:auto-advance-runner-contract": "scripts/pulse-auto-advance-runner-contract.sh",
        "pulse:stop-and-dedup-ledger": "scripts/pulse-stop-and-dedup-ledger.sh",
        "pulse:auto-advance-integration-gate": "scripts/pulse-auto-advance-integration-gate.sh",
        "pulse:pr-ci-gate:update": "scripts/pulse-pr-ci-gate-update.sh",
    }

    for command, script_path in expected_scripts.items():
        assert package_json["scripts"][command] == f"node scripts/run-sh-script.js {script_path}"
        script = REPO_ROOT / script_path
        assert script.is_file()
        assert script.stat().st_mode & stat.S_IXUSR
        text = script.read_text(encoding="utf-8")
        assert "OPENAI_API_KEY" not in text
        assert "ANTHROPIC_API_KEY" not in text
        assert "git push origin" not in text
        assert "gh release create" not in text
        assert "stores_credentials" in text

    runner = read("scripts/pulse-auto-advance.sh")
    for needle in [
        "ao2.pulse-auto-advance-run.v1",
        "ao2.pulse-auto-advance-heartbeat.v1",
        "AO2_PULSE_AUTO_ADVANCE_STOP_FILE",
        ".ao2-local/pulse/STOP",
        "pulse-auto-advance-ledger.jsonl",
        "operator_prompt_sha256",
        "recommended_tasks",
        "pulse-task-manifest.json",
        "pulse:task-executor",
        "AO2_PULSE_TASK_EXECUTOR_MANIFEST",
        "duplicate_eval_loop_digest",
        "waiting_for_new_eval_loop_digest",
        "continue_until_stopped",
        "--forever",
        "MAX_ITERATIONS=0",
        "sleep_seconds",
        "max_iterations",
        "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_STATE",
        ".ao2-local/pulse/pr-ci-gate.json",
        "ao2.pulse-pr-ci-gate.v1",
        "waiting_for_pr_merge_or_ci",
        "required_checks",
        "pr_ci_gate",
        "pulse:pr-ci-gate:update",
        "AO2_PULSE_PR_CI_GATE_UPDATE_STATE",
        "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH",
        "pulse:direct-main-publish",
        "direct_main_publish",
    ]:
        assert needle in runner

    script_needles = {
        "scripts/pulse-direct-main-publish.sh": [
            "ao2.pulse-direct-main-publish.v1",
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_REPO_ROOT",
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_VERIFY_COMMAND",
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_PUSH",
            "git push",
            "stores_credentials",
        ],
        "scripts/pulse-direct-main-publish-contract.sh": [
            "ao2.pulse-direct-main-publish-contract.v1",
            "bash -n scripts/pulse-direct-main-publish.sh",
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_VERIFY_COMMAND",
            "stores_credentials",
        ],
        "scripts/pulse-pr-ci-gate-update.sh": [
            "ao2.pulse-pr-ci-gate-update.v1",
            "ao2.pulse-pr-ci-gate.v1",
            "AO2_PULSE_PR_CI_GATE_UPDATE_STATE",
            ".ao2-local/pulse/pr-ci-gate.json",
            "gh pr view",
            "required_checks",
            "stores_credentials",
        ],
        "scripts/pulse-resume-workspace-cli-fallback.sh": [
            "ao2.pulse-resume-workspace-cli-fallback.v1",
            "cargo run -q -p ao2-cli -- pulse eval-loop run --help",
            "global_ao2_supports_pulse",
            "workspace_cli_supports_pulse",
        ],
        "scripts/pulse-terminal-eval-loop-schema-compatibility.sh": [
            "ao2.pulse-terminal-eval-loop-schema-compatibility.v1",
            "ready_for_next_pulse_task",
            "recommendation_only",
            "terminal",
            "fixed_interval_loop_successor",
        ],
        "scripts/pulse-auto-advance-runner-contract.sh": [
            "ao2.pulse-auto-advance-runner-contract.v1",
            "pulse:auto-advance",
            "bash -n scripts/pulse-auto-advance.sh",
            "recommended_tasks",
            "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH",
        ],
        "scripts/pulse-stop-and-dedup-ledger.sh": [
            "ao2.pulse-stop-and-dedup-ledger.v1",
            "AO2_PULSE_AUTO_ADVANCE_STOP_FILE",
            "duplicate_eval_loop_digest",
            "pulse-auto-advance-ledger.jsonl",
        ],
        "scripts/pulse-auto-advance-integration-gate.sh": [
            "ao2.pulse-auto-advance-integration-gate.v1",
            "pulse:pr-ci-gate:update",
            "pulse:resume-workspace-cli-fallback",
            "pulse:terminal-eval-loop-schema-compatibility",
            "pulse:auto-advance-runner-contract",
            "pulse:stop-and-dedup-ledger",
        ],
    }
    for script_path, needles in script_needles.items():
        text = read(script_path)
        for needle in needles:
            assert needle in text

    for needle in [
        "npm run pulse:auto-advance",
        "npm run pulse:auto-advance -- --forever",
        "npm run pulse:direct-main-publish",
        "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH=1",
        "npm run pulse:resume-workspace-cli-fallback",
        "npm run pulse:terminal-eval-loop-schema-compatibility",
        "npm run pulse:auto-advance-runner-contract",
        "npm run pulse:stop-and-dedup-ledger",
        "npm run pulse:auto-advance-integration-gate",
        "ao2.pulse-auto-advance-run.v1",
        "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_STATE",
        "ao2.pulse-pr-ci-gate.v1",
        "waiting_for_pr_merge_or_ci",
        "npm run pulse:pr-ci-gate:update",
        "ao2.pulse-pr-ci-gate-update.v1",
        "target/pulse-auto-advance/latest/summary.json",
    ]:
        assert needle in verification


def test_pulse_direct_main_publish_commits_and_pushes_temp_repo(tmp_path):
    repo = tmp_path / "repo"
    remote = tmp_path / "origin.git"
    out_root = tmp_path / "publish"
    subprocess.run(["git", "init", "-b", "main", str(repo)], check=True, capture_output=True, text=True)
    subprocess.run(["git", "init", "--bare", str(remote)], check=True, capture_output=True, text=True)
    subprocess.run(["git", "config", "user.email", "pulse@example.invalid"], cwd=repo, check=True)
    subprocess.run(["git", "config", "user.name", "Pulse Test"], cwd=repo, check=True)
    subprocess.run(["git", "remote", "add", "origin", str(remote)], cwd=repo, check=True)
    (repo / "README.md").write_text("initial\n", encoding="utf-8")
    subprocess.run(["git", "add", "README.md"], cwd=repo, check=True)
    subprocess.run(["git", "commit", "-m", "initial"], cwd=repo, check=True, capture_output=True, text=True)
    subprocess.run(["git", "push", "-u", "origin", "main"], cwd=repo, check=True, capture_output=True, text=True)
    (repo / "README.md").write_text("initial\npulse advancement\n", encoding="utf-8")

    result = subprocess.run(
        ["npm", "run", "pulse:direct-main-publish"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_REPO_ROOT": str(repo),
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_ROOT": str(out_root),
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_VERIFY_COMMAND": "python3 -c 'print(\"verify ok\")'",
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_MESSAGE": "Pulse direct main publish test",
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-direct-main-publish.v1"
    assert summary["status"] == "passed"
    assert summary["reason"] == "committed_and_pushed"
    assert summary["branch"] == "main"
    assert summary["changed_paths"] == ["README.md"]
    assert summary["verification"]["status"] == "passed"
    assert summary["commit"]["sha"]
    remote_head = subprocess.check_output(["git", "--git-dir", str(remote), "rev-parse", "main"], text=True).strip()
    assert remote_head == summary["commit"]["sha"]


def test_pulse_direct_main_publish_forces_recursive_pulse_env_off_during_verification(tmp_path):
    repo = tmp_path / "repo"
    remote = tmp_path / "origin.git"
    out_root = tmp_path / "publish"
    env_capture = tmp_path / "verification-env.json"
    verify_script = tmp_path / "verify_env.py"
    subprocess.run(["git", "init", "-b", "main", str(repo)], check=True, capture_output=True, text=True)
    subprocess.run(["git", "init", "--bare", str(remote)], check=True, capture_output=True, text=True)
    subprocess.run(["git", "config", "user.email", "pulse@example.invalid"], cwd=repo, check=True)
    subprocess.run(["git", "config", "user.name", "Pulse Test"], cwd=repo, check=True)
    subprocess.run(["git", "remote", "add", "origin", str(remote)], cwd=repo, check=True)
    (repo / "README.md").write_text("initial\n", encoding="utf-8")
    subprocess.run(["git", "add", "README.md"], cwd=repo, check=True)
    subprocess.run(["git", "commit", "-m", "initial"], cwd=repo, check=True, capture_output=True, text=True)
    subprocess.run(["git", "push", "-u", "origin", "main"], cwd=repo, check=True, capture_output=True, text=True)
    (repo / "README.md").write_text("initial\npulse advancement\n", encoding="utf-8")
    verify_script.write_text(
        "import json, os, pathlib, sys\n"
        f"path = pathlib.Path({str(env_capture)!r})\n"
        "keys = [\n"
        "    'AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH',\n"
        "    'AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED',\n"
        "    'AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT',\n"
        "    'AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY',\n"
        "]\n"
        "observed = {key: os.environ.get(key) for key in keys}\n"
        "path.write_text(json.dumps(observed, sort_keys=True) + '\\n', encoding='utf-8')\n"
        "if any(value != '0' for value in observed.values()):\n"
        "    print(json.dumps(observed, sort_keys=True), file=sys.stderr)\n"
        "    raise SystemExit(44)\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:direct-main-publish"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_REPO_ROOT": str(repo),
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_ROOT": str(out_root),
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_VERIFY_COMMAND": f"python3 {shlex.quote(str(verify_script))}",
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_MESSAGE": "Pulse direct main publish env isolation test",
            "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH": "1",
            "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED": "1",
            "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT": "1",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "1",
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    captured = json.loads(env_capture.read_text(encoding="utf-8"))
    assert captured == {
        "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH": "0",
        "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED": "0",
        "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT": "0",
        "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
    }
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    assert set(summary["verification"]["recursive_pulse_env_forced_off"]) == set(captured)


def test_pulse_direct_main_publish_skips_when_no_changes(tmp_path):
    repo = tmp_path / "repo"
    remote = tmp_path / "origin.git"
    out_root = tmp_path / "publish"
    subprocess.run(["git", "init", "-b", "main", str(repo)], check=True, capture_output=True, text=True)
    subprocess.run(["git", "init", "--bare", str(remote)], check=True, capture_output=True, text=True)
    subprocess.run(["git", "config", "user.email", "pulse@example.invalid"], cwd=repo, check=True)
    subprocess.run(["git", "config", "user.name", "Pulse Test"], cwd=repo, check=True)
    subprocess.run(["git", "remote", "add", "origin", str(remote)], cwd=repo, check=True)
    (repo / "README.md").write_text("initial\n", encoding="utf-8")
    subprocess.run(["git", "add", "README.md"], cwd=repo, check=True)
    subprocess.run(["git", "commit", "-m", "initial"], cwd=repo, check=True, capture_output=True, text=True)
    subprocess.run(["git", "push", "-u", "origin", "main"], cwd=repo, check=True, capture_output=True, text=True)

    result = subprocess.run(
        ["npm", "run", "pulse:direct-main-publish"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_REPO_ROOT": str(repo),
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_ROOT": str(out_root),
            "AO2_PULSE_DIRECT_MAIN_PUBLISH_VERIFY_COMMAND": "python3 -c 'print(\"verify ok\")'",
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "skipped"
    assert summary["reason"] == "no_tracked_or_untracked_changes"
    assert summary["changed_paths"] == []
    assert "verification" not in summary


def test_pulse_pr_ci_gate_update_materializes_waiting_state_from_fixture(tmp_path):
    out_root = tmp_path / "pr-ci-gate-update"
    state_path = tmp_path / "pr-ci-gate.json"
    source_json = tmp_path / "gh-pr-view.json"
    source_json.write_text(
        json.dumps(
            {
                "number": 55,
                "state": "OPEN",
                "isDraft": False,
                "headRefName": "codex/pulse-pr-ci-gate-updater",
                "mergeStateStatus": "BLOCKED",
                "url": "https://github.com/uesugitorachiyo/ao2/pull/55",
                "statusCheckRollup": [
                    {
                        "name": "Verify ubuntu-latest / fmt",
                        "state": "SUCCESS",
                        "conclusion": "SUCCESS",
                    },
                    {
                        "name": "Verify windows-latest / test-cli-non-approval",
                        "state": "PENDING",
                        "conclusion": None,
                    },
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:pr-ci-gate:update"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_PR_CI_GATE_UPDATE_ROOT": str(out_root),
            "AO2_PULSE_PR_CI_GATE_UPDATE_STATE": str(state_path),
            "AO2_PULSE_PR_CI_GATE_UPDATE_SOURCE_JSON": str(source_json),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    state = json.loads(state_path.read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-pr-ci-gate-update.v1"
    assert summary["status"] == "waiting"
    assert summary["state_path"] == str(state_path)
    assert state["schema_version"] == "ao2.pulse-pr-ci-gate.v1"
    assert state["status"] == "waiting"
    assert state["reason"] == "waiting_for_pr_merge_or_ci"
    assert state["branch"] == "codex/pulse-pr-ci-gate-updater"
    assert state["pr"] == {
        "number": 55,
        "state": "OPEN",
        "is_draft": False,
        "url": "https://github.com/uesugitorachiyo/ao2/pull/55",
    }
    assert state["required_checks"][0]["status"] == "SUCCESS"
    assert state["required_checks"][1]["status"] == "PENDING"
    assert state["trust_boundary"] == {"local_only": True, "stores_credentials": False}


def test_pulse_auto_advance_delegates_structured_manifest_to_task_executor(tmp_path):
    pulse_dir = tmp_path / "pulse"
    out_root = tmp_path / "auto-advance"
    ledger = tmp_path / "ledger.jsonl"
    stop_file = tmp_path / "STOP"
    pulse_dir.mkdir()

    eval_loop = {
        "schema_version": "ao2.pulse-eval-loop.v1",
        "status": "ready",
        "recommended_tasks": [
            {
                "id": "risky-pr-report-surface",
                "kind": "product_code",
                "title": "Risky PR report surface",
            }
        ],
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    manifest = {
        "schema_version": "ao2.pulse-task-manifest.v1",
        "trust_boundary": {
            "local_only": True,
            "stores_credentials": False,
            "side_effects": "local_process_execution_and_packet_materialization",
        },
        "tasks": [
            {
                "id": "risky-pr-report-surface",
                "kind": "product_code",
                "title": "Risky PR report surface",
                "objective": "Expose local run record and evaluator closure evidence in the report surface.",
                "files": ["crates/ao2-cli/src/main.rs"],
                "acceptance": ["Report links local run evidence."],
                "verification": [
                    {
                        "command": "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q",
                        "expected_evidence": "pytest.tests.test_public_stabilization",
                    }
                ],
                "stop_conditions": ["Stop if provider API keys are required."],
            }
        ],
    }
    eval_loop_path = pulse_dir / "pulse-eval-loop.json"
    manifest_path = pulse_dir / "pulse-task-manifest.json"
    prompt_path = pulse_dir / "operator-prompt.txt"
    eval_loop_path.write_text(json.dumps(eval_loop, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    prompt_path.write_text("advance product-code tasks\n", encoding="utf-8")
    resume = {
        "schema_version": "ao2.pulse-local-mirror-resume.v1",
        "status": "ready",
        "pulse_eval_loop_path": "pulse-eval-loop.json",
        "pulse_eval_loop_sha256": __import__("hashlib").sha256(eval_loop_path.read_bytes()).hexdigest(),
        "operator_prompt_path": "operator-prompt.txt",
        "operator_prompt_sha256": __import__("hashlib").sha256(prompt_path.read_bytes()).hexdigest(),
        "auto_advance": {"continue_until_stopped": True, "stores_credentials": False},
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    resume_path = pulse_dir / "resume.json"
    resume_path.write_text(json.dumps(resume, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    result = subprocess.run(
        ["npm", "run", "pulse:auto-advance"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_RESUME_JSON": str(resume_path),
            "AO2_PULSE_AUTO_ADVANCE_ROOT": str(out_root),
            "AO2_PULSE_AUTO_ADVANCE_LEDGER": str(ledger),
            "AO2_PULSE_AUTO_ADVANCE_STOP_FILE": str(stop_file),
            "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT": "0",
            "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED": "0",
            "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH": "0",
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    assert summary["task_execution_mode"] == "structured_manifest"
    assert summary["pulse_task_manifest_path"] == str(manifest_path.resolve())
    assert summary["results"][0]["id"] == "pulse-task-executor"
    assert summary["results"][0]["status"] == "passed"
    executor_summary = Path(summary["results"][0]["summary"])
    assert executor_summary.is_file()
    executor = json.loads(executor_summary.read_text(encoding="utf-8"))
    assert executor["counts"]["product_code"] == 1
    assert (executor_summary.parent / "implementation-packets" / "risky-pr-report-surface.md").is_file()


def test_pulse_auto_advance_keeps_structured_manifest_mode_if_manifest_is_rewritten(tmp_path):
    pulse_dir = tmp_path / "pulse"
    out_root = tmp_path / "auto-advance"
    ledger = tmp_path / "ledger.jsonl"
    stop_file = tmp_path / "STOP"
    pulse_dir.mkdir()

    eval_loop = {
        "schema_version": "ao2.pulse-eval-loop.v1",
        "status": "ready",
        "recommended_tasks": [
            {
                "id": "manifest-rewrite",
                "kind": "evidence_gate",
                "title": "Manifest rewrite simulation",
            }
        ],
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    manifest = {
        "schema_version": "ao2.pulse-task-manifest.v1",
        "trust_boundary": {
            "local_only": True,
            "stores_credentials": False,
            "side_effects": "local_process_execution_and_packet_materialization",
        },
        "tasks": [
            {
                "id": "manifest-rewrite",
                "kind": "evidence_gate",
                "title": "Manifest rewrite simulation",
                "command": "python3 -c \"import os, pathlib; pathlib.Path(os.environ['AO2_PULSE_TASK_EXECUTOR_MANIFEST']).unlink()\"",
                "expected_evidence": "manifest.removed.after.executor.start",
            }
        ],
    }
    eval_loop_path = pulse_dir / "pulse-eval-loop.json"
    manifest_path = pulse_dir / "pulse-task-manifest.json"
    prompt_path = pulse_dir / "operator-prompt.txt"
    eval_loop_path.write_text(json.dumps(eval_loop, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    prompt_path.write_text("advance product-code tasks\n", encoding="utf-8")
    resume = {
        "schema_version": "ao2.pulse-local-mirror-resume.v1",
        "status": "ready",
        "pulse_eval_loop_path": "pulse-eval-loop.json",
        "pulse_eval_loop_sha256": __import__("hashlib").sha256(eval_loop_path.read_bytes()).hexdigest(),
        "operator_prompt_path": "operator-prompt.txt",
        "operator_prompt_sha256": __import__("hashlib").sha256(prompt_path.read_bytes()).hexdigest(),
        "auto_advance": {"continue_until_stopped": True, "stores_credentials": False},
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    resume_path = pulse_dir / "resume.json"
    resume_path.write_text(json.dumps(resume, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    result = subprocess.run(
        ["npm", "run", "pulse:auto-advance"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_RESUME_JSON": str(resume_path),
            "AO2_PULSE_AUTO_ADVANCE_ROOT": str(out_root),
            "AO2_PULSE_AUTO_ADVANCE_LEDGER": str(ledger),
            "AO2_PULSE_AUTO_ADVANCE_STOP_FILE": str(stop_file),
            "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT": "0",
            "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED": "0",
            "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH": "0",
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    assert summary["completed_iterations"] == 1
    assert summary["task_execution_mode"] == "structured_manifest"
    assert summary["results"][0]["status"] == "passed"
    assert not manifest_path.exists()


def test_pulse_auto_advance_forever_pauses_generate_next_when_pr_ci_gate_waits(tmp_path):
    pulse_dir = tmp_path / "pulse"
    out_root = tmp_path / "auto-advance"
    ledger = tmp_path / "ledger.jsonl"
    stop_file = tmp_path / "STOP"
    gate_state = tmp_path / "pr-ci-gate.json"
    pulse_dir.mkdir()

    eval_loop = {
        "schema_version": "ao2.pulse-eval-loop.v1",
        "status": "ready",
        "recommended_tasks": [
            {
                "id": "next-product-readiness-task",
                "kind": "evidence_gate",
                "title": "Next product readiness task",
                "command": "python3 -c 'print(\"would run only after PR/CI clears\")'",
            }
        ],
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    eval_loop_path = pulse_dir / "pulse-eval-loop.json"
    prompt_path = pulse_dir / "operator-prompt.txt"
    eval_loop_path.write_text(json.dumps(eval_loop, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    prompt_path.write_text("advance production readiness after PR/CI is green\n", encoding="utf-8")
    eval_loop_sha256 = __import__("hashlib").sha256(eval_loop_path.read_bytes()).hexdigest()
    resume = {
        "schema_version": "ao2.pulse-local-mirror-resume.v1",
        "status": "ready",
        "pulse_eval_loop_path": "pulse-eval-loop.json",
        "pulse_eval_loop_sha256": eval_loop_sha256,
        "operator_prompt_path": "operator-prompt.txt",
        "operator_prompt_sha256": __import__("hashlib").sha256(prompt_path.read_bytes()).hexdigest(),
        "auto_advance": {"continue_until_stopped": True, "stores_credentials": False},
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    resume_path = pulse_dir / "resume.json"
    resume_path.write_text(json.dumps(resume, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    ledger.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-auto-advance-ledger-entry.v1",
                "pulse_eval_loop_sha256": eval_loop_sha256,
                "status": "passed",
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    gate_state.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-pr-ci-gate.v1",
                "status": "waiting",
                "reason": "waiting_for_pr_merge_or_ci",
                "branch": "codex/example",
                "pr": {"number": 99, "state": "OPEN", "is_draft": False},
                "required_checks": [
                    {
                        "name": "Verify ubuntu-latest / test-cli-non-approval",
                        "status": "PENDING",
                    }
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:auto-advance", "--", "--forever", "--sleep-seconds", "0"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_RESUME_JSON": str(resume_path),
            "AO2_PULSE_AUTO_ADVANCE_ROOT": str(out_root),
            "AO2_PULSE_AUTO_ADVANCE_LEDGER": str(ledger),
            "AO2_PULSE_AUTO_ADVANCE_STOP_FILE": str(stop_file),
            "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_STATE": str(gate_state),
            "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_UPDATE": "0",
            "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT": "1",
            "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED": "0",
            "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH": "0",
            "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT_SLEEP_SECONDS": "0",
        },
        capture_output=True,
        text=True,
        check=False,
        timeout=10,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "waiting"
    assert summary["reason"] == "waiting_for_pr_merge_or_ci"
    assert summary["generated_next_packet"] is False
    assert summary["register_next_packet"] is False
    assert summary["pulse_generate_next"]["status"] == "skipped"
    assert summary["pr_ci_gate"]["schema_version"] == "ao2.pulse-pr-ci-gate.v1"
    assert summary["pr_ci_gate"]["status"] == "waiting"
    assert summary["pr_ci_gate"]["required_checks"][0]["status"] == "PENDING"
    assert not list((out_root / "logs").glob("pulse_generate_next-*.log"))


def test_pulse_auto_advance_can_generate_local_only_packet_when_pr_ci_gate_waits(tmp_path):
    pulse_dir = tmp_path / "pulse"
    out_root = tmp_path / "auto-advance"
    ledger = tmp_path / "ledger.jsonl"
    stop_file = tmp_path / "STOP"
    gate_state = tmp_path / "pr-ci-gate.json"
    bin_dir = tmp_path / "bin"
    env_capture = tmp_path / "generate-next-env.json"
    pulse_dir.mkdir()
    bin_dir.mkdir()

    fake_npm = bin_dir / "npm"
    fake_npm.write_text(
        "#!/usr/bin/env bash\n"
        "set -euo pipefail\n"
        "python3 - <<'PY'\n"
        "import json, os, pathlib\n"
        "pathlib.Path(os.environ['AO2_TEST_GENERATE_NEXT_ENV_CAPTURE']).write_text(json.dumps({\n"
        "    'local_only': os.environ.get('AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY'),\n"
        "}) + '\\n', encoding='utf-8')\n"
        "pathlib.Path(os.environ['AO2_PULSE_AUTO_ADVANCE_STOP_FILE']).write_text('stop after local-only generation\\n', encoding='utf-8')\n"
        "PY\n",
        encoding="utf-8",
    )
    fake_npm.chmod(0o755)
    real_npm = subprocess.check_output(["bash", "-lc", "command -v npm"], text=True).strip()

    eval_loop = {
        "schema_version": "ao2.pulse-eval-loop.v1",
        "status": "ready",
        "recommended_tasks": [
            {
                "id": "already-completed",
                "kind": "evidence_gate",
                "title": "Already completed",
                "command": "python3 -c 'print(\"already completed\")'",
            }
        ],
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    eval_loop_path = pulse_dir / "pulse-eval-loop.json"
    prompt_path = pulse_dir / "operator-prompt.txt"
    eval_loop_path.write_text(json.dumps(eval_loop, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    prompt_path.write_text("advance local-only evidence while PR waits\n", encoding="utf-8")
    eval_loop_sha256 = __import__("hashlib").sha256(eval_loop_path.read_bytes()).hexdigest()
    resume = {
        "schema_version": "ao2.pulse-local-mirror-resume.v1",
        "status": "ready",
        "pulse_eval_loop_path": "pulse-eval-loop.json",
        "pulse_eval_loop_sha256": eval_loop_sha256,
        "operator_prompt_path": "operator-prompt.txt",
        "operator_prompt_sha256": __import__("hashlib").sha256(prompt_path.read_bytes()).hexdigest(),
        "auto_advance": {"continue_until_stopped": True, "stores_credentials": False},
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    resume_path = pulse_dir / "resume.json"
    resume_path.write_text(json.dumps(resume, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    ledger.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-auto-advance-ledger-entry.v1",
                "pulse_eval_loop_sha256": eval_loop_sha256,
                "status": "passed",
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    gate_state.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-pr-ci-gate.v1",
                "status": "waiting",
                "reason": "waiting_for_pr_merge_or_ci",
                "branch": "codex/example",
                "pr": {"number": 99, "state": "OPEN", "is_draft": False},
                "required_checks": [
                    {
                        "name": "Verify ubuntu-latest / test-cli-non-approval",
                        "status": "PENDING",
                    }
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        [real_npm, "run", "pulse:auto-advance", "--", "--forever", "--sleep-seconds", "0"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
            "AO2_TEST_GENERATE_NEXT_ENV_CAPTURE": str(env_capture),
            "AO2_PULSE_RESUME_JSON": str(resume_path),
            "AO2_PULSE_AUTO_ADVANCE_ROOT": str(out_root),
            "AO2_PULSE_AUTO_ADVANCE_LEDGER": str(ledger),
            "AO2_PULSE_AUTO_ADVANCE_STOP_FILE": str(stop_file),
            "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_STATE": str(gate_state),
            "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_UPDATE": "0",
            "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT": "1",
            "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED": "1",
            "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH": "0",
            "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT_SLEEP_SECONDS": "0",
        },
        capture_output=True,
        text=True,
        check=False,
        timeout=10,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    captured = json.loads(env_capture.read_text(encoding="utf-8"))
    assert summary["status"] == "stopped"
    assert summary["pulse_generate_next"]["status"] == "passed"
    assert summary["pulse_generate_next"]["local_only_while_pr_blocked"] is True
    assert summary["generated_local_only_packet"] is True
    assert summary["register_next_packet"] is True
    assert captured["local_only"] == "1"


def test_pulse_auto_advance_invokes_direct_main_publish_when_enabled(tmp_path):
    pulse_dir = tmp_path / "pulse"
    out_root = tmp_path / "auto-advance"
    ledger = tmp_path / "ledger.jsonl"
    stop_file = tmp_path / "STOP"
    bin_dir = tmp_path / "bin"
    capture = tmp_path / "direct-main-publish.json"
    pulse_dir.mkdir()
    bin_dir.mkdir()

    fake_npm = bin_dir / "npm"
    fake_npm.write_text(
        "#!/usr/bin/env bash\n"
        "set -euo pipefail\n"
        "if [ \"${1:-}\" = \"run\" ] && [ \"${2:-}\" = \"pulse:direct-main-publish\" ]; then\n"
        "  python3 - <<'PY'\n"
        "import json, os, pathlib\n"
        "pathlib.Path(os.environ['AO2_TEST_DIRECT_MAIN_PUBLISH_CAPTURE']).write_text(json.dumps({\n"
        "  'direct_main_publish': True,\n"
        "  'reason': os.environ.get('AO2_PULSE_DIRECT_MAIN_PUBLISH_REASON'),\n"
        "}) + '\\n', encoding='utf-8')\n"
        "PY\n"
        "  exit 0\n"
        "fi\n"
        "echo unexpected npm invocation: \"$@\" >&2\n"
        "exit 97\n",
        encoding="utf-8",
    )
    fake_npm.chmod(0o755)
    real_npm = subprocess.check_output(["bash", "-lc", "command -v npm"], text=True).strip()

    eval_loop = {
        "schema_version": "ao2.pulse-eval-loop.v1",
        "status": "ready",
        "recommended_tasks": [
            {
                "id": "local-task",
                "kind": "evidence_gate",
                "title": "Local task",
                "command": "python3 -c 'print(\"local task passed\")'",
            }
        ],
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    eval_loop_path = pulse_dir / "pulse-eval-loop.json"
    prompt_path = pulse_dir / "operator-prompt.txt"
    eval_loop_path.write_text(json.dumps(eval_loop, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    prompt_path.write_text("advance and direct-main publish\n", encoding="utf-8")
    resume = {
        "schema_version": "ao2.pulse-local-mirror-resume.v1",
        "status": "ready",
        "pulse_eval_loop_path": "pulse-eval-loop.json",
        "pulse_eval_loop_sha256": __import__("hashlib").sha256(eval_loop_path.read_bytes()).hexdigest(),
        "operator_prompt_path": "operator-prompt.txt",
        "operator_prompt_sha256": __import__("hashlib").sha256(prompt_path.read_bytes()).hexdigest(),
        "auto_advance": {"continue_until_stopped": True, "stores_credentials": False},
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    resume_path = pulse_dir / "resume.json"
    resume_path.write_text(json.dumps(resume, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    result = subprocess.run(
        [real_npm, "run", "pulse:auto-advance"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
            "AO2_TEST_DIRECT_MAIN_PUBLISH_CAPTURE": str(capture),
            "AO2_PULSE_RESUME_JSON": str(resume_path),
            "AO2_PULSE_AUTO_ADVANCE_ROOT": str(out_root),
            "AO2_PULSE_AUTO_ADVANCE_LEDGER": str(ledger),
            "AO2_PULSE_AUTO_ADVANCE_STOP_FILE": str(stop_file),
            "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT": "0",
            "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED": "0",
            "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH": "1",
        },
        capture_output=True,
        text=True,
        check=False,
        timeout=10,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    captured = json.loads(capture.read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    assert summary["direct_main_publish"]["enabled"] is True
    assert summary["direct_main_publish"]["status"] == "passed"
    assert summary["direct_main_publish"]["command"] == "pulse:direct-main-publish"
    assert captured == {"direct_main_publish": True, "reason": "completed_iteration"}


def test_pulse_auto_advance_forever_refreshes_pr_ci_gate_before_generate_next(tmp_path):
    pulse_dir = tmp_path / "pulse"
    out_root = tmp_path / "auto-advance"
    ledger = tmp_path / "ledger.jsonl"
    stop_file = tmp_path / "STOP"
    gate_state = tmp_path / "pr-ci-gate.json"
    source_json = tmp_path / "gh-pr-view.json"
    pulse_dir.mkdir()

    eval_loop = {
        "schema_version": "ao2.pulse-eval-loop.v1",
        "status": "ready",
        "recommended_tasks": [
            {
                "id": "next-product-readiness-task",
                "kind": "evidence_gate",
                "title": "Next product readiness task",
                "command": "python3 -c 'print(\"advance after gate update\")'",
            }
        ],
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    eval_loop_path = pulse_dir / "pulse-eval-loop.json"
    prompt_path = pulse_dir / "operator-prompt.txt"
    eval_loop_path.write_text(json.dumps(eval_loop, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    prompt_path.write_text("advance production readiness after refreshed PR/CI gate\n", encoding="utf-8")
    eval_loop_sha256 = __import__("hashlib").sha256(eval_loop_path.read_bytes()).hexdigest()
    resume = {
        "schema_version": "ao2.pulse-local-mirror-resume.v1",
        "status": "ready",
        "pulse_eval_loop_path": "pulse-eval-loop.json",
        "pulse_eval_loop_sha256": eval_loop_sha256,
        "operator_prompt_path": "operator-prompt.txt",
        "operator_prompt_sha256": __import__("hashlib").sha256(prompt_path.read_bytes()).hexdigest(),
        "auto_advance": {"continue_until_stopped": True, "stores_credentials": False},
        "trust_boundary": {"local_only": True, "stores_credentials": False},
    }
    resume_path = pulse_dir / "resume.json"
    resume_path.write_text(json.dumps(resume, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    ledger.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-auto-advance-ledger-entry.v1",
                "pulse_eval_loop_sha256": eval_loop_sha256,
                "status": "passed",
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    source_json.write_text(
        json.dumps(
            {
                "number": 56,
                "state": "OPEN",
                "isDraft": False,
                "headRefName": "codex/pulse-pr-ci-gate-updater",
                "mergeStateStatus": "BLOCKED",
                "url": "https://github.com/uesugitorachiyo/ao2/pull/56",
                "statusCheckRollup": [
                    {
                        "name": "Verify macos-latest / test-cli-release-readiness",
                        "state": "IN_PROGRESS",
                        "conclusion": None,
                    }
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:auto-advance", "--", "--forever", "--sleep-seconds", "0"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_RESUME_JSON": str(resume_path),
            "AO2_PULSE_AUTO_ADVANCE_ROOT": str(out_root),
            "AO2_PULSE_AUTO_ADVANCE_LEDGER": str(ledger),
            "AO2_PULSE_AUTO_ADVANCE_STOP_FILE": str(stop_file),
            "AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_STATE": str(gate_state),
            "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT": "1",
            "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED": "0",
            "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH": "0",
            "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT_SLEEP_SECONDS": "0",
            "AO2_PULSE_PR_CI_GATE_UPDATE_SOURCE_JSON": str(source_json),
        },
        capture_output=True,
        text=True,
        check=False,
        timeout=10,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    state = json.loads(gate_state.read_text(encoding="utf-8"))
    assert summary["status"] == "waiting"
    assert summary["reason"] == "waiting_for_pr_merge_or_ci"
    assert summary["pr_ci_gate_update"]["status"] == "passed"
    assert summary["pr_ci_gate"]["status"] == "waiting"
    assert state["pr"]["number"] == 56
    assert state["required_checks"][0]["status"] == "PENDING"
    assert not list((out_root / "logs").glob("pulse_generate_next-*.log"))


def test_pulse_daemon_supervisor_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    expected_scripts = {
        "pulse:daemon:start": "node scripts/run-sh-script.js scripts/pulse-daemon.sh start",
        "pulse:daemon:status": "node scripts/run-sh-script.js scripts/pulse-daemon.sh status",
        "pulse:daemon:stop": "node scripts/run-sh-script.js scripts/pulse-daemon.sh stop",
        "pulse:daemon:restart": "node scripts/run-sh-script.js scripts/pulse-daemon.sh restart",
        "pulse:daemon:contract": "node scripts/run-sh-script.js scripts/pulse-daemon-contract.sh",
    }
    for command, expected in expected_scripts.items():
        assert package_json["scripts"][command] == expected

    for script_name, schema in [
        ("pulse-daemon.sh", "ao2.pulse-daemon.v1"),
        ("pulse-daemon-contract.sh", "ao2.pulse-daemon-contract.v1"),
    ]:
        script = REPO_ROOT / "scripts" / script_name
        assert script.is_file()
        assert script.stat().st_mode & stat.S_IXUSR
        text = script.read_text(encoding="utf-8")
        assert schema in text
        assert "OPENAI_API_KEY" not in text
        assert "ANTHROPIC_API_KEY" not in text
        assert "git push origin" not in text
        assert "gh release create" not in text
        assert "stores_credentials" in text

    daemon = read("scripts/pulse-daemon.sh")
    for needle in [
        "launchctl bootstrap",
        "launchctl print",
        "launchctl bootout",
        "launchctl kickstart",
        "tmux new-session",
        "tmux has-session",
        "tmux kill-session",
        "KeepAlive",
        "RunAtLoad",
        "pulse-auto-advance.sh",
        "--forever",
        "STOP",
        "heartbeat_summary",
        "active_backend",
        "process_alive",
    ]:
        assert needle in daemon

    for needle in [
        "npm run pulse:daemon:start",
        "npm run pulse:daemon:status",
        "npm run pulse:daemon:stop",
        "npm run pulse:daemon:restart",
        "npm run pulse:daemon:contract",
        "ao2.pulse-daemon.v1",
        "target/pulse-daemon/latest/summary.json",
    ]:
        assert needle in verification


def test_pulse_generate_next_auto_registration_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    expected_scripts = {
        "pulse:generate-next": "node scripts/run-sh-script.js scripts/pulse-generate-next.sh",
        "pulse:generate-next:contract": "node scripts/run-sh-script.js scripts/pulse-generate-next-contract.sh",
        "pulse:task-board-state": "node scripts/run-sh-script.js scripts/pulse-task-board-state.sh",
        "pulse:next-actions": "node scripts/run-sh-script.js scripts/pulse-next-actions.sh",
    }
    for command, expected in expected_scripts.items():
        assert package_json["scripts"][command] == expected

    for script_name, schema in [
        ("pulse-generate-next.sh", "ao2.pulse-generate-next.v1"),
        ("pulse-generate-next-contract.sh", "ao2.pulse-generate-next-contract.v1"),
        ("pulse-task-board-state.sh", "ao2.pulse-task-board-state.v1"),
        ("pulse-next-actions.sh", "ao2.pulse-next-actions.v1"),
    ]:
        script = REPO_ROOT / "scripts" / script_name
        assert script.is_file()
        assert script.stat().st_mode & stat.S_IXUSR
        text = script.read_text(encoding="utf-8")
        assert schema in text
        assert "OPENAI_API_KEY" not in text
        assert "ANTHROPIC_API_KEY" not in text
        assert "git push origin" not in text
        assert "gh release create" not in text
        assert "stores_credentials" in text

    generator = read("scripts/pulse-generate-next.sh")
    for needle in [
        "ao2.pulse-generate-next.v1",
        "ao2.pulse-next-lengthy-tasks.v1",
        "ao2.ai-task-board.v1",
        "AO2_PULSE_TASK_BOARD_ROOT",
        "task_board_summary",
        "control_plane_readback",
        "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY",
        "local_only_while_pr_blocked",
        "product_code",
        "Risky PR report/evaluator closure UX implementation",
        "cross-platform-compatibility",
        "Ubuntu macOS Windows compatibility evidence",
        "release:cross-os-attestation",
        "ao2.cross-os-release-attestation.v1",
        "pulse-eval-loop.json",
        "pulse-task-manifest.json",
        "ao2.pulse-task-manifest.v1",
        "codex-cron-event-loop-decision.json",
        "codex_cron_event_loop_decision",
        "codex-cron.event-loop-decision.v1",
        "ao2.pulse-codex-cron-event-loop-decision.v1",
        "product_code_execution",
        "packet.md",
        "board.md",
        "task-board.json",
        "ao2.ai-task-board.v1",
        "AO2_PULSE_TASK_BOARD_STATUS_EVIDENCE",
        "ao2.ai-task-board-status-evidence.v1",
        "status_transition_source",
        "stable_task_id",
        "stale_generation",
        "Status evidence ignored",
        "next_action",
        "changed_tasks",
        "field_changes",
        "release_objective",
        "control_plane_readback",
        "executor-evidence.json",
        "AO2_PULSE_GENERATE_NEXT_REGISTER",
        "pulse:register-auto-advance",
        "cursor",
        "recommended_tasks",
    ]:
        assert needle in generator

    runner = read("scripts/pulse-auto-advance.sh")
    for needle in [
        "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT",
        "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED",
        "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY",
        "generated_local_only_packet",
        "local_only_while_pr_blocked",
        "pulse_generate_next",
        "pulse:generate-next",
        "register_next_packet",
        "generated_next_packet",
    ]:
        assert needle in runner

    quality_filter = read("scripts/pulse-next-task-quality-filter.sh")
    for needle in [
        "AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE",
        "stable_task_id",
        "status_evidence_unknown_task_id",
        "status_evidence_stale_generation",
        "status_evidence_matches",
        "status_evidence_match_counts",
    ]:
        assert needle in quality_filter

    for needle in [
        "npm run pulse:generate-next",
        "npm run pulse:generate-next:contract",
        "npm run pulse:task-board-state",
        "npm run pulse:next-actions",
        "ao2.pulse-generate-next.v1",
        "ao2.pulse-task-board-state.v1",
        "ao2.pulse-next-actions.v1",
        "AO2_PULSE_NEXT_ACTIONS_STATUS",
        "ao2.ai-task-board.v1",
        "stable_task_id",
        "task-board.json",
        "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED=1",
        "local-only while PR-blocked mode",
        "target/pulse-generate-next/latest/summary.json",
        "codex-cron.event-loop-decision.v1",
        "ao2.pulse-codex-cron-event-loop-decision.v1",
    ]:
        assert needle in verification


def test_pulse_generate_next_writes_structured_task_manifest(tmp_path):
    out_root = tmp_path / "generate-next"
    packet_root = tmp_path / "packet"
    task_board_root = tmp_path / "task-board"
    cursor = tmp_path / "cursor.json"

    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(packet_root),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(cursor),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    manifest = json.loads((packet_root / "pulse-task-manifest.json").read_text(encoding="utf-8"))
    assert manifest["schema_version"] == "ao2.pulse-task-manifest.v1"
    assert manifest["product_code_execution"] == {"enabled": True, "mode": "dry_run"}
    assert manifest["trust_boundary"] == {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "local_process_execution_and_packet_materialization",
    }
    assert manifest["tasks"]
    assert any(task["kind"] == "product_code" for task in manifest["tasks"])
    assert any(task["kind"] == "evidence_gate" for task in manifest["tasks"])
    for task in manifest["tasks"]:
        if task["kind"] == "evidence_gate":
            assert task["command"].startswith("npm run ") or task["command"].startswith("PYTHONDONTWRITEBYTECODE=")
        if task["kind"] == "product_code":
            assert "command" not in task
            assert task["objective"]
            assert task["files"]
            assert task["acceptance"]
            assert task["verification"]
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert any(item["path"] == "pulse-task-manifest.json" for item in summary["files"])
    assert summary["task_board_summary"] == str(task_board_root / "summary.json")


def test_pulse_generate_next_writes_codex_cron_event_loop_decision(tmp_path):
    out_root = tmp_path / "generate-next"
    packet_root = tmp_path / "packet"
    task_board_root = tmp_path / "task-board"
    cursor = tmp_path / "cursor.json"

    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(packet_root),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(cursor),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    decision_path = packet_root / "codex-cron-event-loop-decision.json"
    decision = json.loads(decision_path.read_text(encoding="utf-8"))
    assert decision["schema_version"] == "codex-cron.event-loop-decision.v1"
    assert decision["event_loop"]["action"] == "continue"
    assert decision["event_loop"]["reason"]
    assert decision["event_loop"]["next_task_id"]
    assert decision["ao2"]["schema_version"] == "ao2.pulse-codex-cron-event-loop-decision.v1"
    assert decision["ao2"]["task_count"] > 0
    assert decision["ao2"]["task_board_summary"] == str(task_board_root / "summary.json")
    assert decision["ao2"]["trust_boundary"] == {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "local_artifact_materialization_only",
    }

    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    packet_summary = json.loads((packet_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["codex_cron_event_loop_decision"] == str(decision_path)
    assert packet_summary["codex_cron_event_loop_decision"] == str(decision_path)
    assert any(item["path"] == "codex-cron-event-loop-decision.json" for item in summary["files"])
    assert any(item["path"] == "codex-cron-event-loop-decision.json" for item in packet_summary["files"])


def test_pulse_generate_next_emits_ai_task_board_control_surface(tmp_path):
    out_root = tmp_path / "generate-next"
    packet_root = tmp_path / "packet"
    task_board_root = tmp_path / "task-board"
    cursor = tmp_path / "cursor.json"

    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(packet_root),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(cursor),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    board = json.loads((task_board_root / "summary.json").read_text(encoding="utf-8"))
    manifest = json.loads((packet_root / "pulse-task-manifest.json").read_text(encoding="utf-8"))

    assert board["schema_version"] == "ao2.ai-task-board.v1"
    assert board["status"] == "ready"
    assert board["release_train"] == {
        "version": "v0.4.81",
        "theme": "AI task board control surface",
    }
    assert board["release_objective"]
    assert board["source_recommendation"]["selection"] == manifest["selection"]
    assert board["control_plane_readback"] == {
        "role": "read_only_observer",
        "requires_credentials": False,
        "can_mutate_ao2_artifacts": False,
        "can_mutate_release_metadata": False,
    }
    assert board["trust_boundary"] == {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "local_artifact_materialization_only",
    }
    assert len(board["tasks"]) == len(manifest["tasks"])
    assert any(task["kind"] == "product_code" for task in board["tasks"])
    for task in board["tasks"]:
        assert task["task_id"]
        assert task["title"]
        assert task["status"] == "proposed"
        assert task["objective"]
        assert task["rationale"]
        assert task["required_evidence"]
        assert task["stop_conditions"]
        assert task["source_recommendation"] == board["source_recommendation"]


def test_pulse_generate_next_writes_operator_board_exports_with_status_lanes(tmp_path):
    out_root = tmp_path / "generate-next"
    packet_root = tmp_path / "packet"
    task_board_root = tmp_path / "task-board"
    cursor = tmp_path / "cursor.json"

    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(packet_root),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(cursor),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    board = json.loads((task_board_root / "summary.json").read_text(encoding="utf-8"))
    board_md = (task_board_root / "board.md").read_text(encoding="utf-8")
    board_html = (task_board_root / "board.html").read_text(encoding="utf-8")

    assert board["exports"]["markdown"] == str(task_board_root / "board.md")
    assert board["exports"]["html"] == str(task_board_root / "board.html")
    for task in board["tasks"]:
        assert task["next_action"]
    for needle in [
        "Status Lanes",
        "Proposed",
        "Product Code",
        "Evidence Gates",
        "Required Evidence",
        "Stop Conditions",
        "Next Action",
    ]:
        assert needle in board_md
    for needle in [
        "<!doctype html>",
        "AO2 AI Task Board",
        "status-lane",
        "Product Code",
        "Evidence Gates",
        "Next Action",
    ]:
        assert needle in board_html


def test_pulse_generate_next_applies_task_board_status_transitions_from_evidence(tmp_path):
    baseline_root = tmp_path / "baseline-generate-next"
    baseline_packet = tmp_path / "baseline-packet"
    baseline_board_root = tmp_path / "baseline-task-board"
    baseline_cursor = tmp_path / "baseline-cursor.json"
    status_evidence = tmp_path / "status-evidence.json"

    baseline_result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(baseline_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(baseline_packet),
            "AO2_PULSE_TASK_BOARD_ROOT": str(baseline_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(baseline_cursor),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert baseline_result.returncode == 0, baseline_result.stderr + baseline_result.stdout
    baseline_board = json.loads((baseline_board_root / "summary.json").read_text(encoding="utf-8"))
    first_task, second_task = baseline_board["tasks"][:2]
    status_evidence.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board-status-evidence.v1",
                "task_statuses": {
                    first_task["task_id"]: {
                        "status": "in_progress",
                        "status_reason": "Executor picked up this task from the generated packet.",
                        "evidence": ["target/pulse-task-executor/latest/summary.json"],
                    },
                    second_task["task_id"]: {
                        "status": "blocked",
                        "status_reason": "Required closure evidence has not landed yet.",
                        "evidence": ["target/pulse-next-task-quality-filter/latest/summary.json"],
                    },
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    out_root = tmp_path / "generate-next"
    packet_root = tmp_path / "packet"
    task_board_root = tmp_path / "task-board"
    cursor = tmp_path / "cursor.json"
    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(packet_root),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(cursor),
            "AO2_PULSE_TASK_BOARD_STATUS_EVIDENCE": str(status_evidence),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    board = json.loads((task_board_root / "summary.json").read_text(encoding="utf-8"))
    board_md = (task_board_root / "board.md").read_text(encoding="utf-8")
    board_html = (task_board_root / "board.html").read_text(encoding="utf-8")
    tasks_by_id = {task["task_id"]: task for task in board["tasks"]}

    assert board["status_transition_source"]["schema_version"] == (
        "ao2.ai-task-board-status-evidence.v1"
    )
    assert board["status_transition_source"]["updates_applied"] == 2
    assert tasks_by_id[first_task["task_id"]]["status"] == "in_progress"
    assert tasks_by_id[first_task["task_id"]]["status_transition"] == {
        "source": str(status_evidence.resolve()),
        "previous_status": "proposed",
        "current_status": "in_progress",
        "reason": "Executor picked up this task from the generated packet.",
        "matched_by": "task_id",
        "evidence": ["target/pulse-task-executor/latest/summary.json"],
    }
    assert tasks_by_id[second_task["task_id"]]["status"] == "blocked"
    assert "In Progress" in board_md
    assert "Blocked" in board_md
    assert "status-in_progress" in board_html
    assert "status-blocked" in board_html


def test_pulse_generate_next_auto_discovers_executor_status_evidence(tmp_path):
    baseline_root = tmp_path / "baseline-generate-next"
    baseline_packet = tmp_path / "baseline-packet"
    baseline_board_root = tmp_path / "baseline-task-board"
    executor_root = tmp_path / "executor"

    baseline_result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(baseline_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(baseline_packet),
            "AO2_PULSE_TASK_BOARD_ROOT": str(baseline_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "baseline-cursor.json"),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(executor_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert baseline_result.returncode == 0, baseline_result.stderr + baseline_result.stdout
    baseline_board = json.loads((baseline_board_root / "summary.json").read_text(encoding="utf-8"))
    first_task, second_task = baseline_board["tasks"][:2]
    executor_root.mkdir(parents=True)
    (executor_root / "task-board-status-evidence.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board-status-evidence.v1",
                "status": "ready",
                "source": "ao2.pulse-task-executor.v1",
                "task_board_generation": baseline_board["source_recommendation"]["generation"],
                "task_statuses": {
                    first_task["task_id"]: {
                        "status": "ready",
                        "status_reason": "Executor materialized this implementation packet.",
                        "evidence": [str(executor_root / "summary.json")],
                    },
                    second_task["task_id"]: {
                        "status": "passed",
                        "status_reason": "Executor completed this evidence gate.",
                        "evidence": [str(executor_root / "summary.json")],
                    },
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    out_root = tmp_path / "generate-next"
    task_board_root = tmp_path / "task-board"
    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(tmp_path / "packet"),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "cursor.json"),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(executor_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    board = json.loads((task_board_root / "summary.json").read_text(encoding="utf-8"))
    tasks_by_id = {task["task_id"]: task for task in board["tasks"]}
    assert board["status_transition_source"]["path"] == str(
        executor_root / "task-board-status-evidence.json"
    )
    assert board["status_transition_source"]["status"] == "applied"
    assert tasks_by_id[first_task["task_id"]]["status"] == "ready"
    assert tasks_by_id[second_task["task_id"]]["status"] == "passed"


def test_pulse_generate_next_carries_status_evidence_by_stable_task_id(tmp_path):
    first_board_root = tmp_path / "task-board-first"
    executor_root = tmp_path / "executor"

    first_result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(tmp_path / "generate-first"),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(tmp_path / "packet-first"),
            "AO2_PULSE_TASK_BOARD_ROOT": str(first_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "first-cursor.json"),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(executor_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert first_result.returncode == 0, first_result.stderr + first_result.stdout
    first_board = json.loads((first_board_root / "summary.json").read_text(encoding="utf-8"))
    first_task = first_board["tasks"][0]
    assert first_task["stable_task_id"]
    executor_root.mkdir(parents=True)
    (executor_root / "task-board-status-evidence.json").write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board-status-evidence.v1",
                "status": "ready",
                "source": "ao2.pulse-task-executor.v1",
                "task_board_generation": 2,
                "task_statuses": {
                    first_task["task_id"]: {
                        "status": "passed",
                        "status_reason": "Generation 1 task evidence should carry by stable id.",
                        "evidence": [str(executor_root / "summary.json")],
                    }
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    second_board_root = tmp_path / "task-board-second"
    second_cursor = tmp_path / "second-cursor.json"
    second_cursor.write_text(
        json.dumps({"generation": 1, "history": [], "index": 0}, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    second_result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(tmp_path / "generate-second"),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(tmp_path / "packet-second"),
            "AO2_PULSE_TASK_BOARD_ROOT": str(second_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(second_cursor),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(executor_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert second_result.returncode == 0, second_result.stderr + second_result.stdout
    second_board = json.loads((second_board_root / "summary.json").read_text(encoding="utf-8"))
    second_task = next(
        task
        for task in second_board["tasks"]
        if task["stable_task_id"] == first_task["stable_task_id"]
    )
    assert second_task["task_id"] != first_task["task_id"]
    assert second_task["status"] == "passed"
    assert second_task["status_transition"]["matched_by"] == "stable_task_id"


def test_pulse_generate_next_renders_stale_status_evidence_warning(tmp_path):
    status_evidence = tmp_path / "stale-status-evidence.json"
    task_board_root = tmp_path / "task-board"
    status_evidence.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board-status-evidence.v1",
                "status": "ready",
                "source": "ao2.pulse-task-executor.v1",
                "task_board_generation": 99,
                "task_statuses": {},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(tmp_path / "generate-next"),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(tmp_path / "packet"),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "cursor.json"),
            "AO2_PULSE_TASK_BOARD_STATUS_EVIDENCE": str(status_evidence),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    board = json.loads((task_board_root / "summary.json").read_text(encoding="utf-8"))
    board_md = (task_board_root / "board.md").read_text(encoding="utf-8")
    board_html = (task_board_root / "board.html").read_text(encoding="utf-8")
    assert board["status_transition_source"]["status"] == "stale_generation"
    assert "Status evidence ignored" in board_md
    assert "stale_generation" in board_html


def test_pulse_generate_next_writes_compact_board_state_summary(tmp_path):
    out_root = tmp_path / "generate-next"
    packet_root = tmp_path / "packet"
    task_board_root = tmp_path / "task-board"

    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(packet_root),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "cursor.json"),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    board = json.loads((task_board_root / "summary.json").read_text(encoding="utf-8"))
    state_summary_path = Path(board["exports"]["state_summary"])
    assert state_summary_path == task_board_root / "board-state-summary.json"
    state_summary = json.loads(state_summary_path.read_text(encoding="utf-8"))
    assert state_summary["schema_version"] == "ao2.ai-task-board-state-summary.v1"
    assert state_summary["status"] == "ready"
    assert state_summary["task_board"] == str(task_board_root / "summary.json")
    assert state_summary["task_count"] == len(board["tasks"])
    assert state_summary["status_counts"] == {"proposed": len(board["tasks"])}
    assert state_summary["next_actions"]
    assert state_summary["trust_boundary"] == {
        "local_only": True,
        "stores_credentials": False,
        "control_plane_can_mutate": False,
    }


def test_pulse_task_board_state_reads_current_board_without_regeneration(tmp_path):
    out_root = tmp_path / "generate-next"
    task_board_root = tmp_path / "task-board"
    state_root = tmp_path / "state"

    generate_result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(tmp_path / "packet"),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "cursor.json"),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert generate_result.returncode == 0, generate_result.stderr + generate_result.stdout

    state_result = subprocess.run(
        ["npm", "run", "pulse:task-board-state"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_TASK_BOARD_STATE_ROOT": str(state_root),
            "AO2_PULSE_TASK_BOARD_STATE_BOARD": str(task_board_root / "summary.json"),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert state_result.returncode == 0, state_result.stderr + state_result.stdout
    summary = json.loads((state_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-task-board-state.v1"
    assert summary["status"] == "passed"
    assert summary["task_board"] == str(task_board_root / "summary.json")
    assert summary["state_summary"] == str(task_board_root / "board-state-summary.json")
    assert summary["task_count"] > 0
    assert summary["trust_boundary"] == {"local_only": True, "stores_credentials": False}


def test_pulse_task_board_state_reports_missing_board(tmp_path):
    state_root = tmp_path / "state"
    missing_board = tmp_path / "missing-summary.json"

    result = subprocess.run(
        ["npm", "run", "pulse:task-board-state"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_TASK_BOARD_STATE_ROOT": str(state_root),
            "AO2_PULSE_TASK_BOARD_STATE_BOARD": str(missing_board),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((state_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-task-board-state.v1"
    assert summary["status"] == "failed"
    assert summary["reason"] == "task_board_missing"
    assert summary["task_board"] == str(missing_board)


def test_pulse_task_board_state_reports_invalid_board_schema(tmp_path):
    state_root = tmp_path / "state"
    board = tmp_path / "summary.json"
    board.write_text(
        json.dumps({"schema_version": "ao2.not-a-task-board.v1"}, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:task-board-state"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_TASK_BOARD_STATE_ROOT": str(state_root),
            "AO2_PULSE_TASK_BOARD_STATE_BOARD": str(board),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((state_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-task-board-state.v1"
    assert summary["status"] == "failed"
    assert summary["reason"] == "task_board_schema_invalid"
    assert summary["task_board"] == str(board)


def test_pulse_task_board_state_reports_invalid_board_json(tmp_path):
    state_root = tmp_path / "state"
    board = tmp_path / "summary.json"
    board.write_text("{\n", encoding="utf-8")

    result = subprocess.run(
        ["npm", "run", "pulse:task-board-state"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_TASK_BOARD_STATE_ROOT": str(state_root),
            "AO2_PULSE_TASK_BOARD_STATE_BOARD": str(board),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((state_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-task-board-state.v1"
    assert summary["status"] == "failed"
    assert summary["reason"] == "task_board_invalid_json:2"
    assert summary["task_board"] == str(board)


def test_pulse_next_actions_reads_current_board_actions(tmp_path):
    out_root = tmp_path / "generate-next"
    task_board_root = tmp_path / "task-board"
    actions_root = tmp_path / "next-actions"

    generate_result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(tmp_path / "packet"),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "cursor.json"),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert generate_result.returncode == 0, generate_result.stderr + generate_result.stdout

    actions_result = subprocess.run(
        ["npm", "run", "pulse:next-actions"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_ACTIONS_ROOT": str(actions_root),
            "AO2_PULSE_NEXT_ACTIONS_BOARD": str(task_board_root / "summary.json"),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert actions_result.returncode == 0, actions_result.stderr + actions_result.stdout
    summary = json.loads((actions_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-next-actions.v1"
    assert summary["status"] == "passed"
    assert summary["task_board"] == str(task_board_root / "summary.json")
    assert summary["next_actions"]
    assert "next-actions.md" in summary["exports"]["markdown"]
    assert summary["trust_boundary"] == {"local_only": True, "stores_credentials": False}
    assert "Next Actions" in (actions_root / "next-actions.md").read_text(encoding="utf-8")
    assert "next_action" in actions_result.stdout


def test_pulse_next_actions_includes_evidence_and_stop_conditions(tmp_path):
    actions_root = tmp_path / "next-actions"
    board = tmp_path / "summary.json"
    board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "tasks": [
                    {
                        "task_id": "safe-task-g507",
                        "stable_task_id": "safe-task",
                        "title": "Safe operator action",
                        "status": "proposed",
                        "next_action": "npm run safe:proof",
                        "rationale": "Keep unattended operators anchored to evidence.",
                        "required_evidence": [
                            "ao2.ai-task-board.v1",
                            "ao2.control-plane-fixture-consumer-smoke.v1",
                        ],
                        "stop_conditions": [
                            "Stop if control-plane readback requires credentials.",
                            "Stop if generated tasks lack evidence requirements.",
                        ],
                    }
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:next-actions"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_ACTIONS_ROOT": str(actions_root),
            "AO2_PULSE_NEXT_ACTIONS_BOARD": str(board),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((actions_root / "summary.json").read_text(encoding="utf-8"))
    markdown = (actions_root / "next-actions.md").read_text(encoding="utf-8")
    action = summary["next_actions"][0]
    assert action["required_evidence"] == [
        "ao2.ai-task-board.v1",
        "ao2.control-plane-fixture-consumer-smoke.v1",
    ]
    assert action["stop_conditions"] == [
        "Stop if control-plane readback requires credentials.",
        "Stop if generated tasks lack evidence requirements.",
    ]
    assert action["rationale"] == "Keep unattended operators anchored to evidence."
    assert "Required evidence" in markdown
    assert "`ao2.control-plane-fixture-consumer-smoke.v1`" in markdown
    assert "Stop conditions" in markdown
    assert "Stop if control-plane readback requires credentials." in markdown


def test_pulse_next_actions_reports_missing_board(tmp_path):
    actions_root = tmp_path / "next-actions"
    missing_board = tmp_path / "missing-summary.json"

    result = subprocess.run(
        ["npm", "run", "pulse:next-actions"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_ACTIONS_ROOT": str(actions_root),
            "AO2_PULSE_NEXT_ACTIONS_BOARD": str(missing_board),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((actions_root / "summary.json").read_text(encoding="utf-8"))
    markdown = (actions_root / "next-actions.md").read_text(encoding="utf-8")
    assert summary["schema_version"] == "ao2.pulse-next-actions.v1"
    assert summary["status"] == "failed"
    assert summary["reason"] == "task_board_missing"
    assert summary["task_board"] == str(missing_board)
    assert "Reason: task_board_missing" in markdown


def test_pulse_next_actions_reports_invalid_board_schema(tmp_path):
    actions_root = tmp_path / "next-actions"
    board = tmp_path / "summary.json"
    board.write_text(
        json.dumps({"schema_version": "ao2.not-a-task-board.v1"}, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:next-actions"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_ACTIONS_ROOT": str(actions_root),
            "AO2_PULSE_NEXT_ACTIONS_BOARD": str(board),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((actions_root / "summary.json").read_text(encoding="utf-8"))
    markdown = (actions_root / "next-actions.md").read_text(encoding="utf-8")
    assert summary["schema_version"] == "ao2.pulse-next-actions.v1"
    assert summary["status"] == "failed"
    assert summary["reason"] == "task_board_schema_invalid"
    assert summary["task_board"] == str(board)
    assert "Reason: task_board_schema_invalid" in markdown


def test_pulse_next_actions_reports_invalid_board_json(tmp_path):
    actions_root = tmp_path / "next-actions"
    board = tmp_path / "summary.json"
    board.write_text("{\n", encoding="utf-8")

    result = subprocess.run(
        ["npm", "run", "pulse:next-actions"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_ACTIONS_ROOT": str(actions_root),
            "AO2_PULSE_NEXT_ACTIONS_BOARD": str(board),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((actions_root / "summary.json").read_text(encoding="utf-8"))
    markdown = (actions_root / "next-actions.md").read_text(encoding="utf-8")
    assert summary["schema_version"] == "ao2.pulse-next-actions.v1"
    assert summary["status"] == "failed"
    assert summary["reason"] == "task_board_invalid_json:2"
    assert summary["task_board"] == str(board)
    assert "Reason: task_board_invalid_json:2" in markdown


def test_pulse_next_actions_filters_by_status(tmp_path):
    actions_root = tmp_path / "next-actions"
    board = tmp_path / "summary.json"
    board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "tasks": [
                    {
                        "task_id": "proposed-task-g1",
                        "stable_task_id": "proposed-task",
                        "title": "Proposed task",
                        "status": "proposed",
                        "next_action": "npm run proposed",
                    },
                    {
                        "task_id": "blocked-task-g1",
                        "stable_task_id": "blocked-task",
                        "title": "Blocked task",
                        "status": "blocked",
                        "next_action": "npm run blocked",
                    },
                    {
                        "task_id": "passed-task-g1",
                        "stable_task_id": "passed-task",
                        "title": "Passed task",
                        "status": "passed",
                        "next_action": "npm run passed",
                    },
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:next-actions"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_ACTIONS_ROOT": str(actions_root),
            "AO2_PULSE_NEXT_ACTIONS_BOARD": str(board),
            "AO2_PULSE_NEXT_ACTIONS_STATUS": " proposed,blocked ",
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((actions_root / "summary.json").read_text(encoding="utf-8"))
    markdown = (actions_root / "next-actions.md").read_text(encoding="utf-8")
    assert summary["schema_version"] == "ao2.pulse-next-actions.v1"
    assert summary["status"] == "passed"
    assert summary["status_filter"] == ["proposed", "blocked"]
    assert [item["status"] for item in summary["next_actions"]] == ["proposed", "blocked"]
    assert "passed-task-g1" not in markdown
    assert "proposed-task-g1" in markdown
    assert "blocked-task-g1" in markdown


def test_pulse_generate_next_writes_ai_task_board_artifact(tmp_path):
    out_root = tmp_path / "generate-next"
    packet_root = tmp_path / "packet"
    task_board_root = tmp_path / "task-board"
    cursor = tmp_path / "cursor.json"

    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(packet_root),
            "AO2_PULSE_TASK_BOARD_ROOT": str(task_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(cursor),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    board = json.loads((packet_root / "task-board.json").read_text(encoding="utf-8"))
    canonical_board = json.loads((task_board_root / "summary.json").read_text(encoding="utf-8"))
    assert board["schema_version"] == "ao2.ai-task-board.v1"
    assert board == canonical_board
    assert board["release_train"] == {
        "version": "v0.4.81",
        "theme": "AI task board control surface",
    }
    assert board["release_objective"]
    assert board["source_recommendation"]["selection"]
    assert board["control_plane_readback"] == {
        "role": "read_only_observer",
        "requires_credentials": False,
        "can_mutate_ao2_artifacts": False,
        "can_mutate_release_metadata": False,
    }
    assert board["trust_boundary"] == {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "local_artifact_materialization_only",
    }
    assert board["tasks"]
    for task in board["tasks"]:
        assert task["task_id"]
        assert task["title"]
        assert task["status"] == "proposed"
        assert task["rationale"]
        assert task["required_evidence"]
        assert task["stop_conditions"]

    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["task_board"] == str(packet_root / "task-board.json")
    assert summary["task_board_summary"] == str(task_board_root / "summary.json")
    assert any(item["path"] == "task-board.json" for item in summary["files"])


def test_pulse_generate_next_records_task_board_history_and_diff(tmp_path):
    history_root = tmp_path / "history"
    cursor = tmp_path / "cursor.json"

    for index in [1, 2]:
        result = subprocess.run(
            ["npm", "run", "pulse:generate-next"],
            cwd=REPO_ROOT,
            env={
                **os.environ,
                "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
                "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
                "AO2_PULSE_GENERATE_NEXT_ROOT": str(tmp_path / f"generate-{index}"),
                "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(tmp_path / f"packet-{index}"),
                "AO2_PULSE_TASK_BOARD_ROOT": str(tmp_path / f"task-board-{index}"),
                "AO2_PULSE_TASK_BOARD_HISTORY_ROOT": str(history_root),
                "AO2_PULSE_GENERATE_NEXT_CURSOR": str(cursor),
            },
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0, result.stderr + result.stdout

    first = json.loads((tmp_path / "task-board-1" / "task-board-diff.json").read_text(encoding="utf-8"))
    second = json.loads((tmp_path / "task-board-2" / "task-board-diff.json").read_text(encoding="utf-8"))
    second_board = json.loads((tmp_path / "task-board-2" / "summary.json").read_text(encoding="utf-8"))

    assert first["schema_version"] == "ao2.ai-task-board-diff.v1"
    assert first["previous_present"] is False
    assert second["schema_version"] == "ao2.ai-task-board-diff.v1"
    assert second["previous_present"] is True
    assert second["current_task_ids"]
    assert set(second["current_task_ids"]) == {
        task["task_id"] for task in second_board["tasks"]
    }
    assert second_board["history"]["latest"] == str(history_root / "latest.json")
    assert second_board["history"]["diff"] == str(tmp_path / "task-board-2" / "task-board-diff.json")
    assert (history_root / "latest.json").is_file()
    assert (history_root / "generation-1.json").is_file()
    assert (history_root / "generation-2.json").is_file()


def test_pulse_generate_next_records_task_board_field_level_diff(tmp_path):
    history_root = tmp_path / "history"

    first_result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(tmp_path / "generate-first"),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(tmp_path / "packet-first"),
            "AO2_PULSE_TASK_BOARD_ROOT": str(tmp_path / "task-board-first"),
            "AO2_PULSE_TASK_BOARD_HISTORY_ROOT": str(history_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "cursor-first.json"),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert first_result.returncode == 0, first_result.stderr + first_result.stdout

    previous_board = json.loads((history_root / "latest.json").read_text(encoding="utf-8"))
    previous_task = previous_board["tasks"][0]
    previous_task["title"] = "Previous title before operator drift"
    previous_task["objective"] = "Previous objective before operator drift."
    previous_task["status"] = "blocked"
    previous_task["rationale"] = "Previous rationale before operator drift."
    previous_task["required_evidence"] = ["previous-evidence.json"]
    previous_task["stop_conditions"] = ["Previous stop condition."]
    (history_root / "latest.json").write_text(
        json.dumps(previous_board, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    second_result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(tmp_path / "generate-second"),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(tmp_path / "packet-second"),
            "AO2_PULSE_TASK_BOARD_ROOT": str(tmp_path / "task-board-second"),
            "AO2_PULSE_TASK_BOARD_HISTORY_ROOT": str(history_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "cursor-second.json"),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert second_result.returncode == 0, second_result.stderr + second_result.stdout
    diff = json.loads((tmp_path / "task-board-second" / "task-board-diff.json").read_text(encoding="utf-8"))

    changed = next(
        item
        for item in diff["changed_tasks"]
        if item["task_id"] == previous_task["task_id"]
    )
    assert set(changed["changed_fields"]) >= {
        "objective",
        "rationale",
        "required_evidence",
        "status",
        "stop_conditions",
        "title",
    }
    assert diff["changed_task_ids"] == [previous_task["task_id"]]
    assert changed["field_changes"]["title"]["previous"] == (
        "Previous title before operator drift"
    )
    assert changed["field_changes"]["title"]["current"] != (
        "Previous title before operator drift"
    )
    assert changed["field_changes"]["required_evidence"]["previous"] == [
        "previous-evidence.json"
    ]


def test_pulse_generate_next_local_only_mode_omits_product_code_tasks(tmp_path):
    out_root = tmp_path / "generate-next"
    packet_root = tmp_path / "packet"
    cursor = tmp_path / "cursor.json"

    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "1",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(packet_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(cursor),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    manifest = json.loads((packet_root / "pulse-task-manifest.json").read_text(encoding="utf-8"))
    eval_loop = json.loads((packet_root / "pulse-eval-loop.json").read_text(encoding="utf-8"))
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))

    assert manifest["generation_mode"] == "local_only_while_pr_blocked"
    assert manifest["product_code_execution"] == {"enabled": False, "mode": "disabled"}
    assert manifest["local_only_while_pr_blocked"] is True
    assert manifest["tasks"]
    assert all(task["kind"] == "evidence_gate" for task in manifest["tasks"])
    assert all("command" in task for task in manifest["tasks"])
    assert eval_loop["generation_mode"] == "local_only_while_pr_blocked"
    assert eval_loop["local_only_while_pr_blocked"] is True
    assert summary["generation_mode"] == "local_only_while_pr_blocked"
    assert summary["local_only_while_pr_blocked"] is True


def test_pulse_generate_next_default_packet_root_matches_local_mirror_source(tmp_path):
    generator = read("scripts/pulse-generate-next.sh")
    assert 'PACKET_ROOT="${AO2_PULSE_GENERATE_NEXT_PACKET_ROOT:-$ROOT/target/pulse-next-recommended-tasks}"' in generator

    out_root = tmp_path / "generate-next"
    packet_root = tmp_path / "pulse-next-recommended-tasks"
    cursor = tmp_path / "cursor.json"

    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(out_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(packet_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(cursor),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    for filename in [
        "packet.md",
        "board.md",
        "executor-evidence.json",
        "pulse-eval-loop.json",
        "pulse-task-manifest.json",
        "summary.json",
    ]:
        assert (packet_root / filename).is_file(), filename

    mirror_root = tmp_path / "pulse-local-mirror"
    mirror = subprocess.run(
        ["npm", "run", "pulse:local-mirror"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_LOCAL_MIRROR_SOURCE": str(packet_root),
            "AO2_PULSE_LOCAL_MIRROR_DEST": str(mirror_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert mirror.returncode == 0, mirror.stderr + mirror.stdout
    mirror_summary = json.loads((mirror_root / "pulse-local-mirror-summary.json").read_text(encoding="utf-8"))
    assert mirror_summary["status"] == "passed"
    assert mirror_summary["missing_required_files"] == []


def test_pulse_generate_next_uses_project_level_strategic_scoring():
    generator = read("scripts/pulse-generate-next.sh")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        "project_level_reassessment",
        "strategic_score",
        "anti_recursion",
        "ledger_history",
        "docs/PRD.md",
        "docs/SDD-risky-pr-run.md",
        "docs/SCHEMAS-AND-INTERFACES.md",
        "docs/IMPLEMENTATION-SLICES.md",
        "public_reliability",
        "cross_platform_correctness",
        "ci_confidence",
        "evidence_quality",
        "security_safety_boundaries",
        "control_plane_integration",
        "release_readiness",
        "developer_operator_usability",
        "novelty",
        "rationale",
        "required_evidence",
        "stop_conditions",
        "avoid narrow recursion",
    ]:
        assert needle in generator

    for needle in [
        "strategic scoring",
        "project-level reassessment",
        "anti-recursion",
        "ledger history",
    ]:
        assert needle in verification


def test_pulse_generate_next_is_locked_to_product_mvp_readiness_not_script_recursion():
    generator = read("scripts/pulse-generate-next.sh")

    for needle in [
        "product_mvp_slice",
        "Risky PR Run MVP product loop",
        "risky-pr:product-readiness",
        "ao2.risky-pr-golden-path.v1",
        "static report",
        "evaluator closure",
        "local run record",
        "Do not create new shell wrappers",
        "script_wrapper_recursion_block",
    ]:
        assert needle in generator
    assert generator.count("npm run risky-pr:golden") == 0


def test_pulse_task_executor_contract_supports_product_code_tasks():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    script = REPO_ROOT / "scripts" / "pulse-task-executor.sh"

    assert (
        package_json["scripts"]["pulse:task-executor"]
        == "node scripts/run-sh-script.js scripts/pulse-task-executor.sh"
    )
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR

    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.pulse-task-manifest.v1",
        "ao2.pulse-task-executor.v1",
        "product_code",
        "product_code_execution",
        "pulse:code-agent-runner",
        "ao2.ai-task-board-status-evidence.v1",
        "task-board-status-evidence.json",
        "status_evidence",
        "evidence_gate",
        "implementation-packets",
        "trust_boundary",
        "stores_credentials",
        "local_only",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text
    assert "git push origin" not in text
    assert "gh release create" not in text

    for needle in [
        "npm run pulse:task-executor",
        "ao2.pulse-task-executor.v1",
        "ao2.pulse-task-manifest.v1",
        "product-code implementation packets",
        "product-code tasks can opt into `pulse:code-agent-runner`",
        "product_code tasks require verification evidence",
        "product_code task cannot close from packet materialization alone",
        "task-board-status-evidence.json",
        "AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE",
    ]:
        assert needle in verification


def test_pulse_task_executor_materializes_product_code_packet_without_command(tmp_path):
    out_root = tmp_path / "executor"
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-task-manifest.v1",
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "local_process_execution_and_packet_materialization",
                },
                "tasks": [
                    {
                        "id": "risky-pr-report-surface",
                        "kind": "product_code",
                        "title": "Risky PR report surface",
                        "objective": "Expose local run record and evaluator closure evidence in the report surface.",
                        "files": ["crates/ao2-cli/src/main.rs", "tests/test_public_stabilization.py"],
                        "acceptance": [
                            "Report contains local run record evidence.",
                            "Evaluator closure cannot pass without evidence.",
                        ],
                        "verification": [
                            {
                                "command": "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q",
                                "expected_evidence": "pytest.tests.test_public_stabilization",
                            }
                        ],
                        "stop_conditions": ["Stop if task requires provider API keys."],
                    },
                    {
                        "id": "node-evidence-gate",
                        "kind": "evidence_gate",
                        "title": "Node evidence gate",
                        "command": "node -e \"console.log('ao2-task-executor-ok')\"",
                        "expected_evidence": "node.stdout.ok",
                    },
                ],
            },
            indent=2,
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:task-executor"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_TASK_EXECUTOR_MANIFEST": str(manifest),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-task-executor.v1"
    assert summary["status"] == "passed"
    assert summary["counts"]["product_code"] == 1
    assert summary["counts"]["evidence_gate"] == 1
    packet = out_root / "implementation-packets" / "risky-pr-report-surface.md"
    assert packet.is_file()
    packet_text = packet.read_text(encoding="utf-8")
    assert "Risky PR report surface" in packet_text
    assert "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest" in packet_text
    gate_result = [item for item in summary["results"] if item["id"] == "node-evidence-gate"][0]
    assert gate_result["status"] == "passed"
    assert gate_result["expected_evidence"] == "node.stdout.ok"


def test_pulse_task_executor_emits_task_board_status_evidence(tmp_path):
    out_root = tmp_path / "executor"
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-task-manifest.v1",
                "selection": "ai-task-board-control-surface",
                "cursor": {"generation": 7},
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "local_process_execution_and_packet_materialization",
                },
                "tasks": [
                    {
                        "id": "product-task",
                        "kind": "product_code",
                        "title": "Product task",
                        "objective": "Materialize implementation packet for the board.",
                        "files": ["scripts/pulse-task-executor.sh"],
                        "acceptance": ["Status evidence marks product packet ready."],
                        "verification": [
                            {
                                "command": "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q",
                                "expected_evidence": "pytest.tests.test_public_stabilization",
                            }
                        ],
                        "stop_conditions": ["Stop if task requires credentials."],
                    },
                    {
                        "id": "evidence-task",
                        "kind": "evidence_gate",
                        "title": "Evidence task",
                        "command": "node -e \"console.log('status-evidence-ok')\"",
                        "expected_evidence": "node.stdout.ok",
                    },
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:task-executor"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_TASK_EXECUTOR_MANIFEST": str(manifest),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    status_evidence_path = Path(summary["status_evidence"])
    assert status_evidence_path == out_root / "task-board-status-evidence.json"
    status_evidence = json.loads(status_evidence_path.read_text(encoding="utf-8"))
    assert status_evidence["schema_version"] == "ao2.ai-task-board-status-evidence.v1"
    assert status_evidence["status"] == "ready"
    assert status_evidence["source"] == "ao2.pulse-task-executor.v1"
    assert status_evidence["task_board_generation"] == 7
    assert status_evidence["task_statuses"]["product-task"]["status"] == "ready"
    assert status_evidence["task_statuses"]["product-task"]["executor_status"] == (
        "packet_materialized"
    )
    assert status_evidence["task_statuses"]["evidence-task"]["status"] == "passed"
    assert status_evidence["task_statuses"]["evidence-task"]["evidence"][0] == (
        str(out_root / "summary.json")
    )
    assert status_evidence["task_statuses"]["evidence-task"]["evidence"][1] == (
        str(out_root / "logs" / "02-evidence-task.log")
    )


def test_pulse_task_executor_dry_runs_product_code_through_code_agent_runner(tmp_path):
    out_root = tmp_path / "executor"
    repo_root = tmp_path / "ao2-feature"
    repo_root.mkdir()
    (repo_root / "src.txt").write_text("before\n", encoding="utf-8")
    subprocess.run(["git", "init"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "config", "user.email", "ao2@example.invalid"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "config", "user.name", "AO2 Test"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "add", "."], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "commit", "-m", "seed"], cwd=repo_root, capture_output=True, text=True, check=True)
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-task-manifest.v1",
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "local_process_execution_and_packet_materialization",
                },
                "product_code_execution": {"enabled": True, "mode": "dry_run"},
                "tasks": [
                    {
                        "id": "local-agent-write",
                        "kind": "product_code",
                        "title": "Local agent write",
                        "objective": "Validate product-code runner packet.",
                        "repo": "ao2-feature",
                        "repo_path": str(repo_root),
                        "branch": "codex/local-agent-write",
                        "files": ["src.txt"],
                        "acceptance": ["src.txt can be updated by guarded runner."],
                        "verification": [
                            {
                                "command": "python3 -c \"from pathlib import Path; assert Path('src.txt').exists()\"",
                                "expected_evidence": "local.file.exists",
                            }
                        ],
                        "stop_conditions": ["Stop if unrelated dirty files are present."],
                    }
                ],
            },
            indent=2,
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:task-executor"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_TASK_EXECUTOR_MANIFEST": str(manifest),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    product = summary["results"][0]
    assert product["status"] == "code_agent_dry_run_passed"
    assert product["code_agent_summary"]
    runner = json.loads(Path(product["code_agent_summary"]).read_text(encoding="utf-8"))
    assert runner["schema_version"] == "ao2.pulse-code-agent-runner.v1"
    assert runner["mode"] == "dry_run"
    assert runner["task"]["allowed_files"] == ["src.txt"]


def test_pulse_task_executor_executes_product_code_through_code_agent_runner_when_enabled(tmp_path):
    out_root = tmp_path / "executor"
    repo_root = tmp_path / "ao2-feature"
    repo_root.mkdir()
    (repo_root / "src.txt").write_text("before\n", encoding="utf-8")
    subprocess.run(["git", "init"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "config", "user.email", "ao2@example.invalid"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "config", "user.name", "AO2 Test"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "add", "."], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "commit", "-m", "seed"], cwd=repo_root, capture_output=True, text=True, check=True)
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-task-manifest.v1",
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "local_process_execution_and_packet_materialization",
                },
                "product_code_execution": {"enabled": True, "mode": "execute"},
                "tasks": [
                    {
                        "id": "local-agent-write",
                        "kind": "product_code",
                        "title": "Local agent write",
                        "objective": "Execute product-code runner packet.",
                        "repo": "ao2-feature",
                        "repo_path": str(repo_root),
                        "branch": "codex/local-agent-write",
                        "files": ["src.txt"],
                        "acceptance": ["src.txt contains after."],
                        "verification": [
                            {
                                "command": "python3 -c \"from pathlib import Path; assert Path('src.txt').read_text() == 'after\\n'\"",
                                "expected_evidence": "local.file.updated",
                            }
                        ],
                        "code_agent": {
                            "command": "python3 -c \"from pathlib import Path; Path('src.txt').write_text('after\\\\n', encoding='utf-8')\""
                        },
                        "stop_conditions": ["Stop if unrelated dirty files are present."],
                    }
                ],
            },
            indent=2,
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:task-executor"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_CODE_AGENT_EXECUTE": "1",
            "AO2_PULSE_TASK_EXECUTOR_MANIFEST": str(manifest),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    product = summary["results"][0]
    assert product["status"] == "code_agent_execute_passed"
    runner = json.loads(Path(product["code_agent_summary"]).read_text(encoding="utf-8"))
    assert runner["mode"] == "execute"
    assert runner["execution"]["invoked_code_agent"] is True
    assert runner["verification_results"][0]["status"] == "passed"


def test_pulse_task_executor_rejects_credential_storing_manifest(tmp_path):
    out_root = tmp_path / "executor"
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-task-manifest.v1",
                "trust_boundary": {"local_only": True, "stores_credentials": True},
                "tasks": [
                    {
                        "id": "unsafe",
                        "kind": "evidence_gate",
                        "title": "Unsafe",
                        "command": "node -e \"console.log('unsafe')\"",
                        "expected_evidence": "unsafe",
                    }
                ],
            }
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:task-executor"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_TASK_EXECUTOR_MANIFEST": str(manifest),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "failed"
    assert summary["reason"] == "credential_storing_manifest_rejected"


def test_pulse_task_executor_rejects_product_code_without_verification_evidence(tmp_path):
    out_root = tmp_path / "executor"
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-task-manifest.v1",
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "local_process_execution_and_packet_materialization",
                },
                "tasks": [
                    {
                        "id": "risky-pr-report-surface",
                        "kind": "product_code",
                        "title": "Risky PR report surface",
                        "objective": "Expose local run record and evaluator closure evidence in the report surface.",
                        "files": ["crates/ao2-runtime/src/lib.rs"],
                        "acceptance": ["Report contains local run record evidence."],
                        "stop_conditions": ["Stop if verification evidence is missing."],
                    }
                ],
            }
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:task-executor"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_TASK_EXECUTOR_MANIFEST": str(manifest),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "failed"
    assert summary["reason"] == "product_code_verification_evidence_missing"
    assert summary["results"][0]["id"] == "risky-pr-report-surface"
    assert summary["results"][0]["status"] == "failed"
    assert summary["results"][0]["reason"] == "product_code_verification_evidence_missing"


def test_pulse_task_executor_isolates_pulse_local_mirror_dest_for_executable_tasks(tmp_path):
    out_root = tmp_path / "executor"
    parent_mirror = tmp_path / "parent-mirror"
    marker = tmp_path / "marker.json"
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-task-manifest.v1",
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "local_process_execution_and_packet_materialization",
                },
                "tasks": [
                    {
                        "id": "mirror-dest-isolation",
                        "kind": "evidence_gate",
                        "title": "Mirror destination isolation",
                        "command": (
                            "python3 -c \"import json, os, pathlib; "
                            "dest = pathlib.Path(os.environ['AO2_PULSE_LOCAL_MIRROR_DEST']); "
                            "assert 'task-executor-local-mirror' in dest.parts; "
                            f"pathlib.Path({str(marker)!r}).write_text(json.dumps({{'dest': str(dest)}}), encoding='utf-8')\""
                        ),
                        "expected_evidence": "ao2.pulse-task-executor.local-mirror-dest-isolated",
                    }
                ],
            },
            indent=2,
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:task-executor"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_LOCAL_MIRROR_DEST": str(parent_mirror),
            "AO2_PULSE_TASK_EXECUTOR_MANIFEST": str(manifest),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    assert summary["results"][0]["status"] == "passed"
    marker_payload = json.loads(marker.read_text(encoding="utf-8"))
    assert marker_payload["dest"] == str((out_root / "task-executor-local-mirror").resolve())
    assert marker_payload["dest"] != str(parent_mirror.resolve())


def test_pulse_code_agent_runner_contract_is_public_safe():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    script = REPO_ROOT / "scripts" / "pulse-code-agent-runner.sh"

    assert (
        package_json["scripts"]["pulse:code-agent-runner"]
        == "node scripts/run-sh-script.js scripts/pulse-code-agent-runner.sh"
    )
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR

    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.pulse-code-agent-runner.v1",
        "ao2.pulse-code-agent-task.v1",
        "dry_run",
        "execute",
        "AO2_PULSE_CODE_AGENT_EXECUTE",
        "codex exec",
        "allowed_files",
        "verification",
        "verification_results",
        "acceptance",
        "stop_conditions",
        "git status --porcelain",
        "post_execution_dirty_files",
    ]:
        assert needle in text
    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "git push origin",
        "gh pr create",
        "gh release create",
    ]:
        assert forbidden not in text

    for needle in [
        "npm run pulse:code-agent-runner",
        "ao2.pulse-code-agent-runner.v1",
        "ao2.pulse-code-agent-task.v1",
        "dry-run validates implementation-task packets",
        "execute mode requires `AO2_PULSE_CODE_AGENT_EXECUTE=1`",
        "does not push, open PRs, publish releases, or store credentials",
    ]:
        assert needle in verification


def test_pulse_code_agent_runner_dry_run_validates_product_code_task(tmp_path):
    out_root = tmp_path / "code-agent-runner"
    repo_root = tmp_path / "ao2-control-plane"
    (repo_root / "crates/ao2-cp-server/src/handlers").mkdir(parents=True)
    (repo_root / "crates/ao2-cp-server/tests").mkdir(parents=True)
    (repo_root / "docs").mkdir()
    (repo_root / "crates/ao2-cp-server/src/handlers/release_publication.rs").write_text(
        "// release publication handler\n",
        encoding="utf-8",
    )
    (repo_root / "crates/ao2-cp-server/tests/release_publication.rs").write_text(
        "// release publication tests\n",
        encoding="utf-8",
    )
    (repo_root / "docs/VERIFICATION.md").write_text("# Verification\n", encoding="utf-8")
    subprocess.run(["git", "init"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "config", "user.email", "ao2@example.invalid"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "config", "user.name", "AO2 Test"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "add", "."], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "commit", "-m", "seed"], cwd=repo_root, capture_output=True, text=True, check=True)

    task = tmp_path / "task.json"
    task.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-code-agent-task.v1",
                "id": "control-plane-support-bundle-alignment",
                "title": "Control-plane support bundle alignment",
                "objective": "Add replay and operator_evidence surfaces to the control-plane release support bundle.",
                "repo": "ao2-control-plane",
                "repo_path": str(repo_root),
                "branch": "codex/control-plane-support-bundle-alignment",
                "allowed_files": [
                    "crates/ao2-cp-server/src/handlers/release_publication.rs",
                    "crates/ao2-cp-server/tests/release_publication.rs",
                    "docs/VERIFICATION.md",
                ],
                "acceptance": [
                    "Support bundle includes replay and operator_evidence top-level surfaces.",
                    "AO2 release support-bundle-verify accepts the exported fixture.",
                ],
                "verification": [
                    {
                        "command": "cargo test -p ao2-cp-server release_support_bundle",
                        "expected_evidence": "cargo.ao2-cp-server.release_support_bundle",
                    },
                    {
                        "command": "ao2 release support-bundle-verify --bundle target/release-support-bundle.json",
                        "expected_evidence": "ao2.release-support-bundle-verify.v1",
                    },
                ],
                "stop_conditions": [
                    "Stop if any provider API-key path is introduced.",
                    "Stop if unrelated dirty files are present.",
                ],
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "dry_run_validation_only",
                },
            },
            indent=2,
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:code-agent-runner", "--", "--task", str(task), "--dry-run"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_CODE_AGENT_RUNNER_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-code-agent-runner.v1"
    assert summary["status"] == "passed"
    assert summary["mode"] == "dry_run"
    assert summary["task"]["id"] == "control-plane-support-bundle-alignment"
    assert summary["task"]["repo"] == "ao2-control-plane"
    assert summary["workspace"]["repo_path"] == str(repo_root)
    assert summary["workspace"]["git_status_checked"] is True
    assert summary["workspace"]["unrelated_dirty_files"] == []
    assert summary["execution"]["would_invoke_code_agent"] is True
    assert summary["execution"]["invoked_code_agent"] is False
    assert summary["execution"]["pushes"] is False
    assert summary["execution"]["opens_pr"] is False
    assert summary["verification"][0]["expected_evidence"] == "cargo.ao2-cp-server.release_support_bundle"


def test_pulse_code_agent_runner_execute_mode_runs_agent_and_verification(tmp_path):
    out_root = tmp_path / "code-agent-runner"
    repo_root = tmp_path / "ao2-feature"
    repo_root.mkdir()
    (repo_root / "src.txt").write_text("before\n", encoding="utf-8")
    subprocess.run(["git", "init"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "config", "user.email", "ao2@example.invalid"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "config", "user.name", "AO2 Test"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "add", "."], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "commit", "-m", "seed"], cwd=repo_root, capture_output=True, text=True, check=True)

    task = tmp_path / "task.json"
    task.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-code-agent-task.v1",
                "id": "local-agent-write",
                "title": "Local agent write",
                "objective": "Update the allowed file and prove verification runs.",
                "repo": "ao2-feature",
                "repo_path": str(repo_root),
                "branch": "codex/local-agent-write",
                "allowed_files": ["src.txt"],
                "acceptance": ["src.txt contains after."],
                "verification": [
                    {
                        "command": "python3 -c \"from pathlib import Path; assert Path('src.txt').read_text() == 'after\\n'\"",
                        "expected_evidence": "local.file.updated",
                    }
                ],
                "code_agent": {
                    "command": "python3 -c \"from pathlib import Path; Path('src.txt').write_text('after\\\\n', encoding='utf-8')\""
                },
                "stop_conditions": ["Stop if unrelated dirty files are present."],
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "local_code_agent_execution",
                },
            },
            indent=2,
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:code-agent-runner", "--", "--task", str(task), "--execute"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_CODE_AGENT_EXECUTE": "1",
            "AO2_PULSE_CODE_AGENT_RUNNER_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-code-agent-runner.v1"
    assert summary["status"] == "passed"
    assert summary["mode"] == "execute"
    assert summary["execution"]["would_invoke_code_agent"] is True
    assert summary["execution"]["invoked_code_agent"] is True
    assert summary["execution"]["exit_code"] == 0
    assert summary["execution"]["pushes"] is False
    assert summary["execution"]["opens_pr"] is False
    assert summary["workspace"]["post_execution_dirty_files"] == [{"path": "src.txt", "status": " M"}]
    assert summary["workspace"]["unrelated_dirty_files_after_execution"] == []
    assert summary["verification_results"][0]["status"] == "passed"
    assert (out_root / "code-agent-prompt.md").is_file()


def test_pulse_code_agent_runner_execute_mode_rejects_unrelated_changes(tmp_path):
    out_root = tmp_path / "code-agent-runner"
    repo_root = tmp_path / "ao2-feature"
    repo_root.mkdir()
    (repo_root / "src.txt").write_text("before\n", encoding="utf-8")
    subprocess.run(["git", "init"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "config", "user.email", "ao2@example.invalid"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "config", "user.name", "AO2 Test"], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "add", "."], cwd=repo_root, capture_output=True, text=True, check=True)
    subprocess.run(["git", "commit", "-m", "seed"], cwd=repo_root, capture_output=True, text=True, check=True)

    task = tmp_path / "task.json"
    task.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-code-agent-task.v1",
                "id": "local-agent-unrelated-write",
                "title": "Local agent unrelated write",
                "objective": "Reject changes outside allowed files.",
                "repo": "ao2-feature",
                "repo_path": str(repo_root),
                "branch": "codex/local-agent-unrelated-write",
                "allowed_files": ["src.txt"],
                "acceptance": ["Only src.txt may change."],
                "verification": [
                    {
                        "command": "python3 -c \"print('should-not-run')\"",
                        "expected_evidence": "verification.not.reached",
                    }
                ],
                "code_agent": {
                    "command": "python3 -c \"from pathlib import Path; Path('other.txt').write_text('bad', encoding='utf-8')\""
                },
                "stop_conditions": ["Stop if unrelated dirty files are present."],
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "local_code_agent_execution",
                },
            },
            indent=2,
        ),
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:code-agent-runner", "--", "--task", str(task), "--execute"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_CODE_AGENT_EXECUTE": "1",
            "AO2_PULSE_CODE_AGENT_RUNNER_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "failed"
    assert summary["reason"] == "unrelated_dirty_files_after_execution"
    assert summary["workspace"]["unrelated_dirty_files_after_execution"] == [{"path": "other.txt", "status": "??"}]
    assert summary.get("verification_results", []) == []


def test_pulse_real_execute_containment_runs_product_code_executor_sandbox(tmp_path):
    out_root = tmp_path / "target" / "pulse-real-execute-containment" / "latest"

    result = subprocess.run(
        ["npm", "run", "pulse:real-execute-containment"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_REAL_EXECUTE_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.pulse-real-execute-containment.v1"
    assert summary["status"] == "passed"
    assert Path(summary["pulse_generate_next_summary"]).is_file()
    assert summary["product_code_execute_fixture"]["status"] == "passed"
    assert summary["product_code_execute_fixture"]["sandbox_repo"].startswith(str(out_root))
    assert summary["product_code_execute_fixture"]["changed_files"] == [{"path": "allowed.txt", "status": " M"}]
    executor = json.loads(Path(summary["pulse_task_executor_summary"]).read_text(encoding="utf-8"))
    assert executor["status"] == "passed"
    assert executor["results"][0]["status"] == "code_agent_execute_passed"
    runner = json.loads(Path(summary["product_code_execute_fixture"]["code_agent_summary"]).read_text(encoding="utf-8"))
    assert runner["mode"] == "execute"
    assert runner["execution"]["invoked_code_agent"] is True
    assert runner["verification_results"][0]["status"] == "passed"


def test_pulse_next_task_quality_filter_rejects_script_wrapper_only_packets(tmp_path):
    packet = tmp_path / "packet.md"
    out_root = tmp_path / "quality"
    packet.write_text(
        "# Packet\n\n"
        "## 1. Pulse wrapper consolidation matrix\n\n"
        "Add another shell wrapper around existing Pulse gates.\n\n"
        "## 2. Script tracking runbook lock\n\n"
        "Refresh script index and runbook proof without product evidence.\n",
        encoding="utf-8",
    )
    env = {
        **os.environ,
        "AO2_PULSE_NEXT_TASK_QUALITY_PACKET": str(packet),
        "AO2_PULSE_NEXT_TASK_QUALITY_ROOT": str(out_root),
    }

    result = subprocess.run(
        ["npm", "run", "pulse:next-task-quality-filter"],
        cwd=REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "failed"
    assert summary["script_wrapper_recursion_block"] is True
    assert summary["product_slice_coverage"] == "missing"


def test_pulse_next_task_quality_filter_allows_support_tasks_inside_product_packets(tmp_path):
    packet = tmp_path / "packet.md"
    out_root = tmp_path / "quality"
    packet.write_text(
        "# Packet\n\n"
        "## 1. Risky PR static report/export\n\n"
        "Build product evidence for the Risky PR Run MVP.\n\n"
        "## 2. Pulse blocked registration resume guard\n\n"
        "Keep registration safe while product evidence advances.\n",
        encoding="utf-8",
    )
    env = {
        **os.environ,
        "AO2_PULSE_NEXT_TASK_QUALITY_PACKET": str(packet),
        "AO2_PULSE_NEXT_TASK_QUALITY_ROOT": str(out_root),
    }

    result = subprocess.run(
        ["npm", "run", "pulse:next-task-quality-filter"],
        cwd=REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    assert summary["script_wrapper_recursion_block"] is False
    assert summary["product_slice_coverage"] == "present"


def test_pulse_next_task_quality_filter_rejects_task_board_missing_drift_fields(tmp_path):
    packet = tmp_path / "packet.md"
    task_board = tmp_path / "task-board.json"
    out_root = tmp_path / "quality"
    packet.write_text(
        "# Packet\n\n"
        "## 1. AI task board control surface\n\n"
        "Build operator-visible product evidence for the control-plane task board.\n",
        encoding="utf-8",
    )
    task_board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "status": "ready",
                "release_objective": "",
                "tasks": [
                    {
                        "task_id": "missing-evidence-and-stop",
                        "title": "Missing evidence and stop",
                        "status": "proposed",
                        "required_evidence": [],
                        "stop_conditions": [],
                    }
                ],
                "trust_boundary": {"local_only": True, "stores_credentials": False},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:next-task-quality-filter"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_TASK_QUALITY_PACKET": str(packet),
            "AO2_PULSE_NEXT_TASK_QUALITY_TASK_BOARD": str(task_board),
            "AO2_PULSE_NEXT_TASK_QUALITY_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "failed"
    assert summary["task_board_drift_gate"] == "failed"
    assert summary["task_board_blockers"] == [
        "release_objective_missing",
        "task_missing_required_evidence:missing-evidence-and-stop",
        "task_missing_stop_conditions:missing-evidence-and-stop",
    ]


def test_pulse_next_task_quality_filter_accepts_complete_task_board(tmp_path):
    packet = tmp_path / "packet.md"
    task_board = tmp_path / "task-board.json"
    out_root = tmp_path / "quality"
    packet.write_text(
        "# Packet\n\n"
        "## 1. AI task board control surface\n\n"
        "Build operator-visible product evidence for the control-plane task board.\n",
        encoding="utf-8",
    )
    task_board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "status": "ready",
                "release_objective": "Expose Pulse work as an operator-readable task board.",
                "tasks": [
                    {
                        "task_id": "complete-task",
                        "title": "Complete task",
                        "status": "proposed",
                        "required_evidence": ["ao2.ai-task-board.v1"],
                        "stop_conditions": ["Stop if readback requires credentials."],
                    }
                ],
                "trust_boundary": {"local_only": True, "stores_credentials": False},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:next-task-quality-filter"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_TASK_QUALITY_PACKET": str(packet),
            "AO2_PULSE_NEXT_TASK_QUALITY_TASK_BOARD": str(task_board),
            "AO2_PULSE_NEXT_TASK_QUALITY_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    assert summary["task_board_drift_gate"] == "passed"
    assert summary["task_board_blockers"] == []


def test_pulse_next_task_quality_filter_rejects_unknown_status_evidence_task_id(tmp_path):
    packet = tmp_path / "packet.md"
    task_board = tmp_path / "task-board.json"
    status_evidence = tmp_path / "status-evidence.json"
    out_root = tmp_path / "quality"
    packet.write_text(
        "# Packet\n\n"
        "## 1. AI task board control surface\n\n"
        "Build operator-visible product evidence for the control-plane task board.\n",
        encoding="utf-8",
    )
    task_board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "status": "ready",
                "release_objective": "Expose Pulse work as an operator-readable task board.",
                "source_recommendation": {"generation": 7},
                "tasks": [
                    {
                        "task_id": "complete-task",
                        "title": "Complete task",
                        "status": "proposed",
                        "required_evidence": ["ao2.ai-task-board.v1"],
                        "stop_conditions": ["Stop if readback requires credentials."],
                    }
                ],
                "trust_boundary": {"local_only": True, "stores_credentials": False},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    status_evidence.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board-status-evidence.v1",
                "status": "ready",
                "task_board_generation": 7,
                "task_statuses": {
                    "ghost-task": {
                        "status": "passed",
                        "status_reason": "This task id is not present on the board.",
                        "evidence": ["target/pulse-task-executor/latest/summary.json"],
                    }
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:next-task-quality-filter"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_TASK_QUALITY_PACKET": str(packet),
            "AO2_PULSE_NEXT_TASK_QUALITY_TASK_BOARD": str(task_board),
            "AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE": str(status_evidence),
            "AO2_PULSE_NEXT_TASK_QUALITY_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["task_board_drift_gate"] == "failed"
    assert summary["status_evidence_gate"] == "failed"
    assert summary["status_evidence_blockers"] == [
        "status_evidence_unknown_task_id:ghost-task"
    ]


def test_pulse_next_task_quality_filter_accepts_stable_status_evidence_task_id(tmp_path):
    packet = tmp_path / "packet.md"
    task_board = tmp_path / "task-board.json"
    status_evidence = tmp_path / "status-evidence.json"
    out_root = tmp_path / "quality"
    packet.write_text(
        "# Packet\n\n"
        "## 1. AI task board control surface\n\n"
        "Build operator-visible product evidence for the control-plane task board.\n",
        encoding="utf-8",
    )
    task_board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "status": "ready",
                "release_objective": "Expose Pulse work as an operator-readable task board.",
                "source_recommendation": {"generation": 7},
                "tasks": [
                    {
                        "task_id": "complete-task-g7",
                        "stable_task_id": "complete-task",
                        "title": "Complete task",
                        "status": "proposed",
                        "required_evidence": ["ao2.ai-task-board.v1"],
                        "stop_conditions": ["Stop if readback requires credentials."],
                    }
                ],
                "trust_boundary": {"local_only": True, "stores_credentials": False},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    status_evidence.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board-status-evidence.v1",
                "status": "ready",
                "task_board_generation": 7,
                "task_statuses": {
                    "complete-task": {
                        "status": "passed",
                        "status_reason": "Stable task id should match the generated board task.",
                        "evidence": ["target/pulse-task-executor/latest/summary.json"],
                    }
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:next-task-quality-filter"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_TASK_QUALITY_PACKET": str(packet),
            "AO2_PULSE_NEXT_TASK_QUALITY_TASK_BOARD": str(task_board),
            "AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE": str(status_evidence),
            "AO2_PULSE_NEXT_TASK_QUALITY_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    assert summary["task_board_drift_gate"] == "passed"
    assert summary["status_evidence_gate"] == "passed"
    assert summary["status_evidence_blockers"] == []
    assert summary["status_evidence_matches"] == [
        {
            "evidence_task_id": "complete-task",
            "task_id": "complete-task-g7",
            "stable_task_id": "complete-task",
            "matched_by": "stable_task_id",
        }
    ]
    assert summary["status_evidence_match_counts"] == {
        "task_id": 0,
        "stable_task_id": 1,
    }


def test_pulse_next_task_quality_filter_reports_exact_task_id_match_telemetry(tmp_path):
    packet = tmp_path / "packet.md"
    task_board = tmp_path / "task-board.json"
    status_evidence = tmp_path / "status-evidence.json"
    out_root = tmp_path / "quality"
    packet.write_text(
        "# Packet\n\n"
        "## 1. AI task board control surface\n\n"
        "Build operator-visible product evidence for the control-plane task board.\n",
        encoding="utf-8",
    )
    task_board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "status": "ready",
                "release_objective": "Expose Pulse work as an operator-readable task board.",
                "source_recommendation": {"generation": 7},
                "tasks": [
                    {
                        "task_id": "complete-task-g7",
                        "stable_task_id": "complete-task",
                        "title": "Complete task",
                        "status": "proposed",
                        "required_evidence": ["ao2.ai-task-board.v1"],
                        "stop_conditions": ["Stop if readback requires credentials."],
                    }
                ],
                "trust_boundary": {"local_only": True, "stores_credentials": False},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    status_evidence.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board-status-evidence.v1",
                "status": "ready",
                "task_board_generation": 7,
                "task_statuses": {
                    "complete-task-g7": {
                        "status": "passed",
                        "status_reason": "Generated task id should match exactly.",
                        "evidence": ["target/pulse-task-executor/latest/summary.json"],
                    }
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:next-task-quality-filter"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_TASK_QUALITY_PACKET": str(packet),
            "AO2_PULSE_NEXT_TASK_QUALITY_TASK_BOARD": str(task_board),
            "AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE": str(status_evidence),
            "AO2_PULSE_NEXT_TASK_QUALITY_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status_evidence_matches"] == [
        {
            "evidence_task_id": "complete-task-g7",
            "task_id": "complete-task-g7",
            "stable_task_id": "complete-task",
            "matched_by": "task_id",
        }
    ]
    assert summary["status_evidence_match_counts"] == {
        "task_id": 1,
        "stable_task_id": 0,
    }


def test_pulse_next_task_quality_filter_rejects_stale_status_evidence_generation(tmp_path):
    packet = tmp_path / "packet.md"
    task_board = tmp_path / "task-board.json"
    status_evidence = tmp_path / "status-evidence.json"
    out_root = tmp_path / "quality"
    packet.write_text(
        "# Packet\n\n"
        "## 1. AI task board control surface\n\n"
        "Build operator-visible product evidence for the control-plane task board.\n",
        encoding="utf-8",
    )
    task_board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "status": "ready",
                "release_objective": "Expose Pulse work as an operator-readable task board.",
                "source_recommendation": {"generation": 7},
                "tasks": [
                    {
                        "task_id": "complete-task",
                        "title": "Complete task",
                        "status": "proposed",
                        "required_evidence": ["ao2.ai-task-board.v1"],
                        "stop_conditions": ["Stop if readback requires credentials."],
                    }
                ],
                "trust_boundary": {"local_only": True, "stores_credentials": False},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    status_evidence.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board-status-evidence.v1",
                "status": "ready",
                "task_board_generation": 6,
                "task_statuses": {
                    "complete-task": {
                        "status": "passed",
                        "status_reason": "This evidence belongs to an older board generation.",
                        "evidence": ["target/pulse-task-executor/latest/summary.json"],
                    }
                },
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "pulse:next-task-quality-filter"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_TASK_QUALITY_PACKET": str(packet),
            "AO2_PULSE_NEXT_TASK_QUALITY_TASK_BOARD": str(task_board),
            "AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE": str(status_evidence),
            "AO2_PULSE_NEXT_TASK_QUALITY_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["task_board_drift_gate"] == "failed"
    assert summary["status_evidence_gate"] == "failed"
    assert summary["status_evidence_blockers"] == [
        "status_evidence_stale_generation:6!=7"
    ]


def test_pulse_task_board_full_loop_generate_execute_validate_regenerate(tmp_path):
    first_root = tmp_path / "generate-first"
    first_packet = tmp_path / "packet-first"
    first_board_root = tmp_path / "task-board-first"
    executor_root = tmp_path / "executor"
    quality_root = tmp_path / "quality"

    first_result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(first_root),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(first_packet),
            "AO2_PULSE_TASK_BOARD_ROOT": str(first_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "first-cursor.json"),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(executor_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert first_result.returncode == 0, first_result.stderr + first_result.stdout
    first_board = json.loads((first_board_root / "summary.json").read_text(encoding="utf-8"))
    product_task = next(task for task in first_board["tasks"] if task["kind"] == "product_code")
    evidence_task = next(task for task in first_board["tasks"] if task["kind"] == "evidence_gate")
    manifest = tmp_path / "loop-manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": "ao2.pulse-task-manifest.v1",
                "cursor": {"generation": first_board["source_recommendation"]["generation"]},
                "trust_boundary": {
                    "local_only": True,
                    "stores_credentials": False,
                    "side_effects": "local_process_execution_and_packet_materialization",
                },
                "tasks": [
                    {
                        "id": product_task["task_id"],
                        "kind": "product_code",
                        "title": product_task["title"],
                        "objective": product_task["objective"],
                        "files": ["scripts/pulse-generate-next.sh"],
                        "acceptance": ["Loop fixture materializes implementation packet."],
                        "verification": [
                            {
                                "command": "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q",
                                "expected_evidence": "pytest.tests.test_public_stabilization",
                            }
                        ],
                        "stop_conditions": product_task["stop_conditions"],
                    },
                    {
                        "id": evidence_task["task_id"],
                        "kind": "evidence_gate",
                        "title": evidence_task["title"],
                        "command": "node -e \"console.log('task-board-full-loop-ok')\"",
                        "expected_evidence": "node.stdout.ok",
                    },
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    executor_result = subprocess.run(
        ["npm", "run", "pulse:task-executor"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_TASK_EXECUTOR_MANIFEST": str(manifest),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(executor_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert executor_result.returncode == 0, executor_result.stderr + executor_result.stdout
    status_evidence = executor_root / "task-board-status-evidence.json"
    assert status_evidence.is_file()

    quality_result = subprocess.run(
        ["npm", "run", "pulse:next-task-quality-filter"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_NEXT_TASK_QUALITY_PACKET": str(first_packet / "packet.md"),
            "AO2_PULSE_NEXT_TASK_QUALITY_TASK_BOARD": str(first_packet / "task-board.json"),
            "AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE": str(status_evidence),
            "AO2_PULSE_NEXT_TASK_QUALITY_ROOT": str(quality_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert quality_result.returncode == 0, quality_result.stderr + quality_result.stdout

    second_board_root = tmp_path / "task-board-second"
    second_result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
            "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY": "0",
            "AO2_PULSE_GENERATE_NEXT_ROOT": str(tmp_path / "generate-second"),
            "AO2_PULSE_GENERATE_NEXT_PACKET_ROOT": str(tmp_path / "packet-second"),
            "AO2_PULSE_TASK_BOARD_ROOT": str(second_board_root),
            "AO2_PULSE_GENERATE_NEXT_CURSOR": str(tmp_path / "second-cursor.json"),
            "AO2_PULSE_TASK_EXECUTOR_ROOT": str(executor_root),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert second_result.returncode == 0, second_result.stderr + second_result.stdout
    second_board = json.loads((second_board_root / "summary.json").read_text(encoding="utf-8"))
    second_tasks = {task["task_id"]: task for task in second_board["tasks"]}
    assert second_tasks[product_task["task_id"]]["status"] == "ready"
    assert second_tasks[evidence_task["task_id"]]["status"] == "passed"
    state_summary = json.loads(
        Path(second_board["exports"]["state_summary"]).read_text(encoding="utf-8")
    )
    assert state_summary["status_counts"]["ready"] == 1
    assert state_summary["status_counts"]["passed"] == 1


def test_control_plane_fixture_consumer_smoke_reads_ai_task_board_fixture(tmp_path):
    out_root = tmp_path / "control-plane-fixture-consumer"
    task_board = tmp_path / "task-board.json"
    task_board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "status": "ready",
                "release_objective": "Expose Pulse work as an operator-readable task board.",
                "tasks": [
                    {
                        "task_id": "complete-task",
                        "title": "Complete task",
                        "status": "proposed",
                        "required_evidence": ["ao2.ai-task-board.v1"],
                        "stop_conditions": ["Stop if readback requires credentials."],
                    }
                ],
                "control_plane_readback": {
                    "role": "read_only_observer",
                    "requires_credentials": False,
                    "can_mutate_ao2_artifacts": False,
                    "can_mutate_release_metadata": False,
                },
                "trust_boundary": {"local_only": True, "stores_credentials": False},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "control-plane:fixture-consumer-smoke"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_CP_FIXTURE_CONSUMER_SMOKE_ROOT": str(out_root),
            "AO2_CP_FIXTURE_CONSUMER_TASK_BOARD": str(task_board),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["task_board_readback"] == {
        "status": "passed",
        "schema_version": "ao2.ai-task-board.v1",
        "task_count": 1,
        "control_plane_role": "read_only_observer",
        "requires_credentials": False,
        "mutates_releases": False,
    }


def test_control_plane_fixture_consumer_smoke_reads_task_board_from_catalog(tmp_path):
    out_root = tmp_path / "control-plane-fixture-consumer"
    task_board = tmp_path / "task-board.json"
    task_board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "status": "ready",
                "release_objective": "Expose Pulse work as an operator-readable task board.",
                "tasks": [
                    {
                        "task_id": "catalog-task",
                        "title": "Catalog task",
                        "status": "proposed",
                        "required_evidence": ["ao2.ai-task-board.v1"],
                        "stop_conditions": ["Stop if catalog readback requires credentials."],
                    }
                ],
                "control_plane_readback": {
                    "role": "read_only_observer",
                    "requires_credentials": False,
                    "can_mutate_ao2_artifacts": False,
                    "can_mutate_release_metadata": False,
                },
                "trust_boundary": {"local_only": True, "stores_credentials": False},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "control-plane:fixture-consumer-smoke"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_CP_FIXTURE_CONSUMER_SMOKE_ROOT": str(out_root),
            "AO2_OPERATOR_INDEX_CP_TASK_BOARD": str(task_board),
            "AO2_CP_FIXTURE_CONSUMER_TASK_BOARD": str(tmp_path / "missing-direct-board.json"),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["task_board_readback"]["status"] == "passed"
    assert summary["task_board_readback"]["source"] == "fixture_catalog"
    assert summary["task_board_readback"]["task_count"] == 1
    catalog = json.loads(
        (
            out_root
            / "operator-index-control-plane-fixture-ingest"
            / "control-plane-fixture-catalog.json"
        ).read_text(encoding="utf-8")
    )
    assert any(
        item.get("source_schema") == "ao2.ai-task-board.v1"
        and item.get("task_board_fixture_reusable") is True
        for item in catalog["control_plane_fixture_catalog"]
    )


def test_control_plane_fixture_consumer_smoke_writes_operator_task_board_view_from_catalog(tmp_path):
    out_root = tmp_path / "control-plane-fixture-consumer"
    task_board = tmp_path / "task-board.json"
    task_board.write_text(
        json.dumps(
            {
                "schema_version": "ao2.ai-task-board.v1",
                "status": "ready",
                "release_objective": "Expose Pulse work as an operator-readable task board.",
                "tasks": [
                    {
                        "task_id": "catalog-task",
                        "title": "Catalog task",
                        "status": "blocked",
                        "rationale": "The operator needs drift-free task state.",
                        "next_action": "npm run pulse:task-executor",
                        "required_evidence": ["ao2.ai-task-board.v1"],
                        "stop_conditions": ["Stop if catalog readback requires credentials."],
                    }
                ],
                "control_plane_readback": {
                    "role": "read_only_observer",
                    "requires_credentials": False,
                    "can_mutate_ao2_artifacts": False,
                    "can_mutate_release_metadata": False,
                },
                "trust_boundary": {"local_only": True, "stores_credentials": False},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        ["npm", "run", "control-plane:fixture-consumer-smoke"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_CP_FIXTURE_CONSUMER_SMOKE_ROOT": str(out_root),
            "AO2_OPERATOR_INDEX_CP_TASK_BOARD": str(task_board),
            "AO2_CP_FIXTURE_CONSUMER_TASK_BOARD": str(tmp_path / "missing-direct-board.json"),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    view = summary["operator_task_board_view"]
    assert view["status"] == "passed"
    assert view["source"] == "fixture_catalog"
    assert view["read_only"] is True
    assert view["task_count"] == 1
    view_summary = json.loads(Path(view["summary"]).read_text(encoding="utf-8"))
    assert view_summary["schema_version"] == "ao2.control-plane-operator-task-board-view.v1"
    assert view_summary["status"] == "passed"
    assert view_summary["task_status_counts"] == {"blocked": 1}
    view_html = Path(view_summary["html"]).read_text(encoding="utf-8")
    for needle in [
        "AO2 Control Plane Task Board",
        "catalog-task",
        "Catalog task",
        "status-blocked",
        "read-only observer",
        "npm run pulse:task-executor",
        "Stop if catalog readback requires credentials.",
    ]:
        assert needle in view_html


def test_control_plane_fixture_consumer_smoke_skips_missing_ai_task_board(tmp_path):
    out_root = tmp_path / "control-plane-fixture-consumer"
    task_board = tmp_path / "missing-task-board.json"

    result = subprocess.run(
        ["npm", "run", "control-plane:fixture-consumer-smoke"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_CP_FIXTURE_CONSUMER_SMOKE_ROOT": str(out_root),
            "AO2_CP_FIXTURE_CONSUMER_TASK_BOARD": str(task_board),
        },
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary = json.loads((out_root / "summary.json").read_text(encoding="utf-8"))
    assert summary["status"] == "passed"
    assert summary["task_board_readback"] == {
        "status": "skipped",
        "path": str(task_board.resolve()),
    }
    assert {
        "name": "ai_task_board_readback",
        "status": "passed",
    } in summary["checks"]


def test_artifact_index_consumer_canary_and_pulse_resume_contracts():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    expected_scripts = {
        "artifacts:index": "node scripts/run-sh-script.js scripts/artifact-index-report.sh",
        "release:artifact-consumer-smoke": "node scripts/run-sh-script.js scripts/release-artifact-consumer-smoke.sh",
        "post-merge:canary": "node scripts/run-sh-script.js scripts/post-merge-canary.sh",
        "pulse:resume": "node scripts/run-sh-script.js scripts/pulse-resume.sh",
    }
    for name, command in expected_scripts.items():
        assert package_json["scripts"][name] == command

    for script_name, schema in [
        ("artifact-index-report.sh", "ao2.artifact-index-report.v1"),
        ("release-artifact-consumer-smoke.sh", "ao2.release-artifact-consumer-smoke.v1"),
        ("post-merge-canary.sh", "ao2.post-merge-canary.v1"),
        ("pulse-resume.sh", "ao2.pulse-resume.v1"),
    ]:
        script = REPO_ROOT / "scripts" / script_name
        assert script.is_file()
        assert script.stat().st_mode & stat.S_IXUSR
        text = script.read_text(encoding="utf-8")
        assert schema in text
        assert "OPENAI_API_KEY" not in text
        assert "ANTHROPIC_API_KEY" not in text

    artifact_index = read("scripts/artifact-index-report.sh")
    for needle in [
        "../ao2-control-plane",
        "target/ci-artifacts",
        "target/dr-restore-drill",
        ".ao2-local/pulse/latest",
        "report.md",
    ]:
        assert needle in artifact_index

    consumer_smoke = read("scripts/release-artifact-consumer-smoke.sh")
    for needle in [
        "--dry-run",
        "gh run download",
        "schema_version",
        "sha256",
        "clean-workspace",
        "uesugitorachiyo/ao2-control-plane",
    ]:
        assert needle in consumer_smoke

    canary = read("scripts/post-merge-canary.sh")
    for needle in [
        "artifacts:index",
        "release:artifact-consumer-smoke",
        "pulse:resume",
        "../ao2-control-plane/scripts/smoke-long-lived-dev.sh",
    ]:
        assert needle in canary

    pulse_resume = read("scripts/pulse-resume.sh")
    for needle in [
        "--dry-run",
        "resume.json",
        "pulse_eval_loop_sha256",
        "shasum",
        "resume_command",
    ]:
        assert needle in pulse_resume

    for needle in [
        "npm run artifacts:index",
        "npm run release:artifact-consumer-smoke -- --dry-run",
        "npm run post-merge:canary",
        "npm run pulse:resume -- --dry-run",
        "ao2.artifact-index-report.v1",
        "ao2.post-merge-canary.v1",
    ]:
        assert needle in verification


def test_release_readiness_regression_gate_includes_artifact_and_resume_gates():
    text = read("scripts/release-readiness-regression-gate.sh")
    for needle in [
        "artifacts:index",
        "release:artifact-consumer-smoke",
        "pulse:resume",
        "artifact_index",
        "release_artifact_consumer_smoke",
        "pulse_resume_dry_run",
        'AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS="ao2/target/ci-artifacts ao2/.ao2-local/pulse/latest ao2-control-plane/target/ci-artifacts"',
        "ao2/target/release-readiness-regression-gate",
    ]:
        assert needle in text


def test_real_artifact_consumer_dashboard_pulse_execute_and_manual_canary_contracts():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["release:artifact-consumer-smoke"]
        == "node scripts/run-sh-script.js scripts/release-artifact-consumer-smoke.sh"
    )

    consumer = read("scripts/release-artifact-consumer-smoke.sh")
    for needle in [
        "--fixture-dir",
        "--require-artifact",
        "--require-schema",
        "required_artifacts",
        "missing_required_artifacts",
        "missing_required_schemas",
        "ao2.release-artifact-consumer-smoke.v1",
        "gh run download",
        "rm -rf \"$OUT_ROOT/clean-workspace\"",
    ]:
        assert needle in consumer

    artifact_index = read("scripts/artifact-index-report.sh")
    for needle in [
        "dashboard.html",
        "ao2.artifact-evidence-dashboard.v1",
        "stale_after_seconds",
        "health",
        "latest_generated_at_utc",
    ]:
        assert needle in artifact_index

    pulse_resume = read("scripts/pulse-resume.sh")
    for needle in [
        "--execute",
        "execution_mode",
        "hash_mismatch",
        "refusing to execute without --execute",
        "shlex.split",
    ]:
        assert needle in pulse_resume

    workflow = read(".github/workflows/local-canary.yml")
    for needle in [
        "name: Local Canary",
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "npm run release:artifact-consumer-smoke -- --dry-run",
        "npm run artifacts:index",
        "npm run pulse:resume -- --dry-run",
        "../ao2-control-plane/scripts/cp-dr-restore-drill.sh --negative-only",
        "actions/upload-artifact@v7.0.1",
        "ao2-local-canary",
    ]:
        assert needle in workflow

    for needle in [
        "npm run release:artifact-consumer-smoke -- --require-artifact",
        "target/artifact-index/latest/dashboard.html",
        "npm run pulse:resume -- --execute",
        ".github/workflows/local-canary.yml",
        "ao2.artifact-evidence-dashboard.v1",
    ]:
        assert needle in verification


def test_release_artifact_consumer_smoke_bounds_gh_downloads():
    consumer = read("scripts/release-artifact-consumer-smoke.sh")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        "AO2_RELEASE_ARTIFACT_DOWNLOAD_TIMEOUT_SECONDS",
        "DOWNLOAD_FAILURES",
        "download_timeout_seconds",
        "download_failures",
        "subprocess.TimeoutExpired",
        "gh run list",
        "gh run download",
        "exit_code",
        "timed_out",
    ]:
        assert needle in consumer

    assert "AO2_RELEASE_ARTIFACT_DOWNLOAD_TIMEOUT_SECONDS" in verification
    assert "download_failures" in verification


def test_artifact_health_local_canary_bundle_and_pulse_execute_simulation_contracts():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["artifacts:health"]
        == "node scripts/run-sh-script.js scripts/artifact-evidence-health.sh"
    )

    health_script = REPO_ROOT / "scripts" / "artifact-evidence-health.sh"
    assert health_script.is_file()
    assert health_script.stat().st_mode & stat.S_IXUSR
    health = health_script.read_text(encoding="utf-8")
    for needle in [
        "ao2.artifact-evidence-health.v1",
        "artifact-index.json",
        "failing_bundles",
        "missing_bundles",
        "stale_bundles",
        "allowed_stale_bundles",
        "empty_bundles",
        "target/artifact-health/latest",
        "stores_credentials",
    ]:
        assert needle in health
    assert "OPENAI_API_KEY" not in health
    assert "ANTHROPIC_API_KEY" not in health

    pulse_resume = read("scripts/pulse-resume.sh")
    for needle in [
        "--resume-json",
        "simulation",
        "simulation_executed",
        "simulation_output_path",
        "ao2.pulse-execute-simulation.v1",
    ]:
        assert needle in pulse_resume

    workflow = read(".github/workflows/local-canary.yml")
    for needle in [
        "npm run artifacts:health",
        "ao2/target/artifact-health",
        "ao2/target/artifact-index/latest/artifact-index.json",
        "ao2/target/artifact-index/latest/dashboard.html",
        "ao2-control-plane/target/dr-restore-drill/local-canary/dr-restore-report.json",
    ]:
        assert needle in workflow

    for needle in [
        "npm run artifacts:health",
        "ao2.artifact-evidence-health.v1",
        "target/artifact-health/latest/summary.json",
        "Pulse execute simulation",
        "ao2.pulse-execute-simulation.v1",
    ]:
        assert needle in verification


def test_local_canary_runner_matches_manual_workflow_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["local:canary"]
        == "node scripts/run-sh-script.js scripts/local-canary.sh"
    )

    runner = REPO_ROOT / "scripts" / "local-canary.sh"
    assert runner.is_file()
    assert runner.stat().st_mode & stat.S_IXUSR
    text = runner.read_text(encoding="utf-8")
    for needle in [
        "ao2.local-canary-run.v1",
        "AO2_LOCAL_CANARY_STEP_TIMEOUT_SECONDS",
        "subprocess.TimeoutExpired",
        "start_new_session",
        "release:artifact-consumer-smoke -- --dry-run",
        "pulse:local-mirror",
        "pulse:resume -- --dry-run",
        "../ao2-control-plane/scripts/cp-dr-restore-drill.sh --negative-only",
        "artifacts:index",
        "artifacts:health",
        "step_results",
        "local-canary-summary.json",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    workflow = read(".github/workflows/local-canary.yml")
    assert "npm run local:canary" in workflow
    assert "ao2/target/local-canary" in workflow

    for needle in [
        "npm run local:canary",
        "ao2.local-canary-run.v1",
        "target/local-canary/latest/local-canary-summary.json",
    ]:
        assert needle in verification


def test_artifact_health_policy_knobs_contract():
    health = read("scripts/artifact-evidence-health.sh")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        "AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS",
        "AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS",
        "AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION",
        "AO2_ARTIFACT_HEALTH_STALE_AFTER_SECONDS",
        "required_roots",
        "allowed_missing_roots",
        "allowed_attention_bundles",
        "allowed_stale_bundles",
        "policy_violations",
        "fail_on_attention",
        "stale_threshold_override_seconds",
    ]:
        assert needle in health

    for needle in [
        "AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS",
        "AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS",
        "AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION",
        "AO2_ARTIFACT_HEALTH_STALE_AFTER_SECONDS",
    ]:
        assert needle in verification


def test_pulse_execute_safety_corpus_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["pulse:execute-safety-corpus"]
        == "node scripts/run-sh-script.js scripts/pulse-execute-safety-corpus.sh"
    )

    corpus = REPO_ROOT / "scripts" / "pulse-execute-safety-corpus.sh"
    assert corpus.is_file()
    assert corpus.stat().st_mode & stat.S_IXUSR
    corpus_text = corpus.read_text(encoding="utf-8")
    for needle in [
        "ao2.pulse-execute-safety-corpus.v1",
        "hash_mismatch",
        "unsafe_output_path",
        "missing_simulation_output_path",
        "failing_simulated_command",
        "dry_run_execute_conflict",
        "expected_exit_code",
        "expected_status",
        "expected_reason",
        "pulse-resume",
        "target/pulse-execute-safety-corpus/latest/summary.json",
    ]:
        assert needle in corpus_text
    assert "OPENAI_API_KEY" not in corpus_text
    assert "ANTHROPIC_API_KEY" not in corpus_text

    pulse_resume = read("scripts/pulse-resume.sh")
    for needle in [
        "simulated_exit_code",
        "simulated failure",
        "unsafe simulation_output_path",
    ]:
        assert needle in pulse_resume

    for needle in [
        "npm run pulse:execute-safety-corpus",
        "ao2.pulse-execute-safety-corpus.v1",
        "target/pulse-execute-safety-corpus/latest/summary.json",
    ]:
        assert needle in verification


def test_ci_artifact_download_contract_and_health_gate_wiring():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["artifacts:ci-download-contract"]
        == "node scripts/run-sh-script.js scripts/ci-artifact-download-contract.sh"
    )

    contract = REPO_ROOT / "scripts" / "ci-artifact-download-contract.sh"
    assert contract.is_file()
    assert contract.stat().st_mode & stat.S_IXUSR
    text = contract.read_text(encoding="utf-8")
    for needle in [
        "ao2.ci-artifact-download-contract.v1",
        "AO2_CI_ARTIFACT_CONTRACT_STEP_TIMEOUT_SECONDS",
        "subprocess.TimeoutExpired",
        "start_new_session",
        "target/ci-artifacts/latest",
        "release:artifact-consumer-smoke",
        "--require-artifact",
        "--require-schema",
        "--fixture-dir",
        "gh run download",
        "schema_versions",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    local_canary = read("scripts/local-canary.sh")
    regression_gate = read("scripts/release-readiness-regression-gate.sh")
    for runner in [local_canary, regression_gate]:
        for needle in [
            "artifacts:ci-download-contract",
            "AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION=1",
            "AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS",
            "target/ci-artifacts",
        ]:
            assert needle in runner

    workflow = read(".github/workflows/local-canary.yml")
    assert "ao2/target/ci-artifacts" in workflow
    assert "npm run artifacts:ci-download-contract" in workflow

    for needle in [
        "npm run artifacts:ci-download-contract",
        "ao2.ci-artifact-download-contract.v1",
        "target/ci-artifacts/latest/summary.json",
        "AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION=1",
    ]:
        assert needle in verification


def test_phase1_promotion_golden_path_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["phase1:promotion-golden"]
        == "node scripts/run-sh-script.js scripts/phase1-promotion-golden-path.sh"
    )

    script = REPO_ROOT / "scripts" / "phase1-promotion-golden-path.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.phase1-promotion-golden-path.v1",
        "smoke:phase1-operator-golden",
        "AO2_PHASE1_API_TOKEN_ENV",
        "Authorization: Bearer",
        "token_leak_scan",
        "dashboard_snapshot",
        "readback_summary",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run phase1:promotion-golden",
        "ao2.phase1-promotion-golden-path.v1",
        "target/phase1-promotion-golden/latest/summary.json",
    ]:
        assert needle in verification


def test_pulse_real_execute_containment_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["pulse:real-execute-containment"]
        == "node scripts/run-sh-script.js scripts/pulse-real-execute-containment.sh"
    )

    script = REPO_ROOT / "scripts" / "pulse-real-execute-containment.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.pulse-real-execute-containment.v1",
        "target/pulse-real-execute-containment/latest",
        "allowed-output",
        "pulse:resume -- --resume-json",
        "pulse:generate-next",
        "pulse:task-executor",
        "AO2_PULSE_CODE_AGENT_EXECUTE=1",
        "product_code_execution",
        "product_code_execute_fixture",
        "pulse_generate_next_summary",
        "pulse_task_executor_summary",
        "code_agent_summary",
        "allowed.txt",
        "--execute",
        "sha256_matches",
        "resume_command_digest",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run pulse:real-execute-containment",
        "ao2.pulse-real-execute-containment.v1",
        "product-code execute fixture",
        "pulse:task-executor",
        "pulse:code-agent-runner",
        "target/pulse-real-execute-containment/latest/summary.json",
    ]:
        assert needle in verification


def test_release_evidence_closure_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["release:evidence-closure"]
        == "node scripts/run-sh-script.js scripts/release-evidence-closure.sh"
    )

    script = REPO_ROOT / "scripts" / "release-evidence-closure.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.release-evidence-closure.v1",
        "AO2_RELEASE_EVIDENCE_CLOSURE_STEP_TIMEOUT_SECONDS",
        "subprocess.TimeoutExpired",
        "start_new_session",
        "local:canary",
        "artifacts:health",
        "phase1:promotion-golden",
        "pulse:execute-safety-corpus",
        "pulse:real-execute-containment",
        "control_plane_restore_negative",
        "closure.html",
        "evidence must exist before evaluator closure accepts a run",
        "ao2/target/release-evidence-closure",
        "AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS",
        "stores_credentials",
    ]:
        assert needle in text
    assert (
        'AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS="ao2/target/ci-artifacts '
        'ao2/.ao2-local/pulse/latest ao2-control-plane/target/ci-artifacts '
        'ao2-control-plane/target/dr-restore-drill"'
    ) in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run release:evidence-closure",
        "ao2.release-evidence-closure.v1",
        "target/release-evidence-closure/latest/summary.json",
        "target/release-evidence-closure/latest/closure.html",
    ]:
        assert needle in verification


def test_mvp_acceptance_matrix_gate_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["mvp:acceptance-matrix-gate"]
        == "node scripts/run-sh-script.js scripts/mvp-acceptance-matrix-gate.sh"
    )

    script = REPO_ROOT / "scripts" / "mvp-acceptance-matrix-gate.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.mvp-acceptance-matrix-gate.v1",
        "risky-pr:golden",
        "AC-01",
        "AC-12",
        "UAT-01",
        "UAT-12",
        "evidence must exist before evaluator closure accepts a run",
        "manual_filesystem_archaeology_required",
        "acceptance_matrix",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run mvp:acceptance-matrix-gate",
        "ao2.mvp-acceptance-matrix-gate.v1",
        "target/mvp-acceptance-matrix/latest/summary.json",
    ]:
        assert needle in verification


def test_no_archaeology_workbench_audit_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["workbench:no-archaeology-audit"]
        == "node scripts/run-sh-script.js scripts/no-archaeology-workbench-audit.sh"
    )

    script = REPO_ROOT / "scripts" / "no-archaeology-workbench-audit.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.no-archaeology-workbench-audit.v1",
        "objective",
        "denied_action",
        "approved_digest",
        "changed_files",
        "test_evidence",
        "rejection_reason",
        "correction",
        "closure_verdict",
        "export_path",
        "replay_status",
        "run_record_link",
        "static_report_link",
        "report_sections",
        "Local Run Record",
        "Static Export Evidence",
        "Evaluator Closure Evidence",
        "Replay Evidence",
        "manual_filesystem_archaeology_required",
        "workbench export",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run workbench:no-archaeology-audit",
        "ao2.no-archaeology-workbench-audit.v1",
        "target/no-archaeology-workbench/latest/summary.json",
        "run-record/report/evaluator-closure links",
    ]:
        assert needle in verification


def test_workbench_operator_evidence_packet_export_contract():
    source = read("crates/ao2-cli/src/main.rs")
    rust_tests = read("crates/ao2-cli/tests/cli_approval_replay.rs")
    verification = read("docs/VERIFICATION.md")
    ci = read(".github/workflows/ci.yml")

    for needle in [
        "operator-packet",
        "ao2.operator-evidence-packet.v1",
        "workbench_operator_evidence_packet_json",
        "run_record",
        "static_report",
        "evidence_pack",
        "evaluator_closure",
        "provider_scorecard",
        "operator_packet_run_id",
        "operator_packet_closure_verdict",
        "operator_packet_replay_status",
        "operator_packet_provider_score",
        "operator_packet_run_record_sha256",
        "operator_packet_evidence_pack_sha256",
    ]:
        assert needle in source

    for needle in [
        "cli_workbench_evidence_export_writes_operator_packet_for_support_readback",
        "kind=operator-packet&run_id=workbench-operator-packet",
        "ao2.operator-evidence-packet.v1",
        "support-verify",
        "support-inspect",
        "support-import",
        "operator_packet_static_report_present",
    ]:
        assert needle in rust_tests

    for needle in [
        "operator evidence packet",
        "ao2.operator-evidence-packet.v1",
        "local run record",
        "static report HTML",
        "evidence pack",
        "evaluator closure verdict",
        "replay status",
        "provider scorecard",
        "support-bundle readback",
    ]:
        assert needle in verification

    for os_name in ["ubuntu-latest", "macos-latest", "windows-latest"]:
        assert os_name in ci
    assert "cargo test -p ao2-cli --test cli_approval_replay cli_workbench_evidence" in ci


def test_control_plane_observer_hardening_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["control-plane:observer-hardening"]
        == "node scripts/run-sh-script.js scripts/control-plane-observer-hardening.sh"
    )

    script = REPO_ROOT / "scripts" / "control-plane-observer-hardening.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.control-plane-observer-hardening.v1",
        "smoke:evidence-control-plane",
        "cp-dr-restore-drill.sh",
        "negative_restore_drill",
        "long_lived_smoke",
        "artifact_health",
        "dashboard_schema_stability",
        "read_only_observer",
        "can_approve_runs",
        "can_mutate_ao2_evidence",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run control-plane:observer-hardening",
        "ao2.control-plane-observer-hardening.v1",
        "target/control-plane-observer-hardening/latest/summary.json",
    ]:
        assert needle in verification


def test_provider_phase2_contract_hardening_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["provider:phase2-contract-hardening"]
        == "node scripts/run-sh-script.js scripts/provider-phase2-contract-hardening.sh"
    )

    script = REPO_ROOT / "scripts" / "provider-phase2-contract-hardening.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.provider-phase2-contract-hardening.v1",
        "provider contract --verify --require codex",
        "provider contract --verify --require claude",
        "transcript_parsing_corpus",
        "sandbox_patch_digest_boundary",
        "exact_approval_enforcement",
        "blocker_taxonomy",
        "fail_closed_live_guards",
        "verify:no-factory-v3",
        "verify:replacement",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run provider:phase2-contract-hardening",
        "ao2.provider-phase2-contract-hardening.v1",
        "target/provider-phase2-contract-hardening/latest/summary.json",
    ]:
        assert needle in verification


def test_no_factory_v3_guard_allows_evaluator_closer_required_contract(tmp_path):
    repo = tmp_path / "repo"
    (repo / "scripts").mkdir(parents=True)
    (repo / "crates" / "ao2-cli" / "src").mkdir(parents=True)
    (repo / "crates" / "ao2-runtime" / "src").mkdir(parents=True)
    (repo / "package.json").write_text('{"scripts": {}}\n', encoding="utf-8")
    (repo / "scripts" / "contract.sh").write_text(
        '"factory_v3_evaluator_closer_required": True,\n',
        encoding="utf-8",
    )
    subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
    subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=repo, check=True)
    subprocess.run(["git", "config", "user.name", "AO2 Test"], cwd=repo, check=True)
    subprocess.run(["git", "add", "."], cwd=repo, check=True)
    subprocess.run(["git", "commit", "-q", "-m", "fixture"], cwd=repo, check=True)

    result = subprocess.run(
        [
            "bash",
            str(REPO_ROOT / "scripts" / "verify-no-factory-v3-green-path.sh"),
        ],
        cwd=repo,
        env={**os.environ, "AO2_ROOT": str(repo), "OUT_DIR": str(tmp_path / "out")},
        text=True,
        capture_output=True,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    report = json.loads((tmp_path / "out" / "no-factory-v3-green-path.json").read_text())
    assert report["status"] == "passed"
    assert report["failure_count"] == 0
    assert report["candidate_count"] == 1
    assert report["trust_boundary"]["factory_v3_role"] == "parity_oracle_or_audit_reference_only"


def test_public_release_train_drill_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["release:train-drill"]
        == "node scripts/run-sh-script.js scripts/public-release-train-drill.sh"
    )

    script = REPO_ROOT / "scripts" / "public-release-train-drill.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.public-release-train-drill.v1",
        "release:evidence-closure",
        "release:readiness:static",
        "release:readiness:regression-gate",
        "release:retention-preflight",
        "release:artifact-consumer-smoke -- --dry-run",
        "release_readiness_static",
        "ci_release_readiness_artifact_consumer_job",
        "release_readiness_artifact_consumer_contract",
        "artifact-closure-index.json",
        "ao2.release-artifact-closure-index.v1",
        "artifact_closure_index_contract",
        "release_train_control_plane_bridge",
        "expected_closure_artifacts",
        '"required_artifacts": expected_closure_artifacts',
        '"artifact_closure_index_contract": artifact_closure_index_contract',
        '"release_artifact_closure_index": str(artifact_closure_index_path)',
        'and artifact_closure_index_contract["status"] == "passed"',
        "AO2_RELEASE_TRAIN_PULSE_SOURCE",
        "release-train-pulse-seed",
        "pulse-eval-loop.json",
        "release:download-verify",
        "install_update_smoke_reference",
        "post-merge:canary",
        "refuses_publish_side_effects_by_default",
        "tag_push_publish_deploy",
        "closure.html",
    ]:
        assert needle in text
    assert "git push origin" not in text
    assert "gh release create" not in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run release:train-drill",
        "ao2.public-release-train-drill.v1",
        "target/public-release-train-drill/latest/summary.json",
        "release readiness static summary",
        "ci_release_readiness_artifact_consumer_job",
        "artifact-closure-index.json",
        "ao2.release-artifact-closure-index.v1",
    ]:
        assert needle in verification


def test_release_train_control_plane_bridge_contract(tmp_path):
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    ci = read(".github/workflows/ci.yml")

    assert (
        package_json["scripts"]["release:train-control-plane-bridge"]
        == "node scripts/run-sh-script.js scripts/release-train-control-plane-bridge.sh"
    )

    script = REPO_ROOT / "scripts" / "release-train-control-plane-bridge.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "AO2_CP_RELEASE_TRAIN_SUMMARY",
        "AO2_PUBLIC_RELEASE_TRAIN_DRILL_ROOT",
        "ao2.release-train-control-plane-bridge.v1",
        "ao2.public-release-train-drill.v1",
        "ao2.cp-release-train-bridge-smoke.v1",
        "smoke-release-train-bridge.py",
        "/api/v1/release/train",
        "/api/v1/release/train.json",
        "read-only-observer",
        "credential_material_included",
        "credential_material_in_urls",
        "env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY",
    ]:
        assert needle in text
    assert "git push origin" not in text
    assert "gh release create" not in text
    assert "OPENAI_API_KEY" not in text.replace("env -u OPENAI_API_KEY", "")
    assert "ANTHROPIC_API_KEY" not in text.replace(" -u ANTHROPIC_API_KEY", "")

    for needle in [
        "npm run release:train-control-plane-bridge",
        "ao2.release-train-control-plane-bridge.v1",
        "target/release-train-control-plane-bridge/latest/summary.json",
        "AO2_CP_RELEASE_TRAIN_SUMMARY",
        "ao2-release-train-control-plane-bridge",
    ]:
        assert needle in verification

    for needle in [
        "release-train-control-plane-bridge-artifacts:",
        "name: Release train control-plane bridge artifacts",
        "repository: uesugitorachiyo/ao2-control-plane",
        "AO2_RELEASE_TRAIN_CP_BRIDGE_ROOT=target/release-train-control-plane-bridge-ci",
        "npm run release:train-control-plane-bridge -- --summary ao2-control-plane/tests/fixtures/public-release-train-summary.json --control-plane-root ao2-control-plane",
        "ao2.release-train-control-plane-bridge.v1",
        "ao2.cp-release-train-bridge-smoke.v1",
        "target/release-train-control-plane-bridge-ci/latest/control-plane-smoke/summary.json",
        "name: ao2-release-train-control-plane-bridge",
        "uses: actions/upload-artifact@v7.0.1",
    ]:
        assert needle in ci

    release_summary = tmp_path / "release-train-summary.json"
    release_summary.write_text(
        json.dumps(
            {
                "schema_version": "ao2.public-release-train-drill.v1",
                "status": "passed",
                "checks": [{"name": "fixture", "status": "passed"}],
                "release_readiness_artifact_consumer_contract": {"status": "passed"},
                "publish_guards": {"refuses_publish_side_effects_by_default": True},
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    out_root = tmp_path / "bridge"
    cp_root = tmp_path / "ao2-control-plane"
    env = os.environ.copy()
    env["AO2_RELEASE_TRAIN_CP_BRIDGE_ROOT"] = str(out_root)
    result = subprocess.run(
        [
            "npm",
            "run",
            "release:train-control-plane-bridge",
            "--",
            "--summary",
            str(release_summary),
            "--control-plane-root",
            str(cp_root),
            "--skip-smoke",
        ],
        cwd=REPO_ROOT,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr + result.stdout

    stable_summary = out_root / "latest" / "release-train-summary.json"
    bridge_summary = out_root / "latest" / "summary.json"
    env_file = out_root / "latest" / "control-plane.env"
    cp_summary = (
        cp_root
        / "target"
        / "release-train-control-plane-bridge"
        / "release-train-summary.json"
    )
    assert stable_summary.is_file()
    assert cp_summary.is_file()
    assert stable_summary.read_text(encoding="utf-8") == cp_summary.read_text(
        encoding="utf-8"
    )
    assert env_file.is_file()

    summary = json.loads(bridge_summary.read_text(encoding="utf-8"))
    assert summary["schema_version"] == "ao2.release-train-control-plane-bridge.v1"
    assert summary["status"] == "passed"
    assert summary["release_train"]["schema_version"] == "ao2.public-release-train-drill.v1"
    assert summary["release_train"]["status"] == "passed"
    assert summary["control_plane"]["configured_env"] == "AO2_CP_RELEASE_TRAIN_SUMMARY"
    assert summary["control_plane"]["stable_summary"] == str(stable_summary)
    assert summary["control_plane"]["mirror_summary"] == str(cp_summary)
    assert summary["control_plane"]["smoke"] == "not_run"
    assert summary["control_plane"]["observer_schema"] == "ao2.cp-release-train-bridge-smoke.v1"
    assert summary["control_plane"]["role"] == "read-only-observer"
    assert summary["control_plane"]["credential_material_included"] is False
    assert summary["control_plane"]["credential_material_in_urls"] is False
    assert summary["trust_boundary"]["local_only"] is True
    assert summary["trust_boundary"]["control_plane_approves_release"] is False
    assert summary["trust_boundary"]["mutates_ao2_artifacts"] is False
    assert "Bearer" not in bridge_summary.read_text(encoding="utf-8")
    assert f"AO2_CP_RELEASE_TRAIN_SUMMARY={stable_summary}" in env_file.read_text(
        encoding="utf-8"
    )


def test_next_lengthy_gate_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["next:lengthy:gate"]
        == "node scripts/run-sh-script.js scripts/next-lengthy-gate.sh"
    )

    script = REPO_ROOT / "scripts" / "next-lengthy-gate.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.next-lengthy-gate.v1",
        "mvp:acceptance-matrix-gate",
        "workbench:no-archaeology-audit",
        "control-plane:observer-hardening",
        "provider:phase2-contract-hardening",
        "release:train-drill",
        "component_summaries",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run next:lengthy:gate",
        "ao2.next-lengthy-gate.v1",
        "target/next-lengthy-gate/latest/summary.json",
    ]:
        assert needle in verification


def test_cross_repo_control_plane_observer_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["control-plane:cross-repo-observer"]
        == "node scripts/run-sh-script.js scripts/cross-repo-control-plane-observer.sh"
    )

    script = REPO_ROOT / "scripts" / "cross-repo-control-plane-observer.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.cross-repo-control-plane-observer.v1",
        "AO2_CONTROL_PLANE_REPO",
        "../ao2-control-plane",
        "smoke-ingest-from-ao2.sh",
        "cp-health-snapshot.sh",
        "cp-dashboard-snapshot.sh",
        "cp-dr-restore-drill.sh",
        "signed_evidence_bundle",
        "read_only_observer",
        "can_approve_runs",
        "can_mutate_ao2_evidence",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run control-plane:cross-repo-observer",
        "ao2.cross-repo-control-plane-observer.v1",
        "target/cross-repo-control-plane-observer/latest/summary.json",
    ]:
        assert needle in verification


def test_release_install_update_fixture_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["release:install-update-fixture"]
        == "node scripts/run-sh-script.js scripts/release-install-update-fixture.sh"
    )

    script = REPO_ROOT / "scripts" / "release-install-update-fixture.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.release-install-update-fixture.v1",
        "AO2_RELEASE_INSTALL_UPDATE_FIXTURE_DIR",
        "SHA256SUMS",
        "provenance.json",
        "signature",
        "checksum_verification",
        "install_smoke",
        "update_smoke",
        "release:download-verify",
        "refuses_publish_side_effects_by_default",
        "stores_credentials",
        'OUT_ROOT="$(cd "$OUT_ROOT" && pwd)"',
        'FIXTURE_DIR="$(cd "$FIXTURE_DIR" && pwd)"',
    ]:
        assert needle in text
    assert "git push origin" not in text
    assert "gh release create" not in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run release:install-update-fixture",
        "ao2.release-install-update-fixture.v1",
        "target/release-install-update-fixture/latest/summary.json",
    ]:
        assert needle in verification


def test_workbench_browser_qa_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["workbench:browser-qa"]
        == "node scripts/run-sh-script.js scripts/workbench-browser-qa.sh"
    )

    script = REPO_ROOT / "scripts" / "workbench-browser-qa.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.workbench-browser-qa.v1",
        "workbench:no-archaeology-audit",
        "browser_review",
        "screenshot_manifest",
        "html_inspection",
        "objective",
        "denied_action",
        "approved_digest",
        "changed_files",
        "test_evidence",
        "manual_filesystem_archaeology_required",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run workbench:browser-qa",
        "ao2.workbench-browser-qa.v1",
        "target/workbench-browser-qa/latest/summary.json",
    ]:
        assert needle in verification


def test_provider_adversarial_corpus_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["provider:adversarial-corpus"]
        == "node scripts/run-sh-script.js scripts/provider-adversarial-corpus.sh"
    )

    script = REPO_ROOT / "scripts" / "provider-adversarial-corpus.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.provider-adversarial-corpus.v1",
        "fixtures/provider-adversarial-corpus",
        "malformed_transcript",
        "approval_boundary_attempt",
        "patch_digest_mismatch",
        "blocker_taxonomy",
        "fail_closed",
        "cargo test -p ao2-adapters transcript",
        "provider:phase2-contract-hardening",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    fixture = REPO_ROOT / "fixtures" / "provider-adversarial-corpus" / "manifest.json"
    assert fixture.is_file()
    manifest = json.loads(fixture.read_text(encoding="utf-8"))
    assert manifest["schema_version"] == "ao2.provider-adversarial-corpus.manifest.v1"
    assert {case["category"] for case in manifest["cases"]} >= {
        "malformed_transcript",
        "approval_boundary_attempt",
        "patch_digest_mismatch",
    }

    for needle in [
        "npm run provider:adversarial-corpus",
        "ao2.provider-adversarial-corpus.v1",
        "target/provider-adversarial-corpus/latest/summary.json",
    ]:
        assert needle in verification


def test_dr_retention_snapshot_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["release:dr-retention-snapshot"]
        == "node scripts/run-sh-script.js scripts/dr-retention-long-run-snapshot.sh"
    )

    script = REPO_ROOT / "scripts" / "dr-retention-long-run-snapshot.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.dr-retention-long-run-snapshot.v1",
        "cp-dr-restore-drill.sh",
        "release:retention-preflight",
        "artifacts:index",
        "artifacts:health",
        "fixture_snapshot_manifest",
        "restore_drill_evidence",
        "retention_preflight_evidence",
        "artifact_health_evidence",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run release:dr-retention-snapshot",
        "ao2.dr-retention-long-run-snapshot.v1",
        "target/dr-retention-long-run-snapshot/latest/summary.json",
    ]:
        assert needle in verification


def test_frontier_lengthy_gate_contract():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["frontier:lengthy:gate"]
        == "node scripts/run-sh-script.js scripts/frontier-lengthy-gate.sh"
    )

    script = REPO_ROOT / "scripts" / "frontier-lengthy-gate.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.frontier-lengthy-gate.v1",
        "control-plane:cross-repo-observer",
        "release:install-update-fixture",
        "workbench:browser-qa",
        "provider:adversarial-corpus",
        "release:dr-retention-snapshot",
        "component_summaries",
        "stores_credentials",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text

    for needle in [
        "npm run frontier:lengthy:gate",
        "ao2.frontier-lengthy-gate.v1",
        "target/frontier-lengthy-gate/latest/summary.json",
    ]:
        assert needle in verification


def test_pulse_lengthy_gate_runner_is_exposed_and_contract_safe():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")

    assert (
        package_json["scripts"]["pulse:lengthy-gate"]
        == "node scripts/run-sh-script.js scripts/pulse-lengthy-gate-runner.sh"
    )
    assert (
        package_json["scripts"]["pulse:lengthy-gate:contract"]
        == "node scripts/run-sh-script.js scripts/pulse-lengthy-gate-runner.sh --contract"
    )

    runner = REPO_ROOT / "scripts" / "pulse-lengthy-gate-runner.sh"
    manifest_path = REPO_ROOT / "scripts" / "pulse-lengthy-gates-manifest.json"
    assert runner.is_file()
    assert runner.stat().st_mode & stat.S_IXUSR
    assert manifest_path.is_file()

    runner_text = runner.read_text(encoding="utf-8")
    for needle in [
        "ao2.pulse-lengthy-gate-runner.v1",
        "missing_package_commands",
        "--contract",
        "--gate",
        "--list",
        "stores_credentials",
        "deletes_files",
        "pushes",
        'status in {"blocked", "failed"}',
    ]:
        assert needle in runner_text
    assert "OPENAI_API_KEY" not in runner_text
    assert "ANTHROPIC_API_KEY" not in runner_text
    assert "gh release create" not in runner_text
    assert "git push origin" not in runner_text

    for needle in [
        "npm run pulse:lengthy-gate:contract",
        "npm run pulse:lengthy-gate -- --gate",
        "ao2.pulse-lengthy-gates-manifest.v1",
        "ao2.pulse-lengthy-gate-runner.v1",
        "manifest-driven",
        "missing_package_commands",
    ]:
        assert needle in verification


def test_pulse_lengthy_gate_manifest_preserves_wrapper_intent_without_private_auth():
    manifest = json.loads(read("scripts/pulse-lengthy-gates-manifest.json"))

    assert manifest["schema_version"] == "ao2.pulse-lengthy-gates-manifest.v1"
    assert manifest["trust_boundary"]["local_only"] is True
    assert manifest["trust_boundary"]["stores_credentials"] is False
    assert manifest["trust_boundary"]["side_effects"] == (
        "runner_blocks_missing_commands_before_execution"
    )
    assert len(manifest["gates"]) >= 10

    replacements = {gate["replaces"] for gate in manifest["gates"]}
    assert "scripts/pulse-consolidation-lengthy-gate.sh" in replacements
    assert "scripts/pulse-useful-lengthy-gate.sh" in replacements
    assert "scripts/pulse-final-sweep-lengthy-gate.sh" in replacements

    text = json.dumps(manifest, sort_keys=True)
    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
    ]:
        assert forbidden not in text

    for gate in manifest["gates"]:
        assert gate["disposition"] == "preserve_then_consolidate"
        assert gate["replaces"].startswith("scripts/")
        assert gate["replaces"].endswith(".sh")
        assert gate["commands"]
        assert all(isinstance(command, str) for command in gate["commands"])


def test_pulse_consolidation_manifest_gate_is_promoted_to_public_scripts():
    package_json = json.loads(read("package.json"))
    manifest = json.loads(read("scripts/pulse-lengthy-gates-manifest.json"))
    gate = next(item for item in manifest["gates"] if item["id"] == "pulse-consolidation")

    expected_scripts = {
        "pulse:shared-gate-lib-audit": "node scripts/run-sh-script.js scripts/pulse-shared-gate-lib-audit.sh",
        "public:hardening": "node scripts/run-sh-script.js scripts/public-hardening-subset.sh",
        "scripts:tracking-intent-audit": "node scripts/run-sh-script.js scripts/script-tracking-intent-audit.sh",
    }
    for script_name, command in expected_scripts.items():
        assert package_json["scripts"][script_name] == command

    assert not [
        command for command in gate["commands"] if command not in package_json["scripts"]
    ]

    for script_name in [
        "pulse-shared-gate-lib-audit.sh",
        "public-hardening-subset.sh",
        "script-tracking-intent-audit.sh",
    ]:
        script = REPO_ROOT / "scripts" / script_name
        assert script.is_file()
        assert script.stat().st_mode & stat.S_IXUSR


def test_public_hardening_ci_workflow_is_tracked_and_public_safe():
    package_json = json.loads(read("package.json"))
    runner = read("scripts/run-sh-script.js")
    gate_lib = read("scripts/lib/pulse-gate-lib.sh")
    expected_scripts = {
        "public:hardening-ci-workflow": "node scripts/run-sh-script.js scripts/public-hardening-ci-workflow.sh",
        "public:hardening-workflow-file-dry-run": "node scripts/run-sh-script.js scripts/public-hardening-workflow-file-dry-run.sh",
        "public:hardening-workflow-tracked-proposal": "node scripts/run-sh-script.js scripts/public-hardening-workflow-tracked-proposal.sh",
        "public:hardening-ci-local-runner-parity": "node scripts/run-sh-script.js scripts/public-hardening-ci-local-runner-parity.sh",
    }
    for script_name, command in expected_scripts.items():
        assert package_json["scripts"][script_name] == command

    assert 'commandExists("bash")' in runner
    assert 'windowsShellCandidates("bash")' in runner
    assert "spawnSync(shell, [script, ...scriptArgs]" in runner

    assert "command -v rg" in gate_lib
    assert "grep -R -n -E" in gate_lib

    workflow = read(".github/workflows/ao2-public-hardening.yml")
    assert re.search(r"(?m)^\s*pull_request:\s*$", workflow)
    assert re.search(r"(?m)^\s*workflow_dispatch:\s*$", workflow)
    assert not re.search(r"(?m)^\s*push:\s*$", workflow)
    assert "permissions:\n  contents: read" in workflow
    assert "uses: actions/checkout@v6.0.3" in workflow
    assert "uses: actions/setup-node@v6.4.0" in workflow
    assert "node-version: \"22\"" in workflow
    assert "package-lock.json" in workflow
    assert "npm-shrinkwrap.json" in workflow
    assert "npm install --ignore-scripts --no-audit --no-fund --package-lock=false" in workflow
    if not (REPO_ROOT / "package-lock.json").exists():
        assert "run: npm ci" not in workflow
    for command in [
        "AO2_PULSE_GENERATE_NEXT_REGISTER=0 npm run pulse:generate-next",
        "AO2_PULSE_LOCAL_MIRROR_SOURCE=target/pulse-next-recommended-tasks npm run pulse:local-mirror",
        "python3 -m pip install pytest",
        "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q",
        "npm run public:hardening",
        "npm run pulse:resume -- --dry-run",
    ]:
        assert f"run: {command}" in workflow

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "target/long-lived-control-plane/api-token",
        "/Users/",
        "gh release create",
        "git push",
        "npm publish",
    ]:
        assert forbidden not in workflow


def test_promoted_public_hardening_ci_scripts_are_clean_checkout_safe():
    promoted = [
        "scripts/public-hardening-ci-workflow.sh",
        "scripts/public-hardening-workflow-file-dry-run.sh",
        "scripts/public-hardening-workflow-tracked-proposal.sh",
        "scripts/public-hardening-ci-local-runner-parity.sh",
    ]
    for script_path in promoted:
        script = REPO_ROOT / script_path
        assert script.is_file()
        assert script.stat().st_mode & stat.S_IXUSR
        text = script.read_text(encoding="utf-8")
        assert "scripts/lib/pulse-gate-lib.sh" in text
        assert "ao2.public-hardening" in text
        if script_path == "scripts/public-hardening-ci-workflow.sh":
            assert 'AO2_PULSE_LOCAL_MIRROR_DEST="$OUT_ROOT/pulse-local-mirror"' in text
            assert '"pulse_local_mirror_seed": str(out_root / "pulse-local-mirror" / "pulse-local-mirror-summary.json")' in text
        for forbidden in [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "target/long-lived-control-plane/api-token",
            "/Users/",
            "gh release create",
            "git push",
            "npm publish",
            "actions/checkout@v4",
            "actions/setup-node@v4",
        ]:
            assert forbidden not in text


def test_promoted_pulse_consolidation_scripts_are_public_safe_and_clean_checkout_safe():
    verification = read("docs/VERIFICATION.md")
    scripts = {
        "pulse-shared-gate-lib-audit.sh": [
            "ao2.pulse-shared-gate-lib-audit.v1",
            "ao2.pulse-gate-lib.v1",
            "ao2_gate_forbidden_string_scan",
        ],
        "public-hardening-subset.sh": [
            "ao2.public-hardening-subset.v1",
            "test_public_stabilization.py",
            "pulse:lengthy-gate:contract",
            "scripts/pulse-lengthy-gate-runner.sh",
        ],
        "script-tracking-intent-audit.sh": [
            "ao2.script-tracking-intent-audit.v1",
            "ao2.script-tracking-manifest.v1",
            "track_in_repo",
        ],
    }

    for script_name, needles in scripts.items():
        text = read(f"scripts/{script_name}")
        for needle in needles:
            assert needle in text
        for forbidden in [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "/Users/torachiyouesugi/Documents/private",
            "target/long-lived-control-plane/api-token",
            "gh release create",
            "git push origin",
        ]:
            assert forbidden not in text

    public_hardening = read("scripts/public-hardening-subset.sh")
    assert "scripts/pulse-consolidation-lengthy-gate.sh" not in public_hardening
    assert "npm run pulse:lengthy-gate -- --gate pulse-consolidation" in verification
    assert "ao2.pulse-lengthy-gate-runner.v1" in verification


def test_script_surface_audit_preserves_local_rsi_scripts_before_promotion():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    assert (
        package_json["scripts"]["scripts:surface-audit"]
        == "node scripts/run-sh-script.js scripts/script-surface-audit.sh"
    )

    script_path = REPO_ROOT / "scripts" / "script-surface-audit.sh"
    assert script_path.is_file()
    assert script_path.stat().st_mode & stat.S_IXUSR
    text = script_path.read_text(encoding="utf-8")

    for needle in [
        "ao2.script-surface-audit.v1",
        "snapshot_manifest",
        "classification_report",
        "missing_package_commands",
        "promote_candidates",
        "defer_control_plane",
        "consolidate",
        "no_auto_promotion",
        "scripts/lib/pulse-gate-lib.sh",
    ]:
        assert needle in text

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text

    assert "npm run scripts:surface-audit" in verification
    assert "ao2.script-surface-audit.v1" in verification
    assert "target/script-surface-audit/latest/summary.json" in verification


def test_shared_gate_library_migration_is_promoted_as_public_safe_gate():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    assert (
        package_json["scripts"]["pulse:shared-gate-library-migration"]
        == "node scripts/run-sh-script.js scripts/shared-gate-library-migration.sh"
    )

    script_path = REPO_ROOT / "scripts" / "shared-gate-library-migration.sh"
    assert script_path.is_file()
    assert script_path.stat().st_mode & stat.S_IXUSR
    text = script_path.read_text(encoding="utf-8")

    for needle in [
        "ao2.shared-gate-library-migration.v1",
        "ao2.shared-gate-library-migration.matrix.v1",
        "pulse:shared-gate-lib-audit",
        "helper_adoption_matrix",
        "behavior_preservation_check",
        "scripts/lib/pulse-gate-lib.sh",
    ]:
        assert needle in text

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text

    assert "npm run pulse:shared-gate-library-migration" in verification
    assert "ao2.shared-gate-library-migration.v1" in verification
    assert "target/shared-gate-library-migration/latest/summary.json" in verification


def test_script_tracking_decision_cleanup_is_promoted_as_public_safe_gate():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    public_hardening = read("scripts/public-hardening-subset.sh")
    assert (
        package_json["scripts"]["scripts:tracking-decision-cleanup"]
        == "node scripts/run-sh-script.js scripts/script-tracking-decision-cleanup.sh"
    )

    script_path = REPO_ROOT / "scripts" / "script-tracking-decision-cleanup.sh"
    assert script_path.is_file()
    assert script_path.stat().st_mode & stat.S_IXUSR
    text = script_path.read_text(encoding="utf-8")

    for needle in [
        "ao2.script-tracking-decision-cleanup.v1",
        "ao2.script-pre-commit-cleanup-list.v1",
        "scripts:tracking-intent-audit",
        "track_in_repo_decisions",
        "keep_local_only_decisions",
        "pre_commit_cleanup_list",
        "scripts/lib/pulse-gate-lib.sh",
    ]:
        assert needle in text

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text

    assert "scripts/script-tracking-decision-cleanup.sh" in public_hardening
    assert "npm run scripts:tracking-decision-cleanup" in verification
    assert "ao2.script-tracking-decision-cleanup.v1" in verification
    assert "target/script-tracking-decision-cleanup/latest/summary.json" in verification


def test_script_tracking_review_pack_is_promoted_as_public_safe_gate():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    public_hardening = read("scripts/public-hardening-subset.sh")
    assert (
        package_json["scripts"]["scripts:tracking-review-pack"]
        == "node scripts/run-sh-script.js scripts/script-tracking-review-pack.sh"
    )

    script_path = REPO_ROOT / "scripts" / "script-tracking-review-pack.sh"
    assert script_path.is_file()
    assert script_path.stat().st_mode & stat.S_IXUSR
    text = script_path.read_text(encoding="utf-8")

    for needle in [
        "ao2.script-tracking-review-pack.v1",
        "ao2.script-tracking-review-pack.payload.v1",
        "scripts:tracking-decision-cleanup",
        "tracked_script_candidates",
        "local_only_artifacts",
        "pre_commit_review",
        "tracking_review_pack",
        "scripts/lib/pulse-gate-lib.sh",
    ]:
        assert needle in text

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text

    assert "scripts/script-tracking-review-pack.sh" in public_hardening
    assert "npm run scripts:tracking-review-pack" in verification
    assert "ao2.script-tracking-review-pack.v1" in verification
    assert "target/script-tracking-review-pack/latest/summary.json" in verification


def test_script_tracking_review_to_commit_plan_is_promoted_as_public_safe_gate():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    public_hardening = read("scripts/public-hardening-subset.sh")
    assert (
        package_json["scripts"]["scripts:tracking-review-to-commit-plan"]
        == "node scripts/run-sh-script.js scripts/script-tracking-review-to-commit-plan.sh"
    )

    script_path = REPO_ROOT / "scripts" / "script-tracking-review-to-commit-plan.sh"
    assert script_path.is_file()
    assert script_path.stat().st_mode & stat.S_IXUSR
    text = script_path.read_text(encoding="utf-8")

    for needle in [
        "ao2.script-tracking-review-to-commit-plan.v1",
        "ao2.script-tracking-commit-plan.payload.v1",
        "scripts:tracking-review-pack",
        "--untracked-files=no",
        "--untracked-files=all",
        "tracked_script_set",
        "excluded_local_artifacts",
        "minimal_commit_plan",
        "pre_commit_review_status",
        "scripts/lib/pulse-gate-lib.sh",
    ]:
        assert needle in text

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text

    assert "scripts/script-tracking-review-to-commit-plan.sh" in public_hardening
    assert "npm run scripts:tracking-review-to-commit-plan" in verification
    assert "ao2.script-tracking-review-to-commit-plan.v1" in verification
    assert "target/script-tracking-review-to-commit-plan/latest/summary.json" in verification


def test_script_tracking_commit_ready_diff_is_promoted_as_public_safe_gate():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    public_hardening = read("scripts/public-hardening-subset.sh")
    assert (
        package_json["scripts"]["scripts:tracking-commit-ready-diff"]
        == "node scripts/run-sh-script.js scripts/script-tracking-commit-ready-diff.sh"
    )

    script_path = REPO_ROOT / "scripts" / "script-tracking-commit-ready-diff.sh"
    assert script_path.is_file()
    assert script_path.stat().st_mode & stat.S_IXUSR
    text = script_path.read_text(encoding="utf-8")

    for needle in [
        "ao2.script-tracking-commit-ready-diff.v1",
        "ao2.script-tracking-commit-ready-diff.manifest.v1",
        "scripts:tracking-review-to-commit-plan",
        "--untracked-files=no",
        "--untracked-files=all",
        "commit_ready_diff_manifest",
        "tracked_file_diff",
        "excluded_local_artifacts",
        "no_commit_or_push",
        "scripts/lib/pulse-gate-lib.sh",
    ]:
        assert needle in text

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text

    assert "scripts/script-tracking-commit-ready-diff.sh" in public_hardening
    assert "npm run scripts:tracking-commit-ready-diff" in verification
    assert "ao2.script-tracking-commit-ready-diff.v1" in verification
    assert "target/script-tracking-commit-ready-diff/latest/summary.json" in verification


def test_script_tracking_ready_review_pack_is_promoted_as_public_safe_gate():
    package_json = json.loads(read("package.json"))
    verification = read("docs/VERIFICATION.md")
    public_hardening = read("scripts/public-hardening-subset.sh")
    assert (
        package_json["scripts"]["scripts:tracking-ready-review-pack"]
        == "node scripts/run-sh-script.js scripts/script-tracking-ready-review-pack.sh"
    )

    script_path = REPO_ROOT / "scripts" / "script-tracking-ready-review-pack.sh"
    assert script_path.is_file()
    assert script_path.stat().st_mode & stat.S_IXUSR
    text = script_path.read_text(encoding="utf-8")

    for needle in [
        "ao2.script-tracking-ready-review-pack.v1",
        "ao2.script-tracking-ready-review-pack.summary.v1",
        "scripts:tracking-commit-ready-diff",
        "human_review_packet",
        "commit_ready_summary",
        "excluded_local_artifacts",
        "no_commit_or_push",
        'item.get("path"',
        "scripts/lib/pulse-gate-lib.sh",
    ]:
        assert needle in text

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text

    assert "scripts/script-tracking-ready-review-pack.sh" in public_hardening
    assert "npm run scripts:tracking-ready-review-pack" in verification
    assert "ao2.script-tracking-ready-review-pack.v1" in verification
    assert "target/script-tracking-ready-review-pack/latest/summary.json" in verification


def test_cross_os_release_attestation_is_ci_safe_and_separates_optional_native_proof():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:cross-os-attestation"]
        == "node scripts/run-sh-script.js scripts/cross-os-release-artifact-attestation.sh"
    )

    script_path = REPO_ROOT / "scripts" / "cross-os-release-artifact-attestation.sh"
    assert script_path.is_file()
    assert script_path.stat().st_mode & stat.S_IXUSR

    text = script_path.read_text(encoding="utf-8")
    for needle in [
        "ao2.cross-os-release-attestation.v1",
        "required_ci_checks",
        "optional_native_checks",
        "platform_matrix",
        "ci_safe_required",
        "native_execution_optional",
        "download_verification_optional",
        "AO2_CROSS_OS_ATTESTATION_ENABLE_THREE_OS",
        "AO2_CROSS_OS_ATTESTATION_ENABLE_DOWNLOAD",
        "AO2_CROSS_OS_ATTESTATION_REQUIRE_NATIVE",
        "AO2_CROSS_OS_ATTESTATION_REQUIRE_DOWNLOAD",
        "macos-aarch64",
        "linux-aarch64",
        "linux-x86_64",
        "windows-x86_64",
        "tag_push_publish_deploy",
        "release_publish",
        "stores_credentials",
    ]:
        assert needle in text

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in text


def test_post_stable_release_verification_workflow_runs_hosted_consumer_smoke():
    workflow = read(".github/workflows/post-stable-release-verification.yml")

    for needle in [
        "name: Post Stable Release Verification",
        "workflow_dispatch:",
        "schedule:",
        "ubuntu-latest",
        "macos-14",
        "windows-latest",
        "AO2_RELEASE_TAG: v0.4.80",
        "ao2-0.4.80-linux-x86_64.tar.gz",
        "ao2-0.4.80-linux-x86_64.tar.gz.sha256",
        "ao2-0.4.80-linux-x86_64.tar.gz.sig",
        "ao2-0.4.80-macos-aarch64.tar.gz",
        "ao2-0.4.80-macos-aarch64.tar.gz.sha256",
        "ao2-0.4.80-macos-aarch64.tar.gz.sig",
        "ao2-0.4.80-windows-x86_64.tar.gz",
        "ao2-0.4.80-windows-x86_64.tar.gz.sha256",
        "ao2-0.4.80-windows-x86_64.tar.gz.sig",
        "ao2-release-provenance.json",
        "ao2-release-provenance.json.sig",
        "ao2-release-signing-public.pem",
        "gh release download",
        "SHA256SUMS",
        "AO2_INSTALL_DIR",
        "install update",
        "--provenance-dir",
        "signature_verified",
        '"status": "installed"',
        "version --json",
        "doctor --json",
        "adapter doctor --provider scripted",
        "post-stable-release-smoke",
    ]:
        assert needle in workflow

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
        "gh release create",
        "git push origin",
        "npm publish",
    ]:
        assert forbidden not in workflow


def test_dual_repo_public_release_verification_index_is_documented():
    doc_path = REPO_ROOT / "docs/release/PUBLIC-RELEASE-VERIFICATION.md"
    assert doc_path.is_file()

    doc = doc_path.read_text(encoding="utf-8")
    readme = read("README.md")
    verification = read("docs/VERIFICATION.md")

    for needle in [
        "# Public Release Verification",
        "uesugitorachiyo/ao2",
        "uesugitorachiyo/ao2-control-plane",
        "v0.4.80",
        "v0.1.13",
        "Post Stable Release Verification",
        ".github/workflows/post-stable-release-verification.yml",
        "post-stable-release-smoke-${{ runner.os }}",
        "Post Release Verification",
        ".github/workflows/post-release-verification.yml",
        "ao2-control-plane-post-release-verification-ubuntu",
        "ao2-control-plane-post-release-verification-macos",
        "ao2-control-plane-post-release-verification-windows",
        "ao2-release-publication-closure",
        "ao2-dual-repo-release-publication-closure-index",
        "ao2-control-plane-release-publication-closure",
        "ao2.release-publication-dry-run-closure.v1",
        "ao2.dual-repo-release-publication-closure-index.v1",
        "ao2.cp-release-publication-closure.v1",
        "read-only",
        "checksum_verified",
        "mutates_github_releases=false",
        "credential_material_included=false",
        "gh run download",
    ]:
        assert needle in doc

    for forbidden in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "/Users/torachiyouesugi/Documents/private",
        "target/long-lived-control-plane/api-token",
    ]:
        assert forbidden not in doc

    assert "[Public release verification](docs/release/PUBLIC-RELEASE-VERIFICATION.md)" in readme
    assert "docs/release/PUBLIC-RELEASE-VERIFICATION.md" in verification
    assert "ao2.dual-repo-release-publication-closure-index.v1" in verification


def test_release_immutability_audit_composes_stable_asset_and_download_checks():
    package_json = json.loads(read("package.json"))
    assert (
        package_json["scripts"]["release:immutability-audit"]
        == "node scripts/run-sh-script.js scripts/release-immutability-audit.sh"
    )

    script_path = REPO_ROOT / "scripts" / "release-immutability-audit.sh"
    assert script_path.is_file()
    assert script_path.stat().st_mode & stat.S_IXUSR

    text = script_path.read_text(encoding="utf-8")
    for needle in [
        "ao2.release-immutability-audit.v1",
        "npm run release:asset-completeness",
        "npm run release:stable-readiness",
        "npm run release:download-verify",
        "AO2_IMMUTABILITY_SKIP_DOWNLOAD_VERIFY",
        "release_metadata",
        "asset_completeness_summary",
        "stable_readiness_summary",
        "download_verify_log",
        "checksums",
        "signed_provenance",
        "release_metadata_coherent",
        "control_plane_approves_release",
        "mutates_releases",
        "target/release-immutability-audit/latest/summary.json",
    ]:
        assert needle in text

    verification = read("docs/VERIFICATION.md")
    install = read("docs/INSTALL.md")
    readme = read("README.md")
    next_patch = read("docs/release/v0.4.81-ai-task-board-control-surface.md")
    pulse_generate_next = read("scripts/pulse-generate-next.sh")

    assert "npm run release:immutability-audit" in verification
    assert "ao2.release-immutability-audit.v1" in verification
    assert "stable public release" in readme
    assert "v0.4.80" in readme
    assert "ao2-0.4.80-linux-aarch64.tar.gz" in readme
    assert "https://youtu.be/p222b0iCpbg" in readme
    assert "stable public release" in install
    assert "v0.4.81" in next_patch
    assert "AI task board" in next_patch
    assert "control surface" in next_patch
    assert "Pulse loops stop\n  drifting" in next_patch
    assert "AO2_PULSE_TASK_BOARD_STATUS_EVIDENCE" in next_patch
    assert "ao2.control-plane-operator-task-board-view.v1" in next_patch
    assert "next_action" in next_patch
    assert "AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE" in next_patch
    assert "ai-task-board-control-surface" in pulse_generate_next
    assert "ao2.ai-task-board.v1" in pulse_generate_next
    assert "status_transition_source" in pulse_generate_next
    assert "task-board.json" in verification
