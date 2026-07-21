#!/usr/bin/env python3
"""AO2 Windows outbound task-board worker.

The worker polls a Mac-hosted AO2 Control Plane and executes only explicit,
allowlisted local actions. It never opens a Windows HTTP listener and never
executes command text supplied by a task payload.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any


CONTROL_TASK_SCHEMA = "ao2.cross-host.control-task.v1"
EXECUTION_AUTHORIZATION_SCHEMA = "ao2.cross-host.execution-authorization.v1"
WORKER_RESULT_SCHEMA = "ao2.cross-host.windows-worker-result.v1"
TASK_BOARD_SCHEMA = "ao2.ai-task-board.v1"
DEFAULT_EXECUTION_KEY_SHA256 = "7fedf62781b08a50abff300425f47c79b72f76f7208024a951d0533ebdb8f28c"
EXECUTION_AUTHORIZATION_ALGORITHM = "rsa-pkcs1v15-sha256"
EXECUTION_AUTHORIZATION_MAX_TTL_SECONDS = 900
EXECUTION_AUTHORIZATION_CLOCK_SKEW_SECONDS = 300
UNSIGNED_OBSERVER_ACTIONS = ("status", "publish_capability")
MAX_CONTROL_TASKS_PER_BOARD = 32
MAX_EXECUTION_AUTHORIZATIONS_PER_POLL = 4
DEFAULT_NODE_ID = "windows-hp255_g10"
DEFAULT_FACTORY_ROOT = Path(r"C:\ao\factory") if os.name == "nt" else Path.cwd()
DEFAULT_STATE_ROOT = (
    Path(os.environ.get("LOCALAPPDATA", str(Path.home()))) / "AO2" / "windows-outbound-worker"
    if os.name == "nt"
    else Path.cwd() / "target" / "windows-outbound-worker"
)
DEFAULT_POLL_INTERVAL_SECONDS = 5.0
DEFAULT_DOCTOR_TIMEOUT_SECONDS = 120.0
DEFAULT_ACTION_TIMEOUT_SECONDS = 300.0
DEFAULT_STACK_QUALIFICATION_TIMEOUT_SECONDS = 600.0
MIN_STACK_QUALIFICATION_TIMEOUT_SECONDS = 30.0
MAX_STACK_QUALIFICATION_TIMEOUT_SECONDS = 3600.0
DEFAULT_OUTPUT_LIMIT_BYTES = 64 * 1024
ALLOWLISTED_ACTIONS = (
    "status",
    "publish_capability",
    "sync_ao_stack",
    "ao2_doctor",
    "timeout_fixture",
    "windows_stack_qualification",
)
CANONICAL_REPOSITORIES = (
    "ao-architecture",
    "ao-mission",
    "ao-blueprint",
    "ao-atlas",
    "ao-foundry",
    "ao-forge",
    "ao-covenant",
    "ao2",
    "ao2-control-plane",
    "ao-command",
    "ao-arena",
    "ao-crucible",
    "ao-sentinel",
    "ao-promoter",
)
ARCHIVED_REPOSITORIES = ("agy-swarms",)
STACK_QUALIFICATION_MODES = ("diagnostic", "targeted", "full", "physical_unique", "toolchain")
STACK_QUALIFICATION_ALLOWED_PARAMETERS = {
    "mode",
    "repositories",
    "repos",
    "timeout_seconds",
    "shard_id",
    "checkpoint_id",
    "profile_digest",
    "global_deadline_seconds",
    "reuse_from_request_id",
    "reuse_profile_digest",
}
STACK_QUALIFICATION_FORBIDDEN_PARAMETERS = {
    "command",
    "commands",
    "args",
    "argv",
    "cwd",
    "working_directory",
    "executable",
    "executable_path",
    "env",
    "environment",
    "powershell",
    "shell",
}
STACK_PROFILE_VERSION = "ao2.windows-stack-qualification.profiles.v1"
TOOLCHAIN_CAPABILITY_TOOLS = ("git", "go", "python", "cargo", "rustc", "node", "npm", "powershell")
FIXED_TOOL_PATH_COMMANDS = {
    "git": ("git", "git.exe"),
    "go": ("go", "go.exe"),
    "python": (sys.executable,),
    "cargo": ("cargo", "cargo.exe"),
    "rustc": ("rustc", "rustc.exe"),
    "node": ("node", "node.exe"),
    "npm": ("npm", "npm.cmd", "npm.exe"),
    "powershell": ("pwsh", "pwsh.exe", "powershell", "powershell.exe"),
}
FIXED_TOOL_VERSION_ARGS = {
    "git": ("--version",),
    "go": ("version",),
    "python": ("--version",),
    "cargo": ("--version",),
    "rustc": ("--version",),
    "node": ("--version",),
    "npm": ("--version",),
    "powershell": ("-NoProfile", "-Command", "$PSVersionTable.PSVersion.ToString()"),
}
ProfileCommand = dict[str, Any]


def ao2_cli_approval_profile_commands(
    group_name: str,
    filters: tuple[str, ...],
    *extra_args: str,
) -> tuple[ProfileCommand, ...]:
    return tuple(
        {
            "name": f"{group_name}-{test_filter}",
            "argv": (
                "cargo",
                "test",
                "-p",
                "ao2-cli",
                "--target-dir",
                "{ao2-full-target-dir}",
                "--test",
                "cli_approval_replay",
                test_filter,
                *extra_args,
            ),
        }
        for test_filter in filters
    )


DIAGNOSTIC_PROFILE: tuple[ProfileCommand, ...] = (
    {"name": "git-head-readback", "argv": ("git", "rev-parse", "HEAD")},
    {"name": "git-clean-readback", "argv": ("git", "status", "--porcelain=v1")},
)
AO2_FULL_WINDOWS_PROFILE: tuple[ProfileCommand, ...] = (
    {"name": "windows-worker-pytest", "argv": ("{python}", "-m", "pytest", "tests/test_windows_outbound_worker.py", "-q")},
    {
        "name": "cargo-test-workspace-non-cli",
        "argv": ("cargo", "test", "--workspace", "--exclude", "ao2-cli", "--target-dir", "{ao2-full-target-dir}"),
    },
    {
        "name": "test-cli-adapter",
        "argv": (
            "cargo",
            "test",
            "-p",
            "ao2-cli",
            "--target-dir",
            "{ao2-full-target-dir}",
            "--test",
            "cli_adapter",
        ),
    },
    {
        "name": "test-cli-report-cockpit",
        "argv": (
            "cargo",
            "test",
            "-p",
            "ao2-cli",
            "--target-dir",
            "{ao2-full-target-dir}",
            "--test",
            "cli_report_cockpit",
        ),
    },
    *ao2_cli_approval_profile_commands(
        "test-cli-approval-core",
        (
            "cli_can_pause",
            "cli_contract",
            "cli_doctor",
            "cli_evidence",
            "cli_git",
            "cli_init",
            "cli_install",
            "cli_memory",
            "cli_repair",
            "cli_report",
            "cli_run",
            "cli_skill",
            "cli_template",
            "cli_upgrade",
            "cli_version",
        ),
    ),
    *ao2_cli_approval_profile_commands("test-cli-approval-control-plane", ("cli_control_plane",)),
    *ao2_cli_approval_profile_commands("test-cli-approval-factory-plan", ("cli_factory_plan",)),
    *ao2_cli_approval_profile_commands("test-cli-approval-factory-queue", ("cli_factory_queue",)),
    *ao2_cli_approval_profile_commands("test-cli-approval-factory-project", ("cli_factory_project",)),
    *ao2_cli_approval_profile_commands(
        "test-cli-approval-factory-other",
        (
            "cli_factory_app",
            "cli_factory_closer",
            "cli_factory_evaluator",
            "cli_factory_governed",
            "cli_factory_greenfield",
            "cli_factory_pack",
            "cli_factory_replacement",
            "cli_factory_run",
            "cli_factory_verify",
            "cli_greenfield",
        ),
    ),
    *ao2_cli_approval_profile_commands("test-cli-approval-plugin", ("cli_plugin",)),
    *ao2_cli_approval_profile_commands(
        "test-cli-approval-pulse-provider-release",
        ("cli_pulse", "cli_provider", "cli_release"),
    ),
    *ao2_cli_approval_profile_commands(
        "test-cli-approval-workbench-core",
        (
            "cli_workbench_api",
            "cli_workbench_evidence",
            "cli_workbench_export",
            "cli_workbench_factory",
            "cli_workbench_greenfield",
            "cli_workbench_launch",
            "cli_workbench_lists",
            "cli_workbench_memory",
            "cli_workbench_obligation",
            "cli_workbench_operator",
            "cli_workbench_serve",
        ),
    ),
    *ao2_cli_approval_profile_commands("test-cli-approval-workbench-project", ("cli_workbench_project_start",)),
    *ao2_cli_approval_profile_commands("test-cli-approval-workbench-provider", ("cli_workbench_provider",)),
    *ao2_cli_approval_profile_commands(
        "test-cli-approval-workbench-queue",
        ("cli_workbench_queue",),
        "--",
        "--test-threads=1",
    ),
    *ao2_cli_approval_profile_commands(
        "test-cli-approval-workbench-release-run-support",
        ("cli_workbench_release", "cli_workbench_run_evidence", "cli_workbench_support"),
    ),
    {
        "name": "test-cli-contract-gate-signing",
        "argv": (
            "cargo",
            "test",
            "-p",
            "ao2-cli",
            "--target-dir",
            "{ao2-full-target-dir}",
            "--test",
            "contract_gate_support_signing",
            "--test",
            "contract_obligation_gate_signing_survey",
            "--test",
            "contract_verify_obligation_gate_signing",
        ),
    },
    {
        "name": "test-cli-factory-control",
        "argv": (
            "cargo",
            "test",
            "-p",
            "ao2-cli",
            "--target-dir",
            "{ao2-full-target-dir}",
            "--test",
            "cp_release_snapshot",
            "--test",
            "factory_bridge",
            "--test",
            "factory_cancel_authority",
            "--test",
            "factory_cancel_transition",
        ),
    },
    {
        "name": "test-cli-release-readiness",
        "argv": (
            "cargo",
            "test",
            "-p",
            "ao2-cli",
            "--target-dir",
            "{ao2-full-target-dir}",
            "--test",
            "release_evaluator_decision",
            "--test",
            "release_gate_obligation_gate_signing",
            "--test",
            "release_handoff_checklist",
            "--test",
            "release_support_bundle_verification",
        ),
    },
    {
        "name": "test-archive-resources",
        "argv": ("npm", "run", "test:archive-resources"),
        "env": {"CARGO_TARGET_DIR": "{ao2-full-target-dir}"},
    },
    {
        "name": "test-cli-release-packaging-sdd",
        "argv": (
            "cargo",
            "test",
            "-p",
            "ao2-cli",
            "--target-dir",
            "{ao2-full-target-dir}",
            "--test",
            "release_packaging",
            "--test",
            "sdd_subcommand",
            "--",
            "--test-threads=1",
        ),
    },
    {"name": "cargo-fmt-check", "argv": ("cargo", "fmt", "--all", "--", "--check")},
    {
        "name": "cargo-clippy",
        "argv": (
            "cargo",
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--target-dir",
            "{ao2-full-target-dir}",
            "--",
            "-D",
            "warnings",
        ),
    },
    {"name": "cargo-build-release", "argv": ("cargo", "build", "--release", "-p", "ao2-cli", "--target-dir", "{ao2-full-target-dir}")},
)
AO2_PHYSICAL_UNIQUE_WINDOWS_PROFILE: tuple[ProfileCommand, ...] = (
    {"name": "windows-worker-pytest", "argv": ("{python}", "-m", "pytest", "tests/test_windows_outbound_worker.py", "-q")},
    {
        "name": "ao2-doctor",
        "argv": (
            "{ao2-prepared-doctor}",
            "doctor",
            "--json",
        ),
    },
    {
        "name": "windows-file-locking-rollback",
        "argv": (
            "cargo",
            "test",
            "-p",
            "ao2-adapters",
            "--target-dir",
            "{ao2-physical-target-dir}",
            "--test",
            "sandbox_adapter",
            "windows",
        ),
    },
    {
        "name": "physical-windows-lifecycle",
        "argv": (
            "{powershell}",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            "scripts/Test-AO2PhysicalWindowsLifecycle.ps1",
        ),
    },
)
WINDOWS_REPOSITORY_PROFILES: dict[str, dict[str, tuple[ProfileCommand, ...]]] = {
    "ao-architecture": {
        "physical_unique": (),
        "targeted": (
            {"name": "architecture-stack-lock", "argv": ("{python}", "scripts/verify_stack_lock.py")},
            {"name": "architecture-current-release", "argv": ("{python}", "scripts/verify_current_release_manifest.py")},
        ),
        "full": (
            {"name": "architecture-pytest", "argv": ("{python}", "-m", "pytest", "scripts", "-q")},
        ),
    },
    "ao-mission": {
        "physical_unique": (),
        "targeted": ({"name": "go-test", "argv": ("go", "test", "./...")},),
        "full": ({"name": "go-test", "argv": ("go", "test", "./...")},),
    },
    "ao-blueprint": {
        "physical_unique": (),
        "targeted": ({"name": "blueprint-production-readiness", "argv": ("{powershell}", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/production-readiness.ps1")},),
        "full": (
            {"name": "go-test", "argv": ("go", "test", "./...")},
            {"name": "blueprint-production-readiness", "argv": ("{powershell}", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/production-readiness.ps1")},
        ),
    },
    "ao-atlas": {
        "physical_unique": (),
        "targeted": ({"name": "atlas-production-readiness", "argv": ("{powershell}", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/production-readiness.ps1")},),
        "full": (
            {"name": "go-test", "argv": ("go", "test", "./...")},
            {"name": "atlas-production-readiness", "argv": ("{powershell}", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/production-readiness.ps1")},
            {"name": "atlas-targeted-regressions", "argv": ("{powershell}", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/recommendation-targeted-regressions.ps1")},
        ),
    },
    "ao-foundry": {
        "physical_unique": (),
        "targeted": ({"name": "go-test", "argv": ("go", "test", "./...")},),
        "full": ({"name": "go-test", "argv": ("go", "test", "./...")},),
    },
    "ao-forge": {
        "physical_unique": (),
        "targeted": ({"name": "go-test", "argv": ("go", "test", "./...")},),
        "full": ({"name": "go-test", "argv": ("go", "test", "./...")},),
    },
    "ao-covenant": {
        "physical_unique": (),
        "targeted": ({"name": "go-test", "argv": ("go", "test", "./...")},),
        "full": (
            {"name": "go-test", "argv": ("go", "test", "./...")},
            {"name": "covenant-release-readiness", "argv": ("{powershell}", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "scripts/release-readiness.ps1")},
        ),
    },
    "ao2": {
        "physical_unique": AO2_PHYSICAL_UNIQUE_WINDOWS_PROFILE,
        "targeted": ({"name": "windows-worker-pytest", "argv": ("{python}", "-m", "pytest", "tests/test_windows_outbound_worker.py", "-q")},),
        "full": AO2_FULL_WINDOWS_PROFILE,
    },
    "ao2-control-plane": {
        "physical_unique": (),
        "targeted": ({"name": "cargo-test-workspace", "argv": ("cargo", "test", "--workspace")},),
        "full": (
            {"name": "cargo-test-workspace", "argv": ("cargo", "test", "--workspace")},
            {"name": "cargo-fmt-check", "argv": ("cargo", "fmt", "--all", "--", "--check")},
            {"name": "cargo-clippy", "argv": ("cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings")},
        ),
    },
    "ao-command": {
        "physical_unique": (),
        "targeted": ({"name": "go-test", "argv": ("go", "test", "./...")},),
        "full": ({"name": "go-test", "argv": ("go", "test", "./...")},),
    },
    "ao-arena": {
        "physical_unique": (),
        "targeted": ({"name": "go-test", "argv": ("go", "test", "./...")},),
        "full": ({"name": "go-test", "argv": ("go", "test", "./...")},),
    },
    "ao-crucible": {
        "physical_unique": (),
        "targeted": ({"name": "go-test", "argv": ("go", "test", "./...")},),
        "full": ({"name": "go-test", "argv": ("go", "test", "./...")},),
    },
    "ao-sentinel": {
        "physical_unique": (),
        "targeted": ({"name": "go-test", "argv": ("go", "test", "./...")},),
        "full": ({"name": "go-test", "argv": ("go", "test", "./...")},),
    },
    "ao-promoter": {
        "physical_unique": (),
        "targeted": ({"name": "go-test", "argv": ("go", "test", "./...")},),
        "full": ({"name": "go-test", "argv": ("go", "test", "./...")},),
    },
}
SECRET_PATTERNS = (
    re.compile(r"(?i)(authorization:\s*bearer\s+)[A-Za-z0-9._~+\-/=]+"),
    re.compile(r"(?i)(AO2_CP_API_TOKEN=)[^\s]+"),
    re.compile(r"(?i)(api[_-]?token['\"]?\s*[:=]\s*['\"]?)[^'\"\s,}]+"),
    re.compile(r"(?i)(password['\"]?\s*[:=]\s*['\"]?)[^'\"\s,}]+"),
)


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def canonical_json_bytes(value: Any) -> bytes:
    return json.dumps(
        value,
        allow_nan=False,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def canonical_public_key_bytes(public_key_pem: str) -> bytes:
    normalized = public_key_pem.replace("\r\n", "\n").replace("\r", "\n").strip()
    return (normalized + "\n").encode("ascii")


def public_key_sha256(public_key_pem: str) -> str:
    return hashlib.sha256(canonical_public_key_bytes(public_key_pem)).hexdigest()


def execution_action_digest(cross_host: dict[str, Any]) -> str:
    unsigned = {key: value for key, value in cross_host.items() if key != "execution_authorization"}
    return hashlib.sha256(canonical_json_bytes(unsigned)).hexdigest()


def parse_utc_timestamp(value: Any) -> datetime | None:
    if not isinstance(value, str) or not value.endswith("Z"):
        return None
    try:
        parsed = datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError:
        return None
    if parsed.tzinfo is None:
        return None
    return parsed.astimezone(timezone.utc)


def resolve_openssl() -> str | None:
    resolved = shutil.which("openssl") or shutil.which("openssl.exe")
    if resolved:
        return str(Path(resolved))
    if os.name == "nt":
        roots = [
            os.environ.get("ProgramFiles"),
            os.environ.get("ProgramFiles(x86)"),
            os.environ.get("ProgramW6432"),
        ]
        for root in roots:
            if not root:
                continue
            for relative in (
                Path("Git") / "usr" / "bin" / "openssl.exe",
                Path("OpenSSL-Win64") / "bin" / "openssl.exe",
                Path("OpenSSL-Win32") / "bin" / "openssl.exe",
            ):
                candidate = Path(root) / relative
                if candidate.is_file():
                    return str(candidate)
    return None


def execution_authorization_subject(
    cross_host: dict[str, Any],
    *,
    public_key_digest: str,
    issued_at: datetime,
    expires_at: datetime,
) -> dict[str, Any]:
    return {
        "schema_version": EXECUTION_AUTHORIZATION_SCHEMA,
        "action_digest": execution_action_digest(cross_host),
        "target_node": str(cross_host.get("target_node") or ""),
        "request_id": str(cross_host.get("request_id") or ""),
        "issued_at": issued_at.astimezone(timezone.utc).isoformat().replace("+00:00", "Z"),
        "expires_at": expires_at.astimezone(timezone.utc).isoformat().replace("+00:00", "Z"),
        "signing_public_key_sha256": public_key_digest,
    }


def authorize_control_task(
    task: dict[str, Any],
    *,
    private_key_path: Path,
    public_key_path: Path,
    ttl_seconds: int = 300,
    issued_at: datetime | None = None,
    openssl_path: str | None = None,
    trusted_key_sha256s: tuple[str, ...] = (DEFAULT_EXECUTION_KEY_SHA256,),
) -> dict[str, Any]:
    if ttl_seconds <= 0 or ttl_seconds > EXECUTION_AUTHORIZATION_MAX_TTL_SECONDS:
        raise ValueError("execution authorization ttl must be between 1 and 900 seconds")
    authorized = json.loads(json.dumps(task))
    cross_host = authorized.get("ao2_cross_host")
    if not isinstance(cross_host, dict):
        raise ValueError("control task is missing ao2_cross_host")
    if not isinstance(cross_host.get("parameters"), dict):
        raise ValueError("control task parameters must be an object")
    cross_host.pop("execution_authorization", None)
    public_key_pem = public_key_path.read_text(encoding="utf-8")
    normalized_public_key = canonical_public_key_bytes(public_key_pem)
    if not (
        normalized_public_key.startswith(b"-----BEGIN PUBLIC KEY-----\n")
        and normalized_public_key.endswith(b"\n-----END PUBLIC KEY-----\n")
        and b"PRIVATE KEY" not in normalized_public_key
    ):
        raise ValueError("execution authorization requires a public key PEM")
    key_digest = public_key_sha256(public_key_pem)
    issued = (issued_at or datetime.now(timezone.utc)).astimezone(timezone.utc)
    subject = execution_authorization_subject(
        cross_host,
        public_key_digest=key_digest,
        issued_at=issued,
        expires_at=issued + timedelta(seconds=ttl_seconds),
    )
    openssl = openssl_path or resolve_openssl()
    if not openssl:
        raise RuntimeError("openssl is required to sign an execution authorization")
    with tempfile.TemporaryDirectory(prefix="ao2-task-authorization-") as temp_dir:
        root = Path(temp_dir)
        derived_public_key_path = root / "derived-public.pem"
        subject_path = root / "subject.json"
        signature_path = root / "subject.sig"
        derive_result = subprocess.run(
            [
                openssl,
                "pkey",
                "-in",
                str(private_key_path),
                "-pubout",
                "-out",
                str(derived_public_key_path),
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=15,
            check=False,
        )
        if derive_result.returncode != 0 or not derived_public_key_path.is_file():
            raise ValueError("openssl could not derive a public key from the private key")
        derived_public_key = canonical_public_key_bytes(
            derived_public_key_path.read_text(encoding="utf-8")
        )
        if derived_public_key != normalized_public_key:
            raise ValueError("execution authorization public key does not match private key")
        trusted = {
            value.lower()
            for value in trusted_key_sha256s
            if re.fullmatch(r"[0-9a-fA-F]{64}", value)
        }
        if key_digest not in trusted:
            raise ValueError("execution authorization public key is not trusted")
        subject_path.write_bytes(canonical_json_bytes(subject))
        result = subprocess.run(
            [
                openssl,
                "dgst",
                "-sha256",
                "-sign",
                str(private_key_path),
                "-out",
                str(signature_path),
                str(subject_path),
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=15,
            check=False,
        )
        if result.returncode != 0 or not signature_path.is_file():
            raise RuntimeError("openssl failed to sign the execution authorization")
        signature = base64.b64encode(signature_path.read_bytes()).decode("ascii")
    cross_host["execution_authorization"] = {
        "subject": subject,
        "signature_algorithm": EXECUTION_AUTHORIZATION_ALGORITHM,
        "signature_base64": signature,
        "public_key_pem": normalized_public_key.decode("ascii"),
    }
    return authorized


def verify_execution_authorization(
    cross_host: dict[str, Any],
    *,
    trusted_key_sha256s: tuple[str, ...],
    state_root: Path,
    now: datetime | None = None,
) -> tuple[bool, str]:
    authorization = cross_host.get("execution_authorization")
    if not isinstance(authorization, dict):
        return False, "execution_authorization_missing"
    if set(authorization) != {
        "subject",
        "signature_algorithm",
        "signature_base64",
        "public_key_pem",
    }:
        return False, "execution_authorization_malformed"
    subject = authorization.get("subject")
    if not isinstance(subject, dict) or set(subject) != {
        "schema_version",
        "action_digest",
        "target_node",
        "request_id",
        "issued_at",
        "expires_at",
        "signing_public_key_sha256",
    }:
        return False, "execution_authorization_malformed"
    if subject.get("schema_version") != EXECUTION_AUTHORIZATION_SCHEMA:
        return False, "execution_authorization_schema_mismatch"
    if authorization.get("signature_algorithm") != EXECUTION_AUTHORIZATION_ALGORITHM:
        return False, "execution_authorization_algorithm_mismatch"
    public_key_pem = authorization.get("public_key_pem")
    signature_base64 = authorization.get("signature_base64")
    if not isinstance(public_key_pem, str) or len(public_key_pem.encode("utf-8")) > 8192:
        return False, "execution_authorization_malformed"
    if not isinstance(signature_base64, str) or len(signature_base64) > 4096:
        return False, "execution_authorization_malformed"
    try:
        public_key_bytes = canonical_public_key_bytes(public_key_pem)
        signature_bytes = base64.b64decode(signature_base64, validate=True)
    except (UnicodeError, ValueError):
        return False, "execution_authorization_malformed"
    key_digest = hashlib.sha256(public_key_bytes).hexdigest()
    trusted = {value.lower() for value in trusted_key_sha256s if re.fullmatch(r"[0-9a-fA-F]{64}", value)}
    if key_digest not in trusted or subject.get("signing_public_key_sha256") != key_digest:
        return False, "execution_authorization_untrusted_key"
    if subject.get("target_node") != cross_host.get("target_node"):
        return False, "execution_authorization_target_mismatch"
    if subject.get("request_id") != cross_host.get("request_id"):
        return False, "execution_authorization_request_mismatch"
    if subject.get("action_digest") != execution_action_digest(cross_host):
        return False, "execution_authorization_action_digest_mismatch"
    issued_at = parse_utc_timestamp(subject.get("issued_at"))
    expires_at = parse_utc_timestamp(subject.get("expires_at"))
    if not issued_at or not expires_at or expires_at <= issued_at:
        return False, "execution_authorization_timestamp_invalid"
    if (expires_at - issued_at).total_seconds() > EXECUTION_AUTHORIZATION_MAX_TTL_SECONDS:
        return False, "execution_authorization_ttl_exceeded"
    current = (now or datetime.now(timezone.utc)).astimezone(timezone.utc)
    if issued_at > current + timedelta(seconds=EXECUTION_AUTHORIZATION_CLOCK_SKEW_SECONDS):
        return False, "execution_authorization_not_yet_valid"
    if current > expires_at:
        return False, "execution_authorization_expired"
    openssl = resolve_openssl()
    if not openssl:
        return False, "execution_authorization_verifier_unavailable"
    verification_root = state_root / "authorization-verification"
    verification_root.mkdir(parents=True, exist_ok=True)
    try:
        with tempfile.TemporaryDirectory(prefix="verify-", dir=verification_root) as temp_dir:
            root = Path(temp_dir)
            subject_path = root / "subject.json"
            signature_path = root / "subject.sig"
            public_key_path = root / "public.pem"
            subject_path.write_bytes(canonical_json_bytes(subject))
            signature_path.write_bytes(signature_bytes)
            public_key_path.write_bytes(public_key_bytes)
            result = subprocess.run(
                [
                    openssl,
                    "dgst",
                    "-sha256",
                    "-verify",
                    str(public_key_path),
                    "-signature",
                    str(signature_path),
                    str(subject_path),
                ],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=5,
                check=False,
            )
    except (OSError, subprocess.TimeoutExpired):
        return False, "execution_authorization_verification_failed"
    if result.returncode != 0:
        return False, "execution_authorization_signature_invalid"
    return True, "execution_authorization_verified"


def redact_text(value: str) -> str:
    redacted = value
    for pattern in SECRET_PATTERNS:
        redacted = pattern.sub(lambda match: match.group(1) + "<redacted>", redacted)
    return redacted


def bound_text(value: str, limit_bytes: int) -> tuple[str, bool]:
    encoded = value.encode("utf-8", errors="replace")
    if len(encoded) <= limit_bytes:
        return value, False
    suffix = "\n...<truncated>"
    suffix_bytes = suffix.encode("utf-8")
    keep = max(limit_bytes - len(suffix_bytes), 0)
    bounded = encoded[:keep].decode("utf-8", errors="ignore") + suffix
    return bounded, True


def sanitize_output(stdout: str, stderr: str, limit_bytes: int) -> tuple[str, bool]:
    combined = "\n".join(part for part in (stdout, stderr) if part)
    return bound_text(redact_text(combined), limit_bytes)


def stderr_category(status: str, stderr: str) -> str:
    if status == "accepted":
        return "none"
    if status == "timed_out":
        return "timeout"
    if not stderr.strip():
        return "none"
    lowered = stderr.lower()
    if "permission" in lowered or "access is denied" in lowered:
        return "permission"
    if "not recognized" in lowered or "not found" in lowered or "no such file" in lowered:
        return "missing_dependency"
    return "nonzero_exit"


def terminate_process_tree(process: subprocess.Popen[str]) -> None:
    if process.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/PID", str(process.pid), "/T", "/F"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        return
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        return


def timeout_stream_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return str(value)


def join_timeout_streams(*values: str) -> str:
    return "\n".join(value for value in values if value)


def run_bounded_child(
    command: list[str],
    *,
    cwd: Path,
    timeout_seconds: float,
    output_limit_bytes: int = DEFAULT_OUTPUT_LIMIT_BYTES,
    env: dict[str, str] | None = None,
) -> dict[str, Any]:
    started = time.monotonic()
    popen_kwargs: dict[str, Any] = {
        "cwd": str(cwd),
        "text": True,
        "stdout": subprocess.PIPE,
        "stderr": subprocess.PIPE,
    }
    if env:
        child_env = os.environ.copy()
        child_env.update(env)
        popen_kwargs["env"] = child_env
    if os.name == "nt":
        popen_kwargs["creationflags"] = getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
    else:
        popen_kwargs["start_new_session"] = True

    try:
        process = subprocess.Popen(command, **popen_kwargs)
    except FileNotFoundError as exc:
        duration = round(time.monotonic() - started, 3)
        executable = command[0] if command else str(exc.filename or "")
        stderr = f"Executable not found: {executable}"
        output, truncated = sanitize_output("", stderr, output_limit_bytes)
        return {
            "status": "failed",
            "exit_code": None,
            "timed_out": False,
            "duration_seconds": duration,
            "output": output,
            "output_truncated": truncated,
            "sanitized_stderr_category": "missing_dependency",
            "command_name": Path(executable).name if executable else "",
        }
    timed_out = False
    try:
        stdout, stderr = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired as exc:
        timed_out = True
        stdout = timeout_stream_text(getattr(exc, "stdout", None) or getattr(exc, "output", None))
        stderr = timeout_stream_text(getattr(exc, "stderr", None))
        terminate_process_tree(process)
        try:
            collected_stdout, collected_stderr = process.communicate(timeout=5)
            stdout = join_timeout_streams(stdout, collected_stdout or "")
            stderr = join_timeout_streams(stderr, collected_stderr or "")
        except subprocess.TimeoutExpired as drain_exc:
            stdout = join_timeout_streams(
                stdout,
                timeout_stream_text(getattr(drain_exc, "stdout", None) or getattr(drain_exc, "output", None)),
            )
            stderr = join_timeout_streams(
                stderr,
                timeout_stream_text(getattr(drain_exc, "stderr", None)),
                "Process output collection timed out after child-process tree termination.",
            )

    duration = round(time.monotonic() - started, 3)
    output, truncated = sanitize_output(stdout or "", stderr or "", output_limit_bytes)
    if timed_out:
        status = "timed_out"
        exit_code = None
    else:
        exit_code = process.returncode
        status = "accepted" if exit_code == 0 else "failed"
    return {
        "status": status,
        "exit_code": exit_code,
        "timed_out": timed_out,
        "duration_seconds": duration,
        "output": output,
        "output_truncated": truncated,
        "sanitized_stderr_category": stderr_category(status, stderr or ""),
        "command_name": Path(command[0]).name if command else "",
    }


class WorkerState:
    LEDGER_SCHEMA = "ao2.windows-outbound-worker-ledger.v1"
    LEDGER_EVENT_SCHEMA = "ao2.windows-outbound-worker-ledger-event.v1"

    def __init__(self, state_root: Path):
        self.state_root = state_root
        self.state_root.mkdir(parents=True, exist_ok=True)
        self.ledger_path = self.state_root / "task-ledger.json"
        self.journal_path = self.state_root / "task-ledger-events.jsonl"
        self.result_outbox_dir = self.state_root / "result-outbox"
        self.result_outbox_dir.mkdir(parents=True, exist_ok=True)
        self._lock = threading.Lock()
        self._ledger = self._load()

    def _blank_ledger(self) -> dict[str, Any]:
        return {"schema_version": self.LEDGER_SCHEMA, "tasks": {}}

    def _load(self) -> dict[str, Any]:
        ledger = self._blank_ledger()
        if self.ledger_path.is_file():
            try:
                loaded = json.loads(self.ledger_path.read_text(encoding="utf-8"))
                if isinstance(loaded, dict):
                    ledger = loaded
                    ledger.setdefault("schema_version", self.LEDGER_SCHEMA)
                    ledger.setdefault("tasks", {})
            except json.JSONDecodeError:
                ledger = self._blank_ledger()
        self._replay_journal(ledger)
        return ledger

    def _replay_journal(self, ledger: dict[str, Any]) -> None:
        if not self.journal_path.is_file():
            return
        tasks = ledger.setdefault("tasks", {})
        for line in self.journal_path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(event, dict) or event.get("schema_version") != self.LEDGER_EVENT_SCHEMA:
                continue
            request_id = event.get("request_id")
            if not isinstance(request_id, str) or not request_id:
                continue
            event_type = event.get("event")
            if event_type == "claim":
                action = event.get("action")
                if not isinstance(action, str) or not action:
                    continue
                tasks.setdefault(
                    request_id,
                    {
                        "action": action,
                        "status": "in_progress",
                        "started_at": event.get("recorded_at_utc", utc_now()),
                    },
                )
            elif event_type == "complete":
                status = event.get("status")
                if not isinstance(status, str) or not status:
                    continue
                item = tasks.setdefault(request_id, {})
                item["status"] = status
                item["completed_at"] = event.get("recorded_at_utc", utc_now())

    def _save(self) -> None:
        tmp = self.ledger_path.with_suffix(".tmp")
        tmp.write_text(json.dumps(self._ledger, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        tmp.replace(self.ledger_path)

    def _append_journal_event(self, event: dict[str, Any]) -> None:
        self.state_root.mkdir(parents=True, exist_ok=True)
        payload = {
            "schema_version": self.LEDGER_EVENT_SCHEMA,
            "recorded_at_utc": utc_now(),
            **event,
        }
        with self.journal_path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(payload, sort_keys=True) + "\n")
            handle.flush()
            os.fsync(handle.fileno())

    def claim(self, request_id: str, action: str) -> bool:
        with self._lock:
            tasks = self._ledger.setdefault("tasks", {})
            if request_id in tasks:
                return False
            self._append_journal_event({"event": "claim", "request_id": request_id, "action": action})
            tasks[request_id] = {"action": action, "status": "in_progress", "started_at": utc_now()}
            self._save()
            return True

    def complete(self, request_id: str, status: str) -> None:
        with self._lock:
            tasks = self._ledger.setdefault("tasks", {})
            self._append_journal_event({"event": "complete", "request_id": request_id, "status": status})
            item = tasks.setdefault(request_id, {})
            item["status"] = status
            item["completed_at"] = utc_now()
            self._save()

    def _result_outbox_path(self, request_id: str) -> Path:
        safe = re.sub(r"[^A-Za-z0-9_.-]+", "_", request_id).strip("._-") or "request"
        digest = hashlib.sha256(request_id.encode("utf-8")).hexdigest()[:16]
        return self.result_outbox_dir / f"{safe[:120]}-{digest}.json"

    def queue_result_board(self, request_id: str, board: dict[str, Any]) -> None:
        with self._lock:
            self.result_outbox_dir.mkdir(parents=True, exist_ok=True)
            path = self._result_outbox_path(request_id)
            tmp = path.with_suffix(".tmp")
            tmp.write_text(json.dumps(board, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            tmp.replace(path)

    def pending_result_paths(self) -> list[Path]:
        return sorted(self.result_outbox_dir.glob("*.json"))

    def remove_queued_result(self, path: Path) -> None:
        try:
            path.unlink()
        except FileNotFoundError:
            return


class MemoryTransport:
    def __init__(self) -> None:
        self.posted: list[dict[str, Any]] = []

    def latest_board(self) -> dict[str, Any] | None:
        return None

    def post_board(self, board: dict[str, Any]) -> None:
        self.posted.append(board)

    def posted_results_by_request_id(self) -> dict[str, dict[str, Any]]:
        results: dict[str, dict[str, Any]] = {}
        for board in self.posted:
            for task in board.get("tasks", []):
                cross_host = task.get("ao2_cross_host") if isinstance(task, dict) else None
                if isinstance(cross_host, dict) and cross_host.get("request_id"):
                    results[str(cross_host["request_id"])] = task
        return results


class HttpTaskBoardTransport:
    def __init__(self, base_url: str, api_token: str) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_token = api_token

    def _request(self, method: str, path: str, body: bytes | None = None) -> Any:
        request = urllib.request.Request(
            self.base_url + path,
            data=body,
            method=method,
            headers={
                "Authorization": "Bearer " + self.api_token,
                "Content-Type": "application/json",
            },
        )
        with urllib.request.urlopen(request, timeout=15) as response:
            return json.load(response)

    def latest_board(self) -> dict[str, Any] | None:
        try:
            return self._request("GET", "/api/v1/ai/task-board/latest")
        except urllib.error.HTTPError as exc:
            if exc.code == 404:
                return None
            raise

    def post_board(self, board: dict[str, Any]) -> None:
        raw = json.dumps(board, separators=(",", ":"), sort_keys=True).encode("utf-8")
        self._request("POST", "/api/v1/ai/task-board", raw)


def control_task(
    *,
    request_id: str,
    action: str,
    parameters: dict[str, Any] | None = None,
    arbitrary_command_execution: bool = False,
    target_node: str = DEFAULT_NODE_ID,
) -> dict[str, Any]:
    return {
        "task_id": f"windows-control-{action}-{request_id}",
        "kind": "cross-host-control",
        "status": "proposed",
        "ao2_cross_host": {
            "schema_version": CONTROL_TASK_SCHEMA,
            "target_node": target_node,
            "request_id": request_id,
            "action": action,
            "parameters": parameters or {},
            "arbitrary_command_execution": arbitrary_command_execution,
            "created_at": utc_now(),
        },
    }


class WindowsOutboundWorker:
    def __init__(
        self,
        *,
        node_id: str,
        factory_root: Path,
        state: WorkerState,
        transport: MemoryTransport | HttpTaskBoardTransport,
        poll_interval_seconds: float = DEFAULT_POLL_INTERVAL_SECONDS,
        output_limit_bytes: int = DEFAULT_OUTPUT_LIMIT_BYTES,
        trusted_execution_key_sha256s: tuple[str, ...] = (DEFAULT_EXECUTION_KEY_SHA256,),
        execution_authorization_verifier: Any = None,
    ) -> None:
        self.node_id = node_id
        self.factory_root = factory_root
        self.state = state
        self.transport = transport
        self.poll_interval_seconds = poll_interval_seconds
        self.output_limit_bytes = output_limit_bytes
        self.trusted_execution_key_sha256s = trusted_execution_key_sha256s
        self.execution_authorization_verifier = (
            execution_authorization_verifier or verify_execution_authorization
        )
        self._threads: list[threading.Thread] = []
        self._stopped = False
        self._thread_lock = threading.Lock()

    def is_stopped(self) -> bool:
        return self._stopped

    def running_action_count(self) -> int:
        with self._thread_lock:
            self._threads = [thread for thread in self._threads if thread.is_alive()]
            return len(self._threads)

    def wait_for_idle(self, timeout_seconds: float) -> bool:
        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            if self.running_action_count() == 0:
                return True
            time.sleep(0.02)
        return self.running_action_count() == 0

    def status_result(self) -> dict[str, Any]:
        return {
            "node_id": self.node_id,
            "hostname": local_hostname(),
            "os_caption": "Microsoft Windows" if os.name == "nt" else sys.platform,
            "os_version": platform_version(),
            "factory_root": str(self.factory_root),
            "state_root": str(self.state.state_root),
            "allowed_actions": list(ALLOWLISTED_ACTIONS),
            "unsigned_observer_actions": list(UNSIGNED_OBSERVER_ACTIONS),
            "execution_authorization_required": True,
            "execution_authorization_schema": EXECUTION_AUTHORIZATION_SCHEMA,
            "execution_authorization_trusted_key_count": len(self.trusted_execution_key_sha256s),
            "worker_source_commit": repository_head(self.factory_root / "ao2"),
            "ao2_repository_worktree": repository_worktree_status(self.factory_root / "ao2"),
            "stack_qualification_profile_version": STACK_PROFILE_VERSION,
            "mac_should_probe_windows": False,
            "windows_http_endpoint": None,
            "windows_inbound_ports_opened": False,
            "running_actions": self.running_action_count(),
        }

    def accept_control_task(self, task: dict[str, Any]) -> str:
        cross_host = task.get("ao2_cross_host") if isinstance(task, dict) else None
        if not isinstance(cross_host, dict):
            return "ignored"
        if cross_host.get("schema_version") != CONTROL_TASK_SCHEMA:
            return "ignored"
        if cross_host.get("target_node") != self.node_id:
            return "ignored"

        request_id = str(cross_host.get("request_id") or "")
        action = str(cross_host.get("action") or "")
        parameters = cross_host.get("parameters") if isinstance(cross_host.get("parameters"), dict) else {}
        arbitrary = bool(cross_host.get("arbitrary_command_execution"))
        if not request_id:
            return "ignored"

        if arbitrary or action not in ALLOWLISTED_ACTIONS:
            self._post_result_best_effort(request_id, action or "unknown", {
                "status": "failed",
                "error_category": "action_not_allowlisted",
                "arbitrary_command_execution": False,
                "allowed_actions": list(ALLOWLISTED_ACTIONS),
            })
            return "rejected"

        if not isinstance(cross_host.get("parameters"), dict):
            self._post_result_best_effort(request_id, action, {
                "status": "failed",
                "error_category": "invalid_parameters",
                "arbitrary_command_execution": False,
                "execution_authorization_required": action not in UNSIGNED_OBSERVER_ACTIONS,
            })
            return "rejected"

        authorization_receipt = None
        if action not in UNSIGNED_OBSERVER_ACTIONS:
            authorized, authorization_status = self.execution_authorization_verifier(
                cross_host,
                trusted_key_sha256s=self.trusted_execution_key_sha256s,
                state_root=self.state.state_root,
            )
            if not authorized:
                self._post_result_best_effort(request_id, action, {
                    "status": "failed",
                    "error_category": authorization_status,
                    "arbitrary_command_execution": False,
                    "execution_authorization_required": True,
                })
                return "rejected"
            authorization = cross_host.get("execution_authorization")
            subject = authorization.get("subject") if isinstance(authorization, dict) else None
            if isinstance(subject, dict):
                authorization_receipt = {
                    "schema_version": EXECUTION_AUTHORIZATION_SCHEMA,
                    "status": "verified",
                    "action_digest": subject["action_digest"],
                    "target_node": subject["target_node"],
                    "request_id": subject["request_id"],
                    "issued_at": subject["issued_at"],
                    "expires_at": subject["expires_at"],
                    "signing_public_key_sha256": subject["signing_public_key_sha256"],
                }

        if not self.state.claim(request_id, action):
            return "duplicate"

        if action in {"status", "publish_capability"}:
            self._post_result_best_effort(request_id, action, self.status_result())
            self.state.complete(request_id, "accepted")
            return "completed"

        thread = threading.Thread(
            target=self._run_action_thread,
            args=(request_id, action, parameters, authorization_receipt),
            name=f"ao2-worker-{request_id}",
            daemon=True,
        )
        with self._thread_lock:
            self._threads.append(thread)
        thread.start()
        return "started"

    def _run_action_thread(
        self,
        request_id: str,
        action: str,
        parameters: dict[str, Any],
        authorization_receipt: dict[str, Any] | None,
    ) -> None:
        try:
            result = self.run_action(action, parameters, request_id=request_id)
        except Exception as exc:  # pragma: no cover - fail closed guard
            result = {
                "status": "failed",
                "error_category": "worker_exception",
                "message": redact_text(str(exc))[:500],
            }
        if authorization_receipt is not None:
            result["execution_authorization"] = authorization_receipt
        self._post_result_best_effort(request_id, action, result)
        self.state.complete(request_id, str(result.get("status", "failed")))

    def run_action(self, action: str, parameters: dict[str, Any], request_id: str = "") -> dict[str, Any]:
        if action == "timeout_fixture":
            sleep_seconds = float(parameters.get("sleep_seconds", 5))
            timeout_seconds = float(parameters.get("timeout_seconds", 0.2))
            return run_bounded_child(
                [sys.executable, "-c", f"import time; time.sleep({sleep_seconds!r})"],
                cwd=self.factory_root if self.factory_root.exists() else Path.cwd(),
                timeout_seconds=timeout_seconds,
                output_limit_bytes=self.output_limit_bytes,
            )
        if action == "ao2_doctor":
            command = ao2_doctor_command(parameters, factory_root=self.factory_root)
            timeout_seconds = float(parameters.get("timeout_seconds", DEFAULT_DOCTOR_TIMEOUT_SECONDS))
            return run_bounded_child(
                command,
                cwd=self.factory_root if self.factory_root.exists() else Path.cwd(),
                timeout_seconds=timeout_seconds,
                output_limit_bytes=self.output_limit_bytes,
            )
        if action == "sync_ao_stack":
            return self.sync_ao_stack(parameters)
        if action == "windows_stack_qualification":
            return self.windows_stack_qualification(parameters, request_id=request_id)
        return {"status": "failed", "error_category": "unimplemented_action"}

    def windows_stack_qualification(self, parameters: dict[str, Any], *, request_id: str) -> dict[str, Any]:
        invalid_keys = sorted(set(parameters) - STACK_QUALIFICATION_ALLOWED_PARAMETERS)
        forbidden_keys = sorted(set(parameters) & STACK_QUALIFICATION_FORBIDDEN_PARAMETERS)
        if invalid_keys or forbidden_keys:
            return {
                "status": "failed",
                "error_category": "unsupported_parameter",
                "unsupported_parameters": sorted(set(invalid_keys + forbidden_keys)),
            }

        mode = str(parameters.get("mode", "diagnostic"))
        if mode not in STACK_QUALIFICATION_MODES:
            return {"status": "failed", "error_category": "invalid_mode", "allowed_modes": list(STACK_QUALIFICATION_MODES)}

        timeout_value = parameters.get("timeout_seconds", DEFAULT_STACK_QUALIFICATION_TIMEOUT_SECONDS)
        try:
            timeout_seconds = float(timeout_value)
        except (TypeError, ValueError):
            return {"status": "failed", "error_category": "invalid_timeout"}
        if timeout_seconds < MIN_STACK_QUALIFICATION_TIMEOUT_SECONDS or timeout_seconds > MAX_STACK_QUALIFICATION_TIMEOUT_SECONDS:
            return {
                "status": "failed",
                "error_category": "timeout_out_of_bounds",
                "min_timeout_seconds": MIN_STACK_QUALIFICATION_TIMEOUT_SECONDS,
                "max_timeout_seconds": MAX_STACK_QUALIFICATION_TIMEOUT_SECONDS,
            }
        global_deadline_seconds = None
        if "global_deadline_seconds" in parameters:
            try:
                global_deadline_seconds = float(parameters["global_deadline_seconds"])
            except (TypeError, ValueError):
                return {"status": "failed", "error_category": "invalid_global_deadline"}
            if global_deadline_seconds <= 0:
                return {"status": "failed", "error_category": "invalid_global_deadline"}

        shard_id = str(parameters["shard_id"]) if "shard_id" in parameters else None
        checkpoint_id = str(parameters["checkpoint_id"]) if "checkpoint_id" in parameters else None
        profile_digest = str(parameters["profile_digest"]) if "profile_digest" in parameters else None
        reuse_from_request_id = (
            str(parameters["reuse_from_request_id"]) if "reuse_from_request_id" in parameters else None
        )
        reuse_profile_digest = (
            str(parameters["reuse_profile_digest"]) if "reuse_profile_digest" in parameters else None
        )
        metadata = {
            "shard_id": shard_id,
            "checkpoint_id": checkpoint_id,
            "profile_digest": profile_digest,
        }
        if reuse_from_request_id and reuse_profile_digest and profile_digest and reuse_profile_digest != profile_digest:
            return {
                "schema_version": "ao2.windows-stack-qualification-result.v1",
                "status": "failed",
                "error_category": "reuse_invalidated",
                "reuse_from_request_id": reuse_from_request_id,
                "reuse_invalidated": True,
                "reuse_profile_digest": reuse_profile_digest,
                "profile_digest": profile_digest,
                "mode": mode,
                "profile_version": STACK_PROFILE_VERSION,
                "repositories": [],
                "results": [],
                "completed_at": utc_now(),
            }

        if mode == "toolchain":
            return windows_toolchain_capability_report(
                node_id=self.node_id,
                worker_source_commit=repository_head(self.factory_root / "ao2"),
                request_id=request_id,
                factory_root=self.factory_root,
                timeout_seconds=min(timeout_seconds, 30.0),
                output_limit_bytes=self.output_limit_bytes,
            )

        repos_value = parameters.get("repositories", parameters.get("repos", list(CANONICAL_REPOSITORIES)))
        if not isinstance(repos_value, list) or not repos_value:
            return {"status": "failed", "error_category": "invalid_repository_list"}

        repositories: list[str] = []
        seen: set[str] = set()
        for item in repos_value:
            if not isinstance(item, str):
                return {"status": "failed", "error_category": "invalid_repository_name"}
            repo_name = item.strip()
            validation_error = validate_canonical_repository_name(repo_name)
            if validation_error:
                return {"status": "failed", "error_category": validation_error, "repository": repo_name}
            if repo_name in seen:
                return {"status": "failed", "error_category": "duplicate_repository", "repository": repo_name}
            seen.add(repo_name)
            repositories.append(repo_name)

        worker_source_commit = repository_head(self.factory_root / "ao2")
        results: list[dict[str, Any]] = []
        cumulative_duration_seconds = 0.0
        deadline_exceeded = False
        for repo_name in repositories:
            repo_path = self.factory_root / repo_name
            if not repository_is_beneath_factory(self.factory_root, repo_path):
                return {"status": "failed", "error_category": "repository_escape", "repository": repo_name}

            repo_head = repository_head(repo_path)
            profile = qualification_profile(repo_name, mode)
            if not (repo_path / ".git").exists():
                results.append(stack_qualification_row(
                    node_id=self.node_id,
                    worker_source_commit=worker_source_commit,
                    request_id=request_id,
                    repo_name=repo_name,
                    repo_head=repo_head,
                    profile_name=mode,
                    command_name="repository-present",
                    child={
                        "status": "failed",
                        "exit_code": None,
                        "timed_out": False,
                        "duration_seconds": 0,
                        "output": "",
                        "output_truncated": False,
                        "sanitized_stderr_category": "missing_repo",
                    },
                    output_limit_bytes=self.output_limit_bytes,
                    metadata=metadata,
                ))
                continue

            if mode == "physical_unique" and not profile:
                results.append(stack_qualification_row(
                    node_id=self.node_id,
                    worker_source_commit=worker_source_commit,
                    request_id=request_id,
                    repo_name=repo_name,
                    repo_head=repo_head,
                    profile_name=mode,
                    command_name="delegated-to-hosted-native-windows",
                    child={
                        "status": "accepted",
                        "exit_code": 0,
                        "timed_out": False,
                        "duration_seconds": 0,
                        "output": "portable Windows coverage delegated to hosted native Windows by coverage ownership contract",
                        "output_truncated": False,
                        "sanitized_stderr_category": "none",
                    },
                    output_limit_bytes=self.output_limit_bytes,
                    metadata=metadata,
                ))
                continue

            for command_spec in profile:
                command = resolve_profile_command(command_spec["argv"], factory_root=self.factory_root)
                command_env = resolve_profile_environment(command_spec.get("env", {}), factory_root=self.factory_root)
                child_kwargs: dict[str, Any] = {
                    "cwd": repo_path,
                    "timeout_seconds": timeout_seconds,
                    "output_limit_bytes": self.output_limit_bytes,
                }
                if command_env:
                    child_kwargs["env"] = command_env
                child = run_bounded_child(command, **child_kwargs)
                results.append(stack_qualification_row(
                    node_id=self.node_id,
                    worker_source_commit=worker_source_commit,
                    request_id=request_id,
                    repo_name=repo_name,
                    repo_head=repo_head,
                    profile_name=mode,
                    command_name=command_spec["name"],
                    child=child,
                    output_limit_bytes=self.output_limit_bytes,
                    metadata=metadata,
                ))
                try:
                    cumulative_duration_seconds += float(child.get("duration_seconds", 0) or 0)
                except (TypeError, ValueError):
                    cumulative_duration_seconds = global_deadline_seconds or cumulative_duration_seconds
                if global_deadline_seconds is not None and cumulative_duration_seconds > global_deadline_seconds:
                    deadline_exceeded = True
                    break
                if child.get("status") != "accepted":
                    break
            if deadline_exceeded:
                break

        status = (
            "accepted"
            if results and not deadline_exceeded and all(item.get("status") == "accepted" for item in results)
            else "failed"
        )
        result = {
            "schema_version": "ao2.windows-stack-qualification-result.v1",
            "status": status,
            "mode": mode,
            "profile_version": STACK_PROFILE_VERSION,
            "repositories": repositories,
            "results": results,
            "completed_at": utc_now(),
        }
        if deadline_exceeded:
            result["error_category"] = "global_deadline_exceeded"
        if global_deadline_seconds is not None:
            result["global_deadline_seconds"] = global_deadline_seconds
        for key, value in metadata.items():
            if value is not None:
                result[key] = value
        return result

    def sync_ao_stack(self, parameters: dict[str, Any]) -> dict[str, Any]:
        repos = parameters.get("repos")
        if not isinstance(repos, list):
            repos = []
        results = []
        for repo_name in repos[:32]:
            if not isinstance(repo_name, str) or any(part in repo_name for part in ("/", "\\", "..")):
                results.append({"repo": str(repo_name), "status": "failed", "error_category": "invalid_repo_name"})
                continue
            repo = self.factory_root / repo_name
            if not (repo / ".git").exists():
                results.append({"repo": repo_name, "status": "failed", "error_category": "missing_repo"})
                continue
            child = run_bounded_child(
                ["git", "pull", "--ff-only", "origin", "main"],
                cwd=repo,
                timeout_seconds=float(parameters.get("timeout_seconds", DEFAULT_ACTION_TIMEOUT_SECONDS)),
                output_limit_bytes=8192,
            )
            results.append({"repo": repo_name, **child})
        status = "accepted" if all(item.get("status") == "accepted" for item in results) else "failed"
        return {"status": status, "repos": results}

    def result_board(self, request_id: str, action: str, result: dict[str, Any]) -> dict[str, Any]:
        return {
            "schema_version": TASK_BOARD_SCHEMA,
            "status": "accepted" if result.get("status") != "failed" else "ready",
            "release_objective": "Report AO2 cross-host Windows worker result back to the Mac host.",
            "source_recommendation": "Windows worker executed or blocked an allowlisted Mac-hosted AO2 task-board action.",
            "release_train": {"version": "local-cross-host", "theme": "windows-worker-result"},
            "tasks": [{
                "task_id": f"windows-worker-result-{action.replace('_', '-')}-{request_id}",
                "title": f"Windows worker result: {action}",
                "kind": "cross-host-worker-result",
                "status": "accepted" if result.get("status") != "failed" else "blocked",
                "objective": f"Return the result for request {request_id}.",
                "confidence": "high",
                "rationale": "The Windows worker posts results as task-board evidence so the Mac host can read them without SSH.",
                "required_evidence": [TASK_BOARD_SCHEMA, CONTROL_TASK_SCHEMA, WORKER_RESULT_SCHEMA],
                "stop_conditions": [
                    "Stop if the result cannot be posted to the Mac control plane.",
                    "Stop if action output contains secret material.",
                    "Stop rather than execute arbitrary command text from a task payload.",
                ],
                "ao2_cross_host": {
                    "schema_version": WORKER_RESULT_SCHEMA,
                    "status": "accepted" if result.get("status") != "failed" else "failed",
                    "node_id": self.node_id,
                    "request_id": request_id,
                    "action": action,
                    "arbitrary_command_execution": False,
                    "result": result,
                    "completed_at": utc_now(),
                },
            }],
            "control_plane_readback": {
                "role": "read_only_observer",
                "requires_credentials": False,
                "can_mutate_ao2_artifacts": False,
                "can_mutate_release_metadata": False,
            },
            "trust_boundary": {"local_only": False, "stores_credentials": False, "mutates_releases": False},
        }

    def post_result(self, request_id: str, action: str, result: dict[str, Any]) -> None:
        board = self.result_board(request_id, action, result)
        self.state.queue_result_board(request_id, board)
        self.flush_result_outbox()

    def _post_result_best_effort(self, request_id: str, action: str, result: dict[str, Any]) -> None:
        try:
            self.post_result(request_id, action, result)
        except Exception as exc:
            sys.stderr.write(
                "result_publish_pending="
                f"{request_id} error={type(exc).__name__}: {redact_text(str(exc))[:300]}\n"
            )

    def flush_result_outbox(self) -> None:
        for path in self.state.pending_result_paths():
            try:
                board = json.loads(path.read_text(encoding="utf-8"))
            except json.JSONDecodeError:
                continue
            self.transport.post_board(board)
            self.state.remove_queued_result(path)

    def poll_once(self) -> str:
        self.flush_result_outbox()
        board = self.transport.latest_board()
        if not board:
            return "no_board"
        tasks = board.get("tasks")
        if not isinstance(tasks, list):
            return "malformed_task_board"
        if len(tasks) > MAX_CONTROL_TASKS_PER_BOARD:
            return "board_task_limit_exceeded"
        request_ids: set[str] = set()
        authorization_count = 0
        for task in tasks:
            cross_host = task.get("ao2_cross_host") if isinstance(task, dict) else None
            if (
                not isinstance(cross_host, dict)
                or cross_host.get("schema_version") != CONTROL_TASK_SCHEMA
                or cross_host.get("target_node") != self.node_id
            ):
                continue
            request_id = str(cross_host.get("request_id") or "")
            if request_id:
                if request_id in request_ids:
                    return "duplicate_control_request_id"
                request_ids.add(request_id)
            action = str(cross_host.get("action") or "")
            if action not in UNSIGNED_OBSERVER_ACTIONS:
                authorization_count += 1
        if authorization_count > MAX_EXECUTION_AUTHORIZATIONS_PER_POLL:
            return "authorization_budget_exceeded"
        accepted = "no_control_task"
        for task in tasks:
            status = self.accept_control_task(task)
            if status != "ignored":
                accepted = status
        return accepted

    def run_forever(self) -> None:
        while not self._stopped:
            try:
                self.poll_once()
            except Exception as exc:
                sys.stderr.write(f"poll_error={type(exc).__name__}: {redact_text(str(exc))[:300]}\n")
            time.sleep(self.poll_interval_seconds)


def platform_version() -> str:
    if os.name == "nt":
        try:
            return subprocess.check_output(["cmd", "/c", "ver"], text=True, stderr=subprocess.DEVNULL).strip()
        except Exception:
            return "windows"
    return sys.platform


def local_hostname() -> str:
    hostname = os.environ.get("COMPUTERNAME") or os.environ.get("HOSTNAME")
    if hostname:
        return hostname
    if hasattr(os, "uname"):
        return os.uname().nodename
    return ""


def validate_canonical_repository_name(repo_name: str) -> str | None:
    if (
        not repo_name
        or repo_name != repo_name.strip()
        or repo_name in {".", ".."}
        or "/" in repo_name
        or "\\" in repo_name
        or ".." in repo_name
        or Path(repo_name).is_absolute()
        or re.match(r"^[A-Za-z]:", repo_name)
    ):
        return "invalid_repository_name"
    if repo_name in ARCHIVED_REPOSITORIES:
        return "archived_repository"
    if repo_name not in CANONICAL_REPOSITORIES:
        return "unknown_repository"
    return None


def repository_is_beneath_factory(factory_root: Path, repo_path: Path) -> bool:
    try:
        repo_path.resolve().relative_to(factory_root.resolve())
        return True
    except ValueError:
        return False


def repository_head(repo_path: Path) -> str:
    if not (repo_path / ".git").exists():
        return "unknown"
    try:
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=repo_path,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=10,
            check=False,
        )
    except Exception:
        return "unknown"
    head = result.stdout.strip()
    if result.returncode == 0 and re.fullmatch(r"[0-9a-fA-F]{40}", head):
        return head.lower()
    return "unknown"


def repository_worktree_status(repo_path: Path, *, entry_limit: int = 32) -> dict[str, Any]:
    limit = max(1, min(int(entry_limit), 32))
    if not (repo_path / ".git").exists():
        return {
            "status": "unavailable",
            "clean": False,
            "entry_count": 0,
            "entries": [],
            "entries_truncated": False,
            "error_category": "missing_repo",
        }
    try:
        result = subprocess.run(
            ["git", "status", "--porcelain=v1", "--untracked-files=all"],
            cwd=repo_path,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            timeout=10,
            check=False,
        )
    except Exception:
        return {
            "status": "unavailable",
            "clean": False,
            "entry_count": 0,
            "entries": [],
            "entries_truncated": False,
            "error_category": "status_failed",
        }
    if result.returncode != 0:
        return {
            "status": "unavailable",
            "clean": False,
            "entry_count": 0,
            "entries": [],
            "entries_truncated": False,
            "error_category": "status_failed",
        }
    raw_entries = result.stdout.splitlines()
    entries = [redact_text(entry)[:240] for entry in raw_entries[:limit]]
    clean = not raw_entries
    return {
        "status": "passed" if clean else "attention",
        "clean": clean,
        "entry_count": len(raw_entries),
        "entries": entries,
        "entries_truncated": len(raw_entries) > limit,
        "error_category": "none",
    }


def qualification_profile(repo_name: str, mode: str) -> tuple[dict[str, tuple[str, ...]], ...]:
    if mode == "diagnostic":
        return DIAGNOSTIC_PROFILE
    return WINDOWS_REPOSITORY_PROFILES[repo_name][mode]


def existing_path(path: Path) -> str | None:
    try:
        if path.is_file():
            return str(path)
    except OSError:
        return None
    return None


def standard_tool_candidates(tool_name: str) -> list[Path]:
    program_files = [os.environ.get("ProgramFiles"), os.environ.get("ProgramFiles(x86)")]
    system_drive = os.environ.get("SystemDrive", "C:") if os.name == "nt" else os.environ.get("SystemDrive")
    user_profile = os.environ.get("USERPROFILE") or os.environ.get("HOME")
    system_root = os.environ.get("SystemRoot")
    candidates: list[Path] = []

    if tool_name == "git":
        for root in program_files:
            if root:
                candidates.extend([Path(root) / "Git" / "cmd" / "git.exe", Path(root) / "Git" / "bin" / "git.exe"])
    elif tool_name == "go":
        for root in program_files:
            if root:
                candidates.append(Path(root) / "Go" / "bin" / "go.exe")
        if system_drive:
            candidates.append(Path(system_drive) / "Go" / "bin" / "go.exe")
    elif tool_name in {"cargo", "rustc"}:
        if user_profile:
            candidates.append(Path(user_profile) / ".cargo" / "bin" / f"{tool_name}.exe")
    elif tool_name == "node":
        for root in program_files:
            if root:
                candidates.append(Path(root) / "nodejs" / "node.exe")
    elif tool_name == "npm":
        for root in program_files:
            if root:
                candidates.extend([Path(root) / "nodejs" / "npm.cmd", Path(root) / "nodejs" / "npm.exe"])
    elif tool_name == "powershell":
        for root in program_files:
            if root:
                candidates.append(Path(root) / "PowerShell" / "7" / "pwsh.exe")
        if system_root:
            candidates.append(Path(system_root) / "System32" / "WindowsPowerShell" / "v1.0" / "powershell.exe")

    return candidates


def resolve_fixed_tool(tool_name: str) -> dict[str, Any]:
    if tool_name not in TOOLCHAIN_CAPABILITY_TOOLS:
        return {"tool": tool_name, "status": "failed", "path": None, "resolution_source": "unsupported_tool"}

    for command_name in FIXED_TOOL_PATH_COMMANDS[tool_name]:
        resolved = command_name if Path(command_name).is_absolute() and Path(command_name).is_file() else shutil.which(command_name)
        if resolved:
            return {
                "tool": tool_name,
                "status": "resolved",
                "path": str(Path(resolved)),
                "resolution_source": "current_python" if command_name == sys.executable else "path",
            }

    for candidate in standard_tool_candidates(tool_name):
        path = existing_path(candidate)
        if path:
            return {
                "tool": tool_name,
                "status": "resolved",
                "path": path,
                "resolution_source": "standard_location",
            }

    return {"tool": tool_name, "status": "failed", "path": None, "resolution_source": "missing"}


def fixed_tool_version_command(tool_name: str, resolved: dict[str, Any]) -> list[str]:
    executable = str(resolved.get("path") or FIXED_TOOL_PATH_COMMANDS[tool_name][0])
    return [executable, *FIXED_TOOL_VERSION_ARGS[tool_name]]


def safe_worker_environment_metadata() -> dict[str, Any]:
    path_value = os.environ.get("PATH", "")
    path_separator = ";" if os.name == "nt" else ":"
    return {
        "os_name": os.name,
        "path_entry_count": len([entry for entry in path_value.split(path_separator) if entry]),
        "has_program_files": bool(os.environ.get("ProgramFiles")),
        "has_program_files_x86": bool(os.environ.get("ProgramFiles(x86)")),
        "has_user_profile": bool(os.environ.get("USERPROFILE") or os.environ.get("HOME")),
        "has_system_root": bool(os.environ.get("SystemRoot")),
    }


def windows_toolchain_capability_report(
    *,
    node_id: str,
    worker_source_commit: str,
    request_id: str,
    factory_root: Path,
    timeout_seconds: float,
    output_limit_bytes: int,
) -> dict[str, Any]:
    cwd = factory_root if factory_root.exists() else Path.cwd()
    capabilities = []
    for tool_name in TOOLCHAIN_CAPABILITY_TOOLS:
        resolved = resolve_fixed_tool(tool_name)
        child = run_bounded_child(
            fixed_tool_version_command(tool_name, resolved),
            cwd=cwd,
            timeout_seconds=timeout_seconds,
            output_limit_bytes=min(output_limit_bytes, 4096),
        )
        capabilities.append({
            "tool": tool_name,
            "status": child.get("status"),
            "resolution_status": resolved.get("status"),
            "resolution_source": resolved.get("resolution_source"),
            "resolved_executable_path": resolved.get("path"),
            "version_command_name": tool_name,
            "exit_code": child.get("exit_code"),
            "timed_out": child.get("timed_out"),
            "duration_seconds": child.get("duration_seconds"),
            "error_category": child.get("sanitized_stderr_category"),
            "bounded_sanitized_output": child.get("output"),
            "output_truncated": child.get("output_truncated"),
        })

    return {
        "schema_version": "ao2.windows-toolchain-capability-result.v1",
        "status": "accepted",
        "mode": "toolchain",
        "profile_version": STACK_PROFILE_VERSION,
        "node_id": node_id,
        "worker_source_commit": worker_source_commit,
        "request_id": request_id,
        "toolchain_status": "ready" if all(item.get("status") == "accepted" for item in capabilities) else "attention",
        "safe_worker_environment": safe_worker_environment_metadata(),
        "toolchain_capabilities": capabilities,
        "completed_at": utc_now(),
    }


def resolve_profile_command(
    argv: tuple[str, ...],
    *,
    factory_root: Path = DEFAULT_FACTORY_ROOT,
) -> list[str]:
    powershell = resolve_fixed_tool("powershell")
    replacements = {
        "{ao2-full-target-dir}": str(factory_root / ".ao2-worker-target" / "ao2-full"),
        "{ao2-physical-target-dir}": str(factory_root / ".ao2-worker-target" / "ao2-physical-unique"),
        "{ao2-prepared-doctor}": str(prepared_ao2_doctor_binary(factory_root)),
        "{python}": sys.executable,
        "{powershell}": str(powershell.get("path") or "powershell.exe"),
    }
    command = [replacements.get(part, part) for part in argv]
    if command:
        executable_name = Path(command[0]).name.lower()
        tool_name = executable_name.removesuffix(".exe").removesuffix(".cmd")
        if tool_name in TOOLCHAIN_CAPABILITY_TOOLS:
            resolved = resolve_fixed_tool(tool_name)
            if resolved.get("status") == "resolved" and resolved.get("path"):
                command[0] = str(resolved["path"])
    return command


def resolve_profile_environment(
    env: dict[str, str],
    *,
    factory_root: Path = DEFAULT_FACTORY_ROOT,
) -> dict[str, str]:
    replacements = {
        "{ao2-full-target-dir}": str(factory_root / ".ao2-worker-target" / "ao2-full"),
    }
    return {key: replacements.get(value, value) for key, value in env.items()}


def prepared_ao2_doctor_binary(factory_root: Path = DEFAULT_FACTORY_ROOT) -> Path:
    executable = "ao2.exe" if os.name == "nt" else "ao2"
    return factory_root / ".ao2-worker-target" / "ao2-doctor" / "debug" / executable


def stack_qualification_row(
    *,
    node_id: str,
    worker_source_commit: str,
    request_id: str,
    repo_name: str,
    repo_head: str,
    profile_name: str,
    command_name: str,
    child: dict[str, Any],
    output_limit_bytes: int,
    metadata: dict[str, str | None] | None = None,
) -> dict[str, Any]:
    output, truncated = sanitize_output(str(child.get("output") or ""), "", output_limit_bytes)
    timed_out = bool(child.get("timed_out"))
    status = str(child.get("status") or "failed")
    row = {
        "node_id": node_id,
        "worker_source_commit": worker_source_commit,
        "request_id": request_id,
        "canonical_repository": repo_name,
        "repository_head": repo_head,
        "verification_profile": profile_name,
        "sanitized_command_name": command_name,
        "status": status,
        "exit_code": child.get("exit_code"),
        "timeout_state": "timed_out" if timed_out else "completed",
        "timed_out": timed_out,
        "duration_seconds": child.get("duration_seconds", 0),
        "error_category": str(child.get("sanitized_stderr_category") or ("timeout" if timed_out else "none")),
        "bounded_sanitized_output": output,
        "output_truncated": bool(child.get("output_truncated")) or truncated,
        "completed_timestamp": utc_now(),
    }
    for key, value in (metadata or {}).items():
        if value is not None:
            row[key] = value
    return row


def ao2_doctor_command(parameters: dict[str, Any], *, factory_root: Path = DEFAULT_FACTORY_ROOT) -> list[str]:
    explicit = parameters.get("ao2_path")
    candidates = []
    if isinstance(explicit, str) and explicit.strip():
        candidates.append(explicit.strip())
    found = shutil.which("ao2.exe") or shutil.which("ao2")
    if found:
        candidates.append(found)
    if os.name == "nt":
        local = Path(os.environ.get("LOCALAPPDATA", "")) / "AO2" / "bin" / "ao2.exe"
        candidates.append(str(local))
    for candidate in candidates:
        if candidate and Path(candidate).exists():
            return [candidate, "doctor", "--json"]
    manifest = factory_root / "ao2" / "Cargo.toml"
    if manifest.exists():
        cargo = resolve_fixed_tool("cargo")
        cargo_path = str(cargo.get("path") or "cargo")
        target_dir = factory_root / ".ao2-worker-target" / "ao2-doctor"
        return [
            cargo_path,
            "run",
            "--manifest-path",
            str(manifest),
            "--target-dir",
            str(target_dir),
            "-p",
            "ao2-cli",
            "--bin",
            "ao2",
            "--",
            "doctor",
            "--json",
        ]
    return ["ao2", "doctor", "--json"]


def token_from_args(args: argparse.Namespace) -> str:
    if args.api_token_env and os.environ.get(args.api_token_env):
        return os.environ[args.api_token_env]
    if args.api_token_file:
        return Path(args.api_token_file).read_text(encoding="utf-8").strip()
    raise SystemExit("set --api-token-file or --api-token-env")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--control-plane-url", required=True)
    parser.add_argument("--api-token-file")
    parser.add_argument("--api-token-env", default="AO2_CP_API_TOKEN")
    parser.add_argument("--node-id", default=os.environ.get("AO2_WINDOWS_WORKER_NODE_ID", DEFAULT_NODE_ID))
    parser.add_argument("--factory-root", type=Path, default=Path(os.environ.get("AO2_WINDOWS_FACTORY_ROOT", str(DEFAULT_FACTORY_ROOT))))
    parser.add_argument("--state-root", type=Path, default=Path(os.environ.get("AO2_WINDOWS_WORKER_STATE_ROOT", str(DEFAULT_STATE_ROOT))))
    parser.add_argument("--poll-interval", type=float, default=DEFAULT_POLL_INTERVAL_SECONDS)
    parser.add_argument("--once", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    transport = HttpTaskBoardTransport(args.control_plane_url, token_from_args(args))
    worker = WindowsOutboundWorker(
        node_id=args.node_id,
        factory_root=args.factory_root,
        state=WorkerState(args.state_root),
        transport=transport,
        poll_interval_seconds=args.poll_interval,
    )
    if args.once:
        print(f"poll_once={worker.poll_once()}")
        return 0
    worker.run_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
