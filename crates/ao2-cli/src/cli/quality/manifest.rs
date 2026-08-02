use anyhow::{bail, Context, Result};
use ao2_policy::secret_redaction_count;
use globset::Glob;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path};
use std::process::Command;

const MANIFEST_NAME: &str = "ao-quality-gates.json";
const MANIFEST_SCHEMA: &str = "ao.quality-gates.v1";
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_STEPS_PER_LEVEL: usize = 128;
const MAX_ARGUMENTS_PER_STEP: usize = 256;
const MAX_ARGUMENT_BYTES: usize = 16 * 1024;
const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "cmd",
    "cmd.exe",
    "powershell",
    "pwsh",
];
const PROVIDER_PROGRAMS: &[&str] = &["codex", "claude", "openai"];

#[derive(Debug)]
pub(super) struct LoadedManifest {
    pub manifest: QualityManifest,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QualityManifest {
    schema_version: String,
    pub repository: String,
    lifecycle: String,
    supported_platforms: Vec<String>,
    required_tools: Vec<String>,
    generated_paths: Vec<String>,
    protected_paths: Vec<String>,
    compatibility: Compatibility,
    pub evidence: Evidence,
    pub levels: Levels,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Compatibility {
    minimum_consumer_version: String,
    owner: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Evidence {
    public_safe: bool,
    pub local_artifact_root: String,
    pub maximum_result_bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Levels {
    pub commit: LevelContract,
    pub push: LevelContract,
    pub full: LevelContract,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LevelContract {
    pub snapshot: String,
    pub maximum_duration_seconds: u64,
    pub network_allowed: bool,
    pub mutates_source: bool,
    pub steps: Vec<StepContract>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StepContract {
    pub id: String,
    pub argv: Vec<String>,
    pub timeout_seconds: u64,
    pub path_triggers: Vec<String>,
}

struct UniqueJson(serde_json::Value);

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor)
    }
}

struct UniqueJsonVisitor;

impl<'de> Visitor<'de> for UniqueJsonVisitor {
    type Value = UniqueJson;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .map(UniqueJson)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::String(value.to_string())))
    }

    fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::String(value)))
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::Null))
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(UniqueJson(serde_json::Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        UniqueJson::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueJson>()? {
            values.push(value.0);
        }
        Ok(UniqueJson(serde_json::Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some(key) = object.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key {key:?}")));
            }
            values.insert(key, object.next_value::<UniqueJson>()?.0);
        }
        Ok(UniqueJson(serde_json::Value::Object(values)))
    }
}

pub(super) fn load_manifest(target: &Path, requested: &Path) -> Result<LoadedManifest> {
    let expected = target.join(MANIFEST_NAME);
    let metadata = fs::symlink_metadata(requested).map_err(|error| {
        anyhow::anyhow!(
            "[MANIFEST_MISSING] cannot inspect {}: {error}",
            requested.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        bail!("[MANIFEST_SYMLINK] manifest must not be a symlink");
    }
    if !metadata.is_file() {
        bail!("[MANIFEST_REGULAR_FILE_REQUIRED] manifest must be a regular file");
    }
    if metadata.len() > MAX_MANIFEST_BYTES {
        bail!("[MANIFEST_SIZE_LIMIT] manifest exceeds {MAX_MANIFEST_BYTES} bytes");
    }
    let actual = requested
        .canonicalize()
        .context("[MANIFEST_PATH_INVALID] resolve manifest")?;
    let expected = expected
        .canonicalize()
        .context("[MANIFEST_PATH_INVALID] resolve root manifest")?;
    if actual != expected {
        bail!("[MANIFEST_PATH_INVALID] manifest must be the repository-root {MANIFEST_NAME}");
    }
    let bytes = fs::read(&actual).context("[MANIFEST_READ_FAILED] read manifest")?;
    let text =
        std::str::from_utf8(&bytes).context("[MANIFEST_UTF8_REQUIRED] manifest must be UTF-8")?;
    if secret_redaction_count(text) != 0 {
        bail!("[MANIFEST_SECRET_MATERIAL_FORBIDDEN] manifest contains secret-like material");
    }
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    let unique = UniqueJson::deserialize(&mut deserializer)
        .context("[MANIFEST_DUPLICATE_KEY] manifest is not duplicate-free JSON")?;
    deserializer
        .end()
        .context("[MANIFEST_TRAILING_JSON] manifest contains trailing JSON")?;
    let manifest: QualityManifest = serde_json::from_value(unique.0)
        .context("[MANIFEST_CONTRACT_INVALID] manifest does not match the typed contract")?;
    validate_manifest(&manifest)?;
    let repository = repository_identity(target)?;
    if manifest.repository != repository {
        bail!(
            "[MANIFEST_REPOSITORY_MISMATCH] manifest repository {:?} does not match target {:?}",
            manifest.repository,
            repository
        );
    }
    Ok(LoadedManifest {
        manifest,
        sha256: format!("sha256:{:x}", Sha256::digest(bytes)),
    })
}

fn validate_manifest(manifest: &QualityManifest) -> Result<()> {
    let mut errors = Vec::new();
    if manifest.schema_version != MANIFEST_SCHEMA {
        errors.push(format!(
            "[MANIFEST_SCHEMA_UNSUPPORTED] expected {MANIFEST_SCHEMA}"
        ));
    }
    if !bounded_identifier(&manifest.repository) {
        errors.push(
            "[MANIFEST_REPOSITORY_INVALID] repository must be a bounded identifier".to_string(),
        );
    }
    if !matches!(
        manifest.lifecycle.as_str(),
        "active_hosted" | "active_local_only"
    ) {
        errors.push("[MANIFEST_LIFECYCLE_INVALID] lifecycle is unsupported".to_string());
    }
    validate_unique_strings(
        "MANIFEST_PLATFORMS_INVALID",
        &manifest.supported_platforms,
        &["linux", "macos", "windows"],
        &mut errors,
    );
    if manifest.required_tools.is_empty()
        || manifest.required_tools.len() > 64
        || manifest
            .required_tools
            .iter()
            .any(|tool| !bounded_argument(tool))
        || !all_unique(&manifest.required_tools)
    {
        errors.push(
            "[MANIFEST_TOOLS_INVALID] required_tools must be unique bounded strings".to_string(),
        );
    }
    for pattern in manifest
        .generated_paths
        .iter()
        .chain(manifest.protected_paths.iter())
        .chain(std::iter::once(&manifest.evidence.local_artifact_root))
    {
        validate_pattern(pattern, &mut errors);
    }
    if manifest.compatibility.minimum_consumer_version != "1.0.0" {
        errors.push(
            "[MANIFEST_COMPATIBILITY_UNSUPPORTED] minimum consumer must be 1.0.0".to_string(),
        );
    }
    if manifest.compatibility.owner != manifest.repository {
        errors.push("[MANIFEST_COMMAND_OWNER_MISMATCH] owner must match repository".to_string());
    }
    if !manifest.evidence.public_safe {
        errors.push("[EVIDENCE_PUBLIC_SAFE_REQUIRED] public_safe must be true".to_string());
    }
    if !(1..=1024 * 1024).contains(&manifest.evidence.maximum_result_bytes) {
        errors.push(
            "[EVIDENCE_SIZE_LIMIT_INVALID] result size limit is outside contract".to_string(),
        );
    }
    if manifest.evidence.maximum_result_bytes < 4096 {
        errors.push(
            "[EVIDENCE_SIZE_LIMIT_TOO_SMALL] result size limit must be at least 4096 bytes"
                .to_string(),
        );
    }
    if manifest
        .evidence
        .local_artifact_root
        .contains(['*', '?', '[', ']', '{', '}'])
    {
        errors.push(
            "[EVIDENCE_ARTIFACT_ROOT_INVALID] local_artifact_root must be a literal path"
                .to_string(),
        );
    }
    validate_level(
        "commit",
        "staged_tree",
        10,
        &manifest.levels.commit,
        &mut errors,
    );
    validate_level(
        "push",
        "outgoing_commits",
        120,
        &manifest.levels.push,
        &mut errors,
    );
    validate_level(
        "full",
        "source_head",
        u64::MAX,
        &manifest.levels.full,
        &mut errors,
    );
    if errors.is_empty() {
        Ok(())
    } else {
        bail!(errors.join("\n"))
    }
}

fn validate_level(
    name: &str,
    snapshot: &str,
    maximum_seconds: u64,
    level: &LevelContract,
    errors: &mut Vec<String>,
) {
    if level.snapshot != snapshot {
        errors.push(format!(
            "[LEVEL_SNAPSHOT_MISMATCH] {name} must use {snapshot}"
        ));
    }
    if level.maximum_duration_seconds == 0 || level.maximum_duration_seconds > maximum_seconds {
        errors.push(format!(
            "[FAST_GATE_DURATION_EXCEEDED] {name} duration is invalid"
        ));
    }
    if name != "full" && level.network_allowed {
        errors.push(format!(
            "[FAST_GATE_NETWORK_FORBIDDEN] {name} must disable network"
        ));
    }
    if name != "full" && level.mutates_source {
        errors.push(format!(
            "[FAST_GATE_MUTATION_FORBIDDEN] {name} must not mutate source"
        ));
    }
    if level.steps.is_empty() || level.steps.len() > MAX_STEPS_PER_LEVEL {
        errors.push(format!(
            "[LEVEL_STEPS_INVALID] {name} has an invalid step count"
        ));
        return;
    }
    let mut ids = HashSet::new();
    for (index, step) in level.steps.iter().enumerate() {
        if !bounded_identifier(&step.id) {
            errors.push(format!(
                "[STEP_ID_INVALID] {name}.steps[{index}] id is invalid"
            ));
        } else if !ids.insert(&step.id) {
            errors.push(format!(
                "[STEP_ID_DUPLICATE] {name} step id {:?} repeats",
                step.id
            ));
        }
        if step.argv.is_empty()
            || step.argv.len() > MAX_ARGUMENTS_PER_STEP
            || step.argv.iter().any(|argument| !bounded_argument(argument))
        {
            errors.push(format!(
                "[STEP_ARGV_REQUIRED] {name}.steps[{index}] argv is invalid"
            ));
        } else {
            validate_argv(name, index, level.network_allowed, &step.argv, errors);
        }
        if step.timeout_seconds == 0 || step.timeout_seconds > level.maximum_duration_seconds {
            errors.push(format!(
                "[STEP_TIMEOUT_INVALID] {name}.steps[{index}] timeout is invalid"
            ));
        }
        if step.path_triggers.is_empty() || step.path_triggers.len() > 128 {
            errors.push(format!(
                "[PATH_PATTERN_UNSAFE] {name}.steps[{index}] requires path triggers"
            ));
        }
        for pattern in &step.path_triggers {
            validate_pattern(pattern, errors);
        }
    }
}

fn validate_argv(
    level: &str,
    index: usize,
    network_allowed: bool,
    argv: &[String],
    errors: &mut Vec<String>,
) {
    let (program, args) = effective_program_and_args(argv);
    if shell_evaluation_requested(argv) {
        errors.push(format!(
            "[SHELL_EVALUATION_FORBIDDEN] {level}.steps[{index}] requests evaluated command text"
        ));
    }
    if PROVIDER_PROGRAMS.contains(&program.as_str()) {
        errors.push(format!(
            "[PROVIDER_COMMAND_FORBIDDEN] {level}.steps[{index}] invokes a provider"
        ));
    }
    if !network_allowed && network_command_requested(program.as_str(), args) {
        errors.push(format!("[NETWORK_COMMAND_FORBIDDEN] {level}.steps[{index}] invokes a network-capable operation"));
    }
}

fn shell_evaluation_requested(argv: &[String]) -> bool {
    let (program, args) = effective_program_and_args(argv);
    (SHELLS.contains(&program.as_str())
        && args.iter().any(|arg| {
            matches!(
                arg.to_ascii_lowercase().as_str(),
                "-c" | "/c" | "-command" | "--command"
            )
        }))
        || (matches!(program.as_str(), "python" | "python3") && args.iter().any(|arg| arg == "-c"))
        || (matches!(program.as_str(), "node" | "perl" | "ruby")
            && args.iter().any(|arg| arg == "-e"))
}

fn effective_program_and_args(argv: &[String]) -> (String, &[String]) {
    let mut index = 0;
    if executable_name(&argv[0]) == "env" {
        index = 1;
        while index < argv.len() && (argv[index].starts_with('-') || argv[index].contains('=')) {
            index += 1;
        }
    }
    if index >= argv.len() {
        return (String::new(), &[]);
    }
    (executable_name(&argv[index]), &argv[index + 1..])
}

fn network_command_requested(program: &str, args: &[String]) -> bool {
    if matches!(program, "curl" | "wget" | "ssh" | "scp" | "sftp" | "gh") {
        return true;
    }
    let subcommand = if program == "git" {
        git_subcommand(args)
    } else {
        args.iter()
            .find(|arg| !arg.starts_with('-'))
            .map(String::as_str)
    };
    matches!(
        (program, subcommand),
        (
            "git",
            Some("clone" | "fetch" | "pull" | "push" | "ls-remote")
        ) | (
            "cargo",
            Some("fetch" | "install" | "login" | "publish" | "search")
        ) | ("go", Some("get" | "install"))
            | ("pip" | "pip3", Some("install"))
    )
}

fn git_subcommand(args: &[String]) -> Option<&str> {
    let mut index = 0;
    while index < args.len() {
        let value = args[index].as_str();
        if matches!(
            value,
            "-c" | "-C" | "--git-dir" | "--work-tree" | "--namespace" | "--config-env"
        ) {
            index += 2;
        } else if value.starts_with('-') {
            index += 1;
        } else {
            return Some(value);
        }
    }
    None
}

fn executable_name(value: &str) -> String {
    value
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase()
}

fn validate_pattern(pattern: &str, errors: &mut Vec<String>) {
    if !safe_relative(pattern) || Glob::new(pattern).is_err() {
        errors.push(format!(
            "[PATH_PATTERN_UNSAFE] unsafe path pattern {pattern:?}"
        ));
    }
}

fn safe_relative(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 4096
        || value.contains(['\0', '\\'])
        || Path::new(value).is_absolute()
        || (value.len() >= 2 && value.as_bytes()[1] == b':')
    {
        return false;
    }
    !Path::new(value).components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

fn bounded_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn bounded_argument(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ARGUMENT_BYTES && !value.contains('\0')
}

fn all_unique(values: &[String]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn validate_unique_strings(
    code: &str,
    values: &[String],
    allowed: &[&str],
    errors: &mut Vec<String>,
) {
    if values.is_empty()
        || !all_unique(values)
        || values
            .iter()
            .any(|value| !allowed.contains(&value.as_str()))
    {
        errors.push(format!(
            "[{code}] values are empty, duplicated, or unsupported"
        ));
    }
}

fn repository_identity(target: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(target)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .context("[MANIFEST_REPOSITORY_IDENTITY_UNAVAILABLE] read origin URL")?;
    if !output.status.success() {
        bail!("[MANIFEST_REPOSITORY_IDENTITY_UNAVAILABLE] remote.origin.url is required");
    }
    let url = std::str::from_utf8(&output.stdout)
        .context("[MANIFEST_REPOSITORY_IDENTITY_INVALID] origin URL is not UTF-8")?
        .trim()
        .trim_end_matches('/');
    let tail = url.rsplit(['/', ':']).next().unwrap_or(url);
    let name = tail.strip_suffix(".git").unwrap_or(tail);
    if !bounded_identifier(name) {
        bail!("[MANIFEST_REPOSITORY_IDENTITY_INVALID] origin repository name is invalid");
    }
    Ok(name.to_string())
}
