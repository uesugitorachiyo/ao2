#!/usr/bin/env python3
"""Authorize bounded AO2 Windows control tasks with the pinned release key."""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import ao2_windows_outbound_worker as worker


MAX_INPUT_BYTES = 1024 * 1024


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--private-key", type=Path, required=True)
    parser.add_argument("--public-key", type=Path, required=True)
    parser.add_argument("--ttl-seconds", type=int, default=300)
    parser.add_argument("--issued-at")
    return parser.parse_args(argv)


def parse_issued_at(value: str | None) -> datetime | None:
    if value is None:
        return None
    parsed = worker.parse_utc_timestamp(value)
    if parsed is None:
        raise SystemExit("--issued-at must be an RFC3339 UTC timestamp ending in Z")
    return parsed


def read_board(path: Path) -> dict[str, Any]:
    if path.stat().st_size > MAX_INPUT_BYTES:
        raise SystemExit(f"task board exceeds {MAX_INPUT_BYTES} bytes")
    try:
        board = json.loads(
            path.read_text(encoding="utf-8"),
            parse_constant=lambda value: (_ for _ in ()).throw(
                ValueError(f"non-standard JSON constant: {value}")
            ),
        )
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise SystemExit(f"cannot parse task board: {type(exc).__name__}") from exc
    if not isinstance(board, dict) or board.get("schema_version") != worker.TASK_BOARD_SCHEMA:
        raise SystemExit(f"task board must use {worker.TASK_BOARD_SCHEMA}")
    tasks = board.get("tasks")
    if not isinstance(tasks, list) or not 1 <= len(tasks) <= 32:
        raise SystemExit("task board must contain between 1 and 32 tasks")
    return board


def authorize_board(
    board: dict[str, Any],
    *,
    private_key: Path,
    public_key: Path,
    ttl_seconds: int,
    issued_at: datetime | None,
) -> tuple[dict[str, Any], int]:
    authorized = json.loads(json.dumps(board))
    count = 0
    for index, task in enumerate(authorized["tasks"]):
        if not isinstance(task, dict):
            raise SystemExit(f"task {index} must be an object")
        cross_host = task.get("ao2_cross_host")
        if not isinstance(cross_host, dict) or cross_host.get("schema_version") != worker.CONTROL_TASK_SCHEMA:
            continue
        action = str(cross_host.get("action") or "")
        if action in worker.UNSIGNED_OBSERVER_ACTIONS:
            continue
        if action not in worker.ALLOWLISTED_ACTIONS or cross_host.get("arbitrary_command_execution") is not False:
            raise SystemExit(f"task {index} is not a bounded allowlisted control action")
        authorized["tasks"][index] = worker.authorize_control_task(
            task,
            private_key_path=private_key,
            public_key_path=public_key,
            ttl_seconds=ttl_seconds,
            issued_at=issued_at,
        )
        count += 1
    if count == 0:
        raise SystemExit("task board contains no command-executing control task to authorize")
    return authorized, count


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    board = read_board(args.input)
    authorized, count = authorize_board(
        board,
        private_key=args.private_key,
        public_key=args.public_key,
        ttl_seconds=args.ttl_seconds,
        issued_at=parse_issued_at(args.issued_at),
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    output = worker.canonical_json_bytes(authorized) + b"\n"
    args.output.write_bytes(output)
    print(f"authorized_tasks={count}")
    print(f"output_sha256={hashlib.sha256(output).hexdigest()}")
    print(f"completed_at={datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
