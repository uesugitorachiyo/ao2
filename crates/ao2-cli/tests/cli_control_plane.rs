use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

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

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn value_for<'a>(output: &'a str, prefix: &str) -> &'a str {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing {prefix} in {output}"))
        .trim()
}

fn normalize_separators(input: &str) -> String {
    input.replace('\\', "/")
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.shutdown(std::net::Shutdown::Write).ok();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn http_body(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn read_server_port(child: &mut std::process::Child) -> u16 {
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    line.trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap()
        .parse::<u16>()
        .unwrap()
}

fn generate_native_signing_key(path: &Path, bits: usize) {
    let output = Command::new("openssl")
        .args([
            "genpkey",
            "-algorithm",
            "RSA",
            "-pkeyopt",
            &format!("rsa_keygen_bits:{bits}"),
            "-out",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        stdout(&output),
        stderr(&output)
    );
}

fn sha256_hex_for_test(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[test]
fn cli_control_plane_ingest_writes_read_only_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "control-plane-ingest-demo",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let workbench_dir = repo.join(".ao2/workbench");
    fs::create_dir_all(&workbench_dir).unwrap();
    fs::write(
        workbench_dir.join("queue.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.workbench-queue-file.v1",
            "jobs": [{
                "job_id": "job-control-plane",
                "run_id": "control-plane-ingest-demo",
                "template": "bug-fix",
                "provider": "scripted",
                "provider_prompt_file": "",
                "max_repair_attempts": 1,
                "retry_of": "",
                "status": "accepted",
                "evidence_pack": repo.join(".ao2/runs/control-plane-ingest-demo/evidence-pack/evidence-pack.json"),
                "cockpit": repo.join(".ao2/runs/control-plane-ingest-demo/cockpit/index.html"),
                "stdout_log": "",
                "stderr_log": "",
                "queued_at_ms": 1,
                "started_at_ms": 2,
                "finished_at_ms": 3,
                "queue_wait_ms": 1,
                "duration_ms": 1,
                "exit_code": 0,
                "retry_count": 0,
                "error": ""
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        workbench_dir.join("audit.jsonl"),
        "{\"schema_version\":\"ao2.workbench-audit-event.v1\",\"timestamp_ms\":4,\"action\":\"start\",\"job_id\":\"job-control-plane\",\"run_id\":\"control-plane-ingest-demo\"}\n",
    )
    .unwrap();

    let ingest = ao2([
        "control-plane",
        "ingest",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(ingest.status.success(), "{}", stderr(&ingest));
    let stdout_json: serde_json::Value = serde_json::from_str(&stdout(&ingest)).unwrap();
    assert_eq!(stdout_json["schema_version"], "ao2.control-plane-ingest.v1");
    let snapshot_path = Path::new(stdout_json["snapshot_path"].as_str().unwrap());
    assert!(snapshot_path.is_file());
    assert!(normalize_separators(&snapshot_path.to_string_lossy())
        .ends_with(".ao2/control-plane/snapshot.json"));
    let snapshot: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(snapshot_path).unwrap()).unwrap();
    assert_eq!(snapshot["schema_version"], "ao2.control-plane-snapshot.v1");
    assert_eq!(
        snapshot["runs"]["runs"][0]["run_id"],
        "control-plane-ingest-demo"
    );
    assert_eq!(snapshot["queue"]["jobs"][0]["job_id"], "job-control-plane");
    assert_eq!(snapshot["audit_events"][0]["action"], "start");
    assert!(snapshot["evidence_packs"][0]
        .as_str()
        .unwrap()
        .contains("evidence-pack.json"));
}

#[test]
fn cli_control_plane_serve_once_returns_dashboard_and_snapshot_api() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "control-plane-serve-demo",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let ingest = ao2([
        "control-plane",
        "ingest",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(ingest.status.success(), "{}", stderr(&ingest));

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "control-plane",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "cp-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let html = http_request(
        port,
        "GET /?token=cp-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(html.starts_with("HTTP/1.1 200 OK"), "{html}");
    assert!(html.contains("AO2 Control Plane"));
    assert!(html.contains("Snapshot"));
    assert!(html.contains("Runs"));
    assert!(html.contains("Queue Jobs"));
    assert!(html.contains("control-plane-serve-demo"));

    let snapshot_response = http_request(
        port,
        "GET /api/control-plane/snapshot?token=cp-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(
        snapshot_response.starts_with("HTTP/1.1 200 OK"),
        "{snapshot_response}"
    );
    let snapshot: serde_json::Value = serde_json::from_str(http_body(&snapshot_response)).unwrap();
    assert_eq!(snapshot["schema_version"], "ao2.control-plane-snapshot.v1");

    let forbidden = http_request(
        port,
        "GET /api/control-plane/snapshot?token=bad HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(
        forbidden.starts_with("HTTP/1.1 403 Forbidden"),
        "{forbidden}"
    );
    let error: serde_json::Value = serde_json::from_str(http_body(&forbidden)).unwrap();
    assert_eq!(error["schema_version"], "ao2.control-plane-error.v1");
    assert_eq!(error["error"], "invalid_api_token");
    child.kill().ok();
    child.wait().ok();
}

#[test]
fn cli_control_plane_export_writes_static_dashboard() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "control-plane-export-demo",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let ingest = ao2([
        "control-plane",
        "ingest",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(ingest.status.success(), "{}", stderr(&ingest));

    let export = ao2([
        "control-plane",
        "export",
        "--target",
        repo.to_str().unwrap(),
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let export_stdout = stdout(&export);
    let control_plane_path = value_for(&export_stdout, "control_plane=");
    let html_path = Path::new(control_plane_path);
    assert!(html_path.is_file());
    assert!(normalize_separators(&html_path.to_string_lossy())
        .ends_with(".ao2/control-plane/index.html"));
    let html = fs::read_to_string(html_path).unwrap();
    assert!(html.contains("AO2 Control Plane"));
    assert!(html.contains("Snapshot"));
    assert!(html.contains("control-plane-export-demo"));
}

#[test]
fn cli_control_plane_index_combines_multiple_repo_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let repo_a = temp.path().join("discount-service-a");
    let repo_b = temp.path().join("discount-service-b");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_a);
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_b);

    let run_a = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo_a.to_str().unwrap(),
        "--run-id",
        "fleet-index-a",
    ]);
    assert!(run_a.status.success(), "{}", stderr(&run_a));
    let run_b = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo_b.to_str().unwrap(),
        "--run-id",
        "fleet-index-b",
    ]);
    assert!(run_b.status.success(), "{}", stderr(&run_b));

    for repo in [&repo_a, &repo_b] {
        let ingest = ao2([
            "control-plane",
            "ingest",
            "--target",
            repo.to_str().unwrap(),
            "--json",
        ]);
        assert!(ingest.status.success(), "{}", stderr(&ingest));
    }

    let fleet_path = temp.path().join("fleet-snapshot.json");
    let index = ao2([
        "control-plane",
        "index",
        "--target",
        repo_a.to_str().unwrap(),
        "--target",
        repo_b.to_str().unwrap(),
        "--out",
        fleet_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(index.status.success(), "{}", stderr(&index));
    let stdout_json: serde_json::Value = serde_json::from_str(&stdout(&index)).unwrap();
    assert_eq!(stdout_json["schema_version"], "ao2.control-plane-index.v1");
    assert_eq!(stdout_json["repository_count"], 2);
    assert!(fleet_path.is_file());
    let fleet: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fleet_path).unwrap()).unwrap();
    assert_eq!(
        fleet["schema_version"],
        "ao2.control-plane-fleet-snapshot.v1"
    );
    assert_eq!(fleet["repositories"].as_array().unwrap().len(), 2);
    assert_eq!(fleet["totals"]["run_count"], 2);
    let fleet_text = serde_json::to_string(&fleet).unwrap();
    assert!(fleet_text.contains("fleet-index-a"));
    assert!(fleet_text.contains("fleet-index-b"));
}

#[test]
fn cli_control_plane_refresh_reingests_targets_and_writes_fleet_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let repo_a = temp.path().join("refresh-service-a");
    let repo_b = temp.path().join("refresh-service-b");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_a);
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_b);

    let run_a = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo_a.to_str().unwrap(),
        "--run-id",
        "fleet-refresh-a",
    ]);
    assert!(run_a.status.success(), "{}", stderr(&run_a));
    let run_b = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo_b.to_str().unwrap(),
        "--run-id",
        "fleet-refresh-b",
    ]);
    assert!(run_b.status.success(), "{}", stderr(&run_b));

    let fleet_path = temp.path().join("fleet-refresh.json");
    let refresh = ao2([
        "control-plane",
        "refresh",
        "--target",
        repo_a.to_str().unwrap(),
        "--target",
        repo_b.to_str().unwrap(),
        "--out",
        fleet_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(refresh.status.success(), "{}", stderr(&refresh));
    let stdout_json: serde_json::Value = serde_json::from_str(&stdout(&refresh)).unwrap();
    assert_eq!(
        stdout_json["schema_version"],
        "ao2.control-plane-refresh.v1"
    );
    assert_eq!(stdout_json["refreshed_repository_count"], 2);
    assert!(repo_a.join(".ao2/control-plane/snapshot.json").is_file());
    assert!(repo_b.join(".ao2/control-plane/snapshot.json").is_file());
    assert!(fleet_path.is_file());
    let fleet: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fleet_path).unwrap()).unwrap();
    assert_eq!(
        fleet["schema_version"],
        "ao2.control-plane-fleet-snapshot.v1"
    );
    assert_eq!(fleet["repositories"].as_array().unwrap().len(), 2);
    let fleet_text = serde_json::to_string(&fleet).unwrap();
    assert!(fleet_text.contains("fleet-refresh-a"));
    assert!(fleet_text.contains("fleet-refresh-b"));
}

#[test]
fn cli_control_plane_health_reports_provider_readiness_rollup() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let smoke = ao2([
        "provider",
        "smoke-all",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(smoke.status.success(), "{}", stderr(&smoke));

    let fleet_path = temp.path().join("provider-readiness-fleet.json");
    let refresh = ao2([
        "control-plane",
        "refresh",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        fleet_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(refresh.status.success(), "{}", stderr(&refresh));
    let fleet: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fleet_path).unwrap()).unwrap();
    assert_eq!(
        fleet["repositories"][0]["snapshot"]["provider_smoke_history"]["schema"],
        "ao2.provider-smoke-history.v1"
    );

    let health = ao2([
        "control-plane",
        "health",
        "--fleet",
        fleet_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(health.status.success(), "{}", stderr(&health));
    let health_json: serde_json::Value = serde_json::from_str(&stdout(&health)).unwrap();
    assert_eq!(health_json["schema_version"], "ao2.control-plane-health.v1");
    assert_eq!(
        health_json["provider_readiness"]["schema"],
        "ao2.provider-readiness-rollup.v1"
    );
    assert_eq!(health_json["provider_readiness"]["repository_count"], 1);
    assert_eq!(
        health_json["provider_readiness"]["missing_history_count"],
        0
    );
    assert_eq!(
        health_json["provider_readiness"]["providers"]["scripted"]["ready_count"],
        1
    );
    let codes = health_json["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|alert| alert["code"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(!codes.contains(&"provider_smoke_missing"));
}

#[test]
fn cli_control_plane_export_and_serve_fleet_dashboard() {
    let temp = tempfile::tempdir().unwrap();
    let repo_a = temp.path().join("discount-service-a");
    let repo_b = temp.path().join("discount-service-b");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_a);
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_b);

    let run_a = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo_a.to_str().unwrap(),
        "--run-id",
        "fleet-dashboard-a",
    ]);
    assert!(run_a.status.success(), "{}", stderr(&run_a));
    let run_b = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo_b.to_str().unwrap(),
        "--run-id",
        "fleet-dashboard-b",
    ]);
    assert!(run_b.status.success(), "{}", stderr(&run_b));

    for repo in [&repo_a, &repo_b] {
        let ingest = ao2([
            "control-plane",
            "ingest",
            "--target",
            repo.to_str().unwrap(),
            "--json",
        ]);
        assert!(ingest.status.success(), "{}", stderr(&ingest));
    }

    let fleet_path = temp.path().join("fleet-dashboard.json");
    let index = ao2([
        "control-plane",
        "index",
        "--target",
        repo_a.to_str().unwrap(),
        "--target",
        repo_b.to_str().unwrap(),
        "--out",
        fleet_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(index.status.success(), "{}", stderr(&index));

    let html_path = temp.path().join("fleet.html");
    let export = ao2([
        "control-plane",
        "export",
        "--fleet",
        fleet_path.to_str().unwrap(),
        "--out",
        html_path.to_str().unwrap(),
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let html = fs::read_to_string(&html_path).unwrap();
    assert!(html.contains("AO2 Control Plane"));
    assert!(html.contains("Repositories"));
    assert!(html.contains("Total Runs"));
    assert!(html.contains("id=\"fleet-search\""));
    assert!(html.contains("id=\"fleet-status-filter\""));
    assert!(html.contains("data-fleet-row"));
    assert!(html.contains("data-search"));
    assert!(html.contains("fleet-dashboard-a"));
    assert!(html.contains("fleet-dashboard-b"));

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "control-plane",
            "serve",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "fleet-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let served_html = http_request(
        port,
        "GET /?token=fleet-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(served_html.starts_with("HTTP/1.1 200 OK"), "{served_html}");
    assert!(served_html.contains("fleet-dashboard-a"));
    assert!(served_html.contains("fleet-dashboard-b"));
    let snapshot_response = http_request(
        port,
        "GET /api/control-plane/snapshot?token=fleet-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(
        snapshot_response.starts_with("HTTP/1.1 200 OK"),
        "{snapshot_response}"
    );
    let snapshot: serde_json::Value = serde_json::from_str(http_body(&snapshot_response)).unwrap();
    assert_eq!(
        snapshot["schema_version"],
        "ao2.control-plane-fleet-snapshot.v1"
    );
    child.kill().ok();
    child.wait().ok();
}

#[test]
fn cli_control_plane_bundle_writes_portable_fleet_support_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let repo_a = temp.path().join("bundle-service-a");
    let repo_b = temp.path().join("bundle-service-b");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_a);
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_b);

    let run_a = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo_a.to_str().unwrap(),
        "--run-id",
        "fleet-bundle-a",
    ]);
    assert!(run_a.status.success(), "{}", stderr(&run_a));
    let run_b = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo_b.to_str().unwrap(),
        "--run-id",
        "fleet-bundle-b",
    ]);
    assert!(run_b.status.success(), "{}", stderr(&run_b));

    let fleet_path = temp.path().join("fleet-bundle-source.json");
    let refresh = ao2([
        "control-plane",
        "refresh",
        "--target",
        repo_a.to_str().unwrap(),
        "--target",
        repo_b.to_str().unwrap(),
        "--out",
        fleet_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(refresh.status.success(), "{}", stderr(&refresh));

    let bundle_dir = temp.path().join("bundle-out");
    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        fleet_path.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let stdout_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    assert_eq!(stdout_json["schema_version"], "ao2.control-plane-bundle.v1");
    let bundle_path = PathBuf::from(stdout_json["bundle_path"].as_str().unwrap());
    let archive_path = PathBuf::from(stdout_json["archive_path"].as_str().unwrap());
    let sha256_path = PathBuf::from(stdout_json["sha256_path"].as_str().unwrap());
    assert!(bundle_path.is_file());
    assert!(archive_path.is_file());
    assert!(sha256_path.is_file());
    let bundle_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&bundle_path).unwrap()).unwrap();
    assert_eq!(
        bundle_json["schema_version"],
        "ao2.control-plane-fleet-bundle.v1"
    );
    assert_eq!(
        bundle_json["fleet_snapshot"]["schema_version"],
        "ao2.control-plane-fleet-snapshot.v1"
    );
    assert_eq!(bundle_json["files"].as_array().unwrap().len(), 3);
    let manifest = fs::read_to_string(&sha256_path).unwrap();
    assert!(manifest.contains("fleet-snapshot.json"));
    assert!(manifest.contains("fleet-bundle.json"));
    assert_eq!(stdout_json["repository_count"], 2);
    assert_eq!(stdout_json["run_count"], 2);
}

#[test]
fn cli_control_plane_bundle_verify_checks_manifest_and_schema() {
    let temp = tempfile::tempdir().unwrap();
    let repo_a = temp.path().join("verify-bundle-service-a");
    let repo_b = temp.path().join("verify-bundle-service-b");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_a);
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_b);

    for (repo, run_id) in [
        (&repo_a, "fleet-bundle-verify-a"),
        (&repo_b, "fleet-bundle-verify-b"),
    ] {
        let run = ao2([
            "run",
            "../../examples/risky-pr-run/risky-pr.yaml",
            "--target",
            repo.to_str().unwrap(),
            "--run-id",
            run_id,
        ]);
        assert!(run.status.success(), "{}", stderr(&run));
    }

    let fleet_path = temp.path().join("fleet-bundle-verify-source.json");
    let refresh = ao2([
        "control-plane",
        "refresh",
        "--target",
        repo_a.to_str().unwrap(),
        "--target",
        repo_b.to_str().unwrap(),
        "--out",
        fleet_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(refresh.status.success(), "{}", stderr(&refresh));

    let bundle_dir = temp.path().join("bundle-verify-out");
    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        fleet_path.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let bundle_path = PathBuf::from(bundle_json["bundle_path"].as_str().unwrap());
    let bundle_stage_dir = bundle_path.parent().unwrap();

    let verify = ao2([
        "control-plane",
        "bundle-verify",
        "--bundle-dir",
        bundle_stage_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.control-plane-bundle-verify.v1"
    );
    assert_eq!(verify_json["verified"], true);
    assert_eq!(verify_json["file_count"], 2);
    assert_eq!(verify_json["repository_count"], 2);
    assert_eq!(verify_json["run_count"], 2);
}

#[test]
fn cli_control_plane_bundle_includes_health_history_and_trend_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    let bundle_dir = temp.path().join("bundle-health-out");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        unhealthy_fleet_path.to_str().unwrap(),
        "--health-history",
        health_dir.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let stdout_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    assert_eq!(stdout_json["schema_version"], "ao2.control-plane-bundle.v1");
    assert_eq!(stdout_json["health_history_entry_count"], 2);

    let bundle_path = PathBuf::from(stdout_json["bundle_path"].as_str().unwrap());
    let stage_dir = bundle_path.parent().unwrap();
    let sha256_path = PathBuf::from(stdout_json["sha256_path"].as_str().unwrap());
    assert!(stage_dir.join("health-history.json").is_file());
    assert!(stage_dir.join("health-trend.json").is_file());
    assert!(stage_dir.join("health-trend.html").is_file());
    assert!(stage_dir.join("health-entries").is_dir());

    let bundle_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&bundle_path).unwrap()).unwrap();
    assert_eq!(
        bundle_json["health_history"]["schema_version"],
        "ao2.control-plane-health-history.v1"
    );
    assert_eq!(
        bundle_json["health_trend"]["schema_version"],
        "ao2.control-plane-health-trend.v1"
    );
    assert_eq!(bundle_json["health_trend"]["trend"], "worsening");
    let roles = bundle_json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["role"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(roles.contains(&"health_history"));
    assert!(roles.contains(&"health_trend"));
    assert!(roles.contains(&"health_dashboard"));
    assert!(roles.contains(&"health_entry"));

    let manifest = fs::read_to_string(&sha256_path).unwrap();
    assert!(manifest.contains("health-history.json"));
    assert!(manifest.contains("health-trend.json"));
    assert!(manifest.contains("health-trend.html"));
    assert!(manifest.contains("health-entries/"));
}

#[test]
fn cli_control_plane_bundle_verify_checks_health_evidence_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    let bundle_dir = temp.path().join("bundle-health-verify-out");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        unhealthy_fleet_path.to_str().unwrap(),
        "--health-history",
        health_dir.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let bundle_path = PathBuf::from(bundle_json["bundle_path"].as_str().unwrap());
    let stage_dir = bundle_path.parent().unwrap();

    let verify = ao2([
        "control-plane",
        "bundle-verify",
        "--bundle-dir",
        stage_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(
        verify_json["schema_version"],
        "ao2.control-plane-bundle-verify.v1"
    );
    assert_eq!(verify_json["verified"], true);
    assert!(verify_json["file_count"].as_u64().unwrap() >= 6);
    let verified_paths = verify_json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(verified_paths.contains(&"health-history.json"));
    assert!(verified_paths.contains(&"health-trend.json"));
    assert!(verified_paths.contains(&"health-trend.html"));
}

#[test]
fn cli_control_plane_bundle_import_writes_offline_support_case_from_directory() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    let bundle_dir = temp.path().join("bundle-import-source");
    let import_dir = temp.path().join("bundle-import-cases");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        unhealthy_fleet_path.to_str().unwrap(),
        "--health-history",
        health_dir.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let bundle_path = PathBuf::from(bundle_json["bundle_path"].as_str().unwrap());
    let stage_dir = bundle_path.parent().unwrap();

    let import = ao2([
        "control-plane",
        "bundle-import",
        "--bundle-dir",
        stage_dir.to_str().unwrap(),
        "--out-dir",
        import_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(import.status.success(), "{}", stderr(&import));
    let import_json: serde_json::Value = serde_json::from_str(&stdout(&import)).unwrap();
    assert_eq!(
        import_json["schema_version"],
        "ao2.control-plane-bundle-import.v1"
    );
    assert_eq!(import_json["verified"], true);
    assert_eq!(import_json["input_kind"], "directory");
    assert_eq!(import_json["repository_count"], 1);
    assert_eq!(import_json["run_count"], 1);
    assert_eq!(import_json["health_history_entry_count"], 2);
    assert_eq!(import_json["health_trend"]["trend"], "worsening");

    let summary_path = PathBuf::from(import_json["summary_path"].as_str().unwrap());
    let index_path = PathBuf::from(import_json["index_path"].as_str().unwrap());
    assert!(summary_path.is_file());
    assert!(index_path.is_file());
    let html = fs::read_to_string(index_path).unwrap();
    assert!(html.contains("Control Plane Support Bundle"));
    assert!(html.contains("worsening"));
    assert!(html.contains("health-trend.html"));
}

#[test]
fn cli_control_plane_bundle_import_extracts_archive_and_verifies_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    let bundle_dir = temp.path().join("bundle-import-archive-source");
    let import_dir = temp.path().join("bundle-import-archive-cases");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        unhealthy_fleet_path.to_str().unwrap(),
        "--health-history",
        health_dir.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let archive_path = PathBuf::from(bundle_json["archive_path"].as_str().unwrap());

    let import = ao2([
        "control-plane",
        "bundle-import",
        "--archive",
        archive_path.to_str().unwrap(),
        "--out-dir",
        import_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(import.status.success(), "{}", stderr(&import));
    let import_json: serde_json::Value = serde_json::from_str(&stdout(&import)).unwrap();
    assert_eq!(
        import_json["schema_version"],
        "ao2.control-plane-bundle-import.v1"
    );
    assert_eq!(import_json["verified"], true);
    assert_eq!(import_json["input_kind"], "archive");
    assert!(import_json["verify"]["file_count"].as_u64().unwrap() >= 6);

    let imported_bundle_dir = PathBuf::from(import_json["bundle_dir"].as_str().unwrap());
    assert!(imported_bundle_dir.join("fleet-bundle.json").is_file());
    assert!(imported_bundle_dir.join("SHA256SUMS").is_file());
    assert!(imported_bundle_dir.join("health-trend.html").is_file());
    assert_eq!(import_json["health_trend"]["trend"], "worsening");
}

#[test]
fn cli_control_plane_bundle_inspect_summarizes_directory_without_import_case() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    let bundle_dir = temp.path().join("bundle-inspect-source");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        unhealthy_fleet_path.to_str().unwrap(),
        "--health-history",
        health_dir.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let bundle_path = PathBuf::from(bundle_json["bundle_path"].as_str().unwrap());
    let stage_dir = bundle_path.parent().unwrap();

    let inspect = ao2([
        "control-plane",
        "bundle-inspect",
        "--bundle-dir",
        stage_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(inspect.status.success(), "{}", stderr(&inspect));
    let inspect_json: serde_json::Value = serde_json::from_str(&stdout(&inspect)).unwrap();
    assert_eq!(
        inspect_json["schema_version"],
        "ao2.control-plane-bundle-inspect.v1"
    );
    assert_eq!(inspect_json["verified"], true);
    assert_eq!(inspect_json["input_kind"], "directory");
    assert_eq!(inspect_json["repository_count"], 1);
    assert_eq!(inspect_json["run_count"], 1);
    assert_eq!(inspect_json["health_history_entry_count"], 2);
    assert_eq!(inspect_json["health_trend"]["trend"], "worsening");
    assert!(inspect_json["verify"]["file_count"].as_u64().unwrap() >= 6);
    assert!(inspect_json.get("case_dir").is_none());
    assert!(inspect_json.get("summary_path").is_none());
}

#[test]
fn cli_control_plane_bundle_inspect_extracts_archive_for_read_only_summary() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    let bundle_dir = temp.path().join("bundle-inspect-archive-source");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        unhealthy_fleet_path.to_str().unwrap(),
        "--health-history",
        health_dir.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let archive_path = PathBuf::from(bundle_json["archive_path"].as_str().unwrap());

    let inspect = ao2([
        "control-plane",
        "bundle-inspect",
        "--archive",
        archive_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(inspect.status.success(), "{}", stderr(&inspect));
    let inspect_json: serde_json::Value = serde_json::from_str(&stdout(&inspect)).unwrap();
    assert_eq!(
        inspect_json["schema_version"],
        "ao2.control-plane-bundle-inspect.v1"
    );
    assert_eq!(inspect_json["verified"], true);
    assert_eq!(inspect_json["input_kind"], "archive");
    assert_eq!(inspect_json["health_trend"]["trend"], "worsening");
    assert!(inspect_json.get("case_dir").is_none());
    assert!(inspect_json.get("summary_path").is_none());

    let inspected_bundle_dir = PathBuf::from(inspect_json["bundle_dir"].as_str().unwrap());
    assert!(inspected_bundle_dir.join("fleet-bundle.json").is_file());
    assert!(inspected_bundle_dir.join("health-trend.html").is_file());
    let files = inspect_json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(files.contains(&"health-trend.html"));
}

#[test]
fn cli_control_plane_bundle_writes_signed_support_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    let bundle_dir = temp.path().join("signed-bundle-source");
    let signing_key = temp.path().join("support-signing-key.pem");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    generate_native_signing_key(&signing_key, 2048);

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        unhealthy_fleet_path.to_str().unwrap(),
        "--health-history",
        health_dir.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "support-lead",
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    assert_eq!(bundle_json["support_metadata_signed"], true);
    let bundle_path = PathBuf::from(bundle_json["bundle_path"].as_str().unwrap());
    let stage_dir = bundle_path.parent().unwrap();
    assert!(stage_dir.join("support-bundle-metadata.json").is_file());
    assert!(stage_dir.join("support-bundle-metadata.json.sig").is_file());
    assert!(stage_dir
        .join("support-bundle-signing-public.pem")
        .is_file());

    let manifest = fs::read_to_string(stage_dir.join("SHA256SUMS")).unwrap();
    assert!(manifest.contains("support-bundle-metadata.json"));
    assert!(manifest.contains("support-bundle-metadata.json.sig"));
    assert!(manifest.contains("support-bundle-signing-public.pem"));

    let inspect = ao2([
        "control-plane",
        "bundle-inspect",
        "--bundle-dir",
        stage_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(inspect.status.success(), "{}", stderr(&inspect));
    let inspect_json: serde_json::Value = serde_json::from_str(&stdout(&inspect)).unwrap();
    assert_eq!(inspect_json["support_metadata"]["present"], true);
    assert_eq!(inspect_json["support_metadata"]["signature_verified"], true);
    assert_eq!(
        inspect_json["support_metadata"]["signer_id"],
        "support-lead"
    );
    assert!(
        inspect_json["support_metadata"]["public_key_sha256"]
            .as_str()
            .unwrap()
            .len()
            >= 64
    );
}

#[test]
fn cli_control_plane_bundle_verify_reports_signed_support_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    let bundle_dir = temp.path().join("signed-bundle-verify-source");
    let signing_key = temp.path().join("support-signing-key.pem");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    generate_native_signing_key(&signing_key, 2048);

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        unhealthy_fleet_path.to_str().unwrap(),
        "--health-history",
        health_dir.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "release-ops",
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let bundle_path = PathBuf::from(bundle_json["bundle_path"].as_str().unwrap());
    let stage_dir = bundle_path.parent().unwrap();

    let verify = ao2([
        "control-plane",
        "bundle-verify",
        "--bundle-dir",
        stage_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(verify_json["verified"], true);
    assert_eq!(verify_json["support_metadata"]["present"], true);
    assert_eq!(verify_json["support_metadata"]["signature_verified"], true);
    assert_eq!(verify_json["support_metadata"]["signer_id"], "release-ops");
    assert!(
        verify_json["support_metadata"]["metadata_sha256"]
            .as_str()
            .unwrap()
            .len()
            >= 64
    );
    assert!(
        verify_json["support_metadata"]["public_key_sha256"]
            .as_str()
            .unwrap()
            .len()
            >= 64
    );
}

#[test]
fn cli_control_plane_bundle_verify_rejects_tampered_signed_metadata_after_manifest_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let stage_dir =
        create_signed_control_plane_bundle(temp.path(), "tampered-verify", "release-ops");

    tamper_support_metadata_and_refresh_manifest(&stage_dir, "attacker");

    let verify = ao2([
        "control-plane",
        "bundle-verify",
        "--bundle-dir",
        stage_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!verify.status.success(), "{}", stdout(&verify));
    assert!(
        stderr(&verify).contains("support bundle metadata signature verification failed"),
        "{}",
        stderr(&verify)
    );
}

#[test]
fn cli_control_plane_bundle_import_rejects_tampered_signed_metadata_after_manifest_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let stage_dir =
        create_signed_control_plane_bundle(temp.path(), "tampered-import", "support-lead");
    let import_dir = temp.path().join("tampered-import-cases");

    tamper_support_metadata_and_refresh_manifest(&stage_dir, "attacker");

    let import = ao2([
        "control-plane",
        "bundle-import",
        "--bundle-dir",
        stage_dir.to_str().unwrap(),
        "--out-dir",
        import_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(!import.status.success(), "{}", stdout(&import));
    assert!(
        stderr(&import).contains("support bundle metadata signature verification failed"),
        "{}",
        stderr(&import)
    );
    assert!(!import_dir.exists() || fs::read_dir(&import_dir).unwrap().next().is_none());
}

#[test]
fn cli_control_plane_bundle_import_renders_signed_trust_panel() {
    let temp = tempfile::tempdir().unwrap();
    let stage_dir = create_signed_control_plane_bundle(temp.path(), "trust-panel", "support-lead");
    let import_dir = temp.path().join("trust-panel-cases");

    let import = ao2([
        "control-plane",
        "bundle-import",
        "--bundle-dir",
        stage_dir.to_str().unwrap(),
        "--out-dir",
        import_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(import.status.success(), "{}", stderr(&import));
    let import_json: serde_json::Value = serde_json::from_str(&stdout(&import)).unwrap();
    let index_path = PathBuf::from(import_json["index_path"].as_str().unwrap());
    let public_key_sha = import_json["support_metadata"]["public_key_sha256"]
        .as_str()
        .unwrap();

    let html = fs::read_to_string(index_path).unwrap();
    assert!(html.contains("Support Bundle Trust"));
    assert!(html.contains("Signature verified"));
    assert!(html.contains("support-lead"));
    assert!(html.contains(public_key_sha));
}

#[test]
fn cli_control_plane_bundle_inspect_text_reports_signed_trust_status() {
    let temp = tempfile::tempdir().unwrap();
    let stage_dir =
        create_signed_control_plane_bundle(temp.path(), "trust-inspect", "support-lead");

    let inspect = ao2([
        "control-plane",
        "bundle-inspect",
        "--bundle-dir",
        stage_dir.to_str().unwrap(),
    ]);
    assert!(inspect.status.success(), "{}", stderr(&inspect));
    let output = stdout(&inspect);
    assert!(output.contains("support_metadata=signature_verified"));
    assert!(output.contains("signer_id=support-lead"));
}

fn create_signed_control_plane_bundle(base: &Path, name: &str, signer_id: &str) -> PathBuf {
    let healthy_fleet_path = base.join(format!("{name}-empty-fleet.json"));
    let unhealthy_fleet_path = base.join(format!("{name}-unhealthy-fleet.json"));
    let health_dir = base.join(format!("{name}-fleet-health"));
    let bundle_dir = base.join(format!("{name}-signed-bundle-source"));
    let signing_key = base.join(format!("{name}-support-signing-key.pem"));
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, base);

    generate_native_signing_key(&signing_key, 2048);

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let bundle = ao2([
        "control-plane",
        "bundle",
        "--fleet",
        unhealthy_fleet_path.to_str().unwrap(),
        "--health-history",
        health_dir.to_str().unwrap(),
        "--out-dir",
        bundle_dir.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        signer_id,
        "--json",
    ]);
    assert!(bundle.status.success(), "{}", stderr(&bundle));
    let bundle_json: serde_json::Value = serde_json::from_str(&stdout(&bundle)).unwrap();
    let bundle_path = PathBuf::from(bundle_json["bundle_path"].as_str().unwrap());
    bundle_path.parent().unwrap().to_path_buf()
}

fn tamper_support_metadata_and_refresh_manifest(stage_dir: &Path, signer_id: &str) {
    let metadata_path = stage_dir.join("support-bundle-metadata.json");
    let mut metadata: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
    metadata["signer_id"] = serde_json::json!(signer_id);
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();
    refresh_sha256sum_entry(stage_dir, "support-bundle-metadata.json");
}

fn refresh_sha256sum_entry(stage_dir: &Path, relative_path: &str) {
    let manifest_path = stage_dir.join("SHA256SUMS");
    let replacement = sha256_hex_for_test(&fs::read(stage_dir.join(relative_path)).unwrap());
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    let updated = manifest
        .lines()
        .map(|line| {
            let mut parts = line.split_whitespace();
            let _digest = parts.next().unwrap_or_default();
            let path = parts.next().unwrap_or_default();
            if path == relative_path {
                format!("{replacement}  {relative_path}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(manifest_path, format!("{updated}\n")).unwrap();
}

#[test]
fn cli_control_plane_sources_save_and_refresh_from_source_list() {
    let temp = tempfile::tempdir().unwrap();
    let repo_a = temp.path().join("sources-service-a");
    let repo_b = temp.path().join("sources-service-b");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_a);
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_b);

    for (repo, run_id) in [(&repo_a, "fleet-sources-a"), (&repo_b, "fleet-sources-b")] {
        let run = ao2([
            "run",
            "../../examples/risky-pr-run/risky-pr.yaml",
            "--target",
            repo.to_str().unwrap(),
            "--run-id",
            run_id,
        ]);
        assert!(run.status.success(), "{}", stderr(&run));
    }

    let sources_path = temp.path().join("fleet-sources.json");
    let save = ao2([
        "control-plane",
        "sources",
        "save",
        "--target",
        repo_a.to_str().unwrap(),
        "--target",
        repo_b.to_str().unwrap(),
        "--out",
        sources_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(save.status.success(), "{}", stderr(&save));
    let save_json: serde_json::Value = serde_json::from_str(&stdout(&save)).unwrap();
    assert_eq!(save_json["schema_version"], "ao2.control-plane-sources.v1");
    assert_eq!(save_json["target_count"], 2);
    assert!(sources_path.is_file());

    let fleet_path = temp.path().join("fleet-from-sources.json");
    let refresh = ao2([
        "control-plane",
        "refresh",
        "--sources",
        sources_path.to_str().unwrap(),
        "--out",
        fleet_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(refresh.status.success(), "{}", stderr(&refresh));
    let fleet: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fleet_path).unwrap()).unwrap();
    assert_eq!(
        fleet["schema_version"],
        "ao2.control-plane-fleet-snapshot.v1"
    );
    assert_eq!(fleet["repositories"].as_array().unwrap().len(), 2);
    let fleet_text = serde_json::to_string(&fleet).unwrap();
    assert!(fleet_text.contains("fleet-sources-a"));
    assert!(fleet_text.contains("fleet-sources-b"));
}

#[test]
fn cli_control_plane_history_records_refresh_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let repo_a = temp.path().join("history-service-a");
    let repo_b = temp.path().join("history-service-b");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_a);
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_b);

    for (repo, run_id) in [(&repo_a, "fleet-history-a"), (&repo_b, "fleet-history-b")] {
        let run = ao2([
            "run",
            "../../examples/risky-pr-run/risky-pr.yaml",
            "--target",
            repo.to_str().unwrap(),
            "--run-id",
            run_id,
        ]);
        assert!(run.status.success(), "{}", stderr(&run));
    }

    let fleet_path = temp.path().join("fleet-history-source.json");
    let history_dir = temp.path().join("fleet-history");
    let refresh = ao2([
        "control-plane",
        "refresh",
        "--target",
        repo_a.to_str().unwrap(),
        "--target",
        repo_b.to_str().unwrap(),
        "--out",
        fleet_path.to_str().unwrap(),
        "--history",
        history_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(refresh.status.success(), "{}", stderr(&refresh));
    let refresh_json: serde_json::Value = serde_json::from_str(&stdout(&refresh)).unwrap();
    let history_path = PathBuf::from(refresh_json["history_path"].as_str().unwrap());
    let history_entry_path = PathBuf::from(refresh_json["history_entry_path"].as_str().unwrap());
    assert!(history_path.is_file());
    assert!(history_entry_path.is_file());
    assert_eq!(history_path, history_dir.join("history.json"));
    let history: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&history_path).unwrap()).unwrap();
    assert_eq!(
        history["schema_version"],
        "ao2.control-plane-fleet-history.v1"
    );
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["repository_count"], 2);
    assert_eq!(entry["run_count"], 2);
    assert!(entry["fleet_snapshot_path"]
        .as_str()
        .unwrap()
        .contains("fleet-snapshot"));
    assert_eq!(
        entry["fleet_snapshot_sha256"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        64
    );
}

#[test]
fn cli_control_plane_history_diff_compares_latest_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let (history_dir, _, _, _) = create_two_entry_control_plane_history(temp.path());

    let diff = ao2([
        "control-plane",
        "history",
        "diff",
        "--history",
        history_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(diff.status.success(), "{}", stderr(&diff));
    let diff_json: serde_json::Value = serde_json::from_str(&stdout(&diff)).unwrap();
    assert_eq!(
        diff_json["schema_version"],
        "ao2.control-plane-history-diff.v1"
    );
    assert_eq!(diff_json["from_index"], 0);
    assert_eq!(diff_json["to_index"], 1);
    assert_eq!(diff_json["repository_count_delta"], 1);
    assert_eq!(diff_json["run_count_delta"], 1);
    assert!(diff_json["added_run_ids"]
        .as_array()
        .unwrap()
        .iter()
        .any(|run_id| run_id == "fleet-history-diff-b"));
    assert!(diff_json["removed_run_ids"].as_array().unwrap().is_empty());
}

#[test]
fn cli_control_plane_history_prune_keeps_newest_entries() {
    let temp = tempfile::tempdir().unwrap();
    let (history_dir, first_entry_path, second_entry_path, _) =
        create_two_entry_control_plane_history(temp.path());
    assert!(first_entry_path.is_file());
    assert!(second_entry_path.is_file());

    let prune = ao2([
        "control-plane",
        "history",
        "prune",
        "--history",
        history_dir.to_str().unwrap(),
        "--keep",
        "1",
        "--json",
    ]);
    assert!(prune.status.success(), "{}", stderr(&prune));
    let prune_json: serde_json::Value = serde_json::from_str(&stdout(&prune)).unwrap();
    assert_eq!(
        prune_json["schema_version"],
        "ao2.control-plane-history-prune.v1"
    );
    assert_eq!(prune_json["kept_count"], 1);
    assert_eq!(prune_json["removed_count"], 1);
    assert!(!first_entry_path.exists());
    assert!(second_entry_path.is_file());

    let history: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(history_dir.join("history.json")).unwrap())
            .unwrap();
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        PathBuf::from(entries[0]["fleet_snapshot_path"].as_str().unwrap()),
        second_entry_path
    );
}

#[test]
fn cli_control_plane_history_export_writes_static_dashboard() {
    let temp = tempfile::tempdir().unwrap();
    let (history_dir, _, _, _) = create_two_entry_control_plane_history(temp.path());
    let html_path = temp.path().join("fleet-history.html");

    let export = ao2([
        "control-plane",
        "history",
        "export",
        "--history",
        history_dir.to_str().unwrap(),
        "--out",
        html_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let export_json: serde_json::Value = serde_json::from_str(&stdout(&export)).unwrap();
    assert_eq!(
        export_json["schema_version"],
        "ao2.control-plane-history-export.v1"
    );
    assert_eq!(export_json["entry_count"], 2);
    assert!(html_path.is_file());
    let html = fs::read_to_string(&html_path).unwrap();
    assert!(html.contains("AO2 Fleet History"));
    assert!(html.contains("History Entries"));
    assert!(html.contains("fleet-snapshot.json"));
    assert!(html.contains("SHA256"));
    assert!(html.contains("fleet-history-diff-a"));
    assert!(html.contains("fleet-history-diff-b"));
}

#[test]
fn cli_control_plane_health_reports_fleet_alerts() {
    let temp = tempfile::tempdir().unwrap();
    let fleet_path = temp.path().join("unhealthy-fleet.json");
    write_unhealthy_fleet_snapshot(&fleet_path, temp.path());

    let health = ao2([
        "control-plane",
        "health",
        "--fleet",
        fleet_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(health.status.success(), "{}", stderr(&health));
    let health_json: serde_json::Value = serde_json::from_str(&stdout(&health)).unwrap();
    assert_eq!(health_json["schema_version"], "ao2.control-plane-health.v1");
    assert_eq!(health_json["status"], "warn");
    assert!(health_json["alert_count"].as_u64().unwrap() >= 3);
    let codes = health_json["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|alert| alert["code"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"run_not_accepted"));
    assert!(codes.contains(&"queue_job_not_accepted"));
    assert!(codes.contains(&"missing_evidence_pack"));
}

#[test]
fn cli_control_plane_health_records_history_entries() {
    let temp = tempfile::tempdir().unwrap();
    let fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    write_unhealthy_fleet_snapshot(&fleet_path, temp.path());

    let health = ao2([
        "control-plane",
        "health",
        "--fleet",
        fleet_path.to_str().unwrap(),
        "--record",
        health_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(health.status.success(), "{}", stderr(&health));
    let health_json: serde_json::Value = serde_json::from_str(&stdout(&health)).unwrap();
    assert_eq!(health_json["schema_version"], "ao2.control-plane-health.v1");
    assert_eq!(health_json["status"], "warn");
    assert!(health_json["alert_count"].as_u64().unwrap() > 0);

    let history_path = PathBuf::from(health_json["health_history_path"].as_str().unwrap());
    let entry_path = PathBuf::from(health_json["health_entry_path"].as_str().unwrap());
    assert_eq!(history_path, health_dir.join("health-history.json"));
    assert!(history_path.is_file());
    assert!(entry_path.is_file());

    let history: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&history_path).unwrap()).unwrap();
    assert_eq!(
        history["schema_version"],
        "ao2.control-plane-health-history.v1"
    );
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["status"], "warn");
    assert!(entries[0]["alert_count"].as_u64().unwrap() > 0);
    assert_eq!(entries[0]["health_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(
        PathBuf::from(entries[0]["health_path"].as_str().unwrap()),
        entry_path
    );
}

#[test]
fn cli_control_plane_health_trend_reports_alert_delta() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let trend = ao2([
        "control-plane",
        "health-trend",
        "--history",
        health_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(trend.status.success(), "{}", stderr(&trend));
    let trend_json: serde_json::Value = serde_json::from_str(&stdout(&trend)).unwrap();
    assert_eq!(
        trend_json["schema_version"],
        "ao2.control-plane-health-trend.v1"
    );
    assert_eq!(trend_json["entry_count"], 2);
    assert_eq!(trend_json["previous_alert_count"], 1);
    assert!(trend_json["latest_alert_count"].as_u64().unwrap() > 1);
    assert!(trend_json["alert_count_delta"].as_i64().unwrap() > 0);
    assert_eq!(trend_json["trend"], "worsening");
}

#[test]
fn cli_control_plane_health_export_writes_trend_dashboard() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    let html_path = temp.path().join("health-trend.html");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let export = ao2([
        "control-plane",
        "health-export",
        "--history",
        health_dir.to_str().unwrap(),
        "--out",
        html_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let export_json: serde_json::Value = serde_json::from_str(&stdout(&export)).unwrap();
    assert_eq!(
        export_json["schema_version"],
        "ao2.control-plane-health-export.v1"
    );
    assert_eq!(export_json["entry_count"], 2);
    assert!(html_path.is_file());
    let html = fs::read_to_string(&html_path).unwrap();
    assert!(html.contains("AO2 Fleet Health Trend"));
    assert!(html.contains("worsening"));
    assert!(html.contains("health-history.json"));
}

#[test]
fn cli_control_plane_health_prune_keeps_newest_entries() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    let mut recorded_paths = Vec::new();
    for fleet_path in [
        &healthy_fleet_path,
        &unhealthy_fleet_path,
        &healthy_fleet_path,
    ] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
        let health_json: serde_json::Value = serde_json::from_str(&stdout(&health)).unwrap();
        recorded_paths.push(PathBuf::from(
            health_json["health_entry_path"].as_str().unwrap(),
        ));
    }

    let prune = ao2([
        "control-plane",
        "health-prune",
        "--history",
        health_dir.to_str().unwrap(),
        "--keep",
        "1",
        "--json",
    ]);
    assert!(prune.status.success(), "{}", stderr(&prune));
    let prune_json: serde_json::Value = serde_json::from_str(&stdout(&prune)).unwrap();
    assert_eq!(
        prune_json["schema_version"],
        "ao2.control-plane-health-prune.v1"
    );
    assert_eq!(prune_json["removed_count"], 2);
    assert_eq!(prune_json["kept_count"], 1);

    assert!(!recorded_paths[0].exists());
    assert!(!recorded_paths[1].exists());
    assert!(recorded_paths[2].exists());

    let history_path = health_dir.join("health-history.json");
    let history: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&history_path).unwrap()).unwrap();
    let entries = history["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        PathBuf::from(entries[0]["health_path"].as_str().unwrap()),
        recorded_paths[2]
    );
}

#[test]
fn cli_control_plane_fleet_dashboard_and_api_expose_health() {
    let temp = tempfile::tempdir().unwrap();
    let fleet_path = temp.path().join("unhealthy-fleet.json");
    write_unhealthy_fleet_snapshot(&fleet_path, temp.path());
    let html_path = temp.path().join("fleet-dashboard.html");

    let export = ao2([
        "control-plane",
        "export",
        "--fleet",
        fleet_path.to_str().unwrap(),
        "--out",
        html_path.to_str().unwrap(),
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let html = fs::read_to_string(&html_path).unwrap();
    assert!(html.contains("Fleet Health"));
    assert!(html.contains("run_not_accepted"));
    assert!(html.contains("missing_evidence_pack"));

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "control-plane",
            "serve",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "cp-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let response = http_request(
        port,
        "GET /api/control-plane/health?token=cp-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let health_json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(health_json["schema_version"], "ao2.control-plane-health.v1");
    assert_eq!(health_json["status"], "warn");
    child.kill().ok();
    child.wait().ok();
}

#[test]
fn cli_control_plane_fleet_dashboard_and_api_expose_health_trend() {
    let temp = tempfile::tempdir().unwrap();
    let healthy_fleet_path = temp.path().join("empty-fleet.json");
    let unhealthy_fleet_path = temp.path().join("unhealthy-fleet.json");
    let health_dir = temp.path().join("fleet-health");
    let html_path = temp.path().join("fleet-dashboard.html");
    write_empty_fleet_snapshot(&healthy_fleet_path);
    write_unhealthy_fleet_snapshot(&unhealthy_fleet_path, temp.path());

    for fleet_path in [&healthy_fleet_path, &unhealthy_fleet_path] {
        let health = ao2([
            "control-plane",
            "health",
            "--fleet",
            fleet_path.to_str().unwrap(),
            "--record",
            health_dir.to_str().unwrap(),
            "--json",
        ]);
        assert!(health.status.success(), "{}", stderr(&health));
    }

    let export = ao2([
        "control-plane",
        "export",
        "--fleet",
        unhealthy_fleet_path.to_str().unwrap(),
        "--health-history",
        health_dir.to_str().unwrap(),
        "--out",
        html_path.to_str().unwrap(),
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let html = fs::read_to_string(&html_path).unwrap();
    assert!(html.contains("Fleet Health Trend"));
    assert!(html.contains("worsening"));
    assert!(html.contains("health-history.json"));

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "control-plane",
            "serve",
            "--fleet",
            unhealthy_fleet_path.to_str().unwrap(),
            "--health-history",
            health_dir.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "cp-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let response = http_request(
        port,
        "GET /api/control-plane/health-trend?token=cp-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let trend_json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        trend_json["schema_version"],
        "ao2.control-plane-health-trend.v1"
    );
    assert_eq!(trend_json["entry_count"], 2);
    assert_eq!(trend_json["trend"], "worsening");
    child.kill().ok();
    child.wait().ok();
}

fn write_unhealthy_fleet_snapshot(path: &Path, base: &Path) {
    let missing_evidence = base.join("missing-evidence-pack.json");
    let run = serde_json::json!({
        "run_id": "unhealthy-run",
        "status": "rejected",
        "workflow_id": "risk-check",
        "digest_failures": 1,
        "evidence_pack": missing_evidence,
        "cockpit": ""
    });
    let queue_job = serde_json::json!({
        "job_id": "job-unhealthy-run",
        "run_id": "unhealthy-run",
        "template": "bug-fix",
        "provider": "scripted",
        "provider_prompt_file": "",
        "max_repair_attempts": 1,
        "retry_of": "",
        "status": "failed",
        "evidence_pack": "",
        "cockpit": "",
        "stdout_log": "",
        "stderr_log": "",
        "queued_at_ms": 1,
        "started_at_ms": 2,
        "finished_at_ms": 3,
        "queue_wait_ms": 1,
        "duration_ms": 1,
        "exit_code": 1,
        "retry_count": 0,
        "error": "scripted failure"
    });
    let snapshot = serde_json::json!({
        "schema_version": "ao2.control-plane-snapshot.v1",
        "generated_at_ms": 1,
        "target": base.join("service-a"),
        "snapshot_path": base.join("service-a/.ao2/control-plane/snapshot.json"),
        "runs": {
            "schema_version": "ao2.runs-list.v1",
            "runs": [run]
        },
        "queue": {
            "schema_version": "ao2.workbench-queue-file.v1",
            "jobs": [queue_job]
        },
        "audit_events": [],
        "evidence_packs": [missing_evidence]
    });
    let repository = serde_json::json!({
        "target": base.join("service-a"),
        "snapshot_path": base.join("service-a/.ao2/control-plane/snapshot.json"),
        "run_count": 1,
        "queue_job_count": 1,
        "audit_event_count": 0,
        "evidence_pack_count": 1,
        "snapshot": snapshot
    });
    let fleet = serde_json::json!({
        "schema_version": "ao2.control-plane-fleet-snapshot.v1",
        "generated_at_ms": 1,
        "repositories": [repository],
        "totals": {
            "repository_count": 1,
            "run_count": 1,
            "queue_job_count": 1,
            "audit_event_count": 0,
            "evidence_pack_count": 1
        }
    });
    fs::write(path, serde_json::to_string_pretty(&fleet).unwrap()).unwrap();
}

fn write_empty_fleet_snapshot(path: &Path) {
    let fleet = serde_json::json!({
        "schema_version": "ao2.control-plane-fleet-snapshot.v1",
        "generated_at_ms": 1,
        "repositories": [],
        "totals": {
            "repository_count": 0,
            "run_count": 0,
            "queue_job_count": 0,
            "audit_event_count": 0,
            "evidence_pack_count": 0
        }
    });
    fs::write(path, serde_json::to_string_pretty(&fleet).unwrap()).unwrap();
}

fn create_two_entry_control_plane_history(base: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let repo_a = base.join("history-diff-service-a");
    let repo_b = base.join("history-diff-service-b");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_a);
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo_b);

    for (repo, run_id) in [
        (&repo_a, "fleet-history-diff-a"),
        (&repo_b, "fleet-history-diff-b"),
    ] {
        let run = ao2([
            "run",
            "../../examples/risky-pr-run/risky-pr.yaml",
            "--target",
            repo.to_str().unwrap(),
            "--run-id",
            run_id,
        ]);
        assert!(run.status.success(), "{}", stderr(&run));
    }

    let history_dir = base.join("fleet-history-ops");
    let first_fleet_path = base.join("fleet-history-first.json");
    let first_refresh = ao2([
        "control-plane",
        "refresh",
        "--target",
        repo_a.to_str().unwrap(),
        "--out",
        first_fleet_path.to_str().unwrap(),
        "--history",
        history_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(first_refresh.status.success(), "{}", stderr(&first_refresh));
    let first_json: serde_json::Value = serde_json::from_str(&stdout(&first_refresh)).unwrap();
    let first_entry_path = PathBuf::from(first_json["history_entry_path"].as_str().unwrap());

    let second_fleet_path = base.join("fleet-history-second.json");
    let second_refresh = ao2([
        "control-plane",
        "refresh",
        "--target",
        repo_a.to_str().unwrap(),
        "--target",
        repo_b.to_str().unwrap(),
        "--out",
        second_fleet_path.to_str().unwrap(),
        "--history",
        history_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        second_refresh.status.success(),
        "{}",
        stderr(&second_refresh)
    );
    let second_json: serde_json::Value = serde_json::from_str(&stdout(&second_refresh)).unwrap();
    let second_entry_path = PathBuf::from(second_json["history_entry_path"].as_str().unwrap());

    (
        history_dir,
        first_entry_path,
        second_entry_path,
        second_fleet_path,
    )
}
