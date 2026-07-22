use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn cli_workbench_memory_search_api_returns_records_with_viewer_token() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let write = ao2([
        "memory",
        "write",
        "--target",
        repo.to_str().unwrap(),
        "--kind",
        "decision",
        "--title",
        "Hermes memory API",
        "--body",
        "Workbench exposes governed memory search to Hermes.",
        "--tag",
        "hermes",
        "--json",
    ]);
    assert!(write.status.success(), "{}", stderr(&write));
    let memory_id = serde_json::from_str::<serde_json::Value>(&stdout(&write)).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let port = line
        .trim()
        .strip_prefix("url=http://127.0.0.1:")
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        "GET /api/memory/search?token=test-token&query=Hermes&limit=5 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    let body = http_body(&response);
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["schema_version"], "ao2.memory-search.v1");
    assert_eq!(json["matches"][0]["id"], memory_id);
}

#[test]
fn cli_workbench_memory_recent_and_link_run_api_manage_records() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    let first = ao2([
        "memory",
        "write",
        "--target",
        repo.to_str().unwrap(),
        "--kind",
        "decision",
        "--title",
        "Earlier Hermes note",
        "--body",
        "This note should appear after the newer one.",
        "--json",
    ]);
    assert!(first.status.success(), "{}", stderr(&first));
    let first_id = serde_json::from_str::<serde_json::Value>(&stdout(&first)).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let second = ao2([
        "memory",
        "write",
        "--target",
        repo.to_str().unwrap(),
        "--kind",
        "conversation-summary",
        "--title",
        "Latest Hermes note",
        "--body",
        "Operators need to see recent AO2 memory without inventing a query.",
        "--tag",
        "hermes",
        "--json",
    ]);
    assert!(second.status.success(), "{}", stderr(&second));
    let second_id = serde_json::from_str::<serde_json::Value>(&stdout(&second)).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let recent_response = http_request(
        port,
        "GET /api/memory/recent?token=test-token&limit=2 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(
        recent_response.starts_with("HTTP/1.1 200 OK"),
        "{recent_response}"
    );
    let recent: serde_json::Value = serde_json::from_str(http_body(&recent_response)).unwrap();
    assert_eq!(recent["schema_version"], "ao2.memory-recent.v1");
    assert_eq!(recent["records"][0]["id"], second_id);
    assert_eq!(recent["records"][1]["id"], first_id);

    let body = format!("memory_id={second_id}&run_id=run-hermes&relationship=source");
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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let link_response = http_request(
        port,
        &format!(
            "POST /api/memory/link-run?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(
        link_response.starts_with("HTTP/1.1 200 OK"),
        "{link_response}"
    );
    let link: serde_json::Value = serde_json::from_str(http_body(&link_response)).unwrap();
    assert_eq!(link["schema_version"], "ao2.memory-run-link.v1");
    assert_eq!(link["memory_id"], second_id);
    assert_eq!(link["run_id"], "run-hermes");

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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let html_response = http_request(
        port,
        "GET /?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(html_response.contains("memory-recent-button"));
    assert!(html_response.contains("memory-link-run-form"));
    assert!(html_response.contains("/api/memory/recent"));
    assert!(html_response.contains("/api/memory/link-run"));
}

#[test]
fn cli_workbench_memory_publish_latest_api_and_controls() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("memory-repo");
    fs::create_dir_all(&repo).unwrap();

    let write = ao2([
        "memory",
        "write",
        "--target",
        repo.to_str().unwrap(),
        "--kind",
        "decision",
        "--title",
        "Workbench publish latest",
        "--body",
        "Operators can publish the newest memory export.",
    ]);
    assert!(write.status.success(), "{}", stderr(&write));

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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let export_body = "query=Workbench&limit=10";
    let export_response = http_request(
        port,
        &format!(
            "POST /api/memory/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            export_body.len(),
            export_body
        ),
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(
        export_response.starts_with("HTTP/1.1 200 OK"),
        "{export_response}"
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cp_port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= 100,
                        "timed out waiting for workbench publish request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/memory/export HTTP/1.1"));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"latest123","stored_at":"2026-05-19T00:00:00Z","ingested_schema_version":"ao2.memory-export.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    // Slice 19: this test exercises the legacy unsigned upload path through
    // the HTTP surface; opt into it explicitly via the escape-valve form
    // param so the slice-19 default-on fail-closed branch does not fire.
    let publish_body = format!(
        "control_plane_url=http://127.0.0.1:{cp_port}&api_token=cp-token&allow_unsigned_memory_export=1"
    );
    let publish_response = http_request(
        port,
        &format!(
            "POST /api/memory/publish-latest?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            publish_body.len(),
            publish_body
        ),
    );
    server.join().unwrap();
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(
        publish_response.starts_with("HTTP/1.1 200 OK"),
        "{publish_response}"
    );
    let published: serde_json::Value = serde_json::from_str(http_body(&publish_response)).unwrap();
    assert_eq!(
        published["schema_version"],
        "ao2.memory-control-plane-publish.v1"
    );
    assert_eq!(published["receipt"]["sha256"], "latest123");

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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let html_response = http_request(
        port,
        "GET /?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(html_response.contains("memory-publish-latest-form"));
    assert!(html_response.contains("/api/memory/publish-latest"));
    // Slice 19: HTML form must surface the escape-valve checkbox so the
    // workbench operator can opt out without hand-crafting a form param.
    assert!(
        html_response.contains("allow_unsigned_memory_export"),
        "memory-publish-latest-form missing escape-valve checkbox"
    );
}

#[test]
fn cli_workbench_memory_publish_latest_api_default_on_rejects_unsigned_when_sidecars_missing() {
    // Slice 19: producer-side default-on signed-memory-export upload for the
    // workbench HTTP surface. When the latest workbench memory export under
    // .ao2/workbench/memory-exports/ has no `.json.sig` + public-key sidecar,
    // POST /api/memory/publish-latest must fail-closed with HTTP 400 and
    // never reach the control plane, unless the operator opts out via the
    // `allow_unsigned_memory_export` form param.
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("memory-repo");
    fs::create_dir_all(&repo).unwrap();

    let write = ao2([
        "memory",
        "write",
        "--target",
        repo.to_str().unwrap(),
        "--kind",
        "decision",
        "--title",
        "Workbench publish default-on fixture",
        "--body",
        "Slice 19: HTTP surface must reject unsigned export by default.",
    ]);
    assert!(write.status.success(), "{}", stderr(&write));

    // Materialize the latest workbench memory export without signing.
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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let export_body = "query=Slice&limit=10";
    let export_response = http_request(
        port,
        &format!(
            "POST /api/memory/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            export_body.len(),
            export_body
        ),
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(
        export_response.starts_with("HTTP/1.1 200 OK"),
        "{export_response}"
    );

    // Bind a control-plane port but never accept — the workbench HTTP
    // handler must fail-closed before reaching the control plane.
    let cp_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    cp_listener.set_nonblocking(true).unwrap();
    let cp_port = cp_listener.local_addr().unwrap().port();

    // 1) Default-on: no escape valve → HTTP 400 + escape-valve guidance.
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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!("control_plane_url=http://127.0.0.1:{cp_port}&api_token=cp-token");
    let response = http_request(
        port,
        &format!(
            "POST /api/memory/publish-latest?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(
        response.starts_with("HTTP/1.1 400"),
        "expected default-on fail-closed; got {response}"
    );
    assert!(
        response.contains("memory publish requires a signed export by default"),
        "missing default-on error: {response}"
    );
    assert!(
        response.contains("allow-unsigned-memory-export"),
        "missing escape-valve guidance: {response}"
    );
    // Control-plane listener never received a connection.
    match cp_listener.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok(_) => panic!("workbench made an HTTP request before fail-closed check fired"),
        Err(error) => panic!("unexpected accept error: {error}"),
    }
}

#[test]
fn cli_workbench_memory_dashboard_proxy_api_and_controls() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let cp_port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut attempts = 0;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    attempts += 1;
                    assert!(
                        attempts <= 100,
                        "timed out waiting for dashboard proxy request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("GET /api/v1/memory/export/dashboard HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        let body = "<!doctype html><h1>AO2 Memory Exports</h1>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!("control_plane_url=http://127.0.0.1:{cp_port}&api_token=cp-token");
    let response = http_request(
        port,
        &format!(
            "POST /api/memory/control-plane-dashboard?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    server.join().unwrap();
    assert!(child.wait().unwrap().success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.memory-control-plane-dashboard.v1"
    );
    assert!(json["dashboard_html"]
        .as_str()
        .unwrap()
        .contains("AO2 Memory Exports"));

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
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let html_response = http_request(
        port,
        "GET /?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(child.wait().unwrap().success());
    assert!(html_response.contains("memory-control-plane-dashboard-form"));
    assert!(html_response.contains("/api/memory/control-plane-dashboard"));
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
        .args(["add", "."])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success());
    assert!(Command::new("git")
        .args(["commit", "-m", "fixture"])
        .current_dir(repo)
        .env("GIT_AUTHOR_DATE", "2024-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2024-01-01T00:00:00Z")
        .output()
        .unwrap()
        .status
        .success());
}

fn read_test_http_request(stream: &mut TcpStream, buffer: &mut [u8]) -> usize {
    let mut total = 0_usize;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    loop {
        match stream.read(&mut buffer[total..]) {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                let request = String::from_utf8_lossy(&buffer[..total]);
                if let Some(header_end) = request.find("\r\n\r\n") {
                    let headers = &request[..header_end];
                    let content_length = headers.lines().find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            if name.eq_ignore_ascii_case("content-length") {
                                value.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                    });
                    let expected = header_end + 4 + content_length.unwrap_or(0);
                    if total >= expected {
                        break;
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => panic!("failed to read test HTTP request: {error}"),
        }
    }
    total
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
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn http_request(port: u16, request: &str) -> String {
    let mut attempts = 0;
    let mut stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(error) if attempts < 50 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(20));
                if attempts == 50 {
                    panic!("failed to connect to workbench port {port}: {error}");
                }
            }
            Err(error) => panic!("failed to connect to workbench port {port}: {error}"),
        }
    };
    stream.write_all(request.as_bytes()).unwrap();
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
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .parse::<u16>()
        .unwrap()
}
