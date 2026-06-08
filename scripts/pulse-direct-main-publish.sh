#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="${AO2_PULSE_DIRECT_MAIN_PUBLISH_REPO_ROOT:-$ROOT}"
OUT_ROOT="${AO2_PULSE_DIRECT_MAIN_PUBLISH_ROOT:-$ROOT/target/pulse-direct-main-publish/latest}"
SUMMARY="$OUT_ROOT/summary.json"
LOG_DIR="$OUT_ROOT/logs"
BRANCH="${AO2_PULSE_DIRECT_MAIN_PUBLISH_BRANCH:-main}"
REMOTE="${AO2_PULSE_DIRECT_MAIN_PUBLISH_REMOTE:-origin}"
PUSH="${AO2_PULSE_DIRECT_MAIN_PUBLISH_PUSH:-1}"
VERIFY_COMMAND="${AO2_PULSE_DIRECT_MAIN_PUBLISH_VERIFY_COMMAND:-PYTHONDONTWRITEBYTECODE=1 python3 -m pytest tests/test_public_stabilization.py -q}"
MESSAGE="${AO2_PULSE_DIRECT_MAIN_PUBLISH_MESSAGE:-Pulse direct main advancement}"
REASON="${AO2_PULSE_DIRECT_MAIN_PUBLISH_REASON:-manual}"

# Guarded direct-main flow: git fetch, git commit, verify ancestry, git push.
rm -rf "$OUT_ROOT"
mkdir -p "$LOG_DIR"

python3 - "$REPO_ROOT" "$OUT_ROOT" "$SUMMARY" "$LOG_DIR" "$BRANCH" "$REMOTE" "$PUSH" "$VERIFY_COMMAND" "$MESSAGE" "$REASON" <<'PY'
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

repo = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
log_dir = Path(sys.argv[4]).resolve()
branch = sys.argv[5]
remote = sys.argv[6]
push_enabled = sys.argv[7] == "1"
verify_command = sys.argv[8]
message = sys.argv[9]
reason = sys.argv[10]

def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

payload = {
    "schema_version": "ao2.pulse-direct-main-publish.v1",
    "generated_at_utc": utc_now(),
    "status": "failed",
    "reason": reason,
    "repo_root": str(repo),
    "branch": branch,
    "remote": remote,
    "push_enabled": push_enabled,
    "changed_paths": [],
    "checks": [],
    "trust_boundary": {
        "local_only": False,
        "stores_credentials": False,
        "side_effects": "git_commit_and_optional_push",
    },
}

def write_summary() -> None:
    summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")

RECURSIVE_PULSE_ENV_FLAGS = [
    "AO2_PULSE_AUTO_ADVANCE_DIRECT_MAIN_PUBLISH",
    "AO2_PULSE_AUTO_ADVANCE_LOCAL_ONLY_WHILE_PR_BLOCKED",
    "AO2_PULSE_AUTO_ADVANCE_GENERATE_NEXT",
    "AO2_PULSE_GENERATE_NEXT_LOCAL_ONLY",
]

def verification_env() -> dict[str, str]:
    env = dict(os.environ)
    for name in RECURSIVE_PULSE_ENV_FLAGS:
        env[name] = "0"
    return env

def run(
    name: str,
    args: list[str],
    *,
    check: bool = True,
    shell: bool = False,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    log_path = log_dir / f"{name}.log"
    with log_path.open("w", encoding="utf-8") as log:
        if shell:
            log.write(f"$ {args[0]}\n")
            result = subprocess.run(
                args[0],
                cwd=repo,
                shell=True,
                text=True,
                stdout=log,
                stderr=subprocess.STDOUT,
                check=False,
                env=env,
            )
        else:
            log.write("$ " + " ".join(args) + "\n")
            result = subprocess.run(
                args,
                cwd=repo,
                text=True,
                stdout=log,
                stderr=subprocess.STDOUT,
                check=False,
                env=env,
            )
    payload["checks"].append({
        "name": name,
        "command": args[0] if shell else " ".join(args),
        "status": "passed" if result.returncode == 0 else "failed",
        "exit_code": int(result.returncode),
        "log": str(log_path),
    })
    if check and result.returncode != 0:
        payload["status"] = "failed"
        payload["reason"] = f"{name}_failed"
        write_summary()
        raise SystemExit(1)
    return result

def git_output(args: list[str]) -> str:
    return subprocess.check_output(["git", *args], cwd=repo, text=True).strip()

def fail(reason_text: str, **extra: object) -> None:
    payload["status"] = "failed"
    payload["reason"] = reason_text
    payload.update(extra)
    write_summary()
    raise SystemExit(1)

if not repo.is_dir():
    fail("repo_root_missing")

run("git_status_probe", ["git", "status", "--short"])
current_branch = git_output(["branch", "--show-current"])
payload["current_branch"] = current_branch
if current_branch != branch:
    fail("branch_mismatch", expected_branch=branch, observed_branch=current_branch)

run("git_fetch", ["git", "fetch", remote, branch])
head_before = git_output(["rev-parse", "HEAD"])
remote_ref = f"{remote}/{branch}"
remote_before = git_output(["rev-parse", remote_ref])
payload["head_before"] = head_before
payload["remote_before"] = remote_before
if head_before != remote_before:
    fail("main_not_equal_to_remote_before_publish", head=head_before, remote_head=remote_before)

tracked = [line for line in git_output(["diff", "--name-only", "HEAD", "--"]).splitlines() if line]
untracked = [line for line in git_output(["ls-files", "--others", "--exclude-standard"]).splitlines() if line]
changed_paths = sorted(dict.fromkeys(tracked + untracked))
payload["changed_paths"] = changed_paths
if not changed_paths:
    payload["status"] = "skipped"
    payload["reason"] = "no_tracked_or_untracked_changes"
    write_summary()
    print(f"summary={summary_path}")
    print("status=skipped")
    raise SystemExit(0)

disallowed_prefixes = ("target/", ".ao2-local/", ".git/", "node_modules/")
disallowed_names = {".env", ".env.local", ".env.production"}
bad_paths = [
    path for path in changed_paths
    if path.startswith(disallowed_prefixes) or Path(path).name in disallowed_names
]
if bad_paths:
    fail("disallowed_paths_present", disallowed_paths=bad_paths)

forbidden_terms = [
    "Bearer" + " ",
    "-----BEGIN" + " ",
    "PRIVATE" + " KEY",
    "ghp" + "_",
    "github" + "_pat" + "_",
    "xoxb" + "-",
    "AK" + "IA",
]
secret_hits = []
for rel in changed_paths:
    path = repo / rel
    if not path.is_file() or path.stat().st_size > 2_000_000:
        continue
    text = path.read_text(encoding="utf-8", errors="ignore")
    for term in forbidden_terms:
        if term in text:
            secret_hits.append({"path": rel, "term": term})
if secret_hits:
    fail("secret_pattern_detected", secret_hits=secret_hits)

verification = run("verification", [verify_command], shell=True, env=verification_env())
payload["verification"] = {
    "command": verify_command,
    "status": "passed" if verification.returncode == 0 else "failed",
    "exit_code": int(verification.returncode),
    "log": str(log_dir / "verification.log"),
    "recursive_pulse_env_forced_off": RECURSIVE_PULSE_ENV_FLAGS,
}

run("git_add", ["git", "add", "--", *changed_paths])
cached = run("git_diff_cached", ["git", "diff", "--cached", "--check"], check=True)
payload["diff_check"] = {"status": "passed", "exit_code": int(cached.returncode)}

commit_message = f"{message}\n\nReason: {reason}\n\nGenerated-by: ao2-pulse-direct-main-publish"
run("git_commit", ["git", "commit", "-m", commit_message])
commit_sha = git_output(["rev-parse", "HEAD"])
payload["commit"] = {"sha": commit_sha, "message": message}

run("git_fetch_after_commit", ["git", "fetch", remote, branch])
ancestor = run("git_merge_base_remote_ancestor", ["git", "merge-base", "--is-ancestor", remote_ref, "HEAD"], check=False)
if ancestor.returncode != 0:
    fail("remote_not_ancestor_after_commit", commit=payload["commit"])

if push_enabled:
    run("git_push", ["git", "push", remote, f"HEAD:{branch}"])
    remote_after = git_output(["rev-parse", remote_ref])
    payload["remote_after"] = remote_after
    if remote_after != commit_sha:
        fail("remote_after_push_mismatch", commit=payload["commit"], remote_after=remote_after)
    payload["status"] = "passed"
    payload["reason"] = "committed_and_pushed"
else:
    payload["status"] = "passed"
    payload["reason"] = "committed_without_push"

write_summary()
print(f"summary={summary_path}")
print(f"status={payload['status']}")
PY
