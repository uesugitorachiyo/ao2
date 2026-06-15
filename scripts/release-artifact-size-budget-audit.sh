#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_RELEASE_ARTIFACT_SIZE_BUDGET_AUDIT_ROOT:-$ROOT/target/release-artifact-size-budget-audit/$(date -u +%Y%m%dT%H%M%SZ)}"
SUMMARY="$OUT_ROOT/summary.json"
CLOSURE_INDEX="${AO2_RELEASE_ARTIFACT_SIZE_BUDGET_AUDIT_CLOSURE_INDEX:-}"
FIXTURE_DIR=""
REPO="${AO2_RELEASE_ARTIFACT_SIZE_BUDGET_AUDIT_REPO:-${GITHUB_REPOSITORY:-uesugitorachiyo/ao2}}"
ARTIFACT_LIMIT="${AO2_RELEASE_ARTIFACT_SIZE_BUDGET_AUDIT_LIMIT:-100}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --closure-index)
      CLOSURE_INDEX="${2:-}"
      if [ -z "$CLOSURE_INDEX" ]; then
        echo "--closure-index requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --fixture-dir)
      FIXTURE_DIR="${2:-}"
      if [ -z "$FIXTURE_DIR" ]; then
        echo "--fixture-dir requires a path" >&2
        exit 2
      fi
      shift 2
      ;;
    --repo)
      REPO="${2:-}"
      if [ -z "$REPO" ]; then
        echo "--repo requires owner/name" >&2
        exit 2
      fi
      shift 2
      ;;
    --limit)
      ARTIFACT_LIMIT="${2:-}"
      if [ -z "$ARTIFACT_LIMIT" ]; then
        echo "--limit requires a positive integer" >&2
        exit 2
      fi
      shift 2
      ;;
    *)
      echo "usage: $0 --closure-index <path> [--fixture-dir <path>] [--repo <owner/name>] [--limit <n>]" >&2
      exit 2
      ;;
  esac
done

if [ "${OPENAI_API_KEY+x}" = "x" ] || [ "${ANTHROPIC_API_KEY+x}" = "x" ]; then
  echo "provider API keys are not accepted by release artifact size budget audit" >&2
  exit 1
fi

if [ -z "$CLOSURE_INDEX" ]; then
  CLOSURE_INDEX="$ROOT/target/release-readiness/latest/artifact-closure-index.json"
fi

mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$CLOSURE_INDEX" "$FIXTURE_DIR" "$REPO" "$ARTIFACT_LIMIT" <<'PY'
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
closure_index_path = Path(sys.argv[4]).resolve()
fixture_dir = sys.argv[5]
repo = sys.argv[6]
artifact_limit = int(sys.argv[7])

def now_iso():
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

def load_artifacts_from_fixture(path):
    metadata_path = Path(path) / "artifacts.json"
    data = json.loads(metadata_path.read_text(encoding="utf-8"))
    if isinstance(data, dict):
        return data.get("artifacts", [])
    if isinstance(data, list):
        return data
    raise ValueError("fixture artifacts.json must be an object with artifacts or a list")

def load_artifacts_from_github(repo_name, limit):
    # gh api is read-only here; the workflow/job permissions only need actions:read.
    if limit <= 0:
        raise ValueError("artifact metadata limit must be positive")
    page_size = min(limit, 100)
    artifacts = []
    page = 1
    while True:
        result = subprocess.run(
            [
                "gh",
                "api",
                f"repos/{repo_name}/actions/artifacts",
                "--method",
                "GET",
                "-f",
                f"per_page={page_size}",
                "-f",
                f"page={page}",
            ],
            cwd=root,
            text=True,
            capture_output=True,
            check=False,
        )
        if result.returncode != 0:
            raise RuntimeError(result.stderr.strip() or result.stdout.strip() or "gh api artifacts failed")
        data = json.loads(result.stdout)
        page_artifacts = data.get("artifacts", [])
        if not page_artifacts:
            return artifacts
        artifacts.extend(page_artifacts)
        page += 1

def parse_created_at(item):
    value = item.get("created_at") or ""
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return datetime.min.replace(tzinfo=timezone.utc)

if not closure_index_path.is_file():
    print(f"artifact-closure-index.json not found: {closure_index_path}", file=sys.stderr)
    raise SystemExit(1)

closure = json.loads(closure_index_path.read_text(encoding="utf-8"))
budget = closure.get("artifact_size_budget", {})
required = {item.get("id"): item for item in closure.get("required_artifacts", [])}
budgeted_ids = list(budget.get("budgeted_artifact_ids", []))

source = "fixture" if fixture_dir else "github"
artifacts = load_artifacts_from_fixture(fixture_dir) if fixture_dir else load_artifacts_from_github(repo, artifact_limit)
metadata_by_name = {}
for item in artifacts:
    if item.get("expired") is True:
        continue
    name = item.get("name")
    if not name:
        continue
    current = metadata_by_name.get(name)
    if current is None or parse_created_at(item) > parse_created_at(current):
        metadata_by_name[name] = item

checked = []
violations = []
for artifact_id in budgeted_ids:
    contract = required.get(artifact_id, {})
    artifact_name = contract.get("artifact_name")
    artifact_budget = contract.get("artifact_size_budget", {})
    max_size = artifact_budget.get("max_size_bytes")
    enforcement = artifact_budget.get("enforcement")
    metadata = metadata_by_name.get(artifact_name or "")
    if (
        not artifact_name
        or not isinstance(max_size, int)
        or max_size <= 0
        or enforcement != "fail_if_hosted_artifact_exceeds_budget"
    ):
        violation = {
            "artifact_id": artifact_id,
            "artifact_name": artifact_name,
            "observed_size_bytes": None,
            "max_size_bytes": max_size,
            "reason": "invalid_size_budget_contract",
        }
        violations.append(violation)
        checked.append({**violation, "status": "failed"})
        continue
    if metadata is None:
        violation = {
            "artifact_id": artifact_id,
            "artifact_name": artifact_name,
            "observed_size_bytes": None,
            "max_size_bytes": max_size,
            "reason": "missing_hosted_artifact",
        }
        violations.append(violation)
        checked.append({**violation, "status": "failed"})
        continue
    observed_size = int(metadata.get("size_in_bytes", -1))
    item = {
        "artifact_id": artifact_id,
        "artifact_name": artifact_name,
        "hosted_artifact_id": metadata.get("id"),
        "observed_size_bytes": observed_size,
        "max_size_bytes": max_size,
        "created_at": metadata.get("created_at"),
        "status": "passed" if observed_size <= max_size else "failed",
    }
    checked.append(item)
    if observed_size > max_size:
        violations.append(
            {
                "artifact_id": artifact_id,
                "artifact_name": artifact_name,
                "observed_size_bytes": observed_size,
                "max_size_bytes": max_size,
                "reason": "size_budget_exceeded",
            }
        )

summary = {
    "schema_version": "ao2.release-artifact-size-budget-audit.v1",
    "status": "passed" if not violations else "failed",
    "generated_at": now_iso(),
    "repo": repo,
    "source": source,
    "artifact_metadata_count": len(artifacts),
    "closure_index": str(closure_index_path),
    "artifact_size_budget": budget,
    "check_count": len(checked),
    "passed_check_count": sum(1 for item in checked if item["status"] == "passed"),
    "failed_check_count": sum(1 for item in checked if item["status"] != "passed"),
    "checked_artifacts": checked,
    "violations": violations,
    "trust_boundary": {
        "local_only": bool(fixture_dir),
        "uses_github_actions_metadata": not bool(fixture_dir),
        "mutates_releases": False,
        "stores_credentials": False,
    },
}

summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print(f"status={summary['status']}")
print(f"check_count={summary['check_count']}")
print(f"failed_check_count={summary['failed_check_count']}")
if summary["status"] != "passed":
    for violation in violations:
        print(f"violation={violation}", file=sys.stderr)
    raise SystemExit(1)
PY
