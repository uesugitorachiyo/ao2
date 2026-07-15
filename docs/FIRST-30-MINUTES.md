# First 30 Minutes With AO2

This guide starts from the public AO2 `v0.5.0` release and ends with a local
governed demo run. It does not require provider API keys, a control-plane
server, release publication access, or external pilot work.

## 1. Download and verify AO2

Install the GitHub CLI if you do not already have it, then download the public
release assets:

```sh
mkdir -p ao2-stable
cd ao2-stable
gh release download v0.5.0 --repo uesugitorachiyo/ao2
shasum -a 256 -c SHA256SUMS
```

Choose the archive for your host:

- `ao2-0.5.0-macos-aarch64.tar.gz`
- `ao2-0.5.0-linux-x86_64.tar.gz`
- `ao2-0.5.0-linux-aarch64.tar.gz`
- `ao2-0.5.0-windows-x86_64.tar.gz`

On macOS or Linux:

```sh
tar -xzf ao2-0.5.0-<platform>.tar.gz
./verify-release.sh
AO2_INSTALL_DIR="$HOME/.local/bin" ./install.sh
```

On Windows PowerShell, extract `ao2-0.5.0-windows-x86_64.tar.gz`, then run:

```powershell
.\Verify-Release.ps1
.\install.ps1
```

## 2. Confirm the installed binary

```sh
ao2 version --json
ao2 doctor --json
```

The installed version should report `0.5.0`. `ao2 doctor --json` should include
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

## 4. Know the support path

- Install/update/rollback: [Install And Update Guide](INSTALL.md)
- Common failures: [Troubleshooting](TROUBLESHOOTING.md)
- Public release evidence: [Public Release Verification](release/PUBLIC-RELEASE-VERIFICATION.md)
- Compatible stable companion: [AO2 Control Plane v0.1.15](https://github.com/uesugitorachiyo/ao2-control-plane/releases/tag/v0.1.15)

Open an issue with the AO2 version, host OS, command, and redacted error output
if the public archive verifies but install or the governed demo fails.
