use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;

#[test]
fn cli_memory_write_search_and_link_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(
        repo.join("README.md"),
        "Hermes should remember AO closure.\n",
    )
    .unwrap();

    let write = ao2([
        "memory",
        "write",
        "--target",
        repo.to_str().unwrap(),
        "--kind",
        "decision",
        "--title",
        "Hermes routes through AO2",
        "--body",
        "Hermes is the front end. AO2 remains the trusted execution boundary.",
        "--tag",
        "hermes",
        "--tag",
        "ao2",
        "--source-run-id",
        "run-123",
        "--source-path",
        "README.md",
        "--json",
    ]);
    assert!(write.status.success(), "{}", stderr(&write));
    let write_json: serde_json::Value = serde_json::from_str(&stdout(&write)).unwrap();
    assert_eq!(write_json["schema_version"], "ao2.memory-record.v1");
    assert_eq!(write_json["kind"], "decision");
    let memory_id = write_json["id"].as_str().unwrap();
    assert!(memory_id.starts_with("mem-"));

    let search = ao2([
        "memory",
        "search",
        "--target",
        repo.to_str().unwrap(),
        "--query",
        "trusted execution",
        "--json",
    ]);
    assert!(search.status.success(), "{}", stderr(&search));
    let search_json: serde_json::Value = serde_json::from_str(&stdout(&search)).unwrap();
    assert_eq!(search_json["schema_version"], "ao2.memory-search.v1");
    assert_eq!(search_json["query"], "trusted execution");
    assert_eq!(search_json["matches"][0]["id"], memory_id);
    assert_eq!(search_json["matches"][0]["source"]["run_id"], "run-123");

    let link = ao2([
        "memory",
        "link-run",
        "--target",
        repo.to_str().unwrap(),
        "--memory-id",
        memory_id,
        "--run-id",
        "run-456",
        "--relationship",
        "follow-up",
        "--json",
    ]);
    assert!(link.status.success(), "{}", stderr(&link));
    let link_json: serde_json::Value = serde_json::from_str(&stdout(&link)).unwrap();
    assert_eq!(link_json["schema_version"], "ao2.memory-run-link.v1");
    assert_eq!(link_json["memory_id"], memory_id);
    assert_eq!(link_json["run_id"], "run-456");

    let records = fs::read_to_string(repo.join(".ao2/memory/records.jsonl")).unwrap();
    assert!(records.contains(memory_id));
    let links = fs::read_to_string(repo.join(".ao2/memory/run-links.jsonl")).unwrap();
    assert!(links.contains("follow-up"));
}

#[test]
fn cli_memory_export_filters_links_and_signs_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("README.md"), "Hermes export evidence.\n").unwrap();

    let hermes = ao2([
        "memory",
        "write",
        "--target",
        repo.to_str().unwrap(),
        "--kind",
        "decision",
        "--title",
        "Hermes keeps AO2 memory",
        "--body",
        "Hermes searches governed memory while AO2 owns evidence.",
        "--tag",
        "hermes",
        "--source-run-id",
        "run-hermes",
        "--source-path",
        "README.md",
        "--json",
    ]);
    assert!(hermes.status.success(), "{}", stderr(&hermes));
    let hermes_json: serde_json::Value = serde_json::from_str(&stdout(&hermes)).unwrap();
    let hermes_id = hermes_json["id"].as_str().unwrap();

    let other = ao2([
        "memory",
        "write",
        "--target",
        repo.to_str().unwrap(),
        "--kind",
        "note",
        "--title",
        "Unrelated adapter note",
        "--body",
        "This record should stay outside a filtered memory export.",
        "--json",
    ]);
    assert!(other.status.success(), "{}", stderr(&other));
    let other_json: serde_json::Value = serde_json::from_str(&stdout(&other)).unwrap();
    let other_id = other_json["id"].as_str().unwrap();

    let link_hermes = ao2([
        "memory",
        "link-run",
        "--target",
        repo.to_str().unwrap(),
        "--memory-id",
        hermes_id,
        "--run-id",
        "run-hermes",
        "--relationship",
        "source",
        "--json",
    ]);
    assert!(link_hermes.status.success(), "{}", stderr(&link_hermes));
    let link_other = ao2([
        "memory",
        "link-run",
        "--target",
        repo.to_str().unwrap(),
        "--memory-id",
        other_id,
        "--run-id",
        "run-other",
        "--relationship",
        "source",
        "--json",
    ]);
    assert!(link_other.status.success(), "{}", stderr(&link_other));

    let signing_key = temp.path().join("memory-export-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let out = temp.path().join("memory-export.json");
    let export = ao2([
        "memory",
        "export",
        "--target",
        repo.to_str().unwrap(),
        "--query",
        "Hermes",
        "--out",
        out.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "hermes-operator",
        "--json",
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    let export_json: serde_json::Value = serde_json::from_str(&stdout(&export)).unwrap();
    assert_eq!(export_json["schema_version"], "ao2.memory-export.v1");
    assert_eq!(export_json["query"], "Hermes");
    assert_eq!(export_json["record_count"], 1);
    assert_eq!(export_json["link_count"], 1);
    assert_eq!(export_json["records"][0]["id"], hermes_id);
    assert_eq!(export_json["links"][0]["run_id"], "run-hermes");
    assert_eq!(export_json["signing"]["signer_id"], "hermes-operator");
    assert!(export_json["sha256"].as_str().unwrap().len() == 64);
    assert!(out.is_file());
    assert!(Path::new(export_json["signature_path"].as_str().unwrap()).is_file());
    assert!(Path::new(export_json["public_key_path"].as_str().unwrap()).is_file());

    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(persisted["records"][0]["id"], hermes_id);
    assert_eq!(persisted["links"][0]["memory_id"], hermes_id);
    assert!(!fs::read_to_string(out).unwrap().contains(other_id));
}

#[test]
fn cli_memory_publish_posts_export_to_control_plane() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("memory-repo");
    fs::create_dir_all(&repo).unwrap();
    let export_path = temp.path().join("memory-export.json");

    let write = ao2([
        "memory",
        "write",
        "--target",
        repo.to_str().unwrap(),
        "--kind",
        "decision",
        "--title",
        "Control-plane publish",
        "--body",
        "Publish memory exports as read-only control-plane evidence.",
    ]);
    assert!(write.status.success(), "{}", stderr(&write));

    let export = ao2([
        "memory",
        "export",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        export_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(export.status.success(), "{}", stderr(&export));

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
                    assert!(attempts <= 100, "timed out waiting for publish request");
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/memory/export HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(
            request.contains("\"schema_version\":\"ao2.memory-export.v1\"")
                || request.contains("\"schema_version\": \"ao2.memory-export.v1\"")
        );
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"abc123","stored_at":"2026-05-19T00:00:00Z","ingested_schema_version":"ao2.memory-export.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "memory",
        "publish",
        "--export",
        export_path.to_str().unwrap(),
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        // Slice 19: memory publish is default-on signed; this test exercises
        // the legacy unsigned upload path via the hidden escape valve.
        "--allow-unsigned-memory-export",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let json: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.memory-control-plane-publish.v1"
    );
    assert_eq!(
        json["receipt"]["schema_version"],
        "ao2.cp-ingest-receipt.v1"
    );
    assert_eq!(
        json["receipt"]["ingested_schema_version"],
        "ao2.memory-export.v1"
    );
}

#[test]
fn cli_memory_publish_default_on_rejects_unsigned_export_without_sidecars() {
    // Slice 19: producer-side default-on signed-memory-export upload for the
    // CLI. When no sibling `.json.sig` + `memory-export-signing-public.pem`
    // files exist, `ao2 memory publish` must fail-closed BEFORE making any
    // HTTP request, so the unsigned export never reaches a control plane.
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("memory-repo");
    fs::create_dir_all(&repo).unwrap();
    let export_path = temp.path().join("memory-export.json");

    let write = ao2([
        "memory",
        "write",
        "--target",
        repo.to_str().unwrap(),
        "--kind",
        "decision",
        "--title",
        "Default-on fail-closed fixture",
        "--body",
        "Slice 19 must reject unsigned export upload by default.",
    ]);
    assert!(write.status.success(), "{}", stderr(&write));

    let export = ao2([
        "memory",
        "export",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        export_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(export.status.success(), "{}", stderr(&export));
    assert!(
        !export_path.with_extension("json.sig").exists(),
        "fixture sanity: export must not have a signature sidecar"
    );

    // Bind a port but never accept — the CLI must exit non-zero before
    // attempting the connection, so the listener should remain idle.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();

    let publish = ao2([
        "memory",
        "publish",
        "--export",
        export_path.to_str().unwrap(),
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    assert!(
        !publish.status.success(),
        "expected fail-closed; got success: {}",
        stdout(&publish)
    );
    let err = stderr(&publish);
    assert!(
        err.contains("memory publish requires a signed export by default"),
        "missing default-on error: {err}"
    );
    assert!(
        err.contains("--allow-unsigned-memory-export"),
        "missing escape-valve guidance: {err}"
    );
    // Confirm the listener never accepted a connection.
    match listener.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok(_) => panic!("CLI made an HTTP request before fail-closed check fired"),
        Err(error) => panic!("unexpected accept error: {error}"),
    }
}

#[test]
fn cli_memory_publish_posts_signed_export_when_sidecars_exist() {
    let temp = tempfile::tempdir().unwrap();
    let export_path = temp.path().join("memory-export.json");
    let memory_export_content = r#"{"schema_version":"ao2.memory-export.v1","record_count":0,"link_count":0,"records":[],"links":[]}"#;
    fs::write(&export_path, memory_export_content).unwrap();
    fs::write(export_path.with_extension("json.sig"), b"signature bytes").unwrap();
    fs::write(
        temp.path().join("memory-export-signing-public.pem"),
        b"public key bytes",
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
                        "timed out waiting for signed publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/memory/export/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request.contains("\"schema_version\":\"ao2.cp-memory-export-signed-upload.v1\""));
        assert!(request.contains("\"signature_sha256\""));
        assert!(request.contains("\"signature_hex\""));
        assert!(request.contains("\"public_key_sha256\""));
        assert!(request.contains("\"public_key_pem\""));
        // schema-1: the producer must carry the exact bytes the sibling `.json.sig`
        // signs — the verbatim export-file content — as base64, so the control plane
        // verifies the signature over those bytes instead of a lossy re-serialization
        // of the parsed `export`.
        {
            use base64::prelude::{Engine as _, BASE64_STANDARD};
            let request_body = request.split("\r\n\r\n").nth(1).expect("request body");
            let upload: serde_json::Value =
                serde_json::from_str(request_body).expect("signed upload body is valid json");
            let export_b64 = upload["export_b64"]
                .as_str()
                .expect("signed upload must carry export_b64");
            let decoded = BASE64_STANDARD
                .decode(export_b64)
                .expect("export_b64 must be valid base64");
            assert_eq!(
                decoded,
                memory_export_content.as_bytes(),
                "export_b64 must decode to the exact signed export bytes"
            );
        }
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"signed123","stored_at":"2026-05-19T00:00:00Z","ingested_schema_version":"ao2.memory-export.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "memory",
        "publish",
        "--export",
        export_path.to_str().unwrap(),
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let json: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(json["signed"], true);
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/memory/export/signed")
    );
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

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}
