# AO2 Agent Instructions

## Status And Role

AO2 is the active, public, local-first governed execution runtime for the AO stack. It compiles authorized workflows, evaluates policy before side effects, binds approvals to exact action digests, runs bounded adapters, and owns local run state, evaluator closure, and replayable evidence.

AO2 Control Plane consumes typed state and evidence as a read-only observer. It does not own AO2 mutation, approval, closure, release, or publication. AO2 does not inherit authorization from readback, historical evidence, or generated recommendations.

## Sources Of Truth

- [docs/PRD.md](docs/PRD.md), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), and [docs/SDD-risky-pr-run.md](docs/SDD-risky-pr-run.md) define product and execution boundaries.
- [docs/SCHEMAS-AND-INTERFACES.md](docs/SCHEMAS-AND-INTERFACES.md), `schemas/`, and [docs/contracts/AO2-CANONICAL-V1.md](docs/contracts/AO2-CANONICAL-V1.md) own wire and evidence contracts.
- [docs/contracts/GITHUB-ISSUE-REPAIR-PACK.md](docs/contracts/GITHUB-ISSUE-REPAIR-PACK.md) owns the strict sanitized historical repair-pack validation and repair-result failure-classification contracts.
- [docs/contracts/AO2-QUALITY-GATES.md](docs/contracts/AO2-QUALITY-GATES.md) owns exact staged, outgoing-commit, and source-head quality execution.
- [docs/SECURITY.md](docs/SECURITY.md) owns fail-closed, secret, provider, approval, and side-effect rules.
- [docs/VERIFICATION.md](docs/VERIFICATION.md), `package.json`, and [`.github/workflows/ci.yml`](.github/workflows/ci.yml) define current commands and CI coverage.
- `Cargo.toml`, the workspace crates, and their tests are authoritative for implemented behavior.

## Ownership And Boundaries

- Keep the MVP local-first and evidence-exact. The runtime, policy, artifacts, approvals, replay, and evaluator closure remain source-owned here.
- Evaluate policy before every side effect. An approval authorizes only the exact recorded digest; drift, missing evidence, unknown state, or denied authority must fail closed.
- Physical-host qualification requires both the existing signed execution authorization and a separate fresh, digest-bound exclusive-host lease. Never treat the lease as authority for arbitrary commands, broad process termination, graphical-session mutation, or cleanup outside its lease-scoped scratch root.
- Do not add provider API-key paths. Do not record secrets, bearer values, private key material, account identifiers, private repository paths, or unredacted provider transcripts.
- Keep `target/`, `dist*/`, `.ao2/`, `.ao2-local/`, and generated run/evidence output out of source changes. Treat published records under `docs/release/` and `docs/beta/` as historical; do not rewrite them to support a current claim.
- Treat root quality manifests as untrusted source-owned declarations. Bind commit checks to the Git index, push checks to the exact base/head range, execute only validated argv arrays, and fail when a selected command changes Git state. Keep optional Git hooks as versioned, logic-free AO2 wrappers; never overwrite unmanaged or custom hook paths.
- Keep `ao-quality-gates.json` aligned with the commands and generated paths owned by this repository. Fast levels may select bounded checks, but the full level must preserve `npm run verify` and hosted CI remains merge authority.
- Treat repair-pack manifests and artifacts as untrusted, read-only inputs. Require root-level direct children, retain and identity-bind the root directory handle, open Unix children through the audited `openat` boundary, deny Windows root replacement while validating, and verify fresh timestamps, platform file identity and link count, streamed sizes and digests, strict JSON, canonical GitHub repository grammar, and the exact L1 safety boundary without unpacking archives, following links, invoking Git/GitHub, accessing the network, or executing repairs.
- Bind Rust workspace reproductions to exactly one safe `--package` value, one
  exact `--test` target, and the matching
  `crates/<package>/tests/<target>.rs` fixture path.
- Source fixtures in `fixtures/`, `tests/fixtures/`, and `examples/` are contracts. Change them only with the consumer tests, never to inflate a result or bypass a negative case.
- Release, deployment, publication, live-provider, credentialed, and direct-main commands require separate explicit authority. A dry run, readiness result, control-plane readback, or instruction file does not grant it.
- Before non-trivial writes, reserve your write scope in the active task record or ignored `target/` artifacts, confirm no overlapping task branch/worktree, and release it in the handoff with cleanup evidence.

## Working Method

- Start from the smallest owned surface. Preserve state-transition, retry, approval, evidence, and artifact provenance invariants across producer/consumer changes.
- Add or update negative tests for fail-closed behavior. Do not hand-edit outputs merely to satisfy schemas, evaluators, release gates, or readbacks.
- Use the four nested instruction scopes for runtime, policy, scripts, and workflows; keep repository-wide safety boundaries summarized here.
- If durable commands, authority, architecture, or ownership changes, update this file in the same pull request.

## Verification

- Runtime/state/side-effect changes: `cargo test -p ao2-runtime`.
- Policy/approval/redaction changes: `cargo test -p ao2-policy`.
- Schema/example changes: `cargo test -p ao2-runtime --test schema_and_examples`.
- Run the smallest affected CLI or script test before the broad gate. For the full Rust workspace use `npm run verify`.
- Quality snapshot or execution changes: `cargo test -p ao2-cli --test quality_exact_snapshot`.
- Quality hook or hook-diagnostic changes: `cargo test -p ao2-cli --test quality_hooks`.
- Run release/readiness commands only when the changed surface requires them and the task separately authorizes their non-publishing mode. Never run a release, deploy, publication, or live-provider command by implication.
- For instruction changes run `python3 ../ao-architecture/scripts/verify_agent_instruction_layout.py --workspace-root .. --repository ao2`. Always run `git diff --check`.

## Evidence And Completion

- Record commands, exit status, relevant artifact or evidence digests, and the source head. Report skipped, unavailable, networked, credentialed, or failed checks explicitly.
- Completion requires focused checks, the applicable broad gate, green pull-request CI, synchronized clean `main`, and task-branch/worktree cleanup.
- Preserve `skills/` and `.claude/skills/` as their existing byte-identical packaging projections. Skill packaging changes are a separate reviewed task.
