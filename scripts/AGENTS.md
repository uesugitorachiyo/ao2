# AO2 Scripts Scope Instructions

## Script Boundaries

- Treat release, promotion, publication, direct-main, deployment, provider, and credentialed scripts as authority-bearing surfaces. Do not run them without separate explicit authority.
- Keep dry-run or preview behavior the default where the script offers it. A readiness or dry-run result must not trigger a live follow-up implicitly.
- Use fail-closed shell evaluation: quote expansions, avoid `eval`, validate paths and digests before mutation, bound temporary directories, and install cleanup traps for task-owned state.
- Preserve macOS/Linux portability and the repository's explicit PowerShell/Windows paths. Do not silently replace a platform gate with a host-only approximation.
- Require an exact, fresh, digest-bound lease before a physical qualification action. Use bounded shared leases for release qualification and fixed lifecycle checks; allow unrelated interactive, Codex, IDE, and multiple SSH sessions while rejecting concrete lease, workload, scratch, and resource conflicts. Keep legacy exclusive v1/v2 evidence compatible. Lease readiness is not release or mutation authority.
- Generated packets, reports, caches, release assets, and evidence under ignored output roots are outputs, not source fixtures.

## Verification

- For shell changes run `bash -n` on the changed shell files and the narrow matching test or `package.json` script in its non-publishing mode.
- For Python changes use the repository's focused pytest selector with bytecode generation disabled.
- Run `npm run verify` when a script changes a Rust consumer contract. Report any release/live gate not run because authority was absent.
