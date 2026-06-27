# AO2 Refactor And Cleanup Agent Workflow

This workflow is for bounded refactor and cleanup work in `ao2`. It is grounded
in the current local workspace:

- The parent folder `/Users/torachiyouesugi/Documents/public` is a multi-repo AO
  stack workspace, not a git repository.
- No exact sibling folder named `ao-stack` was found during inspection.
- `ao2` is the active execution repo for AO work. `ao2-control-plane` is the
  read-only observer. `ao-operator` and `ao-runtime` are deprecated for active
  product work.
- AO2 is a Rust workspace with npm script wrappers, Python guard tests, shell
  gates, repo-native skills, and local evidence under ignored `target/` and
  `.ao2-local/`.

Placement recommendation: hybrid.

This first implementation lives inside `ao2` because cleanup work must respect
AO2's validation commands, evidence boundaries, generated paths, and agent
rules. Reusable cross-repo prompts, MCP connector drafts, and AO stack portfolio
cleanup policy should live outside `ao2` later, either in a sibling support
folder or in the central `ao-architecture` repository. AO2-specific gates should
travel with this repository.

## Refactor Intake

Create one task record from `REFACTOR_TASK_TEMPLATE.md` before editing product
code. The task must name:

- one cleanup category;
- a small file or module scope;
- intended behavior preservation;
- files allowed to change;
- files that must not change;
- validation commands and expected evidence;
- rollback command or revert plan.

Reject vague targets such as "clean up the repo", "modernize everything", or
"make architecture better". Convert them to one bounded task, such as:

- remove unused imports in one crate;
- deduplicate two equivalent helper functions;
- rename one internal helper and update callers;
- split one oversized shell helper into a sourced library plus caller;
- tighten one docs section against current commands.

## Repo Scan

Before edits, the agent runs:

```sh
bash scripts/refactor-scan.sh
git status --short
```

The scan is read-only. It reports current branch, dirty files, known generated
or local-only paths, detected package managers, likely validation commands, and
large surface areas that need extra caution.

Agents must inspect relevant files before editing. Use `rg` for search, prefer
structured parsers for structured files, and avoid sweeping rewrites across:

- `target/`, `.ao2-local/`, `.ao2/runs/`, `.ao2/control-plane/`, `.ao2/memory/`;
- `dist/`, `dist-linux/`, `dist-linux-x86_64/`, `dist-windows/`,
  `dist-provenance/`;
- `.release-signing/`, secret material, env files, release signing keys;
- `Cargo.lock`, deployment config, release metadata, or workflow files unless
  the task explicitly owns them;
- existing dirty files not listed in the task.

## Cleanup Categories

Use one category per task and per commit:

- Formatting cleanup: mechanical formatting only, no behavior edits.
- Dead code removal: remove unreachable or unused code only after compile or
  lint evidence proves it is unused.
- Duplicate logic reduction: consolidate equivalent code with tests around the
  shared behavior.
- Naming consistency: rename narrow internal symbols with compiler-backed
  references.
- Import and dependency cleanup: remove unused imports or dependencies with
  `cargo check`, `cargo test`, or targeted package validation.
- File organization: move or split files only with a written mapping and
  command evidence that imports and tests still pass.
- Documentation and comment cleanup: align docs with current scripts and
  evidence commands. Do not change claims beyond local evidence.
- Test cleanup: simplify fixtures, names, or helper duplication while preserving
  assertions.
- Type and lint cleanup: address a named compiler, clippy, or test warning.
- Architecture candidates: document the candidate and trade-offs first. Do not
  implement architecture-level changes in the same task that discovers them.

## Safety Gates

Every cleanup task uses these gates:

1. Run `git status --short` before editing and record unrelated dirty files.
2. Reserve the intended write scope in the task record.
3. Do not delete broad file sets without explicit human approval.
4. Do not make behavior-changing refactors unless tests already cover the
   behavior or the task adds validation first.
5. Keep one cleanup category per commit.
6. Run the narrowest relevant validation command after edits.
7. Review `git diff --stat` and `git diff --check`.
8. Summarize changed files, validation evidence, and rollback instructions.

Rollback options:

```sh
git diff -- docs/refactor-cleanup scripts/refactor-scan.sh scripts/refactor-check.sh
git restore --staged <paths>
git restore <paths>
```

Only use restore commands for files owned by the current task.

## Validation Loop

Choose validation by risk. Prefer quick, local checks first and broader gates
before commit or PR.

Read-only or docs-only cleanup:

```sh
bash scripts/refactor-check.sh docs
```

Shell script cleanup:

```sh
bash scripts/refactor-check.sh scripts
```

Rust formatting or compile-sensitive cleanup:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

AO2 full local verification:

```sh
npm run verify
```

AO2 broader local gates, when the task touches evidence, Pulse, release, or
cross-repo observer behavior:

```sh
npm run public:hardening
npm run ci:local
npm run gate:full
npm run control-plane:cross-repo-observer
npm run rsi:cross-repo-e2e
```

Do not install global dependencies as part of a cleanup task. If dependencies
are missing, record the missing command and stop.

## Agent Loop

Use a bounded, auditable loop:

1. Scan: run `scripts/refactor-scan.sh` and inspect relevant files.
2. Propose: write or update a task record with scope, category, risk, and
   validation.
3. Edit: make the smallest local change that satisfies the task.
4. Validate: run the declared command and capture the result.
5. Review: inspect `git diff --stat`, `git diff --check`, and risky hunks.
6. Record: update the task record with command output summary and follow-ups.
7. Decide: either stop or create the next task record. Do not auto-continue into
   a new cleanup category.

Any self-improvement in this workflow means improving prompts, task templates,
validation checklists, or tests based on observed failures. It does not mean an
uncontrolled self-modifying agent or autonomous direct-publish loop.

## MCP And Connector Strategy

Recommended lightweight integrations:

- Filesystem and repo search: built-in terminal tools, `rg`, and git.
- Git: status, diff, branch, log, and rollback checks.
- GitHub issues and PRs: optional for turning task records into reviewable
  issues or PRs.
- Test runner: `cargo`, `npm`, `python3 -m pytest`, and existing AO2 scripts.
- Docs search: `rg` over `docs/`, `README.md`, `AGENTS.md`, and `skills/`.
- ast-grep or tree-sitter: useful later for structural Rust/TypeScript shelling
  patterns, but not required for this first pass.
- Semgrep or CodeQL: useful for security-sensitive cleanup and CI-backed
  validation, but do not install them until a task justifies the added weight.
- SonarQube: not recommended for the first pass unless an existing server is
  already part of the project workflow.

Connector rule: document the intended connector, command, input paths, and
expected evidence before adding config or installing tools.

## First Safe Cleanup Candidate

The safest first real cleanup task is documentation-only:

- Target: align a narrow section of `docs/VERIFICATION.md` or a new
  `docs/refactor-cleanup/` task record with current validation commands.
- Category: documentation cleanup.
- Validation: `bash scripts/refactor-check.sh docs`.
- Stop condition: no product code, release metadata, lockfile, deployment, or
  generated artifact changes.
