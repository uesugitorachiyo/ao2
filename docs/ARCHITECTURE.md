# AO2 Architecture

AO2 is a local-first control plane for governed software-delivery agents. The
runtime owns workflow state, policy decisions, artifacts, evidence, and closure.
Agent adapters are replaceable execution clients, not the source of truth.

## MVP Boundary

The first vertical slice is the provider-free `Risky PR Run`. It deliberately
uses deterministic role behavior so the governance path can be verified without
model variability.

Runtime responsibilities:

- compile the workflow reference into a local run record;
- append events as JSONL;
- write immutable artifacts with digests and lineage;
- evaluate side effects before execution;
- create exact-digest approval tickets;
- reject closure when evidence is missing;
- export an evidence pack and static report.

Adapter responsibilities:

- run a bounded local CLI command for a role;
- capture stdout, stderr, exit code, and transcript;
- normalize non-zero exits and timeouts into blocker records;
- return evidence to the runtime instead of owning run state.

Current adapter boundary:

- `scripted` is deterministic and built in;
- `codex` and `claude` can be detected with `ao2 adapter doctor`;
- Codex and Claude provider prompt profiles live in dedicated crates
  (`ao2-adapter-codex`, `ao2-adapter-claude`) while the shared
  `ao2-adapters` crate keeps the sandbox, transcript, and patch-promotion
  contract stable;
- `ao2 provider matrix --json` reports provider doctor state, timeout policy,
  transcript fields, and trust-boundary invariants for scripted, Codex, and
  Claude adapters;
- `ao2 provider registry --json` exposes the Phase 2 provider/plugin registry:
  provider contracts, explicit live-provider guards, extension slots, lifecycle
  gates, and deferred features in a Hermes/control-plane consumable shape;
- `ao2 provider registry --control-plane-url <url> --api-token-env AO2_CP_API_TOKEN --json`
  publishes the same registry snapshot to an observer-only control-plane
  endpoint, and `--signing-key <pem> --signer-id <id>` upgrades that publish to
  a signed registry upload;
- adapter commands can execute in an isolated sandbox copy of the target repo;
- adapter run and prompt commands default to a 900-second timeout, with
  normalized `timeout` blockers when a provider exceeds the bound;
- sandbox execution reports changed files and a diff summary without mutating
  the target repo;
- sandbox patch preview emits an exact action digest;
- sandbox patch apply copies changed files into the target repo only when the
  supplied digest matches the preview;
- provider prompt profiles turn Codex, Claude, or scripted prompts into sandbox
  command invocations;
- provider-backed risky-run execution uses the provider prompt profile for the
  implementer role, then promotes sandbox changes through exact-digest patch
  apply before review/evaluator closure;
- transcript parsing normalizes provider output into changed files, concerns,
  blockers, token usage, optional cost, and a provider-authored summary;
- the risky-run MVP records raw transcript and parsed transcript-summary
  artifacts, and embeds parsed provider summaries in the evidence pack.

Optional hosted or local front ends may read the provider registry, but they do
not become the execution boundary. New adapters should land as separate crates
and reuse the same sandbox, digest, replay, and evidence-pack contracts before
any control-plane dashboard or Hermes workflow presents them as runnable.

## Data Flow

```text
workflow -> run context -> role task -> policy gateway -> artifact store
         -> reviewer concern -> evaluator closure -> evidence export
```

Every role handoff crosses an artifact boundary. Terminal output is never the
durable record of a run.

## Optional Control Plane

AO2 does not require a hosted server. A future `ao2-control-plane` should remain
an optional separate layer that consumes AO2 artifacts, queue snapshots, audit
logs, and token-protected local APIs. The local AO2 runtime must continue to own
policy gates, exact action digests, approvals, replay, evidence packs, and
closure verdicts.

See `docs/AO2-CONTROL-PLANE.md` for the proposed split between the signed local
runtime and a later team/fleet visibility server.

## Trust Model

The local runtime is the trusted coordinator. Adapters, shells, package managers,
network tools, and future model providers are untrusted actors whose side effects
must be mediated by policy.

## Cross-Platform Position

The runtime is implemented in Rust and uses only standard filesystem, process,
and JSON operations for the MVP. It is intended to run on macOS, Linux, and
Windows with stable Rust installed.
