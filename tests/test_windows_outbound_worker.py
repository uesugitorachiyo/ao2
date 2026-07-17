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


def test_windows_stack_qualification_action_is_allowlisted_and_uses_fixed_profile(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    repo = factory / "ao2"
    (repo / ".git").mkdir(parents=True)
    recorded_commands = []

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES):
        recorded_commands.append((list(command), Path(cwd), timeout_seconds, output_limit_bytes))
        return {
            "status": "accepted",
            "exit_code": 0,
            "timed_out": False,
            "duration_seconds": 0.01,
            "output": "ok",
            "output_truncated": False,
            "sanitized_stderr_category": "none",
            "command_name": Path(command[0]).name,
        }

    monkeypatch.setattr(worker, "run_bounded_child", fake_run)
    transport = worker.MemoryTransport()
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=transport,
        poll_interval_seconds=0.01,
    )

    task = worker.control_task(
        request_id="stack-qual-1",
        action="windows_stack_qualification",
        parameters={"mode": "diagnostic", "repositories": ["ao2"]},
    )

    assert runtime.accept_control_task(task) == "started"
    assert runtime.wait_for_idle(timeout_seconds=2) is True
    result = transport.posted_results_by_request_id()["stack-qual-1"]["ao2_cross_host"]["result"]
    assert result["status"] == "accepted"
    assert result["mode"] == "diagnostic"
    assert result["results"][0]["canonical_repository"] == "ao2"
    assert result["results"][0]["request_id"] == "stack-qual-1"
    assert result["results"][0]["worker_source_commit"]
    assert recorded_commands
    assert all("payload" not in " ".join(command) for command, _, _, _ in recorded_commands)
    status = runtime.status_result()
    assert "windows_stack_qualification" in status["allowed_actions"]
    assert status["worker_source_commit"]
    assert status["stack_qualification_profile_version"] == worker.STACK_PROFILE_VERSION


def test_windows_stack_qualification_inventory_matches_worker_contract() -> None:
    worker = load_worker_module()
    inventory = json.loads((ROOT / "docs" / "windows-stack-qualification-inventory.json").read_text(encoding="utf-8"))

    assert inventory["action"] == "windows_stack_qualification"
    assert inventory["profile_version"] == worker.STACK_PROFILE_VERSION
    assert inventory["canonical_repositories"] == list(worker.CANONICAL_REPOSITORIES)
    assert inventory["archived_repositories_rejected"] == list(worker.ARCHIVED_REPOSITORIES)
    assert inventory["modes"] == list(worker.STACK_QUALIFICATION_MODES)
    assert set(inventory["allowed_parameters"]) == worker.STACK_QUALIFICATION_ALLOWED_PARAMETERS
    assert inventory["timeout_bounds_seconds"]["minimum"] == worker.MIN_STACK_QUALIFICATION_TIMEOUT_SECONDS
    assert inventory["timeout_bounds_seconds"]["default"] == worker.DEFAULT_STACK_QUALIFICATION_TIMEOUT_SECONDS
    assert inventory["timeout_bounds_seconds"]["maximum"] == worker.MAX_STACK_QUALIFICATION_TIMEOUT_SECONDS


def test_fixed_tool_resolver_uses_standard_go_location_when_path_lacks_go(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()
    go_exe = tmp_path / "ProgramFiles" / "Go" / "bin" / "go.exe"
    go_exe.parent.mkdir(parents=True)
    go_exe.write_text("placeholder", encoding="utf-8")

    monkeypatch.setenv("PATH", "")
    monkeypatch.setenv("ProgramFiles", str(tmp_path / "ProgramFiles"))

    resolved = worker.resolve_fixed_tool("go")

    assert resolved["status"] == "resolved"
    assert resolved["tool"] == "go"
    assert resolved["resolution_source"] == "standard_location"
    assert Path(resolved["path"]) == go_exe


def test_fixed_profile_commands_resolve_standard_go_without_payload(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()
    go_exe = tmp_path / "ProgramFiles" / "Go" / "bin" / "go.exe"
    go_exe.parent.mkdir(parents=True)
    go_exe.write_text("placeholder", encoding="utf-8")

    monkeypatch.setenv("PATH", "")
    monkeypatch.setenv("ProgramFiles", str(tmp_path / "ProgramFiles"))

    command = worker.resolve_profile_command(("go", "test", "./..."))

    assert command == [str(go_exe), "test", "./..."]


def test_windows_stack_qualification_toolchain_mode_reports_fixed_tools(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()
    recorded_commands = []

    def fake_resolve(tool_name):
        return {
            "tool": tool_name,
            "status": "resolved",
            "path": f"C:/fixed-tools/{tool_name}.exe",
            "resolution_source": "test",
        }

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES):
        recorded_commands.append(list(command))
        return {
            "status": "accepted",
            "exit_code": 0,
            "timed_out": False,
            "duration_seconds": 0.01,
            "output": f"{Path(command[0]).name} version 1.0",
            "output_truncated": False,
            "sanitized_stderr_category": "none",
            "command_name": Path(command[0]).name,
        }

    monkeypatch.setattr(worker, "resolve_fixed_tool", fake_resolve)
    monkeypatch.setattr(worker, "run_bounded_child", fake_run)
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path / "factory",
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    result = runtime.run_action(
        "windows_stack_qualification",
        {"mode": "toolchain", "timeout_seconds": 30},
        request_id="toolchain",
    )

    tools = {item["tool"]: item for item in result["toolchain_capabilities"]}
    assert result["status"] == "accepted"
    assert result["mode"] == "toolchain"
    assert result["schema_version"] == "ao2.windows-toolchain-capability-result.v1"
    assert "go" in tools
    assert "git" in tools
    assert "python" in tools
    assert tools["go"]["resolution_source"] == "test"
    assert "version" in tools["go"]["bounded_sanitized_output"]
    assert all("payload" not in " ".join(command) for command in recorded_commands)


def test_windows_stack_qualification_rejects_noncanonical_and_unsafe_repositories(tmp_path: Path) -> None:
    worker = load_worker_module()
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path / "factory",
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    for repo_name in ["unknown", "agy-swarms", "..", "../ao2", "ao2/control", r"C:\ao\factory\ao2", "/tmp/ao2"]:
        result = runtime.run_action(
            "windows_stack_qualification",
            {"mode": "diagnostic", "repositories": [repo_name]},
            request_id="invalid-repo",
        )
        assert result["status"] == "failed"
        assert result["error_category"] in {"unknown_repository", "archived_repository", "invalid_repository_name"}


def test_windows_stack_qualification_rejects_duplicate_repositories_and_bad_timeout(tmp_path: Path) -> None:
    worker = load_worker_module()
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path / "factory",
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    duplicate = runtime.run_action(
        "windows_stack_qualification",
        {"mode": "diagnostic", "repositories": ["ao2", "ao2"]},
        request_id="duplicate",
    )
    too_short = runtime.run_action(
        "windows_stack_qualification",
        {"mode": "diagnostic", "repositories": ["ao2"], "timeout_seconds": 1},
        request_id="too-short",
    )
    too_long = runtime.run_action(
        "windows_stack_qualification",
        {"mode": "diagnostic", "repositories": ["ao2"], "timeout_seconds": 999999},
        request_id="too-long",
    )

    assert duplicate["status"] == "failed"
    assert duplicate["error_category"] == "duplicate_repository"
    assert too_short["status"] == "failed"
    assert too_short["error_category"] == "timeout_out_of_bounds"
    assert too_long["status"] == "failed"
    assert too_long["error_category"] == "timeout_out_of_bounds"


def test_windows_stack_qualification_rejects_payload_command_overrides(tmp_path: Path) -> None:
    worker = load_worker_module()
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path / "factory",
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    for forbidden_key in ["command", "commands", "cwd", "working_directory", "executable", "env", "powershell"]:
        result = runtime.run_action(
            "windows_stack_qualification",
            {"mode": "diagnostic", "repositories": ["ao2"], forbidden_key: "payload controlled"},
            request_id=f"override-{forbidden_key}",
        )
        assert result["status"] == "failed"
        assert result["error_category"] == "unsupported_parameter"
        assert "payload controlled" not in json.dumps(result)


def test_windows_stack_qualification_bounds_and_redacts_repository_output(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    repo = factory / "ao2"
    (repo / ".git").mkdir(parents=True)

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES):
        return {
            "status": "failed",
            "exit_code": 2,
            "timed_out": False,
            "duration_seconds": 0.01,
            "output": "Authorization: Bearer secret.token.value\n" + ("x" * 2000),
            "output_truncated": False,
            "sanitized_stderr_category": "nonzero_exit",
            "command_name": "fixed-profile-command",
        }

    monkeypatch.setattr(worker, "run_bounded_child", fake_run)
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
        output_limit_bytes=240,
    )

    result = runtime.run_action(
        "windows_stack_qualification",
        {"mode": "diagnostic", "repositories": ["ao2"]},
        request_id="redaction",
    )

    output = result["results"][0]["bounded_sanitized_output"]
    assert result["status"] == "failed"
    assert "secret.token.value" not in output
    assert "<redacted>" in output
    assert len(output.encode("utf-8")) <= 240


def test_windows_stack_qualification_missing_executable_is_command_failure(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    repo = factory / "ao2"
    (repo / ".git").mkdir(parents=True)

    monkeypatch.setattr(
        worker,
        "DIAGNOSTIC_PROFILE",
        ({"name": "missing-fixed-tool", "argv": ("ao2-definitely-missing-tool", "--version")},),
    )

    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    result = runtime.run_action(
        "windows_stack_qualification",
        {"mode": "diagnostic", "repositories": ["ao2"]},
        request_id="missing-tool",
    )

    assert result["status"] == "failed"
    assert result["results"][0]["canonical_repository"] == "ao2"
    assert result["results"][0]["sanitized_command_name"] == "missing-fixed-tool"
    assert result["results"][0]["status"] == "failed"
    assert result["results"][0]["error_category"] == "missing_dependency"
    assert "ao2-definitely-missing-tool" in result["results"][0]["bounded_sanitized_output"]


def test_windows_stack_qualification_timeout_does_not_block_status(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    repo = factory / "ao2"
    (repo / ".git").mkdir(parents=True)

    def slow_timeout(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES):
        time.sleep(0.25)
        return {
            "status": "timed_out",
            "exit_code": None,
            "timed_out": True,
            "duration_seconds": 0.2,
            "output": "",
            "output_truncated": False,
            "sanitized_stderr_category": "timeout",
            "command_name": "fixed-profile-command",
        }

    monkeypatch.setattr(worker, "run_bounded_child", slow_timeout)
    transport = worker.MemoryTransport()
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=transport,
        poll_interval_seconds=0.01,
    )

    started = runtime.accept_control_task(
        worker.control_task(
            request_id="stack-timeout",
            action="windows_stack_qualification",
            parameters={"mode": "diagnostic", "repositories": ["ao2"]},
        )
    )
    status = runtime.accept_control_task(worker.control_task(request_id="status-after-stack-timeout", action="status"))

    assert started == "started"
    assert status == "completed"
    assert runtime.wait_for_idle(timeout_seconds=2) is True
    posted = transport.posted_results_by_request_id()
    assert posted["status-after-stack-timeout"]["ao2_cross_host"]["action"] == "status"
    assert posted["stack-timeout"]["ao2_cross_host"]["result"]["results"][0]["status"] == "timed_out"
