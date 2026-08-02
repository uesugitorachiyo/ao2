use super::{quality_check, QualityHook, QualityHooksCommand, QualityLevel};
use crate::cli_util::atomic_write_text;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

const HOOK_VERSION: &str = "v1";
const MAX_HOOK_BYTES: u64 = 4096;
const MAX_PUSH_INPUT_BYTES: u64 = 64 * 1024;
const ZERO_OID: &str = "0000000000000000000000000000000000000000";
const PRE_COMMIT: &str = "#!/bin/sh\n# ao2-quality-hook:v1\nexec ao2 quality hook-run commit\n";
const PRE_PUSH: &str = "#!/bin/sh\n# ao2-quality-hook:v1\nexec ao2 quality hook-run push\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum HookState {
    Absent,
    Current,
    Stale,
    Unmanaged,
    Unsafe,
}

#[derive(Debug, Serialize)]
struct HookDiagnostic {
    name: &'static str,
    state: HookState,
    path: PathBuf,
    expected_version: &'static str,
}

#[derive(Debug, Serialize)]
struct HooksStatus {
    schema_version: &'static str,
    status: &'static str,
    repository: String,
    target: PathBuf,
    manifest_sha256: String,
    configuration: &'static str,
    hooks_directory: Option<PathBuf>,
    hooks: Vec<HookDiagnostic>,
    optional: bool,
    gate_logic_embedded: bool,
    source_mutation: bool,
    network_access: bool,
    provider_calls: u64,
}

pub(super) fn hooks(command: QualityHooksCommand) -> Result<()> {
    match command {
        QualityHooksCommand::Install { target, json } => install(target, json),
        QualityHooksCommand::Status { target, json } => status(target, json),
    }
}

pub(super) fn hook_run(hook: QualityHook, target: PathBuf) -> Result<()> {
    match hook {
        QualityHook::Commit => quality_check(QualityLevel::Commit, target, None, None, None, false),
        QualityHook::Push => run_push_hook(target),
    }
}

fn status(target: PathBuf, json: bool) -> Result<()> {
    let report = status_report(target)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("quality hooks: {}", report.status);
        for hook in &report.hooks {
            println!("{}: {:?}", hook.name, hook.state);
        }
    }
    Ok(())
}

fn install(target: PathBuf, json: bool) -> Result<()> {
    let report = status_report(target)?;
    if report.configuration != "default_hooks_path" {
        bail!("[HOOKS_PATH_CUSTOM] refusing to modify a custom or unsafe Git hooks path");
    }
    for hook in &report.hooks {
        match hook.state {
            HookState::Unsafe => bail!(
                "[HOOK_UNSAFE] refusing to replace unsafe hook {}",
                hook.name
            ),
            HookState::Unmanaged => {
                bail!(
                    "[HOOK_UNMANAGED] refusing to replace unmanaged hook {}",
                    hook.name
                )
            }
            HookState::Absent | HookState::Current | HookState::Stale => {}
        }
    }
    let hooks_dir = report
        .hooks_directory
        .as_ref()
        .context("[HOOKS_PATH_INVALID] missing default hooks directory")?;
    fs::create_dir_all(hooks_dir).context("[HOOK_INSTALL_FAILED] create hooks directory")?;
    let mut changed = Vec::new();
    for (name, body) in [("pre-commit", PRE_COMMIT), ("pre-push", PRE_PUSH)] {
        let diagnostic = report
            .hooks
            .iter()
            .find(|hook| hook.name == name)
            .context("[HOOK_INSTALL_FAILED] missing hook diagnostic")?;
        if diagnostic.state == HookState::Current {
            continue;
        }
        atomic_write_text(&diagnostic.path, body)
            .with_context(|| format!("[HOOK_INSTALL_FAILED] write {name}"))?;
        make_executable(&diagnostic.path)?;
        changed.push(name);
    }
    let status = if changed.is_empty() {
        "current"
    } else {
        "installed"
    };
    let output = serde_json::json!({
        "schema_version": "ao2.quality-hooks-install.v1",
        "status": status,
        "repository": report.repository,
        "target": report.target,
        "manifest_sha256": report.manifest_sha256,
        "hook_version": HOOK_VERSION,
        "changed_hooks": changed,
        "optional": true,
        "gate_logic_embedded": false,
        "source_mutation": false,
        "network_access": false,
        "provider_calls": 0
    });
    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("quality hooks: {status}");
    }
    Ok(())
}

fn status_report(target: PathBuf) -> Result<HooksStatus> {
    let target = target
        .canonicalize()
        .with_context(|| format!("[TARGET_INVALID] cannot resolve {}", target.display()))?;
    let loaded = super::load_manifest(&target, &target.join("ao-quality-gates.json"))?;
    if custom_hooks_path(&target)?.is_some() {
        return Ok(HooksStatus {
            schema_version: "ao2.quality-hooks-status.v1",
            status: "attention",
            repository: loaded.manifest.repository,
            target,
            manifest_sha256: loaded.sha256,
            configuration: "custom_hooks_path_unsupported",
            hooks_directory: None,
            hooks: Vec::new(),
            optional: true,
            gate_logic_embedded: false,
            source_mutation: false,
            network_access: false,
            provider_calls: 0,
        });
    }
    let hooks_dir = default_hooks_dir(&target)?;
    let hooks_dir_safe = hooks_dir
        .symlink_metadata()
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or_else(|error| error.kind() == io::ErrorKind::NotFound);
    if !hooks_dir_safe {
        return Ok(HooksStatus {
            schema_version: "ao2.quality-hooks-status.v1",
            status: "attention",
            repository: loaded.manifest.repository,
            target,
            manifest_sha256: loaded.sha256,
            configuration: "unsafe_hooks_directory",
            hooks_directory: Some(hooks_dir),
            hooks: Vec::new(),
            optional: true,
            gate_logic_embedded: false,
            source_mutation: false,
            network_access: false,
            provider_calls: 0,
        });
    }
    let hooks = vec![
        inspect_hook("pre-commit", hooks_dir.join("pre-commit"), PRE_COMMIT),
        inspect_hook("pre-push", hooks_dir.join("pre-push"), PRE_PUSH),
    ];
    let status = if hooks.iter().all(|hook| hook.state == HookState::Current) {
        "current"
    } else {
        "attention"
    };
    Ok(HooksStatus {
        schema_version: "ao2.quality-hooks-status.v1",
        status,
        repository: loaded.manifest.repository,
        target,
        manifest_sha256: loaded.sha256,
        configuration: "default_hooks_path",
        hooks_directory: Some(hooks_dir),
        hooks,
        optional: true,
        gate_logic_embedded: false,
        source_mutation: false,
        network_access: false,
        provider_calls: 0,
    })
}

fn custom_hooks_path(target: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["config", "--get", "core.hooksPath"])
        .current_dir(target)
        .output()
        .context("[GIT_COMMAND_FAILED] inspect core.hooksPath")?;
    if output.status.success() {
        let path = String::from_utf8(output.stdout)
            .context("[GIT_OUTPUT_INVALID] core.hooksPath is not UTF-8")?
            .trim()
            .to_string();
        return Ok((!path.is_empty()).then_some(path));
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    bail!(
        "[GIT_COMMAND_FAILED] inspect core.hooksPath: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

fn default_hooks_dir(target: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(target)
        .output()
        .context("[GIT_COMMAND_FAILED] resolve Git common directory")?;
    if !output.status.success() {
        bail!(
            "[GIT_COMMAND_FAILED] resolve Git common directory: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let common = String::from_utf8(output.stdout)
        .context("[GIT_OUTPUT_INVALID] Git common directory is not UTF-8")?;
    Ok(PathBuf::from(common.trim()).join("hooks"))
}

fn inspect_hook(name: &'static str, path: PathBuf, expected: &str) -> HookDiagnostic {
    let state = match path.symlink_metadata() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => HookState::Absent,
        Err(_) => HookState::Unsafe,
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            HookState::Unsafe
        }
        Ok(metadata) if metadata.len() > MAX_HOOK_BYTES => HookState::Unsafe,
        Ok(_) => match fs::read_to_string(&path) {
            Ok(body) if body == expected => HookState::Current,
            Ok(body) if body.starts_with("#!/bin/sh\n# ao2-quality-hook:v") => HookState::Stale,
            Ok(_) => HookState::Unmanaged,
            Err(_) => HookState::Unsafe,
        },
    };
    HookDiagnostic {
        name,
        state,
        path,
        expected_version: HOOK_VERSION,
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .context("[HOOK_INSTALL_FAILED] mark hook executable")
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn run_push_hook(target: PathBuf) -> Result<()> {
    let mut input = io::stdin().take(MAX_PUSH_INPUT_BYTES + 1);
    let mut bytes = Vec::new();
    input
        .read_to_end(&mut bytes)
        .context("[HOOK_PUSH_INPUT_INVALID] read pre-push input")?;
    if bytes.len() as u64 > MAX_PUSH_INPUT_BYTES {
        bail!("[HOOK_PUSH_INPUT_SIZE_LIMIT] pre-push input exceeds limit");
    }
    let text = std::str::from_utf8(&bytes)
        .context("[HOOK_PUSH_INPUT_INVALID] pre-push input must be UTF-8")?;
    let mut bases = BTreeSet::new();
    let mut local_heads = BTreeSet::new();
    let mut has_new_branch = false;
    for line in text.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 4 {
            bail!("[HOOK_PUSH_INPUT_INVALID] expected four pre-push fields");
        }
        let local_sha = fields[1];
        let remote_sha = fields[3];
        if local_sha == ZERO_OID {
            continue;
        }
        validate_oid(local_sha)?;
        validate_oid(remote_sha)?;
        local_heads.insert(local_sha.to_string());
        if remote_sha == ZERO_OID {
            has_new_branch = true;
        } else {
            bases.insert(remote_sha.to_string());
        }
    }
    if local_heads.is_empty() {
        return Ok(());
    }
    if local_heads.len() != 1 {
        bail!("[HOOK_PUSH_MULTIPLE_HEADS] one exact local head is required");
    }
    let target_head = git_text(&target, &["rev-parse", "HEAD"])?;
    if local_heads.first().map(String::as_str) != Some(target_head.as_str()) {
        bail!("[HOOK_PUSH_HEAD_MISMATCH] pushed local head does not equal repository HEAD");
    }
    if has_new_branch {
        return quality_check(QualityLevel::Full, target, None, None, None, false);
    }
    if bases.len() != 1 {
        bail!("[HOOK_PUSH_MULTIPLE_BASES] one exact remote base is required");
    }
    quality_check(
        QualityLevel::Push,
        target,
        None,
        bases.into_iter().next(),
        None,
        false,
    )
}

fn validate_oid(value: &str) -> Result<()> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("[HOOK_PUSH_INPUT_INVALID] object IDs must be 40 hexadecimal characters");
    }
    Ok(())
}

fn git_text(target: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(target)
        .output()
        .context("[GIT_COMMAND_FAILED] execute local Git command")?;
    if !output.status.success() {
        bail!(
            "[GIT_COMMAND_FAILED] {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)
        .context("[GIT_OUTPUT_INVALID] Git output is not UTF-8")?
        .trim()
        .to_string())
}
