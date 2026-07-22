use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn cli_workbench_operator_tokens_enforce_roles_and_render_identity() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let signing_key = temp.path().join("operator-support-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-operator-demo",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "admin-token",
            "--operator-token",
            "viewer:viewer:viewer-token",
            "--operator-token",
            "ops:operator:operator-token",
            "--enable-execution",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "workbench-operator-token-test",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let no_token = http_request(
        port,
        "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(no_token.starts_with("HTTP/1.1 403 Forbidden"), "{no_token}");

    let viewer_html = http_request(
        port,
        "GET /?token=viewer-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(viewer_html.starts_with("HTTP/1.1 200 OK"), "{viewer_html}");
    assert!(viewer_html.contains("data-operator-role=\"viewer\""));
    assert!(viewer_html.contains("data-can-operate=\"false\""));
    assert!(viewer_html.contains("Operator Role"));
    assert!(viewer_html.contains("viewer"));

    let viewer_runs = http_request(
        port,
        "GET /api/runs?token=viewer-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(viewer_runs.starts_with("HTTP/1.1 200 OK"), "{viewer_runs}");
    let runs_json: serde_json::Value = serde_json::from_str(http_body(&viewer_runs)).unwrap();
    assert_eq!(runs_json["schema_version"], "ao2.runs-list.v1");

    let launch_body = "template=bug-fix&provider=scripted&run_id=role-demo&max_repair_attempts=1";
    let viewer_launch = http_request(
        port,
        &format!(
            "POST /api/launch?token=viewer-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            launch_body.len(),
            launch_body
        ),
    );
    assert!(
        viewer_launch.starts_with("HTTP/1.1 403 Forbidden"),
        "{viewer_launch}"
    );
    let viewer_launch_json: serde_json::Value =
        serde_json::from_str(http_body(&viewer_launch)).unwrap();
    assert_eq!(viewer_launch_json["error"], "insufficient_operator_role");
    assert_eq!(viewer_launch_json["required_role"], "operator");
    assert_eq!(viewer_launch_json["operator_role"], "viewer");

    let operator_launch = http_request(
        port,
        &format!(
            "POST /api/launch?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            launch_body.len(),
            launch_body
        ),
    );
    assert!(
        operator_launch.starts_with("HTTP/1.1 200 OK"),
        "{operator_launch}"
    );
    let operator_launch_json: serde_json::Value =
        serde_json::from_str(http_body(&operator_launch)).unwrap();
    assert_eq!(
        operator_launch_json["schema_version"],
        "ao2.workbench-launch.v1"
    );

    let operator_html = http_request(
        port,
        "GET /?token=operator-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(
        operator_html.starts_with("HTTP/1.1 200 OK"),
        "{operator_html}"
    );
    assert!(operator_html.contains("<option value=\"claude\">Claude</option>"));

    child.kill().ok();
    child.wait().ok();
}

#[test]
fn cli_workbench_operator_token_rejects_invalid_role_config() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let output = ao2([
        "workbench",
        "serve",
        "--target",
        repo.to_str().unwrap(),
        "--port",
        "0",
        "--operator-token",
        "bad:owner:token",
    ]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("invalid workbench operator role owner"));
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
