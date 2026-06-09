#!/usr/bin/env python3
"""Preflight resource guard for archive-heavy AO2 tests."""

from __future__ import annotations

import json
import os
import shutil
import sys
import tempfile
import time
from pathlib import Path


SCHEMA_VERSION = "ao2.archive-heavy-test-resource-guard.v1"


def env_float(name: str, default: float) -> float:
    raw = os.environ.get(name)
    if raw is None or raw.strip() == "":
        return default
    try:
        return float(raw)
    except ValueError:
        raise SystemExit(f"{name} must be a number, got {raw!r}")


def env_bool(name: str, default: bool) -> bool:
    raw = os.environ.get(name)
    if raw is None or raw.strip() == "":
        return default
    return raw.strip().lower() in {"1", "true", "yes", "on"}


def disk_entry(label: str, path: Path, min_free_bytes: int) -> dict[str, object]:
    path.mkdir(parents=True, exist_ok=True)
    usage = shutil.disk_usage(path)
    return {
        "label": label,
        "path": str(path),
        "free_bytes": usage.free,
        "min_free_bytes": min_free_bytes,
        "ok": usage.free >= min_free_bytes,
    }


def prune_stale_guard_evidence(out_dir: Path, max_age_hours: float) -> int:
    if not out_dir.exists():
        return 0
    cutoff = time.time() - (max_age_hours * 3600)
    removed = 0
    for child in out_dir.iterdir():
        if child.name == "latest.json":
            continue
        try:
            if child.stat().st_mtime < cutoff:
                if child.is_dir():
                    shutil.rmtree(child)
                else:
                    child.unlink()
                removed += 1
        except FileNotFoundError:
            continue
    return removed


def main() -> int:
    repo_root = Path(os.environ.get("AO2_REPO_ROOT", Path.cwd())).resolve()
    out_dir = Path(
        os.environ.get(
            "AO2_ARCHIVE_TEST_RESOURCE_GUARD_DIR",
            repo_root / "target" / "archive-heavy-test-resources",
        )
    ).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    min_free_gb = env_float("AO2_ARCHIVE_TEST_MIN_FREE_GB", 2.0)
    min_free_bytes = int(min_free_gb * 1024 * 1024 * 1024)
    max_age_hours = env_float("AO2_ARCHIVE_TEST_STALE_HOURS", 24.0)
    expect_single_thread = env_bool("AO2_ARCHIVE_TEST_EXPECT_SINGLE_THREAD", True)

    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", repo_root / "target")).resolve()
    temp_dir = Path(tempfile.gettempdir()).resolve()
    removed_stale_entries = prune_stale_guard_evidence(out_dir, max_age_hours)

    disks = [
        disk_entry("repo", repo_root, min_free_bytes),
        disk_entry("cargo_target", target_dir, min_free_bytes),
        disk_entry("system_temp", temp_dir, min_free_bytes),
    ]
    status = "passed" if all(entry["ok"] for entry in disks) else "failed"
    report = {
        "schema_version": SCHEMA_VERSION,
        "status": status,
        "min_free_gb": min_free_gb,
        "single_thread_required": expect_single_thread,
        "required_cargo_test_args": ["--", "--test-threads=1"]
        if expect_single_thread
        else [],
        "disks": disks,
        "cleanup": {
            "guard_evidence_dir": str(out_dir),
            "stale_hours": max_age_hours,
            "removed_stale_entries": removed_stale_entries,
        },
    }

    report_path = out_dir / "latest.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report, indent=2, sort_keys=True))
    if status != "passed":
        print(
            "archive_heavy_test_resource_guard=failed "
            f"min_free_gb={min_free_gb} report={report_path}",
            file=sys.stderr,
        )
        return 1
    print(f"archive_heavy_test_resource_guard=passed report={report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
