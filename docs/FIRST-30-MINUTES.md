# First 30 Minutes With AO2

This guide starts from the public AO2 `v0.5.11` release and ends with a local
governed demo run. It does not require provider API keys, a control-plane
server, release access, or contact with other users.

## 1. Download and verify AO2

Download the public release assets into an empty directory. If your GitHub CLI
is already authenticated, this is the shortest path:

```sh
mkdir -p ao2-stable
cd ao2-stable
gh release download v0.5.11 --repo uesugitorachiyo/ao2
shasum -a 256 -c SHA256SUMS
```

If `gh release download` asks you to run `gh auth login`, use direct public
release URLs instead. Choose one archive for your host and download
`SHA256SUMS` beside it:

```sh
mkdir -p ao2-stable
cd ao2-stable
base_url="https://github.com/uesugitorachiyo/ao2/releases/download/v0.5.11"
curl -fLO "$base_url/SHA256SUMS"
curl -fLO "$base_url/ao2-0.5.11-macos-aarch64.tar.gz"
grep '  ao2-0.5.11-macos-aarch64.tar.gz$' SHA256SUMS > SHA256SUMS.selected
shasum -a 256 -c SHA256SUMS.selected
```

Choose the archive for your host:

- `ao2-0.5.11-macos-aarch64.tar.gz`
- `ao2-0.5.11-linux-x86_64.tar.gz`
- `ao2-0.5.11-windows-x86_64.tar.gz`

These are the supported public archives. Linux aarch64 hosts may use the Linux
x86_64 archive only under explicit Docker emulation; no Linux aarch64 archive
is published for `v0.5.11`.

On macOS or Linux:

```sh
tar -xzf ao2-0.5.11-<platform>.tar.gz
./verify-release.sh
AO2_INSTALL_DIR="$HOME/.local/bin" ./install.sh
export PATH="$HOME/.local/bin:$PATH"
```

On Windows PowerShell, extract `ao2-0.5.11-windows-x86_64.tar.gz`, then run:

```powershell
.\Verify-Release.ps1
.\install.ps1
```

## 2. Confirm the installed binary

```sh
ao2 version --json
ao2 doctor --json
```

The installed version should report `0.5.11`. `ao2 doctor --json` should include
install verification evidence when the binary came from the signed public
archive.

## 3. Run the governed demo

From an AO2 checkout:

```sh
git clone https://github.com/uesugitorachiyo/ao2.git
cd ao2
tmpdir=$(mktemp -d /tmp/ao2-demo.XXXXXX)
cp -R fixtures/discount-service "$tmpdir/discount-service"
ao2 run examples/risky-pr-run/risky-pr.yaml \
  --target "$tmpdir/discount-service" \
  --run-id demo-run
```

Inspect the retained run evidence:

```sh
ao2 runs show demo-run --target "$tmpdir/discount-service" --json
ao2 report demo-run --target "$tmpdir/discount-service"
```

The demo success signal is `status=Accepted` from `ao2 run` and an accepted run
with `digest_failures` set to `0` in `ao2 runs show`. This first local demo does
not use a live provider transcript, so a nested `provider_score.verdict` of
`fail` is not an install failure.

## 4. Know the support path

- Install/update/rollback: [Install And Update Guide](INSTALL.md). For a first
  operator install, use the download, install, update, rollback, and uninstall
  sections only.
- Common failures: [Troubleshooting](TROUBLESHOOTING.md)
- Public release evidence: [Public Release Verification](release/PUBLIC-RELEASE-VERIFICATION.md)
- Compatible stable companion: [AO2 Control Plane v0.1.19](https://github.com/uesugitorachiyo/ao2-control-plane/releases/tag/v0.1.19)

Open an issue with the AO2 version, host OS, command, and redacted error output
if the public archive verifies but install or the governed demo fails.
