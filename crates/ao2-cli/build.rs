use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let workspace = manifest_dir.join("../..");
    let git_head = workspace.join(".git/HEAD");
    let cargo_lock = workspace.join("Cargo.lock");

    println!("cargo:rerun-if-env-changed=AO2_BUILD_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed={}", git_head.display());
    println!("cargo:rerun-if-changed={}", cargo_lock.display());

    let git_commit = env::var("AO2_BUILD_GIT_COMMIT")
        .ok()
        .or_else(|| env::var("GITHUB_SHA").ok())
        .or_else(|| git_head_from(&workspace))
        .filter(|value| is_sha1(value))
        .unwrap_or_else(|| "unknown".to_string());
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=AO2_GIT_COMMIT={git_commit}");
    println!("cargo:rustc-env=AO2_BUILD_PROFILE={profile}");

    let lock = fs::read_to_string(&cargo_lock).expect("read Cargo.lock");
    let sbom = cyclonedx_from_lock(&lock);
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("ao2.cdx.json"), sbom).expect("write generated SBOM");
}

fn git_head_from(workspace: &PathBuf) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
        .map(|value| value.trim().to_ascii_lowercase())
}

fn is_sha1(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn cyclonedx_from_lock(lock: &str) -> String {
    let mut components = Vec::new();
    let mut name = None;
    let mut version = None;
    for line in lock.lines().chain(std::iter::once("[[package]]")) {
        if line == "[[package]]" {
            if let (Some(name), Some(version)) = (name.take(), version.take()) {
                components.push((name, version));
            }
            continue;
        }
        if name.is_none() {
            name = quoted_value(line, "name");
        }
        if version.is_none() {
            version = quoted_value(line, "version");
        }
    }
    components.sort();
    components.dedup();

    let body = components
        .iter()
        .enumerate()
        .map(|(index, (name, version))| {
            format!(
                "    {{\"type\":\"library\",\"bom-ref\":\"pkg:cargo/{}@{}?index={}\",\"name\":\"{}\",\"version\":\"{}\",\"purl\":\"pkg:cargo/{}@{}\"}}",
                json_escape(name),
                json_escape(version),
                index,
                json_escape(name),
                json_escape(version),
                json_escape(name),
                json_escape(version)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        "{{\n  \"bomFormat\": \"CycloneDX\",\n  \"specVersion\": \"1.5\",\n  \"version\": 1,\n  \"metadata\": {{\"component\": {{\"type\": \"application\", \"name\": \"ao2\", \"version\": \"{}\"}}}},\n  \"components\": [\n{}\n  ]\n}}\n",
        env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".to_string()),
        body
    )
}

fn quoted_value(line: &str, key: &str) -> Option<String> {
    let value = line.strip_prefix(&format!("{key} = \""))?;
    value.strip_suffix('"').map(ToOwned::to_owned)
}

fn json_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
