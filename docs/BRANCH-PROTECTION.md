# Branch Protection

This runbook records the live `main` branch protection expected for the public
AO2 repository.

## Required Settings

Configure `main` with these controls:

- Require status checks to pass before merge.
- Require branches to be up to date before merge.
- Include administrators.
- Require linear history.
- Block force pushes.
- Block branch deletion.

## Required Checks

AO2 uses the `CI` matrix as the required merge gate, plus `Cargo deny (supply
chain)`. The exact required context list is intentionally mirrored in
`scripts/verify-branch-protection.sh` so job-name drift is visible.

Representative required checks include:

- `Cargo deny (supply chain)`
- `Verify ubuntu-latest / fmt`
- `Verify ubuntu-latest / build-release`
- `Verify macos-latest / build-release`
- `Verify windows-latest / build-release`

## Live Verification

After changing branch protection or renaming CI jobs, run:

```sh
scripts/verify-branch-protection.sh
```

The verifier is read-only and defaults to `AO2_BRANCH_PROTECTION_MODE=full`,
which checks the administrative branch-protection endpoint. The scheduled/manual
`Production Readiness Ops` workflow uses:

```sh
AO2_BRANCH_PROTECTION_MODE=limited scripts/verify-branch-protection.sh
```

Limited mode is used because the default GitHub Actions token cannot read every
administrative branch-protection field. It still verifies that `main` is
protected and that the exact required status checks are enforced for everyone.
