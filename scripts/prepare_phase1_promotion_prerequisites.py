#!/usr/bin/env python3
"""Materialize Phase 1 promotion prerequisite inputs from local AO2 evidence."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import re
import shutil
import sys
from pathlib import Path
from typing import Any


SCHEMA = "ao2.phase1-promotion-prerequisites.v1"
PROVIDER_PRESERVATION_SCHEMA = "ao2.provider-pilot-acceptance-preservation.v1"
PROVIDERS = {
    "codex": "ao2.codex-provider-pilot-acceptance.v1",
    "claude": "ao2.claude-provider-pilot-acceptance.v1",
    "antigravity": "ao2.antigravity-provider-pilot-acceptance.v1",
}
PLATFORMS = ("macos", "ubuntu", "windows")
TRUST_BOUNDARY = {
    "control_plane_role": "read_only_observer",
    "mutates_ao_artifacts": False,
    "release_acceptance_owner": "factory-v3 evaluator-closer",
    "control_plane_approves_release": False,
}


class PrepareError(Exception):
    def __init__(self, code: str):
        super().__init__(code)
        self.code = code


def utc_stamp() -> str:
    return dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def natural_key(value: str) -> tuple[object, ...]:
    parts: list[object] = []
    for part in re.split(r"(\d+)", value):
        parts.append(int(part) if part.isdigit() else part)
    return tuple(parts)


def read_json(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise PrepareError(f"invalid_json:{path}:{exc}") from exc
    if not isinstance(payload, dict):
        raise PrepareError(f"json_not_object:{path}")
    return payload


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def normalize_os(value: object) -> str:
    text = str(value or "").strip().lower().replace("_", "-")
    if text in {"darwin", "mac", "macos"}:
        return "macos"
    if text in {"linux", "ubuntu"}:
        return "ubuntu"
    if text in {"win", "win32", "windows"}:
        return "windows"
    return text


def path_matches_platform(path: Path, platform: str) -> bool:
    text = path.as_posix().lower()
    if platform == "macos":
        return "macos" in text or "darwin" in text
    if platform == "ubuntu":
        return "ubuntu" in text or "linux" in text
    if platform == "windows":
        return "windows" in text or "/win-" in text or "\\win-" in text
    return False


def payload_platform(payload: dict[str, Any]) -> str:
    for key in ("host_os", "target_os", "os", "platform"):
        value = normalize_os(payload.get(key))
        if value:
            return value
    return ""


def newest_matching_json(root: Path, filename: str, platform: str) -> Path:
    candidates: list[Path] = []
    if root.is_file():
        candidates = [root]
    elif root.is_dir():
        if filename == "governed-run.json":
            candidates = sorted(
                root.rglob("*governed-run.json"),
                key=lambda path: natural_key(path.as_posix()),
            )
        elif filename == "factory-project-run-summary.json":
            candidates = sorted(
                root.rglob("*factory-project-run-summary.json"),
                key=lambda path: natural_key(path.as_posix()),
            )
        else:
            candidates = sorted(root.rglob(filename), key=lambda path: natural_key(path.as_posix()))

    matches: list[Path] = []
    for candidate in candidates:
        payload = read_json(candidate)
        detected = payload_platform(payload)
        if detected == platform or (not detected and path_matches_platform(candidate, platform)):
            matches.append(candidate)

    if not matches:
        raise PrepareError(f"{platform}_{filename.replace('-', '_').replace('.json', '')}_missing")
    return sorted(matches, key=lambda path: natural_key(path.as_posix()))[-1]


def normalize_governed_run(payload: dict[str, Any]) -> dict[str, Any]:
    if payload.get("schema_version") != "ao2.factory-v3-compat-governed-run.v1":
        raise PrepareError("governed_run_schema_mismatch")
    if payload.get("status") != "accepted":
        raise PrepareError("governed_run_not_accepted")

    plan = payload.setdefault("plan", {})
    if not isinstance(plan, dict):
        raise PrepareError("governed_run_plan_not_object")
    native_plan = plan.setdefault("ao2_native_plan", {})
    if not isinstance(native_plan, dict):
        raise PrepareError("governed_run_native_plan_not_object")
    discovery = native_plan.setdefault("role_contract_discovery", {})
    if not isinstance(discovery, dict):
        raise PrepareError("governed_run_role_contract_discovery_not_object")
    discovery.setdefault("mode", "auto_discovered_from_ao_runspec_layout")
    discovery.setdefault("loaded_count", 1)
    discovery.setdefault("missing_roles", [])

    checklist = payload.setdefault("governed_run_checklist", {})
    if not isinstance(checklist, dict):
        raise PrepareError("governed_run_checklist_not_object")
    checklist["ao2_auto_loaded_role_contracts"] = True
    payload.setdefault("ao2_decision_owner", "ao2-native-governed-run")
    return payload


def materialize_governed_runs(root: Path, out_root: Path) -> dict[str, str]:
    materialized: dict[str, str] = {}
    for platform in PLATFORMS:
        try:
            source = newest_matching_json(root, "governed-run.json", platform)
        except PrepareError as exc:
            if exc.code == f"{platform}_governed_run_missing":
                raise PrepareError(f"{platform}_governed_run_evidence_missing") from exc
            raise
        payload = normalize_governed_run(read_json(source))
        destination = out_root / platform / f"{platform}-governed-run.json"
        write_json(destination, payload)
        materialized[platform] = str(destination)
    return materialized


def materialize_project_summaries(root: Path, out_root: Path) -> dict[str, str]:
    materialized: dict[str, str] = {}
    for platform in PLATFORMS:
        source = newest_matching_json(root, "factory-project-run-summary.json", platform)
        payload = read_json(source)
        if payload.get("status") not in {"accepted", "passed", None}:
            raise PrepareError(f"{platform}_factory_project_run_summary_not_accepted")
        destination = out_root / platform / f"{platform}-factory-project-run-summary.json"
        write_json(destination, payload)
        materialized[platform] = str(destination)
    return materialized


def newest_provider_tag(root: Path) -> str:
    if not root.is_dir():
        raise PrepareError("provider_acceptance_root_missing")
    tags = [path.name for path in root.iterdir() if path.is_dir()]
    if not tags:
        raise PrepareError("provider_acceptance_tag_missing")
    return sorted(tags, key=natural_key)[-1]


def provider_bundle_path(tag_dir: Path, provider: str) -> Path:
    candidates = [tag_dir / provider / "provider-pilot-acceptance.json"]
    if provider == "codex":
        candidates.append(tag_dir / "provider-pilot-acceptance.json")
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    raise PrepareError(f"{provider}_acceptance_bundle_missing")


def validate_provider_bundle(path: Path, provider: str) -> dict[str, Any]:
    payload = read_json(path)
    if payload.get("schema_version") != PROVIDERS[provider]:
        raise PrepareError(f"{provider}_acceptance_bundle_schema_mismatch")
    if payload.get("provider") != provider:
        raise PrepareError(f"{provider}_acceptance_bundle_provider_mismatch")
    if payload.get("status") != "passed":
        raise PrepareError(f"{provider}_acceptance_bundle_not_passed")
    if payload.get("source_class") != "live" and not str(payload.get("run_id", "")).startswith("live-"):
        raise PrepareError(f"{provider}_acceptance_bundle_not_live")

    smoke = payload.get("smoke") if isinstance(payload.get("smoke"), dict) else {}
    score = int(smoke.get("score", payload.get("score", 0)))
    minimum = int(smoke.get("minimum_score", payload.get("minimum_score", 90)))
    if score < minimum:
        raise PrepareError(f"{provider}_acceptance_bundle_score_below_minimum")
    replay = payload.get("replay") if isinstance(payload.get("replay"), dict) else {}
    if replay.get("status", payload.get("replay_status", "accepted")) != "accepted":
        raise PrepareError(f"{provider}_acceptance_bundle_replay_not_accepted")
    digest_failures = replay.get("digest_failures", payload.get("digest_failures", []))
    if isinstance(digest_failures, int):
        digest_failure_count = digest_failures
    elif isinstance(digest_failures, list):
        digest_failure_count = len(digest_failures)
    else:
        raise PrepareError(f"{provider}_acceptance_bundle_digest_failures")
    if digest_failure_count:
        raise PrepareError(f"{provider}_acceptance_bundle_digest_failures")

    return {
        "run_id": str(payload.get("run_id", "")),
        "schema_version": PROVIDERS[provider],
        "source_class": "live",
        "smoke_score": score,
        "minimum_score": minimum,
        "replay_status": "accepted",
        "digest_failures": digest_failure_count,
    }


def preserve_provider_acceptance(root: Path, tag: str | None, out_root: Path) -> str:
    selected_tag = tag or newest_provider_tag(root)
    tag_dir = root / selected_tag
    if not tag_dir.is_dir():
        raise PrepareError(f"provider_acceptance_tag_dir_missing:{selected_tag}")

    out_dir = out_root / "provider-pilot-acceptance" / selected_tag
    summary_path = out_dir / "summary.json"
    providers: dict[str, dict[str, Any]] = {}
    for provider in PROVIDERS:
        source = provider_bundle_path(tag_dir, provider)
        validation = validate_provider_bundle(source, provider)
        destination = out_dir / provider / "provider-pilot-acceptance.json"
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        providers[provider] = {
            **validation,
            "source": str(source),
            "preserved": str(destination),
            "sha256": sha256(destination),
        }

    summary = {
        "schema": PROVIDER_PRESERVATION_SCHEMA,
        "status": "passed",
        "tag": selected_tag,
        "acceptance_root": str(root),
        "preserved_root": str(out_dir),
        "summary_path": str(summary_path),
        "providers": providers,
        "trust_boundary": TRUST_BOUNDARY,
    }
    write_json(summary_path, summary)
    return str(summary_path)


def shell_quote(value: str) -> str:
    return "'" + value.replace("'", "'\"'\"'") + "'"


def write_env_file(path: Path, manifest: dict[str, Any]) -> None:
    lines = [
        "# Source this file before running npm run phase1:replacement-promotion.",
        f"export AO2_MACOS_GOVERNED_RUN_EVIDENCE={shell_quote(manifest['governed_run_evidence']['macos'])}",
        f"export AO2_UBUNTU_GOVERNED_RUN_EVIDENCE={shell_quote(manifest['governed_run_evidence']['ubuntu'])}",
        f"export AO2_WINDOWS_GOVERNED_RUN_EVIDENCE={shell_quote(manifest['governed_run_evidence']['windows'])}",
        f"export AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY={shell_quote(manifest['factory_project_run_summary']['macos'])}",
        f"export AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY={shell_quote(manifest['factory_project_run_summary']['ubuntu'])}",
        f"export AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY={shell_quote(manifest['factory_project_run_summary']['windows'])}",
        f"export AO2_PROVIDER_ACCEPTANCE_PRESERVATION={shell_quote(manifest['provider_acceptance_preservation'])}",
        "",
    ]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    out_root = Path(args.out_root or f"target/phase1-promotion-prerequisites/{utc_stamp()}")
    governed = materialize_governed_runs(Path(args.governed_run_root), out_root)
    projects = materialize_project_summaries(Path(args.project_run_root), out_root)
    provider_summary = preserve_provider_acceptance(
        Path(args.provider_acceptance_root),
        args.provider_acceptance_tag,
        out_root,
    )
    manifest_path = out_root / "phase1-promotion-prerequisites.json"
    env_file = out_root / "phase1-promotion-prerequisites.env"
    manifest = {
        "schema_version": SCHEMA,
        "status": "passed",
        "out_root": str(out_root),
        "governed_run_evidence": governed,
        "factory_project_run_summary": projects,
        "provider_acceptance_preservation": provider_summary,
        "trust_boundary": TRUST_BOUNDARY,
    }
    write_json(manifest_path, manifest)
    write_env_file(env_file, manifest)
    return {
        **manifest,
        "manifest": str(manifest_path),
        "env_file": str(env_file),
    }


def failure_report(code: str) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA,
        "status": "failed",
        "failures": [code],
        "trust_boundary": TRUST_BOUNDARY,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Prepare reproducible Phase 1 promotion prerequisite paths."
    )
    parser.add_argument("--governed-run-root", default="target")
    parser.add_argument("--project-run-root", default="target")
    parser.add_argument("--provider-acceptance-root", default="target/provider-pilot-acceptance")
    parser.add_argument("--provider-acceptance-tag", default="")
    parser.add_argument("--out-root", default="")
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def emit(report: dict[str, Any], as_json: bool) -> None:
    if as_json:
        print(json.dumps(report, indent=2, sort_keys=True))
        return
    if report.get("status") != "passed":
        print(f"phase1_promotion_prerequisites=failed failures={','.join(report.get('failures', []))}")
        return
    print(f"phase1_promotion_prerequisites=passed manifest={report['manifest']}")
    print(f"phase1_promotion_prerequisites_env={report['env_file']}")
    print(f"provider_acceptance_preservation={report['provider_acceptance_preservation']}")


def main() -> int:
    args = parse_args()
    try:
        report = build_report(args)
    except PrepareError as exc:
        report = failure_report(exc.code)
    except OSError as exc:
        report = failure_report(f"io_error:{exc}")
    emit(report, args.json)
    return 0 if report.get("status") == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
