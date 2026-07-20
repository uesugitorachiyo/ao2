# 10 Schemas And Interfaces

Created: 2026-05-16

## Purpose

This document defines the minimum build contracts missing from the original strategy plan.

Implementation should not begin until these contracts are converted into JSON Schemas, TypeScript types, or Rust types.

## Minimum Schemas

### `workflow.schema.json`

Required fields:

- `id`
- `version`
- `name`
- `description`
- `inputs`
- `roles`
- `tasks`
- `dependencies`
- `budgets`
- `tool_scopes`
- `approval_rules`
- `evaluator`

### `role.schema.json`

Required fields:

- `id`
- `purpose`
- `provider_policy`
- `allowed_context`
- `allowed_tools`
- `sandbox_profile`
- `output_schema_ref`
- `evidence_requirements`
- `concern_taxonomy`
- `blocker_taxonomy`

### `task.schema.json`

Discriminated union by `kind`.

MVP task kinds:

- `artifact_transform`
- `agent_session`
- `tool_request`
- `human_approval`
- `command_verification`
- `review`
- `closure_eval`

Required fields:

- `id`
- `kind`
- `role_id`
- `input`
- `output`
- `depends_on`
- `side_effect_class`
- `approval_required`
- `timeout_seconds`
- `retry_policy`
- `idempotency_key`

### `run_record.schema.json`

Required fields:

- `run_id`
- `workspace_id`
- `workflow_ref`
- `status`
- `objective`
- `compiled_plan_digest`
- `events_head`
- `artifacts`
- `policy_decisions`
- `approval_tickets`
- `costs`
- `closure`

Statuses:

- `created`
- `compiled`
- `queued`
- `running`
- `waiting_for_approval`
- `blocked`
- `failed`
- `rejected`
- `accepted`
- `accepted_with_concerns`
- `canceled`
- `replaying`

### `event.schema.json`

Required fields:

- `event_id`
- `event_type`
- `run_id`
- `workflow_id`
- `role_id`
- `task_id`
- `timestamp`
- `actor`
- `causation_id`
- `correlation_id`
- `trace_id`
- `span_id`
- `payload`
- `payload_digest`
- `schema_version`
- `sensitivity`

Minimum event types:

- `run.created`
- `run.compiled`
- `role.started`
- `role.completed`
- `task.started`
- `task.completed`
- `task.failed`
- `task.blocked`
- `tool.requested`
- `tool.allowed`
- `tool.denied`
- `approval.requested`
- `approval.granted`
- `approval.denied`
- `artifact.created`
- `eval.completed`
- `closure.accepted`
- `closure.rejected`
- `budget.warning`
- `budget.exceeded`

### `artifact.schema.json`

Required fields:

- `artifact_id`
- `type`
- `uri`
- `media_type`
- `digest`
- `producer`
- `input_refs`
- `lineage`
- `sensitivity`
- `retention`
- `signature`

MVP artifact types:

- `context_bundle`
- `plan`
- `transcript`
- `patch_summary`
- `changed_files_summary`
- `command_log`
- `test_log`
- `review`
- `policy_decision`
- `approval`
- `closure_report`
- `evidence_pack`

### `ao2.sandbox-patch-approval-subject.v1`

AO2 sandbox patch preview, approval tickets, apply results, and retained
evidence use one canonical approval subject:

```json
{
  "schema_version": "ao2.sandbox-patch-approval-subject.v1",
  "repository_identity": "sha256:<64 lowercase hex characters>",
  "base_commit": "<40 or 64 lowercase hex characters>",
  "operation_type": "sandbox_patch_apply",
  "operations": [
    {
      "order": 0,
      "path": "src/example.rs",
      "kind": "modified",
      "before": {
        "kind": "regular_file",
        "content_sha256": "sha256:<64 lowercase hex characters>",
        "symlink_target_sha256": null,
        "unix_mode": 420
      },
      "after": {
        "kind": "regular_file",
        "content_sha256": "sha256:<64 lowercase hex characters>",
        "symlink_target_sha256": null,
        "unix_mode": 420
      }
    }
  ]
}
```

`repository_identity` hashes the canonical Git common-directory identity. Raw
local repository paths are not emitted. `base_commit` is the exact target
`HEAD`. A target without a valid Git identity and commit is rejected.

Operation paths are normalized repository-relative paths. Absolute paths,
parent traversal, non-normal components, and paths that cannot be represented
canonically are rejected. Operations are sorted by path and have contiguous
zero-based `order` values. `kind` is `added`, `modified`, or `deleted`.

Regular-file state binds the content SHA-256 and supported Unix mode. Symlink
state binds the symlink target bytes and mode without following the link.
Unsupported filesystem entry types fail closed. `before` is absent only for an
addition; `after` is absent only for a deletion.

The action digest is the SHA-256 of the serialized typed subject. Apply
reconstructs the subject from the current target and retained sandbox before
the first target write and requires the reconstructed digest to equal the
approved digest. Repository substitution, target or sandbox content drift,
base-commit drift, mode changes, symlink changes, operation changes, and order
changes therefore invalidate the approval.

P0-A makes the approval action identifier content- and base-bound. It does not
validate who approved it, establish the stronger ticket authority required by
P0-B, or make application transactional. Those remain blocked by P0-B, P0-C,
and P0-D.

### SDD Planning Context Interface

`ao2 sdd plan` scans the target repository into a deterministic `surface_map`
metadata object, then uses token-aware context shrink by default before sending
the provider request. The provider sees bounded file metadata only: paths, kinds,
hashes, and public symbols. AO2 does not send full file bodies and does not rely
on long conversational compaction for SDD planning.

The default planning budget is `8000` cl100k_base tokens and can be overridden
per run:

```sh
ao2 sdd plan \
  --prompt @prompt.md \
  --target /path/to/repo \
  --provider codex \
  --context-budget-tokens 12000 \
  --out plan.json
```

The full scanned map is still measured for provenance. Provider planning,
validation, and `target.surface_map_sha256` all use the same shrunken map.
Each successful plan writes `<target>/target/sdd-planner/<plan_id>/context.json`
and also prints:

- `full_surface_map_sha256`
- `shrunken_surface_map_sha256`
- `full_file_count`
- `shrunken_file_count`
- `context_budget_tokens`
- `context_shrink_enabled`

AO Atlas remains the oversized mission/workgraph context layer. The AO2 SDD
shrinker is the per-run tactical selector for the immediate planning prompt.

### `policy_decision.schema.json`

## SDD Run Spec Provider Modes

`ao2 sdd dispatch --runner ao2` translates an `ao2.sdd-plan.v1` document into
an AO2 run spec that can be executed with `ao2 run --spec <path>`.

For `template_kind: real_project`, the default `ao2 run --spec` path is a
provider-free real_project execution. It is evidence-only: AO2 records the
dependency-ordered SDD task graph, records provider-free task summaries, runs the
configured verifier commands, writes the normal evidence pack and run record,
and does not apply fixture-specific patches or scaffold files from another
template.

Provider-free real-project runs may execute explicitly declared local commands
when a task includes `provider_free.commands`. These commands run in dependency
order from the target repository root, use the same portable command wrapper as
verifier commands, fail the run on a non-zero exit, and are recorded as
`provider_free_command_log` artifacts. AO2 does not infer or synthesize these
commands from prose acceptance criteria.

Implementation work for a real project requires provider-backed execution, for
example `ao2 run --spec <path> --provider scripted` or another configured
provider. In provider-backed mode, AO2 builds prompts from the SDD task graph and
executes each task through the governed runtime before verification and closure.

Required fields:

- `decision_id`
- `principal`
- `action`
- `resource`
- `request_digest`
- `decision`
- `reason`
- `policy_version`
- `approval_ticket_id`
- `created_at`

Decisions:

- `allow`
- `deny`
- `requires_approval`

### `approval_ticket.schema.json`

Required fields:

- `ticket_id`
- `run_id`
- `requested_action`
- `action_digest`
- `risk_class`
- `requester`
- `approver`
- `status`
- `scope`
- `created_at`
- `expires_at`

Statuses:

- `pending`
- `approved`
- `denied`
- `expired`

### `tool_request.schema.json`

Required fields:

- `tool`
- `operation`
- `args`
- `cwd`
- `env_policy`
- `egress`
- `expected_side_effects`
- `principal`
- `run_id`
- `role_id`
- `task_id`

### `tool_result.schema.json`

Required fields:

- `status`
- `stdout_ref`
- `stderr_ref`
- `exit_code`
- `artifacts`
- `sanitized_summary`
- `side_effects_observed`

### `context_bundle.schema.json`

Required fields:

- `bundle_id`
- `role_id`
- `sources`
- `digests`
- `redactions`
- `role_scope`
- `prompt_ref`
- `created_at`

### `closure.schema.json`

Required fields:

- `verdict`
- `acceptance_criteria_results`
- `evidence_refs`
- `unresolved_concerns`
- `blockers`
- `policy_exceptions`
- `cost_summary`
- `created_at`

Verdicts:

- `accepted`
- `accepted_with_concerns`
- `rejected`
- `blocked`
- `needs_human_decision`

### `obligation-ledger.schema.json`

Required fields:

- `schema_version`
- `source_contracts`
- `obligations`
- `summary`
- `verdict`
- `created_at`

Obligation statuses:

- `pass`
- `fail`
- `unverified`
- `waived`

Closure rule:

- `fail` and `unverified` are blocking for complex/risky work unless the
  operator explicitly accepts a waiver.
- Content-preservation obligations must cite concrete path/line evidence for
  each exact fragment.
- Source contracts and generated run-status/evaluation artifacts are not valid
  evidence targets for exact-fragment closure checks.
- Semantic obligations with no extracted exact fragments remain `unverified`
  until an operator records path/line evidence or an explicit waiver. CLI
  annotations use `ao2 contract annotate`; Workbench annotations use
  operator-token-protected `POST /api/obligations/annotate` and update the
  sidecar ledger for the selected run.
- Workbench annotations emit `ao2.workbench-audit-event.v1` events with action
  `obligation_annotate` and write `ao2.workbench-evidence-export.v1` artifacts
  with export kind `obligation-annotation`. When the Workbench has
  `--support-signing-key`, the annotation evidence export is signed and
  verified before the API response returns.

### `report-contract.schema.json`

Required fields:

- `schema_version`
- `required_sections`
- `present_sections`
- `missing_sections`
- `complete`

The canonical schema version is `ao2.report-contract.v1`. Report producers use
this contract to prove an operator can inspect the objective, run health,
governance decisions, approvals, artifacts, closure evidence, replay evidence,
static export paths, and local run record without opening raw evidence JSON.
`ao2 report verify` emits `ao2.report-contract-verification.v1`, and release
support bundles must include that verification as first-class evidence before
the bundle verifier accepts the release handoff.

### Local GitHub draft PR publisher fixture

`ao2 issue draft-pr` exercises the governed issue-to-draft-PR write boundary
against a local fixture API. It never accepts GitHub credentials or a public
endpoint.

Commands:

```text
ao2 issue draft-pr preview --evidence <evidence.json> --out <action.json> --json
ao2 issue draft-pr verify --action <action.json> --expected-action-digest <sha256> --json
ao2 issue draft-pr fixture-publish --action <action.json> --expected-action-digest <sha256> --fixture-api http://127.0.0.1:<port> --json
```

`ao2.github-draft-pr-evidence.v1` is strict and bounded to 64 KiB. It binds:

- canonical issue URL, positive issue number, snapshot SHA-256, and
  `classification=authentic_bug`;
- exact target repository, base and head branches, and full lowercase commit
  SHAs;
- 1-100 sorted unique changed paths, diff/evidence/verification SHA-256
  values, `status=verified`, request/result identifiers, and worker source
  commit;
- bounded draft title and body;
- explicit false flags for prompt injection, security sensitivity, policy
  blocking, issue writes, ready-for-review, review approval, merge, and
  release.

`preview` derives `ao2.github-draft-pr-action.v1`. Its approval digest is
SHA-256 over only the `subject`, serialized with `ao2-canonical-v1` (sorted
keys, no whitespace). It appends one deterministic, bounded `AO2-Evidence:`
footer to the draft body containing the canonical issue URL and issue snapshot
SHA-256. User-supplied footer markers are rejected. The complete pretty JSON
action, including its trailing newline, must fit the same 64 KiB input bound
before `preview` exclusively creates the output. An existing file or symlink is
never followed or overwritten. `verify` requires the exact footer before
checking the digest and rejects unknown fields, malformed or oversized input,
digest drift, identity drift, unverified repair evidence, and any unsafe
boundary. On Unix, all input commands open with `O_NOFOLLOW | O_NONBLOCK`. On
Windows, they retain reparse-point protection, set security QoS to
identification before open, and accept only opened handles for which
`GetFileType` returns `FILE_TYPE_DISK`. Every platform checks regularity and
size on the opened handle before and after the bounded read; Windows rejects
named pipes and devices before entering the read.

`fixture-publish` accepts only credential-free `http` endpoints expressed as a
numeric loopback address plus an explicit port. It follows no redirects, sends
no authorization header, limits response bodies to 256 KiB, and applies one
three-second deadline across each complete connect, request write, and response
read exchange. It uses only:

```text
GET  /ao2/fixture-attestation
GET  /repos/<owner>/<repo>/git/ref/heads/<branch>
GET  /repos/<owner>/<repo>/pulls?state=all&head=<owner:branch>&base=<base>
POST /repos/<owner>/<repo>/pulls
```

Every request carries:

- one fresh 256-bit lowercase hexadecimal `X-AO2-Client-Challenge` generated
  for that invocation;
- the exact canonical action digest and SHA-256 of the exact serialized draft
  request body;
- the target repository, action request path, `draft=true`, and exact base and
  head commits;
- after the first response, the fixture instance identifier supplied by that
  response.

Every accepted response is a strict envelope with
`ao2.local-draft-pr-fixture-exchange-attestation.v1`. The fixture
self-attestation repeats the fixture instance, invocation challenge, action
digest, request-body digest, repository, action request path, draft flag, both
commits, exact exchange method and path, outcome, and pull number when exactly
one pull is applicable. The `pull_number` member is always required and must be
JSON `null` when no pull is applicable. Unknown, missing, or mismatched fields
fail closed.

Before discovery or creation, the client requires
`ao2.local-draft-pr-fixture-attestation.v1` with the strict exchange
self-attestation and the explicit claims `claims_local_only=true`,
`claims_forwarding_capable=false`, and
`claims_external_network_enabled=false`. These are unauthenticated fixture
claims, not proof of process isolation or network behavior. The client then
resolves both base and head refs and requires their current commits to equal
the digest-bound action commits.

No matching pull sends one digest-bound `draft=true` POST. The POST includes
exact base/head commit preconditions. A successful response must attest that
the same fixture instance received the challenge-bound action and request,
reports outcome `created`, reports the exact positive pull number, reports
enforcement of both commits, and claims it neither forwarded nor contacted an
external endpoint. The write self-attestation repeats the challenge, action and
request-body digests, repository, action path, draft flag, commits, outcome,
and applicable pull number. Its `pull_number` member is always required and may
be JSON `null` only for a no-pull outcome. The client validates this strict
self-attestation. It does not authenticate the claims or claim it observed the
fixture write. One exact open draft with the exact positive pull number and
issue-bound body returns an idempotent readback without a POST.

On a strict create-conflict response with an equally strict write attestation,
the client re-reads once. Exactly one matching draft returns
`idempotent_readback_after_create_conflict`; zero, drifted, or multiple results
fail closed. The readback reports `post_performed=true` and
`fixture_write_observed=false`; the exact reread identifies the idempotent
outcome, not an observed fixture write. Pull number zero fails on created,
existing-idempotent, and conflict-recovery paths. Identity drift, missing or
changed evidence binding, non-draft state, closed or merged state, unknown
response fields, and malformed attestations also fail closed.

Publish readbacks report
`client_contact_scope=numeric_loopback_only`,
`fixture_exchange_attestation_status=strict_challenge_bound_self_attestation`,
`fixture_claims_authenticated=false`,
`fixture_write_observed=false`,
`external_write_observability=not_observable_from_client`, and
`behavior_outside_client_observable_boundary=not_claimed`. A 201 response uses
the honest status `fixture_reported_created`. The client reports only its own
POST and prohibited-action behavior. It does not infer public or external
writes from a loopback interaction and makes no claim about fixture behavior
outside the client-observable boundary.

### Privacy-safe troubleshooting support bundle

`ao2 support bundle` converts one bounded local source record into deterministic
observer-only JSON:

```text
ao2 support bundle --input <support-source.json> --out <support-bundle.json> --json
```

The strict input schema is `ao2.troubleshooting-support-source.v0.1`. It
requires:

- AO2 and Control Plane versions;
- operating-system and architecture identifiers;
- workflow and verifier identities;
- approval status and, when applicable, its SHA-256 action digest;
- replay and evidence status plus the retained evidence digest;
- manifest and release SHA-256 digests;
- bounded failure category and phase;
- at most 16 diagnostic log excerpts of at most 2 KiB each;
- one bounded smallest safe next action;
- explicit false observer-only flags for execution, provider calls, issue and
  public writes, and release or deployment.

Input and generated output are each limited to 64 KiB. Unknown or duplicate
fields, malformed JSON, unsupported states, invalid digests, source-like log
content, and authority-widening flags are rejected. On Unix, input is opened
with `O_NOFOLLOW | O_NONBLOCK`; Windows uses reparse-point protection and
accepts only disk-file handles. The command creates the output exclusively and
does not overwrite an existing file or symlink.

The output schema is `ao2.troubleshooting-support-bundle.v0.1`. Credentials,
generic environment assignment values, and Unix, Windows-drive, UNC, or file
URI filesystem paths are removed from the bounded logs. The bundle records
only redaction counts, never original values. It carries a stable
`problem_fingerprint` computed with
`ao2-canonical-v1` over normalized versions, platform, workflow/verifier,
approval, replay/evidence, manifest/release digests, failure identity, and next
action. Log contents, redaction counts, timestamps, and output paths do not
affect that fingerprint.

`governed_issue_route.input_trust=sanitized_untrusted` marks the bundle as
input suitable for the separately governed issue draft projection:

```text
ao2 issue draft-pr preview \
  --evidence <draft-evidence.json> \
  --support-bundle <support-bundle.json> \
  --out <draft-action.json> \
  --json
```

That path strictly revalidates the bundle, reproduces its canonical
fingerprint, and requires the draft evidence digest and body to bind the exact
fingerprint. Altered, malformed, unsafe, or mismatched bundles fail closed. It
does not authorize or perform issue creation. The bundle always emits:

```text
observer_only=true
safe_to_execute=false
executes_work=false
calls_providers=false
issue_write_performed=false
public_write_performed=false
release_or_deployment_performed=false
```

## Minimum Interfaces

```ts
interface EventStore {
  append(event: AoEvent): Promise<void>
  loadRun(runId: string): Promise<AoEvent[]>
}

interface ArtifactStore {
  put(input: ArtifactWrite): Promise<ArtifactRef>
  get(ref: ArtifactRef): Promise<ReadableStream>
}

interface PolicyEngine {
  evaluate(request: ToolRequest): Promise<PolicyDecision>
}

interface ToolGateway {
  execute(request: ToolRequest): Promise<ToolResult>
}

interface AgentAdapter {
  runRole(input: AgentRunInput): Promise<AgentRunResult>
}

interface ClosureEvaluator {
  evaluate(input: ClosureInput): Promise<ClosureReport>
}

interface WorkflowCompiler {
  compile(input: WorkflowCompileInput): Promise<CompiledWorkflow>
}

interface ContextCompiler {
  buildBundle(input: ContextCompileInput): Promise<ContextBundle>
}

interface ApprovalService {
  request(decision: PolicyDecision): Promise<ApprovalTicket>
  resolve(input: ApprovalResolution): Promise<ApprovalTicket>
}

interface ReplayService {
  replay(runId: string): Promise<RunRecord>
}
```

## State Machine

Allowed run transitions:

```text
created -> compiled
compiled -> queued
queued -> running
running -> waiting_for_approval
waiting_for_approval -> running
waiting_for_approval -> blocked
running -> rejected
rejected -> running
running -> accepted
running -> accepted_with_concerns
running -> blocked
running -> failed
running -> canceled
accepted -> replaying
rejected -> replaying
blocked -> replaying
replaying -> accepted
replaying -> rejected
replaying -> blocked
```

Disallowed:

- `created -> accepted`
- `running -> accepted` without closure report
- `waiting_for_approval -> accepted`
- `blocked -> accepted` without resumed run and closure report

## Storage Contract

MVP local storage:

```text
.ao2/
  runs/
    <run-id>/
      events.jsonl
      run-record.json
      artifacts/
        <artifact-id>/
          artifact.json
          content
      approvals/
        <ticket-id>.json
      evidence-pack/
        evidence-pack.json
```

SQLite may be used as the indexed event store, but JSONL and artifact files must remain exportable for inspection.

## Adapter Boundary Rule

The agent adapter may produce proposed actions, transcripts, files, and role outputs.

The agent adapter must not directly execute side-effecting operations outside the `ToolGateway`.

MVP enforcement can be conservative:

- run adapter in a constrained working directory;
- disable network unless approved;
- inspect proposed commands before execution;
- capture transcript as artifact;
- fail closed if the adapter attempts unsupported tool behavior.
