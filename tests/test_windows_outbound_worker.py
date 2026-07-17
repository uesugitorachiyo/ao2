from __future__ import annotations

import importlib.util
import json
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKER_PATH = ROOT / "scripts" / "ao2_windows_outbound_worker.py"


def load_worker_module():
    spec = importlib.util.spec_from_file_location("ao2_windows_outbound_worker", WORKER_PATH)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def test_bounded_child_timeout_kills_process_tree(tmp_path: Path) -> None:
    worker = load_worker_module()
    marker = tmp_path / "grandchild-survived.txt"
    script = tmp_path / "spawn_grandchild.py"
    script.write_text(
        "\n".join(
                [
                    "import subprocess, sys, time",
                    "subprocess.Popen([sys.executable, '-c', %r])"
                    % (
                        "import pathlib,time; time.sleep(1.2); "
                        f"pathlib.Path({str(marker)!r}).write_text('survived')"
                    ),
                    "time.sleep(5)",
                ]
            ),
        encoding="utf-8",
    )

    result = worker.run_bounded_child(
        [sys.executable, str(script)],
        cwd=tmp_path,
        timeout_seconds=0.2,
        output_limit_bytes=4096,
    )
    time.sleep(1.5)

    assert result["status"] == "timed_out"
    assert result["timed_out"] is True
    assert result["exit_code"] is None
    assert result["sanitized_stderr_category"] == "timeout"
    assert marker.exists() is False


def test_child_output_is_redacted_and_size_bounded(tmp_path: Path) -> None:
    worker = load_worker_module()
    script = tmp_path / "leaky.py"
    script.write_text(
        "import os\n"
        "print('Authorization: Bearer abc.def.ghi')\n"
        "print('AO2_CP_API_TOKEN=super-secret-token')\n"
        "print('x' * 2000)\n",
        encoding="utf-8",
    )

    result = worker.run_bounded_child(
        [sys.executable, str(script)],
        cwd=tmp_path,
        timeout_seconds=5,
        output_limit_bytes=240,
    )

    output = result["output"]
    assert result["status"] == "accepted"
    assert "abc.def.ghi" not in output
    assert "super-secret-token" not in output
    assert "<redacted>" in output
    assert result["output_truncated"] is True
    assert len(output.encode("utf-8")) <= 240


def test_worker_keeps_status_responsive_while_slow_action_runs(tmp_path: Path) -> None:
    worker = load_worker_module()
    transport = worker.MemoryTransport()
    state = worker.WorkerState(tmp_path / "state")
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=state,
        transport=transport,
        poll_interval_seconds=0.01,
    )

    slow = worker.control_task(
        request_id="slow-1",
        action="timeout_fixture",
        parameters={"sleep_seconds": 0.6, "timeout_seconds": 2},
    )
    status = worker.control_task(request_id="status-while-slow", action="status")

    assert runtime.accept_control_task(slow) == "started"
    assert runtime.accept_control_task(status) == "completed"

    posted = transport.posted_results_by_request_id()
    assert posted["status-while-slow"]["ao2_cross_host"]["action"] == "status"
    assert runtime.running_action_count() == 1


def test_timeout_result_is_sanitized_and_failed_task_does_not_stop_polling(tmp_path: Path) -> None:
    worker = load_worker_module()
    transport = worker.MemoryTransport()
    state = worker.WorkerState(tmp_path / "state")
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=state,
        transport=transport,
        poll_interval_seconds=0.01,
    )

    timed = worker.control_task(
        request_id="timeout-1",
        action="timeout_fixture",
        parameters={"sleep_seconds": 5, "timeout_seconds": 0.2},
    )
    assert runtime.accept_control_task(timed) == "started"
    runtime.wait_for_idle(timeout_seconds=3)
    assert runtime.is_stopped() is False

    status = worker.control_task(request_id="status-after-timeout", action="status")
    assert runtime.accept_control_task(status) == "completed"
    posted = transport.posted_results_by_request_id()
    timeout_result = posted["timeout-1"]["ao2_cross_host"]["result"]
    assert timeout_result["status"] == "timed_out"
    assert timeout_result["sanitized_stderr_category"] == "timeout"
    assert posted["status-after-timeout"]["ao2_cross_host"]["action"] == "status"


def test_duplicate_and_completed_tasks_are_not_executed_twice(tmp_path: Path) -> None:
    worker = load_worker_module()
    transport = worker.MemoryTransport()
    state = worker.WorkerState(tmp_path / "state")
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=state,
        transport=transport,
        poll_interval_seconds=0.01,
    )
    task = worker.control_task(request_id="dup-1", action="status")

    assert runtime.accept_control_task(task) == "completed"
    assert runtime.accept_control_task(task) == "duplicate"

    restarted = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=worker.WorkerState(tmp_path / "state"),
        transport=transport,
        poll_interval_seconds=0.01,
    )
    assert restarted.accept_control_task(task) == "duplicate"
    assert len(transport.posted_results_by_request_id()) == 1


def test_allowlist_rejects_arbitrary_command_text(tmp_path: Path) -> None:
    worker = load_worker_module()
    transport = worker.MemoryTransport()
    state = worker.WorkerState(tmp_path / "state")
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=state,
        transport=transport,
        poll_interval_seconds=0.01,
    )

    denied = worker.control_task(
        request_id="deny-1",
        action="run",
        parameters={"command": "whoami"},
        arbitrary_command_execution=True,
    )

    assert runtime.accept_control_task(denied) == "rejected"
    result = transport.posted_results_by_request_id()["deny-1"]["ao2_cross_host"]["result"]
    assert result["status"] == "failed"
    assert result["error_category"] == "action_not_allowlisted"
    assert "whoami" not in json.dumps(result)


def test_poll_once_ignores_worker_result_and_non_target_control_tasks(tmp_path: Path) -> None:
    worker = load_worker_module()

    class BoardTransport(worker.MemoryTransport):
        def __init__(self, boards):
            super().__init__()
            self.boards = list(boards)

        def latest_board(self):
            return self.boards.pop(0) if self.boards else None

    result_board = {
        "schema_version": "ao2.ai-task-board.v1",
        "tasks": [{
            "kind": "cross-host-worker-result",
            "ao2_cross_host": {
                "schema_version": "ao2.cross-host.windows-worker-result.v1",
                "request_id": "old-result",
                "action": "status",
                "node_id": "windows-hp255_g10",
            },
        }],
    }
    wrong_target = {
        "schema_version": "ao2.ai-task-board.v1",
        "tasks": [worker.control_task(request_id="wrong-node", action="status", target_node="other-node")],
    }
    good = {
        "schema_version": "ao2.ai-task-board.v1",
        "tasks": [worker.control_task(request_id="right-node", action="status")],
    }

    transport = BoardTransport([result_board, wrong_target, good])
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=worker.WorkerState(tmp_path / "state"),
        transport=transport,
        poll_interval_seconds=0.01,
    )

    assert runtime.poll_once() == "no_control_task"
    assert runtime.poll_once() == "no_control_task"
    assert runtime.poll_once() == "completed"
    assert "right-node" in transport.posted_results_by_request_id()
