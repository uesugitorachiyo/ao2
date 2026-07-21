use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};

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

#[test]
fn cli_report_renders_evidence_cockpit_for_existing_run() {
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
printf 'Summary: cockpit run added validation around discount math\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Concern: tests should cover invalid discount rates\n'
printf 'Input tokens: 15\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "cockpit-cli-run",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let report = ao2([
        "report",
        "cockpit-cli-run",
        "--target",
        repo.to_str().unwrap(),
    ]);
    assert!(report.status.success(), "{}", stderr(&report));
    let report_path = stdout(&report);
    let report_path = report_path.strip_prefix("report=").unwrap().trim();
    let html = fs::read_to_string(report_path).unwrap();

    assert!(html.contains("AO2 Evidence Cockpit"));
    assert!(html.contains("cockpit run added validation around discount math"));
    assert!(html.contains("tests should cover invalid discount rates"));
    assert!(html.contains("policy_denied_git_push"));
    assert!(html.contains("sandbox_patch_apply"));
    assert!(html.contains("closure.accepted"));
    assert!(html.contains("Replay accepted"));
    assert!(html.contains("Run Health"));
    assert!(html.contains("repaired"));
    assert!(html.contains("No operator action required"));
    assert!(html.contains("Repair Attempts"));
    assert!(html.contains("review_missing_tests"));
}

#[test]
fn cli_runs_list_and_show_reports_existing_runs() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "runs-list-show",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let list = ao2(["runs", "list", "--target", repo.to_str().unwrap(), "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let list_json: serde_json::Value = serde_json::from_str(&stdout(&list)).unwrap();
    assert_eq!(list_json["schema_version"], "ao2.runs-list.v1");
    assert_eq!(list_json["runs"][0]["run_id"], "runs-list-show");
    assert_eq!(list_json["runs"][0]["status"], "accepted");
    assert_eq!(list_json["runs"][0]["digest_failures"], 0);
    assert!(Path::new(list_json["runs"][0]["evidence_pack"].as_str().unwrap()).is_file());

    let show = ao2([
        "runs",
        "show",
        "runs-list-show",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json: serde_json::Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(show_json["schema_version"], "ao2.runs-show.v1");
    assert_eq!(show_json["run"]["run_id"], "runs-list-show");
    assert_eq!(show_json["run"]["status"], "accepted");
    assert!(show_json["run"]["workflow_id"]
        .as_str()
        .unwrap()
        .starts_with("risky-pr-run"));
    assert!(!show_json["run"]["objective"].as_str().unwrap().is_empty());
}

#[test]
fn cli_report_open_prints_browser_target_for_existing_cockpit() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "cockpit-open-run",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let report = ao2_with_env(
        [
            "report",
            "cockpit-open-run",
            "--target",
            repo.to_str().unwrap(),
            "--open",
        ],
        [("AO2_TEST_NO_OPEN", "1")],
    );
    assert!(report.status.success(), "{}", stderr(&report));
    let output = stdout(&report);
    assert!(output.contains("report="));
    assert!(output.contains("open_target="));
}

#[test]
fn cli_cockpit_serve_once_returns_existing_report_html() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "cockpit-serve-run",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "cockpit",
            "serve",
            "cockpit-serve-run",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--once",
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
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("AO2 Evidence Cockpit"));
    assert!(response.contains("cockpit-serve-run"));
}

#[test]
fn cli_cockpit_index_lists_runs_and_serves_index_html() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "cockpit-index-run",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));

    let index = ao2(["cockpit", "index", "--target", repo.to_str().unwrap()]);
    assert!(index.status.success(), "{}", stderr(&index));
    let index_path = stdout(&index);
    let index_path = index_path.strip_prefix("cockpit_index=").unwrap().trim();
    let html = fs::read_to_string(index_path).unwrap();
    assert!(html.contains("AO2 Cockpit"));
    assert!(html.contains("cockpit-index-run"));
    assert!(html.contains("accepted"));

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "cockpit",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--index",
            "--port",
            "0",
            "--once",
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
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(response.contains("AO2 Cockpit"));
    assert!(response.contains("cockpit-index-run"));
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
