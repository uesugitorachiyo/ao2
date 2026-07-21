use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;

const TEST_HTTP_ACCEPT_ATTEMPTS: usize = 600;

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

fn read_test_http_request(stream: &mut TcpStream, buffer: &mut [u8]) -> usize {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    stream.read(buffer).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn write_provider_cost_ledger_fixture(
    dir: &Path,
    provider: &str,
    run_id: &str,
    max_budget_usd: f64,
    provider_enforced: bool,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    cost_usd: Option<f64>,
) {
    fs::create_dir_all(dir).unwrap();
    let evidence_pack = dir.join(format!("{run_id}-evidence-pack.json"));
    fs::write(
        &evidence_pack,
        serde_json::json!({
            "schema_version": "ao2.evidence-pack.v1",
            "run_id": run_id,
            "provider_summaries": [{
                "provider": provider,
                "changed_files": ["discount_service/discounts.py"],
                "concerns": [],
                "blockers": [],
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                    "total_tokens": total_tokens
                },
                "cost_usd": cost_usd,
                "raw_summary": "provider fixed discount validation"
            }]
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        dir.join("provider-pilot-acceptance.json"),
        serde_json::json!({
            "schema_version": format!("ao2.{}-provider-pilot-acceptance.v1", provider),
            "status": "passed",
            "provider": provider,
            "run_id": run_id,
            "evidence_pack": evidence_pack,
            "cockpit": dir.join("cockpit").join("index.html"),
            "budget": {
                "max_budget_usd": max_budget_usd,
                "provider_enforced": provider_enforced,
                "timeout_seconds": 900,
                "max_repair_attempts": 1
            },
            "replay": {
                "status": "accepted",
                "event_count": 31,
                "artifact_count": 12,
                "digest_failures": []
            },
            "score": {
                "schema": "ao2.provider-evidence-scorecard.v1",
                "score": 100,
                "max_score": 100,
                "verdict": "ready",
                "run_id": run_id,
                "replay": {
                    "status": "accepted",
                    "digest_failures": 0
                }
            }
        })
        .to_string(),
    )
    .unwrap();
}

fn ao2<const N: usize>(args: [&str; N]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
    command.args(args);
    command.env("AO2_AUTO_APPROVE_SANDBOX_PATCH", "1");
    command.env(
        "AO2_AUTO_APPROVE_SANDBOX_PATCH_APPROVER",
        "human:test-auto-approve",
    );
    command.env_remove("OPENAI_API_KEY");
    command.env_remove("ANTHROPIC_API_KEY");
    command.output().unwrap()
}

fn ao2_without_auto_approval_identity<const N: usize>(args: [&str; N]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ao2"));
    command.args(args);
    command.env("AO2_AUTO_APPROVE_SANDBOX_PATCH", "1");
    command.env_remove("AO2_AUTO_APPROVE_SANDBOX_PATCH_APPROVER");
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

fn write_fake_codex(bin: &Path) {
    fs::create_dir_all(bin).unwrap();
    let unix = bin.join("codex");
    fs::write(
        &unix,
        r#"#!/bin/sh
set -eu
if [ "${1:-}" = "--version" ]; then
  printf "codex fake 0.0.0\n"
  exit 0
fi
mkdir -p discount_service
cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf "Summary: fake Codex provider smoke added validation around discount math\n"
printf "Changed files: discount_service/discounts.py\n"
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&unix, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        bin.join("codex.cmd"),
        r#"@echo off
if "%1"=="--version" (
  echo codex fake 0.0.0
  exit /b 0
)
if not exist discount_service mkdir discount_service
(
echo def calculate_discount(price: float, discount_rate: float^) -^> float:
echo     if price ^< 0:
echo         raise ValueError("price must be non-negative"^)
echo     if discount_rate ^< 0 or discount_rate ^> 1:
echo         raise ValueError("discount_rate must be between 0 and 1"^)
echo     return price * (1 - discount_rate^)
) > discount_service\discounts.py
echo Summary: fake Codex provider smoke added validation around discount math
echo Changed files: discount_service/discounts.py
"#,
    )
    .unwrap();
}

fn write_fake_claude(bin: &Path) {
    fs::create_dir_all(bin).unwrap();
    let unix = bin.join("claude");
    fs::write(
        &unix,
        r#"#!/bin/sh
set -eu
if [ "${1:-}" = "--version" ]; then
  printf "claude fake 0.0.0\n"
  exit 0
fi
mkdir -p discount_service
cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf "Summary: fake Claude provider smoke added validation around discount math\n"
printf "Changed files: discount_service/discounts.py\n"
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&unix, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(
        bin.join("claude.cmd"),
        r#"@echo off
if "%1"=="--version" (
  echo claude fake 0.0.0
  exit /b 0
)
if not exist discount_service mkdir discount_service
(
echo def calculate_discount(price: float, discount_rate: float^) -^> float:
echo     if price ^< 0:
echo         raise ValueError("price must be non-negative"^)
echo     if discount_rate ^< 0 or discount_rate ^> 1:
echo         raise ValueError("discount_rate must be between 0 and 1"^)
echo     return price * (1 - discount_rate^)
) > discount_service\discounts.py
echo Summary: fake Claude provider smoke added validation around discount math
echo Changed files: discount_service/discounts.py
"#,
    )
    .unwrap();
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

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn cli_provider_matrix_reports_contract_and_timeout() {
    let matrix = ao2(["provider", "matrix", "--json"]);
    assert!(matrix.status.success(), "{}", stderr(&matrix));
    let json: serde_json::Value = serde_json::from_str(&stdout(&matrix)).unwrap();

    assert_eq!(json["schema"], "ao2.provider-readiness-matrix.v1");
    assert_eq!(json["default_timeout_seconds"], 900);
    assert_eq!(json["providers"].as_array().unwrap().len(), 4);

    let scripted = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "scripted")
        .expect("scripted provider should be present");
    assert_eq!(scripted["doctor"]["available"], true);
    assert_eq!(
        scripted["execution_boundary"],
        "sandbox_copy_then_digest_patch"
    );
    assert_eq!(scripted["timeout_seconds"], 900);
    assert!(scripted["transcript_fields"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("changed_files")));
    assert!(scripted["policy_invariants"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!(
            "provider cannot write target repo directly"
        )));

    let codex = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "codex")
        .expect("codex provider should be present");
    assert_eq!(codex["metadata_source"], "ao2-adapter-codex");
    assert_eq!(codex["doctor"]["metadata_source"], "ao2-adapter-codex");

    let claude = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "claude")
        .expect("claude provider should be present");
    assert_eq!(claude["metadata_source"], "ao2-adapter-claude");
    assert_eq!(claude["doctor"]["metadata_source"], "ao2-adapter-claude");
}

#[test]
fn cli_provider_registry_exposes_phase2_plugin_contracts() {
    let registry = ao2(["provider", "registry", "--json"]);
    assert!(registry.status.success(), "{}", stderr(&registry));
    let json: serde_json::Value = serde_json::from_str(&stdout(&registry)).unwrap();

    assert_eq!(json["schema"], "ao2.provider-plugin-registry.v1");
    assert_eq!(json["phase"], "phase_2_registry_groundwork");
    assert_eq!(json["trust_boundary"]["execution_owner"], "ao2-local-cli");
    assert_eq!(json["providers"].as_array().unwrap().len(), 4);

    let codex = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "codex")
        .expect("codex provider should be present");
    assert_eq!(codex["contract"]["same_contract_as"], "scripted");
    assert_eq!(codex["crate"], "ao2-adapter-codex");
    assert_eq!(codex["metadata_source"], "ao2-adapter-codex");
    assert_eq!(codex["doctor"]["metadata_source"], "ao2-adapter-codex");
    assert_eq!(codex["guards"]["explicit_live_env"], "AO2_LIVE_CODEX_PILOT");
    assert!(codex["extension_slots"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("factory_hermes_bridge")));

    let claude = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "claude")
        .expect("claude provider should be present");
    assert_eq!(claude["crate"], "ao2-adapter-claude");
    assert_eq!(claude["metadata_source"], "ao2-adapter-claude");
    assert_eq!(claude["doctor"]["metadata_source"], "ao2-adapter-claude");

    assert!(json["lifecycle_gates"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!(
            "provider contract --verify --require codex"
        )));
    assert!(json["phase2_deferred_features"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!(
            "additional provider adapters as separate crates"
        )));
}

#[test]
fn cli_provider_registry_publishes_to_control_plane() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= TEST_HTTP_ACCEPT_ATTEMPTS,
                        "timed out waiting for provider registry publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 32768];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/provider/registry HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request.contains("\"schema\":\"ao2.provider-plugin-registry.v1\""));
        assert!(request.contains("\"execution_owner\":\"ao2-local-cli\""));
        assert!(request.contains("\"provider\":\"codex\""));
        assert!(request.contains("\"explicit_live_env\":\"AO2_LIVE_CODEX_PILOT\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"registry123","stored_at":"2026-05-21T00:00:00Z","ingested_schema_version":"ao2.provider-plugin-registry.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "provider",
        "registry",
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let json: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.provider-registry-control-plane-publish.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/provider/registry")
    );
    assert_eq!(
        json["receipt"]["ingested_schema_version"],
        "ao2.provider-plugin-registry.v1"
    );
    assert_eq!(
        json["dashboard_url"],
        format!("http://127.0.0.1:{port}/api/v1/provider/registry/dashboard")
    );
}

#[test]
fn cli_provider_registry_signs_before_control_plane_publish_when_key_is_provided() {
    let temp = tempfile::tempdir().unwrap();
    let signing_key = temp.path().join("provider-registry-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= TEST_HTTP_ACCEPT_ATTEMPTS,
                        "timed out waiting for signed provider registry publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 65536];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/provider/registry/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(
            request.contains("\"schema_version\":\"ao2.cp-provider-registry-signed-upload.v1\"")
        );
        assert!(request.contains("\"schema\":\"ao2.provider-plugin-registry.v1\""));
        assert!(request.contains("\"schema_version\":\"ao2.cp-provider-registry-signature.v1\""));
        assert!(request.contains("\"signature_algorithm\":\"RSA/SHA-256\""));
        assert!(request.contains("\"signature_hex\""));
        assert!(request.contains("\"public_key_sha256\""));
        assert!(request.contains("\"public_key_pem\""));
        assert!(request.contains("\"signer_id\":\"registry-lead\""));
        let request_body = request
            .split("\r\n\r\n")
            .nth(1)
            .expect("signed provider registry request has body");
        let upload: serde_json::Value = serde_json::from_str(request_body).unwrap();
        let registry_b64 = upload["registry_b64"]
            .as_str()
            .expect("signed provider registry upload carries exact registry_b64 bytes");
        {
            use base64::prelude::{Engine as _, BASE64_STANDARD};
            let decoded = BASE64_STANDARD.decode(registry_b64).unwrap();
            let expected = serde_json::to_string_pretty(&upload["registry"]).unwrap();
            assert_eq!(decoded, expected.as_bytes());
        }
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"signedregistry123","stored_at":"2026-05-21T00:00:00Z","ingested_schema_version":"ao2.provider-plugin-registry.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "provider",
        "registry",
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "registry-lead",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let json: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.provider-registry-control-plane-publish.v1"
    );
    assert_eq!(json["signed"], true);
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/provider/registry/signed")
    );
    assert_eq!(
        json["signature"]["schema_version"],
        "ao2.cp-provider-registry-signature.v1"
    );
    assert_eq!(json["signature"]["signer_id"], "registry-lead");
    assert_eq!(
        json["detail_url"],
        format!("http://127.0.0.1:{port}/api/v1/provider/registry/signedregistry123/detail")
    );
}

#[test]
fn cli_provider_registry_publish_reads_api_token_from_env_without_printing_secret() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= TEST_HTTP_ACCEPT_ATTEMPTS,
                        "timed out waiting for provider registry publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 32768];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/provider/registry HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-registry-token"));
        assert!(request.contains("\"schema\":\"ao2.provider-plugin-registry.v1\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"registryenv123","stored_at":"2026-05-21T00:00:00Z","ingested_schema_version":"ao2.provider-plugin-registry.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2_with_env(
        [
            "provider",
            "registry",
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_TEST_REGISTRY_CP_TOKEN",
            "--json",
        ],
        [("AO2_TEST_REGISTRY_CP_TOKEN", "env-registry-token")],
    );
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    let stderr = stderr(&publish);
    assert!(!stdout.contains("env-registry-token"));
    assert!(!stderr.contains("env-registry-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.provider-registry-control-plane-publish.v1"
    );
    assert_eq!(json["receipt"]["sha256"], "registryenv123");
}

#[test]
fn cli_provider_contract_reports_codex_phase_one_boundary() {
    let contract = ao2(["provider", "contract", "--provider", "codex", "--json"]);
    assert!(contract.status.success(), "{}", stderr(&contract));
    let json: serde_json::Value = serde_json::from_str(&stdout(&contract)).unwrap();

    assert_eq!(json["schema"], "ao2.provider-contract.v1");
    assert_eq!(json["provider"], "codex");
    assert_eq!(json["phase"], "phase_1");
    assert_eq!(json["same_contract_as"], "scripted");
    assert_eq!(json["execution_boundary"], "sandbox_copy_then_digest_patch");
    assert_eq!(
        json["side_effect_boundary"],
        "target mutation only through exact digest patch apply"
    );
    assert_eq!(json["live_execution_guard_env"], "AO2_LIVE_CODEX_SMOKE");
    assert_eq!(json["prompt_command"]["command"], "codex");
    assert_eq!(json["prompt_command"]["timeout_seconds"], 900);
    assert!(json["prompt_command"]["args"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("exec")));
    assert!(json["prompt_command"]["args"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("workspace-write")));
    assert!(json["policy_invariants"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!(
            "provider cannot write target repo directly"
        )));
}

#[test]
fn cli_provider_contract_verify_gates_required_codex() {
    let verify = ao2([
        "provider",
        "contract",
        "--verify",
        "--require",
        "codex",
        "--json",
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();

    assert_eq!(json["schema"], "ao2.provider-contract-verification.v1");
    assert_eq!(json["status"], "verified");
    assert_eq!(json["required_providers"], serde_json::json!(["codex"]));
    assert_eq!(json["contracts"][0]["provider"], "codex");
    assert_eq!(json["contracts"][0]["phase"], "phase_1");
    assert_eq!(json["contracts"][0]["same_contract_as"], "scripted");
    assert!(json["reasons"].as_array().unwrap().is_empty());
}

#[test]
fn cli_provider_contract_verify_fails_closed_for_unknown_provider() {
    let verify = ao2([
        "provider",
        "contract",
        "--verify",
        "--require",
        "bogus",
        "--json",
    ]);
    assert!(!verify.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&verify)).unwrap();

    assert_eq!(json["schema"], "ao2.provider-contract-verification.v1");
    assert_eq!(json["status"], "failed");
    assert_eq!(json["required_providers"], serde_json::json!(["bogus"]));
    assert!(json["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| { reason["provider"] == "bogus" && reason["code"] == "unknown_provider" }));
}

#[test]
fn cli_provider_smoke_all_reports_scripted_ready() {
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
    let json: serde_json::Value = serde_json::from_str(&stdout(&smoke)).unwrap();

    assert_eq!(json["schema"], "ao2.provider-smoke-all.v1");
    assert_eq!(json["minimum_score"], 90);
    let scripted = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "scripted")
        .expect("scripted provider should be present");
    assert_eq!(scripted["available"], true);
    assert_eq!(scripted["verdict"], "ready");
    assert!(scripted["run_id"]
        .as_str()
        .unwrap()
        .starts_with("provider-smoke-"));
    assert_eq!(
        scripted["scorecard"]["schema"],
        "ao2.provider-evidence-scorecard.v1"
    );
    assert!(scripted["score"].as_u64().unwrap() >= 90);
}

#[test]
fn cli_provider_smoke_all_persists_history() {
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
    let json: serde_json::Value = serde_json::from_str(&stdout(&smoke)).unwrap();

    assert_eq!(json["schema"], "ao2.provider-smoke-all.v1");
    let history_path = repo
        .join(".ao2")
        .join("provider-smoke")
        .join("history.json");
    assert_eq!(
        PathBuf::from(json["history_path"].as_str().unwrap()),
        history_path
    );
    assert!(history_path.is_file());
    let history: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(history_path).unwrap()).unwrap();
    assert_eq!(history["schema"], "ao2.provider-smoke-history.v1");
    assert_eq!(history["entry_count"], 1);
    assert_eq!(history["latest"]["schema"], "ao2.provider-smoke-all.v1");
    let scripted = history["latest"]["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "scripted")
        .expect("scripted provider should be present");
    assert_eq!(scripted["verdict"], "ready");
    assert!(scripted["score"].as_u64().unwrap() >= 90);
}

#[test]
fn cli_provider_gate_fails_without_smoke_history() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let gate = ao2([
        "provider",
        "gate",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);

    assert!(!gate.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&gate)).unwrap();
    assert_eq!(json["schema"], "ao2.provider-readiness-gate.v1");
    assert_eq!(json["verdict"], "not_ready");
    assert_eq!(json["required_providers"], serde_json::json!(["scripted"]));
    assert!(json["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == "missing_history"));
}

#[test]
fn cli_provider_gate_passes_after_scripted_smoke() {
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

    let gate = ao2([
        "provider",
        "gate",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);

    assert!(gate.status.success(), "{}", stderr(&gate));
    let json: serde_json::Value = serde_json::from_str(&stdout(&gate)).unwrap();
    assert_eq!(json["schema"], "ao2.provider-readiness-gate.v1");
    assert_eq!(json["verdict"], "ready");
    assert_eq!(json["required_providers"], serde_json::json!(["scripted"]));
    let scripted = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "scripted")
        .expect("scripted provider gate entry should be present");
    assert_eq!(scripted["verdict"], "ready");
    assert!(scripted["score"].as_u64().unwrap() >= 90);
}

#[test]
fn cli_provider_gate_requires_live_provider_when_requested() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_codex(&bin);
    let path = prepend_path(&bin);

    let smoke = ao2([
        "provider",
        "smoke-all",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(smoke.status.success(), "{}", stderr(&smoke));

    let missing_codex = ao2([
        "provider",
        "gate",
        "--target",
        repo.to_str().unwrap(),
        "--require",
        "codex",
        "--json",
    ]);
    assert!(!missing_codex.status.success());
    let missing_json: serde_json::Value = serde_json::from_str(&stdout(&missing_codex)).unwrap();
    assert_eq!(missing_json["verdict"], "not_ready");
    assert!(missing_json["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason["code"] == "provider_not_ready" && reason["provider"] == "codex"));

    let live_smoke = ao2_with_env(
        [
            "provider",
            "smoke-all",
            "--target",
            repo.to_str().unwrap(),
            "--live-provider",
            "codex",
            "--json",
        ],
        [("PATH", path.as_str()), ("AO2_LIVE_CODEX_SMOKE", "1")],
    );
    assert!(live_smoke.status.success(), "{}", stderr(&live_smoke));

    let codex_gate = ao2([
        "provider",
        "gate",
        "--target",
        repo.to_str().unwrap(),
        "--require",
        "codex",
        "--json",
    ]);
    assert!(codex_gate.status.success(), "{}", stderr(&codex_gate));
    let codex_json: serde_json::Value = serde_json::from_str(&stdout(&codex_gate)).unwrap();
    assert_eq!(codex_json["verdict"], "ready");
    assert_eq!(
        codex_json["required_providers"],
        serde_json::json!(["codex"])
    );
    let codex = codex_json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "codex")
        .expect("codex provider gate entry should be present");
    assert_eq!(codex["verdict"], "ready");
    assert!(codex["score"].as_u64().unwrap() >= 90);
}

#[test]
fn cli_provider_pilot_blocks_when_gate_not_ready() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

    let pilot = ao2([
        "provider",
        "pilot",
        "--target",
        repo.to_str().unwrap(),
        "--provider",
        "codex",
        "--provider-prompt-file",
        prompt.to_str().unwrap(),
        "--provider-max-budget-usd",
        "0.20",
        "--json",
    ]);

    assert!(!pilot.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&pilot)).unwrap();
    assert_eq!(json["schema"], "ao2.provider-pilot-plan.v1");
    assert_eq!(json["status"], "blocked");
    assert_eq!(json["provider"], "codex");
    assert_eq!(json["gate"]["schema"], "ao2.provider-readiness-gate.v1");
    assert_eq!(json["gate"]["verdict"], "not_ready");
    assert_eq!(json["command"], serde_json::json!([]));
    assert_eq!(json["shell_command"], "");
    let readiness = &json["readiness_recovery"];
    assert_eq!(readiness["schema"], "ao2.provider-readiness-recovery.v1");
    assert_eq!(readiness["provider"], "codex");
    assert_eq!(readiness["status"], "blocked_until_live_smoke_passes");
    assert_eq!(readiness["guard_env"], "AO2_LIVE_CODEX_SMOKE");
    assert_eq!(readiness["minimum_score"], 90);
    assert_eq!(
        readiness["history_path"],
        repo.join(".ao2")
            .join("provider-smoke")
            .join("history.json")
            .display()
            .to_string()
    );
    assert_eq!(
        readiness["next_action"],
        "run the smoke_command with the guard_env set to 1, then re-run provider pilot"
    );
    let smoke_command = readiness["smoke_command"].as_array().unwrap();
    assert_eq!(
        smoke_command,
        &vec![
            serde_json::json!("ao2"),
            serde_json::json!("provider"),
            serde_json::json!("smoke-all"),
            serde_json::json!("--target"),
            serde_json::json!(repo.display().to_string()),
            serde_json::json!("--live-provider"),
            serde_json::json!("codex"),
            serde_json::json!("--minimum-score"),
            serde_json::json!("90"),
            serde_json::json!("--json"),
        ]
    );
    let smoke_shell = readiness["smoke_shell_command"].as_str().unwrap();
    assert_eq!(smoke_shell, readiness["smoke_posix_shell_command"]);
    assert!(smoke_shell.contains("AO2_LIVE_CODEX_SMOKE=1"));
    assert!(smoke_shell.contains("ao2 provider smoke-all"));
    assert!(smoke_shell.contains("--live-provider codex"));
    assert!(smoke_shell.contains("--minimum-score 90"));
    let smoke_powershell = readiness["smoke_powershell_command"].as_str().unwrap();
    assert!(smoke_powershell.contains("$env:AO2_LIVE_CODEX_SMOKE='1';"));
    assert!(smoke_powershell.contains("ao2 provider smoke-all"));
    assert!(smoke_powershell.contains("--live-provider codex"));
    assert!(smoke_powershell.contains("--minimum-score 90"));
}

#[test]
fn cli_provider_pilot_builds_command_after_gate_passes() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_codex(&bin);
    let path = prepend_path(&bin);
    let prompt = temp.path().join("pilot-prompt.txt");
    fs::write(&prompt, "Fix the discount validation bug.\n").unwrap();

    let smoke = ao2_with_env(
        [
            "provider",
            "smoke-all",
            "--target",
            repo.to_str().unwrap(),
            "--live-provider",
            "codex",
            "--json",
        ],
        [("PATH", path.as_str()), ("AO2_LIVE_CODEX_SMOKE", "1")],
    );
    assert!(smoke.status.success(), "{}", stderr(&smoke));

    let pilot = ao2([
        "provider",
        "pilot",
        "--target",
        repo.to_str().unwrap(),
        "--provider",
        "codex",
        "--provider-prompt-file",
        prompt.to_str().unwrap(),
        "--provider-max-budget-usd",
        "0.20",
        "--json",
    ]);

    assert!(pilot.status.success(), "{}", stderr(&pilot));
    let json: serde_json::Value = serde_json::from_str(&stdout(&pilot)).unwrap();
    assert_eq!(json["schema"], "ao2.provider-pilot-plan.v1");
    assert_eq!(json["status"], "ready");
    assert_eq!(json["mode"], "command_preview");
    assert_eq!(json["provider"], "codex");
    assert_eq!(json["template"], "bug-fix");
    assert_eq!(json["max_budget_usd"], 0.20);
    assert_eq!(json["gate"]["verdict"], "ready");
    assert_eq!(json["provider_prompt_file"], prompt.display().to_string());
    let workflow = PathBuf::from(json["workflow"].as_str().unwrap());
    assert!(workflow.is_file());
    assert_eq!(
        workflow,
        repo.join(".ao2")
            .join("generated-workflows")
            .join("bug-fix.yaml")
    );
    let command = json["command"].as_array().unwrap();
    assert!(command.contains(&serde_json::json!("ao2")));
    assert!(command.contains(&serde_json::json!("run")));
    assert!(command.contains(&serde_json::json!("--template")));
    assert!(command.contains(&serde_json::json!("bug-fix")));
    assert!(command.contains(&serde_json::json!("--provider")));
    assert!(command.contains(&serde_json::json!("codex")));
    assert!(command.contains(&serde_json::json!("--provider-prompt-file")));
    assert!(command.contains(&serde_json::json!(prompt.display().to_string())));
    assert!(command.contains(&serde_json::json!("--provider-max-budget-usd")));
    assert!(command.contains(&serde_json::json!("0.20")));
    let approval = &json["approval_packet"];
    assert_eq!(approval["schema_version"], "ao2.provider-pilot-approval.v1");
    assert_eq!(approval["status"], "approval_required");
    assert_eq!(approval["approval_mode"], "exact_action_digest");
    assert_eq!(approval["required_form_field"], "approval_action_digest");
    assert_eq!(approval["provider"], "codex");
    assert_eq!(approval["explicit_live_env"], "AO2_LIVE_CODEX_PILOT");
    assert_eq!(approval["max_budget_usd"], 0.20);
    assert_eq!(
        approval["next_action"],
        "submit approval_action_digest with the exact action_digest to start this provider pilot"
    );
    assert_eq!(approval["action_digest"].as_str().unwrap().len(), 64);
    let shell_command = json["shell_command"].as_str().unwrap();
    assert!(shell_command.contains("ao2 run --template bug-fix"));
    assert!(shell_command.contains("--provider codex"));
    assert!(shell_command.contains("--provider-max-budget-usd 0.20"));
    assert!(shell_command.contains("--provider-prompt-file"));
}

#[test]
fn cli_provider_cost_ledger_aggregates_acceptance_budget_usage_and_costs() {
    let temp = tempfile::tempdir().unwrap();
    let acceptance_root = temp.path().join("acceptance-root");
    write_provider_cost_ledger_fixture(
        &acceptance_root.join("v0.4.67"),
        "codex",
        "live-codex-provider-pilot",
        1.00,
        false,
        Some(1000),
        Some(500),
        Some(1500),
        Some(0.12),
    );
    write_provider_cost_ledger_fixture(
        &acceptance_root.join("v0.4.67").join("claude"),
        "claude",
        "live-claude-provider-pilot",
        1.00,
        true,
        Some(2000),
        Some(750),
        Some(2750),
        Some(0.34),
    );

    let ledger = ao2([
        "provider",
        "cost-ledger",
        "--acceptance-root",
        acceptance_root.to_str().unwrap(),
        "--json",
    ]);

    assert!(ledger.status.success(), "{}", stderr(&ledger));
    let json: serde_json::Value = serde_json::from_str(&stdout(&ledger)).unwrap();
    assert_eq!(json["schema_version"], "ao2.provider-cost-ledger.v1");
    assert_eq!(json["status"], "ready");
    assert_eq!(json["entry_count"], 2);
    assert_eq!(json["totals"]["max_budget_usd"], 2.0);
    assert_eq!(json["totals"]["observed_cost_usd"], 0.46);
    assert_eq!(json["totals"]["input_tokens"], 3000);
    assert_eq!(json["totals"]["output_tokens"], 1250);
    assert_eq!(json["totals"]["total_tokens"], 4250);
    assert_eq!(json["providers"]["codex"]["entry_count"], 1);
    assert_eq!(
        json["providers"]["codex"]["provider_enforced_budget"],
        false
    );
    assert_eq!(json["providers"]["claude"]["entry_count"], 1);
    assert_eq!(
        json["providers"]["claude"]["provider_enforced_budget"],
        true
    );
    assert_eq!(json["entries"][0]["release_tag"], "v0.4.67");
    assert_eq!(json["entries"][0]["provider"], "claude");
    assert_eq!(json["entries"][1]["provider"], "codex");
}

#[test]
fn cli_provider_cost_trend_reports_release_over_release_budget_usage_and_cost_delta() {
    let temp = tempfile::tempdir().unwrap();
    let acceptance_root = temp.path().join("acceptance-root");
    write_provider_cost_ledger_fixture(
        &acceptance_root.join("v0.4.66"),
        "codex",
        "old-codex-provider-pilot",
        1.00,
        false,
        Some(700),
        Some(300),
        Some(1000),
        Some(0.10),
    );
    write_provider_cost_ledger_fixture(
        &acceptance_root.join("v0.4.67"),
        "codex",
        "live-codex-provider-pilot",
        1.00,
        false,
        Some(1000),
        Some(500),
        Some(1500),
        Some(0.12),
    );
    write_provider_cost_ledger_fixture(
        &acceptance_root.join("v0.4.67").join("claude"),
        "claude",
        "live-claude-provider-pilot",
        1.00,
        true,
        Some(2000),
        Some(750),
        Some(2750),
        Some(0.34),
    );

    let trend = ao2([
        "provider",
        "cost-trend",
        "--acceptance-root",
        acceptance_root.to_str().unwrap(),
        "--json",
    ]);

    assert!(trend.status.success(), "{}", stderr(&trend));
    let json: serde_json::Value = serde_json::from_str(&stdout(&trend)).unwrap();
    assert_eq!(json["schema_version"], "ao2.provider-cost-trend.v1");
    assert_eq!(json["status"], "ready");
    assert_eq!(json["release_count"], 2);
    assert_eq!(json["latest_release_tag"], "v0.4.67");
    assert_eq!(json["previous_release_tag"], "v0.4.66");
    assert_eq!(json["delta"]["max_budget_usd"], 1.0);
    assert_eq!(json["delta"]["observed_cost_usd"], 0.36);
    assert_eq!(json["delta"]["total_tokens"], 3250);
    assert_eq!(json["releases"][0]["release_tag"], "v0.4.66");
    assert_eq!(json["releases"][1]["release_tag"], "v0.4.67");
    assert_eq!(json["releases"][1]["entry_count"], 2);
    assert_eq!(
        json["releases"][1]["providers"]["claude"]["total_tokens"],
        2750
    );
    assert_eq!(json["providers"]["codex"]["release_count"], 2);
    assert_eq!(json["providers"]["codex"]["observed_cost_usd"], 0.22);
    assert_eq!(json["providers"]["claude"]["release_count"], 1);
}

#[test]
fn cli_provider_smoke_all_guards_live_codex_without_explicit_env() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_codex(&bin);
    let path = prepend_path(&bin);

    let smoke = ao2_with_env(
        [
            "provider",
            "smoke-all",
            "--target",
            repo.to_str().unwrap(),
            "--live-provider",
            "codex",
            "--json",
        ],
        [("PATH", path.as_str())],
    );
    assert!(smoke.status.success(), "{}", stderr(&smoke));
    let json: serde_json::Value = serde_json::from_str(&stdout(&smoke)).unwrap();
    let codex = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "codex")
        .expect("codex provider should be present");
    assert_eq!(codex["available"], true);
    assert_eq!(codex["verdict"], "guarded");
    assert_eq!(codex["guard_env"], "AO2_LIVE_CODEX_SMOKE");
    assert_eq!(codex["run_id"], "");
}

#[test]
fn cli_provider_smoke_all_runs_live_codex_when_explicitly_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_codex(&bin);
    let path = prepend_path(&bin);

    let smoke = ao2_with_env(
        [
            "provider",
            "smoke-all",
            "--target",
            repo.to_str().unwrap(),
            "--live-provider",
            "codex",
            "--json",
        ],
        [("PATH", path.as_str()), ("AO2_LIVE_CODEX_SMOKE", "1")],
    );
    assert!(smoke.status.success(), "{}", stderr(&smoke));
    let json: serde_json::Value = serde_json::from_str(&stdout(&smoke)).unwrap();
    let codex = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "codex")
        .expect("codex provider should be present");
    assert_eq!(codex["available"], true);
    assert_eq!(codex["verdict"], "ready");
    assert!(codex["run_id"]
        .as_str()
        .unwrap()
        .starts_with("provider-smoke-codex-"));
    assert_eq!(
        codex["scorecard"]["schema"],
        "ao2.provider-evidence-scorecard.v1"
    );
    assert!(codex["score"].as_u64().unwrap() >= 90);
    let history: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo.join(".ao2/provider-smoke/history.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(history["entry_count"], 1);
    assert!(serde_json::to_string(&history["latest"])
        .unwrap()
        .contains("provider-smoke-codex-"));
}

#[test]
fn cli_provider_smoke_all_guards_live_claude_without_explicit_env() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_claude(&bin);
    let path = prepend_path(&bin);

    let smoke = ao2_with_env(
        [
            "provider",
            "smoke-all",
            "--target",
            repo.to_str().unwrap(),
            "--live-provider",
            "claude",
            "--json",
        ],
        [("PATH", path.as_str())],
    );
    assert!(smoke.status.success(), "{}", stderr(&smoke));
    let json: serde_json::Value = serde_json::from_str(&stdout(&smoke)).unwrap();
    let claude = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "claude")
        .expect("claude provider should be present");
    assert_eq!(claude["available"], true);
    assert_eq!(claude["verdict"], "guarded");
    assert_eq!(claude["guard_env"], "AO2_LIVE_CLAUDE_SMOKE");
    assert_eq!(claude["run_id"], "");
}

#[test]
fn cli_provider_smoke_all_runs_live_claude_when_explicitly_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let bin = temp.path().join("bin");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    write_fake_claude(&bin);
    let path = prepend_path(&bin);

    let smoke = ao2_with_env(
        [
            "provider",
            "smoke-all",
            "--target",
            repo.to_str().unwrap(),
            "--live-provider",
            "claude",
            "--json",
        ],
        [("PATH", path.as_str()), ("AO2_LIVE_CLAUDE_SMOKE", "1")],
    );
    assert!(smoke.status.success(), "{}", stderr(&smoke));
    let json: serde_json::Value = serde_json::from_str(&stdout(&smoke)).unwrap();
    let claude = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["provider"] == "claude")
        .expect("claude provider should be present");
    assert_eq!(claude["available"], true);
    assert_eq!(claude["verdict"], "ready");
    assert!(claude["run_id"]
        .as_str()
        .unwrap()
        .starts_with("provider-smoke-claude-"));
    assert_eq!(
        claude["scorecard"]["schema"],
        "ao2.provider-evidence-scorecard.v1"
    );
    assert!(claude["score"].as_u64().unwrap() >= 90);
    let history: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo.join(".ao2/provider-smoke/history.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(history["entry_count"], 1);
    assert!(serde_json::to_string(&history["latest"])
        .unwrap()
        .contains("provider-smoke-claude-"));
}

#[test]
fn cli_provider_run_requires_explicit_auto_approval_identity() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("prompt.sh");
    fs::write(
        &prompt_path,
        r#"cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: explicit approval identity regression\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 10\n'
"#,
    )
    .unwrap();

    let run = ao2_without_auto_approval_identity([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "provider-cli-explicit-approver",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=WaitingForApproval"));
    let evidence: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            repo.join(".ao2/runs/provider-cli-explicit-approver/evidence-pack/evidence-pack.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let pending = evidence["approvals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|ticket| {
            ticket["requested_action"] == "sandbox:apply" && ticket["status"] == "pending"
        })
        .expect("sandbox approval remains pending without explicit auto approver");
    assert_eq!(pending["approver"], serde_json::Value::Null);
}

#[test]
fn cli_provider_score_rates_provider_evidence_for_existing_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("prompt.sh");
    fs::write(
        &prompt_path,
        r#"cat > discount_service/discounts.py <<'PY'
def calculate_discount(price: float, discount_rate: float) -> float:
    if price < 0:
        raise ValueError("price must be non-negative")
    if discount_rate < 0 or discount_rate > 1:
        raise ValueError("discount_rate must be between 0 and 1")
    return price * (1 - discount_rate)
PY
printf 'Summary: scorecard run added validation around discount math\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 10\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "provider-score",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Accepted"));

    let score = ao2([
        "provider",
        "score",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "provider-score",
        "--json",
    ]);
    assert!(score.status.success(), "{}", stderr(&score));
    let json: serde_json::Value = serde_json::from_str(&stdout(&score)).unwrap();

    assert_eq!(json["schema"], "ao2.provider-evidence-scorecard.v1");
    assert_eq!(json["run_id"], "provider-score");
    assert_eq!(json["verdict"], "ready");
    assert!(json["score"].as_u64().unwrap() >= 90);
    assert!(json["provider_summary_count"].as_u64().unwrap() > 0);
    assert!(json["applied_files_count"].as_u64().unwrap() > 0);
    assert_eq!(json["replay"]["digest_failures"], 0);

    let dimension_names = json["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|dimension| dimension["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(dimension_names.contains(&"replay_integrity"));
    assert!(dimension_names.contains(&"provider_summary"));
    assert!(dimension_names.contains(&"changed_files"));
    assert!(dimension_names.contains(&"blocker_hygiene"));
    assert!(dimension_names.contains(&"policy_boundary"));
}

#[test]
fn cli_provider_score_fails_rejected_replay_even_with_provider_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("prompt.sh");
    fs::write(
        &prompt_path,
        r#"cat > tests/test_discounts.py <<'PY'
import pytest

from discount_service.discounts import calculate_discount


def test_applies_discount():
    assert calculate_discount(100.0, 0.25) == 75.0


def test_rejects_negative_price():
    with pytest.raises(ValueError):
        calculate_discount(-1.0, 0.1)


def test_rejects_discount_rate_below_zero():
    with pytest.raises(ValueError):
        calculate_discount(100.0, -0.1)


def test_rejects_discount_rate_above_one():
    with pytest.raises(ValueError):
        calculate_discount(100.0, 1.1)
PY
printf 'Summary: scorecard run added validation tests but missed implementation\n'
printf 'Changed files: tests/test_discounts.py\n'
printf 'Input tokens: 10\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "provider-score-rejected",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Rejected"));

    let score = ao2([
        "provider",
        "score",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "provider-score-rejected",
        "--json",
    ]);
    assert!(score.status.success(), "{}", stderr(&score));
    let json: serde_json::Value = serde_json::from_str(&stdout(&score)).unwrap();

    assert_eq!(json["schema"], "ao2.provider-evidence-scorecard.v1");
    assert_eq!(json["run_id"], "provider-score-rejected");
    assert_eq!(json["replay"]["status"], "rejected");
    assert_eq!(json["replay"]["digest_failures"], 0);
    assert_eq!(json["verdict"], "fail");
    assert_eq!(json["applied_files_count"], 1);
    assert_eq!(json["applied_files"][0], "tests/test_discounts.py");
    assert!(json["score"].as_u64().unwrap() < 90);

    let replay_dimension = json["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|dimension| dimension["name"] == "replay_integrity")
        .unwrap();
    assert_eq!(replay_dimension["status"], "fail");
    assert_eq!(
        replay_dimension["evidence"],
        "replay status is not accepted"
    );
}
