from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
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
    fake_state = tmp_path / "fake-state"
    fake_state.mkdir()
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
    count_file="$AO2_TEST_FAKE_STATE/status-count"
    count=0
    [ ! -f "$count_file" ] || count=$(cat "$count_file")
    count=$((count + 1))
    printf '%s\n' "$count" > "$count_file"
    [ "${FAKE_GIT_STATUS_ERROR_ON_CALL:-0}" != "$count" ] || exit 91
    [ "${FAKE_GIT_DIRTY_ON_STATUS_CALL:-0}" != "$count" ] || printf ' M changed-after-verification\n'
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
      *origin/main*)
        count_file="$AO2_TEST_FAKE_STATE/origin-count"
        count=0
        [ ! -f "$count_file" ] || count=$(cat "$count_file")
        count=$((count + 1))
        printf '%s\n' "$count" > "$count_file"
        if [ "${FAKE_GIT_ORIGIN_MOVES_ON_CALL:-0}" = "$count" ]; then
          echo moved-origin-head
        else
          echo implementation-head
        fi
        ;;
      *)
        count_file="$AO2_TEST_FAKE_STATE/head-count"
        count=0
        [ ! -f "$count_file" ] || count=$(cat "$count_file")
        count=$((count + 1))
        printf '%s\n' "$count" > "$count_file"
        if [ "${FAKE_GIT_HEAD_MOVES_ON_CALL:-0}" = "$count" ]; then
          echo moved-publisher-head
        else
          echo implementation-head
        fi
        ;;
    esac
    ;;
  ls-remote)
    count_file="$AO2_TEST_FAKE_STATE/tag-lookup-count"
    count=0
    [ ! -f "$count_file" ] || count=$(cat "$count_file")
    count=$((count + 1))
    printf '%s\n' "$count" > "$count_file"
    [ "${FAKE_GIT_TAG_LOOKUP_ERROR:-0}" = "0" ] || exit 128
    [ "${FAKE_GIT_TAG_LOOKUP_ERROR_ON_CALL:-0}" != "$count" ] || exit 128
    [ "${FAKE_GIT_TAG_EXISTS:-0}" = "1" ] && echo "tag-object refs/tags/v0.5.0-beta.1" && exit 0
    exit 2
    ;;
  show)
    case "$2" in
      *:scripts/release-verify-approved-assets.py)
        cat "$AO2_TEST_REPO_ROOT/scripts/release-verify-approved-assets.py"
        ;;
      *:scripts/release-publication-contract.sh)
        cat "$AO2_TEST_REPO_ROOT/scripts/release-publication-contract.sh"
        ;;
      *) exit 98 ;;
    esac
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
    [ "${FAKE_GH_RELEASE_LOOKUP_ERROR:-0}" = "0" ] || exit 92
    [ "${FAKE_GH_RELEASE_EXISTS:-0}" = "1" ] && exit 0
    exit 1
    ;;
  'api --include')
    count_file="$AO2_TEST_FAKE_STATE/release-lookup-count"
    count=0
    [ ! -f "$count_file" ] || count=$(cat "$count_file")
    count=$((count + 1))
    printf '%s\n' "$count" > "$count_file"
    [ "${FAKE_GH_RELEASE_LOOKUP_ERROR:-0}" = "0" ] || {
      printf '%s\n' "${FAKE_GH_RELEASE_LOOKUP_ERROR_TEXT:-network unavailable}" >&2
      exit 92
    }
    [ "${FAKE_GH_RELEASE_LOOKUP_ERROR_ON_CALL:-0}" != "$count" ] || {
      printf '%s\n' "${FAKE_GH_RELEASE_LOOKUP_ERROR_TEXT:-network unavailable}" >&2
      exit 92
    }
    if [ "${FAKE_GH_RELEASE_EXISTS:-0}" = "1" ]; then
      printf 'HTTP/2.0 200 OK\n\n{}\n'
      exit 0
    fi
    printf 'HTTP/2.0 404 Not Found\n\n{}\n' >&2
    exit 1
    ;;
  'api repos/uesugitorachiyo/ao2/releases/latest')
    count_file="$AO2_TEST_FAKE_STATE/latest-count"
    count=0
    [ ! -f "$count_file" ] || count=$(cat "$count_file")
    count=$((count + 1))
    printf '%s\n' "$count" > "$count_file"
    [ "${FAKE_GH_LATEST_ERROR:-0}" = "0" ] || exit 93
    [ "${FAKE_GH_LATEST_ERROR_ON_CALL:-0}" != "$count" ] || exit 93
    if [ -f "$AO2_TEST_FAKE_STATE/release-created" ]; then
      printf '%s\n' "${FAKE_GH_LATEST_AFTER_CREATE:-v0.4.81}"
    else
      printf '%s\n' "${FAKE_GH_LATEST_TAG:-v0.4.81}"
    fi
    ;;
  'release create')
    printf 'MUTATION gh-release-create %s\n' "$*" >> "$AO2_TEST_COMMAND_LOG"
    : > "$AO2_TEST_FAKE_STATE/release-created"
    printf 'https://example.invalid/release\n'
    ;;
  *)
    printf 'unexpected fake gh command: %s\n' "$*" >&2
    exit 97
    ;;
esac
""",
    )
    make_executable(
        fake_bin / "python3",
        f"""#!{sys.executable}
import os
from pathlib import Path
import subprocess
import sys

args = sys.argv[1:]
if args and args[0].endswith("release-verify-approved-assets.py"):
    state = Path(os.environ["AO2_TEST_FAKE_STATE"])
    count_file = state / "verifier-count"
    count = int(count_file.read_text()) + 1 if count_file.exists() else 1
    count_file.write_text(str(count))
    should_fail = count == int(os.environ.get("FAKE_VERIFIER_FAIL_ON_CALL", "0"))
    if should_fail:
        publication = Path(args[args.index("--publication-dir") + 1])
        target = publication / "ao2-0.5.0-beta.1-linux-aarch64.tar.gz"
        target.chmod(0o600)
        target.write_bytes(target.read_bytes() + b"stage drift")
    result = subprocess.run([{sys.executable!r}, *args])
    if (
        result.returncode == 0
        and count == int(os.environ.get("FAKE_MUTATE_SOURCE_AFTER_VERIFIER_CALL", "0"))
    ):
        source_publication = Path(os.environ["AO2_TEST_SOURCE_PUBLICATION"])
        source_asset = source_publication / "ao2-0.5.0-beta.1-linux-aarch64.tar.gz"
        source_asset.write_bytes(source_asset.read_bytes() + b"source drift after snapshot")
        Path(os.environ["AO2_TEST_SOURCE_NOTES"]).write_text("changed original notes\\n")
        Path(os.environ["AO2_TEST_SOURCE_LIST"]).write_text("changed-original-list\\n")
    raise SystemExit(result.returncode)
os.execv({sys.executable!r}, [{sys.executable!r}, *args])
""",
    )
    metadata.update(
        {
            "fake_bin": fake_bin,
            "command_log": command_log,
            "env": {
                "PATH": f"{fake_bin}{os.pathsep}{os.environ['PATH']}",
                "AO2_TEST_COMMAND_LOG": str(command_log),
                "AO2_TEST_FAKE_STATE": str(fake_state),
                "AO2_TEST_REPO_ROOT": str(Path(__file__).resolve().parents[1]),
                "AO2_TEST_SOURCE_PUBLICATION": metadata["publication"],
                "AO2_TEST_SOURCE_NOTES": metadata["notes"],
                "AO2_TEST_SOURCE_LIST": metadata["publication_list"],
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
        if Path(token).name in set(ASSET_NAMES)
    }
    assert observed_assets == set(ASSET_NAMES)
    assert len(observed_assets) == 23
    assert str(promotion_case["publication"]) not in release
    assert f"--notes-file {promotion_case['notes']}" not in release


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
        (
            {"FAKE_GIT_TAG_LOOKUP_ERROR": "1"},
            "remote tag lookup failed",
        ),
        (
            {"FAKE_GH_RELEASE_LOOKUP_ERROR": "1"},
            "GitHub release lookup failed",
        ),
        (
            {"FAKE_GH_LATEST_TAG": "v0.4.80"},
            "latest stable release mismatch",
        ),
        (
            {"FAKE_GH_LATEST_ERROR": "1"},
            "latest stable release lookup failed",
        ),
        (
            {"FAKE_GIT_STATUS_ERROR_ON_CALL": "1"},
            "git status failed",
        ),
        (
            {"FAKE_GIT_DIRTY_ON_STATUS_CALL": "2"},
            "dirty worktree",
        ),
        (
            {"FAKE_GIT_HEAD_MOVES_ON_CALL": "2"},
            "publisher implementation HEAD changed",
        ),
        (
            {"FAKE_GIT_ORIGIN_MOVES_ON_CALL": "2"},
            "origin/main changed",
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


def test_pre_tag_verifier_failure_never_reaches_mutation(
    promotion_case: dict[str, object],
) -> None:
    result = run_publisher(
        promotion_case,
        env_changes={
            "AO2_RELEASE_PUBLISH_APPROVED_MODE": "live",
            "FAKE_VERIFIER_FAIL_ON_CALL": "3",
        },
    )
    assert result.returncode != 0
    assert "approved asset hash mismatch" in result.stderr
    assert_no_mutation(promotion_case)


def test_post_publication_latest_stable_drift_is_reported(
    promotion_case: dict[str, object],
) -> None:
    result = run_publisher(
        promotion_case,
        env_changes={
            "AO2_RELEASE_PUBLISH_APPROVED_MODE": "live",
            "FAKE_GH_LATEST_AFTER_CREATE": TAG,
        },
    )
    assert result.returncode != 0
    assert "latest stable release mismatch" in result.stderr
    log = Path(promotion_case["command_log"]).read_text()
    assert "MUTATION gh-release-create" in log


def test_publisher_executes_helpers_from_the_bound_commit(
    promotion_case: dict[str, object],
) -> None:
    result = run_publisher(promotion_case)
    assert result.returncode == 0, result.stderr
    log = Path(promotion_case["command_log"]).read_text()
    assert "git show implementation-head:scripts/release-verify-approved-assets.py" in log
    assert "git show implementation-head:scripts/release-publication-contract.sh" in log
    assert_no_mutation(promotion_case)


def test_original_inputs_can_drift_after_snapshot_without_changing_published_bytes(
    promotion_case: dict[str, object],
) -> None:
    result = run_publisher(
        promotion_case,
        env_changes={
            "AO2_RELEASE_PUBLISH_APPROVED_MODE": "live",
            "FAKE_MUTATE_SOURCE_AFTER_VERIFIER_CALL": "2",
        },
    )
    assert result.returncode == 0, result.stderr
    assert Path(promotion_case["notes"]).read_text() == "changed original notes\n"
    assert Path(promotion_case["publication_list"]).read_text() == "changed-original-list\n"
    log = Path(promotion_case["command_log"]).read_text()
    assert "MUTATION gh-release-create" in log
    assert str(promotion_case["publication"]) not in next(
        line for line in log.splitlines() if line.startswith("MUTATION gh-release-create")
    )


@pytest.mark.parametrize(
    ("change", "expected_error", "expect_tag_push"),
    [
        ({"FAKE_GIT_TAG_LOOKUP_ERROR_ON_CALL": "2"}, "remote tag lookup failed", False),
        ({"FAKE_GH_RELEASE_LOOKUP_ERROR_ON_CALL": "2"}, "GitHub release lookup failed", False),
        ({"FAKE_GH_LATEST_ERROR_ON_CALL": "2"}, "latest stable release lookup failed", False),
        ({"FAKE_GH_RELEASE_LOOKUP_ERROR_ON_CALL": "3"}, "GitHub release lookup failed", True),
        ({"FAKE_GH_LATEST_ERROR_ON_CALL": "3"}, "latest stable release lookup failed", True),
        ({"FAKE_GIT_STATUS_ERROR_ON_CALL": "4"}, "git status failed", True),
        ({"FAKE_GIT_HEAD_MOVES_ON_CALL": "4"}, "publisher implementation HEAD changed", True),
        ({"FAKE_GIT_STATUS_ERROR_ON_CALL": "5"}, "git status failed", True),
        ({"FAKE_VERIFIER_FAIL_ON_CALL": "4"}, "approved asset hash mismatch", True),
    ],
)
def test_stage_specific_failure_never_reaches_next_mutation(
    promotion_case: dict[str, object],
    change: dict[str, str],
    expected_error: str,
    expect_tag_push: bool,
) -> None:
    result = run_publisher(
        promotion_case,
        env_changes={**change, "AO2_RELEASE_PUBLISH_APPROVED_MODE": "live"},
    )
    assert result.returncode != 0
    assert expected_error in result.stderr
    log = Path(promotion_case["command_log"]).read_text()
    assert ("MUTATION git-push" in log) is expect_tag_push
    assert "MUTATION gh-release-create" not in log


def test_non_http_error_text_containing_404_does_not_fail_open(
    promotion_case: dict[str, object],
) -> None:
    result = run_publisher(
        promotion_case,
        env_changes={
            "AO2_RELEASE_PUBLISH_APPROVED_MODE": "live",
            "FAKE_GH_RELEASE_LOOKUP_ERROR_ON_CALL": "1",
            "FAKE_GH_RELEASE_LOOKUP_ERROR_TEXT": "proxy failed with 404 from an unrelated upstream",
        },
    )
    assert result.returncode != 0
    assert "GitHub release lookup failed" in result.stderr
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


def test_existing_publication_contract_rejects_missing_key_in_default_mode(
    tmp_path: Path,
) -> None:
    root = Path(__file__).resolve().parents[1]
    notes = tmp_path / "notes.md"
    notes.write_text("# External Beta\n\nThis is an external beta.\n")
    env = os.environ.copy()
    env.update(
        {
            "AO2_VERSION": VERSION,
            "AO2_RELEASE_TAG": TAG,
            "AO2_RELEASE_CHANNEL": "prerelease",
            "AO2_RELEASE_TITLE": f"AO2 {TAG} External Beta",
            "AO2_RELEASE_NOTES_FILE": str(notes),
            "AO2_RELEASE_PRIVATE_KEY": str(tmp_path / "missing.pem"),
            "AO2_RELEASE_CONTRACT_REQUIRE_ASSETS": "0",
        }
    )
    default = subprocess.run(
        ["bash", str(root / "scripts/release-publication-contract.sh")],
        cwd=root,
        env=env,
        text=True,
        capture_output=True,
    )
    assert default.returncode != 0
    assert "release signing material is missing" in default.stderr
    promotion = subprocess.run(
        [
            "bash",
            str(root / "scripts/release-publication-contract.sh"),
            "--promote-approved-assets",
        ],
        cwd=root,
        env=env,
        text=True,
        capture_output=True,
    )
    assert promotion.returncode == 0, promotion.stderr
