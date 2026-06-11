#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AO2_RELEASE_TAG="${AO2_RELEASE_TAG:-v$("$ROOT/scripts/current-version.sh")}"
AO2_RELEASE_REPO="${AO2_RELEASE_REPO:-uesugitorachiyo/ao2}"
AO2_CP_RELEASE_TAG="${AO2_CP_RELEASE_TAG:-v0.1.12}"
AO2_CP_RELEASE_REPO="${AO2_CP_RELEASE_REPO:-uesugitorachiyo/ao2-control-plane}"
AO2_STABLE_PROMOTION_ROOT="${AO2_STABLE_PROMOTION_ROOT:-$ROOT/target/stable-promotion-workflow/latest}"
AO2_STABLE_PROMOTION_CONFIRM="${AO2_STABLE_PROMOTION_CONFIRM:-}"
# Default release train confirmation: AO2_STABLE_PROMOTION_CONFIRM=promote-stable-v0.4.80-v0.1.12
READINESS_ROOT="$AO2_STABLE_PROMOTION_ROOT/stable-release-readiness"
READINESS_SUMMARY="$READINESS_ROOT/summary.json"
SUMMARY="$AO2_STABLE_PROMOTION_ROOT/summary.json"
PLAN="$AO2_STABLE_PROMOTION_ROOT/plan.json"
PROMOTION_LOG="$AO2_STABLE_PROMOTION_ROOT/promotion.log"
READINESS_LOG="$AO2_STABLE_PROMOTION_ROOT/stable-readiness.log"

rm -rf "$AO2_STABLE_PROMOTION_ROOT"
mkdir -p "$AO2_STABLE_PROMOTION_ROOT"

AO2_STABLE_RELEASE_READINESS_ROOT="$READINESS_ROOT" npm run release:stable-readiness \
  > "$READINESS_LOG" 2>&1

python3 - "$READINESS_SUMMARY" "$PLAN" "$AO2_RELEASE_REPO" "$AO2_RELEASE_TAG" \
  "$AO2_CP_RELEASE_REPO" "$AO2_CP_RELEASE_TAG" "$AO2_STABLE_PROMOTION_CONFIRM" <<'PY'
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

readiness_path = Path(sys.argv[1])
plan_path = Path(sys.argv[2])
ao2_repo = sys.argv[3]
ao2_tag = sys.argv[4]
cp_repo = sys.argv[5]
cp_tag = sys.argv[6]
confirm = sys.argv[7]

readiness = json.loads(readiness_path.read_text(encoding="utf-8"))
allowed_channel_blockers = {"stable_release_absent", "current_channel_is_prerelease"}
expected = {
    ("ao2", ao2_repo, ao2_tag),
    ("ao2-control-plane", cp_repo, cp_tag),
}
observed = {
    (component.get("name"), component.get("repo"), component.get("tag"))
    for component in readiness.get("components", [])
}
missing_components = sorted(
    (
        {"name": name, "repo": repo, "tag": tag}
        for name, repo, tag in expected.difference(observed)
    ),
    key=lambda item: (item["name"], item["repo"], item["tag"]),
)
non_channel_blockers = [
    blocker
    for blocker in readiness.get("promotion_blockers", [])
    if blocker.get("code") not in allowed_channel_blockers
]
channel_blockers = [
    blocker
    for blocker in readiness.get("promotion_blockers", [])
    if blocker.get("code") in allowed_channel_blockers
]

required_confirm = f"promote-stable-{ao2_tag}-{cp_tag}"
confirmed = confirm == required_confirm
stable_channel_only = not non_channel_blockers and not missing_components and bool(channel_blockers)
already_stable = bool(readiness.get("stable_release_ready")) and not readiness.get("promotion_blockers")
status = "ready_to_promote" if stable_channel_only else "already_stable" if already_stable else "blocked"

blockers = []
if missing_components:
    blockers.append(
        {
            "code": "release_component_missing_from_readiness",
            "severity": "blocking",
            "components": missing_components,
            "message": "Stable promotion requires AO2 and ao2-control-plane readiness components.",
        }
    )
if non_channel_blockers:
    blockers.append(
        {
            "code": "non_channel_promotion_blockers_present",
            "severity": "blocking",
            "blockers": non_channel_blockers,
            "message": "Stable promotion can only proceed after non-channel blockers are resolved.",
        }
    )

plan = {
    "schema_version": "ao2.stable-promotion-workflow.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": status,
    "dry_run": not confirmed,
    "confirmed": confirmed,
    "required_confirm": required_confirm,
    "stable_channel_only": stable_channel_only,
    "readiness_summary": str(readiness_path),
    "promotion_targets": [
        {"name": "ao2", "repo": ao2_repo, "tag": ao2_tag},
        {"name": "ao2-control-plane", "repo": cp_repo, "tag": cp_tag},
    ],
    "channel_blockers": channel_blockers,
    "non_channel_blockers": non_channel_blockers,
    "blockers": blockers,
    "planned_commands": [
        f"gh release edit {ao2_tag} --repo {ao2_repo} --prerelease=false --latest",
        f"gh release edit {cp_tag} --repo {cp_repo} --prerelease=false --latest",
    ],
    "trust_boundary": {
        "queries_public_releases": True,
        "mutates_releases": confirmed,
        "stores_credentials": False,
    },
}
plan_path.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

plan_status="$(
  python3 - "$PLAN" <<'PY'
import json
import sys
from pathlib import Path
print(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))["status"])
PY
)"

promotion_status="not_attempted"
if [ "$AO2_STABLE_PROMOTION_CONFIRM" = "promote-stable-$AO2_RELEASE_TAG-$AO2_CP_RELEASE_TAG" ]; then
  if [ "$plan_status" != "ready_to_promote" ]; then
    echo "refusing stable promotion because plan status is $plan_status" >&2
    cp "$PLAN" "$SUMMARY"
    exit 1
  fi
  {
    gh release edit "$AO2_RELEASE_TAG" \
      --repo "$AO2_RELEASE_REPO" \
      --prerelease=false \
      --latest
    gh release edit "$AO2_CP_RELEASE_TAG" \
      --repo "$AO2_CP_RELEASE_REPO" \
      --prerelease=false \
      --latest
  } > "$PROMOTION_LOG" 2>&1
  promotion_status="promoted"
fi

python3 - "$PLAN" "$SUMMARY" "$promotion_status" "$PROMOTION_LOG" <<'PY'
import json
import sys
from pathlib import Path

plan_path = Path(sys.argv[1])
summary_path = Path(sys.argv[2])
promotion_status = sys.argv[3]
promotion_log = Path(sys.argv[4])
payload = json.loads(plan_path.read_text(encoding="utf-8"))
if promotion_status == "promoted":
    payload["status"] = "promoted"
payload["promotion_status"] = promotion_status
payload["promotion_log"] = str(promotion_log)
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={payload['status']}")
print(f"dry_run={str(payload['dry_run']).lower()}")
print(f"promotion_status={promotion_status}")
PY
