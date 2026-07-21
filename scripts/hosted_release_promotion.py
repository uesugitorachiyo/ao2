#!/usr/bin/env python3
"""Authenticate, stage, and verify immutable AO2 hosted release assets."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import re
import shutil
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CANDIDATE_VALIDATOR_PATH = ROOT / "scripts" / "validate_hosted_release_candidates.py"
WORKFLOW_PATH = ".github/workflows/public-release-build.yml"
PLAN_SCHEMA = "ao2.hosted-release-promotion-plan.v1"
BOUNDARY_SCHEMA = "ao2.hosted-release-dry-run-boundary.v1"
REPORT_SCHEMA = "ao2.hosted-release-publication.v1"
MAX_JSON_BYTES = 2 * 1024 * 1024
TARGETS = ("linux-x86_64", "macos-aarch64", "windows-x86_64")
PLAN_KEYS = {
    "schema_version",
    "status",
    "version",
    "tag",
    "source_sha",
    "approved_asset_manifest_sha256",
    "physical_windows_evidence_sha256",
    "artifacts",
    "windows",
    "rejection_policy",
    "trust_boundary",
}
PLAN_ARTIFACT_KEYS = {
    "target",
    "runner",
    "target_triple",
    "archive",
    "sha256",
    "canonical_public_archive",
}
BOUNDARY = {
    "schema_version": BOUNDARY_SCHEMA,
    "status": "passed",
    "dry_run": True,
    "publication_status": "not_attempted",
    "publication_status: not_attempted": True,
    "tag_creation_attempted": False,
    "tag_creation_attempted: false": True,
    "release_creation_attempted": False,
    "release_creation_attempted: false": True,
    "public_upload_attempted": False,
    "public_upload_attempted: false": True,
}
WINDOWS_BOUNDARY = {
    "canonical_target_triple": "x86_64-pc-windows-msvc",
    "canonical_runner": "windows-latest",
    "linux_mingw_cross_build": {
        "target_triple": "x86_64-pc-windows-gnu",
        "classification": "non_authoritative",
        "canonical_public_windows_archive": False,
    },
}
TRUST_BOUNDARY = {
    "build_jobs_mutate_releases": False,
    "plan_job_mutates_releases": False,
    "stores_credentials": False,
    "uses_workflow_scoped_github_token": True,
}
REJECTION_POLICY = [
    "missing_artifact",
    "duplicate_artifact",
    "stale_source_sha",
    "substituted_archive",
    "unexpected_artifact",
    "version_tag_mismatch",
    "approved_manifest_mismatch",
    "physical_windows_evidence_mismatch",
    "incorrect_live_confirmation",
]


class PromotionValidationError(ValueError):
    """Raised when a frozen hosted release input is not exact and safe."""


def _candidate_validator():
    spec = importlib.util.spec_from_file_location(
        "_ao2_hosted_candidate_validator",
        CANDIDATE_VALIDATOR_PATH,
    )
    if spec is None or spec.loader is None:
        raise PromotionValidationError("could not load hosted candidate validator")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _json_object(path: Path, label: str) -> dict[str, Any]:
    if not path.is_file() or path.is_symlink():
        raise PromotionValidationError(f"missing or unsafe {label}")
    if path.stat().st_size > MAX_JSON_BYTES:
        raise PromotionValidationError(f"{label} exceeds size limit")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise PromotionValidationError(f"invalid {label} JSON") from exc
    if not isinstance(value, dict):
        raise PromotionValidationError(f"{label} must be an object")
    return value


def _safe_file_inventory(root: Path) -> list[str]:
    if not root.is_dir() or root.is_symlink():
        raise PromotionValidationError(f"unsafe directory: {root}")
    files = []
    for path in root.rglob("*"):
        if path.is_symlink():
            raise PromotionValidationError(f"unsafe symlink: {path}")
        if path.is_file():
            files.append(path.relative_to(root).as_posix())
        elif not path.is_dir():
            raise PromotionValidationError(f"unsafe filesystem entry: {path}")
    return sorted(files)


def _require_sha256(value: str, label: str) -> None:
    if re.fullmatch(r"[0-9a-f]{64}", value) is None:
        raise PromotionValidationError(f"{label} must be a lowercase SHA-256")


def _require_source_and_version(source_sha: str, version: str, tag: str) -> None:
    if re.fullmatch(r"[0-9a-f]{40}", source_sha) is None:
        raise PromotionValidationError("source_sha must be a lowercase 40-character SHA")
    if re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version) is None:
        raise PromotionValidationError("version must be numeric semver")
    if tag != f"v{version}":
        raise PromotionValidationError("tag and version mismatch")


def _expected_artifact_names(source_sha: str) -> set[str]:
    return {
        *(f"ao2-hosted-native-candidate-{target}-{source_sha}" for target in TARGETS),
        f"ao2-hosted-release-promotion-plan-{source_sha}",
    }


def validate_frozen_run(
    run: dict[str, Any],
    artifacts: dict[str, Any],
    run_id: str,
    current_run_id: str,
    repository: str,
    source_sha: str,
) -> dict[str, Any]:
    if re.fullmatch(r"[1-9][0-9]{0,19}", run_id) is None:
        raise PromotionValidationError("frozen run ID is invalid")
    if run_id == current_run_id:
        raise PromotionValidationError("frozen run must be a prior workflow run")
    if run.get("id") != int(run_id):
        raise PromotionValidationError("frozen run ID mismatch")
    if run.get("event") != "workflow_dispatch":
        raise PromotionValidationError("frozen run must use workflow_dispatch")
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        raise PromotionValidationError("frozen run must be completed and successful")
    if run.get("head_sha") != source_sha:
        raise PromotionValidationError("frozen run source SHA mismatch")
    if run.get("path") != WORKFLOW_PATH:
        raise PromotionValidationError("frozen run workflow path mismatch")
    repository_value = run.get("repository")
    head_repository = run.get("head_repository")
    if not isinstance(repository_value, dict) or repository_value.get("full_name") != repository:
        raise PromotionValidationError("frozen run repository mismatch")
    if not isinstance(head_repository, dict) or head_repository.get("full_name") != repository:
        raise PromotionValidationError("frozen run head repository mismatch")
    repository_id = repository_value.get("id")
    if not isinstance(repository_id, int) or head_repository.get("id") != repository_id:
        raise PromotionValidationError("frozen run repository identity mismatch")

    rows = artifacts.get("artifacts")
    if not isinstance(rows, list):
        raise PromotionValidationError("artifact metadata must contain a list")
    required = _expected_artifact_names(source_sha)
    found: dict[str, dict[str, Any]] = {}
    for row in rows:
        if not isinstance(row, dict) or row.get("name") not in required:
            continue
        name = row["name"]
        if name in found:
            raise PromotionValidationError(f"duplicate frozen artifact: {name}")
        if row.get("expired") is not False:
            raise PromotionValidationError(f"frozen artifact is expired: {name}")
        binding = row.get("workflow_run")
        if (
            not isinstance(binding, dict)
            or binding.get("id") != int(run_id)
            or binding.get("head_sha") != source_sha
            or binding.get("repository_id") != repository_id
            or binding.get("head_repository_id") != repository_id
        ):
            raise PromotionValidationError(f"frozen artifact run binding mismatch: {name}")
        found[name] = row
    if set(found) != required:
        raise PromotionValidationError(
            f"frozen artifact inventory mismatch: missing={sorted(required - set(found))}"
        )
    return {
        "schema_version": "ao2.hosted-release-frozen-run.v1",
        "status": "passed",
        "run_id": int(run_id),
        "source_sha": source_sha,
        "artifact_names": sorted(found),
    }


def _validate_plan(
    plan_root: Path,
    source_sha: str,
    version: str,
    tag: str,
    manifest_sha256: str,
    plan_sha256: str,
    physical_evidence_sha256: str,
) -> tuple[dict[str, Any], Path]:
    expected_inventory = [
        "dry-run-boundary.json",
        "promotion-plan.json",
        "promotion-plan.sha256",
    ]
    if _safe_file_inventory(plan_root) != expected_inventory:
        raise PromotionValidationError("promotion plan inventory is not exact")
    _require_sha256(plan_sha256, "promotion plan digest")
    _require_sha256(manifest_sha256, "approved manifest digest")
    plan_path = plan_root / "promotion-plan.json"
    if _sha256(plan_path) != plan_sha256:
        raise PromotionValidationError("promotion plan digest mismatch")
    digest_file = (plan_root / "promotion-plan.sha256").read_text(encoding="ascii")
    if digest_file != plan_sha256 + "\n":
        raise PromotionValidationError("promotion plan digest file mismatch")
    plan = _json_object(plan_path, "promotion plan")
    if set(plan) != PLAN_KEYS:
        raise PromotionValidationError("promotion plan keys are not exact")
    expected_scalars = {
        "schema_version": PLAN_SCHEMA,
        "status": "passed",
        "source_sha": source_sha,
        "version": version,
        "tag": tag,
        "approved_asset_manifest_sha256": manifest_sha256,
    }
    for key, expected in expected_scalars.items():
        if plan.get(key) != expected:
            raise PromotionValidationError(f"promotion plan {key} mismatch")
    physical_digest = plan.get("physical_windows_evidence_sha256")
    if not isinstance(physical_digest, str):
        raise PromotionValidationError("physical Windows evidence digest is missing")
    _require_sha256(physical_digest, "physical Windows evidence digest")
    _require_sha256(
        physical_evidence_sha256,
        "expected physical Windows evidence digest",
    )
    if physical_digest != physical_evidence_sha256:
        raise PromotionValidationError(
            "physical Windows evidence digest mismatch"
        )
    if plan.get("windows") != WINDOWS_BOUNDARY:
        raise PromotionValidationError("promotion plan Windows boundary mismatch")
    if plan.get("trust_boundary") != TRUST_BOUNDARY:
        raise PromotionValidationError("promotion plan trust boundary mismatch")
    if plan.get("rejection_policy") != REJECTION_POLICY:
        raise PromotionValidationError("promotion plan rejection policy mismatch")
    if _json_object(plan_root / "dry-run-boundary.json", "dry-run boundary") != BOUNDARY:
        raise PromotionValidationError("dry-run boundary is not exact")
    return plan, plan_path


def stage_publication(
    candidate_root: Path,
    plan_root: Path,
    publication_root: Path,
    source_sha: str,
    version: str,
    tag: str,
    manifest_sha256: str,
    plan_sha256: str,
    physical_evidence_sha256: str,
) -> dict[str, Any]:
    _require_source_and_version(source_sha, version, tag)
    plan, plan_path = _validate_plan(
        plan_root,
        source_sha,
        version,
        tag,
        manifest_sha256,
        plan_sha256,
        physical_evidence_sha256,
    )
    validator = _candidate_validator()
    try:
        validated = validator.validate_candidates(candidate_root, source_sha, version)
    except validator.CandidateValidationError as exc:
        raise PromotionValidationError(str(exc)) from exc

    plan_artifacts = plan.get("artifacts")
    if not isinstance(plan_artifacts, list) or len(plan_artifacts) != len(TARGETS):
        raise PromotionValidationError("promotion plan artifact inventory mismatch")
    by_target: dict[str, dict[str, Any]] = {}
    for item in plan_artifacts:
        if not isinstance(item, dict) or set(item) != PLAN_ARTIFACT_KEYS:
            raise PromotionValidationError("promotion plan artifact keys are not exact")
        target = item.get("target")
        if target not in TARGETS or target in by_target:
            raise PromotionValidationError("promotion plan artifact target mismatch")
        by_target[target] = item
    validated_by_target = {item["target"]: item for item in validated["artifacts"]}
    publication_sources: dict[str, Path] = {}
    for target in TARGETS:
        item = by_target.get(target)
        actual = validated_by_target[target]
        expected_name = f"ao2-{version}-{target}.tar.gz"
        if item is None:
            raise PromotionValidationError("promotion plan artifact target mismatch")
        for key in ("runner", "target_triple"):
            if item.get(key) != actual[key]:
                raise PromotionValidationError(f"promotion plan artifact {key} mismatch")
        if (
            Path(str(item.get("archive"))).name != expected_name
            or item.get("canonical_public_archive") is not True
        ):
            raise PromotionValidationError("promotion plan archive contract mismatch")
        if item.get("sha256") != actual["archive_sha256"]:
            raise PromotionValidationError("promotion plan archive digest mismatch")
        matches = list(candidate_root.rglob(expected_name))
        if len(matches) != 1:
            raise PromotionValidationError(f"candidate archive inventory mismatch for {target}")
        publication_sources[expected_name] = matches[0]

    if publication_root.exists():
        if publication_root.is_symlink() or any(publication_root.iterdir()):
            raise PromotionValidationError("publication directory must be absent or empty")
    else:
        publication_root.mkdir(parents=True)
    for name, source in publication_sources.items():
        shutil.copyfile(source, publication_root / name)
    shutil.copyfile(plan_path, publication_root / "promotion-plan.json")
    checksum_names = sorted([*publication_sources, "promotion-plan.json"])
    checksum_lines = [
        f"{_sha256(publication_root / name)}  {name}" for name in checksum_names
    ]
    (publication_root / "SHA256SUMS").write_text(
        "\n".join(checksum_lines) + "\n",
        encoding="ascii",
    )
    assets = sorted([*checksum_names, "SHA256SUMS"])
    return {
        "schema_version": REPORT_SCHEMA,
        "status": "passed",
        "source_sha": source_sha,
        "version": version,
        "tag": tag,
        "promotion_plan_sha256": plan_sha256,
        "physical_windows_evidence_sha256": plan["physical_windows_evidence_sha256"],
        "assets": assets,
    }


def verify_publication(
    publication_root: Path,
    source_sha: str,
    version: str,
    tag: str,
    manifest_sha256: str,
    plan_sha256: str,
    physical_evidence_sha256: str,
) -> dict[str, Any]:
    _require_source_and_version(source_sha, version, tag)
    expected_archives = [f"ao2-{version}-{target}.tar.gz" for target in TARGETS]
    expected = sorted([*expected_archives, "promotion-plan.json", "SHA256SUMS"])
    if _safe_file_inventory(publication_root) != expected:
        raise PromotionValidationError("public asset inventory is not exact")
    sums_path = publication_root / "SHA256SUMS"
    rows = sums_path.read_text(encoding="ascii").splitlines()
    parsed: dict[str, str] = {}
    for row in rows:
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9._-]+)", row)
        if match is None or match.group(2) in parsed:
            raise PromotionValidationError("public checksum row is invalid")
        parsed[match.group(2)] = match.group(1)
    if set(parsed) != set(expected) - {"SHA256SUMS"}:
        raise PromotionValidationError("public checksum inventory is not exact")
    for name, digest in parsed.items():
        if _sha256(publication_root / name) != digest:
            raise PromotionValidationError(f"public asset digest mismatch: {name}")
    if _sha256(publication_root / "promotion-plan.json") != plan_sha256:
        raise PromotionValidationError("promotion plan digest mismatch")
    plan = _json_object(publication_root / "promotion-plan.json", "promotion plan")
    for key, expected_value in {
        "schema_version": PLAN_SCHEMA,
        "status": "passed",
        "source_sha": source_sha,
        "version": version,
        "tag": tag,
        "approved_asset_manifest_sha256": manifest_sha256,
    }.items():
        if plan.get(key) != expected_value:
            raise PromotionValidationError(f"promotion plan {key} mismatch")
    if set(plan) != PLAN_KEYS:
        raise PromotionValidationError("promotion plan keys are not exact")
    physical_digest = plan.get("physical_windows_evidence_sha256")
    if not isinstance(physical_digest, str):
        raise PromotionValidationError("physical Windows evidence digest is missing")
    _require_sha256(physical_digest, "physical Windows evidence digest")
    _require_sha256(
        physical_evidence_sha256,
        "expected physical Windows evidence digest",
    )
    if physical_digest != physical_evidence_sha256:
        raise PromotionValidationError(
            "physical Windows evidence digest mismatch"
        )
    if plan.get("windows") != WINDOWS_BOUNDARY:
        raise PromotionValidationError("promotion plan Windows boundary mismatch")
    if plan.get("trust_boundary") != TRUST_BOUNDARY:
        raise PromotionValidationError("promotion plan trust boundary mismatch")
    if plan.get("rejection_policy") != REJECTION_POLICY:
        raise PromotionValidationError("promotion plan rejection policy mismatch")
    plan_artifacts = plan.get("artifacts")
    if not isinstance(plan_artifacts, list) or len(plan_artifacts) != len(TARGETS):
        raise PromotionValidationError("promotion plan artifact inventory mismatch")
    by_target: dict[str, dict[str, Any]] = {}
    for item in plan_artifacts:
        if not isinstance(item, dict) or set(item) != PLAN_ARTIFACT_KEYS:
            raise PromotionValidationError("promotion plan artifact keys are not exact")
        target = item.get("target")
        if target not in TARGETS or target in by_target:
            raise PromotionValidationError("promotion plan artifact target mismatch")
        by_target[target] = item
    validator = _candidate_validator()
    for target in TARGETS:
        archive = publication_root / f"ao2-{version}-{target}.tar.gz"
        try:
            archive_result = validator._validate_archive(
                archive,
                target,
                source_sha,
                version,
            )
        except validator.CandidateValidationError as exc:
            raise PromotionValidationError(str(exc)) from exc
        item = by_target[target]
        if (
            item.get("sha256") != archive_result["archive_sha256"]
            or Path(str(item.get("archive"))).name != archive.name
            or item.get("canonical_public_archive") is not True
        ):
            raise PromotionValidationError("promotion plan archive digest mismatch")
        if (
            item.get("runner") != validator.TARGETS[target]["runner"]
            or item.get("target_triple") != validator.TARGETS[target]["target_triple"]
        ):
            raise PromotionValidationError("promotion plan archive platform mismatch")
    return {
        "schema_version": "ao2.hosted-release-public-verification.v1",
        "status": "passed",
        "source_sha": source_sha,
        "version": version,
        "tag": tag,
        "promotion_plan_sha256": plan_sha256,
        "assets": expected,
    }


def _write_report(path: Path, report: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    run = subparsers.add_parser("validate-run")
    run.add_argument("--run-metadata", required=True, type=Path)
    run.add_argument("--artifacts-metadata", required=True, type=Path)
    run.add_argument("--run-id", required=True)
    run.add_argument("--current-run-id", required=True)
    run.add_argument("--repository", required=True)
    run.add_argument("--source-sha", required=True)
    run.add_argument("--out", required=True, type=Path)

    for command in ("stage", "verify-public"):
        child = subparsers.add_parser(command)
        child.add_argument(
            "--candidate-root",
            required=command == "stage",
            type=Path,
        )
        child.add_argument("--plan-root", required=command == "stage", type=Path)
        child.add_argument("--publication-root", required=True, type=Path)
        child.add_argument("--source-sha", required=True)
        child.add_argument("--version", required=True)
        child.add_argument("--tag", required=True)
        child.add_argument("--manifest-sha256", required=True)
        child.add_argument("--plan-sha256", required=True)
        child.add_argument("--physical-evidence-sha256", required=True)
        child.add_argument("--out", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.command == "validate-run":
            report = validate_frozen_run(
                _json_object(args.run_metadata, "run metadata"),
                _json_object(args.artifacts_metadata, "artifact metadata"),
                args.run_id,
                args.current_run_id,
                args.repository,
                args.source_sha,
            )
        elif args.command == "stage":
            report = stage_publication(
                args.candidate_root,
                args.plan_root,
                args.publication_root,
                args.source_sha,
                args.version,
                args.tag,
                args.manifest_sha256,
                args.plan_sha256,
                args.physical_evidence_sha256,
            )
        else:
            report = verify_publication(
                args.publication_root,
                args.source_sha,
                args.version,
                args.tag,
                args.manifest_sha256,
                args.plan_sha256,
                args.physical_evidence_sha256,
            )
    except (OSError, PromotionValidationError) as exc:
        print(json.dumps({"status": "failed", "error": str(exc)}, sort_keys=True))
        return 2
    _write_report(args.out, report)
    print(json.dumps(report, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
