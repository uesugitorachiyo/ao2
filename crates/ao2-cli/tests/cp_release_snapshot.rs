use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;

fn run_ao2_release_snapshot(cp_url: &str, token: &str, out: &Path) -> serde_json::Value {
    let ao2 = env!("CARGO_BIN_EXE_ao2");
    let output = Command::new(ao2)
        .args([
            "cp",
            "release-snapshot",
            "--cp-url",
            cp_url,
            "--api-token",
            token,
            "--write-json",
            out.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run ao2 cp release-snapshot");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("snapshot is valid json")
}

fn handle_request(stream: &mut TcpStream, body: &str, content_type: &str, status_line: &str) {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    let mut buffer = [0_u8; 4096];
    let _ = stream.read(&mut buffer);
    let response = format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .expect("write response");
    stream.flush().expect("flush response");
}

fn accept_one(listener: &TcpListener) -> TcpStream {
    let mut attempts = 0;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                attempts += 1;
                assert!(attempts < 200, "timed out waiting for incoming request");
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

fn read_request_headers(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(250)))
        .ok();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut request = Vec::new();
    loop {
        let mut buffer = [0_u8; 1024];
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for complete request headers"
                );
            }
            Err(error) => panic!("read request failed: {error}"),
        }
    }
    String::from_utf8_lossy(&request).into_owned()
}

#[test]
fn cp_release_snapshot_captures_all_four_endpoints() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let port = listener.local_addr().expect("addr").port();

    let server = std::thread::spawn(move || {
        // The CLI fires GETs sequentially for: readiness, handoff,
        // support-bundle, publication/latest. Order matches the
        // `endpoints` array in `cp_release_snapshot`.
        let canned: &[(&str, &str, &str)] = &[
            (
                "readiness",
                r#"{"schema_version":"ao2.cp-release-readiness.v1","status":"ready"}"#,
                "200 OK",
            ),
            (
                "handoff",
                r#"{"schema_version":"ao2.cp-release-candidate-handoff.v1","status":"ready"}"#,
                "200 OK",
            ),
            (
                "support_bundle_status",
                r#"{"schema_version":"ao2.cp-release-support-bundle.v1","status":"assembled"}"#,
                "200 OK",
            ),
            (
                "publication_latest",
                r#"{"schema_version":"ao2.control-plane-error.v1","code":"not_found","message":"not found"}"#,
                "404 Not Found",
            ),
        ];
        for (_name, body, status) in canned.iter() {
            let mut stream = accept_one(&listener);
            handle_request(&mut stream, body, "application/json", status);
        }
    });

    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join("snap.json");
    let cp_url = format!("http://127.0.0.1:{port}");
    let snapshot = run_ao2_release_snapshot(&cp_url, "test-token", &out);

    server.join().expect("server thread");

    assert_eq!(snapshot["schema_version"], "ao2.cp-release-snapshot.v1");
    assert_eq!(snapshot["cp_url"], cp_url);
    let summary = &snapshot["summary"];
    assert_eq!(summary["endpoint_count"], 4);
    assert_eq!(summary["ok_count"], 3);
    assert_eq!(summary["error_count"], 1);

    let endpoints = snapshot["endpoints"].as_object().expect("endpoints");
    assert_eq!(endpoints.len(), 4);
    assert_eq!(endpoints["readiness"]["ok"], true);
    assert_eq!(
        endpoints["readiness"]["schema"],
        "ao2.cp-release-readiness.v1"
    );
    assert_eq!(endpoints["handoff"]["ok"], true);
    assert_eq!(
        endpoints["handoff"]["schema"],
        "ao2.cp-release-candidate-handoff.v1"
    );
    assert_eq!(endpoints["support_bundle_status"]["ok"], true);
    assert_eq!(endpoints["publication_latest"]["ok"], false);
    assert!(endpoints["publication_latest"]["error"]
        .as_str()
        .expect("error string")
        .contains("404"));

    // Sanity: every OK endpoint records body_bytes > 0 and a 64-hex sha256.
    for name in ["readiness", "handoff", "support_bundle_status"] {
        let ep = &endpoints[name];
        let bytes = ep["body_bytes"].as_u64().expect("body_bytes");
        assert!(bytes > 0, "{name} body_bytes should be > 0, got {bytes}");
        let sha = ep["body_sha256"].as_str().expect("body_sha256");
        assert_eq!(sha.len(), 64, "{name} sha256 should be 64 hex chars");
        assert!(
            sha.chars().all(|c| c.is_ascii_hexdigit()),
            "{name} sha256 must be hex"
        );
    }

    // Trust-boundary invariants
    assert_eq!(
        snapshot["trust_boundary"]["control_plane_role"],
        "read_only_observer"
    );
    assert_eq!(snapshot["trust_boundary"]["mutates_ao_artifacts"], false);

    // Canonical on-disk file must be sorted-keys, single-line JSON.
    let on_disk = std::fs::read_to_string(&out).expect("canonical written");
    let trimmed = on_disk.trim_end_matches('\n');
    assert!(
        trimmed.starts_with("{\"captured_at_utc\":"),
        "canonical JSON should start with the first sorted key, got: {}",
        &trimmed[..trimmed.len().min(80)]
    );
    // No literal newlines inside the JSON body (single-line canonical form).
    assert!(
        !trimmed.contains('\n'),
        "canonical JSON must be single-line, found newline"
    );
    // No key-quote followed by literal whitespace before the value:
    // canonical serde_json::to_string output uses `"key":value` with
    // no inserted spaces. Embedded string content with `: ` is fine —
    // we look only at key→value boundaries.
    let parsed: serde_json::Value = serde_json::from_str(trimmed).expect("on-disk JSON must parse");
    assert_eq!(parsed["schema_version"], "ao2.cp-release-snapshot.v1");
}

#[test]
fn cp_release_snapshot_propagates_endpoint_failures() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let port = listener.local_addr().expect("addr").port();

    let server = std::thread::spawn(move || {
        // All four endpoints return 500 — we want the snapshot to record
        // four errors and still emit a well-formed bundle.
        let body =
            r#"{"schema_version":"ao2.control-plane-error.v1","code":"internal","message":"boom"}"#;
        for _ in 0..4 {
            let mut stream = accept_one(&listener);
            handle_request(
                &mut stream,
                body,
                "application/json",
                "500 Internal Server Error",
            );
        }
    });

    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join("snap.json");
    let cp_url = format!("http://127.0.0.1:{port}");
    let snapshot = run_ao2_release_snapshot(&cp_url, "test-token", &out);

    server.join().expect("server thread");

    assert_eq!(snapshot["summary"]["endpoint_count"], 4);
    assert_eq!(snapshot["summary"]["ok_count"], 0);
    assert_eq!(snapshot["summary"]["error_count"], 4);
    let endpoints = snapshot["endpoints"].as_object().expect("endpoints");
    for (name, ep) in endpoints {
        assert_eq!(ep["ok"], false, "{name} should be err");
        let err = ep["error"].as_str().expect("error");
        assert!(
            err.contains("500"),
            "{name} error should mention 500, got: {err}"
        );
    }
}

#[test]
fn cp_release_snapshot_sends_bearer_authorization_header() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let port = listener.local_addr().expect("addr").port();
    let token = "snapshot-test-token";

    let server_token = token.to_string();
    let server = std::thread::spawn(move || {
        for _ in 0..4 {
            let mut stream = accept_one(&listener);
            let request = read_request_headers(&mut stream);
            assert!(
                request.contains(&format!("Authorization: Bearer {server_token}")),
                "request must carry Bearer token, got headers:\n{request}"
            );
            let body = r#"{"schema_version":"ao2.cp-release-readiness.v1","status":"ready"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            stream.flush().expect("flush response");
        }
    });

    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join("snap.json");
    let cp_url = format!("http://127.0.0.1:{port}");
    let snapshot = run_ao2_release_snapshot(&cp_url, token, &out);

    server.join().expect("server thread");
    assert_eq!(snapshot["summary"]["ok_count"], 4);
}
