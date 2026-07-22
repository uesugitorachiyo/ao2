#![allow(dead_code, unused_imports)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use tar::Archive;

#[test]
fn cli_workbench_release_summary_enrich_and_gate_api() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let dist = temp.path().join("dist");
    let provenance = temp.path().join("dist-provenance");
    let summary_path = temp.path().join("summary.json");
    let enriched_path = temp.path().join("summary.enriched.json");
    let api_gate_artifact_path = temp.path().join("release-gate-api-artifact.json");

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "workbench-release-gate-source",
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    let evidence_dir = repo
        .join(".ao2")
        .join("runs")
        .join("workbench-release-gate-source")
        .join("evidence-pack");
    fs::write(
        evidence_dir.join("obligation-gate-closure.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.obligation-gate.v1",
            "stage": "closure",
            "status": "passed",
            "verdict": "accepted",
            "summary": {"pass": 2, "fail": 0, "unverified": 0, "waived": 0}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "local_smoke": "passed",
            "native_windows_required": false,
            "windows_native_smoke": "skipped",
            "windows_skip_reason": "windows_ssh_unreachable"
        }))
        .unwrap(),
    )
    .unwrap();

    let package = ao2([
        "release",
        "package",
        "--out-dir",
        dist.to_str().unwrap(),
        "--version",
        "9.9.9-workbench",
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    let archive = package_json["archive"].as_str().unwrap();
    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-workbench")
        .env("AO2_MACOS_ARCHIVE", archive)
        .env("AO2_LINUX_ARCHIVE", archive)
        .env("AO2_LINUX_X86_64_ARCHIVE", archive)
        .env("AO2_WINDOWS_ARCHIVE", archive)
        .env("AO2_RELEASE_PROVENANCE_DIR", &provenance)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));

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
    let body = format!(
        "summary={}&out={}",
        percent_encode_for_test(summary_path.to_str().unwrap()),
        percent_encode_for_test(enriched_path.to_str().unwrap())
    );
    let response = http_request(
        port,
        &format!(
            "POST /api/release-summary/enrich?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    assert!(child.wait().unwrap().success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema"], "ao2.release-summary-enrich.v1");
    assert_eq!(json["run_id"], "workbench-release-gate-source");
    assert!(enriched_path.is_file());

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
    // Slice 17: workbench /api/release-gate defaults to require-signing.
    // This fixture's enriched summary has no obligation_gates block, so we
    // explicitly opt out of the signing requirement via the new escape valve
    // form param. A separate slice-17 test exercises the default-on behaviour
    // and asserts it fails closed on this same fixture.
    let body = format!(
        "summary={}&provenance_dir={}&macos_archive={}&linux_archive={}&linux_x86_64_archive={}&windows_archive={}&artifact_out={}&allow_unsigned_obligation_gates=1",
        percent_encode_for_test(enriched_path.to_str().unwrap()),
        percent_encode_for_test(provenance.to_str().unwrap()),
        percent_encode_for_test(archive),
        percent_encode_for_test(archive),
        percent_encode_for_test(archive),
        percent_encode_for_test(archive),
        percent_encode_for_test(api_gate_artifact_path.to_str().unwrap())
    );
    let response = http_request(
        port,
        &format!(
            "POST /api/release-gate?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    assert!(child.wait().unwrap().success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema"], "ao2.release-gate.v1");
    assert_eq!(json["status"], "verified");
    assert_eq!(
        json["artifact_path"],
        api_gate_artifact_path.to_str().unwrap()
    );
    assert!(api_gate_artifact_path.is_file());
    let gate_artifact_path = temp.path().join("release-gate-artifact.json");
    fs::write(
        &gate_artifact_path,
        serde_json::to_string_pretty(&json).unwrap(),
    )
    .unwrap();

    // Slice 17: post the same fixture WITHOUT the escape valve and assert the
    // gate fails closed because obligation-gate signing is now required by
    // default. The fixture's enriched summary has no obligation_gates block,
    // so the signing-verification subreport surfaces
    // `obligation_gate_signing_no_gates` which flips the gate to failed.
    let default_on_artifact_path = temp.path().join("release-gate-default-on-artifact.json");
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
    let default_on_body = format!(
        "summary={}&provenance_dir={}&macos_archive={}&linux_archive={}&linux_x86_64_archive={}&windows_archive={}&artifact_out={}",
        percent_encode_for_test(enriched_path.to_str().unwrap()),
        percent_encode_for_test(provenance.to_str().unwrap()),
        percent_encode_for_test(archive),
        percent_encode_for_test(archive),
        percent_encode_for_test(archive),
        percent_encode_for_test(archive),
        percent_encode_for_test(default_on_artifact_path.to_str().unwrap())
    );
    let default_on_response = http_request(
        port,
        &format!(
            "POST /api/release-gate?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            default_on_body.len(),
            default_on_body
        ),
    );
    assert!(child.wait().unwrap().success());
    // The workbench dispatcher returns HTTP 400 when the release gate status
    // is not "verified" (so callers can fail closed on the HTTP layer too).
    assert!(
        default_on_response.starts_with("HTTP/1.1 400 Bad Request"),
        "{default_on_response}"
    );
    let default_on_json: serde_json::Value =
        serde_json::from_str(http_body(&default_on_response)).unwrap();
    assert_eq!(default_on_json["schema"], "ao2.release-gate.v1");
    assert_eq!(
        default_on_json["status"], "failed",
        "without allow_unsigned_obligation_gates the workbench /api/release-gate must fail closed: {default_on_json:#?}"
    );
    // The report must surface an obligation_gate_signing block (default-on
    // mirrors the CLI's slice-11 behaviour) and one of its reasons must
    // explain why it failed.
    let signing_block = &default_on_json["obligation_gate_signing"];
    assert!(
        signing_block.is_object(),
        "obligation_gate_signing must be populated by default: {default_on_json:#?}"
    );
    let reasons = default_on_json["reasons"]
        .as_array()
        .expect("release-gate reasons array");
    assert!(
        reasons.iter().any(|reason| reason["code"]
            .as_str()
            .map(|code| code == "obligation_gate_signing_unverified")
            .unwrap_or(false)),
        "expected obligation_gate_signing_unverified reason in {reasons:#?}"
    );

    // Slice 17 back-compat: the legacy `require_obligation_gate_signing` form
    // param is accepted but ignored (no-op). Posting it without the escape
    // valve must still fail closed.
    let legacy_artifact_path = temp.path().join("release-gate-legacy-artifact.json");
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
    let legacy_body = format!(
        "summary={}&provenance_dir={}&macos_archive={}&linux_archive={}&linux_x86_64_archive={}&windows_archive={}&artifact_out={}&require_obligation_gate_signing=1",
        percent_encode_for_test(enriched_path.to_str().unwrap()),
        percent_encode_for_test(provenance.to_str().unwrap()),
        percent_encode_for_test(archive),
        percent_encode_for_test(archive),
        percent_encode_for_test(archive),
        percent_encode_for_test(archive),
        percent_encode_for_test(legacy_artifact_path.to_str().unwrap())
    );
    let legacy_response = http_request(
        port,
        &format!(
            "POST /api/release-gate?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            legacy_body.len(),
            legacy_body
        ),
    );
    assert!(child.wait().unwrap().success());
    // Same HTTP-400 contract as the default-on case above.
    assert!(
        legacy_response.starts_with("HTTP/1.1 400 Bad Request"),
        "{legacy_response}"
    );
    let legacy_json: serde_json::Value = serde_json::from_str(http_body(&legacy_response)).unwrap();
    assert_eq!(legacy_json["schema"], "ao2.release-gate.v1");
    assert_eq!(
        legacy_json["status"], "failed",
        "legacy require_obligation_gate_signing=1 must be a no-op (signing already required by default) so this still fails closed: {legacy_json:#?}"
    );

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
    let response = http_request(
        port,
        &format!(
            "GET /api/release-gate/artifact?token=test-token&path={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            percent_encode_for_test(gate_artifact_path.to_str().unwrap())
        ),
    );
    assert!(child.wait().unwrap().success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let artifact: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(artifact["schema"], "ao2.workbench-release-gate-artifact.v1");
    assert_eq!(artifact["artifact"]["schema"], "ao2.release-gate.v1");
    assert_eq!(artifact["artifact"]["status"], "verified");

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
        &format!(
            "GET /?token=test-token&release_gate_artifact={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            percent_encode_for_test(gate_artifact_path.to_str().unwrap())
        ),
    );
    assert!(child.wait().unwrap().success());
    assert!(html_response.contains("release-summary-enrich-form"));
    assert!(html_response.contains("/api/release-summary/enrich"));
    assert!(html_response.contains("release-gate-form"));
    assert!(html_response.contains("/api/release-gate"));
    assert!(html_response.contains("release-gate-artifact-form"));
    assert!(html_response.contains("release-gate-artifact-path"));
    assert!(html_response.contains(&format!(
        "value=\"{}\"",
        gate_artifact_path.to_str().unwrap()
    )));
    assert!(html_response.contains("/api/release-gate/artifact"));
}
#[test]
fn cli_workbench_release_health_api_checks_release_assets() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let assets = temp.path().join("release-assets");
    fs::create_dir_all(&assets).unwrap();
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let macos = package_test_archive(temp.path(), "macos-aarch64");
    let linux = package_test_archive(temp.path(), "linux-aarch64");
    let linux_x86_64 = package_test_archive(temp.path(), "linux-x86_64");
    let windows = package_test_archive(temp.path(), "windows-x86_64");
    let sign = release_sign_command()
        .env("AO2_VERSION", "9.9.9-test")
        .env("AO2_MACOS_ARCHIVE", &macos)
        .env("AO2_LINUX_ARCHIVE", &linux)
        .env("AO2_LINUX_X86_64_ARCHIVE", &linux_x86_64)
        .env("AO2_WINDOWS_ARCHIVE", &windows)
        .env("AO2_RELEASE_PROVENANCE_DIR", &assets)
        .env(
            "AO2_RELEASE_PRIVATE_KEY",
            temp.path().join("release-key.pem"),
        )
        .output()
        .unwrap();
    assert!(sign.status.success(), "{}", stderr(&sign));
    for archive in [&macos, &linux, &linux_x86_64, &windows] {
        fs::copy(archive, assets.join(archive.file_name().unwrap())).unwrap();
    }
    fs::write(
        assets.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.9-test",
            "release_repo": "uesugitorachiyo/ao2",
            "status": "verified",
            "platforms": {
                "macos-aarch64": {
                    "status": "passed",
                    "marker": "macos_download_rollback=passed"
                },
                "linux-x86_64": {
                    "status": "passed",
                    "marker": "ubuntu_download_rollback=passed"
                },
                "windows-x86_64": {
                    "status": "passed",
                    "marker": "windows_download_rollback=passed"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--provenance-dir",
            assets.to_str().unwrap(),
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
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        &format!(
            "GET /api/release-health?token=test-token&release=v9.9.9-test&release_asset_dir={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            assets.display()
        ),
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");

    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.doctor.v1");
    assert_eq!(json["release"]["release_tag"], "v9.9.9-test");
    assert_eq!(json["release"]["asset_source"], "directory");
    assert_eq!(json["release"]["assets_available"], true);
    assert_eq!(json["release"]["provenance_verified"], true);
    assert_eq!(json["release"]["rollback"]["checked"], true);
    assert_eq!(json["release"]["rollback"]["status"], "verified");
    assert_eq!(
        json["release"]["rollback"]["schema_version"],
        "ao2.release-rollback-summary.v1"
    );
    assert_eq!(
        json["release"]["rollback"]["platforms"]["windows-x86_64"]["status"],
        "passed"
    );
}
#[test]
fn cli_workbench_release_history_api_compares_downloaded_releases() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let releases = temp.path().join("release-download");
    let v1 = releases.join("v9.9.8-test");
    let v2 = releases.join("v9.9.9-test");
    fs::create_dir_all(&v1).unwrap();
    fs::create_dir_all(&v2).unwrap();
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    fs::write(
        v1.join("release-doctor.json"),
        serde_json::json!({
            "status": "attention",
            "release": {
                "release_tag": "v9.9.8-test",
                "assets_available": false,
                "asset_count": 14,
                "provenance_verified": true,
                "provenance_tag_matches": true
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        v1.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.8-test",
            "status": "incomplete",
            "platforms": {
                "macos-aarch64": {"status": "passed"},
                "linux-x86_64": {"status": "passed"},
                "windows-x86_64": {"status": "skipped"}
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        v2.join("release-doctor.json"),
        serde_json::json!({
            "status": "ok",
            "release": {
                "release_tag": "v9.9.9-test",
                "assets_available": true,
                "asset_count": 15,
                "provenance_verified": true,
                "provenance_tag_matches": true
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        v2.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.9-test",
            "status": "verified",
            "platforms": {
                "macos-aarch64": {"status": "passed"},
                "linux-x86_64": {"status": "passed"},
                "windows-x86_64": {"status": "passed"}
            }
        })
        .to_string(),
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
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap()
        .parse::<u16>()
        .unwrap();

    let response = http_request(
        port,
        &format!(
            "GET /api/release-history?token=test-token&release_download_dir={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            releases.display()
        ),
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");

    let json: serde_json::Value = serde_json::from_str(http_body(&response)).unwrap();
    assert_eq!(json["schema_version"], "ao2.release-history.v1");
    assert_eq!(
        json["release_download_dir"].as_str().unwrap(),
        releases.to_string_lossy()
    );
    assert_eq!(json["trend"]["entry_count"], 2);
    assert_eq!(json["trend"]["latest_release_tag"], "v9.9.9-test");
    assert_eq!(json["trend"]["latest_health_score"], 6);
    assert_eq!(json["trend"]["regression_count"], 0);
    assert_eq!(json["trend"]["attention_count"], 1);
    assert_eq!(json["entries"].as_array().unwrap().len(), 2);
    assert_eq!(json["entries"][0]["release_tag"], "v9.9.9-test");
    assert_eq!(json["entries"][0]["status"], "ok");
    assert_eq!(json["entries"][0]["assets_available"], true);
    assert_eq!(json["entries"][0]["rollback_status"], "verified");
    assert_eq!(json["entries"][0]["platforms"]["windows-x86_64"], "passed");
    assert_eq!(json["entries"][0]["health_score"], 6);
    assert_eq!(json["entries"][0]["trend_status"], "improved");
    assert_eq!(json["entries"][0]["previous_release_tag"], "v9.9.8-test");
    assert_eq!(json["entries"][1]["release_tag"], "v9.9.8-test");
    assert_eq!(json["entries"][1]["rollback_status"], "incomplete");
    assert_eq!(json["entries"][1]["trend_status"], "baseline");
}
#[test]
fn cli_workbench_release_history_export_attaches_to_signed_support_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let releases = temp.path().join("release-download");
    let v1 = releases.join("v9.9.9-test");
    fs::create_dir_all(&v1).unwrap();
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    fs::write(
        v1.join("release-doctor.json"),
        serde_json::json!({
            "status": "ok",
            "release": {
                "release_tag": "v9.9.9-test",
                "assets_available": true,
                "asset_count": 15,
                "provenance_verified": true,
                "provenance_tag_matches": true
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        v1.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.9-test",
            "status": "verified",
            "platforms": {
                "macos-aarch64": {"status": "passed"},
                "linux-x86_64": {"status": "passed"},
                "windows-x86_64": {"status": "passed"}
            }
        })
        .to_string(),
    )
    .unwrap();
    let signing_key = temp.path().join("release-history-support-key.pem");
    generate_native_signing_key(&signing_key, 3072);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "test-token",
            "--enable-execution",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "release-history-test",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let export_body = format!(
        "kind=release-history&release_download_dir={}",
        releases.display()
    );
    let evidence_export_response = http_request(
        port,
        &format!(
            "POST /api/runs/evidence/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            export_body.len(),
            export_body
        ),
    );
    assert!(
        evidence_export_response.starts_with("HTTP/1.1 200 OK"),
        "{evidence_export_response}"
    );
    let evidence_export: serde_json::Value =
        serde_json::from_str(http_body(&evidence_export_response)).unwrap();
    assert_eq!(evidence_export["export_kind"], "release-history");
    assert_eq!(
        evidence_export["export"]["release_history"]["trend"]["latest_release_tag"],
        "v9.9.9-test"
    );
    let export_path = PathBuf::from(evidence_export["export_path"].as_str().unwrap());
    assert!(export_path.is_file(), "{}", export_path.display());

    let support_response = http_request(
        port,
        "POST /api/queue/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(
        support_response.starts_with("HTTP/1.1 200 OK"),
        "{support_response}"
    );
    let support_export: serde_json::Value =
        serde_json::from_str(http_body(&support_response)).unwrap();
    assert_eq!(support_export["support_metadata"]["present"], true);
    assert_eq!(
        support_export["support_metadata"]["signature_verified"],
        true
    );
    assert_eq!(
        support_export["support_metadata"]["metadata"]["evidence_export_count"],
        1
    );
    let bundle_path = PathBuf::from(support_export["bundle_path"].as_str().unwrap());
    let bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(bundle_path).unwrap()).unwrap();
    assert_eq!(bundle["evidence_exports"].as_array().unwrap().len(), 1);
    assert_eq!(bundle["evidence_exports"][0]["kind"], "release-history");
    assert_eq!(
        bundle["evidence_exports"][0]["content"]["export"]["release_history"]["trend"]
            ["latest_release_tag"],
        "v9.9.9-test"
    );
    let _ = child.kill();
    let _ = child.wait();
}
#[test]
fn cli_workbench_release_comparison_api_generates_and_verifies_signed_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let releases = temp.path().join("release-download");
    let out_dir = temp.path().join("release-comparison-bundles");
    let release = releases.join("v9.9.9-test");
    fs::create_dir_all(&release).unwrap();
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    fs::write(
        release.join("release-doctor.json"),
        serde_json::json!({
            "status": "ok",
            "release": {
                "release_tag": "v9.9.9-test",
                "assets_available": true,
                "asset_count": 15,
                "provenance_verified": true,
                "provenance_tag_matches": true
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        release.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.9-test",
            "status": "verified",
            "platforms": {
                "macos-aarch64": {"status": "passed"},
                "linux-x86_64": {"status": "passed"},
                "windows-x86_64": {"status": "passed"}
            }
        })
        .to_string(),
    )
    .unwrap();
    let signing_key = temp.path().join("release-comparison-support-key.pem");
    generate_native_signing_key(&signing_key, 3072);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "test-token",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "release-comparison-test",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let create_body = format!(
        "release_download_dir={}&out_dir={}",
        releases.display(),
        out_dir.display()
    );
    let create_response = http_request(
        port,
        &format!(
            "POST /api/release-comparison?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            create_body.len(),
            create_body
        ),
    );
    assert!(
        create_response.starts_with("HTTP/1.1 200 OK"),
        "{create_response}"
    );
    let created: serde_json::Value = serde_json::from_str(http_body(&create_response)).unwrap();
    assert_eq!(
        created["schema_version"],
        "ao2.workbench-release-comparison.v1"
    );
    assert_eq!(
        created["release_comparison"]["schema_version"],
        "ao2.release-comparison-bundle.v1"
    );
    assert_eq!(
        created["release_comparison"]["support_metadata"]["signature_verified"],
        true
    );
    assert_eq!(
        created["release_comparison"]["support_metadata"]["signer_id"],
        "release-comparison-test"
    );
    let bundle_dir = created["release_comparison"]["bundle_dir"]
        .as_str()
        .unwrap();
    assert!(PathBuf::from(bundle_dir)
        .join("release-comparison.json")
        .is_file());

    let verify_response = http_request(
        port,
        &format!(
            "GET /api/release-comparison/verify?token=test-token&bundle_dir={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            bundle_dir
        ),
    );
    assert!(
        verify_response.starts_with("HTTP/1.1 200 OK"),
        "{verify_response}"
    );
    let verified: serde_json::Value = serde_json::from_str(http_body(&verify_response)).unwrap();
    assert_eq!(
        verified["schema_version"],
        "ao2.workbench-release-comparison-verification.v1"
    );
    assert_eq!(verified["verification"]["status"], "verified");
    assert_eq!(verified["verification"]["manifest_verified"], true);
    assert_eq!(verified["verification"]["signature_verified"], true);
    assert_eq!(
        verified["verification"]["latest_release_tag"],
        "v9.9.9-test"
    );
    let _ = child.kill();
    let _ = child.wait();
}
#[test]
fn cli_workbench_release_comparison_export_attaches_to_signed_support_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let releases = temp.path().join("release-download");
    let out_dir = temp.path().join("release-comparison-bundles");
    let release = releases.join("v9.9.9-test");
    fs::create_dir_all(&release).unwrap();
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    fs::write(
        release.join("release-doctor.json"),
        serde_json::json!({
            "status": "ok",
            "release": {
                "release_tag": "v9.9.9-test",
                "assets_available": true,
                "asset_count": 15,
                "provenance_verified": true,
                "provenance_tag_matches": true
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        release.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.9-test",
            "status": "verified",
            "platforms": {
                "macos-aarch64": {"status": "passed"},
                "linux-x86_64": {"status": "passed"},
                "windows-x86_64": {"status": "passed"}
            }
        })
        .to_string(),
    )
    .unwrap();
    let signing_key = temp.path().join("release-comparison-support-key.pem");
    generate_native_signing_key(&signing_key, 3072);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "test-token",
            "--enable-execution",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "release-comparison-export-test",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let create_body = format!(
        "release_download_dir={}&out_dir={}",
        releases.display(),
        out_dir.display()
    );
    let create_response = http_request(
        port,
        &format!(
            "POST /api/release-comparison?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            create_body.len(),
            create_body
        ),
    );
    assert!(
        create_response.starts_with("HTTP/1.1 200 OK"),
        "{create_response}"
    );
    let created: serde_json::Value = serde_json::from_str(http_body(&create_response)).unwrap();
    let bundle_dir = created["release_comparison"]["bundle_dir"]
        .as_str()
        .unwrap();

    let export_body = format!("kind=release-comparison-verification&bundle_dir={bundle_dir}");
    let evidence_export_response = http_request(
        port,
        &format!(
            "POST /api/runs/evidence/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            export_body.len(),
            export_body
        ),
    );
    assert!(
        evidence_export_response.starts_with("HTTP/1.1 200 OK"),
        "{evidence_export_response}"
    );
    let evidence_export: serde_json::Value =
        serde_json::from_str(http_body(&evidence_export_response)).unwrap();
    assert_eq!(
        evidence_export["export_kind"],
        "release-comparison-verification"
    );
    assert_eq!(
        evidence_export["export"]["release_comparison_verification"]["status"],
        "verified"
    );
    assert_eq!(
        evidence_export["export"]["release_comparison_verification"]["latest_release_tag"],
        "v9.9.9-test"
    );
    assert_eq!(
        evidence_export["export"]["release_comparison_verification"]["signature_verified"],
        true
    );
    let export_path = PathBuf::from(evidence_export["export_path"].as_str().unwrap());
    assert!(export_path.is_file(), "{}", export_path.display());

    let support_response = http_request(
        port,
        "POST /api/queue/export?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(
        support_response.starts_with("HTTP/1.1 200 OK"),
        "{support_response}"
    );
    let support_export: serde_json::Value =
        serde_json::from_str(http_body(&support_response)).unwrap();
    assert_eq!(
        support_export["support_metadata"]["metadata"]["evidence_export_count"],
        1
    );
    let support_bundle_path = PathBuf::from(support_export["bundle_path"].as_str().unwrap());
    let support_bundle_dir = support_bundle_path.parent().unwrap().to_path_buf();
    let bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&support_bundle_path).unwrap()).unwrap();
    assert_eq!(
        bundle["evidence_exports"][0]["kind"],
        "release-comparison-verification"
    );
    assert_eq!(
        bundle["evidence_exports"][0]["content"]["export"]["release_comparison_verification"]
            ["latest_release_tag"],
        "v9.9.9-test"
    );
    let _ = child.kill();
    let _ = child.wait();

    let inspect = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        support_bundle_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(inspect.status.success(), "{}", stderr(&inspect));
    let inspect_json: serde_json::Value = serde_json::from_str(&stdout(&inspect)).unwrap();
    assert_eq!(
        inspect_json["evidence_exports"][0]["kind"],
        "release-comparison-verification"
    );
    assert_eq!(
        inspect_json["evidence_exports"][0]["release_comparison_latest_release_tag"],
        "v9.9.9-test"
    );
    assert_eq!(
        inspect_json["evidence_exports"][0]["release_comparison_signature_verified"],
        true
    );

    let inspect_text = ao2([
        "workbench",
        "support-inspect",
        "--bundle-dir",
        support_bundle_dir.to_str().unwrap(),
    ]);
    assert!(inspect_text.status.success(), "{}", stderr(&inspect_text));
    assert!(stdout(&inspect_text).contains(
        "evidence_export_1=release-comparison-verification v9.9.9-test releases=1 regressions=0"
    ));
}
#[test]
fn cli_workbench_release_comparison_latest_returns_newest_verified_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let releases = temp.path().join("release-download");
    let out_dir = temp.path().join("release-comparison-bundles");
    let release = releases.join("v9.9.9-test");
    fs::create_dir_all(&release).unwrap();
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    fs::write(
        release.join("release-doctor.json"),
        serde_json::json!({
            "status": "ok",
            "release": {
                "release_tag": "v9.9.9-test",
                "assets_available": true,
                "asset_count": 15,
                "provenance_verified": true,
                "provenance_tag_matches": true
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        release.join("release-rollback-summary.json"),
        serde_json::json!({
            "schema_version": "ao2.release-rollback-summary.v1",
            "release_tag": "v9.9.9-test",
            "status": "verified",
            "platforms": {
                "macos-aarch64": {"status": "passed"},
                "linux-x86_64": {"status": "passed"},
                "windows-x86_64": {"status": "passed"}
            }
        })
        .to_string(),
    )
    .unwrap();
    let signing_key = temp.path().join("release-comparison-support-key.pem");
    generate_native_signing_key(&signing_key, 3072);
    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "test-token",
            "--support-signing-key",
            signing_key.to_str().unwrap(),
            "--support-signer-id",
            "release-comparison-latest-test",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);

    let create_body = format!(
        "release_download_dir={}&out_dir={}",
        releases.display(),
        out_dir.display()
    );
    let create_response = http_request(
        port,
        &format!(
            "POST /api/release-comparison?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            create_body.len(),
            create_body
        ),
    );
    assert!(
        create_response.starts_with("HTTP/1.1 200 OK"),
        "{create_response}"
    );
    let created: serde_json::Value = serde_json::from_str(http_body(&create_response)).unwrap();
    let created_bundle_dir = created["release_comparison"]["bundle_dir"]
        .as_str()
        .unwrap();

    let latest_response = http_request(
        port,
        &format!(
            "GET /api/release-comparison/latest?token=test-token&bundle_root={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            out_dir.display()
        ),
    );
    assert!(
        latest_response.starts_with("HTTP/1.1 200 OK"),
        "{latest_response}"
    );
    let latest: serde_json::Value = serde_json::from_str(http_body(&latest_response)).unwrap();
    assert_eq!(
        latest["schema_version"],
        "ao2.workbench-latest-release-comparison.v1"
    );
    assert_eq!(latest["bundle_dir"], created_bundle_dir);
    assert_eq!(latest["verification"]["status"], "verified");
    assert_eq!(latest["verification"]["latest_release_tag"], "v9.9.9-test");
    assert_eq!(latest["verification"]["signature_verified"], true);
    assert_eq!(latest["candidates_checked"], 1);
    let _ = child.kill();
    let _ = child.wait();
}
#[test]
fn cli_workbench_release_retention_preview_and_prune_removes_old_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    let releases = temp.path().join("release-download");
    let bundles = temp.path().join("release-comparison-bundles");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    for tag in ["v9.9.7-test", "v9.9.8-test", "v9.9.9-test"] {
        let dir = releases.join(tag);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("release-doctor.json"), "{}").unwrap();
    }
    for name in [
        "release-comparison-0001",
        "release-comparison-0002",
        "release-comparison-0003",
    ] {
        let dir = bundles.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("release-comparison.json"), "{}").unwrap();
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "workbench",
            "serve",
            "--target",
            repo.to_str().unwrap(),
            "--port",
            "0",
            "--api-token",
            "test-token",
        ])
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let port = read_server_port(&mut child);
    let body = format!(
        "release_download_dir={}&bundle_root={}&keep_releases=1&keep_bundles=2&dry_run=1",
        releases.display(),
        bundles.display()
    );
    let preview_response = http_request(
        port,
        &format!(
            "POST /api/release-retention/prune?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        ),
    );
    assert!(
        preview_response.starts_with("HTTP/1.1 200 OK"),
        "{preview_response}"
    );
    let preview: serde_json::Value = serde_json::from_str(http_body(&preview_response)).unwrap();
    assert_eq!(
        preview["schema_version"],
        "ao2.workbench-release-retention-prune.v1"
    );
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["removed_release_count"], 2);
    assert_eq!(preview["removed_bundle_count"], 1);
    assert_eq!(preview["total_removed_count"], 3);
    assert!(releases.join("v9.9.7-test").is_dir());
    assert!(bundles.join("release-comparison-0001").is_dir());

    let prune_body = body.replace("dry_run=1", "dry_run=0");
    let prune_response = http_request(
        port,
        &format!(
            "POST /api/release-retention/prune?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            prune_body.len(),
            prune_body
        ),
    );
    assert!(
        prune_response.starts_with("HTTP/1.1 200 OK"),
        "{prune_response}"
    );
    let pruned: serde_json::Value = serde_json::from_str(http_body(&prune_response)).unwrap();
    assert_eq!(pruned["dry_run"], false);
    assert_eq!(pruned["kept_release_count"], 1);
    assert_eq!(pruned["kept_bundle_count"], 2);
    assert!(!releases.join("v9.9.7-test").exists());
    assert!(!releases.join("v9.9.8-test").exists());
    assert!(releases.join("v9.9.9-test").is_dir());
    assert!(!bundles.join("release-comparison-0001").exists());
    assert!(bundles.join("release-comparison-0002").is_dir());
    assert!(bundles.join("release-comparison-0003").is_dir());
    let _ = child.kill();
    let _ = child.wait();
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

fn sha256_hex_for_test(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn canonical_sha256_for_test(value: &serde_json::Value) -> String {
    fn write_value(out: &mut String, value: &serde_json::Value) {
        match value {
            serde_json::Value::Null => out.push_str("null"),
            serde_json::Value::Bool(value) => {
                out.push_str(if *value { "true" } else { "false" });
            }
            serde_json::Value::Number(value) => out.push_str(&value.to_string()),
            serde_json::Value::String(value) => write_string(out, value),
            serde_json::Value::Array(values) => {
                out.push('[');
                for (index, item) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_value(out, item);
                }
                out.push(']');
            }
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (index, key) in keys.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(out, key);
                    out.push(':');
                    write_value(out, &map[*key]);
                }
                out.push('}');
            }
        }
    }
    fn write_string(out: &mut String, value: &str) {
        out.push('"');
        for ch in value.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{08}' => out.push_str("\\b"),
                '\u{0c}' => out.push_str("\\f"),
                ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
                ch => out.push(ch),
            }
        }
        out.push('"');
    }
    let mut canonical = String::new();
    write_value(&mut canonical, value);
    sha256_hex_for_test(canonical.as_bytes())
}

fn sha256_path(path: &Path) -> String {
    let body = fs::read(path).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(body);
    format!("{:x}", hasher.finalize())
}

fn archive_entries(path: &Path) -> Vec<String> {
    let archive = fs::File::open(path).expect("open archive");
    let decoder = GzDecoder::new(archive);
    let mut archive = Archive::new(decoder);
    let mut entries = archive
        .entries()
        .expect("archive entries")
        .map(|entry| {
            entry
                .expect("archive entry")
                .path()
                .expect("entry path")
                .to_string_lossy()
                .trim_start_matches("./")
                .to_string()
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn archive_text_entry(path: &Path, wanted: &str) -> String {
    let archive = fs::File::open(path).expect("open archive");
    let decoder = GzDecoder::new(archive);
    let mut archive = Archive::new(decoder);
    for entry in archive.entries().expect("archive entries") {
        let mut entry = entry.expect("archive entry");
        let path = entry
            .path()
            .expect("entry path")
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        if path == wanted {
            let mut body = String::new();
            entry.read_to_string(&mut body).expect("read archive text");
            return body;
        }
    }
    panic!("missing archive entry {wanted}");
}

fn release_sign_command() -> Command {
    let mut command = Command::new(sh_command());
    command
        .arg("../../scripts/release-sign-provenance.sh")
        .env("AO2_BIN", env!("CARGO_BIN_EXE_ao2"));
    command
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

fn sh_command() -> PathBuf {
    ao2_adapters::posix_shell_command().unwrap_or_else(|| PathBuf::from("sh"))
}

fn normalize_separators(input: &str) -> String {
    input.replace('\\', "/")
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

fn package_test_archive(root: &Path, target_label: &str) -> PathBuf {
    let out_dir = root.join(format!("dist-{target_label}"));
    let package = ao2([
        "release",
        "package",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--version",
        "9.9.9-test",
        "--binary",
        env!("CARGO_BIN_EXE_ao2"),
        "--target-label",
        target_label,
    ]);
    assert!(package.status.success(), "{}", stderr(&package));
    let package_json: serde_json::Value = serde_json::from_str(&stdout(&package)).unwrap();
    PathBuf::from(package_json["archive"].as_str().unwrap())
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

fn value_for<'a>(output: &'a str, prefix: &str) -> &'a str {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .unwrap_or_else(|| panic!("missing prefix {prefix} in output:\n{output}"))
}

fn http_request(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn accept_test_connection(listener: &TcpListener, label: &str) -> TcpStream {
    let mut attempts = 0;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                attempts += 1;
                assert!(attempts <= 300, "timed out waiting for {label}");
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

fn http_body(response: &str) -> &str {
    response.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn percent_encode_for_test(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(char::from(byte));
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn get_queue(port: u16) -> serde_json::Value {
    let response = http_request(
        port,
        "GET /api/queue?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str(http_body(&response)).unwrap()
}

fn start_queue_job(port: u16, run_id: &str, prompt_path: &Path) -> serde_json::Value {
    let body = format!(
        "template=bug-fix&provider=scripted&run_id={run_id}&provider_prompt_file={}&max_repair_attempts=1",
        prompt_path.to_str().unwrap()
    );
    let request = format!(
        "POST /api/queue/start?token=test-token HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let response = http_request(port, &request);
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    serde_json::from_str(http_body(&response)).unwrap()
}

fn wait_for_queue_job_status(port: u16, run_id: &str, expected_status: &str) -> serde_json::Value {
    wait_for_queue_job_status_with_attempts(port, run_id, expected_status, 300)
}

fn wait_for_workbench_support_fixture_job(port: u16, run_id: &str) -> serde_json::Value {
    let attempts = if cfg!(windows) { 900 } else { 300 };
    wait_for_queue_job_status_with_attempts(port, run_id, "accepted", attempts)
}

fn wait_for_queue_job_status_with_attempts(
    port: u16,
    run_id: &str,
    expected_status: &str,
    attempts: usize,
) -> serde_json::Value {
    let mut last_job = None;
    for _ in 0..attempts {
        let queue = get_queue(port);
        let job = queue["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|job| job["run_id"] == run_id)
            .cloned();
        if let Some(job) = job {
            if job["status"] == expected_status {
                return job;
            }
            last_job = Some(job);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!(
        "{}",
        queue_wait_timeout_message(run_id, expected_status, last_job.as_ref())
    );
}

fn queue_wait_timeout_message(
    run_id: &str,
    expected_status: &str,
    last_job: Option<&serde_json::Value>,
) -> String {
    let mut message = format!("job {run_id} did not reach status {expected_status}");
    let Some(job) = last_job else {
        message.push_str("; last_observed_job=none");
        return message;
    };

    let last_status = queue_wait_field(job, "status");
    message.push_str(&format!(
        "; last_status={}",
        if last_status.is_empty() {
            "<missing>"
        } else {
            &last_status
        }
    ));
    for field in ["exit_code", "error", "stdout_log", "stderr_log"] {
        let value = queue_wait_field(job, field);
        if !value.is_empty() {
            message.push_str(&format!("; {field}={value}"));
        }
    }
    message
}

fn queue_wait_field(job: &serde_json::Value, field: &str) -> String {
    match job.get(field) {
        Some(serde_json::Value::String(value)) => value.clone(),
        Some(serde_json::Value::Number(value)) => value.to_string(),
        Some(serde_json::Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
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
