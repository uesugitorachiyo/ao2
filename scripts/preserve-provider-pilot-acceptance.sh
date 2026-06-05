#!/bin/sh
set -eu

if ! command -v python3 >/dev/null 2>&1; then
  printf '%s\n' "provider_pilot_acceptance_preservation=failed reason=missing_python3" >&2
  exit 127
fi

python3 - <<'PY'
from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import sys
import time
from pathlib import Path


SCHEMA = "ao2.provider-pilot-acceptance-preservation.v1"
PROVIDERS = {
    "codex": "ao2.codex-provider-pilot-acceptance.v1",
    "claude": "ao2.claude-provider-pilot-acceptance.v1",
    "antigravity": "ao2.antigravity-provider-pilot-acceptance.v1",
}


def fail(message: str) -> None:
    print(f"provider_pilot_acceptance_preservation=failed reason={message}", file=sys.stderr)
    raise SystemExit(1)


def natural_key(value: str) -> tuple[object, ...]:
    parts: list[object] = []
    for part in re.split(r"(\d+)", value):
        if part.isdigit():
            parts.append(int(part))
        else:
            parts.append(part)
    return tuple(parts)


def newest_tag(root: Path) -> str:
    if not root.is_dir():
        fail(f"acceptance_root_missing:{root}")
    candidates = [path.name for path in root.iterdir() if path.is_dir()]
    if not candidates:
        fail(f"acceptance_tag_missing:{root}")
    return sorted(candidates, key=natural_key)[-1]


def candidate_paths(tag_dir: Path, provider: str) -> list[Path]:
    candidates = [tag_dir / provider / "provider-pilot-acceptance.json"]
    if provider == "codex":
        candidates.append(tag_dir / "provider-pilot-acceptance.json")
    return candidates


def acceptance_path(tag_dir: Path, provider: str) -> Path:
    for candidate in candidate_paths(tag_dir, provider):
        if candidate.is_file():
            return candidate
    fail(f"{provider}_acceptance_bundle_missing:{tag_dir}")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def live_source_class(path: Path, payload: dict[str, object]) -> str:
    explicit = payload.get("source_class")
    if explicit == "live":
        return "live"
    normalized_parts = path.as_posix().split("/")
    has_target_acceptance_root = False
    for index, part in enumerate(normalized_parts[:-1]):
        if (
            part == "target"
            and index + 1 < len(normalized_parts)
            and normalized_parts[index + 1] == "provider-pilot-acceptance"
        ):
            has_target_acceptance_root = True
            break
    run_id = str(payload.get("run_id", ""))
    if has_target_acceptance_root and run_id.startswith("live-"):
        return "live"
    return str(explicit or "unknown")


def validate_bundle(path: Path, provider: str) -> dict[str, object]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail(f"{provider}_acceptance_bundle_invalid_json:{exc}")
    expected_schema = PROVIDERS[provider]
    if payload.get("schema_version") != expected_schema:
        fail(f"{provider}_acceptance_bundle_schema_mismatch")
    if payload.get("provider") != provider:
        fail(f"{provider}_acceptance_bundle_provider_mismatch")
    if payload.get("status") != "passed":
        fail(f"{provider}_acceptance_bundle_not_passed")
    source_class = live_source_class(path, payload)
    if source_class != "live":
        fail(f"{provider}_acceptance_bundle_not_live")

    smoke = payload.get("smoke") if isinstance(payload.get("smoke"), dict) else {}
    score_field = payload.get("score")
    if isinstance(score_field, dict):
        score_default = score_field.get("score", 0)
    else:
        score_default = score_field if score_field is not None else 0
    pilot = payload.get("pilot") if isinstance(payload.get("pilot"), dict) else {}
    score = int(smoke.get("score", score_default))
    minimum_score = int(smoke.get("minimum_score", pilot.get("minimum_score", payload.get("minimum_score", 90))))
    if score < minimum_score:
        fail(f"{provider}_acceptance_bundle_score_below_minimum")

    replay = payload.get("replay") if isinstance(payload.get("replay"), dict) else {}
    replay_status = replay.get("status", payload.get("replay_status", "accepted"))
    if replay_status != "accepted":
        fail(f"{provider}_acceptance_bundle_replay_not_accepted")
    digest_failures = replay.get("digest_failures", payload.get("digest_failures", []))
    if isinstance(digest_failures, int):
        digest_failure_count = digest_failures
    elif isinstance(digest_failures, list):
        digest_failure_count = len(digest_failures)
    else:
        fail(f"{provider}_acceptance_bundle_digest_failures")
    if digest_failure_count:
        fail(f"{provider}_acceptance_bundle_digest_failures")

    return {
        "run_id": str(payload.get("run_id", "")),
        "schema_version": expected_schema,
        "source_class": source_class,
        "smoke_score": score,
        "minimum_score": minimum_score,
        "replay_status": "accepted",
        "digest_failures": digest_failure_count,
    }


def main() -> int:
    root = Path(os.environ.get("AO2_PROVIDER_PILOT_ACCEPTANCE_ROOT", "target/provider-pilot-acceptance"))
    tag = os.environ.get("AO2_PROVIDER_PILOT_PRESERVE_TAG", "").strip() or newest_tag(root)
    tag_dir = root / tag
    if not tag_dir.is_dir():
        fail(f"acceptance_tag_dir_missing:{tag_dir}")

    out_dir = Path(
        os.environ.get(
            "AO2_PROVIDER_PILOT_PRESERVE_OUT",
            f"target/release-evidence/provider-pilot-acceptance/{tag}",
        )
    )
    summary_path = Path(os.environ.get("AO2_PROVIDER_PILOT_PRESERVE_JSON", str(out_dir / "summary.json")))
    out_dir.mkdir(parents=True, exist_ok=True)
    summary_path.parent.mkdir(parents=True, exist_ok=True)

    providers: dict[str, dict[str, object]] = {}
    for provider in ("codex", "claude", "antigravity"):
        source = acceptance_path(tag_dir, provider)
        validation = validate_bundle(source, provider)
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
        "schema": SCHEMA,
        "status": "passed",
        "tag": tag,
        "acceptance_root": str(root),
        "preserved_root": str(out_dir),
        "summary_path": str(summary_path),
        "providers": providers,
        "generated_at_ms": int(time.time() * 1000),
    }
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"provider_pilot_acceptance_preservation=passed tag={tag} summary={summary_path}")
    print(f"provider_pilot_acceptance_preserved_root={out_dir}")
    return 0


raise SystemExit(main())
PY
