from pathlib import Path


REPO = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (REPO / path).read_text(encoding="utf-8")


def test_ao2_cli_uses_in_repo_runtime_without_standalone_ao_runtime_dependency():
    root_cargo = read("Cargo.toml")
    cli_cargo = read("crates/ao2-cli/Cargo.toml")
    runtime_cargo = read("crates/ao2-runtime/Cargo.toml")
    cli_main = read("crates/ao2-cli/src/main.rs")
    lockfile = read("Cargo.lock")
    readme = read("README.md")

    assert '"crates/ao2-runtime"' in root_cargo
    assert 'name = "ao2-runtime"' in runtime_cargo
    assert 'ao2-runtime = { path = "../ao2-runtime" }' in cli_cargo
    assert "use ao2_runtime::" in cli_main

    assert 'name = "ao2-runtime"' in lockfile
    assert 'name = "ao-runtime"' not in lockfile
    assert 'name = "ao-operator"' not in lockfile
    assert "AO2 Native Runtime And Platform Evidence" in readme
    assert (
        "AO2 does not depend on the deprecated standalone `ao-runtime` repository"
        in readme
    )


def test_ci_proves_ubuntu_and_windows_release_runtime_smoke_evidence():
    ci = read(".github/workflows/ci.yml")
    windows_smoke = read(".github/workflows/windows-release-smoke.yml")
    python_guard = read("scripts/ci-python-guard-artifacts.sh")

    assert "tests/test_ao2_native_runtime_platform_evidence.py" in python_guard
    assert "release-archive-hosted-smoke" in ci
    assert "os: [ubuntu-latest, macos-latest, windows-latest]" in ci
    assert "scripts/release-archive-hosted-smoke.sh" in ci
    assert "./scripts/release-archive-hosted-smoke.ps1" in ci
    assert "runs-on: windows-latest" in windows_smoke
    assert "./scripts/smoke-windows-release.ps1" in windows_smoke


def test_branch_protection_requires_hosted_release_archive_smoke_for_platforms():
    verifier = read("scripts/verify-branch-protection.sh")
    runbook = read("docs/BRANCH-PROTECTION.md")

    for check_name in [
        "Release archive hosted smoke ubuntu-latest",
        "Release archive hosted smoke macos-latest",
        "Release archive hosted smoke windows-latest",
    ]:
        assert check_name in verifier
        assert check_name in runbook
