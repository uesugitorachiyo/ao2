import json
import os
import re
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
    assert "permissions:\n  contents: read" in ci

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
    ]
    for needle in required:
        assert needle in text
    assert "cat target/long-lived-control-plane/api-token" not in text


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
    }
    for command, expected in expected_scripts.items():
        assert package_json["scripts"][command] == expected

    for script_name, schema in [
        ("pulse-generate-next.sh", "ao2.pulse-generate-next.v1"),
        ("pulse-generate-next-contract.sh", "ao2.pulse-generate-next-contract.v1"),
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
        "product_code_execution",
        "packet.md",
        "board.md",
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

    for needle in [
        "npm run pulse:generate-next",
        "npm run pulse:generate-next:contract",
        "ao2.pulse-generate-next.v1",
        "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED=1",
        "local-only while PR-blocked mode",
        "target/pulse-generate-next/latest/summary.json",
    ]:
        assert needle in verification


def test_pulse_generate_next_writes_structured_task_manifest(tmp_path):
    out_root = tmp_path / "generate-next"
    packet_root = tmp_path / "packet"
    cursor = tmp_path / "cursor.json"

    result = subprocess.run(
        ["npm", "run", "pulse:generate-next"],
        cwd=REPO_ROOT,
        env={
            **os.environ,
            "AO2_PULSE_GENERATE_NEXT_REGISTER": "0",
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
        "release:readiness:regression-gate",
        "release:retention-preflight",
        "release:artifact-consumer-smoke -- --dry-run",
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
    ]:
        assert needle in verification


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
