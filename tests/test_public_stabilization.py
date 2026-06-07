import json
import re
import stat
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
        "Policy Decisions",
        "Closure Reports",
        "Run Markers",
    ]
    for needle in required:
        assert needle in text
    assert "OPENAI_API_KEY=" not in text
    assert "ANTHROPIC_API_KEY=" not in text


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
        "Prefer public reliability, Ubuntu/macOS/Windows correctness, CI confidence, "
        "evidence quality, security/safety boundaries, control-plane integration, "
        "release readiness, and developer/operator usability. Avoid narrow recursion "
        "or low-value daemon work unless it is the bottleneck. Generate next lengthy "
        "tasks with rationale, required evidence, and stop conditions, then register "
        "and continue through the AO2 event loop."
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
        "pulse:resume-workspace-cli-fallback": "scripts/pulse-resume-workspace-cli-fallback.sh",
        "pulse:terminal-eval-loop-schema-compatibility": "scripts/pulse-terminal-eval-loop-schema-compatibility.sh",
        "pulse:auto-advance-runner-contract": "scripts/pulse-auto-advance-runner-contract.sh",
        "pulse:stop-and-dedup-ledger": "scripts/pulse-stop-and-dedup-ledger.sh",
        "pulse:auto-advance-integration-gate": "scripts/pulse-auto-advance-integration-gate.sh",
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
        "duplicate_eval_loop_digest",
        "waiting_for_new_eval_loop_digest",
        "continue_until_stopped",
        "--forever",
        "MAX_ITERATIONS=0",
        "sleep_seconds",
        "max_iterations",
    ]:
        assert needle in runner

    script_needles = {
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
        ],
        "scripts/pulse-stop-and-dedup-ledger.sh": [
            "ao2.pulse-stop-and-dedup-ledger.v1",
            "AO2_PULSE_AUTO_ADVANCE_STOP_FILE",
            "duplicate_eval_loop_digest",
            "pulse-auto-advance-ledger.jsonl",
        ],
        "scripts/pulse-auto-advance-integration-gate.sh": [
            "ao2.pulse-auto-advance-integration-gate.v1",
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
        "npm run pulse:resume-workspace-cli-fallback",
        "npm run pulse:terminal-eval-loop-schema-compatibility",
        "npm run pulse:auto-advance-runner-contract",
        "npm run pulse:stop-and-dedup-ledger",
        "npm run pulse:auto-advance-integration-gate",
        "ao2.pulse-auto-advance-run.v1",
        "target/pulse-auto-advance/latest/summary.json",
    ]:
        assert needle in verification


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
        "cross-platform-compatibility",
        "Ubuntu macOS Windows compatibility evidence",
        "release:cross-os-attestation",
        "ao2.cross-os-release-attestation.v1",
        "pulse-eval-loop.json",
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
        "target/pulse-generate-next/latest/summary.json",
    ]:
        assert needle in verification


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
        "stores_credentials",
    ]:
        assert needle in text
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
    ]:
        assert needle in verification


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
    expected_scripts = {
        "public:hardening-ci-workflow": "node scripts/run-sh-script.js scripts/public-hardening-ci-workflow.sh",
        "public:hardening-workflow-file-dry-run": "node scripts/run-sh-script.js scripts/public-hardening-workflow-file-dry-run.sh",
        "public:hardening-workflow-tracked-proposal": "node scripts/run-sh-script.js scripts/public-hardening-workflow-tracked-proposal.sh",
        "public:hardening-ci-local-runner-parity": "node scripts/run-sh-script.js scripts/public-hardening-ci-local-runner-parity.sh",
    }
    for script_name, command in expected_scripts.items():
        assert package_json["scripts"][script_name] == command

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
        "AO2_PULSE_LOCAL_MIRROR_SOURCE=target/pulse-next-recommended-tasks/generated-next npm run pulse:local-mirror",
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
