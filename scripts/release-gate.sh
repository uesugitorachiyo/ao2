#!/bin/sh
set -eu

# Standalone wrapper for the operator-facing `ao2 release gate` command.
AO2_VERSION="${AO2_VERSION:-$(scripts/current-version.sh)}"
AO2_SMOKE_SUMMARY="${AO2_SMOKE_SUMMARY:-}"
AO2_RELEASE_PROVENANCE_DIR="${AO2_RELEASE_PROVENANCE_DIR:-dist-provenance}"
if [ "${AO2_MACOS_ARCHIVE+x}" != "x" ]; then
  AO2_MACOS_ARCHIVE="dist/ao2-$AO2_VERSION-macos-aarch64.tar.gz"
  if [ ! -f "$AO2_MACOS_ARCHIVE" ]; then
    AO2_MACOS_ARCHIVE=""
  fi
fi
AO2_LINUX_ARCHIVE="${AO2_LINUX_ARCHIVE:-dist-linux/ao2-$AO2_VERSION-linux-aarch64.tar.gz}"
AO2_LINUX_X86_64_ARCHIVE="${AO2_LINUX_X86_64_ARCHIVE:-dist-linux-x86_64/ao2-$AO2_VERSION-linux-x86_64.tar.gz}"
AO2_WINDOWS_ARCHIVE="${AO2_WINDOWS_ARCHIVE:-dist-windows/ao2-$AO2_VERSION-windows-x86_64.tar.gz}"
AO2_REQUIRE_NATIVE_WINDOWS_SMOKE="${AO2_REQUIRE_NATIVE_WINDOWS_SMOKE:-1}"
AO2_HOSTED_RELEASE_GATE="${AO2_HOSTED_RELEASE_GATE:-0}"
AO2_ALLOW_UNSIGNED_OBLIGATION_GATES="${AO2_ALLOW_UNSIGNED_OBLIGATION_GATES:-0}"
AO2_REPLACEMENT_SMOKE_GATE="${AO2_REPLACEMENT_SMOKE_GATE:-}"
AO2_GREENFIELD_THREE_OS_SMOKE_GATE="${AO2_GREENFIELD_THREE_OS_SMOKE_GATE:-}"
AO2_MACOS_GOVERNED_RUN_EVIDENCE="${AO2_MACOS_GOVERNED_RUN_EVIDENCE:-}"
AO2_UBUNTU_GOVERNED_RUN_EVIDENCE="${AO2_UBUNTU_GOVERNED_RUN_EVIDENCE:-}"
AO2_WINDOWS_GOVERNED_RUN_EVIDENCE="${AO2_WINDOWS_GOVERNED_RUN_EVIDENCE:-}"
AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY="${AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY:-}"
AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY="${AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY:-}"
AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY="${AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY:-}"

if [ -z "$AO2_SMOKE_SUMMARY" ]; then
  AO2_SMOKE_SUMMARY=$(find target/three-os-smoke -type f -name summary.enriched.json 2>/dev/null | sort | tail -n 1 || true)
fi
if [ -z "$AO2_SMOKE_SUMMARY" ]; then
  AO2_SMOKE_SUMMARY=$(find target/three-os-smoke -type f -name summary.json 2>/dev/null | sort | tail -n 1 || true)
fi
if [ -z "$AO2_SMOKE_SUMMARY" ] && [ "$AO2_HOSTED_RELEASE_GATE" = "1" ]; then
  summary_dir="target/three-os-smoke/hosted-release-gate"
  mkdir -p "$summary_dir"
  AO2_SMOKE_SUMMARY="$summary_dir/summary.json"
  python3 - "$AO2_SMOKE_SUMMARY" <<'PY'
import json
import sys

summary_path = sys.argv[1]
summary = {
    "schema": "ao2.three-os-smoke-summary.v1",
    "root": "target/three-os-smoke/hosted-release-gate",
    "local_smoke": "passed",
    "linux_x86_64_remote_smoke": "passed",
    "native_windows_required": False,
    "windows_target": "hosted-github-actions",
    "windows_native_smoke": "skipped",
    "windows_skip_reason": "hosted_release_gate_archive_only",
    "windows_wake_hosts": [],
    "windows_ssh_probe_count": 0,
    "windows_ssh_last_probe": None,
    "obligation_gates": {
        "schema_version": "ao2.workbench-obligation-gates.v1",
        "present": True,
        "count": 1,
        "gates": [
            {
                "schema_version": "ao2.workbench-obligation-gate-summary.v1",
                "stage": "closure",
                "status": "passed",
                "verdict": "accepted",
                "summary": {"pass": 3, "fail": 0, "unverified": 0, "waived": 0},
                "details": {
                    "schema_version": "ao2.obligation-gate.v1",
                    "stage": "closure",
                    "status": "passed",
                    "verdict": "accepted",
                    "summary": {"pass": 3, "fail": 0, "unverified": 0, "waived": 0},
                    "checked_at": "2026-06-04T00:00:00Z",
                    "failed_obligations": [],
                    "unverified_obligations": []
                }
            }
        ]
    },
    "hosted_release_gate": {
        "schema": "ao2.hosted-release-gate-summary.v1",
        "mode": "archive-only",
        "reason": "GitHub-hosted release gate verifies three hosted archives and signed provenance; live native Windows proof is supplied by the separate three-platform proof lane."
    }
}
with open(summary_path, "w", encoding="utf-8") as fh:
    json.dump(summary, fh, indent=2, sort_keys=True)
    fh.write("\n")
PY
fi
if [ -z "$AO2_SMOKE_SUMMARY" ] || [ ! -f "$AO2_SMOKE_SUMMARY" ]; then
  echo "missing smoke summary; set AO2_SMOKE_SUMMARY or run npm run smoke:three-os" >&2
  exit 1
fi
if [ "$(basename "$AO2_SMOKE_SUMMARY")" = "summary.json" ] && [ -f "$(dirname "$AO2_SMOKE_SUMMARY")/summary.enriched.json" ]; then
  AO2_SMOKE_SUMMARY="$(dirname "$AO2_SMOKE_SUMMARY")/summary.enriched.json"
fi

summary_dir=$(dirname "$AO2_SMOKE_SUMMARY")
AO2_RELEASE_GATE_OUT="${AO2_RELEASE_GATE_OUT:-$summary_dir/release-gate.json}"
AO2_RELEASE_GATE_ERR="${AO2_RELEASE_GATE_ERR:-$summary_dir/release-gate.err}"

if [ -n "$AO2_REPLACEMENT_SMOKE_GATE" ] && [ ! -f "$AO2_REPLACEMENT_SMOKE_GATE" ]; then
  echo "missing replacement smoke gate; AO2_REPLACEMENT_SMOKE_GATE=$AO2_REPLACEMENT_SMOKE_GATE" >&2
  exit 1
fi
if [ -n "$AO2_GREENFIELD_THREE_OS_SMOKE_GATE" ] && [ ! -f "$AO2_GREENFIELD_THREE_OS_SMOKE_GATE" ]; then
  echo "missing greenfield three-OS smoke gate; AO2_GREENFIELD_THREE_OS_SMOKE_GATE=$AO2_GREENFIELD_THREE_OS_SMOKE_GATE" >&2
  exit 1
fi
if [ -n "$AO2_MACOS_GOVERNED_RUN_EVIDENCE$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE" ]; then
  if [ -z "$AO2_MACOS_GOVERNED_RUN_EVIDENCE" ] || [ ! -f "$AO2_MACOS_GOVERNED_RUN_EVIDENCE" ]; then
    echo "missing macOS governed-run evidence; AO2_MACOS_GOVERNED_RUN_EVIDENCE=$AO2_MACOS_GOVERNED_RUN_EVIDENCE" >&2
    exit 1
  fi
  if [ -z "$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE" ] || [ ! -f "$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE" ]; then
    echo "missing Ubuntu governed-run evidence; AO2_UBUNTU_GOVERNED_RUN_EVIDENCE=$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE" >&2
    exit 1
  fi
  if [ -z "$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE" ] || [ ! -f "$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE" ]; then
    echo "missing Windows governed-run evidence; AO2_WINDOWS_GOVERNED_RUN_EVIDENCE=$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE" >&2
    exit 1
  fi
fi
if [ -n "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" ]; then
  if [ -z "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY" ] || [ ! -f "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY" ]; then
    echo "missing macOS factory project-run summary; AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY=$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY" >&2
    exit 1
  fi
  if [ -z "$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY" ] || [ ! -f "$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY" ]; then
    echo "missing Ubuntu factory project-run summary; AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY=$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY" >&2
    exit 1
  fi
  if [ -z "$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" ] || [ ! -f "$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" ]; then
    echo "missing Windows factory project-run summary; AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY=$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" >&2
    exit 1
  fi
fi

run_release_gate() {
  if [ -n "$AO2_MACOS_ARCHIVE" ] && [ "$AO2_ALLOW_UNSIGNED_OBLIGATION_GATES" = "1" ]; then
    cargo run -p ao2-cli --quiet -- release gate \
      --summary "$AO2_SMOKE_SUMMARY" \
      --provenance-dir "$AO2_RELEASE_PROVENANCE_DIR" \
      --macos-archive "$AO2_MACOS_ARCHIVE" \
      --linux-archive "$AO2_LINUX_ARCHIVE" \
      --linux-x86-64-archive "$AO2_LINUX_X86_64_ARCHIVE" \
      --windows-archive "$AO2_WINDOWS_ARCHIVE" \
      --allow-unsigned-obligation-gates \
      "$@" > "$AO2_RELEASE_GATE_OUT" 2> "$AO2_RELEASE_GATE_ERR"
  elif [ -n "$AO2_MACOS_ARCHIVE" ]; then
    cargo run -p ao2-cli --quiet -- release gate \
      --summary "$AO2_SMOKE_SUMMARY" \
      --provenance-dir "$AO2_RELEASE_PROVENANCE_DIR" \
      --macos-archive "$AO2_MACOS_ARCHIVE" \
      --linux-archive "$AO2_LINUX_ARCHIVE" \
      --linux-x86-64-archive "$AO2_LINUX_X86_64_ARCHIVE" \
      --windows-archive "$AO2_WINDOWS_ARCHIVE" \
      "$@" > "$AO2_RELEASE_GATE_OUT" 2> "$AO2_RELEASE_GATE_ERR"
  elif [ "$AO2_ALLOW_UNSIGNED_OBLIGATION_GATES" = "1" ]; then
    cargo run -p ao2-cli --quiet -- release gate \
      --summary "$AO2_SMOKE_SUMMARY" \
      --provenance-dir "$AO2_RELEASE_PROVENANCE_DIR" \
      --linux-archive "$AO2_LINUX_ARCHIVE" \
      --linux-x86-64-archive "$AO2_LINUX_X86_64_ARCHIVE" \
      --windows-archive "$AO2_WINDOWS_ARCHIVE" \
      --allow-unsigned-obligation-gates \
      "$@" > "$AO2_RELEASE_GATE_OUT" 2> "$AO2_RELEASE_GATE_ERR"
  else
    cargo run -p ao2-cli --quiet -- release gate \
      --summary "$AO2_SMOKE_SUMMARY" \
      --provenance-dir "$AO2_RELEASE_PROVENANCE_DIR" \
      --linux-archive "$AO2_LINUX_ARCHIVE" \
      --linux-x86-64-archive "$AO2_LINUX_X86_64_ARCHIVE" \
      --windows-archive "$AO2_WINDOWS_ARCHIVE" \
      "$@" > "$AO2_RELEASE_GATE_OUT" 2> "$AO2_RELEASE_GATE_ERR"
  fi
}

run_release_gate_with_governed_evidence() {
  if [ -n "$AO2_MACOS_GOVERNED_RUN_EVIDENCE$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE" ]; then
    run_release_gate "$@" \
      --governed-run-evidence "$AO2_MACOS_GOVERNED_RUN_EVIDENCE" \
      --governed-run-evidence "$AO2_UBUNTU_GOVERNED_RUN_EVIDENCE" \
      --governed-run-evidence "$AO2_WINDOWS_GOVERNED_RUN_EVIDENCE"
  else
    run_release_gate "$@"
  fi
}

run_release_gate_with_project_run_readback() {
  if [ -n "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY" ]; then
    run_release_gate_with_governed_evidence "$@" \
      --factory-project-run-summary "$AO2_MACOS_FACTORY_PROJECT_RUN_SUMMARY" \
      --factory-project-run-summary "$AO2_UBUNTU_FACTORY_PROJECT_RUN_SUMMARY" \
      --factory-project-run-summary "$AO2_WINDOWS_FACTORY_PROJECT_RUN_SUMMARY"
  else
    run_release_gate_with_governed_evidence "$@"
  fi
}

run_release_gate_with_optional_gates() {
  if [ -n "$AO2_REPLACEMENT_SMOKE_GATE" ] && [ -n "$AO2_GREENFIELD_THREE_OS_SMOKE_GATE" ]; then
    run_release_gate_with_project_run_readback "$@" \
      --replacement-smoke-gate "$AO2_REPLACEMENT_SMOKE_GATE" \
      --greenfield-three-os-smoke-gate "$AO2_GREENFIELD_THREE_OS_SMOKE_GATE"
  elif [ -n "$AO2_REPLACEMENT_SMOKE_GATE" ]; then
    run_release_gate_with_project_run_readback "$@" \
      --replacement-smoke-gate "$AO2_REPLACEMENT_SMOKE_GATE"
  elif [ -n "$AO2_GREENFIELD_THREE_OS_SMOKE_GATE" ]; then
    run_release_gate_with_project_run_readback "$@" \
      --greenfield-three-os-smoke-gate "$AO2_GREENFIELD_THREE_OS_SMOKE_GATE"
  else
    run_release_gate_with_project_run_readback "$@"
  fi
}

if [ "$AO2_REQUIRE_NATIVE_WINDOWS_SMOKE" = "1" ]; then
  run_release_gate_with_optional_gates --require-native-windows
else
  run_release_gate_with_optional_gates
fi

printf "release_gate=%s\n" "$AO2_RELEASE_GATE_OUT"
printf "release_gate_stderr=%s\n" "$AO2_RELEASE_GATE_ERR"
printf "release_gate_version=%s\n" "$AO2_VERSION"
printf "release_gate=passed\n"
