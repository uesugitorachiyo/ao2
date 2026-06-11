# Verification Ledger

Last verified: 2026-05-27

## Commands

```sh
npm run verify
npm run build:release
npm run package:local
npm run phase1:prepare-prerequisites
npm run phase1:promote
AO2_PHASE1_DASHBOARD_SNAPSHOT=1 npm run phase1:promote
npm run phase1:dashboard-snapshot
npm run ci:local
npm run ci:license-provenance
npm run release:verify-provenance
npm run smoke:three-os
npm run verify:replacement   # 4-step replacement-parity composite (Phase 2 readiness)
npm run verify:no-factory-v3 # factory-v3 green-path regression guard
npm run risky-pr:golden      # local Risky PR Run golden path with report/cockpit assertions
npm run risky-pr:product-readiness # one-run product readiness gate for local run record/report/closure evidence
npm run evaluator:closure-corpus # evaluator closure negative/positive evidence corpus
npm run approval:exact-digest-gate # exact-digest approval denial/approval/replay/report gate
npm run smoke:evidence-control-plane # signed evidence-pack publish/readback contract smoke
npm run smoke:phase1-operator-golden # signed Phase 1 publish/readback/dashboard smoke
npm run release:readiness    # local release-readiness guardrails for AO2 + control-plane
npm run release:readiness:regression-gate # local static/smoke/Pulse/control-plane evidence gate
npm run local:canary         # local equivalent of the manual Local Canary workflow
npm run artifacts:ci-download-contract # real CI artifact download contract
npm run artifacts:index      # local cross-repo artifact index/report
npm run artifacts:health     # summarize latest local artifact index health
AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION=1 npm run artifacts:health
npm run release:artifact-consumer-smoke -- --dry-run # CI artifact consumer smoke contract
npm run release:artifact-consumer-smoke -- --require-artifact ao2-python-guard --require-schema ao2.python-guard-ci-artifacts.v1
npm run post-merge:canary    # local post-merge AO2 + control-plane canary
npm run pulse:register-auto-advance # register the local Pulse auto-advance prompt
npm run pulse:auto-advance # run the registered local Pulse task packet once with stop/dedup guards
npm run pulse:auto-advance -- --forever # keep polling registered Pulse packets until STOP/failure/interruption
npm run pulse:generate-next # generate and register the next local Pulse packet from daemon evidence
npm run pulse:generate-next:contract # static contract for next-packet generation
npm run pulse:task-executor # execute structured Pulse task manifests and materialize product-code implementation packets
npm run pulse:daemon:start # install/load the local supervisor for Pulse auto-advance
npm run pulse:daemon:status # report supervisor and Pulse heartbeat evidence
npm run pulse:daemon:stop # stop the supervisor and write the local STOP file
npm run pulse:daemon:restart # restart the local Pulse supervisor
npm run pulse:daemon:contract # static contract for the local Pulse daemon
npm run pulse:resume-workspace-cli-fallback # verify workspace CLI fallback for Pulse resume
npm run pulse:terminal-eval-loop-schema-compatibility # normalize script packets to terminal eval-loop evidence
npm run pulse:auto-advance-runner-contract # static contract for the local auto-advance runner
npm run pulse:stop-and-dedup-ledger # stop signal and duplicate digest ledger evidence
npm run pulse:auto-advance-integration-gate # composed auto-advance restart gate
npm run pulse:lengthy-gate:contract # static contract for the manifest-driven lengthy gate runner
npm run pulse:lengthy-gate -- --gate pulse-consolidation # run one promoted lengthy gate by manifest id
npm run pulse:shared-gate-lib-audit # shared Pulse gate helper audit
npm run pulse:shared-gate-library-migration # shared gate helper migration evidence
npm run public:hardening # CI-friendly public hardening subset
npm run public:hardening-ci-workflow # local evidence for tracked public hardening CI workflow
npm run public:hardening-workflow-file-dry-run # render side-effect-free public hardening workflow preview
npm run public:hardening-workflow-tracked-proposal # tracked workflow proposal evidence
npm run public:hardening-ci-local-runner-parity # compare workflow commands to local runner commands
npm run scripts:tracking-intent-audit # script tracking intent and promotion manifest
npm run scripts:tracking-decision-cleanup # script promotion decision cleanup evidence
npm run scripts:tracking-review-pack # script promotion pre-commit review pack evidence
npm run scripts:tracking-review-to-commit-plan # script promotion minimal commit plan evidence
npm run scripts:tracking-commit-ready-diff # script promotion commit-ready diff evidence
npm run scripts:tracking-ready-review-pack # script promotion ready review packet evidence
npm run scripts:surface-audit # preserve and classify local RSI/Pulse scripts before promotion
npm run pulse:next-task-quality-filter # next task quality filter
npm run pulse:quality-filter-negative-corpus # Pulse quality filter negative fixtures
npm run pulse:quality-filter-required-gate # required Pulse quality gate boundary evidence
npm run pulse:resume -- --dry-run # validate the resumable Pulse event-loop command
npm run pulse:resume -- --execute # explicitly resume the latest local Pulse event loop
npm run pulse:execute-safety-corpus # Pulse execute-mode refusal/simulation corpus
npm run pulse:real-execute-containment # bounded real Pulse execute fixture
npm run phase1:promotion-golden # Phase 1 promotion golden readback/token-boundary evidence
npm run release:evidence-closure # final local release evidence closure JSON/HTML
npm run mvp:acceptance-matrix-gate # PRD AC / Risky PR UAT evidence matrix
npm run workbench:no-archaeology-audit # cockpit/workbench inspectability audit
npm run control-plane:observer-hardening # read-only observer + restore hardening
npm run provider:phase2-contract-hardening # provider contract Phase 2 hardening gate
npm run release:train-drill # side-effect-free public release train rehearsal
npm run release:cross-os-attestation # CI-safe cross-OS release artifact attestation
npm run next:lengthy:gate # aggregate local gate for the next lengthy task set
npm run control-plane:cross-repo-observer # cross-repo AO2/control-plane observer integration
npm run release:install-update-fixture # signed fixture install/update verification
npm run workbench:browser-qa # no-archaeology workbench browser-review evidence
npm run provider:adversarial-corpus # adversarial provider transcript corpus gate
npm run release:dr-retention-snapshot # DR/retention long-run fixture snapshot
npm run frontier:lengthy:gate # aggregate local gate for the frontier lengthy task set
npm run gate:full            # 3-stage ready-to-ship gate (guard + replacement + release-gate)
scripts/smoke-release-archives.sh
```

Pulse event-loop evidence written under `target/pulse-next-recommended-tasks`
is local and ignored, but it can be removed by `cargo clean`. When preserving a
local chain across cleanup, mirror the same packet, board, executor evidence,
and `pulse-eval-loop.json` under `.ao2-local/pulse/`; that path is also local
and ignored, but is outside Cargo's build directory:

```sh
npm run pulse:local-mirror
npm run pulse:register-auto-advance
npm run pulse:auto-advance
npm run pulse:auto-advance -- --forever
npm run pulse:pr-ci-gate:update
npm run pulse:generate-next
npm run pulse:task-board-state
npm run pulse:next-actions
npm run pulse:daemon:start
npm run pulse:daemon:status
```
The Pulse RSI core is local-first and public-safe. `npm run pulse:register-auto-advance`
records the operator prompt in `.ao2-local/pulse/latest/operator-prompt.txt`,
adds `operator_prompt_sha256` to `resume.json`, and emits
`ao2.pulse-auto-advance-registration.v1` at
`target/pulse-auto-advance-registration/latest/summary.json`.
`npm run pulse:auto-advance` verifies the registered eval-loop and prompt
digests, honors `.ao2-local/pulse/STOP`, rejects duplicate eval-loop digests via
`.ao2-local/pulse/pulse-auto-advance-ledger.jsonl`, runs `recommended_tasks`,
and emits `ao2.pulse-auto-advance-run.v1` at
`target/pulse-auto-advance/latest/summary.json`. When the packet contains a
sibling `pulse-task-manifest.json`, auto-advance delegates execution to
`npm run pulse:task-executor` so `product_code` tasks become implementation
packets instead of command-only shell tasks. With `--forever`, it writes
`ao2.pulse-auto-advance-heartbeat.v1` while waiting and calls
`npm run pulse:generate-next` after each successful packet. Before generating
the next packet, auto-advance runs `npm run pulse:pr-ci-gate:update` to refresh
the local PR/CI gate state from the current read-only GitHub PR/check view. The
updater emits `ao2.pulse-pr-ci-gate-update.v1` under
`target/pulse-pr-ci-gate-update/latest/summary.json` and writes
`ao2.pulse-pr-ci-gate.v1` to `AO2_PULSE_PR_CI_GATE_UPDATE_STATE`, defaulting to
`.ao2-local/pulse/pr-ci-gate.json`. Auto-advance then reads that same file via
`AO2_PULSE_AUTO_ADVANCE_PR_CI_GATE_STATE`. When the gate state reports an open
or draft PR, pending or failed `required_checks`, or a non-green gate status,
auto-advance emits `waiting_for_pr_merge_or_ci` with a `pr_ci_gate` summary and
skips `pulse:generate-next` so RSI does not create more work before the current
PR is merged and green. For local-only while PR-blocked mode, start the runner
or daemon with `AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED=1`. In that
opt-in mode the PR gate still blocks normal product-code advancement, but
auto-advance may call `npm run pulse:generate-next` with
`AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY=1` and register a
`generated_local_only_packet` containing evidence/readiness tasks only. This
lets overnight Pulse runs keep collecting local evidence while an open PR waits
for review or merge, without creating another PR or product-code implementation
packet.

`npm run pulse:direct-main-publish` is the opt-in direct-main publishing gate
used by unattended Pulse runs that should commit and push from the CLI instead
of opening a PR. Start auto-advance with
`AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH=1` to enable it after each
successful task batch. The publisher emits
`ao2.pulse-direct-main-publish.v1` under
`target/pulse-direct-main-publish/latest/summary.json`, requires the current
branch to be `main`, fetches `origin/main`, requires local `HEAD` to equal the
remote before publishing, rejects disallowed local artifact/credential paths,
runs `AO2_PULSE_DIRECT_MAIN_PUBLISH_VERIFY_COMMAND` (default:
`PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py
-q`) with recursive Pulse auto-advance, local-only generation, and direct-main
publish environment flags forced off, commits the validated changed paths,
verifies the remote is still an ancestor, and pushes `HEAD:main`. If there are
no tracked or untracked source changes, it exits successfully with
`status=skipped` and does not commit.

`npm run pulse:generate-next` emits `ao2.pulse-generate-next.v1` at
`target/pulse-generate-next/latest/summary.json` and writes a fresh
`packet.md`, `board.md`, `executor-evidence.json`,
`pulse-eval-loop.json`, and
`pulse-task-manifest.json` / `ao2.pulse-next-lengthy-tasks.v1` packet under
`target/pulse-next-recommended-tasks`, which is also the default
`pulse:local-mirror` source used by release-readiness gates. It also emits an
operator-readable `ao2.ai-task-board.v1` control-surface artifact at
`target/pulse-task-board/latest/summary.json`, plus companion
`board.md` and `board.html` exports grouped into status and work-type lanes.
Tasks include generation-specific `task_id` values plus stable `stable_task_id`
values so status evidence can carry across board generations without binding
operators to a stale generated identifier. It also writes
`target/pulse-task-board/latest/task-board-diff.json` and keeps
generation snapshots under
`${AO2_PULSE_TASK_BOARD_HISTORY_ROOT:-.ao2-local/pulse/task-board-history}` so
operators can see whether the selected work changed between Pulse generations.
When `AO2_PULSE_TASK_BOARD_STATUS_EVIDENCE` points at an
`ao2.ai-task-board-status-evidence.v1` JSON file, the generated board applies
task-id or stable-task-id keyed status transitions and records
`status_transition_source` plus per-task `status_transition` evidence. Evidence
from a mismatched generation is ignored with a visible `stale_generation`
warning rendered in `board.md` and `board.html` so old executor output cannot
silently influence the current board. The diff now includes
`changed_task_ids`, `changed_tasks`, and field-level `field_changes` for task
title, objective, status, rationale, required evidence, and stop conditions.
If `AO2_PULSE_TASK_BOARD_STATUS_EVIDENCE` is not set, `pulse:generate-next`
auto-discovers AO2's own executor output at
`${AO2_PULSE_TASK_EXECUTOR_ROOT:-target/pulse-task-executor/latest}/task-board-status-evidence.json`
and applies it only when the evidence generation matches the generated board.
The generator also writes
`target/pulse-task-board/latest/board-state-summary.json` using
`ao2.ai-task-board-state-summary.v1`, a compact read-only summary with task
status counts and next actions for dashboard/control-plane ingestion.
`npm run pulse:task-board-state` reads the current board without regeneration
and emits `ao2.pulse-task-board-state.v1` at
`target/pulse-task-board-state/latest/summary.json` for local dashboards,
operator scripts, or any standalone AO2 install that only needs the current
task state. `npm run pulse:next-actions` reads the same current board and emits
`ao2.pulse-next-actions.v1` plus
`target/pulse-next-actions/latest/next-actions.md`, giving standalone AO2
operators a compact command for the current actionable task list. Set
`AO2_PULSE_NEXT_ACTIONS_STATUS=proposed,in_progress` to show only specific task
statuses. Both commands write failed summaries for missing, invalid-schema, or
invalid-JSON board inputs so local operators can diagnose stale or malformed
board artifacts without regenerating.
The board preserves the current release objective, source recommendation,
rationale, required evidence, stop conditions, and read-only control-plane
readback semantics without granting mutation authority. For the Risky PR
product MVP and AI task-board selections, the manifest includes a product-code
implementation packet before the supporting evidence gates. Generated
product-code manifests include
`product_code_execution.enabled=true` with `mode=dry_run` so the next executor
pass validates code-agent runner packet compatibility without granting write
execution. Generated packets use strategic scoring instead of blind
rotation: each cycle performs project-level reassessment
against `docs/PRD.md`, `docs/SDD-risky-pr-run.md`,
`docs/SCHEMAS-AND-INTERFACES.md`, and `docs/IMPLEMENTATION-SLICES.md`, samples
ledger history from `.ao2-local/pulse/pulse-auto-advance-ledger.jsonl`, applies
anti-recursion penalties, and includes rationale, required evidence, stop
conditions, and per-candidate `strategic_score` metadata. `npm run
pulse:daemon:start` runs the forever loop through launchctl or a detached tmux
fallback; `npm run pulse:daemon:status` emits `ao2.pulse-daemon.v1` at
`target/pulse-daemon/latest/summary.json`.

The generated `task-board.json` uses `ao2.ai-task-board.v1` for the v0.4.81 AI
task board/control-surface train. It records the release objective, selected
task lane, recommended task statuses, operator `next_action`, rationale,
evidence requirements, stop conditions, and a read-only control-plane trust
boundary so Pulse can expose operator-visible work without giving the control
plane release mutation authority.
`npm run pulse:next-task-quality-filter` reads that task-board artifact when it
is present, fails closed if the release objective, required evidence, or stop
conditions are missing, and records `task_board_drift_gate` plus
`task_board_blockers` in its summary. When
`AO2_PULSE_NEXT_TASK_QUALITY_STATUS_EVIDENCE` points at
`ao2.ai-task-board-status-evidence.v1`, the quality filter also fails closed on
unknown task ids or stale `task_board_generation` values and records
`status_evidence_gate` plus `status_evidence_blockers`. Evidence keys may use
the generated `task_id` or the task's stable `stable_task_id`, matching
`pulse:generate-next` status carryover semantics without accepting arbitrary
unknown IDs. The quality-filter summary records `status_evidence_matches` and
`status_evidence_match_counts` so operators can see whether evidence matched by
generated or stable task id.
`npm run control-plane:fixture-consumer-smoke` can also read the task board
through `AO2_CP_FIXTURE_CONSUMER_TASK_BOARD`, or through the fixture catalog produced by
`evidence:operator-index-control-plane-fixture-ingest` when
`AO2_OPERATOR_INDEX_CP_TASK_BOARD` points at a valid board. Both paths record
read-only `task_board_readback` without credentials or release mutation
authority. When readback passes, the smoke also writes
`operator-task-board-view/summary.json` with
`ao2.control-plane-operator-task-board-view.v1` and a local
`operator-task-board.html` read-only operator view.

`npm run pulse:task-executor` reads an `ao2.pulse-task-manifest.v1` manifest
from `.ao2-local/pulse/latest/pulse-task-manifest.json` by default and emits
`ao2.pulse-task-executor.v1` evidence under
`target/pulse-task-executor/latest/summary.json`. Evidence-gate and verification
tasks may run local commands, while `product_code` tasks materialize
product-code implementation packets under `implementation-packets/` without
requiring a shell command. product_code tasks require verification evidence:
the executor rejects packets that do not name both a verification command and
expected evidence. A product_code task cannot close from packet materialization alone.
The executor also writes
`target/pulse-task-executor/latest/task-board-status-evidence.json` using
`ao2.ai-task-board-status-evidence.v1`, and its summary exposes that path as
`status_evidence` so the next `pulse:generate-next` run can apply executor
results back to the AI task board.
When a manifest sets `product_code_execution.enabled=true`, product-code tasks can opt into `pulse:code-agent-runner`
instead of packet-only materialization.
`product_code_execution.mode=dry_run` validates the generated
`ao2.pulse-code-agent-task.v1` packet through the runner and records the runner
summary path in task-executor evidence. `product_code_execution.mode=execute`
uses the same guarded runner execution path and still requires
`AO2_PULSE_CODE_AGENT_EXECUTE=1`, allowed-file checks, unrelated-dirty-file
checks, and declared verification evidence before the task can pass.
The executor rejects non-local manifests and any manifest that stores
credentials.

`npm run pulse:code-agent-runner -- --task <task.json> --dry-run` validates an
`ao2.pulse-code-agent-task.v1` implementation-task packet and emits
`ao2.pulse-code-agent-runner.v1` evidence under
`target/pulse-code-agent-runner/latest/summary.json`. The command's
dry-run validates implementation-task packets by checking local-only trust boundaries, allowed
files, acceptance criteria, required verification commands and expected
evidence, target git worktree status, and unrelated dirty files. This MVP is a
guarded bridge for future code-agent execution: it does not push, open PRs, publish releases, or store credentials.
`npm run pulse:code-agent-runner -- --task <task.json> --execute` enables the
same runner in guarded local execution mode; execute mode requires `AO2_PULSE_CODE_AGENT_EXECUTE=1`.
The runner writes a prompt artifact,
invokes the task's local code-agent command or `codex exec`, strips provider API
key environment variables, rejects unrelated dirty files after execution, and
requires all declared verification commands to pass before emitting
`status=passed`.

The Pulse lengthy-gate surface is manifest-driven so local RSI follow-up
wrappers can be preserved before promotion without adding one public command per
wrapper. `scripts/pulse-lengthy-gates-manifest.json` records the preserved
wrapper names, disposition, and npm command sequence for each consolidated
gate. `npm run pulse:lengthy-gate:contract` validates the
`ao2.pulse-lengthy-gates-manifest.v1` manifest and writes
`ao2.pulse-lengthy-gate-runner.v1` evidence under
`target/pulse-lengthy-gate/latest/summary.json`. `npm run pulse:lengthy-gate --
--gate <id>` runs only a named promoted gate; if a manifest command is not
exposed in `package.json`, the runner blocks before execution and reports
`missing_package_commands`. The runner is local-only, stores no credentials,
does not delete files, and does not push.

`npm run scripts:surface-audit` snapshots untracked local RSI/Pulse shell
scripts into ignored evidence, classifies each script as promote candidate,
local-only, consolidate, defer control-plane, or remove-later, and reports
missing package command references without running those wrappers. The gate emits
`ao2.script-surface-audit.v1` at
`target/script-surface-audit/latest/summary.json`, plus
`snapshot-manifest.json` and `classification-report.md`. This is preservation
and decision support only: it does not auto-promote, delete, push, publish, or
store credentials.

`npm run scripts:tracking-decision-cleanup` promotes the local script tracking
decision cleanup wrapper into a tracked, public-safe evidence gate. It runs the
script tracking intent audit, writes `ao2.script-tracking-decision-cleanup.v1`
at `target/script-tracking-decision-cleanup/latest/summary.json`, and emits
`target/script-tracking-decision-cleanup/latest/pre-commit-cleanup-list.json`
with track-in-repo and local-only decision lists. The gate is local-only, stores
no credentials, and performs no publishing or repository mutation.

`npm run scripts:tracking-review-pack` promotes the local script tracking
review pack wrapper into a tracked, public-safe evidence gate. It runs the
script tracking decision cleanup gate, writes
`ao2.script-tracking-review-pack.v1` at
`target/script-tracking-review-pack/latest/summary.json`, and emits
`tracking-review-pack.json` plus `tracking-review-pack.md` for manual
pre-commit review of tracked script candidates and local-only artifacts. The
gate is local-only, stores no credentials, and performs no publishing or
repository mutation.

`npm run scripts:tracking-review-to-commit-plan` promotes the local script
tracking review-to-commit-plan wrapper into a tracked, public-safe evidence
gate. It runs the script tracking review pack gate, writes
`ao2.script-tracking-review-to-commit-plan.v1` at
`target/script-tracking-review-to-commit-plan/latest/summary.json`, and emits
`minimal-commit-plan.json` with tracked PR files separated from untracked
local-only script artifacts. The gate is local-only, stores no credentials, and
performs no publishing or repository mutation.

`npm run scripts:tracking-commit-ready-diff` promotes the local script tracking
commit-ready-diff wrapper into a tracked, public-safe evidence gate. It runs the
review-to-commit-plan gate, writes `ao2.script-tracking-commit-ready-diff.v1`
at `target/script-tracking-commit-ready-diff/latest/summary.json`, and emits
`commit-ready-diff-manifest.json` with tracked PR files separated from
untracked local-only script artifacts. The gate is local-only, stores no
credentials, and performs no publishing or repository mutation.

`npm run scripts:tracking-ready-review-pack` promotes the local script tracking
ready-review-pack wrapper into a tracked, public-safe evidence gate. It runs the
commit-ready-diff gate, writes `ao2.script-tracking-ready-review-pack.v1` at
`target/script-tracking-ready-review-pack/latest/summary.json`, and emits
`human-review-packet.md` plus `commit-ready-summary.json` with tracked PR files
separated from untracked local-only script artifacts. The gate is local-only,
stores no credentials, and performs no publishing or repository mutation.

`npm run pulse:shared-gate-library-migration` promotes the local helper
migration wrapper into a tracked, public-safe evidence gate. It runs the shared
gate helper audit, writes `ao2.shared-gate-library-migration.v1` at
`target/shared-gate-library-migration/latest/summary.json`, and emits a helper
adoption matrix at
`target/shared-gate-library-migration/latest/helper-adoption-matrix.json`.
The gate is local-only, stores no credentials, and performs no publishing or
repository mutation.


The mirror also writes `.ao2-local/pulse/latest/resume.json` and
`.ao2-local/pulse/latest/resume-command.sh` with the latest
`pulse-eval-loop.json` digest so a later local event-loop run can resume the
chain after `target/` cleanup. Validate that resume packet without executing a
new loop by running:

```sh
npm run pulse:resume -- --dry-run
```

Result:

- `cargo fmt --all -- --check`: passed
- `cargo test --workspace`: passed (ao2 + ao2-control-plane)
- `cargo clippy --workspace --all-targets -- -D warnings`: passed
- `cargo build --release -p ao2-cli`: passed
- local release-binary smoke path: passed
- `npm run verify:replacement`: PASS 4/4 (provider-readiness producer,
  factory-v3 parity oracle, provider-contract-verify all required,
  license-provenance gate)
- `npm run verify:no-factory-v3`: PASS with `failure_count = 0`; emits
  `ao2.no-factory-v3-green-path.v1`
- `npm run gate:full`: PASS 3/3 (no-factory-v3 guard, replacement-parity,
  then canonical `ao2 release gate`); emits
  `ao2.release-gate-with-replacement-parity.v1`
- `npm run risky-pr:golden`: runs the provider-free Risky PR Run through
  policy denial, exact approval, evaluator rejection, correction, accepted
  closure, replay, evidence-pack export, report rendering, and cockpit index;
  emits `ao2.risky-pr-golden-path.v1`. The generated static report exposes
  `Local Run Record`, `Static Export Evidence`, `Objective`, `Run Health`,
  `Policy Decisions`, `Approvals`, `Artifacts`, `Evaluator Closure Evidence`,
  and `Replay Evidence` sections, and the sibling
  `ao2.risky-pr-static-report-index.v1` JSON sidecar maps operator questions
  to report/export/replay evidence without filesystem archaeology. The sidecar
  records the required report sections, the sections present in the rendered
  HTML, and a fail-closed `report_contract_complete` result. The golden path
  also runs `ao2 report verify`, which emits
  `ao2.report-contract-verification.v1` against the reusable
  `ao2.report-contract.v1` schema. The report and index expose denied
  `request_digest` and approved `action_digest` values under the
  `approval_boundary` summary so operators can inspect the exact approval
  boundary without opening raw evidence JSON. The same run assembles a
  release support bundle through `ao2 release support-bundle-build`, generates
  the embedded report-contract verification from the static report inputs,
  includes `ao2.install-verification-evidence.v1` as first-class install
  evidence, verifies the `ao2.cp-release-support-bundle.v1` bundle and
  `SHA256SUMS`, and records the `ao2.release-support-bundle-build.v1` result
  in the golden summary. CI also runs the same command in the
  `Risky PR golden release support bundle artifacts` job and uploads
  `ao2-risky-pr-golden-release-support-bundle` with the golden summary,
  `ao2.risky-pr-golden-artifact-manifest.v1` `artifact-manifest.json`,
  report-contract verification, support-bundle build result,
  `release-support-bundle.json`, and `SHA256SUMS`. The same job checks out
  ao2-control-plane and runs its ao2-control-plane offline verifier against
  the generated bundle/checksum pair, uploading
  `release-support-bundle-control-plane-verify.json` with the handoff
  evidence. `tests/fixtures/release-support-bundle-contract-v1.json` is the
  shared AO2/control-plane contract fixture for this handoff shape; AO2's
  release-support verifier and ao2-control-plane's offline verifier both
  consume a byte-identical copy so schema drift fails before release. CI's
  `Release support fixture parity with ao2-control-plane` job checks out both
  repos, compares the two fixture files byte-for-byte, and uploads
  `ao2-release-support-fixture-parity` with SHA-256 evidence.
- `npm run risky-pr:control-plane-bridge`: validates a downloaded Risky PR
  golden `artifact-manifest.json`, writes a stable local
  `target/risky-pr-golden-control-plane-bridge/latest/artifact-manifest.json`,
  mirrors it to
  `../ao2-control-plane/target/risky-pr-golden-control-plane-bridge/artifact-manifest.json`,
  emits `control-plane.env` with
  `AO2_CP_RISKY_PR_GOLDEN_ARTIFACT_MANIFEST`, and can smoke the read-only
  ao2-control-plane observer endpoints when `--cp-base-url` and
  `AO2_CP_API_TOKEN` are provided.
- `npm run release:train-control-plane-bridge`: generates or accepts an AO2
  public release-train drill summary, writes a stable local
  `target/release-train-control-plane-bridge/latest/release-train-summary.json`,
  mirrors it to
  `../ao2-control-plane/target/release-train-control-plane-bridge/release-train-summary.json`,
  emits `control-plane.env` with `AO2_CP_RELEASE_TRAIN_SUMMARY`, then runs the
  ao2-control-plane `smoke-release-train-bridge.py` read-only observer smoke
  unless `--skip-smoke` is supplied. The bridge emits
  `ao2.release-train-control-plane-bridge.v1` at
  `target/release-train-control-plane-bridge/latest/summary.json`. CI runs a
  fixture-backed invocation with
  `ao2-control-plane/tests/fixtures/public-release-train-summary.json` as
  `Release train control-plane bridge artifacts` and uploads
  `ao2-release-train-control-plane-bridge` with both the bridge summary and the
  `ao2.cp-release-train-bridge-smoke.v1` readback evidence.
- `npm run risky-pr:product-readiness`: runs the Risky PR golden path once,
  then verifies local run record, static report/export, and evaluator closure
  evidence from that single run; emits
  `ao2.risky-pr-product-readiness-gate.v1`
- `npm run evaluator:closure-corpus`: runs the Risky PR golden path, mutates
  evidence fixtures for `missing_test_evidence`, `unresolved_high_concern`,
  `invalid_artifact_digest`, and `unapproved_risky_action`, and verifies the
  accepted corrected run as `accepted_after_correction`. It emits
  `ao2.evaluator-closure-corpus.v1` and proves accepted closure stays bound to
  concrete evidence, valid artifact digests, and exact approval.
- `npm run approval:exact-digest-gate`: runs the Risky PR golden path, verifies
  the broad `git push` action remains denied, the narrow filesystem write is
  approved only for the exact action digest, a modified approval digest is
  rejected by gate evidence, replay has zero digest failures, and the static
  report/index expose the denial, approval, and replay boundary. It emits
  `ao2.exact-digest-approval-gate.v1`.
- `npm run smoke:evidence-control-plane`: builds AO2 and ao2-control-plane,
  publishes a signed `ao2.evidence-pack.v1`, reads dashboard/detail/latest
  observer endpoints, pins the `ao2.cp-evidence-pack-dashboard.v1`,
  `ao2.cp-evidence-pack-detail.v1`, and `ao2.cp-ingest-receipt.v1` schemas,
  and verifies the control-plane remains a read-only observer
- `npm run smoke:phase1-operator-golden`: runs the signed Phase 1 decision
  publish/readback path against a local control-plane instance, checks the
  dashboard, operator panel, and Phase 1 operator support bundle verification,
  and emits
  `ao2.phase1-operator-golden-path-smoke.v1`
- `npm run release:readiness`: checks public CI triggers, manual release
  workflows, branch protection, latest `main` CI status in both public repos,
  and the local next-length verification commands; emits
  `ao2.release-readiness-local.v1` plus local `report.md`, `report.html`, and
  an `artifact-closure-index.json` file with
  `ao2.release-artifact-closure-index.v1` coverage for
  `ao2-release-readiness`, `ao2-release-train-control-plane-bridge`,
  `ao2-ai-task-board-control-plane-bridge`,
  `ao2-dual-repo-installed-release-smoke`,
  `ao2-release-publication-closure`,
  `ao2-dual-repo-release-publication-closure-index`, and
  `ao2-release-readiness-consumer`
- `Release readiness artifact consumer`: CI job that depends on
  `Release readiness artifacts` and `Release train control-plane bridge
  artifacts`, the AI task-board bridge, dual-repo installed release smoke, and
  `Release publication closure artifacts`, plus the dual-repo publication
  closure index; downloads `ao2-release-readiness`,
  `ao2-release-train-control-plane-bridge`,
  `ao2-ai-task-board-control-plane-bridge`,
  `ao2-dual-repo-installed-release-smoke`, and
  `ao2-release-publication-closure`; validates the
  `ao2.release-readiness-local.v1` summary/status/core cross-OS checks, the
  control-plane bridge/readback schemas, and the
  `ao2.release-publication-dry-run-closure.v1` publication/stable readiness
  fields. The companion `Dual-repo release publication closure index` job
  downloads AO2's `ao2-release-publication-closure` and the latest successful
  control-plane `ao2-control-plane-release-publication-closure`, validates
  `ao2.cp-release-publication-closure.v1`, and uploads
  `ao2-dual-repo-release-publication-closure-index` with
  `ao2.dual-repo-release-publication-closure-index.v1` evidence. The consumer
  then uploads `ao2-release-readiness-consumer` with
  `ao2.release-readiness-artifact-consumer.v1` evidence.
  The operator-facing dual-repo evidence index is documented in
  `docs/release/PUBLIC-RELEASE-VERIFICATION.md`, including AO2 hosted
  post-stable release verification, control-plane post-release verification,
  `ao2-control-plane-release-publication-closure`, and
  `ao2.dual-repo-release-publication-closure-index.v1` evidence.
- `npm run release:readiness:regression-gate`: runs static release readiness,
  Phase 1 operator golden-path smoke, Pulse local mirror, Pulse resume dry-run,
  real CI artifact download contract, artifact indexing, fail-on-attention
  artifact health, release artifact consumer dry-run, and the
  ao2-control-plane long-lived smoke into one local evidence bundle; emits
  `ao2.release-readiness-regression-gate.v1`
- `npm run artifacts:ci-download-contract`: runs
  `release:artifact-consumer-smoke` in non-dry-run mode by default using
  `gh run download`, validates required artifact names and `schema_version`
  values, mirrors AO2 evidence to `target/ci-artifacts/latest`, mirrors
  control-plane evidence to `../ao2-control-plane/target/ci-artifacts/latest`,
  and emits `ao2.ci-artifact-download-contract.v1` at
  `target/ci-artifacts/latest/summary.json`. Use `--fixture-dir <path>` for
  deterministic local tests.
- `npm run artifacts:index`: scans AO2 and ao2-control-plane local/CI evidence
  roots, writes `ao2.artifact-index-report.v1`, renders a local `report.md`,
  and writes the `ao2.artifact-evidence-dashboard.v1` HTML dashboard at
  `target/artifact-index/latest/dashboard.html`
- `npm run artifacts:health`: reads the latest `artifact-index.json`, writes
  `ao2.artifact-evidence-health.v1` to
  `target/artifact-health/latest/summary.json`, and groups failing, missing,
  stale, allowed missing/stale, empty, and healthy evidence bundles for local
  triage. Roots listed in `AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS` are
  reported under allowed attention when missing or stale, but they do not count
  toward `AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION=1` failures.
  Policy knobs:
  `AO2_ARTIFACT_HEALTH_REQUIRED_ROOTS`,
  `AO2_ARTIFACT_HEALTH_ALLOWED_MISSING_ROOTS`,
  `AO2_ARTIFACT_HEALTH_FAIL_ON_ATTENTION`, and
  `AO2_ARTIFACT_HEALTH_STALE_AFTER_SECONDS`
- `npm run release:artifact-consumer-smoke -- --dry-run`: records the clean
  GitHub Actions artifact consumer workflow without downloading artifacts; a
  non-dry run uses `gh run download` and records checksums plus discovered
  `schema_version` values in `ao2.release-artifact-consumer-smoke.v1`.
  `AO2_RELEASE_ARTIFACT_DOWNLOAD_TIMEOUT_SECONDS` bounds each `gh run list` and
  `gh run download`; timeout or download errors are emitted as
  `download_failures` instead of leaving a release dry-run waiting indefinitely.
- `.github/workflows/local-canary.yml`: manual GitHub Actions canary for the
  public repos; it runs the artifact consumer smoke, CI artifact download
  contract, Pulse local mirror/resume dry-run, control-plane negative restore
  drill, artifact index, fail-on-attention artifact health, and uploads
  `ao2-local-canary`
- `npm run local:canary`: runs the same local canary sequence and writes
  `ao2.local-canary-run.v1` to
  `target/local-canary/latest/local-canary-summary.json`
- Pulse execute simulation: a local resume fixture can set
  `simulation=true` and `simulation_output_path` so
  `npm run pulse:resume -- --resume-json <fixture> --execute` writes
  `ao2.pulse-execute-simulation.v1` evidence without starting a real Pulse
  loop
- `npm run pulse:execute-safety-corpus`: runs hash mismatch, unsafe output
  path, missing simulation output path, failing simulated command, and
  dry-run/execute conflict fixtures, then writes
  `ao2.pulse-execute-safety-corpus.v1` to
  `target/pulse-execute-safety-corpus/latest/summary.json`
- `npm run pulse:real-execute-containment`: creates a deterministic local
  resume fixture, executes it through `npm run pulse:resume -- --resume-json
  <fixture> --execute`, permits writes only under
  `target/pulse-real-execute-containment/latest/allowed-output`, then runs a
  product-code execute fixture through `pulse:generate-next`,
  `pulse:task-executor`, and `pulse:code-agent-runner` in a temporary git repo
  below the evidence root. The fixture requires `AO2_PULSE_CODE_AGENT_EXECUTE=1`,
  changes only `allowed.txt`, records `product_code_execute_fixture`,
  `pulse_generate_next_summary`, `pulse_task_executor_summary`, and
  `code_agent_summary`, and emits `ao2.pulse-real-execute-containment.v1` at
  `target/pulse-real-execute-containment/latest/summary.json`
- `npm run phase1:promotion-golden`: runs the signed Phase 1 operator golden
  smoke, records readback/dashboard evidence, scans logs/artifacts for bearer
  token leaks, preserves the `AO2_PHASE1_API_TOKEN_ENV` boundary, and emits
  `ao2.phase1-promotion-golden-path.v1` at
  `target/phase1-promotion-golden/latest/summary.json`
- `npm run release:evidence-closure`: runs CI artifact download, local canary,
  the Risky PR golden path, Phase 1 promotion golden evidence, Pulse execute
  safety, bounded real Pulse execute, control-plane negative restore evidence,
  artifact index, and fail-on-attention artifact health, then writes
  `ao2.release-evidence-closure.v1` at
  `target/release-evidence-closure/latest/summary.json` plus
  `target/release-evidence-closure/latest/closure.html`. The closure rejects
  release acceptance when Risky PR digest-boundary evidence is missing from the
  static report `approval_boundary`, when denied request or approved action
  digest summaries are absent, when replay has digest failures, or when the
  operator-visible report cannot prove test evidence, replay status, and
  closure verdict before release closure. The release packaging regression
  suite also covers `AO2_RELEASE_EVIDENCE_CLOSURE_FIXTURE=missing_digest_boundary`,
  which removes the generated `approval_boundary` before validation and proves
  the release closure path stays fail-closed for corrupted digest evidence.
- `npm run mvp:acceptance-matrix-gate`: runs the provider-free Risky PR golden
  path and maps PRD `AC-01` through `AC-12` plus SDD `UAT-01` through
  `UAT-12` to concrete evidence without manual filesystem archaeology; emits
  `ao2.mvp-acceptance-matrix-gate.v1` at
  `target/mvp-acceptance-matrix/latest/summary.json`
- `npm run workbench:no-archaeology-audit`: generates cockpit and workbench
  evidence for a Risky PR run, then proves the operator can answer objective,
  denied action, approved digest, changed files, test evidence, rejection
  reason, correction, closure verdict, export path, and replay status from
  evidence surfaces alone. It also verifies run-record/report/evaluator-closure
  links, including `Local Run Record`, `Static Export Evidence`,
  `Evaluator Closure Evidence`, and `Replay Evidence` report sections; emits
  `ao2.no-archaeology-workbench-audit.v1` at
  `target/no-archaeology-workbench/latest/summary.json`
- `npm run control-plane:observer-hardening`: composes signed evidence-pack
  ingest/readback, synthetic signed operator-packet readback, a real Workbench
  operator-packet control-plane smoke, negative restore drill, long-lived
  control-plane smoke, artifact index, and fail-on-attention artifact health
  while verifying the control plane remains a read-only observer; emits
  `ao2.control-plane-observer-hardening.v1` at
  `target/control-plane-observer-hardening/latest/summary.json`
- `npm run smoke:workbench-operator-packet-control-plane`: runs a local
  governed fixture, serves the Workbench, posts `kind=operator-packet` to
  `/api/runs/evidence/publish`, publishes the signed
  `ao2.operator-evidence-packet.v1` to ao2-control-plane, reads back dashboard,
  detail, latest, raw packet, and signature endpoints, and emits the
  workbench operator-packet control-plane smoke summary
  `ao2.workbench-operator-packet-control-plane-smoke.v1` under
  `target/workbench-operator-packet-control-plane-smoke/`. CI also runs this
  as the Ubuntu/macOS/Windows `Workbench operator packet control-plane smoke`
  matrix job with `ao2-control-plane` checked out as a sibling repository and
  uploads one smoke evidence artifact per OS.
- `npm run smoke:workbench-operator-packet-control-plane:index`: validates the
  downloaded Ubuntu/macOS/Windows smoke artifacts, requires each OS to have an
  `ao2.workbench-operator-packet-control-plane-smoke.v1` summary, fails on
  missing OS coverage, `token_leak_detected=true`, non-accepted evaluator
  closure or replay status, or missing provider-score evidence, and emits
  `ao2.workbench-operator-packet-control-plane-smoke-index.v1` under
  `target/workbench-operator-packet-control-plane-smoke-index/latest/`. CI runs
  this after the smoke matrix and uploads the index artifact.
- `npm run provider:phase2-contract-hardening`: verifies Codex and Claude
  provider contracts, replacement parity, no-factory-v3 guardrails, transcript
  parsing, sandbox patch digest boundaries, exact approval enforcement,
  blocker taxonomy, and fail-closed live guards; emits
  `ao2.provider-phase2-contract-hardening.v1` at
  `target/provider-phase2-contract-hardening/latest/summary.json`
- `npm run release:train-drill`: rehearses release evidence closure, the
  release readiness static summary, release readiness regression, retention preflight
  with pruning disabled, artifact consumer dry-run, and post-merge canary
  without tag, push, publish, or deploy side effects. The release readiness
  static summary must include the
  `ci_release_readiness_artifact_consumer_job` proof and the
  `ci_dual_repo_release_publication_closure_index_job` proof plus the
  `artifact-closure-index.json` /
  `ao2.release-artifact-closure-index.v1` required artifact list before the
  drill accepts.
  It records install/update verification as a `release:download-verify`
  reference for real release assets and emits
  `ao2.public-release-train-drill.v1` at
  `target/public-release-train-drill/latest/summary.json`
- `npm run release:train-control-plane-bridge`: extends the release train drill
  into ao2-control-plane readback by materializing
  `AO2_CP_RELEASE_TRAIN_SUMMARY`, checking `/api/v1/release/train(.json)`, and
  preserving the read-only observer trust boundary; emits
  `ao2.release-train-control-plane-bridge.v1` at
  `target/release-train-control-plane-bridge/latest/summary.json`; CI uploads
  this as `ao2-release-train-control-plane-bridge`
- `npm run next:lengthy:gate`: runs the five lengthy-task gates above and emits
  `ao2.next-lengthy-gate.v1` at
  `target/next-lengthy-gate/latest/summary.json`
- `npm run control-plane:cross-repo-observer`: runs a signed AO2 evidence
  bundle through ao2-control-plane ingest/readback, verifies the public
  control-plane observer helper scripts, runs the restore drill, and preserves
  the read-only observer boundary; emits
  `ao2.cross-repo-control-plane-observer.v1` at
  `target/cross-repo-control-plane-observer/latest/summary.json`
- `npm run release:install-update-fixture`: builds a local signed fixture
  archive with `SHA256SUMS`, `provenance.json`, and a signature sidecar,
  verifies checksum/install/update behavior, references `release:download-verify`
  for real release assets, and emits `ao2.release-install-update-fixture.v1`
  at `target/release-install-update-fixture/latest/summary.json`
- `npm run workbench:browser-qa`: runs the no-archaeology workbench audit,
  statically inspects the generated HTML review surface, records a screenshot
  manifest for browser-review evidence, and emits `ao2.workbench-browser-qa.v1`
  at `target/workbench-browser-qa/latest/summary.json`
- `npm run provider:adversarial-corpus`: runs the provider Phase 2 hardening
  gate plus focused transcript parser tests against
  `fixtures/provider-adversarial-corpus`, covering malformed transcript,
  approval boundary, patch digest mismatch, blocker taxonomy, and fail-closed
  cases; emits `ao2.provider-adversarial-corpus.v1` at
  `target/provider-adversarial-corpus/latest/summary.json`
- `npm run release:dr-retention-snapshot`: composes the control-plane restore
  drill, retention preflight, artifact index, and artifact health into a
  fixture snapshot manifest; emits `ao2.dr-retention-long-run-snapshot.v1` at
  `target/dr-retention-long-run-snapshot/latest/summary.json`
- `npm run frontier:lengthy:gate`: runs the five frontier lengthy gates above
  and emits `ao2.frontier-lengthy-gate.v1` at
  `target/frontier-lengthy-gate/latest/summary.json`
- `npm run post-merge:canary`: runs artifact indexing, release artifact
  consumer dry-run, public release download checksum verification,
  cross-repo release asset completeness, Pulse resume dry-run, and the
  ao2-control-plane long-lived smoke into
  `ao2.post-merge-canary.v1`
- `npm run release:asset-completeness`: queries the AO2 and ao2-control-plane
  stable public releases, requires the expected release assets, downloads each
  `SHA256SUMS`, and emits `ao2.release-asset-completeness.v1` plus
  `dashboard.html` showing stable-vs-prerelease release state
- `npm run release:asset-publication-readiness`: composes cross-OS attestation
  and public ship dry-run evidence, using a local `release-artifact-fixture`
  with `ao2-python-guard` / `ao2.python-guard-ci-artifacts.v1` so publication
  readiness stays local-first instead of depending on live GitHub artifact
  downloads
- `npm run release:stable-readiness`: consumes the release asset completeness
  report, records prerelease-only and signed-provenance blockers, and emits
  `ao2.stable-release-readiness.v1` plus `dashboard.html` for stable promotion
  review
- `npm run release:stable-promotion-workflow`: reruns stable readiness, emits
  `ao2.stable-promotion-workflow.v1`, and stays in dry-run mode unless
  `AO2_STABLE_PROMOTION_CONFIRM=promote-stable-<ao2-tag>-<control-plane-tag>`
  is set before flipping the AO2 and ao2-control-plane GitHub Releases from
  prerelease to stable. Confirmed promotion is also blocked by a
  post-release verification evidence gate: the workflow downloads the latest
  successful AO2 `Post Stable Release Verification` artifacts and
  ao2-control-plane `Post Release Verification` artifacts, validates
  signatures/checksum summaries, and emits
  `ao2.stable-promotion-evidence-gate.v1` with
  `post-release verification evidence gate` status before any release mutation
  is attempted
- `npm run release:operator-evidence-bundle`: downloads the operator-facing
  release evidence set into one local folder and emits
  `ao2.operator-release-evidence-bundle.v1`. It verifies the AO2
  `ao2-dual-repo-release-publication-closure-index`, AO2 Linux/macOS/Windows
  post-stable install/update evidence, and ao2-control-plane
  Ubuntu/macOS/Windows post-release summaries with checksum and trust-boundary
  checks. Use `AO2_OPERATOR_RELEASE_EVIDENCE_FIXTURE_DIR=<path>` for offline
  fixture verification. The scheduled/manual `Operator Release Evidence Audit`
  workflow runs the same command on GitHub Actions and uploads
  `ao2-operator-release-evidence-bundle` so operators can download
  `summary.json` and point `AO2_CP_OPERATOR_RELEASE_EVIDENCE_SUMMARY` at it for
  read-only control-plane dashboard readback.
- `npm run release:immutability-audit`: composes asset completeness, stable
  readiness, full release download verification, checksum validation, signed
  provenance verification, GitHub asset digest checks, and release metadata
  coherence into `ao2.release-immutability-audit.v1` at
  `target/release-immutability-audit/latest/summary.json`
- `npm run release:sync-provenance-assets`: queries the configured AO2 GitHub
  Release and local `dist-provenance` sidecars, emits
  `ao2.release-sync-provenance-assets.v1`, and stays in dry-run mode unless
  `AO2_RELEASE_SYNC_CONFIRM=sync-<tag>` is set before uploading provenance
  sidecars with `gh release upload`
- `npm run release:publication-dry-run-closure`: composes release asset
  publication readiness, provenance sync dry-run, and stable readiness into a
  release publication dry-run closure at
  `target/release-publication-dry-run-closure/latest/summary.json` using
  `ao2.release-publication-dry-run-closure.v1`; it records
  `publication_ready`, `stable_release_ready`, dry-run upload status, and
  explicit `release_publish=not executed` guards without mutating GitHub
  Releases
- `npm run phase1:promote`: prepares Phase 1 prerequisites, runs promotion
  preflights, publishes to ao2-control-plane when
  `AO2_PHASE1_CONTROL_PLANE_URL` is set, and may capture a dashboard snapshot
  with `AO2_PHASE1_DASHBOARD_SNAPSHOT=1`
- stable public release archives at v0.4.80 (macOS aarch64, Linux aarch64,
  Linux x86_64, Windows x86_64) are SHA256 verified from the published
  `SHA256SUMS`; signed provenance sidecars are required before stable-promotion
  readiness can pass
- `.github/workflows/post-stable-release-verification.yml` runs a hosted
  consumer smoke for the stable public release on Ubuntu, macOS, and Windows:
  download the published archive plus signed provenance sidecars, verify
  `SHA256SUMS`, install via `ao2 install update --provenance-dir` into a
  temporary bin directory, require `signature_verified` install-update evidence,
  then run `ao2 version --json`, `ao2 doctor --json`, and
  `ao2 adapter doctor --provider scripted`
- `npm run release:cross-os-attestation` emits
  `ao2.cross-os-release-attestation.v1` at
  `target/cross-os-release-artifact-attestation/latest/summary.json`; by
  default it runs CI-safe required checks and records native three-OS smoke plus
  public release download verification as optional evidence. Set
  `AO2_CROSS_OS_ATTESTATION_ENABLE_THREE_OS=1` or
  `AO2_CROSS_OS_ATTESTATION_ENABLE_DOWNLOAD=1` to execute those optional lanes.
  Set `AO2_CROSS_OS_ATTESTATION_REQUIRE_NATIVE=1` or
  `AO2_CROSS_OS_ATTESTATION_REQUIRE_DOWNLOAD=1` to make missing optional proof a
  blocking failure.

## Phase 1 Promotion Token Boundary

Before a local Phase 1 promotion, point AO2 at the self-hosted control plane and
name the environment variable containing the local bearer token:

```sh
export AO2_PHASE1_CONTROL_PLANE_URL=http://127.0.0.1:3000
export AO2_PHASE1_API_TOKEN_ENV=AO2_CP_API_TOKEN
export AO2_CP_API_TOKEN=<redacted-local-token>
npm run phase1:prepare-prerequisites
npm run phase1:promote
```

AO2 passes the token as `--api-token-env AO2_CP_API_TOKEN`; do not put bearer
token values in command-line arguments, URLs, tracked docs, or generated
evidence. To include the read-only observer dashboard in the same local
promotion evidence, run:

```sh
AO2_PHASE1_DASHBOARD_SNAPSHOT=1 npm run phase1:promote
npm run phase1:dashboard-snapshot
```

Workspace test coverage:

- provider-free risky PR run rejects once, corrects, then accepts;
- interactive approval pause/resume persists pending and approved tickets;
- replay reconstructs status from local events and fails on digest mismatch;
- CLI pause/approve/resume/replay path works end to end;
- CLI `version --json` reports package, version, target, git commit, build
  profile, and release schema compatibility;
- CLI `install update` verifies archive checksum, verifies detached signature,
  validates `RELEASE-MANIFEST.json`, and installs the target binary;
- CLI `init` writes provider presets under `.ao2/provider-profiles.json`;
- CLI `provider list` and `provider doctor` expose provider fast-start checks;
- CLI `run --template <name>` materializes embedded templates under
  `.ao2/generated-workflows/` and executes them without requiring manual YAML
  paths;
- local CLI adapter captures transcript and blocker metadata;
- sandbox adapter execution captures changed files and diff summary without
  mutating the target repository;
- CLI adapter sandbox run reports diff without mutating target repository;
- sandbox patch preview computes an exact action digest;
- sandbox patch apply rejects mismatched digests and promotes files only after
  exact digest approval;
- provider prompt profiles build sandbox-only Codex and Claude invocations;
- scripted provider prompt runs in sandbox without mutating target repository;
- provider transcript parsing extracts changed files, concerns, blockers, token
  usage, optional cost, and provider summaries;
- provider transcript parsing ignores prompt-template labels in the adapter
  command section and only parses provider output when stdout/stderr sections
  are present;
- CLI provider prompt run works through `ao2 adapter prompt`;
- provider-backed risky-run implementer uses provider prompt sandbox execution,
  sandbox patch preview, and exact-digest sandbox patch apply;
- provider-backed risky-run evidence includes `provider_transcript_summary`
  artifacts and embedded `provider_summaries` in `evidence-pack.json`;
- CLI evidence cockpit renders provider summaries, policy decisions, approvals,
  artifacts, replay integrity, run markers, and closure verdict for an existing
  run;
- CLI local workbench renders run history, provider health, task templates, and
  signed upgrade commands for a repository;
- CLI `provider contract --provider codex --json` reports schema
  `ao2.provider-contract.v1` with `phase_1`, `same_contract_as=scripted`,
  sandbox/digest execution boundary, exact-digest side-effect boundary, live
  guard env, prompt command, transcript fields, policy invariants, and evidence
  contract for the Codex CLI adapter;
- CLI `provider contract --verify --require codex --json` reports schema
  `ao2.provider-contract-verification.v1`, returns `status=verified` for the
  Codex Phase 1 contract, and fails closed with JSON `status=failed` and
  reason code `unknown_provider` for unknown required providers;
- release archive smoke runs `provider contract --verify --require codex
  --json` through the installed packaged binary and requires
  `provider_contract_verify=passed`;
- CLI local workbench renders a read-only `Provider Contracts` table showing
  scripted, Codex, and Claude adapter phases, shared boundary, live guards, and
  prompt command shape;
- CLI local workbench renders `Provider Contract Verification`, and served
  Workbench exposes viewer-token protected `/api/provider-contracts` with
  schema `ao2.provider-contract-verification.v1`;
- CLI local workbench can be served over a local HTTP listener for
  cross-platform operator use and smoke checks;
- CLI local workbench exposes token-protected `/api/runs`, `/api/templates`,
  and `/api/doctor` endpoints when served locally;
- CLI local workbench exposes viewer-token protected `/api/runs/evidence`,
  composes replay status, digest counts, provider scorecard summary, provider
  transcript summaries, closure verdicts, optional obligation ledger verdicts,
  run-record/report/evaluator-closure links, and cockpit/evidence links, and
  renders a Run Evidence Summary control in the Workbench UI;
- CLI local workbench exposes viewer-token protected
  `/api/runs/evidence/diff`, compares two run evidence summaries for
  status/verdict changes, digest failure delta, provider summary delta, score
  delta, closure verdict changes, and evidence links, and renders a Run
  Evidence Diff control in the Workbench UI;
- CLI local workbench exposes viewer-token protected
  `/api/runs/evidence/changes`, compares a selected run to the previous local
  run through the same diff contract, rejects runs without a previous baseline,
  and renders a Changed Since Previous control in the Workbench UI;
- CLI local workbench exposes operator-token protected
  `/api/runs/evidence/export`, writes summary, diff, changed-evidence, or
  operator evidence packet JSON support handoff artifacts under
  `.ao2/workbench/evidence-exports/`, rejects viewer tokens, and renders
  Export Summary / Export Diff / Export Changes controls in the Workbench UI.
  Operator packets use `ao2.operator-evidence-packet.v1` and bundle the local
  run record, static report HTML, evidence pack, evaluator closure verdict,
  replay status, and provider scorecard for signed support-bundle readback;
- CLI local workbench exposes operator-token protected
  `/api/runs/evidence/publish`, publishes either a signed evidence pack
  (`kind=evidence-pack`, default) or a real run-derived signed operator packet
  (`kind=operator-packet`) to ao2-control-plane with the server-side
  `--support-signing-key`; the control plane remains a read-only observer and
  receives the operator packet through `/api/v1/operator-packet/signed`;
- CLI local workbench exposes operator-token protected
  `/api/obligations/annotate`, records manual path/line evidence or explicit
  waivers into a run's sidecar obligation ledger, appends an audited
  `obligation_annotate` Workbench event, writes a
  `kind=obligation-annotation` evidence export, signs that export when
  `--support-signing-key` is enabled, rejects viewer tokens, and renders an
  Obligation Annotation control in the Run Evidence section;
- CLI local workbench exposes operator-token protected
  `/api/obligations/gate`, re-checks a run's sidecar obligation ledger against
  the target repository at a named stage, writes
  `obligation-gate-<stage>.json` next to the ledger, appends an audited
  `obligation_gate` Workbench event, writes a `kind=obligation-gate` evidence
  export, signs that export when `--support-signing-key` is enabled, rejects
  viewer tokens, and renders Midpoint Gate / Closure Gate controls in the Run
  Evidence section;
- CLI local workbench run evidence summaries expose recent
  `obligation-gate-*.json` sidecars, and evidence-pack publish enriches signed
  uploads with sibling `obligation_gates` metadata before posting to
  ao2-control-plane;
- CLI local workbench exposes a token-protected `/api/launch` endpoint that
  validates template/provider choices and returns an explicit governed run
  command preview without browser-triggered execution;
- CLI local workbench rejects `/api/queue/start` unless started with
  `--enable-execution`;
- CLI local workbench execution queue can run a scripted governed task,
  refresh status through `/api/queue`, and report evidence-pack/cockpit paths;
- CLI local workbench persists queue history to `.ao2/workbench/queue.json`
  and restores failed job history after server restart;
- CLI local workbench can cancel a running queued job through
  `/api/queue/cancel`;
- CLI local workbench can retry a failed queued job through `/api/queue/retry`
  and renders cancel/retry/open-evidence controls;
- CLI local workbench exposes `/api/queue/job` with persisted stdout/stderr
  logs for a single queued job and renders a Details control;
- CLI local workbench exposes `/api/queue/job/logs` with bounded live
  stdout/stderr tails while jobs are running and renders an inline Logs
  control that refreshes the selected job;
- CLI local workbench exposes token-protected `/queue/job` HTML detail pages
  with stdout/stderr logs, evidence links, and queue timing/exit metrics;
- CLI local workbench persists queue timing fields, child exit code, and retry
  count for accepted jobs;
- CLI local workbench appends cancel/retry queue operator actions to
  `.ao2/workbench/audit.jsonl`;
- CLI local workbench exposes token-protected `/api/queue/audit` with action
  and job filters and renders the queue audit panel in the workbench UI;
- CLI local workbench exposes token-protected `/api/queue/export` and writes a
  local support bundle containing queue state, audit events, and job logs;
- CLI local workbench support bundles attach existing Workbench evidence
  exports from `.ao2/workbench/evidence-exports/`, including export kind,
  SHA256, generated timestamp, JSON content, and operator-packet run/closure/
  replay/provider score summaries;
- signed Workbench support-bundle metadata records `evidence_export_count`, and
  support-bundle verification checks that signed count against the bundle body;
- CLI local workbench exposes viewer-token protected `/api/support/latest`,
  verifies the newest local Workbench support bundle before returning
  `ao2.workbench-support-latest.v1`, and renders a `Latest Support Packet`
  panel with queue/audit/log/evidence counts, signed trust metadata, bundle
  link, and attached evidence export rows;
- CLI local workbench support verify, inspect, and import expose concise
  summaries for attached evidence exports, and imported support-case HTML
  renders an `Evidence Exports` table for offline operators;
- CLI local workbench exposes operator-only `/api/provider-smoke` behind
  explicit `--enable-execution`, runs the deterministic provider smoke loop,
  and persists `.ao2/provider-smoke/history.json`;
- CLI local workbench exposes operator-only `/api/provider-pilot/start` behind
  explicit `--enable-execution`, rechecks provider readiness before queueing,
  returns the blocked provider pilot report when the gate is not ready, and
  queues ready provider pilot runs through the persistent Workbench queue;
- CLI local workbench exposes operator-only `/api/provider-pilot/preflight`
  without requiring execution mode, validates provider pilot local inputs,
  reads provider readiness history, and returns a structured preflight report
  before queueing;
- CLI local workbench supports multiple operator tokens with `viewer`,
  `operator`, and `admin` roles;
- CLI local workbench allows viewer tokens to read runs, queue state, job
  details, and audit events while rejecting mutating API calls with
  `insufficient_operator_role`;
- CLI local workbench rejects invalid `--operator-token` role configuration
  before binding a server;
- CLI local workbench supports queue filtering by status/template;
- CLI local workbench enforces configurable persisted queue history retention
  with `--queue-retention`;
- CLI `control-plane ingest --json` writes a read-only
  `.ao2/control-plane/snapshot.json` containing run summaries, evidence-pack
  paths, queue jobs, audit events, and provider smoke history;
- CLI `control-plane export` writes a read-only static control-plane dashboard
  from `.ao2/control-plane/snapshot.json`;
- CLI `control-plane serve` serves a token-protected local control-plane
  dashboard and `GET /api/control-plane/snapshot`;
- CLI `control-plane index` combines multiple repository snapshots into a
  read-only `ao2.control-plane-fleet-snapshot.v1` aggregate;
- CLI `control-plane refresh` regenerates target snapshots and writes the
  read-only fleet aggregate in one command;
- CLI `control-plane sources save` writes reusable fleet target lists and
  `control-plane refresh --sources` refreshes from that list;
- CLI `control-plane refresh --history` records fleet snapshot history with
  snapshot paths, checksums, and aggregate counts;
- CLI `control-plane history diff` compares retained fleet snapshots and
  reports repository/run deltas plus added and removed run IDs;
- CLI `control-plane history prune` removes old retained fleet snapshots and
  rewrites `history.json` with the newest entries;
- CLI `control-plane history export` writes a static `AO2 Fleet History`
  dashboard with counts, checksums, snapshot paths, and run IDs;
- CLI `control-plane health` reports fleet alerts with schema
  `ao2.control-plane-health.v1` for unhealthy runs, queue jobs, evidence packs,
  empty fleets, empty history, missing provider smoke history, and provider
  smoke readiness;
- CLI `control-plane health --record` writes local health history with schema
  `ao2.control-plane-health-history.v1`, immutable health JSON files, and
  SHA256 entries;
- CLI `control-plane health-trend` reports latest/previous alert counts,
  alert-count delta, and trend state with schema
  `ao2.control-plane-health-trend.v1`;
- CLI `control-plane health-prune` removes older retained health JSON files
  and rewrites `health-history.json` with the newest entries;
- CLI `control-plane health-export` writes a static
  `AO2 Fleet Health Trend` dashboard with schema
  `ao2.control-plane-health-export.v1`;
- CLI `control-plane export --fleet` writes a read-only fleet dashboard with
  repository, run, queue, audit, evidence-pack totals, and a fleet health
  alert panel plus provider readiness rollup, and includes `Fleet Health Trend`
  when `--health-history` is supplied;
- CLI fleet dashboards include local text and status filters without mutating
  AO2 state;
- CLI `control-plane serve --fleet` serves the token-protected fleet dashboard
  and returns the fleet snapshot from `GET /api/control-plane/snapshot` plus
  fleet alerts from `GET /api/control-plane/health` and, when configured,
  trend data from `GET /api/control-plane/health-trend`;
- CLI `control-plane bundle` writes a portable fleet bundle, checksum manifest,
  and tar.gz archive for support handoff, and can include health history,
  health entries, trend JSON, and trend HTML with `--health-history`;
- CLI `control-plane bundle --signing-key` writes support metadata, derives a
  public key, signs the metadata with RSA/SHA-256, and includes metadata,
  signature, and public key files in `SHA256SUMS`;
- CLI `control-plane bundle-verify` checks fleet bundle schema and all
  `SHA256SUMS` entries and reports signed support metadata verification when
  present. Signed support metadata is fail-closed: the metadata, signature, and
  public key files must all be present, and RSA/SHA-256 verification must pass
  even if `SHA256SUMS` has been refreshed after metadata tampering;
- CLI `control-plane bundle-inspect` verifies a transferred archive or
  extracted bundle directory and prints a read-only support summary without
  writing an import case. Text output includes signed support metadata trust
  status and signer id when signed metadata is present;
- CLI `control-plane bundle-import` verifies a transferred archive or extracted
  bundle directory before creating a permanent offline support case, writes an
  `import-summary.json`, keeps verified bundle files under `bundle/`, and
  renders a static support `index.html` with a `Support Bundle Trust` section
  for signed metadata status, signer id, metadata SHA256, and public-key
  SHA256;
- release archives include `install.sh`, `install.ps1`, `verify-release.sh`,
  `Verify-Release.ps1`, and bundled checksum coverage for binary, installer,
  verifier, manifest, version, and README payload files;
- release archives include `RELEASE-MANIFEST.json` with package, target,
  binary path, and packaged binary checksum metadata;
- release archives include `RELEASE-VERIFICATION.json` with schema
  `ao2.release-archive-offline-verification.v1`, no provider API-key
  requirement, evaluator-closer release ownership, and no control-plane release
  approval or AO2 artifact mutation authority;
- `ao2 install update` enforces the offline release verification report and
  `SHA256SUMS` coverage before it creates or mutates the install directory, and
  reports `offline_verification.status = "verified"` on success;
- `ao2 install update` writes `<binary>.install-verification.json` with schema
  `ao2.install-verification-evidence.v1`, `ao2 doctor --json` reports it under
  `install.verification_evidence`, and release evidence bundles require it as
  a checksum-covered artifact with verified offline status and read-only
  control-plane trust-boundary fields;
- direct archive installers write the same
  `<binary>.install-verification.json` sidecar after packaged-binary checksum
  verification, and release archive smoke can emit
  `ao2.release-archive-smoke.v1` JSON that points to sidecar evidence for each
  exercised OS leg;
- Unix installer verifies the packaged binary checksum and installs to a
  user-writable `AO2_INSTALL_DIR` without admin access;
- release install smoke installs the macOS archive and runs a scripted
  real-project repair from the installed binary;
- release install smoke installs the Linux/aarch64 archive in Ubuntu Docker and
  runs a scripted real-project repair from the installed binary;
- release install smoke validates the Windows archive contains `install.ps1`,
  `bin/ao2.exe`, and a matching binary checksum;
- three-OS smoke records optional native Windows SSH execution as
  `windows_native_smoke=passed` or `windows_native_smoke=skipped` while still
  requiring macOS install smoke, Ubuntu Docker install smoke, release
  provenance, and Windows archive/static validation to pass;
- setting `AO2_REQUIRE_NATIVE_WINDOWS_SMOKE=1` makes native Windows execution a
  strict gate; when reachable, it runs installed `ao2.exe` on
  `antho@10.0.0.96`, performs a scripted repair, and replays with zero digest
  failures;
- real-project templates exist for bug fixes, small refactors, dependency
  upgrades, and test generation;
- installed CLI can list and print embedded task templates;
- workflow metadata from template files drives run workflow id, objective, and
  verifier command;
- provider-backed real-project templates use a generic verifier-first closure
  path instead of discount-service fixture assumptions;
- provider-backed real-project templates can rerun the provider prompt after a
  verifier failure and accept when a repair attempt passes the verifier;
- sandbox copying and snapshots ignore VCS, dependency, virtualenv, build,
  coverage, and framework cache directories;
- real-repo pilot runs for `bug-fix` and `test-generation` accepted on
  disposable copies of `/tmp/ao2-public/secure-agent-profile`;
- both real-repo pilot runs generated evidence cockpits and replayed with zero
  digest failures;
- authenticated Codex and Claude real-repo pilot runs accepted on disposable
  copies of `/tmp/ao2-public/secure-agent-profile`;
- authenticated real-repo provider summaries parsed changed files and provider
  summaries for Codex and Claude;
- Node real-project pilots accepted with `npm test`, `npm run typecheck`, and
  `npm test --workspace @ao2/node-pilot` verifier commands;
- evidence packs include the workflow verifier command for cross-language pilot
  auditability;
- provider smoke history persists with schema `ao2.provider-smoke-history.v1`
  and control-plane health emits `ao2.provider-readiness-rollup.v1`;
- real-project repair attempts record a `repair_prompt` artifact before
  rerunning the provider;
- structured repair prompts include the failing verifier output and prior
  provider transcript summaries;
- structured repair context validated on disposable Node and Python
  real-project pilots;
- provider-authenticated structured repair UAT accepted with Codex and Claude;
- provider summaries remained parseable after Codex and Claude structured
  repair prompt use;
- provider-backed runs record repair attempts after reviewer rejection;
- zero repair budget stops as rejected with replayable evidence and
  `repair_budget_exhausted` markers;
- verifier failure during repair records a failed attempt and retries until the
  budget accepts or exhausts;
- evidence cockpit renders recorded repair attempts;
- CLI risky-run provider flags work through `ao2 run --provider ...`;
- adapter doctor reports scripted provider without an external binary;
- risky-run evidence includes an `adapter_transcript` artifact and
  `adapter.completed` event;
- policy requires approval for git push, delete, package install, network
  egress, parent traversal, broad writes, and raw secret access;
- forbidden provider API key preflight fails closed;
- canonical schema files exist and parse as JSON Schema documents;
- risky PR example declares required event and evidence-pack contract.

## CLI Smoke Test: One-Shot Run

```sh
tmpdir=$(mktemp -d /tmp/ao2-cli-demo.XXXXXX)
cp -R fixtures/discount-service "$tmpdir/discount-service"
/usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  cargo run -p ao2-cli --bin ao2 -- \
  run examples/risky-pr-run/risky-pr.yaml \
  --target "$tmpdir/discount-service" \
  --run-id demo-run
/usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  cargo run -p ao2-cli --bin ao2 -- \
  status demo-run --target "$tmpdir/discount-service"
/usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  cargo run -p ao2-cli --bin ao2 -- \
  export demo-run --target "$tmpdir/discount-service"
/usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  cargo run -p ao2-cli --bin ao2 -- \
  report demo-run --target "$tmpdir/discount-service"
```

Result:

- final run status: `accepted`;
- exported evidence pack: `.ao2/runs/demo-run/evidence-pack/evidence-pack.json`;
- static report: `.ao2/runs/demo-run/report/index.html`;
- evidence cockpit: `.ao2/runs/demo-run/cockpit/index.html`;
- event log: `.ao2/runs/demo-run/events.jsonl`;
- run record: `.ao2/runs/demo-run/run-record.json`.

## CLI Smoke Test: Interactive Approval And Replay

```sh
tmpdir=$(mktemp -d /tmp/ao2-interactive-demo.XXXXXX)
cp -R fixtures/discount-service "$tmpdir/discount-service"
/usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  target/release/ao2 run examples/risky-pr-run/risky-pr.yaml \
  --target "$tmpdir/discount-service" \
  --run-id release-demo \
  --pause-for-approval
ticket=<approval-ticket-id>
/usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  target/release/ao2 approve "$ticket" \
  --target "$tmpdir/discount-service" \
  --approver human:release-smoke
/usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  target/release/ao2 run --resume release-demo \
  --target "$tmpdir/discount-service"
/usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  target/release/ao2 replay release-demo \
  --target "$tmpdir/discount-service"
```

Result:

- paused status: `waiting_for_approval`;
- approval status: `approved`;
- resumed status: `accepted`;
- replay status: `accepted`;
- replay event count: `24`;
- replay artifact count: `9`;
- digest failures: none.
- adapter transcript artifact: present.

## Release Support Bundle Assembly

`ao2 release support-bundle-build` assembles the public
`ao2.cp-release-support-bundle.v1` contract from explicit release assembly,
readiness, handoff, cockpit, evaluator decision, storage support, replay, and
operator evidence JSON files, plus required install-verification evidence using
schema `ao2.install-verification-evidence.v1` and hosted release archive smoke
evidence using schema `ao2.release-archive-hosted-smoke.v1`. The build can
either generate the embedded `ao2.report-contract-verification.v1` by running
the same report-contract verifier from `--report-target` and `--report-run-id`
inputs, or accept a precomputed `--report-contract-verification` JSON file. It
writes `release-support-bundle.json` and `SHA256SUMS`, then verifies the
generated bundle before returning success. The emitted bundle includes the
control-plane portable manifest, `ci_evidence_index`, canonical per-surface
digests, and a
canonical bundle digest in `SHA256SUMS`; it is directly verifiable with
ao2-control-plane's offline verifier:

```sh
python3 ../ao2-control-plane/scripts/verify_release_support_bundle.py \
  --json \
  --checksums /path/to/release-support-bundle/SHA256SUMS \
  /path/to/release-support-bundle/release-support-bundle.json
```

The strict verifier fails closed when the static report contract is missing,
incomplete, or failed, when install verification is missing, trust-unsafe, or
not offline-verified, when hosted release archive smoke is missing, failed, or
trust-unsafe, or when candidate-correlation triage is absent or inconsistent
across the release assembly, readiness, handoff, and cockpit surfaces. This
proves the operator-facing HTML, install evidence, hosted CI install smoke, and
control-plane handoff contract remained inspectable before release review.

Regression coverage:

```sh
cargo test -p ao2-cli --test release_support_bundle_verification
```

The same test target runs in CI release-readiness shards on Ubuntu, macOS, and
Windows, so bundle assembly and strict support-bundle verification stay covered
across the supported release platforms.

## Archive-Heavy Test Resource Guard

Archive-producing tests can temporarily consume multiple gigabytes while Cargo
build artifacts, release archives, extracted payloads, and system temp
directories coexist. Run the guarded local path before release packaging work:

```sh
npm run test:archive-resources
npm run test:archive-heavy
```

The guard writes `target/archive-heavy-test-resources/latest.json` with schema
`ao2.archive-heavy-test-resource-guard.v1`, checks free space for the repo,
Cargo target directory, and system temp directory, and prunes only its own stale
guard evidence. Tune it with `AO2_ARCHIVE_TEST_MIN_FREE_GB`,
`AO2_ARCHIVE_TEST_STALE_HOURS`, and
`AO2_ARCHIVE_TEST_RESOURCE_GUARD_DIR`.

Hosted release packaging CI runs `npm run test:archive-resources` before
`release_packaging` and executes archive-heavy shards with `--test-threads=1`
to avoid parallel archive extraction and packaging pressure.

## Adapter Doctor Smoke Test

```sh
target/release/ao2 adapter doctor --provider scripted
target/release/ao2 adapter doctor --provider codex
target/release/ao2 adapter doctor --provider claude
target/release/ao2 provider matrix --json
```

Expected result:

- `scripted` reports `available: true` with a built-in version;
- `codex` reports the locally installed Codex CLI version when present;
- `claude` reports the locally installed Claude Code CLI version when present.
- `provider matrix --json` reports schema
  `ao2.provider-readiness-matrix.v1`, the 900-second default provider timeout,
  sandbox/digest patch boundaries, transcript summary fields, and policy
  invariants for scripted, Codex, and Claude.

## Local CI Workaround

```sh
npm run ci:local
```

This is the active gate while hosted private runners are unavailable. It runs
formatting, tests, clippy, release build, interactive approval/resume, replay,
and evidence digest checks locally.

For the full local/self-hosted verification policy, including strict
macOS-orchestrated three-platform release proof with direct Windows SSH, see
`docs/LOCAL-SELF-HOSTED-VERIFICATION.md`.

## GitHub CI Status

Public repository:

```text
https://github.com/uesugitorachiyo/ao2
```

The CI workflow at `.github/workflows/ci.yml` runs on pull request and `main`
push, and can also be dispatched manually. Release workflows stay explicit
operator gates: `.github/workflows/release-gate.yml` and
`.github/workflows/public-release-build.yml` use `workflow_dispatch` only.
Local and self-hosted verification remain valid pre-release evidence, but public
hosted CI is the default regression guard for ordinary changes.

The public hardening workflow at `.github/workflows/ao2-public-hardening.yml`
runs on pull request and manual dispatch only. It uses read-only repository
permissions, seeds a local Pulse packet with `pulse:generate-next`, mirrors that
packet with `pulse:local-mirror`, then runs the public stabilization tests,
`public:hardening`, and `pulse:resume -- --dry-run`. The local parity commands
above emit `ao2.public-hardening-ci-workflow.v1`,
`ao2.public-hardening-workflow-file-dry-run.v1`,
`ao2.public-hardening-workflow-tracked-proposal.v1`, and
`ao2.public-hardening-ci-local-runner-parity.v1` under `target/`.

## Current Production Readiness Boundary

The local governed delivery slice is operational with interactive approval,
replay, policy hardening tests, local adapter contract, adapter transcript
artifacts, isolated sandbox execution, exact-digest sandbox patch apply,
provider prompt profiles for Codex/Claude/scripted execution, a provider-backed
risky-run implementer path, parsed provider transcript summaries, local evidence
cockpit generation, a guarded local workbench execution queue, real-project task
templates, generic real-project
provider-backed closure, provider-backed autonomous repair attempts with retry
budget evidence, release build, checksum-aware release archive installers, local
release archive packaging, live OAuth CLI UAT for Codex/Claude, a manual private
GitHub Release, provider smoke history, workbench-triggered provider smoke, a
control-plane provider readiness rollup, and a local CI gate. The next
production gate is bringing one real provider adapter into the same readiness
loop while keeping policy gates, replay, evidence, and scorecards unchanged.

## Real Repo Pilot

See `docs/REAL-REPO-PILOT.md`.

Result:

- `bug-fix` template on disposable `secure-agent-profile` copy: accepted,
  replay digest failures `0`.
- `test-generation` template on disposable `secure-agent-profile` copy:
  accepted, replay digest failures `0`.
- authenticated Codex `bug-fix` template on disposable `secure-agent-profile`
  copy: accepted, replay digest failures `0`.
- authenticated Claude `test-generation` template on disposable
  `secure-agent-profile` copy: accepted, replay digest failures `0`.

## Node Real Repo Pilot

See `docs/NODE-REAL-REPO-PILOT.md`.

Result:

- `npm test` verifier: accepted, replay digest failures `0`.
- `npm run typecheck` verifier: accepted, replay digest failures `0`.
- `npm test --workspace @ao2/node-pilot` verifier: accepted, replay digest
  failures `0`.

## Structured Repair Pilot

See `docs/STRUCTURED-REPAIR-PILOT.md`.

Result:

- Node repair-context pilot: accepted, replay digest failures `0`.
- Python repair-context pilot: accepted, replay digest failures `0`.
- repair prompt artifacts include verifier output and provider summaries.

## Live Structured Repair UAT

See `docs/LIVE-STRUCTURED-REPAIR-UAT.md`.

Result:

- Codex structured repair UAT: accepted, replay digest failures `0`.
- Claude structured repair UAT: accepted, replay digest failures `0`.
- provider summaries parsed for both providers before and after repair.

## Release Install Smoke

See `docs/RELEASE-INSTALL-SMOKE.md`.

Result:

- macOS/aarch64 install smoke: accepted scripted repair, replay digest failures
  `0`.
- Ubuntu 24.04 Docker install smoke: accepted scripted repair, replay digest
  failures `0`.
- Windows/x86_64 archive smoke: `install.ps1`, `bin/ao2.exe`, and checksum
  validation passed.
- All release archives include `RELEASE-MANIFEST.json` with schema
  `ao2.release-manifest.v1`.
- Signed release provenance verifies with `npm run release:verify-provenance`.
- Release install smoke verifies signed provenance when `dist-provenance/`
  exists.
- Windows native installer execution passed on `antho@10.0.0.96`
  (`HP255_G10`).
- Windows native evidence:
  `C:\ao2-public\AppData\Local\Temp\ao2-three-os-smoke\run\repo\.ao2\runs\windows-install-smoke-repair\evidence-pack\evidence-pack.json`.
- Windows native cockpit:
  `C:\ao2-public\AppData\Local\Temp\ao2-three-os-smoke\run\repo\.ao2\runs\windows-install-smoke-repair\cockpit\index.html`.
- Three-OS release smoke:
  `/tmp/ao2-public/ao2/target/three-os-smoke/20260518063529/report.md`.

## Install Health And Recovery

Result:

- `ao2 doctor --json` reports install, PATH, persisted install verification
  evidence, signed provenance, dependency, and scripted provider health.
- `ao2 upgrade check` reports release metadata from local fixture files and
  direct release URLs.
- `ao2 upgrade apply` installs a signed release selected from metadata, reports
  the install verification sidecar path, and preserves rollback.
- `ao2 install update` verifies the signed archive, offline release report, and
  checksum coverage before preserving the previous installed binary as
  `<binary>.rollback`.
- `ao2 install rollback` restores that previous binary for default or custom
  install directories.
- `ao2 report --open` prints the generated cockpit path and platform open
  target.
- `ao2 cockpit serve --port 0 --once` serves the generated cockpit over local
  HTTP for deterministic smoke checks.
- `ao2 workbench export --open` writes the local operator workbench to
  `.ao2/workbench/index.html`.
- `ao2 workbench serve --port 0 --once` serves the operator workbench over
  local HTTP for deterministic smoke checks.
- `ao2 workbench serve --api-token <token>` protects local workbench APIs.
- `GET /api/provider-matrix?token=<token>` returns the same provider
  readiness matrix as `ao2 provider matrix --json`.
- The workbench renders a `Provider Readiness` panel with provider
  availability, timeout, sandbox boundary, transcript fields, and policy
  invariants.
- The workbench renders `Provider Safety Warnings` before launch and returns
  matching `provider_warnings` from `/api/launch` and `/api/queue/start`.
- `POST /api/launch?token=<token>` returns an `ao2 run --template ...`
  command preview for explicit shell execution.
- `ao2 workbench serve --enable-execution` enables token-protected
  `/api/queue/start`, `/api/queue/cancel`, `/api/queue/retry`,
  `/api/queue/job`, and `/api/queue`.
- `ao2 workbench serve --queue-retention <count>` controls persisted queue
  history size under `.ao2/workbench/queue.json`.
- `ao2 workbench serve --support-signing-key <pem>` signs
  `/api/queue/export` support metadata, derives
  `support-bundle-signing-public.pem`, and returns
  `support_metadata.signature_verified=true` when verification succeeds.
- `ao2 workbench support-verify --bundle-dir <dir>` verifies workbench
  support-bundle schema, queue schema, signed metadata, bundle digest, and
  signed count fields. JSON output includes attached evidence export
  summaries, including `ao2.operator-evidence-packet.v1` run ID, closure
  verdict, replay status, provider score, and artifact digests when present.
- `ao2 workbench support-inspect --bundle-dir <dir>` returns the same verified
  trust status as read-only JSON or text without creating a support case,
  including attached evidence export counts and run/operator-packet subjects.
- `ao2 workbench support-import --bundle-dir <dir>` verifies before copying a
  bundle into an offline support case with `import-summary.json` and
  `index.html`, including an `Evidence Exports` table when the bundle carries
  Workbench evidence exports.
- Workbench support verification rejects tampered signed metadata and tampered
  support bundle bodies before import writes any case artifacts.
- The workbench renders the latest local support bundle trust status and
  refreshes the trust panel after `/api/queue/export`.

Regression tests:

```sh
cargo test -p ao2-cli cli_doctor_reports_install_provider_release_and_path_health
cargo test -p ao2-cli cli_upgrade_check_reports_latest_release_from_fixture
cargo test -p ao2-cli cli_upgrade_apply_installs_signed_release_and_keeps_rollback
cargo test -p ao2-cli cli_install_update_keeps_previous_binary_for_rollback
cargo test -p ao2-cli cli_report_open_prints_browser_target_for_existing_cockpit
cargo test -p ao2-cli cli_cockpit_serve_once_returns_existing_report_html
cargo test -p ao2-cli cli_workbench_export_builds_operator_dashboard
cargo test -p ao2-cli cli_workbench_serve_once_returns_dashboard_html
cargo test -p ao2-cli cli_workbench_api_returns_runs_with_token
cargo test -p ao2-cli cli_workbench_api_returns_provider_matrix_with_token
cargo test -p ao2-cli cli_workbench_launch_api_builds_governed_run_command
cargo test -p ao2-cli cli_workbench_queue_requires_explicit_execution_flag
cargo test -p ao2-cli cli_workbench_queue_executes_scripted_run_and_reports_evidence
cargo test -p ao2-cli cli_workbench_queue_persists_failed_history_across_restart
cargo test -p ao2-cli cli_workbench_queue_can_cancel_running_job
cargo test -p ao2-cli cli_workbench_queue_can_retry_failed_job_and_renders_controls
cargo test -p ao2-cli cli_workbench_queue_job_detail_reports_logs
cargo test -p ao2-cli cli_workbench_queue_filters_by_status
cargo test -p ao2-cli cli_workbench_queue_retention_prunes_old_jobs
```

## Dogfood UAT

See `docs/DOGFOOD-UAT.md`.

Result:

- scripted dogfood repair: accepted
- replay digest failures: `[]`
- cockpit file generated:
  `/tmp/ao2-dogfood-upgrade-cockpit.AF1L8q/repo/.ao2/runs/dogfood-upgrade-cockpit-hardening/cockpit/index.html`
- local cockpit server returned valid cockpit HTML from
  `http://127.0.0.1:51843/`
- v0.2.0 external repo dogfood against `pallets/click`: accepted
- v0.2.0 external repo replay digest failures: `[]`
- `ao2 runs list`, `ao2 runs show`, `ao2 cockpit index`, and
  `ao2 cockpit serve --index --port 0 --once` verified against that external
  repo copy

## Live Provider UAT

See `docs/LIVE-UAT.md`.

Result:

- Codex CLI live run: accepted, replay digest failures `0`.
- Claude Code CLI live run: accepted, replay digest failures `0`.
- provider adapters are bounded by the default 900-second timeout and report
  normalized `timeout` blockers when a local CLI exceeds the configured bound.
- `npm run smoke:provider:codex` is safe by default and skips unless
  `AO2_LIVE_CODEX_SMOKE=1` is set.
- The Codex provider smoke removes `OPENAI_API_KEY` and `ANTHROPIC_API_KEY`,
  checks `adapter doctor --provider codex`, uses local Codex CLI OAuth only,
  records a provider smoke report and history entry, and requires a ready
  provider scorecard.
- `npm run smoke:provider:claude` is safe by default and skips unless
  `AO2_LIVE_CLAUDE_SMOKE=1` is set.
- The Claude provider smoke removes `OPENAI_API_KEY` and `ANTHROPIC_API_KEY`,
  checks `adapter doctor --provider claude`, uses local Claude Code CLI OAuth
  only, records a provider smoke report and history entry, and requires a ready
  provider scorecard.
- `ao2 provider score --target <repo> --run-id <id> --json` returns
  `ao2.provider-evidence-scorecard.v1` for existing provider-backed runs.
- Provider scorecards rate replay integrity, provider summary parse quality,
  changed-file evidence, blocker hygiene, and sandbox/policy boundary evidence,
  then emit `ready`, `warn`, or `fail`.
- `ao2 provider smoke-all --target <repo> --json` runs a deterministic
  scripted provider smoke, scores it, and reports Codex/Claude doctor state
  without invoking live model commands.
- Provider smoke-all persists `.ao2/provider-smoke/history.json`, and
  execution-enabled workbench operators can trigger the same loop from the
  `Run Provider Smoke` button.
- `ao2 provider smoke-all --live-provider codex --target <repo> --json`
  keeps Codex guarded unless `AO2_LIVE_CODEX_SMOKE=1` is set, then runs Codex
  through the same sandbox/evidence/score/history path as scripted provider
  smoke.
- Execution-enabled workbench provider smoke accepts `live_provider=codex` and
  requires both an operator token and the same `AO2_LIVE_CODEX_SMOKE=1` server
  environment gate before Codex runs.
- `ao2 provider smoke-all --live-provider claude --target <repo> --json`
  keeps Claude guarded unless `AO2_LIVE_CLAUDE_SMOKE=1` is set, then runs
  Claude through the same sandbox/evidence/score/history path as scripted
  provider smoke.
- Execution-enabled workbench provider smoke accepts `live_provider=claude`
  and requires both an operator token and the same `AO2_LIVE_CLAUDE_SMOKE=1`
  server environment gate before Claude runs.
- `ao2 provider gate --target <repo> --json` reads provider smoke history
  without invoking live providers, passes for ready scripted smoke by default,
  and fails closed with schema `ao2.provider-readiness-gate.v1` when history is
  missing or required providers are not ready.
- `ao2 provider gate --target <repo> --require codex --require claude --json`
  requires explicit live-provider smoke history for both providers and returns
  non-zero until both are ready at or above the requested minimum score.
- `ao2 provider pilot --target <repo> --provider codex --provider-prompt-file
  <file> --json` returns schema `ao2.provider-pilot-plan.v1`, embeds the
  provider readiness gate, and exits non-zero with `status: blocked` when live
  provider smoke history is not ready.
- After a ready live-provider gate, `ao2 provider pilot` materializes the
  selected real-project template and emits an explicit `ao2 run --template ...
  --provider ... --provider-prompt-file ...` command preview without invoking
  Codex, Claude, or any workflow.
- Provider pilot previews and provider-backed runs accept
  `--provider-max-budget-usd`; Claude provider prompts receive the matching
  `--max-budget-usd` CLI guard, while Codex acceptance bundles record the cap
  and rely on timeout plus repair-attempt bounds because Codex CLI has no
  direct max-budget flag.
- `ao2 provider cost-ledger --acceptance-root <dir> --json` and Workbench
  `/api/provider-pilot/cost-ledger` return `ao2.provider-cost-ledger.v1` by
  recursively verifying retained provider-pilot acceptance bundles and
  aggregating configured budget, observed provider cost, token totals, and
  per-provider budget-enforcement status from evidence packs.
- `ao2 provider cost-trend --acceptance-root <dir> --json` and Workbench
  `/api/provider-pilot/cost-trend` return `ao2.provider-cost-trend.v1` by
  grouping the verified ledger by release tag and reporting latest-vs-previous
  budget, observed-cost, and token deltas.
- Workbench `/api/provider-pilot` is operator-only, returns the same
  `ao2.provider-pilot-plan.v1` schema as the CLI, returns HTTP 400 with
  `status: blocked` when the readiness gate is not ready, and returns HTTP 200
  with a command preview after ready live-provider smoke history exists.
- Control-plane snapshots include provider smoke history, and fleet health
  reports provider readiness with schema `ao2.provider-readiness-rollup.v1`.
- The workbench run table renders provider score and verdict data from local
  evidence packs.
- Workbench launch and queue APIs reject `minimum_score` requests with
  `minimum_provider_score_not_met` when the named run has no scorecard or the
  score is below threshold.
- Linux/aarch64 Docker package: generated and checksum verified.
- Windows/x86_64 Docker package: generated and checksum verified.
