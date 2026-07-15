// Emits VERGEN_GIT_SHA so orchestrator.rs can stamp the real git SHA into
// Provenance.engine_sha via option_env!. Release packaging passes
// AO2_BUILD_GIT_COMMIT explicitly, which avoids a build-time libgit2
// dependency in cross-platform package builders.
use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=AO2_BUILD_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");

    if let Some(sha) = env::var("AO2_BUILD_GIT_COMMIT")
        .ok()
        .or_else(|| env::var("GITHUB_SHA").ok())
        .or_else(git_head)
        .filter(|value| is_sha1(value))
    {
        println!("cargo:rustc-env=VERGEN_GIT_SHA={sha}");
    }
}

fn git_head() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
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
