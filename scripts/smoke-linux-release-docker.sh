#!/bin/sh
set -eu

AO2_VERSION="${AO2_VERSION:-$(scripts/current-version.sh)}"
AO2_LINUX_X86_64_ARCHIVE="${AO2_LINUX_X86_64_ARCHIVE:-dist-linux-x86_64/ao2-$AO2_VERSION-linux-x86_64.tar.gz}"
AO2_LINUX_X86_64_IMAGE="${AO2_LINUX_X86_64_IMAGE:-${AO2_UBUNTU_IMAGE:-ubuntu:24.04}}"
AO2_LINUX_X86_64_DOCKER_LOG="${AO2_LINUX_X86_64_DOCKER_LOG:-}"
AO2_LINUX_X86_64_SMOKE_ROOT="${AO2_LINUX_X86_64_SMOKE_ROOT:-$PWD/target/release-smoke-linux-x86_64/$(date +%Y%m%d%H%M%S)}"
AO2_RELEASE_ROLLBACK_VERIFY="${AO2_RELEASE_ROLLBACK_VERIFY:-1}"

if [ ! -f "$AO2_LINUX_X86_64_ARCHIVE" ]; then
  echo "missing Linux x86_64 release archive: $AO2_LINUX_X86_64_ARCHIVE" >&2
  exit 1
fi

archive_name=$(basename "$AO2_LINUX_X86_64_ARCHIVE")
archive_dir=$(CDPATH= cd -- "$(dirname -- "$AO2_LINUX_X86_64_ARCHIVE")" && pwd)
mkdir -p "$AO2_LINUX_X86_64_SMOKE_ROOT"
AO2_LINUX_X86_64_SMOKE_ROOT=$(CDPATH= cd -- "$AO2_LINUX_X86_64_SMOKE_ROOT" && pwd)

run_smoke() {
  docker run --rm --platform linux/amd64 \
    -e AO2_RELEASE_ROLLBACK_VERIFY="$AO2_RELEASE_ROLLBACK_VERIFY" \
    -v "$archive_dir":/dist:ro \
    -v "$AO2_LINUX_X86_64_SMOKE_ROOT":/smoke \
    "$AO2_LINUX_X86_64_IMAGE" \
    sh -lc '
      set -eu
      export DEBIAN_FRONTEND=noninteractive
      if ! command -v git >/dev/null 2>&1 || ! command -v jq >/dev/null 2>&1; then
        apt-get update >/dev/null
        apt-get install -y --no-install-recommends ca-certificates git jq >/dev/null
        rm -rf /var/lib/apt/lists/*
      fi
      archive_name="$1"
      work=/smoke/linux-x86_64
      extract="$work/extract"
      install_dir="$work/bin"
      rollback_runner="$extract/bin/ao2"
      repo="$work/repo"
      rm -rf "$work"
      mkdir -p "$extract" "$repo/src"
      git init -q "$repo"
      printf "initial\n" > "$repo/src/value.txt"
      git -C "$repo" add -A
      GIT_AUTHOR_NAME="AO2 Test" \
        GIT_AUTHOR_EMAIL="ao2-test@example.invalid" \
        GIT_COMMITTER_NAME="AO2 Test" \
        GIT_COMMITTER_EMAIL="ao2-test@example.invalid" \
        git -C "$repo" commit -q -m fixture
      tar -xzf "/dist/$archive_name" -C "$extract"
      test -f "$extract/RELEASE-MANIFEST.json"
      grep -q "\"schema_version\": \"ao2.release-manifest.v1\"" "$extract/RELEASE-MANIFEST.json"
      grep -q "\"target\": \"linux-x86_64\"" "$extract/RELEASE-MANIFEST.json"
      grep -q "\"binary\": \"ao2\"" "$extract/RELEASE-MANIFEST.json"
      AO2_INSTALL_DIR="$install_dir" sh "$extract/install.sh"
      "$install_dir/ao2" --help >/dev/null
      "$install_dir/ao2" version --json > "$work/version.json"
      grep -q "\"target\": \"linux-x86_64\"" "$work/version.json"
      if [ "$AO2_RELEASE_ROLLBACK_VERIFY" = "1" ]; then
        test -x "$rollback_runner"
        cp "$install_dir/ao2" "$install_dir/ao2.rollback"
        "$rollback_runner" install rollback --install-dir "$install_dir" --target-label linux-x86_64 > "$work/rollback.json"
        grep -q "\"status\": \"rolled_back\"" "$work/rollback.json"
        "$install_dir/ao2" version --json > "$work/version-after-rollback.json"
        grep -q "\"target\": \"linux-x86_64\"" "$work/version-after-rollback.json"
        printf "linux_x86_64_install_rollback=passed\n"
      fi
      "$install_dir/ao2" adapter doctor --provider scripted >/dev/null
      "$install_dir/ao2" provider matrix --json >/dev/null
      "$install_dir/ao2" provider contract --verify --require codex --json > "$work/provider-contract-verify.json"
      grep -q "\"schema\": \"ao2.provider-contract-verification.v1\"" "$work/provider-contract-verify.json"
      grep -q "\"status\": \"verified\"" "$work/provider-contract-verify.json"
      cat > "$work/workflow.yaml" <<'"'"'YAML'"'"'
id: linux-x86-64-install-smoke-repair
version: smoke
template_kind: real_project
objective: Verify installed AO2 can run a scripted real-project repair on Docker Linux x86_64.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: test "$(cat src/value.txt)" = ok
acceptance:
  - Installed AO2 runs a scripted repair.
  - Replay has zero digest failures.
YAML
      cat > "$work/prompt.sh" <<'"'"'SH'"'"'
mkdir -p src
if [ -n "${AO2_REPAIR_VERIFIER_OUTPUT:-}" ]; then
  printf "ok\n" > src/value.txt
else
  printf "bad\n" > src/value.txt
fi
printf "Summary: wrote repair-aware smoke value\n"
printf "Changed files: src/value.txt\n"
SH
      /usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY "$install_dir/ao2" run "$work/workflow.yaml" \
        --target "$repo" \
        --run-id linux-x86-64-install-smoke-repair \
        --provider scripted \
        --provider-prompt-file "$work/prompt.sh" \
        --max-repair-attempts 1 > "$work/run.out"
      approval_count=0
      while grep -q "status=WaitingForApproval" "$work/run.out"; do
        ticket_id=$(jq -r ".approvals[] | select(.requested_action == \"sandbox:apply\" and .status == \"pending\") | .ticket_id" \
          "$repo/.ao2/runs/linux-x86-64-install-smoke-repair/evidence-pack/evidence-pack.json")
        test -n "$ticket_id"
        "$install_dir/ao2" approve "$ticket_id" \
          --target "$repo" \
          --approver human:release-smoke > "$work/approve.out"
        grep -q "status=approved" "$work/approve.out"
        approval_count=$((approval_count + 1))
        test "$approval_count" -le 2
        "$install_dir/ao2" run --resume linux-x86-64-install-smoke-repair \
          --target "$repo" > "$work/run.out"
      done
      test "$approval_count" -eq 2
      grep -q "status=Accepted" "$work/run.out"
      /usr/bin/env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY "$install_dir/ao2" replay linux-x86-64-install-smoke-repair --target "$repo" > "$work/replay.json"
      grep -q "\"digest_failures\": \\[\\]" "$work/replay.json"
      test "$(cat "$repo/src/value.txt")" = ok
      printf "linux_x86_64_evidence=%s\n" "$repo/.ao2/runs/linux-x86-64-install-smoke-repair/evidence-pack/evidence-pack.json"
      printf "linux_x86_64_cockpit=%s\n" "$repo/.ao2/runs/linux-x86-64-install-smoke-repair/cockpit/index.html"
      printf "linux_x86_64_docker_smoke=passed\n"
    ' sh "$archive_name"
}

if [ -n "$AO2_LINUX_X86_64_DOCKER_LOG" ]; then
  run_smoke > "$AO2_LINUX_X86_64_DOCKER_LOG" 2>&1
  cat "$AO2_LINUX_X86_64_DOCKER_LOG"
else
  run_smoke
fi
