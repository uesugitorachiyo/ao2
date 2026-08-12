# Physical Bounded Host Lease Design

## Problem

AO2's exclusive physical-host lease rejects unrelated interactive sessions and
therefore reserves an entire Ubuntu or Windows host even for a fixed no-op
lifecycle check. That turns harmless SSH, IDE, and Codex activity into a
qualification blocker without proving a stronger property about the command
being run.

## Decision

Add `ao2.physical-host-bounded-lease.v1` as a separate strict lease contract.
It permits any non-negative number of interactive sessions, interactive AO
workloads, and SSH connections. It fails closed unless the producer reports:

- `isolation_mode=bounded_shared`;
- no conflicting lease, workload, or scratch identities;
- resource limits satisfied;
- one exact lease-owned scratch and cleanup root;
- the existing approval, digest, freshness, natural-completion, no-broad-kill,
  no-graphical-session-mutation, abort, and release protections.

The existing exclusive v1/v2 schemas remain byte- and behavior-compatible.
The release-sensitive `windows_stack_qualification:physical_unique` profile
continues to accept only those exclusive schemas. Bounded leases are accepted
only by the fixed Ubuntu and Windows `lifecycle_noop` offline profiles, so they
cannot authorize arbitrary worker commands or weaken release qualification.

## Verification

Regression tests must prove that bounded leases accept multiple SSH,
interactive sessions, and unrelated AO workloads; reject each concrete
conflict and unsafe boundary; remain unavailable to `physical_unique`; and
preserve current v1/v2 behavior. One short native Ubuntu and Windows
coexistence canary will validate an exact fresh bounded lease without signing
out, locking, stopping unrelated work, or starting a worker service.

No provider, credential, release, deployment, publication, inbound Windows
HTTP, arbitrary remote execution, broad cleanup, or session mutation is added.
