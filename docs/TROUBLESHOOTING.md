# AO2 Troubleshooting

Use this page for first-pass support on AO2 `v0.5.0` installs and local runs.
Keep logs redacted before opening an issue. Do not include tokens, API keys,
private repository names, or unreleased evidence values.

For public issue reports, start from
[AO2 Support Reproduction Checklist](SUPPORT-REPRODUCTION.md). The same file is
available at `docs/SUPPORT-REPRODUCTION.md` in the repository.

## Approvals

AO2 approval prompts bind the proposed change to exact content digests. If an
approval is rejected or the run waits longer than expected:

1. Re-run the command that produced the approval request and compare the
   approval subject digest.
2. Confirm the target files did not change between preview and approval.
3. Inspect retained evidence under the run directory before retrying.
4. Retry from a clean target copy when the digest changed unexpectedly.

Approval failures are expected when the approved input no longer matches the
target state. Do not override a digest mismatch without understanding what
changed.

## Manifests

Release archives include `RELEASE-MANIFEST.json`, `SHA256SUMS`, installer
scripts, verifier scripts, and provenance files. If verification fails:

1. Download the full asset set from the same release tag.
2. Run `shasum -a 256 -c SHA256SUMS` from the asset directory.
3. Extract the archive again into an empty directory.
4. Run `./verify-release.sh` on macOS/Linux or `.\Verify-Release.ps1` on
   Windows before installing.

Do not mix files from different release tags.

## Command Not Found After Install

The archive installer prints the install directory. For the default Unix
location, add it to the current shell before running the first checks:

```sh
export PATH="$HOME/.local/bin:$PATH"
ao2 version --json
ao2 doctor --json
```

If you installed with a custom `AO2_INSTALL_DIR`, add that directory to `PATH`
or run `/path/to/bin/ao2 version --json` directly.

## Local Pilots

AO2 can run local scripted workflows without provider credentials. For a first
support reproduction, prefer the governed demo in
[First 30 Minutes With AO2](FIRST-30-MINUTES.md). If a provider-backed local
pilot is involved, first reproduce the issue with the `scripted` provider or a
provider-free workflow. That keeps support focused on AO2 behavior instead of
external CLI state.

## Rollback

`ao2 install update` preserves the previous binary as `<binary>.rollback` when
one exists. Restore it with:

```sh
ao2 install rollback
ao2 version --json
ao2 doctor --json
```

Use `--install-dir /path/to/bin` when AO2 was installed outside the default
location.

## Offline Verification

For already-downloaded public assets:

```sh
shasum -a 256 -c SHA256SUMS
tar -xzf ao2-0.5.0-<platform>.tar.gz
./verify-release.sh
```

On Windows:

```powershell
.\Verify-Release.ps1
```

Offline verification should run before install, update, rollback testing, or a
support issue. If it fails, keep the failing command output and asset names, but
do not paste private paths or credential values into public issues.

## Public Release Evidence

Use [Public Release Verification](release/PUBLIC-RELEASE-VERIFICATION.md) to
inspect hosted post-release smoke and consumer evidence. The AO2 `v0.5.0`
stable release is paired with AO2 Control Plane `v0.1.15`.
