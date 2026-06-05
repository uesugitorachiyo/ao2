#!/usr/bin/env python3
"""Smoke AO2 provider-registry publish/readback against a local control plane.

Starts a temporary ao2-cp-server, publishes `ao2 provider registry` with
AO2_CP_API_TOKEN via `--api-token-env`, reads the observer endpoints back, and
asserts provider metadata-source fields plus observer-only trust boundaries.

The token value is never printed. Summary JSON records TOKEN_REDACTED.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import platform
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


TOKEN_ENV = "AO2_CP_API_TOKEN"
TOKEN_REDACTED = "TOKEN_REDACTED"
PROVIDER_DASHBOARD_JSON = "/api/v1/provider/registry/dashboard.json"
PROVIDER_LATEST = "/api/v1/provider/registry/latest"
PROVIDER_DETAIL_JSON = "/api/v1/provider/registry/{sha}/detail.json"


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def timestamp() -> str:
    return dt.datetime.now(dt.UTC).strftime("%Y%m%dT%H%M%SZ")


def exe_name(name: str) -> str:
    return f"{name}.exe" if platform.system().lower() == "windows" else name


def binary_path(root: Path, package_binary: str, profile: str) -> Path:
    profile_dir = "release" if profile == "release" else "debug"
    return root / "target" / profile_dir / exe_name(package_binary)


def scrub_env() -> dict[str, str]:
    env = os.environ.copy()
    env.pop("OPENAI_API_KEY", None)
    env.pop("ANTHROPIC_API_KEY", None)
    return env


def run_command(
    args: list[str],
    *,
    cwd: Path,
    timeout: int,
    env: dict[str, str] | None = None,
    stdout_path: Path | None = None,
    stderr_path: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    stdout_handle = stdout_path.open("w", encoding="utf-8") if stdout_path else subprocess.PIPE
    stderr_handle = stderr_path.open("w", encoding="utf-8") if stderr_path else subprocess.PIPE
    try:
        result = subprocess.run(
            args,
            cwd=str(cwd),
            env=env,
            text=True,
            stdout=stdout_handle,
            stderr=stderr_handle,
            timeout=timeout,
            check=False,
        )
    finally:
        if stdout_path:
            stdout_handle.close()
        if stderr_path:
            stderr_handle.close()
    if result.returncode != 0:
        raise RuntimeError(
            f"command failed rc={result.returncode} timeout={timeout}: {' '.join(args)}"
        )
    return result


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def fetch_json(url: str, token: str, timeout: int) -> Any:
    request = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/json",
        },
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def fetch_health(url: str, timeout: int) -> bool:
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return 200 <= int(response.status) < 300
    except (OSError, urllib.error.URLError):
        return False


def wait_for_server(base_url: str, proc: subprocess.Popen[str], timeout: int) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"ao2-cp-server exited early rc={proc.returncode}")
        if fetch_health(f"{base_url}/healthz", 2):
            return
        time.sleep(0.25)
    raise TimeoutError(f"timed out waiting for ao2-cp-server healthz timeout={timeout}")


def provider_by_name(items: list[Any], name: str) -> Any:
    for item in items:
        if item.get("provider") == name:
            return item
    raise AssertionError(f"missing provider {name}")


def assert_provider_metadata(item: Any, provider: str, expected_source: str) -> None:
    assert item.get("provider") == provider
    assert item.get("metadata_source") == expected_source, item
    assert item.get("doctor_metadata_source") == expected_source, item
    assert item.get("adapter_crate") in (expected_source, None), item
    assert item.get("adapter_kind"), item


def assert_raw_provider_metadata(item: Any, provider: str, expected_source: str) -> None:
    assert item.get("provider") == provider
    assert item.get("metadata_source") == expected_source, item
    assert item.get("crate") == expected_source, item
    doctor = item.get("doctor") or {}
    assert doctor.get("metadata_source") == expected_source, item


def assert_token_absent(paths: list[Path], token: str) -> None:
    for path in paths:
        if not path.exists() or path.is_dir():
            continue
        content = path.read_text(encoding="utf-8", errors="replace")
        if token in content:
            raise AssertionError(f"secret token leaked into {path}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--control-plane-repo", type=Path, default=repo_root().parent / "ao2-control-plane")
    parser.add_argument("--out-dir", type=Path, default=None)
    parser.add_argument("--profile", choices=["debug", "release"], default="debug")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--command-timeout-seconds", type=int, default=180)
    parser.add_argument("--server-start-timeout-seconds", type=int, default=30)
    parser.add_argument("--http-timeout-seconds", type=int, default=15)
    args = parser.parse_args()

    root = repo_root()
    cp_repo = args.control_plane_repo.resolve()
    if not cp_repo.exists():
        raise SystemExit(f"control plane repo not found: {cp_repo}")

    out_dir = (args.out_dir or (root / "target" / f"provider-registry-control-plane-smoke-{timestamp()}")).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    data_dir = out_dir / "cp-data"
    data_dir.mkdir(parents=True, exist_ok=True)

    env = scrub_env()
    token = secrets.token_hex(32)
    env[TOKEN_ENV] = token

    cargo = shutil.which("cargo")
    if not cargo:
        raise SystemExit("cargo not found on PATH")

    if not args.skip_build:
        run_command(
            [cargo, "build", "--package", "ao2-cli"],
            cwd=root,
            timeout=args.command_timeout_seconds,
            env=env,
            stdout_path=out_dir / "build-ao2.stdout.log",
            stderr_path=out_dir / "build-ao2.stderr.log",
        )
        run_command(
            [cargo, "build", "--manifest-path", str(cp_repo / "Cargo.toml"), "--package", "ao2-cp-server"],
            cwd=cp_repo,
            timeout=args.command_timeout_seconds,
            env=env,
            stdout_path=out_dir / "build-control-plane.stdout.log",
            stderr_path=out_dir / "build-control-plane.stderr.log",
        )

    ao2_bin = binary_path(root, "ao2", args.profile)
    cp_bin = binary_path(cp_repo, "ao2-cp-server", args.profile)
    if not ao2_bin.exists():
        raise SystemExit(f"ao2 binary not found: {ao2_bin}")
    if not cp_bin.exists():
        raise SystemExit(f"ao2-cp-server binary not found: {cp_bin}")

    port = free_port()
    base_url = f"http://127.0.0.1:{port}"
    server_stdout = (out_dir / "ao2-cp-server.stdout.log").open("w", encoding="utf-8")
    server_stderr = (out_dir / "ao2-cp-server.stderr.log").open("w", encoding="utf-8")
    server = subprocess.Popen(
        [
            str(cp_bin),
            "--bind",
            f"127.0.0.1:{port}",
            "--data-dir",
            str(data_dir),
        ],
        cwd=str(cp_repo),
        env=env,
        text=True,
        stdout=server_stdout,
        stderr=server_stderr,
    )

    generated_paths = [
        out_dir / "build-ao2.stdout.log",
        out_dir / "build-ao2.stderr.log",
        out_dir / "build-control-plane.stdout.log",
        out_dir / "build-control-plane.stderr.log",
        out_dir / "ao2-cp-server.stdout.log",
        out_dir / "ao2-cp-server.stderr.log",
        out_dir / "publish.json",
        out_dir / "latest.json",
        out_dir / "dashboard.json",
        out_dir / "detail.json",
        out_dir / "summary.json",
    ]

    try:
        wait_for_server(base_url, server, args.server_start_timeout_seconds)

        # provider registry --control-plane-url publishes AO2-produced registry evidence.
        publish = run_command(
            [
                str(ao2_bin),
                "provider",
                "registry",
                "--control-plane-url",
                base_url,
                "--api-token-env",
                TOKEN_ENV,
                "--json",
            ],
            cwd=root,
            timeout=args.command_timeout_seconds,
            env=env,
        )
        (out_dir / "publish.json").write_text(publish.stdout, encoding="utf-8")
        publish_json = json.loads(publish.stdout)
        sha = publish_json["receipt"]["sha256"]
        assert len(sha) == 64, sha
        assert publish_json["signed"] is False
        assert publish_json["endpoint"].endswith("/api/v1/provider/registry")
        assert publish_json["latest_url"].endswith(PROVIDER_LATEST)

        latest = fetch_json(f"{base_url}{PROVIDER_LATEST}", token, args.http_timeout_seconds)
        dashboard = fetch_json(
            f"{base_url}{PROVIDER_DASHBOARD_JSON}",
            token,
            args.http_timeout_seconds,
        )
        detail_path = PROVIDER_DETAIL_JSON.format(sha=sha)
        detail = fetch_json(f"{base_url}{detail_path}", token, args.http_timeout_seconds)

        (out_dir / "latest.json").write_text(json.dumps(latest, indent=2, sort_keys=True), encoding="utf-8")
        (out_dir / "dashboard.json").write_text(json.dumps(dashboard, indent=2, sort_keys=True), encoding="utf-8")
        (out_dir / "detail.json").write_text(json.dumps(detail, indent=2, sort_keys=True), encoding="utf-8")

        assert latest["schema"] == "ao2.provider-plugin-registry.v1"
        assert latest["trust_boundary"]["control_plane_role"] == "read_only_observer_only"
        assert latest["trust_boundary"]["provider_api_key_auth"] == "forbidden"
        assert dashboard["schema_version"] == "ao2.cp-provider-registry-dashboard.v1"
        assert dashboard["status"] == "observed"
        assert dashboard["trust_boundary"]["role"] == "read_only_observer"
        assert dashboard["trust_boundary"]["mutates_ao_artifacts"] is False
        assert detail["schema_version"] == "ao2.cp-provider-registry-detail.v1"
        assert detail["sha256"] == sha

        raw_codex = provider_by_name(latest["providers"], "codex")
        raw_claude = provider_by_name(latest["providers"], "claude")
        dashboard_codex = provider_by_name(dashboard["providers"], "codex")
        dashboard_claude = provider_by_name(dashboard["providers"], "claude")
        detail_codex = provider_by_name(detail["providers"], "codex")
        detail_claude = provider_by_name(detail["providers"], "claude")

        assert_raw_provider_metadata(raw_codex, "codex", "ao2-adapter-codex")
        assert_raw_provider_metadata(raw_claude, "claude", "ao2-adapter-claude")
        assert_provider_metadata(dashboard_codex, "codex", "ao2-adapter-codex")
        assert_provider_metadata(dashboard_claude, "claude", "ao2-adapter-claude")
        assert_provider_metadata(detail_codex, "codex", "ao2-adapter-codex")
        assert_provider_metadata(detail_claude, "claude", "ao2-adapter-claude")

        summary = {
            "schema_version": "ao2.provider-registry-control-plane-smoke.v1",
            "status": "passed",
            "platform": platform.platform(),
            "base_url": base_url,
            "token": TOKEN_REDACTED,
            "provider_registry_sha256": sha,
            "signed": False,
            "publish": str(out_dir / "publish.json"),
            "latest": str(out_dir / "latest.json"),
            "dashboard_json": str(out_dir / "dashboard.json"),
            "detail_json": str(out_dir / "detail.json"),
            "metadata_sources": {
                "codex": dashboard_codex["metadata_source"],
                "claude": dashboard_claude["metadata_source"],
            },
            "doctor_metadata_sources": {
                "codex": dashboard_codex["doctor_metadata_source"],
                "claude": dashboard_claude["doctor_metadata_source"],
            },
            "control_plane_role": dashboard["trust_boundary"]["role"],
            "mutates_ao_artifacts": dashboard["trust_boundary"]["mutates_ao_artifacts"],
            "provider_api_key_auth": latest["trust_boundary"]["provider_api_key_auth"],
        }
        (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8")
        assert_token_absent(generated_paths, token)
        print(f"provider_registry_control_plane_readback=passed")
        print(f"provider_registry_control_plane_summary={out_dir / 'summary.json'}")
        print(f"provider_registry_sha256={sha}")
        return 0
    finally:
        if server.poll() is None:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=10)
        server_stdout.close()
        server_stderr.close()


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"provider_registry_control_plane_readback=failed error={exc}", file=sys.stderr)
        raise
