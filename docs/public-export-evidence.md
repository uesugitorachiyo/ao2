# Public Export Evidence

## Scope

This folder is a clean-copy public export for the `ao2` repository. It was
created without private git history.

## Source Baseline

- Source repository: `ao2`
- Code export baseline commit: `165e9b3510a088ef2c14324ef67e0eb0eeb43085`
- SDD planning/execution commit: `ab700c1af8c568f87e5bf7c5bd5ac1ae4103c9a7`
- Export strategy: clean copy, no private git history

## Included Surface

- Core Rust workspace, schemas, examples, fixtures, packages, and scripts
- Public README, license files, notice, install, security, architecture, and verification docs
- Public CI/release workflows only

## Excluded Surface

- Private git history and local runtime state
- Private coordination docs, status logs, SDD execution artifacts, and handoff notes
- Release signing state, generated `target`/`dist` artifacts, and local absolute paths
- Private release workflow and private repo references

## Verification

- `npm run verify`: PASS
- `bash scripts/check-public-export.sh`: PASS before initial public git commit
- Read-only public-export audit found generated `target/` output after verification and checker self-literals; both were corrected before repository initialization.

## Publication Status

No GitHub remote or push was configured during this export. Public publication
still requires operator approval.
