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
        "npm run release:readiness",
        "ao2.risky-pr-golden-path.v1",
        "ao2.release-readiness-local.v1",
    ]:
        assert needle in verification
