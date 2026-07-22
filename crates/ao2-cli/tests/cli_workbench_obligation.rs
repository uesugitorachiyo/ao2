use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[test]
fn cli_workbench_obligation_annotation_api_updates_sidecar_ledger() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let signing_key = temp.path().join("obligation-annotation-key.pem");
    generate_native_signing_key(&signing_key, 3072);
    let prompt_path = temp.path().join("prompt.sh");
    fs::write(
        &prompt_path,
        r#"printf 'Summary: obligation annotation api fixture\n'
printf 'Changed files: README.md\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-obligation-annotate",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let ledger_dir = repo
        .join(".ao2")
        .join("runs")
        .join("workbench-obligation-annotate")
        .join("evidence-pack");
    fs::write(
        ledger_dir.join("obligation-ledger.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.obligation-ledger.v1",
            "source_contracts": [],
            "obligations": [{
                "id": "OBL-001",
                "kind": "must",
                "statement": "MUST keep the business rule understandable to operators.",
                "source_path": "SPEC.md",
                "source_line": 1,
                "source_excerpt_hash": "sha256:placeholder",
                "expected_fragments": [],
                "status": "unverified",
                "evidence": [],
                "waiver": null
            }],
            "summary": {"pass": 0, "fail": 0, "unverified": 1, "waived": 0},
            "verdict": "rejected",
            "created_at": "2026-05-19T00:00:00Z"
        }))
        .unwrap(),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
            "--api-token",
            "viewer-token",
            "--operator-token",
            "ops:operator:operator-token",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "obligation-lead",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port_split_suffix(&mut child);
    let body = "run_id=workbench-obligation-annotate&obligation_id=OBL-001&evidence_path=README.md&evidence_line=3&detail=operator-facing+rule+is+documented";
    let request = format!(
        "POST /api/obligations/annotate?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.workbench-obligation-annotation.v1"
    );
    assert_eq!(json["ledger"]["verdict"], "accepted");
    assert_eq!(json["ledger"]["summary"]["pass"], 1);
    assert_eq!(
        json["ledger"]["obligations"][0]["evidence"][0]["path"],
        "README.md"
    );
    assert_eq!(json["audit_event"]["action"], "obligation_annotate");
    assert_eq!(json["audit_event"]["operator_id"], "ops");
    assert_eq!(
        json["audit_event"]["run_id"],
        "workbench-obligation-annotate"
    );
    assert_eq!(json["audit_event"]["obligation_id"], "OBL-001");
    assert_eq!(
        json["evidence_export"]["export_kind"],
        "obligation-annotation"
    );
    assert_eq!(json["evidence_export"]["signature"]["present"], true);
    assert_eq!(
        json["evidence_export"]["signature"]["signature_verified"],
        true
    );
    assert_eq!(
        json["evidence_export"]["signature"]["signer_id"],
        "obligation-lead"
    );
    let evidence_export_path =
        PathBuf::from(json["evidence_export"]["export_path"].as_str().unwrap());
    assert!(evidence_export_path.is_file());
    assert!(PathBuf::from(
        json["evidence_export"]["signature"]["signature_path"]
            .as_str()
            .unwrap()
    )
    .is_file());
    let evidence_export: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&evidence_export_path).unwrap()).unwrap();
    assert_eq!(evidence_export["export_kind"], "obligation-annotation");
    assert_eq!(
        evidence_export["export"]["annotation"]["run_id"],
        "workbench-obligation-annotate"
    );
    assert_eq!(
        evidence_export["export"]["annotation"]["before_ledger_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        evidence_export["export"]["annotation"]["after_ledger_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    let audit = fs::read_to_string(repo.join(".ao2/workbench/audit.jsonl")).unwrap();
    assert!(audit.contains("\"action\":\"obligation_annotate\""));
    assert!(audit.contains("\"operator_id\":\"ops\""));
}

#[test]
fn cli_workbench_obligation_gate_api_writes_stage_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let signing_key = temp.path().join("obligation-gate-key.pem");
    generate_native_signing_key(&signing_key, 3072);
    let prompt_path = temp.path().join("prompt.sh");
    fs::write(
        &prompt_path,
        r#"printf 'Summary: obligation gate api fixture\n'
printf 'Changed files: README.md\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-obligation-gate",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let ledger_dir = repo
        .join(".ao2")
        .join("runs")
        .join("workbench-obligation-gate")
        .join("evidence-pack");
    fs::write(
        ledger_dir.join("obligation-ledger.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.obligation-ledger.v1",
            "source_contracts": [],
            "obligations": [{
                "id": "OBL-001",
                "kind": "content_preservation",
                "statement": "MUST preserve `net = gross - fees` exactly.",
                "source_path": "SPEC.md",
                "source_line": 1,
                "source_excerpt_hash": "sha256:placeholder",
                "expected_fragments": ["net = gross - fees"],
                "status": "unverified",
                "evidence": [],
                "waiver": null
            }],
            "summary": {"pass": 0, "fail": 0, "unverified": 1, "waived": 0},
            "verdict": "rejected",
            "created_at": "2026-05-19T00:00:00Z"
        }))
        .unwrap(),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
            "--api-token",
            "viewer-token",
            "--operator-token",
            "ops:operator:operator-token",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "obligation-lead",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = "run_id=workbench-obligation-gate&stage=midpoint";
    let request = format!(
        "POST /api/obligations/gate?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let response = http_request(port, &request);
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-obligation-gate.v1");
    assert_eq!(json["gate"]["schema_version"], "ao2.obligation-gate.v1");
    assert_eq!(json["gate"]["stage"], "midpoint");
    assert_eq!(json["gate"]["status"], "failed");
    assert_eq!(json["gate"]["summary"]["fail"], 1);
    assert_eq!(json["audit_event"]["action"], "obligation_gate");
    assert_eq!(json["audit_event"]["operator_id"], "ops");
    let gate_path = PathBuf::from(json["gate_path"].as_str().unwrap());
    assert!(gate_path.is_file());
    let gate_file: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&gate_path).unwrap()).unwrap();
    assert_eq!(gate_file["schema_version"], "ao2.obligation-gate.v1");
    assert_eq!(gate_file["status"], "failed");
    assert_eq!(json["evidence_export"]["export_kind"], "obligation-gate");
    assert_eq!(json["evidence_export"]["signature"]["present"], true);
    assert_eq!(
        json["evidence_export"]["signature"]["signature_verified"],
        true
    );
    let audit = fs::read_to_string(repo.join(".ao2/workbench/audit.jsonl")).unwrap();
    assert!(audit.contains("\"action\":\"obligation_gate\""));
}

#[test]
fn cli_workbench_obligation_gate_api_default_on_rejects_unsigned_when_workbench_lacks_signing_key()
{
    // Slice 18: producer-side default-on signing for the workbench HTTP
    // surface. When the workbench is started WITHOUT --support-signing-key
    // and the operator does not opt out via the form param, POST
    // /api/obligations/gate must fail-closed with HTTP 400 and never write
    // a raw obligation-gate-*.json artifact under .ao2/runs/.
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("prompt.sh");
    fs::write(
        &prompt_path,
        r#"printf 'Summary: obligation gate api default-on fixture\n'
printf 'Changed files: README.md\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-obligation-gate-default-on",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let ledger_dir = repo
        .join(".ao2")
        .join("runs")
        .join("workbench-obligation-gate-default-on")
        .join("evidence-pack");
    fs::write(
        ledger_dir.join("obligation-ledger.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.obligation-ledger.v1",
            "source_contracts": [],
            "obligations": [{
                "id": "OBL-001",
                "kind": "content_preservation",
                "statement": "MUST preserve `net = gross - fees` exactly.",
                "source_path": "SPEC.md",
                "source_line": 1,
                "source_excerpt_hash": "sha256:placeholder",
                "expected_fragments": ["net = gross - fees"],
                "status": "unverified",
                "evidence": [],
                "waiver": null
            }],
            "summary": {"pass": 0, "fail": 0, "unverified": 1, "waived": 0},
            "verdict": "rejected",
            "created_at": "2026-05-25T00:00:00Z"
        }))
        .unwrap(),
    )
    .unwrap();

    // Note: workbench is intentionally started WITHOUT --support-signing-key
    // so the slice-18 fail-closed branch fires.
    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
            "--api-token",
            "viewer-token",
            "--operator-token",
            "ops:operator:operator-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    // 1) No escape valve: fail-closed.
    let default_on_body = "run_id=workbench-obligation-gate-default-on&stage=midpoint";
    let default_on_request = format!(
        "POST /api/obligations/gate?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        default_on_body.len(),
        default_on_body
    );
    let default_on_response = http_request(port, &default_on_request);
    let status = child.wait().unwrap();
    assert!(status.success());

    assert!(
        default_on_response.starts_with("HTTP/1.1 400"),
        "default-on must fail-closed with HTTP 400; got:\n{default_on_response}"
    );
    let default_on_json: serde_json::Value =
        serde_json::from_str(http_body(&default_on_response)).unwrap();
    assert_eq!(default_on_json["schema_version"], "ao2.workbench-error.v1");
    let err = default_on_json["error"].as_str().unwrap();
    assert!(
        err.contains("--support-signing-key by default")
            && err.contains("allow_unsigned_obligation_gates"),
        "fail-closed error must surface escape-valve guidance; got: {err}"
    );

    // Fail-closed must NOT have written any obligation-gate-*.json artifact
    // (the producer fails BEFORE atomic_write_text). Verify the evidence-pack
    // directory still has only the ledger.
    let evidence_pack_files: Vec<_> = fs::read_dir(&ledger_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("obligation-gate-")
        })
        .collect();
    assert!(
        evidence_pack_files.is_empty(),
        "fail-closed must NOT write any obligation-gate-*.json artifact under the evidence-pack; found: {evidence_pack_files:?}"
    );
}

#[test]
fn cli_workbench_obligation_gate_api_allow_unsigned_form_param_preserves_legacy_behavior() {
    // Slice 18: explicit form-param escape valve preserves the legacy
    // unsigned-emission path for operators that have not yet provisioned
    // a workbench-side signing key. The gate is produced and the response
    // signature block reports `present=false`.
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let prompt_path = temp.path().join("prompt.sh");
    fs::write(
        &prompt_path,
        r#"printf 'Summary: obligation gate api escape valve fixture\n'
printf 'Changed files: README.md\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-obligation-gate-allow-unsigned",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let ledger_dir = repo
        .join(".ao2")
        .join("runs")
        .join("workbench-obligation-gate-allow-unsigned")
        .join("evidence-pack");
    fs::write(
        ledger_dir.join("obligation-ledger.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.obligation-ledger.v1",
            "source_contracts": [],
            "obligations": [{
                "id": "OBL-001",
                "kind": "content_preservation",
                "statement": "MUST preserve `net = gross - fees` exactly.",
                "source_path": "SPEC.md",
                "source_line": 1,
                "source_excerpt_hash": "sha256:placeholder",
                "expected_fragments": ["net = gross - fees"],
                "status": "unverified",
                "evidence": [],
                "waiver": null
            }],
            "summary": {"pass": 0, "fail": 0, "unverified": 1, "waived": 0},
            "verdict": "rejected",
            "created_at": "2026-05-25T00:00:00Z"
        }))
        .unwrap(),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
            "--api-token",
            "viewer-token",
            "--operator-token",
            "ops:operator:operator-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let body = "run_id=workbench-obligation-gate-allow-unsigned&stage=midpoint&allow_unsigned_obligation_gates=1";
    let request = format!(
        "POST /api/obligations/gate?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let response = http_request(port, &request);
    let status = child.wait().unwrap();
    assert!(status.success());

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "escape valve must restore HTTP 200; got:\n{response}"
    );
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.workbench-obligation-gate.v1");
    assert_eq!(json["gate"]["schema_version"], "ao2.obligation-gate.v1");
    assert_eq!(json["gate"]["stage"], "midpoint");
    assert_eq!(json["gate"]["status"], "failed");
    // Unsigned: evidence_export present but signature.present = false.
    assert_eq!(json["evidence_export"]["export_kind"], "obligation-gate");
    assert_eq!(json["evidence_export"]["signature"]["present"], false);
    assert_eq!(
        json["evidence_export"]["signature"]["signature_verified"],
        false
    );
}

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

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
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

fn read_server_port_split_suffix(child: &mut std::process::Child) -> u16 {
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    line.trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .parse::<u16>()
        .unwrap()
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn http_body(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
}
