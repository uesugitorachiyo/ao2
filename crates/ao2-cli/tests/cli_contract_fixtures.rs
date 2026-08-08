use std::fs;
use std::process::{Command, Output};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ContractFixture {
    cases: Vec<ContractCase>,
}

#[derive(Debug, Deserialize)]
struct ContractCase {
    name: String,
    args: Vec<String>,
    expected_status: i32,
    stdout_contains: Vec<String>,
    stderr_contains: Vec<String>,
    stderr_empty: bool,
}

fn ao2(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(args)
        .output()
        .expect("run ao2")
}

fn fixture() -> ContractFixture {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/cli-contract/help-surfaces.v1.json"
    );
    let bytes = fs::read(path).expect("read CLI contract fixture");
    serde_json::from_slice(&bytes).expect("parse CLI contract fixture")
}

#[test]
fn cli_help_and_safe_json_surfaces_match_contract_fixture() {
    let fixture = fixture();
    assert!(
        !fixture.cases.is_empty(),
        "CLI contract fixture must contain at least one case"
    );

    for case in fixture.cases {
        let output = ao2(&case.args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(case.expected_status),
            "case {} args {:?}\nstdout:\n{}\nstderr:\n{}",
            case.name,
            case.args,
            stdout,
            stderr
        );
        for needle in &case.stdout_contains {
            assert!(
                stdout.contains(needle),
                "case {} stdout missing {:?}\nstdout:\n{}",
                case.name,
                needle,
                stdout
            );
        }
        for needle in &case.stderr_contains {
            assert!(
                stderr.contains(needle),
                "case {} stderr missing {:?}\nstderr:\n{}",
                case.name,
                needle,
                stderr
            );
        }
        if case.stderr_empty {
            assert!(
                stderr.trim().is_empty(),
                "case {} expected empty stderr, got:\n{}",
                case.name,
                stderr
            );
        }
    }
}

#[test]
fn cli_help_uses_platform_neutral_binary_name() {
    let dir = tempfile::tempdir().expect("temporary directory");
    let renamed = dir
        .path()
        .join(format!("ao2-renamed{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_ao2"), &renamed).expect("copy AO2 executable");

    let output = Command::new(renamed)
        .arg("--help")
        .output()
        .expect("run renamed AO2 executable");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Usage: ao2 <COMMAND>"),
        "help exposed executable filename:\n{stdout}"
    );
}
