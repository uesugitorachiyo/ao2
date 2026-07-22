use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;

#[test]
fn cli_evidence_publish_signs_and_posts_evidence_pack_to_control_plane() {
    let temp = tempfile::tempdir().unwrap();
    let evidence_pack_path = temp.path().join("evidence-pack.json");
    let gate_path = temp.path().join("obligation-gate-midpoint.json");
    let signing_key = temp.path().join("evidence-pack-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    fs::write(
        &evidence_pack_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.evidence-pack.v1",
            "run_id": "publish-evidence-run",
            "verdict": "accepted",
            "artifacts": [],
            "approvals": []
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &gate_path,
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
                        attempts <= 100,
                        "timed out waiting for evidence publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/evidence-pack/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request.contains("\"schema_version\":\"ao2.cp-evidence-pack-signed-upload.v1\""));
        assert!(request.contains("\"schema_version\":\"ao2.evidence-pack.v1\""));
        assert!(request.contains("\"run_id\":\"publish-evidence-run\""));
        assert!(request.contains("\"obligation_gates\""));
        assert!(request.contains("\"schema_version\":\"ao2.obligation-gate.v1\""));
        assert!(request.contains("\"stage\":\"midpoint\""));
        assert!(request.contains("\"signature_algorithm\":\"RSA/SHA-256\""));
        assert!(request.contains("\"signature_hex\""));
        assert!(request.contains("\"public_key_sha256\""));
        assert!(request.contains("\"public_key_pem\""));
        assert!(request.contains("\"signer_id\":\"local-operator\""));
        // schema-1: carry the exact signed bytes (the enriched, pretty-printed pack the
        // signature covers) as base64 so the control plane verifies over them rather than
        // a lossy re-serialization of the parsed `evidence_pack`.
        {
            use base64::prelude::{Engine as _, BASE64_STANDARD};
            let request_body = request.split("\r\n\r\n").nth(1).expect("request body");
            let upload: serde_json::Value =
                serde_json::from_str(request_body).expect("signed upload body is valid json");
            let evidence_pack_b64 = upload["evidence_pack_b64"]
                .as_str()
                .expect("signed upload must carry evidence_pack_b64");
            let decoded = BASE64_STANDARD
                .decode(evidence_pack_b64)
                .expect("evidence_pack_b64 must be valid base64");
            let reserialized = serde_json::to_string_pretty(&upload["evidence_pack"])
                .expect("evidence_pack re-serializes");
            assert_eq!(
                decoded,
                reserialized.as_bytes(),
                "evidence_pack_b64 must decode to the exact signed (pretty-printed) pack bytes"
            );
        }
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"evidence123","stored_at":"2026-05-20T00:00:00Z","ingested_schema_version":"ao2.evidence-pack.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "evidence",
        "publish",
        "--evidence-pack",
        evidence_pack_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "local-operator",
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
        "ao2.evidence-pack-control-plane-publish.v1"
    );
    assert_eq!(json["signed"], true);
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/evidence-pack/signed")
    );
    assert_eq!(
        json["receipt"]["ingested_schema_version"],
        "ao2.evidence-pack.v1"
    );
    assert_eq!(
        json["detail_url"],
        format!("http://127.0.0.1:{port}/api/v1/evidence-pack/evidence123/detail")
    );
    assert_eq!(
        json["dashboard_url"],
        format!("http://127.0.0.1:{port}/api/v1/evidence-pack/dashboard")
    );
}

#[test]
fn cli_evidence_publish_operator_packet_signs_and_posts_to_control_plane() {
    let temp = tempfile::tempdir().unwrap();
    let operator_packet_path = temp.path().join("operator-packet.json");
    let signing_key = temp.path().join("operator-packet-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    fs::write(
        &operator_packet_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.operator-evidence-packet.v1",
            "run_id": "publish-operator-run",
            "status": "passed",
            "operator_id": "local-operator",
            "summary": {"evidence_count": 2},
            "evidence": [],
            "trust_boundary": {
                "control_plane_role": "read_only_observer_after_signed_operator_packet",
                "mutates_ao2": false
            }
        }))
        .unwrap(),
    )
    .unwrap();
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
                        attempts <= 100,
                        "timed out waiting for operator packet publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/operator-packet/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request.contains("\"schema_version\":\"ao2.cp-operator-packet-signed-upload.v1\""));
        assert!(request.contains("\"schema_version\":\"ao2.operator-evidence-packet.v1\""));
        assert!(request.contains("\"run_id\":\"publish-operator-run\""));
        assert!(request.contains("\"signature_algorithm\":\"RSA/SHA-256\""));
        assert!(request.contains("\"signature_hex\""));
        assert!(request.contains("\"public_key_sha256\""));
        assert!(request.contains("\"public_key_pem\""));
        assert!(request.contains("\"signer_id\":\"local-operator\""));
        {
            use base64::prelude::{Engine as _, BASE64_STANDARD};
            let request_body = request.split("\r\n\r\n").nth(1).expect("request body");
            let upload: serde_json::Value =
                serde_json::from_str(request_body).expect("signed upload body is valid json");
            let operator_packet_b64 = upload["operator_packet_b64"]
                .as_str()
                .expect("signed upload must carry operator_packet_b64");
            let decoded = BASE64_STANDARD
                .decode(operator_packet_b64)
                .expect("operator_packet_b64 must be valid base64");
            let reserialized = serde_json::to_string_pretty(&upload["operator_packet"])
                .expect("operator_packet re-serializes");
            assert_eq!(
                decoded,
                reserialized.as_bytes(),
                "operator_packet_b64 must decode to the exact signed packet bytes"
            );
        }
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"operator123","stored_at":"2026-06-07T00:00:00Z","ingested_schema_version":"ao2.operator-evidence-packet.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "evidence",
        "publish-operator-packet",
        "--operator-packet",
        operator_packet_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "local-operator",
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
        "ao2.operator-packet-control-plane-publish.v1"
    );
    assert_eq!(json["signed"], true);
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/operator-packet/signed")
    );
    assert_eq!(
        json["receipt"]["ingested_schema_version"],
        "ao2.operator-evidence-packet.v1"
    );
    assert_eq!(
        json["detail_url"],
        format!("http://127.0.0.1:{port}/api/v1/operator-packet/operator123/detail")
    );
    assert_eq!(
        json["dashboard_url"],
        format!("http://127.0.0.1:{port}/api/v1/operator-packet/dashboard")
    );
}

#[test]
fn cli_evidence_publish_reads_api_token_from_env_without_printing_secret() {
    let temp = tempfile::tempdir().unwrap();
    let evidence_pack_path = temp.path().join("evidence-pack.json");
    let signing_key = temp.path().join("evidence-pack-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    fs::write(
        &evidence_pack_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.evidence-pack.v1",
            "run_id": "publish-evidence-env-token-run",
            "verdict": "accepted",
            "artifacts": [],
            "approvals": []
        }))
        .unwrap(),
    )
    .unwrap();
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
                        attempts <= 100,
                        "timed out waiting for evidence publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/evidence-pack/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-evidence-token"));
        assert!(request.contains("\"run_id\":\"publish-evidence-env-token-run\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"evidenceenv123","stored_at":"2026-05-20T00:00:00Z","ingested_schema_version":"ao2.evidence-pack.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2_with_env(
        [
            "evidence",
            "publish",
            "--evidence-pack",
            evidence_pack_path.to_str().unwrap(),
            "--signing-key",
            signing_key.to_str().unwrap(),
            "--signer-id",
            "local-operator",
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_TEST_EVIDENCE_CP_TOKEN",
            "--json",
        ],
        [("AO2_TEST_EVIDENCE_CP_TOKEN", "env-evidence-token")],
    );
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    let stderr = stderr(&publish);
    assert!(!stdout.contains("env-evidence-token"));
    assert!(!stderr.contains("env-evidence-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.evidence-pack-control-plane-publish.v1"
    );
    assert_eq!(json["receipt"]["sha256"], "evidenceenv123");
}

fn read_test_http_request(stream: &mut TcpStream, buffer: &mut [u8]) -> usize {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut total = 0;
    loop {
        match stream.read(&mut buffer[total..]) {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                if total >= 4
                    && buffer[..total]
                        .windows(4)
                        .any(|window| window == b"\r\n\r\n")
                {
                    let header = String::from_utf8_lossy(&buffer[..total]);
                    let content_length = header
                        .lines()
                        .find_map(|line| line.strip_prefix("Content-Length: "))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    let body_start = header.find("\r\n\r\n").unwrap() + 4;
                    if total >= body_start + content_length {
                        break;
                    }
                }
                assert!(total < buffer.len(), "request exceeded test buffer");
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => panic!("read request failed: {error}"),
        }
    }
    total
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

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}
