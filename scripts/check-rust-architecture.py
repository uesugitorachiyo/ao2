#!/usr/bin/env python3
"""Check AO2 CLI architecture ratchet budgets.

The guard is intentionally structural. It does not prove behavior parity by
itself; it prevents the known monoliths and risky boundaries from growing while
the decomposition waves move behavior-neutral code into domain modules.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BASELINE = ROOT / ".github" / "architecture-baseline.json"
PROD_DIR = ROOT / "crates" / "ao2-cli" / "src"
TEST_DIR = ROOT / "crates" / "ao2-cli" / "tests"


def physical_lines(path: Path) -> int:
    return len(path.read_text(encoding="utf-8").splitlines())


def count_pattern(path: Path, pattern: str) -> int:
    return len(re.findall(pattern, path.read_text(encoding="utf-8"), re.MULTILINE))


def relative(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def rust_files(root: Path) -> list[Path]:
    return sorted(path for path in root.glob("*.rs") if path.is_file())


def cargo_metadata_edges() -> list[str]:
    metadata = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=True,
    )
    payload = json.loads(metadata.stdout)
    workspace_ids = set(payload.get("workspace_members", []))
    package_by_id = {package["id"]: package for package in payload.get("packages", [])}
    workspace_names = {
        package["name"]
        for package_id, package in package_by_id.items()
        if package_id in workspace_ids
    }
    edges: set[str] = set()
    for package_id in workspace_ids:
        package = package_by_id[package_id]
        source = package["name"]
        for dep in package.get("dependencies", []):
            target = dep["name"]
            if target in workspace_names:
                edges.add(f"{source}->{target}")
    return sorted(edges)


def unsafe_blocks() -> list[str]:
    hits: list[str] = []
    for path in sorted((ROOT / "crates").glob("**/*.rs")):
        text = path.read_text(encoding="utf-8")
        for lineno, line in enumerate(text.splitlines(), start=1):
            if re.search(r"\bunsafe\s*\{", line):
                hits.append(f"{relative(path)}:{lineno}:{line.strip()}")
    return hits


def module_declarations(files: list[Path]) -> list[str]:
    modules: list[str] = []
    for path in files:
        text = path.read_text(encoding="utf-8")
        for match in re.finditer(r"^(?:pub\s+)?mod\s+([A-Za-z0-9_]+)\s*;", text, re.MULTILINE):
            modules.append(f"{relative(path)}::{match.group(1)}")
    return sorted(modules)


def measure() -> dict[str, Any]:
    prod_files = rust_files(PROD_DIR)
    test_files = rust_files(TEST_DIR)
    main_rs = PROD_DIR / "main.rs"
    approval_replay = TEST_DIR / "cli_approval_replay.rs"
    prod_lines = {relative(path): physical_lines(path) for path in prod_files}
    test_lines = {relative(path): physical_lines(path) for path in test_files}
    test_counts = {
        relative(path): count_pattern(path, r"^\s*#\[(?:tokio::)?test\]")
        for path in test_files
    }
    total_prod = sum(prod_lines.values())
    total_tests = sum(test_lines.values())
    return {
        "source": {
            "production_total_lines": total_prod,
            "main_rs_lines": physical_lines(main_rs),
            "main_rs_source_concentration_percent": round(
                physical_lines(main_rs) / total_prod * 100, 4
            ),
            "main_rs_top_level_functions": count_pattern(
                main_rs, r"^(?:pub\s+)?(?:async\s+)?fn\s+[A-Za-z0-9_]+"
            ),
            "main_rs_top_level_types": count_pattern(
                main_rs, r"^(?:pub\s+)?(?:struct|enum|trait|type)\s+[A-Za-z0-9_]+"
            ),
            "file_lines": prod_lines,
            "module_declarations": module_declarations(prod_files),
        },
        "tests": {
            "integration_test_file_total_lines": total_tests,
            "integration_test_static_count": sum(test_counts.values()),
            "cli_approval_replay_rs_lines": physical_lines(approval_replay),
            "cli_approval_replay_tests": test_counts.get(relative(approval_replay), 0),
            "cli_approval_replay_line_concentration_percent": round(
                physical_lines(approval_replay) / total_tests * 100, 4
            ),
            "file_lines": test_lines,
            "file_tests": test_counts,
        },
        "unsafe_blocks": unsafe_blocks(),
        "workspace_dependency_edges": cargo_metadata_edges(),
    }


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def fail_if(condition: bool, failures: list[str], message: str) -> None:
    if condition:
        failures.append(message)


def compare_to_baseline(current: dict[str, Any], baseline: dict[str, Any]) -> list[str]:
    failures: list[str] = []
    budgets = baseline["budgets"]
    source = current["source"]
    tests = current["tests"]
    base_source = baseline["measurements"]["source"]
    base_tests = baseline["measurements"]["tests"]

    fail_if(
        source["production_total_lines"] > budgets["production_total_lines_soft_cap"],
        failures,
        "production source total exceeds decomposition soft cap",
    )
    fail_if(
        source["main_rs_lines"] > base_source["main_rs_lines"],
        failures,
        "crates/ao2-cli/src/main.rs grew above ratchet baseline",
    )
    fail_if(
        source["main_rs_top_level_functions"] > base_source["main_rs_top_level_functions"],
        failures,
        "main.rs top-level function count grew above ratchet baseline",
    )
    fail_if(
        source["main_rs_top_level_types"] > base_source["main_rs_top_level_types"],
        failures,
        "main.rs top-level type count grew above ratchet baseline",
    )
    fail_if(
        tests["cli_approval_replay_rs_lines"] > base_tests["cli_approval_replay_rs_lines"],
        failures,
        "cli_approval_replay.rs grew above ratchet baseline",
    )
    fail_if(
        len(current["unsafe_blocks"]) > len(baseline["measurements"]["unsafe_blocks"]),
        failures,
        "new Rust unsafe block detected",
    )

    base_edges = set(baseline["measurements"]["workspace_dependency_edges"])
    current_edges = set(current["workspace_dependency_edges"])
    new_edges = sorted(current_edges - base_edges)
    fail_if(bool(new_edges), failures, f"new workspace dependency edges detected: {new_edges}")

    base_prod_files = set(base_source["file_lines"])
    for path, line_count in source["file_lines"].items():
        if path not in base_prod_files:
            fail_if(
                line_count > budgets["new_production_file_lines_hard_ceiling"],
                failures,
                f"new production file exceeds line ceiling: {path}",
            )

    base_test_files = set(base_tests["file_lines"])
    for path, line_count in tests["file_lines"].items():
        test_count = tests["file_tests"].get(path, 0)
        if path not in base_test_files:
            fail_if(
                line_count > budgets["new_integration_test_file_lines_hard_ceiling"],
                failures,
                f"new integration-test file exceeds line ceiling: {path}",
            )
            fail_if(
                test_count > budgets["new_integration_test_file_tests_hard_ceiling"],
                failures,
                f"new integration-test file exceeds test-count ceiling: {path}",
            )
            if path.startswith("crates/ao2-cli/tests/common/"):
                fail_if(
                    line_count > budgets["shared_test_leaf_lines_hard_ceiling"],
                    failures,
                    f"shared test-support leaf exceeds line ceiling: {path}",
                )

    return failures


def base_baseline_from_git(ref: str) -> dict[str, Any] | None:
    result = subprocess.run(
        ["git", "show", f"{ref}:.github/architecture-baseline.json"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        return None
    return json.loads(result.stdout)


def check_baseline_not_raised(current: dict[str, Any], base: dict[str, Any]) -> list[str]:
    failures: list[str] = []
    ratchets = [
        ("measurements.source.main_rs_lines", ("measurements", "source", "main_rs_lines")),
        (
            "measurements.source.main_rs_top_level_functions",
            ("measurements", "source", "main_rs_top_level_functions"),
        ),
        (
            "measurements.source.main_rs_top_level_types",
            ("measurements", "source", "main_rs_top_level_types"),
        ),
        (
            "measurements.tests.cli_approval_replay_rs_lines",
            ("measurements", "tests", "cli_approval_replay_rs_lines"),
        ),
    ]
    for label, keys in ratchets:
        cur: Any = current
        old: Any = base
        for key in keys:
            cur = cur[key]
            old = old[key]
        if cur > old:
            failures.append(f"baseline ratchet increased: {label} {old} -> {cur}")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE)
    parser.add_argument("--write-baseline", action="store_true")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check-base-ref", default="origin/main")
    args = parser.parse_args()

    current = measure()
    if args.write_baseline:
        head = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip()
        tag_target = subprocess.check_output(
            ["git", "rev-parse", "v0.5.3^{commit}"], cwd=ROOT, text=True
        ).strip()
        payload = {
            "schema_version": "ao2.rust_architecture_baseline.v1",
            "baseline_source": {
                "head": head,
                "v0_5_3_tag_target": tag_target,
            },
            "measurements": current,
            "budgets": {
                "production_total_lines_soft_cap": 79000,
                "new_production_file_lines_hard_ceiling": 5000,
                "new_integration_test_file_lines_hard_ceiling": 4000,
                "new_integration_test_file_tests_hard_ceiling": 35,
                "shared_test_leaf_lines_hard_ceiling": 500,
            },
            "notes": [
                "Initial ratchet was measured at AO2 v0.5.3 post-hackathon start SHA.",
                "Existing oversize files are grandfathered debt and may not grow.",
            ],
        }
        target = args.output or args.baseline
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"wrote {target}")
        return 0

    baseline = load_json(args.baseline)
    failures = compare_to_baseline(current, baseline)
    base_baseline = base_baseline_from_git(args.check_base_ref)
    if base_baseline is not None:
        failures.extend(check_baseline_not_raised(baseline, base_baseline))

    if failures:
        print("AO2 Rust architecture guard failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print("AO2 Rust architecture guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
