#!/usr/bin/env python3
"""Import digest-bound physical-Windows evidence from fixed environment inputs."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Mapping

import physical_windows_qualification as qualification


ARTIFACT_FILES = ("evidence.json", "summary.json")
DESTINATION = Path("target/physical-windows-qualification")
ENVIRONMENT_KEYS = (
    "EVIDENCE_BASE64",
    "EVIDENCE_SHA256",
    "SOURCE_SHA",
    "VERSION",
    "GITHUB_SHA",
)


class ArtifactImportError(ValueError):
    """Raised when the fixed import boundary cannot be satisfied."""


def _utc_now() -> datetime:
    return datetime.now(timezone.utc)


def _required_environment(environ: Mapping[str, str]) -> dict[str, str]:
    values: dict[str, str] = {}
    for name in ENVIRONMENT_KEYS:
        value = environ.get(name)
        if not isinstance(value, str) or not value:
            raise ArtifactImportError(f"{name} must be a non-empty environment value")
        values[name] = value
    return values


def relative_file_inventory(root: Path) -> list[str]:
    inventory: list[str] = []
    for path in root.rglob("*"):
        if path.is_symlink():
            raise ArtifactImportError(f"artifact inventory contains a symbolic link: {path.name}")
        if path.is_file():
            inventory.append(path.relative_to(root).as_posix())
    return sorted(inventory)


def verify_exact_inventory(root: Path) -> None:
    inventory = relative_file_inventory(root)
    if inventory != list(ARTIFACT_FILES):
        raise ArtifactImportError(
            f"artifact inventory must be exactly {list(ARTIFACT_FILES)}, got {inventory}"
        )


def atomic_write(path: Path, payload: bytes) -> None:
    temporary = path.with_name(f".{path.name}.tmp")
    try:
        with temporary.open("xb") as output:
            output.write(payload)
            output.flush()
            os.fsync(output.fileno())
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def _materialize(destination: Path, evidence_bytes: bytes, summary_bytes: bytes) -> None:
    if destination.exists() or destination.is_symlink():
        raise ArtifactImportError(f"artifact destination already exists: {destination}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(
        tempfile.mkdtemp(
            prefix=f".{destination.name}-",
            dir=destination.parent,
        )
    )
    promoted = False
    try:
        atomic_write(staging / "evidence.json", evidence_bytes)
        atomic_write(staging / "summary.json", summary_bytes)
        verify_exact_inventory(staging)
        if destination.exists() or destination.is_symlink():
            raise ArtifactImportError(f"artifact destination already exists: {destination}")
        staging.replace(destination)
        promoted = True
        try:
            verify_exact_inventory(destination)
        except Exception:
            shutil.rmtree(destination, ignore_errors=True)
            raise
    finally:
        if not promoted:
            shutil.rmtree(staging, ignore_errors=True)


def import_qualification(
    repository: Path,
    environ: Mapping[str, str],
) -> Path:
    repository = repository.resolve()
    values = _required_environment(environ)
    source_sha = values["SOURCE_SHA"]
    expected_digest = values["EVIDENCE_SHA256"]
    version = values["VERSION"]
    destination = repository / DESTINATION

    if destination.exists() or destination.is_symlink():
        raise ArtifactImportError(f"artifact destination already exists: {destination}")
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise ArtifactImportError("SOURCE_SHA must be a lowercase 40-character SHA")
    if not re.fullmatch(r"[0-9a-f]{64}", expected_digest):
        raise ArtifactImportError("EVIDENCE_SHA256 must be a lowercase SHA-256 digest")

    head = subprocess.check_output(
        ["git", "rev-parse", "HEAD"],
        cwd=repository,
        text=True,
    ).strip()
    if head != source_sha:
        raise ArtifactImportError("repository HEAD does not match SOURCE_SHA")
    if values["GITHUB_SHA"] != source_sha:
        raise ArtifactImportError("GITHUB_SHA does not match SOURCE_SHA")

    discovered_version = subprocess.check_output(
        [str(repository / "scripts" / "current-version.sh")],
        cwd=repository,
        text=True,
    ).strip()
    if discovered_version != version:
        raise ArtifactImportError("discovered repository version does not match VERSION")

    evidence = qualification.decode_import_payload(
        values["EVIDENCE_BASE64"],
        expected_digest,
    )
    summary = qualification.validate_evidence(
        evidence,
        source_sha,
        version,
        _utc_now(),
    )
    evidence_bytes = qualification.canonical_json(evidence)
    summary_bytes = qualification.canonical_json(summary)
    if summary.get("physical_evidence_sha256") != expected_digest:
        raise ArtifactImportError("summary evidence digest does not match EVIDENCE_SHA256")

    _materialize(destination, evidence_bytes, summary_bytes)
    return destination


def main() -> int:
    try:
        import_qualification(Path.cwd(), os.environ)
    except (
        ArtifactImportError,
        qualification.ValidationError,
        OSError,
        subprocess.SubprocessError,
    ) as exc:
        print(f"physical Windows qualification import failed: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
