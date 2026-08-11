# GitHub Issue Contribution Packet Contract

`ao2 issue contribution-packet verify` validates one sealed, public-safe
contribution packet without executing work, accessing the network, invoking
Git or GitHub, approving work, mutating a repository or fork, or publishing.

```text
ao2 issue contribution-packet verify --root <packet-root> --packet <packet-root>/packet.json [--json]
```

The packet is strict JSON with schema
`ao2.github-issue-contribution-packet.v1`. It binds:

- a bounded packet identifier, canonical GitHub repository, positive issue
  number, exact 40-character source SHA, and issue-snapshot SHA-256;
- direct-child reproduction, patch, test, and contribution-policy artifacts
  by exact size and SHA-256;
- local-human authorship and at least one explicit limitation;
- a fresh timestamp, current source and issue assertions, governance state,
  optional digest-bound maintainer-feedback evidence, and the complete denied
  safety boundary.

Artifacts must be regular direct children of the retained root directory.
Links, reparse points, traversal, nested paths, replacement races, malformed or
duplicate JSON, unknown fields, altered bytes, and oversized inputs fail
closed. Reproduction and test evidence must repeat the exact repository,
issue, and source identity. Reproduction must report `reproduced_failure`, all
three test classes must report `passed`, and policy evidence must identify a
nonempty license and `contribution_policy=accepted`.

Accepted governance states are `review_ready`, `denied`, `pending`,
`revision_requested`, `rejected`, and `cancelled`. Only `review_ready` emits
`contribution_ready=true`; this is technical readiness, not authority.
Maintainer feedback is a strict direct-child artifact bound to the same issue
and source identity. It may set `technical_state_changed=true`, which revokes
review readiness, but `mutation_authority_granted` must remain false.

Every successful readback records `mutation_authorized=false`,
`executes_work=false`, `approves_work=false`, and `publishes=false`. Fork,
branch, pull-request, publication, release, and deployment activity always
requires separate exact authority outside this contract.
