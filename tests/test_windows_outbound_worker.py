from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKER_PATH = ROOT / "scripts" / "ao2_windows_outbound_worker.py"
AUTHORIZER_PATH = ROOT / "scripts" / "authorize_windows_control_task.py"
PHYSICAL_LIFECYCLE_PROBE_PATH = ROOT / "scripts" / "Test-AO2PhysicalWindowsLifecycle.ps1"


def load_worker_module():
    spec = importlib.util.spec_from_file_location("ao2_windows_outbound_worker", WORKER_PATH)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def allow_test_execution(*_args, **_kwargs):
    return True, "test_fixture_authorized"


def generate_rsa_keypair(tmp_path: Path) -> tuple[Path, Path]:
    tmp_path.mkdir(parents=True, exist_ok=True)
    private_key = tmp_path / "task-authorization-private.pem"
    public_key = tmp_path / "task-authorization-public.pem"
    openssl = load_worker_module().resolve_openssl()
    assert openssl is not None
    subprocess.run(
        [openssl, "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048", "-out", private_key],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    subprocess.run(
        [openssl, "pkey", "-in", private_key, "-pubout", "-out", public_key],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return private_key, public_key


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


def test_bounded_child_timeout_collection_timeout_is_structured(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()
    killed: list[int] = []

    class StubbornProcess:
        pid = 1234
        returncode = None

        def poll(self):
            return None

        def communicate(self, timeout=None):
            raise worker.subprocess.TimeoutExpired(
                ["cargo", "test", "--workspace"],
                timeout,
                output="partial stdout",
                stderr="partial stderr",
            )

    process = StubbornProcess()
    monkeypatch.setattr(worker.subprocess, "Popen", lambda command, **kwargs: process)
    monkeypatch.setattr(worker, "terminate_process_tree", lambda child: killed.append(child.pid))

    result = worker.run_bounded_child(
        ["cargo", "test", "--workspace"],
        cwd=tmp_path,
        timeout_seconds=0.01,
        output_limit_bytes=4096,
    )

    assert killed == [1234]
    assert result["status"] == "timed_out"
    assert result["timed_out"] is True
    assert result["exit_code"] is None
    assert result["sanitized_stderr_category"] == "timeout"
    assert result["command_name"] == "cargo"
    assert "partial stdout" in result["output"]


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


def test_successful_child_with_stderr_reports_no_error_category(tmp_path: Path) -> None:
    worker = load_worker_module()
    script = tmp_path / "progress_on_stderr.py"
    script.write_text(
        "import sys\n"
        "print('normal output')\n"
        "print('Finished dev profile', file=sys.stderr)\n",
        encoding="utf-8",
    )

    result = worker.run_bounded_child(
        [sys.executable, str(script)],
        cwd=tmp_path,
        timeout_seconds=5,
        output_limit_bytes=4096,
    )

    assert result["status"] == "accepted"
    assert result["exit_code"] == 0
    assert result["sanitized_stderr_category"] == "none"
    assert "Finished dev profile" in result["output"]


def test_repository_worktree_status_is_bounded_and_redacted(tmp_path: Path) -> None:
    worker = load_worker_module()
    repo = tmp_path / "ao2"
    repo.mkdir()
    subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
    subprocess.run(["git", "config", "user.name", "AO2 Test"], cwd=repo, check=True)
    subprocess.run(["git", "config", "user.email", "ao2@example.invalid"], cwd=repo, check=True)
    tracked = repo / "tracked.txt"
    tracked.write_text("original\n", encoding="utf-8")
    subprocess.run(["git", "add", "tracked.txt"], cwd=repo, check=True)
    subprocess.run(["git", "commit", "-qm", "fixture"], cwd=repo, check=True)
    tracked.write_text("changed\n", encoding="utf-8")
    (repo / "AO2_CP_API_TOKEN=do-not-expose.txt").write_text("fixture\n", encoding="utf-8")

    result = worker.repository_worktree_status(repo, entry_limit=32)

    assert result["status"] == "attention"
    assert result["clean"] is False
    assert result["entry_count"] == 2
    assert result["entries_truncated"] is False
    assert len(result["entries"]) == 2
    assert any("tracked.txt" in entry for entry in result["entries"])
    assert "do-not-expose" not in json.dumps(result)
    assert "<redacted>" in json.dumps(result)


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
        execution_authorization_verifier=allow_test_execution,
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
    status_result = posted["status-while-slow"]["ao2_cross_host"]["result"]
    assert status_result["ao2_repository_worktree"]["status"] == "unavailable"
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
        execution_authorization_verifier=allow_test_execution,
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


def test_completed_action_result_is_retried_from_durable_outbox_after_post_failure(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()

    class FlakyTransport(worker.MemoryTransport):
        def __init__(self) -> None:
            super().__init__()
            self.failures_remaining = 1

        def post_board(self, board):
            if self.failures_remaining:
                self.failures_remaining -= 1
                raise OSError("network unreachable")
            super().post_board(board)

    transport = FlakyTransport()
    state = worker.WorkerState(tmp_path / "state")
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=state,
        transport=transport,
        poll_interval_seconds=0.01,
        execution_authorization_verifier=allow_test_execution,
    )
    monkeypatch.setattr(
        runtime,
        "run_action",
        lambda action, parameters, request_id="": {
            "status": "accepted",
            "request_id": request_id,
            "marker": "original-result",
        },
    )

    assert runtime.accept_control_task(worker.control_task(request_id="deliver-later", action="ao2_doctor")) == "started"
    assert runtime.wait_for_idle(timeout_seconds=2) is True
    assert "deliver-later" not in transport.posted_results_by_request_id()

    assert runtime.poll_once() == "no_board"

    posted = transport.posted_results_by_request_id()
    result = posted["deliver-later"]["ao2_cross_host"]["result"]
    assert result["status"] == "accepted"
    assert result["marker"] == "original-result"
    assert state._ledger["tasks"]["deliver-later"]["status"] == "accepted"


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


def test_scaled_worker_ledger_recovers_duplicate_protection_after_primary_json_corruption(
    tmp_path: Path, monkeypatch
) -> None:
    worker = load_worker_module()
    state_root = tmp_path / "state"
    state = worker.WorkerState(state_root)

    for index in range(1200):
        request_id = f"scaled-ledger-{index:04d}"
        assert state.claim(request_id, "status") is True
        state.complete(request_id, "accepted")

    state.ledger_path.write_text('{"schema_version": "truncated", "tasks": {', encoding="utf-8")

    transport = worker.MemoryTransport()
    executions: list[str] = []
    restarted = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=worker.WorkerState(state_root),
        transport=transport,
        poll_interval_seconds=0.01,
        execution_authorization_verifier=allow_test_execution,
    )
    monkeypatch.setattr(
        restarted,
        "run_action",
        lambda action, parameters, request_id="": executions.append(request_id)
        or {"status": "accepted", "request_id": request_id},
    )

    task = worker.control_task(request_id="scaled-ledger-1199", action="ao2_doctor")

    assert restarted.accept_control_task(task) == "duplicate"
    assert executions == []
    assert "scaled-ledger-1199" not in transport.posted_results_by_request_id()


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


def test_ao2_doctor_uses_factory_cargo_fallback_when_binary_is_not_installed(
    tmp_path: Path, monkeypatch
) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    ao2_repo = factory / "ao2"
    manifest = ao2_repo / "Cargo.toml"
    manifest.parent.mkdir(parents=True)
    manifest.write_text("[workspace]\n", encoding="utf-8")
    cargo = tmp_path / "tools" / "cargo.exe"
    cargo.parent.mkdir(parents=True)
    cargo.write_text("placeholder", encoding="utf-8")
    recorded_commands = []

    monkeypatch.setenv("PATH", "")
    monkeypatch.setenv("LOCALAPPDATA", str(tmp_path / "local-app-data"))

    def fake_resolve(tool_name):
        assert tool_name == "cargo"
        return {
            "tool": "cargo",
            "status": "resolved",
            "path": str(cargo),
            "resolution_source": "test",
        }

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES):
        recorded_commands.append((list(command), Path(cwd), timeout_seconds, output_limit_bytes))
        return {
            "status": "accepted",
            "exit_code": 0,
            "timed_out": False,
            "duration_seconds": 0.01,
            "output": '{"status":"ok"}',
            "output_truncated": False,
            "sanitized_stderr_category": "none",
            "command_name": Path(command[0]).name,
        }

    monkeypatch.setattr(worker, "resolve_fixed_tool", fake_resolve)
    monkeypatch.setattr(worker, "run_bounded_child", fake_run)
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    result = runtime.run_action("ao2_doctor", {"timeout_seconds": 300}, request_id="doctor")

    assert result["status"] == "accepted"
    doctor_target_dir = factory / ".ao2-worker-target" / "ao2-doctor"
    assert recorded_commands == [
        (
            [
                str(cargo),
                "run",
                "--manifest-path",
                str(manifest),
                "--target-dir",
                str(doctor_target_dir),
                "-p",
                "ao2-cli",
                "--bin",
                "ao2",
                "--",
                "doctor",
                "--json",
            ],
            factory,
            300,
            worker.DEFAULT_OUTPUT_LIMIT_BYTES,
        )
    ]


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
        execution_authorization_verifier=allow_test_execution,
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


def test_ao2_full_profile_cargo_test_uses_isolated_target_dir(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    repo = factory / "ao2"
    (repo / ".git").mkdir(parents=True)
    recorded_commands = []

    def fake_resolve_fixed_tool(tool_name: str):
        paths = {
            "cargo": "cargo.exe",
            "npm": "npm.cmd",
            "powershell": "powershell.exe",
        }
        return {"tool": tool_name, "status": "resolved", "path": paths.get(tool_name, tool_name)}

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES, **_kwargs):
        recorded_commands.append(list(command))
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

    monkeypatch.setattr(worker, "resolve_fixed_tool", fake_resolve_fixed_tool)
    monkeypatch.setattr(worker, "run_bounded_child", fake_run)
    transport = worker.MemoryTransport()
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=transport,
        poll_interval_seconds=0.01,
        execution_authorization_verifier=allow_test_execution,
    )

    task = worker.control_task(
        request_id="ao2-full",
        action="windows_stack_qualification",
        parameters={"mode": "full", "repositories": ["ao2"]},
    )

    assert runtime.accept_control_task(task) == "started"
    assert runtime.wait_for_idle(timeout_seconds=2) is True
    cargo_test = next(
        command
        for command in recorded_commands
        if Path(command[0]).name.lower() in {"cargo", "cargo.exe"}
        and command[1:4] == ["test", "--workspace", "--exclude"]
    )
    assert "--target-dir" in cargo_test
    assert cargo_test[cargo_test.index("--target-dir") + 1] == str(factory / ".ao2-worker-target" / "ao2-full")
    assert not any(
        Path(command[0]).name.lower() in {"cargo", "cargo.exe"}
        and command[1:3] == ["test", "--workspace"]
        and "--exclude" not in command
        for command in recorded_commands
    )


def test_ao2_full_profile_npm_verify_inherits_isolated_cargo_target(tmp_path: Path, monkeypatch) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    repo = factory / "ao2"
    (repo / ".git").mkdir(parents=True)
    recorded_envs = []

    def fake_resolve_fixed_tool(tool_name: str):
        paths = {
            "cargo": "cargo.exe",
            "npm": "npm.cmd",
            "powershell": "powershell.exe",
        }
        return {"tool": tool_name, "status": "resolved", "path": paths.get(tool_name, tool_name)}

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES, env=None):
        if Path(command[0]).name.lower() in {"npm", "npm.cmd"} and command[1:] == ["run", "test:archive-resources"]:
            recorded_envs.append(dict(env or {}))
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

    monkeypatch.setattr(worker, "resolve_fixed_tool", fake_resolve_fixed_tool)
    monkeypatch.setattr(worker, "run_bounded_child", fake_run)
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
        execution_authorization_verifier=allow_test_execution,
    )

    task = worker.control_task(
        request_id="ao2-full",
        action="windows_stack_qualification",
        parameters={"mode": "full", "repositories": ["ao2"]},
    )

    assert runtime.accept_control_task(task) == "started"
    assert runtime.wait_for_idle(timeout_seconds=2) is True
    assert recorded_envs
    assert recorded_envs == [{"CARGO_TARGET_DIR": str(factory / ".ao2-worker-target" / "ao2-full")}]


def test_ao2_full_profile_uses_windows_ci_partitions_instead_of_monolithic_workspace(
    tmp_path: Path, monkeypatch
) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    repo = factory / "ao2"
    (repo / ".git").mkdir(parents=True)
    recorded_commands = []

    def fake_resolve_fixed_tool(tool_name: str):
        paths = {
            "cargo": "cargo.exe",
            "npm": "npm.cmd",
            "powershell": "powershell.exe",
        }
        return {"tool": tool_name, "status": "resolved", "path": paths.get(tool_name, tool_name)}

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES, **_kwargs):
        recorded_commands.append(list(command))
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

    monkeypatch.setattr(worker, "resolve_fixed_tool", fake_resolve_fixed_tool)
    monkeypatch.setattr(worker, "run_bounded_child", fake_run)
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
        execution_authorization_verifier=allow_test_execution,
    )

    task = worker.control_task(
        request_id="ao2-full-partitions",
        action="windows_stack_qualification",
        parameters={"mode": "full", "repositories": ["ao2"]},
    )

    assert runtime.accept_control_task(task) == "started"
    assert runtime.wait_for_idle(timeout_seconds=2) is True
    joined = [" ".join(command) for command in recorded_commands]
    assert any("cargo.exe test --workspace --exclude ao2-cli" in command for command in joined)
    assert any("cargo.exe test -p ao2-cli" in command and "cli_adapter" in command for command in joined)
    assert any("cargo.exe test -p ao2-cli" in command and "cli_plugin_pulse" in command for command in joined)
    assert any("cargo.exe test -p ao2-cli" in command and "cli_plugin_package" in command for command in joined)
    assert any(
        "cargo.exe test -p ao2-cli" in command and "cli_plugin_consumer_lifecycle" in command
        for command in joined
    )
    assert any(
        "cargo.exe test -p ao2-cli" in command and "cli_plugin_release_candidate" in command
        for command in joined
    )
    assert any(
        "cargo.exe test -p ao2-cli" in command and "cli_plugin_distribution" in command
        for command in joined
    )
    assert any(
        "cargo.exe test -p ao2-cli" in command and "cli_plugin_adapter" in command
        for command in joined
    )
    assert any(
        "cargo.exe test -p ao2-cli" in command and "cli_plugin_wrapper_harness" in command
        for command in joined
    )
    assert any("cargo.exe test -p ao2-cli" in command and "cli_factory_plan" in command for command in joined)
    assert not any("cli_approval_replay cli_factory_plan" in command for command in joined)
    assert any("cargo.exe test -p ao2-cli" in command and "cli_factory_queue_core" in command for command in joined)
    assert any("cargo.exe test -p ao2-cli" in command and "cli_approval_replay cli_factory_queue" in command for command in joined)
    assert any("cargo.exe test -p ao2-cli" in command and "cli_factory_pack" in command for command in joined)
    assert not any("cli_approval_replay cli_factory_pack" in command for command in joined)
    assert any("cargo.exe test -p ao2-cli" in command and "cli_factory_verify" in command for command in joined)
    assert not any("cli_approval_replay cli_factory_verify" in command for command in joined)
    assert any("cargo.exe test -p ao2-cli" in command and "cli_factory_run" in command for command in joined)
    assert not any("cli_approval_replay cli_factory_run" in command for command in joined)
    assert not any("cli_approval_replay cli_factory_app" in command for command in joined)
    assert any(
        "cargo.exe test -p ao2-cli" in command and "cli_factory_evaluator_closer" in command
        for command in joined
    )
    assert not any("cli_approval_replay cli_factory_evaluator" in command for command in joined)
    assert not any("cli_approval_replay cli_factory_closer" in command for command in joined)
    assert any(
        "cargo.exe test -p ao2-cli" in command and "cli_factory_greenfield_spec_ingest" in command
        for command in joined
    )
    assert not any("cli_approval_replay cli_factory_greenfield" in command for command in joined)
    assert any(
        "cargo.exe test -p ao2-cli" in command and "cli_greenfield_three_os" in command
        for command in joined
    )
    assert not any("cli_approval_replay cli_greenfield" in command for command in joined)
    assert any(
        "cargo.exe test -p ao2-cli" in command and "cli_factory_governed" in command
        for command in joined
    )
    assert not any("cli_approval_replay cli_factory_governed" in command for command in joined)
    assert any(
        "cargo.exe test -p ao2-cli" in command and "cli_factory_replacement" in command
        for command in joined
    )
    assert not any("cli_approval_replay cli_factory_replacement" in command for command in joined)
    assert any("cargo.exe test -p ao2-cli" in command and "cli_release_install" in command for command in joined)
    assert any("cargo.exe test -p ao2-cli" in command and "cli_workbench_queue" in command for command in joined)
    assert any("npm.cmd run test:archive-resources" in command for command in joined)
    assert any("cargo.exe clippy --locked --workspace --all-targets --all-features" in command for command in joined)
    assert any("cargo.exe build --release -p ao2-cli" in command for command in joined)
    assert not any("cargo.exe test --workspace --target-dir" in command for command in joined)


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
        execution_authorization_verifier=allow_test_execution,
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


def test_windows_stack_qualification_records_shard_checkpoint_deadline_and_profile_digest(
    tmp_path: Path, monkeypatch
) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    repo = factory / "ao2"
    (repo / ".git").mkdir(parents=True)

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES):
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
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    result = runtime.run_action(
        "windows_stack_qualification",
        {
            "mode": "diagnostic",
            "repositories": ["ao2"],
            "shard_id": "diagnostic-ao2",
            "checkpoint_id": "checkpoint-001",
            "profile_digest": "sha256:test-profile",
            "global_deadline_seconds": 120,
        },
        request_id="sharded-diagnostic",
    )

    assert result["status"] == "accepted"
    assert result["shard_id"] == "diagnostic-ao2"
    assert result["checkpoint_id"] == "checkpoint-001"
    assert result["profile_digest"] == "sha256:test-profile"
    assert result["global_deadline_seconds"] == 120
    assert result["results"][0]["shard_id"] == "diagnostic-ao2"
    assert result["results"][0]["checkpoint_id"] == "checkpoint-001"
    assert result["results"][0]["profile_digest"] == "sha256:test-profile"


def test_windows_stack_qualification_global_deadline_stops_before_next_profile_row(
    tmp_path: Path, monkeypatch
) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    repo = factory / "ao2"
    (repo / ".git").mkdir(parents=True)
    commands_seen: list[list[str]] = []

    monkeypatch.setattr(
        worker,
        "DIAGNOSTIC_PROFILE",
        (
            {"name": "slow-row", "argv": ("python", "-c", "slow")},
            {"name": "should-not-run", "argv": ("python", "-c", "later")},
        ),
    )

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES):
        commands_seen.append(list(command))
        return {
            "status": "accepted",
            "exit_code": 0,
            "timed_out": False,
            "duration_seconds": 31.0,
            "output": "slow ok",
            "output_truncated": False,
            "sanitized_stderr_category": "none",
            "command_name": Path(command[0]).name,
        }

    monkeypatch.setattr(worker, "run_bounded_child", fake_run)
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    result = runtime.run_action(
        "windows_stack_qualification",
        {"mode": "diagnostic", "repositories": ["ao2"], "global_deadline_seconds": 30},
        request_id="deadline",
    )

    assert result["status"] == "failed"
    assert result["error_category"] == "global_deadline_exceeded"
    assert len(commands_seen) == 1
    assert [row["sanitized_command_name"] for row in result["results"]] == ["slow-row"]


def test_windows_stack_qualification_invalidates_reuse_when_profile_digest_changes(
    tmp_path: Path, monkeypatch
) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    repo = factory / "ao2"
    (repo / ".git").mkdir(parents=True)
    commands_seen: list[list[str]] = []

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES):
        commands_seen.append(list(command))
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
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    result = runtime.run_action(
        "windows_stack_qualification",
        {
            "mode": "diagnostic",
            "repositories": ["ao2"],
            "reuse_from_request_id": "retained-windows-run",
            "reuse_profile_digest": "sha256:old-profile",
            "profile_digest": "sha256:new-profile",
        },
        request_id="reuse-invalidated",
    )

    assert result["status"] == "failed"
    assert result["error_category"] == "reuse_invalidated"
    assert result["reuse_from_request_id"] == "retained-windows-run"
    assert result["reuse_invalidated"] is True
    assert commands_seen == []


def test_windows_stack_qualification_physical_unique_runs_only_contract_physical_rows(
    tmp_path: Path, monkeypatch
) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    for repo_name in ("ao2", "ao2-control-plane", "ao-command"):
        (factory / repo_name / ".git").mkdir(parents=True)
    commands_seen: list[list[str]] = []

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES, env=None):
        commands_seen.append(list(command))
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
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    result = runtime.run_action(
        "windows_stack_qualification",
        {
            "mode": "physical_unique",
            "repositories": ["ao2", "ao2-control-plane", "ao-command"],
            "profile_digest": "sha256:physical-unique",
        },
        request_id="physical-unique",
    )

    assert result["status"] == "accepted"
    assert result["mode"] == "physical_unique"
    command_names = [row["sanitized_command_name"] for row in result["results"]]
    assert command_names == [
        "windows-worker-pytest",
        "ao2-doctor",
        "windows-file-locking-rollback",
        "physical-windows-lifecycle",
        "delegated-to-hosted-native-windows",
        "delegated-to-hosted-native-windows",
    ]
    assert all("release-readiness" not in " ".join(command) for command in commands_seen)
    assert all("clippy" not in " ".join(command) for command in commands_seen)
    assert len(commands_seen) == 4


def test_physical_unique_doctor_uses_prepared_binary_not_cargo_run(
    tmp_path: Path, monkeypatch
) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    (factory / "ao2" / ".git").mkdir(parents=True)
    commands_seen: list[list[str]] = []

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES, env=None):
        commands_seen.append(list(command))
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
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    result = runtime.run_action(
        "windows_stack_qualification",
        {"mode": "physical_unique", "repositories": ["ao2"]},
        request_id="physical-unique-prepared-doctor",
    )

    assert result["status"] == "accepted"
    assert commands_seen[1] == [
        str(worker.prepared_ao2_doctor_binary(factory)),
        "doctor",
        "--json",
    ]
    assert "cargo" not in Path(commands_seen[1][0]).name.lower()
    assert "run" not in commands_seen[1]


def test_physical_unique_lifecycle_probe_is_fixed_and_rejects_task_execution_overrides(
    tmp_path: Path, monkeypatch
) -> None:
    worker = load_worker_module()
    factory = tmp_path / "factory"
    (factory / "ao2" / ".git").mkdir(parents=True)
    commands_seen: list[tuple[list[str], Path, dict[str, str] | None]] = []

    def fake_run(command, *, cwd, timeout_seconds, output_limit_bytes=worker.DEFAULT_OUTPUT_LIMIT_BYTES, env=None):
        commands_seen.append((list(command), Path(cwd), env))
        return {
            "status": "accepted",
            "exit_code": 0,
            "timed_out": False,
            "duration_seconds": 0.01,
            "output": "{}",
            "output_truncated": False,
            "sanitized_stderr_category": "none",
            "command_name": Path(command[0]).name,
        }

    monkeypatch.setattr(worker, "run_bounded_child", fake_run)
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=factory,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        poll_interval_seconds=0.01,
    )

    result = runtime.run_action(
        "windows_stack_qualification",
        {"mode": "physical_unique", "repositories": ["ao2"]},
        request_id="physical-lifecycle-fixed",
    )

    assert result["status"] == "accepted"
    lifecycle_command = next(
        command
        for command, _, _ in commands_seen
        if any(part.endswith("Test-AO2PhysicalWindowsLifecycle.ps1") for part in command)
    )
    assert lifecycle_command[1:] == [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        "scripts/Test-AO2PhysicalWindowsLifecycle.ps1",
    ]
    assert next(cwd for command, cwd, _ in commands_seen if command == lifecycle_command) == factory / "ao2"
    assert next(env for command, _, env in commands_seen if command == lifecycle_command) is None

    for forbidden_key in worker.STACK_QUALIFICATION_FORBIDDEN_PARAMETERS:
        denied = runtime.run_action(
            "windows_stack_qualification",
            {"mode": "physical_unique", "repositories": ["ao2"], forbidden_key: "task-controlled"},
            request_id=f"physical-lifecycle-{forbidden_key}",
        )
        assert denied["status"] == "failed"
        assert denied["error_category"] == "unsupported_parameter"


def test_physical_lifecycle_probe_reads_the_workspace_version_without_emitting_command_lines() -> None:
    probe = PHYSICAL_LIFECYCLE_PROBE_PATH.read_text(encoding="utf-8")

    assert 'Get-SourceVersion -CargoTomlPath (Join-Path $repositoryRoot "Cargo.toml")' in probe
    assert 'Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $PID"' in probe
    assert "$workerProcessId = [int]$probeProcess.ParentProcessId" in probe
    assert 'Get-CimInstance -ClassName Win32_Process -Filter "ProcessId = $workerProcessId"' in probe
    assert "worker_script_matches" in probe
    assert "probe_parent_is_worker" in probe
    assert "worker_executable_is_python" in probe
    assert "Get-ScheduledTaskInfo" in probe
    assert "last_task_result" in probe
    assert "result_acceptable" in probe
    assert "267009" in probe
    assert "action_matches_worker" in probe
    assert "$task.Actions" in probe
    assert "Select-Object -First 1" not in probe
    assert "Name = 'python.exe'" not in probe
    assert "taskeng.exe" not in probe
    assert "taskhostw.exe" not in probe
    assert "command_line" not in probe
    assert "request_id" not in probe
    assert "result_id" not in probe
    assert "completed_at" not in probe
    assert "AO2_BUILD_GIT_COMMIT" in probe
    assert '"build", "--locked", "-p", "ao2-cli", "--bin", "ao2", "--target-dir", $targetRoot' in probe
    assert (
        '"build", "--locked", "--release", "-p", "ao2-cli", "--bin", "ao2", "--target-dir", $targetRoot'
        in probe
    )
    assert "version --json" in probe
    assert "release package" in probe
    assert "BUILD-PROVENANCE.json" in probe
    assert "RELEASE-MANIFEST.json" in probe
    assert 'Join-Path $extractRoot "install.ps1"' in probe
    assert "ao2.exe.install-verification.json" in probe
    assert "$installVerification.offline_verification.schema_version" in probe
    assert "$installVerification.offline_verification.status" in probe
    assert "$installVerification.offline_verification.checksum_coverage_verified" in probe
    assert '$installVerification.release_acceptance_owner -ne "factory-v3 evaluator-closer"' in probe
    assert "install rollback --install-dir $installRoot" in probe
    assert "Get-FileHash -Algorithm SHA256" in probe
    assert "rollback_runner_separate" in probe
    assert "temp_cleanup_completed" in probe
    assert "ConvertTo-Json -Compress" in probe


def test_physical_lifecycle_probe_handles_clean_git_output_on_windows_powershell() -> None:
    probe = PHYSICAL_LIFECYCLE_PROBE_PATH.read_text(encoding="utf-8")

    assert '$cleanTree = @(& git -C $repositoryRoot status --porcelain 2>$null).Count -eq 0' in probe


def test_physical_lifecycle_probe_returns_nonzero_after_a_caught_failure() -> None:
    probe = PHYSICAL_LIFECYCLE_PROBE_PATH.read_text(encoding="utf-8")

    assert "$result | ConvertTo-Json -Compress -Depth 5" in probe
    assert "if (-not $lifecycleSucceeded) {" in probe
    assert "exit 1" in probe


def test_physical_lifecycle_probe_emits_only_a_fixed_failure_stage() -> None:
    probe = PHYSICAL_LIFECYCLE_PROBE_PATH.read_text(encoding="utf-8")

    assert '$failureStage = "source-cleanliness"' in probe
    assert '$failureStage = "debug-build"' in probe
    assert '$failureStage = "debug-identity"' in probe
    assert (
        '[Console]::Error.WriteLine("physical_windows_lifecycle_failure_stage=$failureStage")'
        in probe
    )
    assert "$_.Exception.Message" not in probe


def test_physical_lifecycle_probe_preserves_native_exit_codes_under_strict_error_handling() -> None:
    probe = PHYSICAL_LIFECYCLE_PROBE_PATH.read_text(encoding="utf-8")

    assert "function Invoke-QuietNativeCommand" in probe
    assert '$ErrorActionPreference = "Continue"' in probe
    assert "return [int]$nativeExitCode" in probe
    assert probe.count("Invoke-QuietNativeCommand -FilePath cargo") == 2
    assert "Invoke-QuietNativeCommand -FilePath tar" in probe
    assert "& cargo build" not in probe
    assert "& tar -xzf" not in probe


def test_execution_authorization_accepts_exact_short_lived_signed_action(
    tmp_path: Path, monkeypatch
) -> None:
    worker = load_worker_module()
    private_key, public_key = generate_rsa_keypair(tmp_path)
    task = worker.control_task(request_id="signed-doctor", action="ao2_doctor")
    signed = worker.authorize_control_task(
        task,
        private_key_path=private_key,
        public_key_path=public_key,
        ttl_seconds=300,
        trusted_key_sha256s=(worker.public_key_sha256(public_key.read_text()),),
    )
    transport = worker.MemoryTransport()
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=worker.WorkerState(tmp_path / "state"),
        transport=transport,
        trusted_execution_key_sha256s=(worker.public_key_sha256(public_key.read_text()),),
    )
    monkeypatch.setattr(
        runtime,
        "run_action",
        lambda action, parameters, request_id="": {
            "status": "accepted",
            "action": action,
            "request_id": request_id,
        },
    )

    assert runtime.accept_control_task(signed) == "started"
    assert runtime.wait_for_idle(timeout_seconds=2)
    result = transport.posted_results_by_request_id()["signed-doctor"]["ao2_cross_host"]["result"]
    assert result["status"] == "accepted"
    assert result["request_id"] == "signed-doctor"
    receipt = result["execution_authorization"]
    assert receipt["status"] == "verified"
    assert receipt["request_id"] == "signed-doctor"
    assert receipt["target_node"] == "windows-hp255_g10"
    assert receipt["action_digest"] == worker.execution_action_digest(
        signed["ao2_cross_host"]
    )
    assert receipt["signing_public_key_sha256"] == worker.public_key_sha256(
        public_key.read_text()
    )
    assert "signature" not in json.dumps(receipt)
    assert "public_key_pem" not in receipt
    assert runtime.accept_control_task(signed) == "duplicate"


def test_execution_authorization_rejects_unsigned_and_altered_mutations(tmp_path: Path) -> None:
    worker = load_worker_module()
    private_key, public_key = generate_rsa_keypair(tmp_path)
    trusted = (worker.public_key_sha256(public_key.read_text()),)
    transport = worker.MemoryTransport()
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=worker.WorkerState(tmp_path / "state"),
        transport=transport,
        trusted_execution_key_sha256s=trusted,
    )

    unsigned = worker.control_task(request_id="unsigned-sync", action="sync_ao_stack")
    assert runtime.accept_control_task(unsigned) == "rejected"
    unsigned_result = transport.posted_results_by_request_id()["unsigned-sync"]["ao2_cross_host"]["result"]
    assert unsigned_result["error_category"] == "execution_authorization_missing"
    assert "unsigned-sync" not in runtime.state._ledger["tasks"]

    signed = worker.authorize_control_task(
        worker.control_task(
            request_id="altered-qualification",
            action="windows_stack_qualification",
            parameters={"mode": "physical_unique", "repositories": ["ao2"]},
        ),
        private_key_path=private_key,
        public_key_path=public_key,
        trusted_key_sha256s=trusted,
    )
    signed["ao2_cross_host"]["parameters"]["repositories"] = ["ao-command"]
    assert runtime.accept_control_task(signed) == "rejected"
    altered_result = transport.posted_results_by_request_id()["altered-qualification"]["ao2_cross_host"]["result"]
    assert altered_result["error_category"] == "execution_authorization_action_digest_mismatch"
    assert "altered-qualification" not in runtime.state._ledger["tasks"]


def test_execution_authorization_rejects_stale_and_untrusted_signatures(tmp_path: Path) -> None:
    worker = load_worker_module()
    private_key, public_key = generate_rsa_keypair(tmp_path)
    stale = worker.authorize_control_task(
        worker.control_task(request_id="stale-doctor", action="ao2_doctor"),
        private_key_path=private_key,
        public_key_path=public_key,
        issued_at=datetime.now(timezone.utc) - timedelta(hours=1),
        ttl_seconds=300,
        trusted_key_sha256s=(worker.public_key_sha256(public_key.read_text()),),
    )
    transport = worker.MemoryTransport()
    trusted_runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=worker.WorkerState(tmp_path / "trusted-state"),
        transport=transport,
        trusted_execution_key_sha256s=(worker.public_key_sha256(public_key.read_text()),),
    )

    assert trusted_runtime.accept_control_task(stale) == "rejected"
    stale_result = transport.posted_results_by_request_id()["stale-doctor"]["ao2_cross_host"]["result"]
    assert stale_result["error_category"] == "execution_authorization_expired"

    other_private, other_public = generate_rsa_keypair(tmp_path / "other")
    untrusted = worker.authorize_control_task(
        worker.control_task(request_id="untrusted-doctor", action="ao2_doctor"),
        private_key_path=other_private,
        public_key_path=other_public,
        trusted_key_sha256s=(worker.public_key_sha256(other_public.read_text()),),
    )
    assert trusted_runtime.accept_control_task(untrusted) == "rejected"
    untrusted_result = transport.posted_results_by_request_id()["untrusted-doctor"]["ao2_cross_host"]["result"]
    assert untrusted_result["error_category"] == "execution_authorization_untrusted_key"


def test_execution_authorization_preserves_unsigned_observer_probes_and_target_filter(
    tmp_path: Path,
) -> None:
    worker = load_worker_module()
    transport = worker.MemoryTransport()
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=worker.WorkerState(tmp_path / "state"),
        transport=transport,
        trusted_execution_key_sha256s=(worker.DEFAULT_EXECUTION_KEY_SHA256,),
    )

    assert runtime.accept_control_task(
        worker.control_task(request_id="observer-status", action="status")
    ) == "completed"
    assert runtime.accept_control_task(
        worker.control_task(
            request_id="wrong-node-mutation",
            action="sync_ao_stack",
            target_node="other-node",
        )
    ) == "ignored"
    assert runtime.accept_control_task(
        worker.control_task(request_id="observer-status", action="status")
    ) == "duplicate"


def test_authorizer_function_emits_canonical_verifiable_board(tmp_path: Path) -> None:
    worker = load_worker_module()
    private_key, public_key = generate_rsa_keypair(tmp_path)
    board = {
        "schema_version": worker.TASK_BOARD_SCHEMA,
        "tasks": [
            worker.control_task(
                request_id="cli-qualification",
                action="windows_stack_qualification",
                parameters={"mode": "physical_unique", "repositories": ["ao2"]},
            )
        ],
    }
    authorized_task = worker.authorize_control_task(
        board["tasks"][0],
        private_key_path=private_key,
        public_key_path=public_key,
        ttl_seconds=300,
        trusted_key_sha256s=(worker.public_key_sha256(public_key.read_text()),),
    )
    authorized = {"schema_version": worker.TASK_BOARD_SCHEMA, "tasks": [authorized_task]}
    authorized_bytes = worker.canonical_json_bytes(authorized) + b"\n"
    assert json.loads(authorized_bytes) == authorized
    verified, category = worker.verify_execution_authorization(
        authorized["tasks"][0]["ao2_cross_host"],
        trusted_key_sha256s=(worker.public_key_sha256(public_key.read_text()),),
        state_root=tmp_path / "state",
    )
    assert (verified, category) == (True, "execution_authorization_verified")
    assert private_key.read_text() not in authorized_bytes.decode()


def test_authorizer_cli_rejects_malformed_and_oversized_input(tmp_path: Path) -> None:
    private_key, public_key = generate_rsa_keypair(tmp_path)
    output_path = tmp_path / "authorized.json"
    for name, payload in (
        ("malformed.json", b"{"),
        ("nonstandard.json", b'{"schema_version":"ao2.ai-task-board.v1","tasks":[NaN]}'),
        ("oversized.json", b"x" * (1024 * 1024 + 1)),
    ):
        input_path = tmp_path / name
        input_path.write_bytes(payload)
        result = subprocess.run(
            [
                sys.executable,
                str(AUTHORIZER_PATH),
                "--input",
                str(input_path),
                "--output",
                str(output_path),
                "--private-key",
                str(private_key),
                "--public-key",
                str(public_key),
            ],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        assert result.returncode != 0
    assert not output_path.exists()


def test_authorizer_rejects_private_mismatched_and_untrusted_public_keys(tmp_path: Path) -> None:
    worker = load_worker_module()
    private_key, public_key = generate_rsa_keypair(tmp_path / "first")
    other_private, other_public = generate_rsa_keypair(tmp_path / "second")
    task = worker.control_task(request_id="key-validation", action="ao2_doctor")
    trusted = (worker.public_key_sha256(public_key.read_text()),)

    for candidate_private, candidate_public, expected in (
        (private_key, private_key, "public key PEM"),
        (private_key, other_public, "does not match private key"),
        (other_private, other_public, "not trusted"),
    ):
        try:
            worker.authorize_control_task(
                task,
                private_key_path=candidate_private,
                public_key_path=candidate_public,
                trusted_key_sha256s=trusted,
            )
        except ValueError as exc:
            assert expected in str(exc)
        else:
            raise AssertionError(f"expected key validation failure: {expected}")


def test_worker_rejects_non_object_parameters_before_authorization_or_execution(
    tmp_path: Path,
) -> None:
    worker = load_worker_module()
    verifier_calls = []
    executions = []
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=worker.WorkerState(tmp_path / "state"),
        transport=worker.MemoryTransport(),
        execution_authorization_verifier=lambda *_args, **_kwargs: verifier_calls.append(True)
        or (True, "verified"),
    )
    runtime.run_action = lambda *_args, **_kwargs: executions.append(True) or {"status": "accepted"}
    task = worker.control_task(request_id="array-parameters", action="ao2_doctor")
    task["ao2_cross_host"]["parameters"] = ["ignored-by-old-worker"]

    assert runtime.accept_control_task(task) == "rejected"
    assert verifier_calls == []
    assert executions == []
    assert "array-parameters" not in runtime.state._ledger["tasks"]


def test_poll_once_bounds_task_count_authorization_work_and_duplicate_ids(tmp_path: Path) -> None:
    worker = load_worker_module()

    class BoardTransport(worker.MemoryTransport):
        def __init__(self, board):
            super().__init__()
            self.board = board

        def latest_board(self):
            return self.board

    verifier_calls = []
    over_budget = {
        "schema_version": worker.TASK_BOARD_SCHEMA,
        "tasks": [
            worker.control_task(request_id=f"budget-{index}", action="ao2_doctor")
            for index in range(worker.MAX_EXECUTION_AUTHORIZATIONS_PER_POLL + 1)
        ],
    }
    runtime = worker.WindowsOutboundWorker(
        node_id="windows-hp255_g10",
        factory_root=tmp_path,
        state=worker.WorkerState(tmp_path / "budget-state"),
        transport=BoardTransport(over_budget),
        execution_authorization_verifier=lambda *_args, **_kwargs: verifier_calls.append(True)
        or (False, "invalid"),
    )
    assert runtime.poll_once() == "authorization_budget_exceeded"
    assert verifier_calls == []

    oversized = {
        "schema_version": worker.TASK_BOARD_SCHEMA,
        "tasks": [
            worker.control_task(request_id=f"status-{index}", action="status")
            for index in range(worker.MAX_CONTROL_TASKS_PER_BOARD + 1)
        ],
    }
    runtime.transport = BoardTransport(oversized)
    assert runtime.poll_once() == "board_task_limit_exceeded"

    duplicate = {
        "schema_version": worker.TASK_BOARD_SCHEMA,
        "tasks": [
            worker.control_task(request_id="same-id", action="ao2_doctor"),
            worker.control_task(request_id="same-id", action="sync_ao_stack"),
        ],
    }
    runtime.transport = BoardTransport(duplicate)
    assert runtime.poll_once() == "duplicate_control_request_id"
    assert verifier_calls == []
