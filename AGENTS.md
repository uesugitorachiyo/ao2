# AO2 Agent Instructions

AO2 is a public, local-first governed software-delivery project. Use the
build-facing docs in `docs/` as the source of truth.

Rules:

- Before non-trivial writes, reserve the write scope in the active
  conversation. Store task-created files in the connected project folder, not
  in this AO2 instance or its ignored `target/` tree.
- Build against `docs/PRD.md`, `docs/SDD-risky-pr-run.md`, `docs/SCHEMAS-AND-INTERFACES.md`, and `docs/IMPLEMENTATION-SLICES.md`.
- Keep the MVP local-first.
- Do not add provider API-key auth paths.
- Do not use `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`.
- Do not record secrets, bearer tokens, private key material, local account
  identifiers, or private repo paths in tracked files.
- No side-effecting tool action should bypass policy.
- Evidence must exist before evaluator closure accepts a run.

## V3 Pool Overlay

This worktree is one sticky AO2 Public V3 Production workspace. Before
normal task work, read `..\V3_POOL_STATUS.json`,
`..\V3_OPERATING_MODES.md`, and `..\V3_CURRENT_BUILD.md`, then verify
that this exact instance is the task-owned guard claim. Keep the printed
V3 `AO2_ROOT` as the sole context anchor through task closure.

## Mandatory Project-File Storage Boundary

This numbered instance is an execution/context workspace, not a destination
for user project documents. Store every task-created temporary, intermediate,
supporting, or final work product in the connected project folder or an
appropriate project subfolder. This includes Markdown/text notes, drafts,
reports, screenshots, images, exports, archives, evidence packets, and final
deliverables. If the destination is missing, ambiguous, or unsafe to infer,
ask the user before creating the file.

Do not leave user project work anywhere in this instance, including `target/`.
Existing AO2 source, explicitly authorized AO2 maintenance, `.ao2-local`/`.ao2`
runtime state, dependency caches, and conventional compiler/build output are
allowed only for their stated operational purpose and must not be used to hide
project work. They may exist during execution, but release permits ignored
operational files only when their exact paths and hashes match the controlled
`..\V3_INSTANCE_HYGIENE_BASELINES.json`. Claim and release never auto-enroll
new files; remove disposable output or preserve recoverable work outside the
instance before cleanup.

Before release, run
`python ..\scripts\validate_v3_instance_hygiene.py --instance <id>`. The guard
also enforces this check. A failure requires classification and recovery outside
the numbered instance before confirmed debris is removed; never delete unclear
user work blindly.

Read `..\AO_MASTER_PROMPT_OPERATION_ORDER.md`,
`..\V3_LOCAL_SKILL_TRIGGER_MAP.md`,
`..\V3_MASTER_PROMPT_INSTRUCTION_RUBRIC.md`, and
`..\V3_ACCESSORY_ACCEPTANCE_POLICY.md` when their routes apply. Use the
shared local skill mirror at `..\references\local-skills`. Thought
Experiment remains mandatory for decision making, planning, and
implementation; Engineering Research records Mode A or Mode B before
research; Scope-to-Deliverable remains user-triggered for future tasks.

Thought Experiment is always used for decision making, planning, and
implementation regardless of task size. Engineering Research is the standard
research method when research is needed; use Mode A for local files and Mode B
for external sources. Scope-to-Deliverable Workflow is user-triggered heavy
mode: use the full workflow when the user names it, otherwise Codex may borrow
small useful gates when they improve quality.

AO Architecture accessory sync starts only from an explicit user manual
command. A manual sync refreshes the local accessory discovery inventory and
decision-map evidence and records validation. It must not automatically
install new or changed accessories. It must not automatically activate new or
changed accessories. It must not automatically route to new or changed
accessories. It must not automatically depend on new or changed accessories.
It must not automatically rewrite policy for new or changed accessories.
Discovery, candidate / pending review, validation, and explicit acceptance
remain separate states.

Do not make one-instance-only durable pool-policy changes. Any controlled
pool-wide instruction change requires a declared all-five transaction and
one failed instance blocks the pool. Never use or record provider API keys
or secrets.

AO Architecture accessory sync starts only from an explicit user manual
command. It must not automatically install, activate, route, depend on,
or rewrite policy for new or changed accessories.

<!-- TARGET_B_V3_EXECUTION_MASTER_PROMPT:BEGIN -->
Active V3 Execution Master Prompt: TARGET_B_V3_EXECUTION_MASTER_PROMPT.md
State: active_production
Origin candidate SHA-256: 97E2405CCD935E1525157FE7606997BC76B98B3ADA61337601A6DF1BBAAB2DA0
Effective prompt SHA-256: AFA29E7055B65CBB79FCD9451D157C83E5908E4C123DB18D504272B914B42032
Transformation: v3-production-effective-transform-20260713-v1 + portable-relative-paths-20260719
<!-- TARGET_B_V3_EXECUTION_MASTER_PROMPT:END -->
