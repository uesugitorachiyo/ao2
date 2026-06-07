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
npm run public:hardening # CI-friendly public hardening subset
npm run public:hardening-ci-workflow # local evidence for tracked public hardening CI workflow
npm run public:hardening-workflow-file-dry-run # render side-effect-free public hardening workflow preview
npm run public:hardening-workflow-tracked-proposal # tracked workflow proposal evidence
npm run public:hardening-ci-local-runner-parity # compare workflow commands to local runner commands
npm run scripts:tracking-intent-audit # script tracking intent and promotion manifest
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
npm run pulse:generate-next
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
`target/pulse-auto-advance/latest/summary.json`. With `--forever`, it writes
`ao2.pulse-auto-advance-heartbeat.v1` while waiting and calls
`npm run pulse:generate-next` after each successful packet.

`npm run pulse:generate-next` emits `ao2.pulse-generate-next.v1` at
`target/pulse-generate-next/latest/summary.json` and writes a fresh
`packet.md`, `board.md`, `executor-evidence.json`, `pulse-eval-loop.json`, and
`ao2.pulse-next-lengthy-tasks.v1` packet. Generated packets use strategic scoring
instead of blind rotation: each cycle performs project-level reassessment
against `docs/PRD.md`, `docs/SDD-risky-pr-run.md`,
`docs/SCHEMAS-AND-INTERFACES.md`, and `docs/IMPLEMENTATION-SLICES.md`, samples
ledger history from `.ao2-local/pulse/pulse-auto-advance-ledger.jsonl`, applies
anti-recursion penalties, and includes rationale, required evidence, stop
conditions, and per-candidate `strategic_score` metadata. `npm run
pulse:daemon:start` runs the forever loop through launchctl or a detached tmux
fallback; `npm run pulse:daemon:status` emits `ao2.pulse-daemon.v1` at
`target/pulse-daemon/latest/summary.json`.

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
  emits `ao2.risky-pr-golden-path.v1`
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
  `ao2.release-readiness-local.v1` plus local `report.md` and `report.html`
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
  `schema_version` values in `ao2.release-artifact-consumer-smoke.v1`
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
  `target/pulse-real-execute-containment/latest/allowed-output`, and emits
  `ao2.pulse-real-execute-containment.v1` at
  `target/pulse-real-execute-containment/latest/summary.json`
- `npm run phase1:promotion-golden`: runs the signed Phase 1 operator golden
  smoke, records readback/dashboard evidence, scans logs/artifacts for bearer
  token leaks, preserves the `AO2_PHASE1_API_TOKEN_ENV` boundary, and emits
  `ao2.phase1-promotion-golden-path.v1` at
  `target/phase1-promotion-golden/latest/summary.json`
- `npm run release:evidence-closure`: runs CI artifact download, local canary,
  Phase 1 promotion golden evidence, Pulse execute safety, bounded real Pulse
  execute, control-plane negative restore evidence, artifact index, and
  fail-on-attention artifact health, then writes
  `ao2.release-evidence-closure.v1` at
  `target/release-evidence-closure/latest/summary.json` plus
  `target/release-evidence-closure/latest/closure.html`
- `npm run mvp:acceptance-matrix-gate`: runs the provider-free Risky PR golden
  path and maps PRD `AC-01` through `AC-12` plus SDD `UAT-01` through
  `UAT-12` to concrete evidence without manual filesystem archaeology; emits
  `ao2.mvp-acceptance-matrix-gate.v1` at
  `target/mvp-acceptance-matrix/latest/summary.json`
- `npm run workbench:no-archaeology-audit`: generates cockpit and workbench
  evidence for a Risky PR run, then proves the operator can answer objective,
  denied action, approved digest, changed files, test evidence, rejection
  reason, correction, closure verdict, export path, and replay status from
  evidence surfaces alone; emits `ao2.no-archaeology-workbench-audit.v1` at
  `target/no-archaeology-workbench/latest/summary.json`
- `npm run control-plane:observer-hardening`: composes signed evidence-pack
  ingest/readback, negative restore drill, long-lived control-plane smoke,
  artifact index, and fail-on-attention artifact health while verifying the
  control plane remains a read-only observer; emits
  `ao2.control-plane-observer-hardening.v1` at
  `target/control-plane-observer-hardening/latest/summary.json`
- `npm run provider:phase2-contract-hardening`: verifies Codex and Claude
  provider contracts, replacement parity, no-factory-v3 guardrails, transcript
  parsing, sandbox patch digest boundaries, exact approval enforcement,
  blocker taxonomy, and fail-closed live guards; emits
  `ao2.provider-phase2-contract-hardening.v1` at
  `target/provider-phase2-contract-hardening/latest/summary.json`
- `npm run release:train-drill`: rehearses release evidence closure, release
  readiness regression, retention preflight with pruning disabled, artifact
  consumer dry-run, and post-merge canary without tag, push, publish, or deploy
  side effects. It records install/update verification as a
  `release:download-verify` reference for real release assets and emits
  `ao2.public-release-train-drill.v1` at
  `target/public-release-train-drill/latest/summary.json`
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
  consumer dry-run, Pulse resume dry-run, and the ao2-control-plane long-lived
  smoke into `ao2.post-merge-canary.v1`
- `npm run phase1:promote`: prepares Phase 1 prerequisites, runs promotion
  preflights, publishes to ao2-control-plane when
  `AO2_PHASE1_CONTROL_PLANE_URL` is set, and may capture a dashboard snapshot
  with `AO2_PHASE1_DASHBOARD_SNAPSHOT=1`
- four cross-OS archives at v0.4.80 (macOS aarch64, Linux aarch64, Linux
  x86_64, Windows x86_64) all SHA256 + RSA signature verified
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
  and cockpit/evidence links, and renders a Run Evidence Summary control in the
  Workbench UI;
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
  `/api/runs/evidence/export`, writes summary, diff, or changed-evidence JSON
  support handoff artifacts under `.ao2/workbench/evidence-exports/`, rejects
  viewer tokens, and renders Export Summary / Export Diff / Export Changes
  controls in the Workbench UI;
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
  SHA256, generated timestamp, and JSON content;
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
- release archives include `install.sh`, `install.ps1`, and bundled binary
  checksums;
- release archives include `RELEASE-MANIFEST.json` with package, target,
  binary path, and packaged binary checksum metadata;
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

- `ao2 doctor --json` reports install, PATH, signed provenance, dependency,
  and scripted provider health.
- `ao2 upgrade check` reports release metadata from local fixture files and
  direct release URLs.
- `ao2 upgrade apply` installs a signed release selected from metadata and
  preserves rollback.
- `ao2 install update` preserves the previous installed binary as
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
  summaries.
- `ao2 workbench support-inspect --bundle-dir <dir>` returns the same verified
  trust status as read-only JSON or text without creating a support case,
  including attached evidence export counts and run subjects.
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
