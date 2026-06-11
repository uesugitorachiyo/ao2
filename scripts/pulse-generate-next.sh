#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_PULSE_GENERATE_NEXT_ROOT:-$ROOT/target/pulse-generate-next/latest}"
PACKET_ROOT="${AO2_PULSE_GENERATE_NEXT_PACKET_ROOT:-$ROOT/target/pulse-next-recommended-tasks}"
TASK_BOARD_ROOT="${AO2_PULSE_TASK_BOARD_ROOT:-$ROOT/target/pulse-task-board/latest}"
TASK_BOARD_HISTORY_ROOT="${AO2_PULSE_TASK_BOARD_HISTORY_ROOT:-$ROOT/.ao2-local/pulse/task-board-history}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
CURSOR_FILE="${AO2_PULSE_GENERATE_NEXT_CURSOR:-$ROOT/.ao2-local/pulse/pulse-generate-next-cursor.json}"
REGISTER="${AO2_PULSE_GENERATE_NEXT_REGISTER:-1}"
LOCAL_ONLY="${AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY:-0}"
STATUS_EVIDENCE="${AO2_PULSE_TASK_BOARD_STATUS_EVIDENCE:-}"
DEFAULT_AUTO_ADVANCE_PROMPT="After each task batch, re-evaluate AO2 and ao2-control-plane at project level. Choose next tasks by highest long-term value, not similarity to last tasks. Prefer the Risky PR Run MVP product loop, local run record, static report/export, evaluator closure evidence, public reliability, Ubuntu/macOS/Windows correctness, CI confidence, evidence quality, security/safety boundaries, control-plane integration, release readiness, and developer/operator usability. Do not create new shell wrappers unless they directly unlock a product-slice or release-readiness bottleneck. Avoid narrow recursion or low-value daemon work unless it is the bottleneck. Generate next lengthy tasks with rationale, required evidence, and stop conditions, then register and continue through the AO2 event loop."
AUTO_ADVANCE_PROMPT="${AO2_PULSE_AUTO_ADVANCE_PROMPT:-$DEFAULT_AUTO_ADVANCE_PROMPT}"

rm -rf "$OUT_ROOT" "$PACKET_ROOT" "$TASK_BOARD_ROOT"
mkdir -p "$OUT_ROOT" "$LOG_DIR" "$PACKET_ROOT" "$TASK_BOARD_ROOT" "$TASK_BOARD_HISTORY_ROOT" "$(dirname "$CURSOR_FILE")"

python3 - "$ROOT" "$OUT_ROOT" "$PACKET_ROOT" "$TASK_BOARD_ROOT" "$TASK_BOARD_HISTORY_ROOT" "$SUMMARY" "$CURSOR_FILE" "$STATUS_EVIDENCE" "$LOCAL_ONLY" <<'PY'
import html
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
packet_root = Path(sys.argv[3]).resolve()
task_board_root = Path(sys.argv[4]).resolve()
task_board_history_root = Path(sys.argv[5]).resolve()
summary_path = Path(sys.argv[6]).resolve()
cursor_file = Path(sys.argv[7]).resolve()
status_evidence_arg = sys.argv[8]
status_evidence_path = Path(status_evidence_arg).resolve() if status_evidence_arg else None
local_only_while_pr_blocked = sys.argv[9] == "1"
generation_mode = "local_only_while_pr_blocked" if local_only_while_pr_blocked else "normal"

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

dimensions = [
    "product_mvp_slice",
    "public_reliability",
    "cross_platform_correctness",
    "ci_confidence",
    "evidence_quality",
    "security_safety_boundaries",
    "control_plane_integration",
    "release_readiness",
    "developer_operator_usability",
    "novelty",
]

project_docs = [
    "docs/PRD.md",
    "docs/SDD-risky-pr-run.md",
    "docs/SCHEMAS-AND-INTERFACES.md",
    "docs/IMPLEMENTATION-SLICES.md",
]

doc_text = {}
for rel in project_docs:
    path = root / rel
    doc_text[rel] = path.read_text(encoding="utf-8", errors="replace") if path.is_file() else ""

ledger_path = root / ".ao2-local" / "pulse" / "pulse-auto-advance-ledger.jsonl"
ledger_history = []
if ledger_path.is_file():
    for line in ledger_path.read_text(encoding="utf-8", errors="replace").splitlines()[-25:]:
        try:
            ledger_history.append(json.loads(line))
        except json.JSONDecodeError:
            ledger_history.append({"raw": line[:160]})

previous_summary_path = root / "target" / "pulse-generate-next" / "latest" / "summary.json"
previous_selection = None
if previous_summary_path.is_file():
    try:
        previous_selection = json.loads(previous_summary_path.read_text(encoding="utf-8")).get("selection")
    except json.JSONDecodeError:
        previous_selection = None

catalog = [
    {
        "id": "risky-pr-product-mvp",
        "title": "Risky PR Run MVP product loop",
        "keywords": ["risky-pr", "risky pr", "static report", "evaluator closure", "local run record", "mvp", "product loop"],
        "scores": {
            "product_mvp_slice": 8,
            "public_reliability": 5,
            "cross_platform_correctness": 4,
            "ci_confidence": 5,
            "evidence_quality": 6,
            "security_safety_boundaries": 5,
            "control_plane_integration": 3,
            "release_readiness": 6,
            "developer_operator_usability": 5,
        },
        "tasks": [
            ("ao2-risky-pr-product-readiness-gate", "Risky PR product-readiness gate", "npm run risky-pr:product-readiness", "ao2.risky-pr-product-readiness-gate.v1", "Prove local run record, static report/export, and evaluator closure evidence from one risky-pr golden run."),
            ("ao2-risky-pr-evaluator-closure-evidence", "Risky PR evaluator closure evidence hardening", "npm run release:evidence-closure", "ao2.release-evidence-closure.v1", "Keep evaluator closure aligned with the evidence-before-closure rule."),
            ("ao2-risky-pr-ci-matrix-proof", "Risky PR CI matrix proof", "npm run release:readiness:static", "ao2.release-readiness-local.v1", "Use public CI-oriented readiness checks for product readiness instead of local-only script churn."),
        ],
        "rationale": "This is the highest-value product_mvp_slice: it advances AO2 toward a shippable local-first Risky PR Run workflow instead of only improving Pulse wrappers.",
        "required_evidence": ["ao2.risky-pr-product-readiness-gate.v1", "ao2.risky-pr-golden-path.v1", "ao2.evidence-pack.v1", "ao2.release-evidence-closure.v1"],
        "stop_conditions": ["Stop if risky-pr golden evidence is missing.", "Stop if generated work is only new shell wrappers for two consecutive iterations.", "Stop if evaluator closure can pass without evidence."],
    },
    {
        "id": "ai-task-board-control-surface",
        "title": "AI task board control surface",
        "keywords": ["ai task board", "control surface", "pulse drift", "v0.4.81", "recommended tasks", "task board"],
        "scores": {
            "product_mvp_slice": 7,
            "public_reliability": 5,
            "cross_platform_correctness": 3,
            "ci_confidence": 4,
            "evidence_quality": 7,
            "security_safety_boundaries": 5,
            "control_plane_integration": 6,
            "release_readiness": 5,
            "developer_operator_usability": 7,
        },
        "tasks": [
            ("ao2-ai-task-board-schema", "AI task board schema", "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q", "ao2.ai-task-board.v1", "Define the operator-visible task board contract with status, rationale, evidence requirements, and stop conditions."),
            ("ao2-ai-task-board-pulse-export", "Pulse task-board export", "npm run pulse:generate-next:contract", "ao2.ai-task-board.v1", "Emit a task-board packet from Pulse without breaking the existing recommended-task packet contract."),
            ("ao2-ai-task-board-control-plane-readback", "AI task board control-plane readback", "npm run control-plane:fixture-consumer-smoke", "ao2.control-plane-fixture-consumer-smoke.v1", "Prove the control plane can consume the board as a read-only observer without credentials or mutation authority."),
            ("ao2-ai-task-board-drift-gate", "Pulse drift gate", "npm run pulse:next-task-quality-filter", "ao2.pulse-next-task-quality-filter.v1", "Reject generated task packets that omit release objective, evidence requirements, or stop conditions."),
        ],
        "rationale": "This starts the v0.4.81 train by giving operators an explicit control surface for next work, so Pulse loops stop drifting after batch completion.",
        "required_evidence": ["ao2.ai-task-board.v1", "ao2.pulse-generate-next-contract.v1", "ao2.control-plane-fixture-consumer-smoke.v1"],
        "stop_conditions": ["Stop if the task board can mutate AO2 release metadata.", "Stop if generated tasks lack evidence requirements or stop conditions.", "Stop if control-plane readback requires credentials or private local paths."],
    },
    {
        "id": "generator-health",
        "title": "Pulse generator and daemon health",
        "keywords": ["pulse", "daemon", "eval-loop", "evidence"],
        "scores": {
            "public_reliability": 2,
            "cross_platform_correctness": 2,
            "ci_confidence": 3,
            "evidence_quality": 4,
            "security_safety_boundaries": 2,
            "control_plane_integration": 1,
            "release_readiness": 2,
            "developer_operator_usability": 3,
        },
        "tasks": [
            ("ao2-pulse-generate-next-contract", "Pulse next-packet generator contract", "npm run pulse:generate-next:contract", "ao2.pulse-generate-next-contract.v1", "Keep the strategic next-packet planner contract covered."),
            ("ao2-pulse-daemon-contract", "Pulse daemon contract", "npm run pulse:daemon:contract", "ao2.pulse-daemon-contract.v1", "Keep launchctl/tmux supervisor coverage current."),
            ("ao2-pulse-daemon-status", "Pulse daemon status", "npm run pulse:daemon:status", "ao2.pulse-daemon.v1", "Read back active backend, process liveness, and heartbeat state."),
            ("ao2-pulse-auto-advance-runner-contract", "Pulse auto-advance runner contract", "npm run pulse:auto-advance-runner-contract", "ao2.pulse-auto-advance-runner-contract.v1", "Keep runner heartbeat, stop, and dedup behavior visible."),
            ("ao2-pulse-resume-workspace-cli-fallback", "Pulse workspace CLI fallback", "npm run pulse:resume-workspace-cli-fallback", "ao2.pulse-resume-workspace-cli-fallback.v1", "Keep stale global ao2 detection and workspace CLI fallback current."),
        ],
        "rationale": "Use only when local event-loop evidence shows the planner or supervisor is the current bottleneck.",
        "required_evidence": ["ao2.pulse-generate-next-contract.v1", "ao2.pulse-daemon-contract.v1", "ao2.pulse-auto-advance-runner-contract.v1"],
        "stop_conditions": ["Stop if daemon status is unobservable after contract checks.", "Stop if runner dedup or STOP-file behavior fails."],
    },
    {
        "id": "public-stabilization",
        "title": "Public stabilization smoke",
        "keywords": ["public", "ci", "verification", "release", "evidence"],
        "scores": {
            "public_reliability": 5,
            "cross_platform_correctness": 3,
            "ci_confidence": 5,
            "evidence_quality": 4,
            "security_safety_boundaries": 3,
            "control_plane_integration": 2,
            "release_readiness": 4,
            "developer_operator_usability": 3,
        },
        "tasks": [
            ("ao2-public-stabilization-tests", "Public stabilization tests", "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q", "pytest.tests.test_public_stabilization", "Run the local public stabilization suite after daemon changes."),
            ("ao2-pulse-terminal-schema-compatibility", "Pulse terminal schema compatibility", "npm run pulse:terminal-eval-loop-schema-compatibility", "ao2.pulse-terminal-eval-loop-schema-compatibility.v1", "Keep script-backed packet compatibility with terminal eval-loop evidence fresh."),
            ("ao2-pulse-auto-advance-integration-gate", "Pulse auto-advance integration gate", "npm run pulse:auto-advance-integration-gate", "ao2.pulse-auto-advance-integration-gate.v1", "Compose the auto-advance runner support checks."),
            ("ao2-pulse-resume-dry-run", "Pulse resume dry-run", "npm run pulse:resume -- --dry-run", "ao2.pulse-resume.v1", "Verify the mirrored resume packet and prompt hash without executing a CLI chain."),
            ("ao2-pulse-daemon-status-readback", "Pulse daemon status readback", "npm run pulse:daemon:status", "ao2.pulse-daemon.v1", "Confirm the supervisor remains observable from local evidence."),
        ],
        "rationale": "Public reliability and CI confidence remain high-value because AO2 is now public and needs repeatable local proof before repository-facing changes.",
        "required_evidence": ["pytest.tests.test_public_stabilization", "ao2.pulse-terminal-eval-loop-schema-compatibility.v1", "ao2.pulse-auto-advance-integration-gate.v1"],
        "stop_conditions": ["Stop if public stabilization tests fail.", "Stop if resume dry-run detects prompt or eval-loop digest mismatch."],
    },
    {
        "id": "cross-platform-compatibility",
        "title": "Ubuntu macOS Windows compatibility evidence",
        "keywords": ["ubuntu", "linux", "macos", "windows", "cross-os", "release"],
        "scores": {
            "public_reliability": 4,
            "cross_platform_correctness": 6,
            "ci_confidence": 4,
            "evidence_quality": 4,
            "security_safety_boundaries": 2,
            "control_plane_integration": 2,
            "release_readiness": 5,
            "developer_operator_usability": 3,
        },
        "tasks": [
            ("ao2-cross-os-release-attestation", "Cross-OS release attestation", "npm run release:cross-os-attestation", "ao2.cross-os-release-attestation.v1", "Record macOS, Ubuntu/Linux, and Windows platform compatibility attestations without publish side effects."),
            ("ao2-public-stabilization-tests-cross-platform-contracts", "Public stabilization cross-platform contracts", "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q", "pytest.tests.test_public_stabilization", "Assert workflow, package, and platform contract coverage remains stable."),
            ("ao2-pulse-terminal-schema-cross-platform", "Pulse terminal schema cross-platform compatibility", "npm run pulse:terminal-eval-loop-schema-compatibility", "ao2.pulse-terminal-eval-loop-schema-compatibility.v1", "Keep generated packets compatible with the Rust CLI contract across target platforms."),
            ("ao2-pulse-resume-workspace-cli-cross-platform", "Pulse workspace CLI fallback cross-platform", "npm run pulse:resume-workspace-cli-fallback", "ao2.pulse-resume-workspace-cli-fallback.v1", "Keep stale binary detection visible so platform runners use the workspace CLI when needed."),
            ("ao2-pulse-daemon-status-cross-platform", "Pulse daemon status cross-platform readback", "npm run pulse:daemon:status", "ao2.pulse-daemon.v1", "Confirm the local supervisor remains observable while platform evidence runs."),
        ],
        "rationale": "Cross-platform correctness is a project-level release constraint, especially for Ubuntu, macOS, and Windows public usage.",
        "required_evidence": ["ao2.cross-os-release-attestation.v1", "pytest.tests.test_public_stabilization", "ao2.pulse-resume-workspace-cli-fallback.v1"],
        "stop_conditions": ["Stop if any platform attestation fails.", "Stop if fallback CLI detection is not observable."],
    },
    {
        "id": "control-plane-integration",
        "title": "AO2 control-plane integration confidence",
        "keywords": ["control-plane", "evidence", "dashboard", "observer", "receipt"],
        "scores": {
            "public_reliability": 4,
            "cross_platform_correctness": 2,
            "ci_confidence": 3,
            "evidence_quality": 5,
            "security_safety_boundaries": 5,
            "control_plane_integration": 6,
            "release_readiness": 4,
            "developer_operator_usability": 4,
        },
        "tasks": [
            ("ao2-control-plane-local-bootstrap", "Control-plane local bootstrap", "npm run control-plane:local-bootstrap", "ao2.control-plane-local-bootstrap.v1", "Verify token-safe local control-plane startup/readback without recording secrets."),
            ("ao2-control-plane-cross-repo-observer", "Cross-repo control-plane observer", "npm run control-plane:cross-repo-observer", "ao2.control-plane-cross-repo-observer.v1", "Keep AO2/control-plane read-only observer integration visible."),
            ("ao2-operator-index-control-plane-readback", "Operator index control-plane readback drill", "npm run evidence:operator-index-control-plane-readback-drill", "ao2.operator-index-control-plane-readback-drill.v1", "Prove operator evidence can be read back through fixture receipts."),
            ("ao2-control-plane-fixture-consumer-smoke", "Control-plane fixture consumer smoke", "npm run control-plane:fixture-consumer-smoke", "ao2.control-plane-fixture-consumer-smoke.v1", "Keep fixture catalog consumption fail-closed."),
            ("ao2-pulse-resume-dry-run-after-control-plane", "Pulse resume dry-run after control-plane evidence", "npm run pulse:resume -- --dry-run", "ao2.pulse-resume.v1", "Verify event-loop continuity after control-plane integration checks."),
        ],
        "rationale": "Control-plane integration is a core project outcome, but it must stay token-safe and read-only unless policy explicitly approves mutation.",
        "required_evidence": ["ao2.control-plane-local-bootstrap.v1", "ao2.control-plane-cross-repo-observer.v1", "ao2.operator-index-control-plane-readback-drill.v1"],
        "stop_conditions": ["Stop if a token leak scan fails.", "Stop if observer/readback evidence is missing or mutable."],
    },
    {
        "id": "release-readiness",
        "title": "Public release readiness and rollback proof",
        "keywords": ["release", "readiness", "rollback", "artifact", "ship"],
        "scores": {
            "public_reliability": 5,
            "cross_platform_correctness": 4,
            "ci_confidence": 4,
            "evidence_quality": 4,
            "security_safety_boundaries": 4,
            "control_plane_integration": 3,
            "release_readiness": 6,
            "developer_operator_usability": 3,
        },
        "tasks": [
            ("ao2-release-readiness-regression-gate", "Release readiness regression gate", "npm run release:readiness:regression-gate", "ao2.release-readiness-regression-gate.v1", "Aggregate local release-readiness proof without publish side effects."),
            ("ao2-release-asset-publication-readiness", "Release asset publication readiness", "npm run release:asset-publication-readiness", "ao2.release-asset-publication-readiness.v1", "Check release asset naming/checksum readiness without publishing."),
            ("ao2-release-public-ship-dry-run", "Public ship dry-run", "npm run release:public-ship-dry-run", "ao2.public-ship-dry-run.v1", "Exercise rollback manifest and publish guards without external release writes."),
            ("ao2-release-cutover-readiness-lock", "Release cutover readiness lock", "npm run release:cutover-readiness-lock", "ao2.release-cutover-readiness-lock.v1", "Record no-publish cutover lock evidence."),
            ("ao2-public-stabilization-tests-release-readiness", "Public stabilization tests for release readiness", "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q", "pytest.tests.test_public_stabilization", "Keep public contracts green after release-readiness checks."),
        ],
        "rationale": "Release readiness is high leverage for public AO2 because it converts accumulated local evidence into a repeatable ship/no-ship decision.",
        "required_evidence": ["ao2.release-readiness-regression-gate.v1", "ao2.release-asset-publication-readiness.v1", "ao2.public-ship-dry-run.v1"],
        "stop_conditions": ["Stop if any command attempts tag push, publish, deploy, or release creation.", "Stop if rollback evidence is absent."],
    },
    {
        "id": "developer-operator-usability",
        "title": "Developer and operator usability evidence",
        "keywords": ["operator", "workbench", "dashboard", "accessibility", "browser"],
        "scores": {
            "public_reliability": 3,
            "cross_platform_correctness": 2,
            "ci_confidence": 3,
            "evidence_quality": 5,
            "security_safety_boundaries": 3,
            "control_plane_integration": 4,
            "release_readiness": 3,
            "developer_operator_usability": 6,
        },
        "tasks": [
            ("ao2-workbench-operator-cockpit-uat", "Operator cockpit UAT", "npm run workbench:operator-cockpit-uat", "ao2.operator-cockpit-uat.v1", "Check the operator cockpit surfaces expected decisions without filesystem archaeology."),
            ("ao2-evidence-dashboard-accessibility-audit", "Evidence dashboard accessibility audit", "npm run evidence:dashboard-accessibility-audit", "ao2.evidence-dashboard-accessibility-audit.v1", "Audit evidence dashboard accessibility, links, and overlap constraints."),
            ("ao2-evidence-dashboard-browser-qa", "Evidence dashboard browser QA", "npm run evidence:dashboard-browser-qa", "ao2.evidence-dashboard-browser-qa.v1", "Run browser-backed dashboard traversal evidence."),
            ("ao2-evidence-dashboard-visual-baseline-lock", "Evidence dashboard visual baseline lock", "npm run evidence:dashboard-visual-baseline-lock", "ao2.evidence-dashboard-visual-baseline-lock.v1", "Lock visual baseline evidence for operator-facing review surfaces."),
            ("ao2-pulse-resume-dry-run-after-usability", "Pulse resume dry-run after usability evidence", "npm run pulse:resume -- --dry-run", "ao2.pulse-resume.v1", "Verify event-loop continuity after operator usability checks."),
        ],
        "rationale": "Operator usability reduces the chance that public users need local archaeology to understand AO2 state and evidence.",
        "required_evidence": ["ao2.operator-cockpit-uat.v1", "ao2.evidence-dashboard-accessibility-audit.v1", "ao2.evidence-dashboard-browser-qa.v1"],
        "stop_conditions": ["Stop if browser evidence cannot be produced.", "Stop if dashboard links or viewport checks fail."],
    },
    {
        "id": "security-safety-boundaries",
        "title": "Security and policy boundary evidence",
        "keywords": ["security", "policy", "provider", "safety", "fail-closed"],
        "scores": {
            "public_reliability": 4,
            "cross_platform_correctness": 2,
            "ci_confidence": 4,
            "evidence_quality": 4,
            "security_safety_boundaries": 6,
            "control_plane_integration": 3,
            "release_readiness": 4,
            "developer_operator_usability": 3,
        },
        "tasks": [
            ("ao2-pulse-execute-safety-corpus", "Pulse execute safety corpus", "npm run pulse:execute-safety-corpus", "ao2.pulse-execute-safety-corpus.v1", "Keep execute-mode refusal/simulation boundaries covered."),
            ("ao2-pulse-real-execute-containment", "Pulse real execute containment", "npm run pulse:real-execute-containment", "ao2.pulse-real-execute-containment.v1", "Verify bounded real execute fixtures cannot escape containment."),
            ("ao2-provider-contract-hardening", "Provider contract hardening", "npm run provider:phase2-contract-hardening", "ao2.provider-phase2-contract-hardening.v1", "Harden provider contracts without adding provider API-key auth paths."),
            ("ao2-provider-pilot-safety-regression", "Provider pilot safety regression matrix", "npm run provider:pilot-safety-regression-matrix", "ao2.provider-pilot-safety-regression-matrix.v1", "Keep provider pilot safety failures fail-closed."),
            ("ao2-pulse-quality-filter-required-gate", "Pulse quality filter required gate", "npm run pulse:quality-filter-required-gate", "ao2.pulse-quality-filter-required-gate.v1", "Ensure low-value recursion is blocked before registration."),
        ],
        "rationale": "Security and policy boundaries protect public operation and keep automation from turning into unauthorized side effects.",
        "required_evidence": ["ao2.pulse-execute-safety-corpus.v1", "ao2.pulse-real-execute-containment.v1", "ao2.provider-phase2-contract-hardening.v1"],
        "stop_conditions": ["Stop if any side-effecting tool action bypasses policy.", "Stop if provider-key auth paths are introduced."],
    },
]

def score_candidate(candidate: dict, recent_selection_ids: list[str], tie_break: int) -> dict:
    scores = dict(candidate.get("scores", {}))
    base = sum(int(scores.get(name, 0)) for name in dimensions if name != "novelty")
    doc_hits = 0
    for text in doc_text.values():
        lowered = text.lower()
        doc_hits += sum(1 for keyword in candidate.get("keywords", []) if keyword.lower() in lowered)
    doc_bonus = min(doc_hits, 8)
    repeat_count = recent_selection_ids.count(candidate["id"])
    anti_recursion = {
        "recent_selection_count": repeat_count,
        "penalty": repeat_count * 12,
        "policy": "avoid narrow recursion",
    }
    novelty = max(0, 6 - repeat_count * 2)
    scores["novelty"] = novelty
    generator_health_gate = 0
    if candidate["id"] == "generator-health":
        generator_health_gate = -10
        daemon_summary_path = root / "target" / "pulse-daemon" / "latest" / "summary.json"
        auto_summary_path = root / "target" / "pulse-auto-advance" / "latest" / "summary.json"
        for path in [daemon_summary_path, auto_summary_path]:
            if not path.is_file():
                generator_health_gate += 5
                continue
            try:
                status = json.loads(path.read_text(encoding="utf-8")).get("status")
            except json.JSONDecodeError:
                status = "unreadable"
            if status not in {"running", "passed", "waiting_for_new_eval_loop_digest", "stopped"}:
                generator_health_gate += 5
    strategic_score = base + doc_bonus + novelty + generator_health_gate - anti_recursion["penalty"]
    return {
        "id": candidate["id"],
        "title": candidate["title"],
        "strategic_score": strategic_score,
        "scores": scores,
        "doc_bonus": doc_bonus,
        "anti_recursion": anti_recursion,
        "generator_health_gate": generator_health_gate,
        "tie_break": tie_break,
        "rationale": candidate["rationale"],
        "required_evidence": candidate["required_evidence"],
        "stop_conditions": candidate["stop_conditions"],
    }

cursor = {"index": 0, "generation": 0, "history": []}
if cursor_file.is_file():
    try:
        cursor.update(json.loads(cursor_file.read_text(encoding="utf-8")))
    except json.JSONDecodeError:
        pass

generation = int(cursor.get("generation", 0)) + 1
cursor_index = int(cursor.get("index", 0)) % len(catalog)
recent_selection_ids = [item for item in cursor.get("history", []) if isinstance(item, str)][-6:]
if previous_selection:
    recent_selection_ids.append(str(previous_selection))

project_level_reassessment = {
    "source_docs": project_docs,
    "source_doc_sha256": {
        rel: hashlib.sha256(text.encode("utf-8")).hexdigest()
        for rel, text in doc_text.items()
    },
    "ledger_history": {
        "path": str(ledger_path),
        "entry_count_sampled": len(ledger_history),
        "recent_statuses": [str(item.get("status", "unknown")) for item in ledger_history[-5:] if isinstance(item, dict)],
    },
    "selection_policy": (
        "Score candidates by project-level value, evidence gaps, release readiness, "
        "product_mvp_slice coverage, and anti-recursion; avoid narrow recursion unless daemon evidence identifies it as the bottleneck. "
        "Do not create new shell wrappers unless they directly unlock a product-slice or release-readiness bottleneck."
    ),
    "script_wrapper_recursion_block": {
        "enabled": True,
        "policy": "Do not create new shell wrappers unless they directly unlock product_mvp_slice or release-readiness evidence.",
    },
}

strategic_scores = [
    score_candidate(candidate, recent_selection_ids, (offset - cursor_index) % len(catalog))
    for offset, candidate in enumerate(catalog)
]
strategic_scores.sort(key=lambda item: (-item["strategic_score"], item["tie_break"], item["id"]))
selected_score = strategic_scores[0]
selection = next(candidate for candidate in catalog if candidate["id"] == selected_score["id"])

history = (recent_selection_ids + [selection["id"]])[-12:]
next_cursor = {"index": (cursor_index + 1) % len(catalog), "generation": generation, "history": history}
cursor_file.write_text(json.dumps(next_cursor, indent=2, sort_keys=True) + "\n", encoding="utf-8")

tasks = []
if selection["id"] == "risky-pr-product-mvp" and not local_only_while_pr_blocked:
    tasks.append({
        "id": f"ao2-risky-pr-report-evaluator-closure-ux-g{generation}",
        "kind": "product_code",
        "title": "Risky PR report/evaluator closure UX implementation",
        "objective": (
            "Make the Risky PR Run MVP easier to inspect by surfacing the local run record, "
            "static report/export links, and evaluator closure evidence as a product-facing implementation slice."
        ),
        "files": [
            "crates/ao2-cli/src/main.rs",
            "crates/ao2-cli/tests/cli_approval_replay.rs",
            "docs/VERIFICATION.md",
        ],
        "acceptance": [
            "Risky PR report output exposes local run record and static report/export evidence without filesystem archaeology.",
            "Evaluator closure evidence remains required before closure can pass.",
            "The implementation stays local-first and adds no provider API-key auth path.",
        ],
        "verification": [
            {
                "command": "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q",
                "expected_evidence": "pytest.tests.test_public_stabilization",
            },
            {
                "command": "npm run risky-pr:product-readiness",
                "expected_evidence": "ao2.risky-pr-product-readiness-gate.v1",
            },
        ],
        "stop_conditions": [
            "Stop if product evidence cannot be produced from one local risky-pr golden run.",
            "Stop if the task would require provider API keys or credential storage.",
            "Stop if evaluator closure can pass without evidence.",
        ],
        "why": "Create an actual product-code implementation packet before running supporting evidence gates.",
        "rationale": selection["rationale"],
        "required_evidence": selection["required_evidence"],
    })
if selection["id"] == "ai-task-board-control-surface" and not local_only_while_pr_blocked:
    tasks.append({
        "id": f"ao2-ai-task-board-control-surface-implementation-g{generation}",
        "kind": "product_code",
        "title": "AI task board control-surface implementation",
        "objective": (
            "Start the v0.4.81 control-surface slice by defining the task-board contract "
            "and wiring Pulse generated packets to expose release objective, rationale, "
            "evidence requirements, and stop conditions as operator-visible product state."
        ),
        "files": [
            "scripts/pulse-generate-next.sh",
            "tests/test_public_stabilization.py",
            "docs/release/v0.4.81-ai-task-board-control-surface.md",
            "docs/VERIFICATION.md",
        ],
        "acceptance": [
            "Pulse emits an operator-readable task-board packet using ao2.ai-task-board.v1.",
            "Task-board entries preserve evidence requirements and stop conditions.",
            "The control-plane remains a read-only observer with no release mutation authority.",
        ],
        "verification": [
            {
                "command": "PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q",
                "expected_evidence": "pytest.tests.test_public_stabilization",
            },
            {
                "command": "npm run pulse:generate-next:contract",
                "expected_evidence": "ao2.pulse-generate-next-contract.v1",
            },
            {
                "command": "npm run control-plane:fixture-consumer-smoke",
                "expected_evidence": "ao2.control-plane-fixture-consumer-smoke.v1",
            },
        ],
        "stop_conditions": [
            "Stop if the task board can mutate AO2 release metadata.",
            "Stop if generated tasks lack evidence requirements or stop conditions.",
            "Stop if control-plane readback requires credentials or private local paths.",
        ],
        "why": "Create the actual product-code control surface before running supporting evidence gates.",
        "rationale": selection["rationale"],
        "required_evidence": selection["required_evidence"],
    })
for task_id, title, command, expected_evidence, why in selection["tasks"]:
    tasks.append({
        "id": f"{task_id}-g{generation}",
        "kind": "evidence_gate",
        "title": title,
        "command": command,
        "expected_evidence": expected_evidence,
        "why": why,
        "rationale": selection["rationale"],
        "required_evidence": selection["required_evidence"],
        "stop_conditions": selection["stop_conditions"],
    })

if local_only_while_pr_blocked:
    tasks = [task for task in tasks if task.get("kind") == "evidence_gate"]

packet_md = (
    f"# AO2 Pulse Generated Packet {generation}\n\n"
    f"Generation mode: `{generation_mode}`\n\n"
    f"Selection: `{selection['id']}` - {selection['title']}\n\n"
    "Generated by `npm run pulse:generate-next` from local daemon/ledger evidence.\n\n"
    "Project-level reassessment:\n"
    + "".join(f"\n- `{rel}`\n" for rel in project_docs)
    + "\n"
    f"Strategic score: `{selected_score['strategic_score']}`\n\n"
    f"Rationale: {selection['rationale']}\n\n"
    "Required evidence:\n"
    + "".join(f"\n- `{item}`\n" for item in selection["required_evidence"])
    + "\nStop conditions:\n"
    + "".join(f"\n- {item}\n" for item in selection["stop_conditions"])
    + "\n"
    "Next tasks:\n"
    + "".join(f"\n- `{task['id']}`: {task['title']} - {task['why']}\n" for task in tasks)
)
board_md = (
    f"# AO2 Pulse Generated Board {generation}\n\n"
    f"Generation mode: `{generation_mode}`\n\n"
    f"Selection: `{selection['id']}`\n\n"
    f"Strategic score: `{selected_score['strategic_score']}`\n\n"
    + "".join(f"- [ ] {task['title']} (`{task['id']}`)\n" for task in tasks)
)
executor = {
    "schema_version": "ao2.pulse-generate-next-executor-evidence.v1",
    "generated_at_utc": utc_now(),
    "status": "passed",
    "cursor": next_cursor,
    "selection": selection["id"],
    "generation_mode": generation_mode,
    "local_only_while_pr_blocked": local_only_while_pr_blocked,
    "project_level_reassessment": project_level_reassessment,
    "strategic_score": selected_score,
    "strategic_scores": strategic_scores,
    "rationale": selection["rationale"],
    "required_evidence": selection["required_evidence"],
    "stop_conditions": selection["stop_conditions"],
    "source_summaries": {
        "daemon": str(root / "target" / "pulse-daemon" / "latest" / "summary.json"),
        "auto_advance": str(root / "target" / "pulse-auto-advance" / "latest" / "summary.json"),
        "registration": str(root / "target" / "pulse-auto-advance-registration" / "latest" / "summary.json"),
    },
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
eval_loop = {
    "schema_version": "ao2.pulse-eval-loop.v1",
    "status": "ready",
    "mode": "local_script_backed_recommendation",
    "generation_mode": generation_mode,
    "local_only_while_pr_blocked": local_only_while_pr_blocked,
    "generated_at_utc": utc_now(),
    "cursor": next_cursor,
    "selection": selection["id"],
    "project_level_reassessment": project_level_reassessment,
    "strategic_score": selected_score,
    "strategic_scores": strategic_scores,
    "rationale": selection["rationale"],
    "required_evidence": selection["required_evidence"],
    "stop_conditions": selection["stop_conditions"],
    "recommended_tasks": tasks,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
    "side_effects": {"repo_apply": False, "provider_execution": False},
}
task_manifest = {
    "schema_version": "ao2.pulse-task-manifest.v1",
    "generated_at_utc": utc_now(),
    "status": "ready",
    "selection": selection["id"],
    "generation_mode": generation_mode,
    "local_only_while_pr_blocked": local_only_while_pr_blocked,
    "cursor": next_cursor,
    "product_code_execution": {
        "enabled": not local_only_while_pr_blocked,
        "mode": "disabled" if local_only_while_pr_blocked else "dry_run",
    },
    "tasks": tasks,
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "local_process_execution_and_packet_materialization",
    },
}
release_train = {
    "version": "v0.4.81",
    "theme": "AI task board control surface",
}
release_objective = (
    "Expose Pulse's next recommended work as an operator-readable AI task board "
    "with rationale, evidence requirements, stop conditions, and read-only "
    "control-plane semantics."
)
control_plane_role = "read_only_observer"
source_recommendation = {
    "selection": selection["id"],
    "title": selection["title"],
    "generation": generation,
    "generation_mode": generation_mode,
    "strategic_score": selected_score["strategic_score"],
}

allowed_task_statuses = {
    "proposed",
    "ready",
    "in_progress",
    "blocked",
    "passed",
    "skipped",
}
task_status_updates = {}
status_transition_source = {
    "schema_version": "ao2.ai-task-board-status-evidence.v1",
    "status": "skipped",
    "path": str(status_evidence_path) if status_evidence_path else None,
    "updates_applied": 0,
    "allowed_statuses": sorted(allowed_task_statuses),
}
if status_evidence_path and status_evidence_path.is_file():
    try:
        status_evidence = json.loads(status_evidence_path.read_text(encoding="utf-8"))
        task_status_updates = {
            str(task_id): value
            for task_id, value in status_evidence.get("task_statuses", {}).items()
            if isinstance(value, dict)
        }
        status_transition_source.update({
            "status": "loaded",
            "schema_version": status_evidence.get(
                "schema_version",
                "ao2.ai-task-board-status-evidence.v1",
            ),
        })
    except json.JSONDecodeError as exc:
        status_transition_source.update({
            "status": "invalid_json",
            "error": str(exc),
        })

task_board_tasks = []
for task in tasks:
    item = {
        "task_id": task["id"],
        "title": task["title"],
        "kind": task["kind"],
        "status": "proposed",
        "objective": task.get("objective") or task.get("why") or selection["rationale"],
        "confidence": "medium",
        "rationale": task.get("rationale") or selection["rationale"],
        "required_evidence": task.get("required_evidence") or selection["required_evidence"],
        "stop_conditions": task.get("stop_conditions") or selection["stop_conditions"],
        "source_recommendation": source_recommendation,
        "release_train": release_train,
    }
    status_update = task_status_updates.get(item["task_id"])
    if status_update:
        requested_status = str(status_update.get("status", "")).strip().lower().replace("-", "_")
        if requested_status in allowed_task_statuses:
            item["status"] = requested_status
            item["status_transition"] = {
                "source": str(status_evidence_path),
                "previous_status": "proposed",
                "current_status": requested_status,
                "reason": str(status_update.get("status_reason") or status_update.get("reason") or ""),
                "evidence": [
                    str(value)
                    for value in status_update.get("evidence", [])
                    if value is not None
                ],
            }
            status_transition_source["updates_applied"] += 1
    task_board_tasks.append(item)
if status_transition_source["updates_applied"]:
    status_transition_source["status"] = "applied"

task_board = {
    "schema_version": "ao2.ai-task-board.v1",
    "generated_at_utc": utc_now(),
    "status": "ready",
    "artifact_root": str(task_board_root),
    "release_train": release_train,
    "release_objective": release_objective,
    "source_recommendation": source_recommendation,
    "status_transition_source": status_transition_source,
    "tasks": task_board_tasks,
    "control_plane_readback": {
        "role": control_plane_role,
        "requires_credentials": False,
        "can_mutate_ao2_artifacts": False,
        "can_mutate_release_metadata": False,
    },
    "trust_boundary": {
        "local_only": True,
        "stores_credentials": False,
        "side_effects": "local_artifact_materialization_only",
    },
}
previous_path = task_board_history_root / "latest.json"
previous_board = None
if previous_path.is_file():
    try:
        previous_board = json.loads(previous_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        previous_board = None

current_task_ids = [item["task_id"] for item in task_board_tasks]
previous_task_ids = [
    str(item.get("task_id"))
    for item in (previous_board or {}).get("tasks", [])
    if isinstance(item, dict) and item.get("task_id")
]
previous_tasks_by_id = {
    str(item.get("task_id")): item
    for item in (previous_board or {}).get("tasks", [])
    if isinstance(item, dict) and item.get("task_id")
}
current_tasks_by_id = {item["task_id"]: item for item in task_board_tasks}
field_diff_names = [
    "title",
    "objective",
    "status",
    "rationale",
    "required_evidence",
    "stop_conditions",
]
changed_tasks = []
for task_id in sorted(set(current_task_ids).intersection(previous_task_ids)):
    previous_task = previous_tasks_by_id.get(task_id, {})
    current_task = current_tasks_by_id.get(task_id, {})
    field_changes = {}
    for field_name in field_diff_names:
        previous_value = previous_task.get(field_name)
        current_value = current_task.get(field_name)
        if previous_value != current_value:
            field_changes[field_name] = {
                "previous": previous_value,
                "current": current_value,
            }
    if field_changes:
        changed_tasks.append({
            "task_id": task_id,
            "changed_fields": sorted(field_changes),
            "field_changes": field_changes,
        })
task_board_diff = {
    "schema_version": "ao2.ai-task-board-diff.v1",
    "generated_at_utc": utc_now(),
    "status": "ready",
    "previous_present": previous_board is not None,
    "previous": str(previous_path) if previous_board is not None else None,
    "current": str(task_board_history_root / f"generation-{generation}.json"),
    "previous_task_ids": previous_task_ids,
    "current_task_ids": current_task_ids,
    "added_task_ids": sorted(set(current_task_ids).difference(previous_task_ids)),
    "removed_task_ids": sorted(set(previous_task_ids).difference(current_task_ids)),
    "unchanged_task_ids": sorted(set(current_task_ids).intersection(previous_task_ids)),
    "changed_task_ids": [item["task_id"] for item in changed_tasks],
    "changed_tasks": changed_tasks,
    "field_diff_names": field_diff_names,
    "selection_changed": (previous_board or {}).get("source_recommendation", {}).get("selection") != source_recommendation["selection"] if previous_board else None,
    "release_objective_changed": (previous_board or {}).get("release_objective") != release_objective if previous_board else None,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
task_board["exports"] = {
    "markdown": str(task_board_root / "board.md"),
    "html": str(task_board_root / "board.html"),
}
task_board["history"] = {
    "root": str(task_board_history_root),
    "current": str(task_board_history_root / f"generation-{generation}.json"),
    "latest": str(task_board_history_root / "latest.json"),
    "diff": str(task_board_root / "task-board-diff.json"),
    "previous_present": previous_board is not None,
}
task_board["diff"] = task_board_diff

status_lanes = {}
kind_lanes = {"product_code": [], "evidence_gate": []}
for item in task_board_tasks:
    status_lanes.setdefault(str(item["status"]).replace("_", " ").title(), []).append(item)
    kind_lanes.setdefault(item["kind"], []).append(item)

def task_md(item: dict) -> str:
    evidence = ", ".join(f"`{value}`" for value in item["required_evidence"])
    stops = "; ".join(item["stop_conditions"])
    return (
        f"- [ ] `{item['task_id']}` **{item['title']}**\n"
        f"  - Kind: `{item['kind']}`\n"
        f"  - Status: `{item['status']}`\n"
        f"  - Required Evidence: {evidence}\n"
        f"  - Stop Conditions: {stops}\n"
    )

task_board_md = (
    f"# AO2 AI Task Board {generation}\n\n"
    f"Release train: `{release_train['version']}` - {release_train['theme']}\n\n"
    f"{release_objective}\n\n"
    f"Source recommendation: `{source_recommendation['selection']}`\n\n"
    "Control plane role: read-only observer; no credential requirement; no release mutation authority.\n\n"
    "## Status Lanes\n\n"
    + "".join(
        f"### {status}\n\n" + "".join(task_md(item) for item in items) + "\n"
        for status, items in sorted(status_lanes.items())
    )
    + "## Work Type Lanes\n\n"
    + "### Product Code\n\n"
    + "".join(task_md(item) for item in kind_lanes.get("product_code", []))
    + "\n### Evidence Gates\n\n"
    + "".join(task_md(item) for item in kind_lanes.get("evidence_gate", []))
)

def task_card(item: dict) -> str:
    evidence = "".join(f"<li><code>{html.escape(str(value))}</code></li>" for value in item["required_evidence"])
    stops = "".join(f"<li>{html.escape(str(value))}</li>" for value in item["stop_conditions"])
    status_class = "status-" + html.escape(str(item["status"]).lower().replace(" ", "_"))
    transition = item.get("status_transition") or {}
    transition_reason = str(transition.get("reason") or "")
    transition_html = (
        f"<p class=\"status-reason\">{html.escape(transition_reason)}</p>"
        if transition_reason
        else ""
    )
    return (
        f"<article class=\"task-card {status_class}\">"
        f"<h3>{html.escape(item['title'])}</h3>"
        f"<p><code>{html.escape(item['task_id'])}</code> - <code>{html.escape(item['kind'])}</code> - <span class=\"status-pill {status_class}\">{html.escape(str(item['status']).replace('_', ' ').title())}</span></p>"
        + transition_html +
        f"<p>{html.escape(item['objective'])}</p>"
        "<h4>Required Evidence</h4><ul>" + evidence + "</ul>"
        "<h4>Stop Conditions</h4><ul>" + stops + "</ul>"
        "</article>"
    )

task_board_html = (
    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">"
    "<title>AO2 AI Task Board</title>"
    "<style>body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px;color:#17202a}"
    ".status-lane{border:1px solid #d7dde2;border-radius:6px;padding:16px;margin:16px 0}"
    ".task-card{border-top:1px solid #e6eaee;padding:12px 0}.task-card:first-child{border-top:0}"
    "code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;background:#f4f6f8;padding:1px 4px;border-radius:4px}"
    "</style></head><body>"
    f"<h1>AO2 AI Task Board</h1><p>{html.escape(release_objective)}</p>"
    f"<p>Release train: <code>{html.escape(release_train['version'])}</code> - {html.escape(release_train['theme'])}</p>"
    "<h2>Status Lanes</h2>"
    + "".join(
        f"<section class=\"status-lane\"><h2>{html.escape(status)}</h2>"
        + "".join(task_card(item) for item in items)
        + "</section>"
        for status, items in sorted(status_lanes.items())
    )
    + "<h2>Product Code</h2>"
    + "".join(task_card(item) for item in kind_lanes.get("product_code", []))
    + "<h2>Evidence Gates</h2>"
    + "".join(task_card(item) for item in kind_lanes.get("evidence_gate", []))
    + "</body></html>\n"
)

(packet_root / "packet.md").write_text(packet_md, encoding="utf-8")
(packet_root / "board.md").write_text(board_md, encoding="utf-8")
(packet_root / "executor-evidence.json").write_text(json.dumps(executor, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(packet_root / "pulse-eval-loop.json").write_text(json.dumps(eval_loop, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(packet_root / "pulse-task-manifest.json").write_text(json.dumps(task_manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(task_board_root / "summary.json").write_text(json.dumps(task_board, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(task_board_root / "board.md").write_text(task_board_md, encoding="utf-8")
(task_board_root / "board.html").write_text(task_board_html, encoding="utf-8")
(task_board_root / "task-board-diff.json").write_text(json.dumps(task_board_diff, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(task_board_history_root / f"generation-{generation}.json").write_text(json.dumps(task_board, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(task_board_history_root / "latest.json").write_text(json.dumps(task_board, indent=2, sort_keys=True) + "\n", encoding="utf-8")
(packet_root / "task-board.json").write_text(json.dumps(task_board, indent=2, sort_keys=True) + "\n", encoding="utf-8")

files = []
for path in sorted(packet_root.iterdir()):
    if path.is_file():
        files.append({"path": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})

packet_summary = {
    "schema_version": "ao2.pulse-next-lengthy-tasks.v1",
    "generated_at_utc": utc_now(),
    "status": "ready",
    "artifact_root": str(packet_root),
    "cursor": next_cursor,
    "selection": selection["id"],
    "generation_mode": generation_mode,
    "local_only_while_pr_blocked": local_only_while_pr_blocked,
    "task_board_summary": str(task_board_root / "summary.json"),
    "project_level_reassessment": project_level_reassessment,
    "strategic_score": selected_score,
    "strategic_scores": strategic_scores,
    "rationale": selection["rationale"],
    "required_evidence": selection["required_evidence"],
    "stop_conditions": selection["stop_conditions"],
    "task_count": len(tasks),
    "tasks": tasks,
    "task_board": str(packet_root / "task-board.json"),
    "files": files,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
(packet_root / "summary.json").write_text(json.dumps(packet_summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
files.append({"path": "summary.json", "sha256": hashlib.sha256((packet_root / "summary.json").read_bytes()).hexdigest()})

summary = {
    "schema_version": "ao2.pulse-generate-next.v1",
    "generated_at_utc": utc_now(),
    "status": "ready",
    "artifact_root": str(out_root),
    "packet_root": str(packet_root),
    "cursor": next_cursor,
    "selection": selection["id"],
    "generation_mode": generation_mode,
    "local_only_while_pr_blocked": local_only_while_pr_blocked,
    "task_board_summary": str(task_board_root / "summary.json"),
    "project_level_reassessment": project_level_reassessment,
    "strategic_score": selected_score,
    "strategic_scores": strategic_scores,
    "rationale": selection["rationale"],
    "required_evidence": selection["required_evidence"],
    "stop_conditions": selection["stop_conditions"],
    "recommended_tasks": tasks,
    "task_board": str(packet_root / "task-board.json"),
    "files": files,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"packet_root={packet_root}")
print("status=ready")
PY

if [ "$REGISTER" = "1" ]; then
  AO2_PULSE_LOCAL_MIRROR_SOURCE="$PACKET_ROOT" \
    AO2_PULSE_AUTO_ADVANCE_PROMPT="$AUTO_ADVANCE_PROMPT" \
    npm run pulse:register-auto-advance
fi
