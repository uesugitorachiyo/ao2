use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::{Archive, Builder};

fn copy_fixture(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn copy_git_fixture(src: &Path, dst: &Path) {
    copy_fixture(src, dst);
    init_existing_git_repo(dst);
}

fn copy_hosted_release_fixture(dst: &Path) {
    copy_fixture(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hosted-release-publication-v1"),
        dst,
    );
}

fn init_existing_git_repo(repo: &Path) {
    assert!(Command::new("git")
        .args(["init"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["config", "user.email", "ao2-test@example.invalid"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["config", "user.name", "AO2 Test"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["config", "core.longpaths", "true"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
}

fn ao2<const N: usize>(args: [&str; N]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(args)
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .output()
        .unwrap()
}

fn ao2_with_env<const N: usize, const M: usize>(
    args: [&str; N],
    env: [(&str, &str); M],
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
    command.args(args);
    command.envs(env);
    command.env("AO2_AUTO_APPROVE_SANDBOX_PATCH", "1");
    command.env(
        "AO2_AUTO_APPROVE_SANDBOX_PATCH_APPROVER",
        "human:test-auto-approve",
    );
    command.env_remove("OPENAI_API_KEY");
    command.env_remove("ANTHROPIC_API_KEY");
    command.output().unwrap()
}

fn generate_native_signing_key(path: &Path, bits: usize) {
    let output = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(["workbench", "support-keygen", "--out"])
        .arg(path)
        .args(["--bits", &bits.to_string()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        path.is_file(),
        "native signing key exists: {}",
        path.display()
    );
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn prepend_path(bin: &Path) -> String {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = std::env::split_paths(&current).collect::<Vec<_>>();
    paths.insert(0, bin.to_path_buf());
    std::env::join_paths(paths)
        .unwrap()
        .to_string_lossy()
        .to_string()
}

fn extract_test_tar_gz(archive_path: &Path, extract_dir: &Path) {
    fs::create_dir_all(extract_dir).expect("create extract dir");
    let archive = fs::File::open(archive_path).expect("open archive");
    let decoder = GzDecoder::new(archive);
    let mut archive = Archive::new(decoder);
    archive.unpack(extract_dir).expect("extract archive");
}

fn create_test_tar_gz(stage_dir: &Path, archive_path: &Path) {
    let archive = fs::File::create(archive_path).expect("create archive");
    let encoder = GzEncoder::new(archive, Compression::default());
    let mut tar = Builder::new(encoder);
    tar.append_dir_all(".", stage_dir).expect("append archive");
    let encoder = tar.into_inner().expect("finish tar");
    encoder.finish().expect("finish gzip");
}

fn sign_test_release_archive(root: &Path, archive: &Path, provenance: &Path) {
    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", archive)
        .env("AO2_LINUX_ARCHIVE", archive)
        .env("AO2_LINUX_X86_64_ARCHIVE", archive)
        .env("AO2_WINDOWS_ARCHIVE", archive)
        .env("AO2_RELEASE_PROVENANCE_DIR", provenance)
        .env("AO2_RELEASE_PRIVATE_KEY", root.join("release-key.pem"))
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));
}

fn release_sign_command() -> Command {
    let mut command = Command::new(sh_command());
    command
        .arg("../../scripts/release-sign-provenance.sh")
        .env("AO2_BIN", env!("CARGO_BIN_EXE_ao2"));
    command
}

fn package_test_archive(root: &Path, target_label: &str) -> PathBuf {
    let out_dir = root.join(format!("dist-{target_label}"));
    let package = ao2([
        "release",
        "package",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--version",
        "9.9.9-test",
        "--binary",
        env!("CARGO_BIN_EXE_ao2"),
        "--target-label",
        target_label,
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    PathBuf::from(package_json["archive"].as_str().unwrap())
}

fn sh_command() -> PathBuf {
    ao2_adapters::posix_shell_command().unwrap_or_else(|| PathBuf::from("sh"))
}
#[test]
fn cli_release_smoke_summary_verifies_optional_windows_skip() {
    let temp = tempfile::tempdir().unwrap();
    let summary_path = temp.path().join("summary.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": false,
            "windows_native_smoke": "skipped",
            "windows_skip_reason": "windows_ssh_unreachable",
            "windows_ssh_probe_count": 1,
            "windows_wake_hosts": ["255.255.255.255", "10.0.0.255"]
        }))
        .unwrap(),
    )
    .unwrap();

    let verify = ao2([
        "release",
        "smoke-summary",
        "--summary",
        summary_path.to_str().unwrap(),
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(json["schema"], "ao2.three-os-smoke-summary-verification.v1");
    assert_eq!(json["status"], "verified");
    assert_eq!(json["summary"]["windows_native_smoke"], "skipped");
    assert!(json["reasons"].as_array().unwrap().is_empty());
}

#[test]
fn cli_release_smoke_summary_fails_when_native_windows_required() {
    let temp = tempfile::tempdir().unwrap();
    let summary_path = temp.path().join("summary.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": true,
            "windows_native_smoke": "skipped",
            "windows_skip_reason": "windows_ssh_unreachable",
            "windows_ssh_probe_count": 21,
            "windows_wake_hosts": ["255.255.255.255", "10.0.0.255"]
        }))
        .unwrap(),
    )
    .unwrap();

    let verify = ao2([
        "release",
        "smoke-summary",
        "--summary",
        summary_path.to_str().unwrap(),
        "--require-native-windows",
    ]);
    assert!(!verify.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(json["schema"], "ao2.three-os-smoke-summary-verification.v1");
    assert_eq!(json["status"], "failed");
    assert!(json["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == "native_windows_not_passed"));
}

#[test]
fn cli_release_smoke_summary_fails_on_windows_log_package_false_positive() {
    let temp = tempfile::tempdir().unwrap();
    let windows_log = temp.path().join("windows-smoke.log");
    let summary_path = temp.path().join("summary.json");
    fs::write(
        &windows_log,
        "\
windows_execute attempt=1/1
bash : The term 'bash' is not recognized as the name of a cmdlet
missing ao2-control-plane release archive: dist/ao2-control-plane-0.1.0-windows-x86_64.tar.gz
windows_release_smoke=passed
windows_native_smoke=passed
",
    )
    .unwrap();
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": true,
            "windows_native_smoke": "passed",
            "windows_log": windows_log
        }))
        .unwrap(),
    )
    .unwrap();

    let verify = ao2([
        "release",
        "smoke-summary",
        "--summary",
        summary_path.to_str().unwrap(),
        "--require-native-windows",
    ]);
    assert!(!verify.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(json["status"], "failed");
    assert!(json["reasons"].as_array().unwrap().iter().any(|reason| {
        reason["code"] == "windows_smoke_log_hard_failure"
            && reason["message"]
                .as_str()
                .unwrap()
                .contains("missing ao2-control-plane release archive")
    }));
}

#[test]
fn cli_release_smoke_summary_accepts_relocated_absolute_windows_log_sidecar() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("original");
    let relocated = temp.path().join("relocated");
    fs::create_dir_all(&original).unwrap();
    fs::create_dir_all(&relocated).unwrap();
    let original_log = original.join("windows-smoke.log");
    let original_summary = original.join("summary.enriched.json");
    let relocated_log = relocated.join("windows-smoke.log");
    let relocated_summary = relocated.join("summary.enriched.json");
    fs::write(
        &original_log,
        "\
windows_release_smoke=passed
windows_native_smoke=passed
",
    )
    .unwrap();
    fs::write(
        &original_summary,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": true,
            "windows_native_smoke": "passed",
            "windows_log": original_log
        }))
        .unwrap(),
    )
    .unwrap();
    fs::copy(&original_summary, &relocated_summary).unwrap();
    fs::copy(&original_log, &relocated_log).unwrap();
    fs::remove_dir_all(&original).unwrap();

    let verify = ao2([
        "release",
        "smoke-summary",
        "--summary",
        relocated_summary.to_str().unwrap(),
        "--require-native-windows",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(json["status"], "verified");
    assert!(json["reasons"].as_array().unwrap().is_empty());
}

#[test]
fn cli_release_smoke_summary_fails_on_windows_powershell_verifier_false_positive() {
    let temp = tempfile::tempdir().unwrap();
    let windows_log = temp.path().join("windows-smoke.log");
    let summary_path = temp.path().join("summary.json");
    fs::write(
        &windows_log,
        "\
ao2_control_plane_package=passed
ForEach-Object : Cannot bind argument to parameter 'Value' because it is null.
At C:\\ao2-public-test\\AppData\\Local\\Temp\\ao2-control-plane-three-os-smoke\\run\\target\\three-os-release-smoke\\windows-smoke\\extract\\Verify-ReleaseSupportBundle.ps1:74 char:25
windows_release_smoke=passed
windows_native_smoke=passed
",
    )
    .unwrap();
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": true,
            "windows_native_smoke": "passed",
            "windows_log": windows_log
        }))
        .unwrap(),
    )
    .unwrap();

    let verify = ao2([
        "release",
        "smoke-summary",
        "--summary",
        summary_path.to_str().unwrap(),
        "--require-native-windows",
    ]);
    assert!(!verify.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(json["status"], "failed");
    assert!(json["reasons"].as_array().unwrap().iter().any(|reason| {
        reason["code"] == "windows_smoke_log_hard_failure"
            && reason["message"]
                .as_str()
                .unwrap()
                .contains("Verify-ReleaseSupportBundle.ps1")
    }));
}

#[test]
fn cli_release_gate_verifies_archives_provenance_and_smoke_summary() {
    let temp = tempfile::tempdir().unwrap();
    let dist = temp.path().join("dist");
    let provenance = temp.path().join("dist-provenance");
    let summary_path = temp.path().join("summary.json");

    let package = ao2([
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        "9.9.9-test",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let archive = package_json["archive"].as_str().unwrap();

    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", archive)
        .env("AO2_LINUX_ARCHIVE", archive)
        .env("AO2_LINUX_X86_64_ARCHIVE", archive)
        .env("AO2_WINDOWS_ARCHIVE", archive)
        .env("AO2_RELEASE_PROVENANCE_DIR", &provenance)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));

    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": false,
            "windows_native_smoke": "skipped",
            "windows_skip_reason": "windows_ssh_unreachable",
            "obligation_gates": {
                "schema_version": "ao2.cp-obligation-gates.v1",
                "present": true,
                "count": 1,
                "gates": [{
                    "schema_version": "ao2.cp-obligation-gate-summary.v1",
                    "stage": "closure",
                    "status": "passed",
                    "verdict": "accepted",
                    "summary": {"pass": 3, "fail": 0, "unverified": 0, "waived": 0}
                }]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    // Pass --allow-unsigned-obligation-gates to keep this legacy
    // archives+provenance+smoke test focused on its original invariants;
    // signing-required-by-default is exercised in
    // release_gate_obligation_gate_signing.rs.
    let gate = ao2([
        "release",
        "gate",
        "--summary",
        summary_path.to_str().unwrap(),
        "--provenance-dir",
        provenance.to_str().unwrap(),
        "--macos-archive",
        archive,
        "--linux-archive",
        archive,
        "--linux-x86-64-archive",
        archive,
        "--windows-archive",
        archive,
        "--allow-unsigned-obligation-gates",
    ]);
    assert!(gate.status.success(), "{}", stderr(&gate));
    let json: serde_json::Value = serde_json::from_str(&stdout(&gate)).unwrap();
    assert_eq!(json["schema"], "ao2.release-gate.v1");
    assert_eq!(json["status"], "verified");
    assert_eq!(json["release"]["provenance_verified"], true);
    assert_eq!(json["release"]["archive_count"], 4);
    assert_eq!(json["smoke"]["status"], "verified");
    assert_eq!(json["obligation_gates"]["status"], "verified");
}

#[test]
fn cli_release_gate_fails_without_obligation_gate_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let dist = temp.path().join("dist");
    let provenance = temp.path().join("dist-provenance");
    let summary_path = temp.path().join("summary.json");

    let package = ao2([
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        "9.9.9-test",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let archive = package_json["archive"].as_str().unwrap();

    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", archive)
        .env("AO2_LINUX_ARCHIVE", archive)
        .env("AO2_LINUX_X86_64_ARCHIVE", archive)
        .env("AO2_WINDOWS_ARCHIVE", archive)
        .env("AO2_RELEASE_PROVENANCE_DIR", &provenance)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));

    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": false,
            "windows_native_smoke": "skipped",
            "windows_skip_reason": "windows_ssh_unreachable"
        }))
        .unwrap(),
    )
    .unwrap();

    // Pass --allow-unsigned-obligation-gates to keep this test focused on
    // the missing-metadata failure path; signing-required-by-default is
    // exercised in release_gate_obligation_gate_signing.rs.
    let gate = ao2([
        "release",
        "gate",
        "--summary",
        summary_path.to_str().unwrap(),
        "--provenance-dir",
        provenance.to_str().unwrap(),
        "--macos-archive",
        archive,
        "--linux-archive",
        archive,
        "--linux-x86-64-archive",
        archive,
        "--windows-archive",
        archive,
        "--allow-unsigned-obligation-gates",
    ]);
    assert!(!gate.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&gate)).unwrap();
    assert_eq!(json["schema"], "ao2.release-gate.v1");
    assert_eq!(json["status"], "failed");
    assert_eq!(json["obligation_gates"]["status"], "failed");
    assert!(json["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == "obligation_gate_metadata_failed"));
    assert!(json["obligation_gates"]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == "missing_obligation_gate_metadata"));
}

#[test]
fn cli_release_summary_enrich_embeds_latest_obligation_gate_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "release-summary-obligation-source",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let evidence_dir = repo
        .join(".ao2")
        .join("runs")
        .join("release-summary-obligation-source")
        .join("evidence-pack");
    fs::write(
        evidence_dir.join("obligation-gate-closure.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.obligation-gate.v1",
            "stage": "closure",
            "status": "passed",
            "verdict": "accepted",
            "summary": {"pass": 4, "fail": 0, "unverified": 0, "waived": 0}
        }))
        .unwrap(),
    )
    .unwrap();

    let summary_path = temp.path().join("summary.json");
    let enriched_path = temp.path().join("summary.enriched.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": false,
            "windows_native_smoke": "skipped",
            "windows_skip_reason": "windows_ssh_unreachable"
        }))
        .unwrap(),
    )
    .unwrap();

    let enrich = ao2([
        "release",
        "summary-enrich",
        "--summary",
        summary_path.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        enriched_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(enrich.status.success(), "{}", stderr(&enrich));
    let json: serde_json::Value = serde_json::from_str(&stdout(&enrich)).unwrap();
    assert_eq!(json["schema"], "ao2.release-summary-enrich.v1");
    assert_eq!(json["status"], "written");
    assert_eq!(json["run_id"], "release-summary-obligation-source");
    assert_eq!(json["obligation_gates"]["count"], 1);

    let enriched: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&enriched_path).unwrap()).unwrap();
    assert_eq!(enriched["schema"], "ao2.three-os-smoke-summary.v1");
    assert_eq!(enriched["obligation_gates"]["present"], true);
    assert_eq!(enriched["obligation_gates"]["gates"][0]["stage"], "closure");
    assert_eq!(
        enriched["obligation_gate_source"]["run_id"],
        "release-summary-obligation-source"
    );
}

#[test]
fn cli_release_summary_enrich_accepts_explicit_obligation_gate_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("empty-target");
    fs::create_dir_all(&repo).unwrap();
    let summary_path = temp.path().join("summary.json");
    let enriched_path = temp.path().join("summary.enriched.json");
    let midpoint_gate_path = temp.path().join("midpoint-obligation-gate.json");
    let closure_gate_path = temp.path().join("closure-obligation-gate.json");

    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "linux_x86_64_remote_smoke": "passed",
            "native_windows_required": true,
            "windows_native_smoke": "passed"
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &midpoint_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.obligation-gate.v1",
            "stage": "midpoint",
            "status": "passed",
            "verdict": "accepted",
            "summary": {"pass": 2, "fail": 0, "unverified": 0, "waived": 0}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &closure_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.obligation-gate.v1",
            "stage": "closure",
            "status": "passed",
            "verdict": "accepted",
            "summary": {"pass": 3, "fail": 0, "unverified": 0, "waived": 0}
        }))
        .unwrap(),
    )
    .unwrap();

    let enrich = ao2([
        "release",
        "summary-enrich",
        "--summary",
        summary_path.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--obligation-gate",
        midpoint_gate_path.to_str().unwrap(),
        "--obligation-gate",
        closure_gate_path.to_str().unwrap(),
        "--out",
        enriched_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(enrich.status.success(), "{}", stderr(&enrich));
    let json: serde_json::Value = serde_json::from_str(&stdout(&enrich)).unwrap();
    assert_eq!(json["schema"], "ao2.release-summary-enrich.v1");
    assert_eq!(json["status"], "written");
    assert_eq!(json["obligation_gates"]["present"], true);
    assert_eq!(json["obligation_gates"]["count"], 2);
    assert_eq!(json["obligation_gates"]["gates"][0]["stage"], "midpoint");
    assert_eq!(json["obligation_gates"]["gates"][1]["stage"], "closure");

    let enriched: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&enriched_path).unwrap()).unwrap();
    assert_eq!(enriched["obligation_gates"]["present"], true);
    assert_eq!(
        enriched["obligation_gate_source"]["source"],
        "explicit-artifacts"
    );
    assert_eq!(enriched["obligation_gate_source"]["gate_count"], 2);
}

#[test]
fn cli_release_gate_fails_closed_when_native_windows_required() {
    let temp = tempfile::tempdir().unwrap();
    let dist = temp.path().join("dist");
    let provenance = temp.path().join("dist-provenance");
    let summary_path = temp.path().join("summary.json");

    let package = ao2([
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        "9.9.9-test",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let archive = package_json["archive"].as_str().unwrap();

    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", archive)
        .env("AO2_LINUX_ARCHIVE", archive)
        .env("AO2_LINUX_X86_64_ARCHIVE", archive)
        .env("AO2_WINDOWS_ARCHIVE", archive)
        .env("AO2_RELEASE_PROVENANCE_DIR", &provenance)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));

    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": true,
            "windows_native_smoke": "skipped",
            "windows_skip_reason": "windows_ssh_unreachable"
        }))
        .unwrap(),
    )
    .unwrap();

    let gate = ao2([
        "release",
        "gate",
        "--summary",
        summary_path.to_str().unwrap(),
        "--provenance-dir",
        provenance.to_str().unwrap(),
        "--macos-archive",
        archive,
        "--linux-archive",
        archive,
        "--linux-x86-64-archive",
        archive,
        "--windows-archive",
        archive,
        "--require-native-windows",
    ]);
    assert!(!gate.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&gate)).unwrap();
    assert_eq!(json["schema"], "ao2.release-gate.v1");
    assert_eq!(json["status"], "failed");
    assert!(json["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == "smoke_summary_failed"));
}

#[test]
fn cli_doctor_release_checks_local_release_assets() {
    let temp = tempfile::tempdir().unwrap();
    let assets = temp.path().join("release-assets");
    let provenance = assets.clone();
    fs::create_dir_all(&assets).unwrap();

    let macos = package_test_archive(temp.path(), "macos-aarch64");
    let linux = package_test_archive(temp.path(), "linux-aarch64");
    let linux_x86_64 = package_test_archive(temp.path(), "linux-x86_64");
    let windows = package_test_archive(temp.path(), "windows-x86_64");

    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", &macos)
        .env("AO2_LINUX_ARCHIVE", &linux)
        .env("AO2_LINUX_X86_64_ARCHIVE", &linux_x86_64)
        .env("AO2_WINDOWS_ARCHIVE", &windows)
        .env("AO2_RELEASE_PROVENANCE_DIR", &provenance)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));

    for archive in [&macos, &linux, &linux_x86_64, &windows] {
        fs::copy(archive, assets.join(archive.file_name().unwrap())).unwrap();
    }
    fs::write(
        assets.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.9-test",
            "release_repo": "uesugitorachiyo/ao2",
            "status": "verified",
            "platforms": {
                "macos-aarch64": {
                    "status": "passed",
                    "marker": "macos_download_rollback=passed"
                },
                "linux-x86_64": {
                    "status": "passed",
                    "marker": "ubuntu_download_rollback=passed"
                },
                "windows-x86_64": {
                    "status": "passed",
                    "marker": "windows_download_rollback=passed"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let doctor = ao2([
        "doctor",
        "--json",
        "--release",
        "v9.9.9-test",
        "--release-asset-dir",
        assets.to_str().unwrap(),
        "--provenance-dir",
        provenance.to_str().unwrap(),
    ]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json: serde_json::Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(json["schema_version"], "ao2.doctor.v1");
    assert_eq!(json["release"]["release_tag"], "v9.9.9-test");
    assert_eq!(json["release"]["asset_source"], "directory");
    assert_eq!(json["release"]["assets_available"], true);
    assert_eq!(json["release"]["asset_count"], 15);
    assert_eq!(json["release"]["provenance_verified"], true);
    assert_eq!(json["release"]["rollback"]["checked"], true);
    assert_eq!(json["release"]["rollback"]["status"], "verified");
}

#[test]
fn cli_doctor_accepts_exact_hosted_release_publication() {
    let temp = tempfile::tempdir().unwrap();
    let assets = temp.path().join("release-assets");
    let install_dir = temp.path().join("bin");
    fs::create_dir_all(&install_dir).unwrap();
    copy_hosted_release_fixture(&assets);
    let binary_name = if cfg!(windows) { "ao2.exe" } else { "ao2" };
    fs::copy(env!("CARGO_BIN_EXE_ao2"), install_dir.join(binary_name)).unwrap();
    let path = prepend_path(&install_dir);

    let doctor = ao2_with_env(
        [
            "doctor",
            "--json",
            "--install-dir",
            install_dir.to_str().unwrap(),
            "--release",
            "v9.9.9",
            "--release-asset-dir",
            assets.to_str().unwrap(),
            "--provenance-dir",
            assets.to_str().unwrap(),
        ],
        [("PATH", path.as_str())],
    );

    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json: serde_json::Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(
        json["release"]["expected_assets"],
        serde_json::json!([
            "ao2-9.9.9-macos-aarch64.tar.gz",
            "ao2-9.9.9-linux-x86_64.tar.gz",
            "ao2-9.9.9-windows-x86_64.tar.gz",
            "promotion-plan.json",
            "SHA256SUMS",
        ])
    );
    assert_eq!(json["release"]["asset_count"], 5);
    assert_eq!(json["release"]["assets_available"], true);
    assert_eq!(json["release"]["provenance_verified"], true);
    assert_eq!(json["release"]["provenance_tag_matches"], true);
    assert_eq!(json["release"]["hosted_contract"]["status"], "verified");
    assert_eq!(
        json["release"]["hosted_contract"]["checksums_verified"],
        true
    );
    assert_eq!(
        json["release"]["hosted_contract"]["promotion_plan_verified"],
        true
    );
}

#[test]
fn cli_doctor_rejects_hosted_release_digest_tampering() {
    let temp = tempfile::tempdir().unwrap();
    let assets = temp.path().join("release-assets");
    copy_hosted_release_fixture(&assets);
    fs::write(
        assets.join("ao2-9.9.9-windows-x86_64.tar.gz"),
        "altered hosted Windows candidate\n",
    )
    .unwrap();

    let doctor = ao2([
        "doctor",
        "--json",
        "--release",
        "v9.9.9",
        "--release-asset-dir",
        assets.to_str().unwrap(),
        "--provenance-dir",
        assets.to_str().unwrap(),
    ]);

    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json: serde_json::Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(json["status"], "attention");
    assert_eq!(json["release"]["assets_available"], true);
    assert_eq!(json["release"]["provenance_verified"], false);
    assert_eq!(json["release"]["hosted_contract"]["status"], "invalid");
    assert_eq!(
        json["release"]["hosted_contract"]["checksums_verified"],
        false
    );
}

#[test]
fn cli_doctor_release_github_asset_check_times_out() {
    let temp = tempfile::tempdir().unwrap();
    let provenance = temp.path().join("provenance");
    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir_all(&provenance).unwrap();
    fs::create_dir_all(&fake_bin).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let gh = fake_bin.join("gh");
        fs::write(
            &gh,
            r#"#!/bin/sh
sleep 30
printf '{"assets":[],"isDraft":false,"isPrerelease":false}\n'
"#,
        )
        .unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(windows)]
    {
        fs::write(
            fake_bin.join("gh.cmd"),
            r#"@echo off
ping -n 31 127.0.0.1 >NUL
echo {"assets":[],"isDraft":false,"isPrerelease":false}
"#,
        )
        .unwrap();
    }

    let path = prepend_path(&fake_bin);
    let started = std::time::Instant::now();
    let doctor = ao2_with_env(
        [
            "doctor",
            "--json",
            "--release",
            "v9.9.9-test",
            "--provenance-dir",
            provenance.to_str().unwrap(),
        ],
        [("PATH", path.as_str()), ("AO2_DOCTOR_GH_TIMEOUT_MS", "100")],
    );
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "doctor should not wait for hanging gh release view"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(json["release"]["asset_source"], "github");
    assert_eq!(json["release"]["assets_available"], false);
    assert_eq!(json["release"]["error"], "gh_timed_out");
}

#[test]
fn cli_install_update_verifies_archive_signature_and_installs_binary() {
    let temp = tempfile::tempdir().unwrap();
    let dist = temp.path().join("dist");
    let provenance = temp.path().join("dist-provenance");
    let install_dir = temp.path().join("bin");

    let package = ao2([
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        "9.9.9-test",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let archive = package_json["archive"].as_str().unwrap();

    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", archive)
        .env("AO2_LINUX_ARCHIVE", archive)
        .env("AO2_LINUX_X86_64_ARCHIVE", archive)
        .env("AO2_WINDOWS_ARCHIVE", archive)
        .env("AO2_RELEASE_PROVENANCE_DIR", &provenance)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));

    let update = ao2([
        "install",
        "update",
        "--archive",
        archive,
        "--provenance-dir",
        provenance.to_str().unwrap(),
        "--install-dir",
        install_dir.to_str().unwrap(),
    ]);
    assert!(update.status.success(), "{}", stderr(&update));
    let json: serde_json::Value = serde_json::from_str(&stdout(&update)).unwrap();
    assert_eq!(json["status"], "installed");
    assert_eq!(json["version"], "9.9.9-test");
    assert_eq!(json["signature_verified"], true);
    assert_eq!(json["offline_verification"]["status"], "verified");
    assert_eq!(
        json["offline_verification"]["schema_version"],
        "ao2.release-archive-offline-verification.v1"
    );
    assert_eq!(json["offline_verification"]["checksum_file"], "SHA256SUMS");
    assert_eq!(
        json["offline_verification"]["verification_report"],
        "RELEASE-VERIFICATION.json"
    );
    assert_eq!(
        json["offline_verification"]["checksum_coverage_verified"],
        true
    );
    assert_eq!(
        json["offline_verification"]["provider_api_keys_required"],
        false
    );
    assert_eq!(
        json["offline_verification"]["control_plane_approves_release"],
        false
    );
    assert_eq!(json["offline_verification"]["mutates_ao_artifacts"], false);
    assert_eq!(
        json["offline_verification"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    let evidence_path = Path::new(json["install_verification_evidence"].as_str().unwrap());
    assert!(evidence_path.is_file());
    let evidence: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(evidence_path).unwrap()).unwrap();
    assert_eq!(
        evidence["schema_version"],
        "ao2.install-verification-evidence.v1"
    );
    assert_eq!(evidence["status"], "verified");
    assert_eq!(evidence["signature_verified"], true);
    assert_eq!(evidence["offline_verification"]["status"], "verified");
    assert_eq!(
        evidence["offline_verification"]["release_acceptance_owner"],
        "factory-v3 evaluator-closer"
    );
    assert!(Path::new(json["installed_binary"].as_str().unwrap()).is_file());

    let installed = Command::new(json["installed_binary"].as_str().unwrap())
        .arg("version")
        .arg("--json")
        .output()
        .unwrap();
    assert!(installed.status.success(), "{}", stderr(&installed));
}

#[test]
fn cli_install_update_rejects_signed_archive_without_release_verification_report() {
    let temp = tempfile::tempdir().unwrap();
    let dist = temp.path().join("dist");
    let provenance = temp.path().join("dist-provenance");
    let install_dir = temp.path().join("bin");
    let extract_dir = temp.path().join("repacked");

    let package = ao2([
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        "9.9.9-test",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let archive = Path::new(package_json["archive"].as_str().unwrap());

    extract_test_tar_gz(archive, &extract_dir);
    fs::remove_file(extract_dir.join("RELEASE-VERIFICATION.json")).unwrap();
    let repacked_archive = dist.join("ao2-9.9.9-test-missing-release-verification.tar.gz");
    create_test_tar_gz(&extract_dir, &repacked_archive);
    sign_test_release_archive(temp.path(), &repacked_archive, &provenance);

    let update = ao2([
        "install",
        "update",
        "--archive",
        repacked_archive.to_str().unwrap(),
        "--provenance-dir",
        provenance.to_str().unwrap(),
        "--install-dir",
        install_dir.to_str().unwrap(),
    ]);
    assert!(
        !update.status.success(),
        "install should fail without release verification report"
    );
    assert!(
        stderr(&update).contains("release verification report"),
        "stderr:\n{}",
        stderr(&update)
    );
    assert!(
        !install_dir.exists(),
        "install directory should not be created after verification failure"
    );
}

#[test]
fn cli_doctor_reports_install_provider_release_and_path_health() {
    let temp = tempfile::tempdir().unwrap();
    let install_dir = temp.path().join("bin");
    let provenance = temp.path().join("provenance");
    let dist = temp.path().join("dist");
    fs::create_dir_all(&install_dir).unwrap();

    let binary_name = if cfg!(windows) { "ao2.exe" } else { "ao2" };
    let installed_binary = install_dir.join(binary_name);
    fs::copy(env!("CARGO_BIN_EXE_ao2"), &installed_binary).unwrap();

    let package = ao2([
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        "9.9.9-test",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let archive = package_json["archive"].as_str().unwrap();

    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", archive)
        .env("AO2_LINUX_ARCHIVE", archive)
        .env("AO2_LINUX_X86_64_ARCHIVE", archive)
        .env("AO2_WINDOWS_ARCHIVE", archive)
        .env("AO2_RELEASE_PROVENANCE_DIR", &provenance)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));

    let update = ao2([
        "install",
        "update",
        "--archive",
        archive,
        "--provenance-dir",
        provenance.to_str().unwrap(),
        "--install-dir",
        install_dir.to_str().unwrap(),
    ]);
    assert!(update.status.success(), "{}", stderr(&update));
    let update_json: serde_json::Value = serde_json::from_str(&stdout(&update)).unwrap();
    assert!(Path::new(
        update_json["install_verification_evidence"]
            .as_str()
            .unwrap()
    )
    .is_file());

    let path = prepend_path(&install_dir);
    let doctor = ao2_with_env(
        [
            "doctor",
            "--json",
            "--install-dir",
            install_dir.to_str().unwrap(),
            "--provenance-dir",
            provenance.to_str().unwrap(),
        ],
        [("PATH", path.as_str())],
    );
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json: serde_json::Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["install"]["installed"], true);
    assert_eq!(json["install"]["on_path"], true);
    assert_eq!(json["install"]["verification_evidence"]["present"], true);
    assert_eq!(
        json["install"]["verification_evidence"]["status"],
        "verified"
    );
    assert_eq!(
        json["install"]["verification_evidence"]["schema_version"],
        "ao2.install-verification-evidence.v1"
    );
    assert_eq!(
        json["install"]["verification_evidence"]["offline_verification"]["status"],
        "verified"
    );
    assert_eq!(json["release"]["provenance_verified"], true);
    assert_eq!(json["providers"]["scripted"]["available"], true);
    assert_eq!(json["dependencies"]["native_crypto"], true);
    assert!(json["dependencies"].get("openssl").is_none());
    assert_eq!(json["dependencies"]["curl"], true);
    assert_eq!(json["dependencies"]["tar"], true);
}

#[test]
fn cli_upgrade_check_reports_latest_release_from_fixture() {
    let temp = tempfile::tempdir().unwrap();
    let release_file = temp.path().join("release.json");
    fs::write(
        &release_file,
        r#"{
  "tagName": "v9.9.9",
  "assets": [
    {"name": "ao2-9.9.9-macos-aarch64.tar.gz", "digest": "sha256:test"}
  ]
}"#,
    )
    .unwrap();

    let check = ao2([
        "upgrade",
        "check",
        "--release-file",
        release_file.to_str().unwrap(),
    ]);
    assert!(check.status.success(), "{}", stderr(&check));
    let json: serde_json::Value = serde_json::from_str(&stdout(&check)).unwrap();
    assert_eq!(json["current_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["latest_version"], "9.9.9");
    assert_eq!(json["update_available"], true);
    assert!(json["assets"][0]["name"]
        .as_str()
        .unwrap()
        .contains("ao2-9.9.9"));
}

#[test]
fn cli_upgrade_apply_installs_signed_release_and_keeps_rollback() {
    let temp = tempfile::tempdir().unwrap();
    let dist = temp.path().join("dist");
    let provenance = temp.path().join("provenance");
    let asset_dir = temp.path().join("assets");
    let download_dir = temp.path().join("downloads");
    let install_dir = temp.path().join("bin");
    fs::create_dir_all(&install_dir).unwrap();
    fs::create_dir_all(&asset_dir).unwrap();

    let binary_name = if cfg!(windows) { "ao2.exe" } else { "ao2" };
    let installed_binary = install_dir.join(binary_name);
    fs::write(&installed_binary, "old-binary\n").unwrap();

    let package = ao2([
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        "9.9.9-test",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let archive = Path::new(package_json["archive"].as_str().unwrap());
    let archive_name = archive.file_name().unwrap().to_str().unwrap();

    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", archive)
        .env("AO2_LINUX_ARCHIVE", archive)
        .env("AO2_LINUX_X86_64_ARCHIVE", archive)
        .env("AO2_WINDOWS_ARCHIVE", archive)
        .env("AO2_RELEASE_PROVENANCE_DIR", &provenance)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));

    fs::copy(archive, asset_dir.join(archive_name)).unwrap();
    for entry in fs::read_dir(&provenance).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), asset_dir.join(entry.file_name())).unwrap();
        }
    }

    let release_file = temp.path().join("release.json");
    fs::write(
        &release_file,
        format!(
            r#"{{
  "tagName": "v9.9.9-test",
  "assets": [
    {{"name": "{archive_name}"}},
    {{"name": "{archive_name}.sha256"}},
    {{"name": "{archive_name}.sig"}},
    {{"name": "ao2-release-signing-public.pem"}},
    {{"name": "ao2-release-provenance.json"}},
    {{"name": "ao2-release-provenance.json.sig"}}
  ]
}}"#
        ),
    )
    .unwrap();

    let apply = ao2([
        "upgrade",
        "apply",
        "--release-file",
        release_file.to_str().unwrap(),
        "--asset-dir",
        asset_dir.to_str().unwrap(),
        "--download-dir",
        download_dir.to_str().unwrap(),
        "--install-dir",
        install_dir.to_str().unwrap(),
    ]);
    assert!(apply.status.success(), "{}", stderr(&apply));
    let json: serde_json::Value = serde_json::from_str(&stdout(&apply)).unwrap();
    assert_eq!(json["status"], "upgraded");
    assert_eq!(json["check"]["latest_version"], "9.9.9-test");
    assert_eq!(json["install"]["signature_verified"], true);
    assert_eq!(
        json["install"]["offline_verification"]["status"],
        "verified"
    );
    assert!(Path::new(
        json["install"]["install_verification_evidence"]
            .as_str()
            .unwrap()
    )
    .is_file());
    assert!(Path::new(json["install"]["installed_binary"].as_str().unwrap()).is_file());
    assert_eq!(
        fs::read_to_string(json["install"]["rollback_binary"].as_str().unwrap()).unwrap(),
        "old-binary\n"
    );
}

#[test]
fn cli_upgrade_apply_can_use_github_release_downloaded_assets() {
    let temp = tempfile::tempdir().unwrap();
    let dist = temp.path().join("dist");
    let provenance = temp.path().join("provenance");
    let fake_gh_assets = temp.path().join("fake-gh-assets");
    let download_dir = temp.path().join("gh-downloads");
    let install_dir = temp.path().join("bin");
    fs::create_dir_all(&install_dir).unwrap();
    fs::create_dir_all(&fake_gh_assets).unwrap();

    let binary_name = if cfg!(windows) { "ao2.exe" } else { "ao2" };
    fs::write(install_dir.join(binary_name), "old-binary\n").unwrap();

    let package = ao2([
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        "9.9.9-test",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let archive = Path::new(package_json["archive"].as_str().unwrap());

    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", archive)
        .env("AO2_LINUX_ARCHIVE", archive)
        .env("AO2_LINUX_X86_64_ARCHIVE", archive)
        .env("AO2_WINDOWS_ARCHIVE", archive)
        .env("AO2_RELEASE_PROVENANCE_DIR", &provenance)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));

    fs::copy(archive, fake_gh_assets.join(archive.file_name().unwrap())).unwrap();
    for entry in fs::read_dir(&provenance).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), fake_gh_assets.join(entry.file_name())).unwrap();
        }
    }

    let apply = ao2_with_env(
        [
            "upgrade",
            "apply",
            "--github-release",
            "v9.9.9-test",
            "--repo",
            "example/private",
            "--download-dir",
            download_dir.to_str().unwrap(),
            "--install-dir",
            install_dir.to_str().unwrap(),
        ],
        [(
            "AO2_TEST_FAKE_GH_ASSET_DIR",
            fake_gh_assets.to_str().unwrap(),
        )],
    );
    assert!(apply.status.success(), "{}", stderr(&apply));
    let json: serde_json::Value = serde_json::from_str(&stdout(&apply)).unwrap();
    assert_eq!(json["status"], "upgraded");
    assert_eq!(json["check"]["latest_version"], "9.9.9-test");
    assert_eq!(json["install"]["signature_verified"], true);
    assert!(Path::new(json["install"]["installed_binary"].as_str().unwrap()).is_file());
}

#[test]
fn cli_install_update_keeps_previous_binary_for_rollback() {
    let temp = tempfile::tempdir().unwrap();
    let dist = temp.path().join("dist");
    let provenance = temp.path().join("provenance");
    let install_dir = temp.path().join("bin");
    fs::create_dir_all(&install_dir).unwrap();

    let binary_name = if cfg!(windows) { "ao2.exe" } else { "ao2" };
    let installed_binary = install_dir.join(binary_name);
    fs::write(&installed_binary, "old-binary\n").unwrap();

    let package = ao2([
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        "9.9.9-test",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let archive = package_json["archive"].as_str().unwrap();

    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", archive)
        .env("AO2_LINUX_ARCHIVE", archive)
        .env("AO2_LINUX_X86_64_ARCHIVE", archive)
        .env("AO2_WINDOWS_ARCHIVE", archive)
        .env("AO2_RELEASE_PROVENANCE_DIR", &provenance)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));

    let update = ao2([
        "install",
        "update",
        "--archive",
        archive,
        "--provenance-dir",
        provenance.to_str().unwrap(),
        "--install-dir",
        install_dir.to_str().unwrap(),
    ]);
    assert!(update.status.success(), "{}", stderr(&update));
    let json: serde_json::Value = serde_json::from_str(&stdout(&update)).unwrap();
    let rollback_binary = json["rollback_binary"].as_str().unwrap();
    assert_eq!(fs::read_to_string(rollback_binary).unwrap(), "old-binary\n");

    let rollback = ao2([
        "install",
        "rollback",
        "--install-dir",
        install_dir.to_str().unwrap(),
    ]);
    assert!(rollback.status.success(), "{}", stderr(&rollback));
    let rollback_json: serde_json::Value = serde_json::from_str(&stdout(&rollback)).unwrap();
    assert_eq!(rollback_json["status"], "rolled_back");
    assert_eq!(
        fs::read_to_string(installed_binary).unwrap(),
        "old-binary\n"
    );
}

#[test]
fn cli_release_compare_writes_signed_release_comparison_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let releases = temp.path().join("release-download");
    let out_dir = temp.path().join("release-comparison-bundles");
    let v1 = releases.join("v9.9.8-test");
    let v2 = releases.join("v9.9.9-test");
    fs::create_dir_all(&v1).unwrap();
    fs::create_dir_all(&v2).unwrap();
    fs::write(
        v1.join("release-doctor.json"),
        serde_json::json!({
            "status": "attention",
            "release": {
                "release_tag": "v9.9.8-test",
                "assets_available": false,
                "asset_count": 14,
                "provenance_verified": true,
                "provenance_tag_matches": true
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        v1.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.8-test",
            "status": "incomplete",
            "platforms": {
                "macos-aarch64": {"status": "passed"},
                "linux-x86_64": {"status": "passed"},
                "windows-x86_64": {"status": "skipped"}
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        v2.join("release-doctor.json"),
        serde_json::json!({
            "status": "ok",
            "release": {
                "release_tag": "v9.9.9-test",
                "assets_available": true,
                "asset_count": 15,
                "provenance_verified": true,
                "provenance_tag_matches": true
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        v2.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.9-test",
            "status": "verified",
            "platforms": {
                "macos-aarch64": {"status": "passed"},
                "linux-x86_64": {"status": "passed"},
                "windows-x86_64": {"status": "passed"}
            }
        })
        .to_string(),
    )
    .unwrap();
    let signing_key = temp.path().join("release-comparison-key.pem");
    generate_native_signing_key(&signing_key, 3072);

    let output = ao2([
        "release",
        "compare",
        "--release-download-dir",
        releases.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "release-lead",
        "--json",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let json: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(json["schema_version"], "ao2.release-comparison-bundle.v1");
    assert_eq!(
        json["release_history"]["trend"]["latest_release_tag"],
        "v9.9.9-test"
    );
    assert_eq!(json["release_history"]["trend"]["latest_health_score"], 6);
    assert_eq!(json["release_history"]["trend"]["attention_count"], 1);
    assert_eq!(json["support_metadata"]["present"], true);
    assert_eq!(json["support_metadata"]["signature_verified"], true);
    assert_eq!(json["support_metadata"]["signer_id"], "release-lead");
    assert_eq!(json["support_metadata"]["metadata"]["release_count"], 2);
    assert_eq!(
        json["support_metadata"]["metadata"]["latest_release_tag"],
        "v9.9.9-test"
    );
    let bundle_dir = PathBuf::from(json["bundle_dir"].as_str().unwrap());
    assert!(bundle_dir.join("release-comparison.json").is_file());
    assert!(bundle_dir.join("release-history.json").is_file());
    assert!(bundle_dir
        .join("release-comparison-metadata.json")
        .is_file());
    assert!(bundle_dir
        .join("release-comparison-metadata.json.sig")
        .is_file());
    assert!(bundle_dir
        .join("release-comparison-signing-public.pem")
        .is_file());
    assert!(bundle_dir.join("SHA256SUMS").is_file());
}

#[test]
fn cli_release_compare_verify_validates_signed_bundle_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let releases = temp.path().join("release-download");
    let out_dir = temp.path().join("release-comparison-bundles");
    let release = releases.join("v9.9.9-test");
    fs::create_dir_all(&release).unwrap();
    fs::write(
        release.join("release-doctor.json"),
        serde_json::json!({
            "status": "ok",
            "release": {
                "release_tag": "v9.9.9-test",
                "assets_available": true,
                "asset_count": 15,
                "provenance_verified": true,
                "provenance_tag_matches": true
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        release.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.9-test",
            "status": "verified",
            "platforms": {
                "macos-aarch64": {"status": "passed"},
                "linux-x86_64": {"status": "passed"},
                "windows-x86_64": {"status": "passed"}
            }
        })
        .to_string(),
    )
    .unwrap();
    let signing_key = temp.path().join("release-comparison-key.pem");
    generate_native_signing_key(&signing_key, 3072);
    let compare = ao2([
        "release",
        "compare",
        "--release-download-dir",
        releases.to_str().unwrap(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "release-lead",
        "--json",
    ]);
    assert!(compare.status.success(), "{}", stderr(&compare));
    let comparison: serde_json::Value = serde_json::from_str(&stdout(&compare)).unwrap();
    let bundle_dir = PathBuf::from(comparison["bundle_dir"].as_str().unwrap());

    let verify = ao2([
        "release",
        "compare-verify",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let report: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        report["schema_version"],
        "ao2.release-comparison-verification.v1"
    );
    assert_eq!(report["status"], "verified");
    assert_eq!(report["signature_verified"], true);
    assert_eq!(report["manifest_verified"], true);
    assert_eq!(report["latest_release_tag"], "v9.9.9-test");
    assert_eq!(report["signer_id"], "release-lead");

    fs::write(bundle_dir.join("release-history.json"), "{}").unwrap();
    let tampered = ao2([
        "release",
        "compare-verify",
        "--bundle-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!tampered.status.success());
    assert!(stderr(&tampered).contains("release comparison bundle verification failed"));
}
