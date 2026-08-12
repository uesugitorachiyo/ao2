#!/usr/bin/env python3
"""Validate the exact three-platform candidate contract used by hosted releases."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import tarfile
from pathlib import Path, PurePosixPath
from typing import Any


SCHEMA_VERSION = "ao2.hosted-native-candidate-gate.v1"
SUMMARY_SCHEMA = "ao2.release-archive-hosted-smoke.v1"
BUILD_PROVENANCE_SCHEMA = "ao2.build-provenance.v1"
MANIFEST_SCHEMA = "ao2.release-manifest.v1"
VERIFICATION_SCHEMA = "ao2.release-archive-offline-verification.v1"
WINDOWS_OWNERSHIP_SCHEMA = "ao2.windows-coverage-ownership.v1"
MAX_ARCHIVE_BYTES = 100 * 1024 * 1024
MAX_MEMBER_BYTES = 64 * 1024 * 1024
MAX_EXPANDED_BYTES = 192 * 1024 * 1024
TARGETS = {
    "linux-x86_64": {
        "binary": "bin/ao2",
        "runner": "ubuntu-latest",
        "target_triple": "x86_64-unknown-linux-gnu",
    },
    "macos-aarch64": {
        "binary": "bin/ao2",
        "runner": "macos-latest",
        "target_triple": "aarch64-apple-darwin",
    },
    "windows-x86_64": {
        "binary": "bin/ao2.exe",
        "runner": "windows-latest",
        "target_triple": "x86_64-pc-windows-msvc",
    },
}
SUMMARY_KEYS = {
    "archive",
    "control_plane_approves_release",
    "install_verification_evidence",
    "install_verification_schema",
    "installed_binary",
    "mutates_ao_artifacts",
    "provider_api_keys_required",
    "release_acceptance_owner",
    "schema_version",
    "status",
    "target",
    "version",
}
COMMON_ARCHIVE_MEMBERS = {
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
    "install.ps1",
    "install.sh",
    "verify-release.sh",
}


class CandidateValidationError(ValueError):
    """Raised when a hosted native candidate is malformed or mismatched."""


def _json_object(raw: bytes, label: str) -> dict[str, Any]:
    try:
        value = json.loads(raw.decode("utf-8-sig"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise CandidateValidationError(f"invalid {label} JSON") from exc
    if not isinstance(value, dict):
        raise CandidateValidationError(f"{label} must be an object")
    return value


def _require_exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise CandidateValidationError(
            f"{label} has unexpected keys: {sorted(set(value) ^ expected)}"
        )


def _safe_inventory(root: Path) -> list[str]:
    if not root.is_dir() or root.is_symlink():
        raise CandidateValidationError(f"candidate artifact is not a safe directory: {root}")
    inventory = []
    for path in root.rglob("*"):
        if path.is_symlink():
            raise CandidateValidationError(f"candidate artifact contains symlink: {path}")
        if path.is_file():
            inventory.append(path.relative_to(root).as_posix())
        elif not path.is_dir():
            raise CandidateValidationError(f"candidate artifact contains unsafe entry: {path}")
    return sorted(inventory)


def _archive_members(archive: Path, target: str) -> dict[str, bytes]:
    expected = COMMON_ARCHIVE_MEMBERS | {TARGETS[target]["binary"]}
    if not archive.is_file() or archive.is_symlink():
        raise CandidateValidationError(f"missing or unsafe archive for {target}")
    if archive.stat().st_size > MAX_ARCHIVE_BYTES:
        raise CandidateValidationError(f"archive exceeds size limit for {target}")
    files: dict[str, bytes] = {}
    expanded = 0
    try:
        with tarfile.open(archive, "r:gz") as bundle:
            for member in bundle.getmembers():
                path = PurePosixPath(member.name)
                if (
                    not member.isfile()
                    or path.is_absolute()
                    or ".." in path.parts
                    or member.name != path.as_posix()
                    or member.name in files
                ):
                    raise CandidateValidationError(
                        f"unsafe archive member for {target}: {member.name}"
                    )
                if member.size < 0 or member.size > MAX_MEMBER_BYTES:
                    raise CandidateValidationError(
                        f"archive member exceeds size limit for {target}: {member.name}"
                    )
                expanded += member.size
                if expanded > MAX_EXPANDED_BYTES:
                    raise CandidateValidationError(f"archive expansion exceeds limit for {target}")
                source = bundle.extractfile(member)
                if source is None:
                    raise CandidateValidationError(
                        f"could not read archive member for {target}: {member.name}"
                    )
                files[member.name] = source.read()
    except (tarfile.TarError, OSError) as exc:
        raise CandidateValidationError(f"invalid tar archive for {target}") from exc
    if set(files) != expected:
        raise CandidateValidationError(
            f"archive inventory mismatch for {target}: {sorted(set(files) ^ expected)}"
        )
    return files


def _validate_checksums(files: dict[str, bytes], target: str) -> None:
    try:
        lines = files["SHA256SUMS"].decode("ascii").splitlines()
    except UnicodeDecodeError as exc:
        raise CandidateValidationError(f"invalid checksum encoding for {target}") from exc
    checksums: dict[str, str] = {}
    for line in lines:
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9._/-]+)", line)
        if match is None or match.group(2) in checksums:
            raise CandidateValidationError(f"invalid checksum row for {target}")
        checksums[match.group(2)] = match.group(1)
    expected = set(files) - {"SHA256SUMS"}
    if set(checksums) != expected:
        raise CandidateValidationError(f"checksum inventory mismatch for {target}")
    for name, digest in checksums.items():
        if hashlib.sha256(files[name]).hexdigest() != digest:
            raise CandidateValidationError(f"checksum mismatch for {target}: {name}")


def _validate_archive(
    archive: Path,
    target: str,
    source_sha: str,
    version: str,
) -> dict[str, str]:
    files = _archive_members(archive, target)
    provenance = _json_object(files["BUILD-PROVENANCE.json"], f"{target} build provenance")
    _require_exact_keys(
        provenance,
        {"build_profile", "git_commit", "package", "schema_version", "target", "version"},
        f"{target} build provenance",
    )
    expected_provenance = {
        "build_profile": "release",
        "git_commit": source_sha,
        "package": "ao2",
        "schema_version": BUILD_PROVENANCE_SCHEMA,
        "target": target,
        "version": version,
    }
    for key, expected in expected_provenance.items():
        if provenance.get(key) != expected:
            raise CandidateValidationError(
                f"{target} build provenance {key} mismatch"
            )
    _validate_checksums(files, target)
    if files["VERSION"] != f"{version}\n".encode():
        raise CandidateValidationError(f"{target} VERSION mismatch")
    manifest = _json_object(files["RELEASE-MANIFEST.json"], f"{target} release manifest")
    expected_binary = TARGETS[target]["binary"]
    for key, expected in {
        "schema_version": MANIFEST_SCHEMA,
        "package": f"ao2-{version}-{target}",
        "version": version,
        "target": target,
        "binary_path": expected_binary,
        "checksum_file": "SHA256SUMS",
        "build_provenance": "BUILD-PROVENANCE.json",
    }.items():
        if manifest.get(key) != expected:
            raise CandidateValidationError(f"{target} release manifest {key} mismatch")
    if manifest.get("binary_sha256") != hashlib.sha256(files[expected_binary]).hexdigest():
        raise CandidateValidationError(f"{target} release manifest binary_sha256 mismatch")
    if set(manifest.get("files", [])) != set(files):
        raise CandidateValidationError(f"{target} release manifest files mismatch")
    verification = _json_object(
        files["RELEASE-VERIFICATION.json"],
        f"{target} release verification",
    )
    for key, expected in {
        "schema_version": VERIFICATION_SCHEMA,
        "status": "packaged",
        "target": target,
        "version": version,
        "binary_path": expected_binary,
        "provider_api_keys_required": False,
        "control_plane_approves_release": False,
        "mutates_ao_artifacts": False,
    }.items():
        if verification.get(key) != expected:
            raise CandidateValidationError(f"{target} release verification {key} mismatch")
    return {
        "archive": archive.name,
        "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
        "build_provenance_sha256": hashlib.sha256(files["BUILD-PROVENANCE.json"]).hexdigest(),
    }


def _validate_windows_ownership(path: Path) -> None:
    ownership = _json_object(path.read_bytes(), "Windows coverage ownership")
    expected = {
        "schema_version": WINDOWS_OWNERSHIP_SCHEMA,
        "status": "passed",
        "hosted_windows_portable_suite_owner": True,
        "physical_windows_mode": "physical_bounded",
        "target_triple": "x86_64-pc-windows-msvc",
        "linux_mingw_x86_64_pc_windows_gnu": "non_authoritative",
    }
    if ownership != expected:
        raise CandidateValidationError("Windows coverage ownership mismatch")


def validate_candidates(root: Path, source_sha: str, version: str) -> dict[str, Any]:
    if re.fullmatch(r"[0-9a-f]{40}", source_sha) is None:
        raise CandidateValidationError("source_sha must be a lowercase 40-character SHA")
    if re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) is None:
        raise CandidateValidationError("version must be numeric semver")
    if not root.is_dir() or root.is_symlink():
        raise CandidateValidationError("candidate root must be a safe directory")
    children = sorted(root.iterdir())
    if any(child.is_symlink() or not child.is_dir() for child in children):
        raise CandidateValidationError("candidate root inventory contains a non-directory entry")
    summaries = sorted(root.rglob("summary.json"))
    if {summary.parent for summary in summaries} != set(children):
        raise CandidateValidationError("candidate root inventory does not match artifact summaries")
    seen: dict[str, dict[str, Any]] = {}
    artifacts = []
    for summary_path in summaries:
        artifact_root = summary_path.parent
        summary = _json_object(summary_path.read_bytes(), f"candidate summary {summary_path}")
        _require_exact_keys(summary, SUMMARY_KEYS, f"candidate summary {summary_path}")
        target = summary.get("target")
        if target not in TARGETS or target in seen:
            raise CandidateValidationError(f"unexpected or duplicate target: {target}")
        expected_archive = f"ao2-{version}-{target}.tar.gz"
        archive_name = re.split(r"[\\/]", str(summary.get("archive", "")))[-1]
        expected_inventory = ["dist/" + expected_archive, "summary.json"]
        if target == "windows-x86_64":
            expected_inventory.append("windows-coverage-ownership.json")
        if _safe_inventory(artifact_root) != sorted(expected_inventory):
            raise CandidateValidationError(f"candidate artifact inventory mismatch for {target}")
        for key, expected in {
            "schema_version": SUMMARY_SCHEMA,
            "status": "passed",
            "version": version,
            "archive": summary["archive"],
            "install_verification_schema": "ao2.install-verification-evidence.v1",
            "provider_api_keys_required": False,
            "control_plane_approves_release": False,
            "mutates_ao_artifacts": False,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
        }.items():
            if summary.get(key) != expected:
                raise CandidateValidationError(f"{target} summary {key} mismatch")
        if archive_name != expected_archive:
            raise CandidateValidationError(f"{target} summary archive mismatch")
        archive_result = _validate_archive(
            artifact_root / "dist" / expected_archive,
            target,
            source_sha,
            version,
        )
        if target == "windows-x86_64":
            _validate_windows_ownership(artifact_root / "windows-coverage-ownership.json")
        artifacts.append(
            {
                "target": target,
                "runner": TARGETS[target]["runner"],
                "target_triple": TARGETS[target]["target_triple"],
                **archive_result,
            }
        )
        seen[target] = summary
    if set(seen) != set(TARGETS):
        raise CandidateValidationError(
            f"candidate target mismatch: missing={sorted(set(TARGETS) - set(seen))} "
            f"unexpected={sorted(set(seen) - set(TARGETS))}"
        )
    return {
        "schema_version": SCHEMA_VERSION,
        "status": "passed",
        "source_sha": source_sha,
        "version": version,
        "artifacts": sorted(artifacts, key=lambda item: item["target"]),
        "trust_boundary": {
            "mutates_ao_artifacts": False,
            "mutates_releases": False,
            "requires_signing_credentials": False,
            "signed_four_archive_release_gate": "separate_canonical_gate",
        },
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        report = validate_candidates(args.root, args.source_sha, args.version)
    except CandidateValidationError as exc:
        print(json.dumps({"status": "failed", "error": str(exc)}, sort_keys=True))
        return 2
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
