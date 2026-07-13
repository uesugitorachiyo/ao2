from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tarfile

import pytest


VERSION = "0.5.0-beta.1"
TAG = f"v{VERSION}"
RUNTIME_COMMIT = "2333b4a2242d5993d4f5d5b5ac2fc28b1b58a8cb"
TARGETS = (
    "linux-aarch64",
    "linux-x86_64",
    "macos-aarch64",
    "windows-x86_64",
)
ASSET_NAMES = (
    "SHA256SUMS",
    *(
        name
        for target in TARGETS
        for name in (
            f"ao2-{VERSION}-{target}.sbom.cdx.json",
            f"ao2-{VERSION}-{target}.tar.gz",
            f"ao2-{VERSION}-{target}.tar.gz.sha256",
            f"ao2-{VERSION}-{target}.tar.gz.sig",
        )
    ),
    "ao2-release-artifact-closure-index.json",
    "ao2-release-provenance.json",
    "ao2-release-provenance.json.sig",
    "ao2-release-readiness-summary.json",
    "ao2-release-signing-public.pem",
    "ao2-release-train-control-plane-bridge-summary.json",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def build_archive(root: Path, publication: Path, target: str) -> Path:
    payload = root / f"payload-{target}"
    (payload / "bin").mkdir(parents=True)
    binary = "ao2.exe" if target == "windows-x86_64" else "ao2"
    (payload / "bin" / binary).write_bytes(f"ao2 fixture {target}\n".encode())
    (payload / "VERSION").write_text(f"{VERSION}\n")
    write_json(
        payload / "BUILD-PROVENANCE.json",
        {
            "build_profile": "release",
            "git_commit": RUNTIME_COMMIT,
            "package": "ao2",
            "schema_version": "ao2.build-provenance.v1",
            "target": target,
            "version": VERSION,
        },
    )
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "name": "ao2",
                "version": VERSION,
            }
        },
        "components": [],
    }
    write_json(payload / "SBOM.cdx.json", sbom)
    write_json(publication / f"ao2-{VERSION}-{target}.sbom.cdx.json", sbom)
    (payload / "LICENSE").write_text("Apache License\n")
    (payload / "NOTICE").write_text("AO2 fixture notice\n")
    (payload / "README.txt").write_text(
        f"AO2 {VERSION}\nVerify before installing. See UNINSTALL.txt.\n"
    )
    (payload / "UNINSTALL.txt").write_text(
        "Remove ao2, ao2.rollback, and ao2.install-verification.json.\n"
    )
    (payload / "install.sh").write_text("#!/bin/sh\nset -eu\n# install fixture\n")
    (payload / "install.ps1").write_text(
        '$ErrorActionPreference = "Stop"\n# install fixture\n'
    )
    (payload / "verify-release.sh").write_text(
        "#!/bin/sh\nset -eu\n# offline verification fixture\n"
    )
    (payload / "Verify-Release.ps1").write_text(
        '$ErrorActionPreference = "Stop"\n# offline verification fixture\n'
    )
    write_json(
        payload / "RELEASE-VERIFICATION.json",
        {
            "binary": binary,
            "binary_path": f"bin/{binary}",
            "checksum_file": "SHA256SUMS",
            "control_plane_approves_release": False,
            "mutates_ao_artifacts": False,
            "provider_api_keys_required": False,
            "schema_version": "ao2.release-archive-offline-verification.v1",
            "status": "packaged",
            "target": target,
            "version": VERSION,
        },
    )
    files = {
        "BUILD-PROVENANCE.json",
        "LICENSE",
        "NOTICE",
        "README.txt",
        "RELEASE-MANIFEST.json",
        "RELEASE-VERIFICATION.json",
        "SBOM.cdx.json",
        "SHA256SUMS",
        "UNINSTALL.txt",
        "VERSION",
        "Verify-Release.ps1",
        f"bin/{binary}",
        "install.ps1",
        "install.sh",
        "verify-release.sh",
    }
    write_json(
        payload / "RELEASE-MANIFEST.json",
        {
            "binary": binary,
            "binary_path": f"bin/{binary}",
            "files": sorted(files),
            "installers": ["install.sh", "install.ps1"],
            "legal_files": ["LICENSE", "NOTICE"],
            "schema_version": "ao2.release-manifest.v1",
            "target": target,
            "uninstall": "UNINSTALL.txt",
            "verifiers": ["verify-release.sh", "Verify-Release.ps1"],
            "version": VERSION,
        },
    )
    checksums = []
    for path in sorted(p for p in payload.rglob("*") if p.is_file()):
        name = path.relative_to(payload).as_posix()
        if name != "SHA256SUMS":
            checksums.append(f"{sha256(path)}  {name}")
    (payload / "SHA256SUMS").write_text("\n".join(checksums) + "\n")
    archive = publication / f"ao2-{VERSION}-{target}.tar.gz"
    with tarfile.open(archive, "w:gz") as bundle:
        for path in sorted(p for p in payload.rglob("*") if p.is_file()):
            bundle.add(path, arcname=path.relative_to(payload).as_posix())
    return archive


def make_executable(path: Path, text: str) -> None:
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


@pytest.fixture(scope="module")
def approved_fixture(tmp_path_factory: pytest.TempPathFactory) -> Path:
    root = tmp_path_factory.mktemp("approved-promotion")
    publication = root / "publication"
    publication.mkdir()
    openssl = shutil.which("openssl")
    assert openssl, "OpenSSL is required for promotion tests"
    private_key = root / "fixture-private.pem"
    public_key = publication / "ao2-release-signing-public.pem"
    subprocess.run(
        [openssl, "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", private_key],
        check=True,
        capture_output=True,
    )
    subprocess.run(
        [openssl, "pkey", "-in", private_key, "-pubout", "-out", public_key],
        check=True,
        capture_output=True,
    )
    archives = []
    for target in TARGETS:
        archive = build_archive(root, publication, target)
        archives.append(archive)
        (publication / f"{archive.name}.sha256").write_text(
            f"{sha256(archive)}  {archive.name}\n"
        )
        subprocess.run(
            [
                openssl,
                "dgst",
                "-sha256",
                "-sign",
                private_key,
                "-out",
                publication / f"{archive.name}.sig",
                archive,
            ],
            check=True,
            capture_output=True,
        )
    provenance = publication / "ao2-release-provenance.json"
    write_json(
        provenance,
        {
            "archives": [
                {
                    "checksum": f"{archive.name}.sha256",
                    "name": archive.name,
                    "sha256": sha256(archive),
                    "signature": f"{archive.name}.sig",
                }
                for archive in archives
            ],
            "git_commit": RUNTIME_COMMIT,
            "package": "ao2",
            "release_tag": TAG,
            "schema_version": "ao2.release-provenance.v1",
            "signature_algorithm": "RSA-2048/SHA-256 test fixture",
            "version": VERSION,
        },
    )
    subprocess.run(
        [
            openssl,
            "dgst",
            "-sha256",
            "-sign",
            private_key,
            "-out",
            publication / "ao2-release-provenance.json.sig",
            provenance,
        ],
        check=True,
        capture_output=True,
    )
    for name in (
        "ao2-release-artifact-closure-index.json",
        "ao2-release-readiness-summary.json",
        "ao2-release-train-control-plane-bridge-summary.json",
    ):
        write_json(publication / name, {"status": "passed"})
    sums = []
    for name in ASSET_NAMES:
        if name != "SHA256SUMS":
            sums.append(f"{sha256(publication / name)}  {name}")
    (publication / "SHA256SUMS").write_text("\n".join(sums) + "\n")
    publication_list = root / "publication.assets.txt"
    publication_list.write_text("\n".join(ASSET_NAMES) + "\n")
    manifest = root / "approved-assets.sha256"
    manifest.write_text(
        "\n".join(f"{sha256(publication / name)}  {name}" for name in ASSET_NAMES)
        + "\n"
    )
    notes = root / "notes.md"
    notes.write_text(
        f"# AO2 {TAG} External Beta\n\n"
        "This external beta is not AO2 1.0.\n\n"
        "Upgrade with `ao2 install update`; rollback with `ao2 install rollback`.\n"
    )
    metadata = {
        "publication": str(publication),
        "publication_list": str(publication_list),
        "manifest": str(manifest),
        "manifest_sha256": sha256(manifest),
        "notes": str(notes),
        "notes_sha256": sha256(notes),
    }
    write_json(root / "fixture.json", metadata)
    return root


@pytest.fixture
def promotion_case(tmp_path: Path, approved_fixture: Path) -> dict[str, object]:
    fixture = tmp_path / "fixture"
    shutil.copytree(approved_fixture, fixture)
    metadata = json.loads((fixture / "fixture.json").read_text())
    for key in ("publication", "publication_list", "manifest", "notes"):
        metadata[key] = str(fixture / Path(metadata[key]).relative_to(approved_fixture))
    fake_bin = tmp_path / "bin"
    fake_bin.mkdir()
    command_log = tmp_path / "commands.log"
    make_executable(
        fake_bin / "git",
        """#!/bin/sh
set -eu
printf 'git %s\n' "$*" >> "$AO2_TEST_COMMAND_LOG"
case "$1" in
  cat-file)
    [ "${FAKE_GIT_TARGET_MISSING:-0}" = "0" ]
    ;;
  status)
    exit 0
    ;;
  remote)
    printf '%s\n' "${FAKE_GIT_REMOTE_URL:-https://github.com/uesugitorachiyo/ao2.git}"
    ;;
  rev-parse)
    case "$*" in
      *refs/tags/*)
        [ "${FAKE_GIT_TAG_EXISTS:-0}" = "1" ] && echo tag-object && exit 0
        exit 1
        ;;
      *origin/main*) echo implementation-head ;;
      *) echo implementation-head ;;
    esac
    ;;
  ls-remote)
    [ "${FAKE_GIT_TAG_EXISTS:-0}" = "1" ] && echo "tag-object refs/tags/v0.5.0-beta.1" && exit 0
    exit 2
    ;;
  tag)
    printf 'MUTATION git-tag %s\n' "$*" >> "$AO2_TEST_COMMAND_LOG"
    ;;
  push)
    printf 'MUTATION git-push %s\n' "$*" >> "$AO2_TEST_COMMAND_LOG"
    ;;
esac
""",
    )
    make_executable(
        fake_bin / "gh",
        """#!/bin/sh
set -eu
printf 'gh %s\n' "$*" >> "$AO2_TEST_COMMAND_LOG"
case "$1 $2" in
  'release view')
    [ "${FAKE_GH_RELEASE_EXISTS:-0}" = "1" ] && exit 0
    exit 1
    ;;
  'api repos/uesugitorachiyo/ao2/releases/latest')
    printf 'v0.4.81\n'
    ;;
  'release create')
    printf 'MUTATION gh-release-create %s\n' "$*" >> "$AO2_TEST_COMMAND_LOG"
    printf 'https://example.invalid/release\n'
    ;;
  *)
    printf 'unexpected fake gh command: %s\n' "$*" >&2
    exit 97
    ;;
esac
""",
    )
    metadata.update(
        {
            "fake_bin": fake_bin,
            "command_log": command_log,
            "env": {
                "PATH": f"{fake_bin}{os.pathsep}{os.environ['PATH']}",
                "AO2_TEST_COMMAND_LOG": str(command_log),
                "AO2_VERSION": VERSION,
                "AO2_RELEASE_REPO": "uesugitorachiyo/ao2",
                "AO2_RELEASE_TAG": TAG,
                "AO2_RELEASE_TARGET_COMMIT": RUNTIME_COMMIT,
                "AO2_RELEASE_CHANNEL": "prerelease",
                "AO2_RELEASE_TITLE": f"AO2 {TAG} External Beta",
                "AO2_RELEASE_NOTES_FILE": metadata["notes"],
                "AO2_RELEASE_NOTES_SHA256": metadata["notes_sha256"],
                "AO2_RELEASE_PUBLICATION_DIR": metadata["publication"],
                "AO2_RELEASE_PUBLICATION_LIST": metadata["publication_list"],
                "AO2_RELEASE_EXPECTED_ASSET_MANIFEST": metadata["manifest"],
                "AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256": metadata[
                    "manifest_sha256"
                ],
                "AO2_RELEASE_EXPECTED_LATEST_STABLE_TAG": "v0.4.81",
                "AO2_RELEASE_PUBLISH_APPROVED_MODE": "dry-run",
                "AO2_RELEASE_PUBLISH_APPROVED_CONFIRM": (
                    f"publish-approved-{TAG}-{metadata['manifest_sha256']}"
                ),
            },
        }
    )
    return metadata


def run_publisher(
    case: dict[str, object],
    *,
    env_changes: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    root = Path(__file__).resolve().parents[1]
    env = os.environ.copy()
    env.update(case["env"])
    if env_changes:
        env.update(env_changes)
    return subprocess.run(
        ["bash", str(root / "scripts/release-publish-approved-assets.sh")],
        cwd=root,
        env=env,
        text=True,
        capture_output=True,
    )


def assert_no_mutation(case: dict[str, object]) -> None:
    log = Path(case["command_log"])
    if log.exists():
        assert "MUTATION " not in log.read_text()


def test_package_command_exposes_approved_asset_publisher() -> None:
    root = Path(__file__).resolve().parents[1]
    package = json.loads((root / "package.json").read_text())
    assert package["scripts"]["release:publish-approved"] == (
        "node scripts/run-sh-script.js scripts/release-publish-approved-assets.sh"
    )


def test_dry_run_verifies_complete_set_without_mutation(promotion_case: dict[str, object]) -> None:
    result = run_publisher(promotion_case)
    assert result.returncode == 0, result.stderr
    assert "release_publish_approved_assets_dry_run=passed" in result.stdout
    assert "release_approved_asset_count=23" in result.stdout
    assert "release_approved_artifact_signatures=passed" in result.stdout
    assert "release_approved_artifact_contents=passed" in result.stdout
    assert_no_mutation(promotion_case)


def test_exact_approved_live_promotion_uses_prerelease_and_exact_assets(
    promotion_case: dict[str, object],
) -> None:
    result = run_publisher(
        promotion_case,
        env_changes={"AO2_RELEASE_PUBLISH_APPROVED_MODE": "live"},
    )
    assert result.returncode == 0, result.stderr
    log = Path(promotion_case["command_log"]).read_text().splitlines()
    mutation_lines = [line for line in log if line.startswith("MUTATION ")]
    assert [line.split()[1] for line in mutation_lines] == [
        "git-tag",
        "git-push",
        "gh-release-create",
    ]
    release = mutation_lines[-1]
    assert "--prerelease" in release
    assert "--latest=false" in release
    observed_assets = {
        Path(token).name
        for token in release.split()
        if token.startswith(str(promotion_case["publication"]))
    }
    assert observed_assets == set(ASSET_NAMES)
    assert len(observed_assets) == 23


@pytest.mark.parametrize(
    ("change", "expected_error"),
    [
        (
            {
                "AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256": "0" * 64,
                "AO2_RELEASE_PUBLISH_APPROVED_CONFIRM": (
                    f"publish-approved-{TAG}-{'0' * 64}"
                ),
            },
            "approved manifest SHA-256 mismatch",
        ),
        (
            {"AO2_RELEASE_NOTES_SHA256": "0" * 64},
            "release notes SHA-256 mismatch",
        ),
        (
            {"AO2_RELEASE_TARGET_COMMIT": "f" * 40, "FAKE_GIT_TARGET_MISSING": "1"},
            "runtime target commit does not exist",
        ),
        (
            {"FAKE_GIT_TAG_EXISTS": "1"},
            "refusing to reuse existing release tag",
        ),
        (
            {"FAKE_GH_RELEASE_EXISTS": "1"},
            "refusing to overwrite existing release",
        ),
        (
            {"AO2_RELEASE_CHANNEL": "stable"},
            "prerelease version requires AO2_RELEASE_CHANNEL=prerelease",
        ),
        (
            {"AO2_RELEASE_PUBLISH_APPROVED_CONFIRM": "wrong"},
            "exact approved promotion confirmation",
        ),
        (
            {"FAKE_GIT_REMOTE_URL": "https://github.com/example/not-ao2.git"},
            "origin remote does not match release repository",
        ),
    ],
)
def test_failed_preflight_never_reaches_mutation(
    promotion_case: dict[str, object], change: dict[str, str], expected_error: str
) -> None:
    change = {**change, "AO2_RELEASE_PUBLISH_APPROVED_MODE": "live"}
    result = run_publisher(promotion_case, env_changes=change)
    assert result.returncode != 0
    assert expected_error in result.stderr
    assert_no_mutation(promotion_case)


def test_changed_asset_fails_before_mutation(promotion_case: dict[str, object]) -> None:
    archive = Path(promotion_case["publication"]) / f"ao2-{VERSION}-linux-aarch64.tar.gz"
    archive.write_bytes(archive.read_bytes() + b"drift")
    result = run_publisher(
        promotion_case,
        env_changes={"AO2_RELEASE_PUBLISH_APPROVED_MODE": "live"},
    )
    assert result.returncode != 0
    assert "approved asset hash mismatch" in result.stderr
    assert_no_mutation(promotion_case)


@pytest.mark.parametrize("kind", ["missing", "extra"])
def test_missing_or_extra_asset_fails_before_mutation(
    promotion_case: dict[str, object], kind: str
) -> None:
    publication = Path(promotion_case["publication"])
    if kind == "missing":
        (publication / ASSET_NAMES[1]).unlink()
        expected = "approved staged asset is missing"
    else:
        (publication / "unexpected.bin").write_bytes(b"extra")
        expected = "publication directory has unlisted asset"
    result = run_publisher(
        promotion_case,
        env_changes={"AO2_RELEASE_PUBLISH_APPROVED_MODE": "live"},
    )
    assert result.returncode != 0
    assert expected in result.stderr
    assert_no_mutation(promotion_case)


def test_changed_release_notes_file_fails_before_mutation(
    promotion_case: dict[str, object],
) -> None:
    Path(promotion_case["notes"]).write_text("changed notes\n")
    result = run_publisher(
        promotion_case,
        env_changes={"AO2_RELEASE_PUBLISH_APPROVED_MODE": "live"},
    )
    assert result.returncode != 0
    assert "release notes SHA-256 mismatch" in result.stderr
    assert_no_mutation(promotion_case)


def test_publisher_contains_no_build_sign_package_provider_or_overwrite_path() -> None:
    root = Path(__file__).resolve().parents[1]
    script = (root / "scripts/release-publish-approved-assets.sh").read_text().lower()
    forbidden = (
        "release:build-all",
        "cargo build",
        "docker",
        "qemu",
        "ssh ",
        "scp ",
        "package-linux",
        "package-windows",
        "release-stage-publication-assets",
        "release-sign-provenance",
        ".release-signing",
        "release:ship",
        "smoke:provider",
        "--clobber",
        "gh release upload",
    )
    for token in forbidden:
        assert token not in script, f"forbidden promotion operation: {token}"


def test_existing_publication_contract_keeps_signing_material_required_by_default() -> None:
    root = Path(__file__).resolve().parents[1]
    contract = (root / "scripts/release-publication-contract.sh").read_text()
    assert 'AO2_RELEASE_CONTRACT_MODE="build-and-publish"' in contract
    assert '--promote-approved-assets) AO2_RELEASE_CONTRACT_MODE="promote-approved-assets"' in contract
    assert 'AO2_RELEASE_PRIVATE_KEY' in contract
    publisher = (root / "scripts/release-publish-approved-assets.sh").read_text()
    assert 'release-publication-contract.sh" --promote-approved-assets' in publisher
