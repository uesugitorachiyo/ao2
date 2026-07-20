use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use sha2::{Digest, Sha256};

const FIXTURE: &str = "tests/fixtures/github-issue-draft/valid-evidence.json";
const SUBJECT_FIXTURE: &str = "tests/fixtures/github-issue-draft/canonical-subject.json";
const EXISTING_FIXTURE: &str = "tests/fixtures/github-issue-draft/existing-exact-draft.json";
const FIXTURE_INSTANCE: &str = "fixture-test-instance";

fn ao2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args(args)
        .output()
        .expect("run ao2")
}

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE)
}

fn named_fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn write_json(value: &serde_json::Value) -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("input.json");
    fs::write(&path, serde_json::to_vec_pretty(value).expect("serialize")).expect("write");
    (temp, path)
}

fn valid_evidence() -> serde_json::Value {
    serde_json::from_slice(&fs::read(fixture_path()).expect("read fixture")).expect("parse fixture")
}

fn preview(evidence: &Path) -> (tempfile::TempDir, PathBuf, serde_json::Value) {
    let temp = tempfile::tempdir().expect("tempdir");
    let action = temp.path().join("action.json");
    let output = ao2(&[
        "issue",
        "draft-pr",
        "preview",
        "--evidence",
        evidence.to_str().expect("utf8"),
        "--out",
        action.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value = serde_json::from_slice(&output.stdout).expect("preview json");
    (temp, action, value)
}

fn rejected(args: &[&str], needle: &str) {
    let output = ao2(args);
    assert!(!output.status.success(), "unexpected success");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(needle),
        "stderr did not contain {needle:?}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    let mut bytes = Vec::new();
    let mut headers_end = None;
    loop {
        let mut buffer = [0_u8; 4096];
        let count = stream.read(&mut buffer).expect("read request");
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if headers_end.is_none() {
            headers_end = bytes.windows(4).position(|part| part == b"\r\n\r\n");
        }
        if let Some(index) = headers_end {
            let headers = String::from_utf8_lossy(&bytes[..index + 4]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(str::trim)
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= index + 4 + length {
                break;
            }
        }
    }
    String::from_utf8(bytes).expect("request utf8")
}

fn respond(stream: &mut TcpStream, status: &str, body: &str, extra_headers: &str) {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
        body.len()
    )
    .expect("respond");
}

#[derive(Clone)]
struct FixtureBinding {
    client_challenge: String,
    request_body_sha256: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn request_header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        candidate.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn request_method_and_path(request: &str) -> (&str, &str) {
    let mut parts = request
        .lines()
        .next()
        .expect("request line")
        .split_whitespace();
    (
        parts.next().expect("request method"),
        parts.next().expect("request path"),
    )
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn capture_fixture_binding(action: &serde_json::Value, request: &str) -> FixtureBinding {
    let client_challenge = request_header(request, "X-AO2-Client-Challenge")
        .expect("client challenge")
        .to_string();
    let request_body_sha256 = request_header(request, "X-AO2-Request-Body-SHA256")
        .expect("request body digest")
        .to_string();
    assert!(is_lower_sha256(&client_challenge));
    assert!(is_lower_sha256(&request_body_sha256));
    assert_eq!(
        request_header(request, "X-AO2-Action-Digest"),
        action["approval"]["action_digest"].as_str()
    );
    assert_eq!(
        request_header(request, "X-AO2-Repository"),
        action["subject"]["repository"]["target"].as_str()
    );
    assert_eq!(
        request_header(request, "X-AO2-Action-Request-Path"),
        action["subject"]["request"]["path"].as_str()
    );
    assert_eq!(request_header(request, "X-AO2-Draft"), Some("true"));
    assert_eq!(
        request_header(request, "X-AO2-Base-Commit"),
        action["subject"]["repository"]["base_commit"].as_str()
    );
    assert_eq!(
        request_header(request, "X-AO2-Head-Commit"),
        action["subject"]["repository"]["head_commit"].as_str()
    );
    assert_eq!(request_header(request, "X-AO2-Fixture-Instance-Id"), None);
    FixtureBinding {
        client_challenge,
        request_body_sha256,
    }
}

fn assert_bound_fixture_request(
    action: &serde_json::Value,
    binding: &FixtureBinding,
    request: &str,
) {
    assert_eq!(
        request_header(request, "X-AO2-Client-Challenge"),
        Some(binding.client_challenge.as_str())
    );
    assert_eq!(
        request_header(request, "X-AO2-Request-Body-SHA256"),
        Some(binding.request_body_sha256.as_str())
    );
    assert_eq!(
        request_header(request, "X-AO2-Action-Digest"),
        action["approval"]["action_digest"].as_str()
    );
    assert_eq!(
        request_header(request, "X-AO2-Repository"),
        action["subject"]["repository"]["target"].as_str()
    );
    assert_eq!(
        request_header(request, "X-AO2-Action-Request-Path"),
        action["subject"]["request"]["path"].as_str()
    );
    assert_eq!(request_header(request, "X-AO2-Draft"), Some("true"));
    assert_eq!(
        request_header(request, "X-AO2-Base-Commit"),
        action["subject"]["repository"]["base_commit"].as_str()
    );
    assert_eq!(
        request_header(request, "X-AO2-Head-Commit"),
        action["subject"]["repository"]["head_commit"].as_str()
    );
    assert_eq!(
        request_header(request, "X-AO2-Fixture-Instance-Id"),
        Some(FIXTURE_INSTANCE)
    );
}

fn exchange_attestation(
    action: &serde_json::Value,
    binding: &FixtureBinding,
    request: &str,
    outcome: &str,
    pull_number: Option<u64>,
) -> serde_json::Value {
    let (method, path) = request_method_and_path(request);
    serde_json::json!({
        "schema_version": "ao2.local-draft-pr-fixture-exchange-attestation.v1",
        "fixture_instance_id": FIXTURE_INSTANCE,
        "client_challenge": binding.client_challenge,
        "action_digest": action["approval"]["action_digest"],
        "request_body_sha256": binding.request_body_sha256,
        "repository": action["subject"]["repository"]["target"],
        "action_request_path": action["subject"]["request"]["path"],
        "draft": action["subject"]["request"]["body"]["draft"],
        "base_commit": action["subject"]["repository"]["base_commit"],
        "head_commit": action["subject"]["repository"]["head_commit"],
        "exchange_method": method,
        "exchange_path": path,
        "outcome": outcome,
        "pull_number": pull_number
    })
}

fn bound_local_attestation(
    action: &serde_json::Value,
    binding: &FixtureBinding,
    request: &str,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.local-draft-pr-fixture-attestation.v1",
        "fixture_instance_id": FIXTURE_INSTANCE,
        "claims_local_only": true,
        "claims_forwarding_capable": false,
        "claims_external_network_enabled": false,
        "fixture_exchange_attestation": exchange_attestation(
            action,
            binding,
            request,
            "fixture_attestation",
            None
        )
    })
}

fn bound_ref_response(
    action: &serde_json::Value,
    binding: &FixtureBinding,
    request: &str,
    reference: &str,
    commit: &str,
    outcome: &str,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.local-draft-pr-fixture-ref.v1",
        "ref": reference,
        "commit": commit,
        "fixture_exchange_attestation": exchange_attestation(
            action,
            binding,
            request,
            outcome,
            None
        )
    })
}

fn bound_pulls_response(
    action: &serde_json::Value,
    binding: &FixtureBinding,
    request: &str,
    pulls: serde_json::Value,
) -> serde_json::Value {
    let pull_number = pulls
        .as_array()
        .filter(|pulls| pulls.len() == 1)
        .and_then(|pulls| pulls[0]["number"].as_u64());
    serde_json::json!({
        "pulls": pulls,
        "fixture_exchange_attestation": exchange_attestation(
            action,
            binding,
            request,
            "pull_discovery",
            pull_number
        )
    })
}

fn write_attestation(
    action: &serde_json::Value,
    binding: &FixtureBinding,
    outcome: &str,
    pull_number: Option<u64>,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.local-draft-pr-fixture-write-attestation.v1",
        "fixture_instance_id": FIXTURE_INSTANCE,
        "client_challenge": binding.client_challenge,
        "action_digest": action["approval"]["action_digest"],
        "request_body_sha256": binding.request_body_sha256,
        "repository": action["subject"]["repository"]["target"],
        "action_request_path": action["subject"]["request"]["path"],
        "draft": action["subject"]["request"]["body"]["draft"],
        "base_commit": action["subject"]["repository"]["base_commit"],
        "head_commit": action["subject"]["repository"]["head_commit"],
        "outcome": outcome,
        "pull_number": pull_number,
        "preconditions_enforced": true,
        "claims_external_endpoint_contacted": false,
        "claims_forwarded": false
    })
}

fn bound_created_response(
    action: &serde_json::Value,
    binding: &FixtureBinding,
    request: &str,
) -> serde_json::Value {
    serde_json::json!({
        "pull": exact_draft_response(action),
        "fixture_exchange_attestation": exchange_attestation(
            action,
            binding,
            request,
            "created",
            Some(9)
        ),
        "fixture_write_attestation": write_attestation(
            action,
            binding,
            "created",
            Some(9)
        )
    })
}

fn bound_conflict_response(
    action: &serde_json::Value,
    binding: &FixtureBinding,
    request: &str,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.local-draft-pr-fixture-conflict.v1",
        "status": "conflict",
        "fixture_exchange_attestation": exchange_attestation(
            action,
            binding,
            request,
            "conflict",
            None
        ),
        "fixture_write_attestation": write_attestation(
            action,
            binding,
            "conflict",
            None
        )
    })
}

fn serve_bound_prewrite_checks(
    listener: &TcpListener,
    action: &serde_json::Value,
) -> FixtureBinding {
    let (mut attest, _) = listener.accept().expect("accept attestation");
    let request = read_request(&mut attest);
    assert!(request.starts_with("GET /ao2/fixture-attestation HTTP/1.1"));
    let binding = capture_fixture_binding(action, &request);
    respond(
        &mut attest,
        "200 OK",
        &bound_local_attestation(action, &binding, &request).to_string(),
        "",
    );
    drop(attest);

    for (path, reference, commit, outcome) in [
        (
            "/repos/uesugitorachiyo/ao-crucible/git/ref/heads/main",
            "refs/heads/main",
            action["subject"]["repository"]["base_commit"]
                .as_str()
                .expect("base commit"),
            "base_ref",
        ),
        (
            "/repos/uesugitorachiyo/ao-crucible/git/ref/heads/ao2%2Fissue-8-bounded-repair",
            "refs/heads/ao2/issue-8-bounded-repair",
            action["subject"]["repository"]["head_commit"]
                .as_str()
                .expect("head commit"),
            "head_ref",
        ),
    ] {
        let (mut get, _) = listener.accept().expect("accept ref read");
        let request = read_request(&mut get);
        assert!(request.starts_with(&format!("GET {path} HTTP/1.1")));
        assert_bound_fixture_request(action, &binding, &request);
        respond(
            &mut get,
            "200 OK",
            &bound_ref_response(action, &binding, &request, reference, commit, outcome).to_string(),
            "",
        );
    }
    binding
}

fn assert_missing_pull_number_rejected(missing_from: &str) {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest")
        .to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let missing_from = missing_from.to_owned();
    let server = std::thread::spawn(move || {
        let (mut attest, _) = listener.accept().expect("accept attestation");
        let request = read_request(&mut attest);
        let binding = capture_fixture_binding(&action, &request);
        let mut response = bound_local_attestation(&action, &binding, &request);
        if missing_from == "attestation" {
            response["fixture_exchange_attestation"]
                .as_object_mut()
                .expect("exchange attestation")
                .remove("pull_number");
            respond(&mut attest, "200 OK", &response.to_string(), "");
            return;
        }
        respond(&mut attest, "200 OK", &response.to_string(), "");
        drop(attest);

        for (index, (path, reference, commit, outcome)) in [
            (
                "/repos/uesugitorachiyo/ao-crucible/git/ref/heads/main",
                "refs/heads/main",
                action["subject"]["repository"]["base_commit"]
                    .as_str()
                    .expect("base commit"),
                "base_ref",
            ),
            (
                "/repos/uesugitorachiyo/ao-crucible/git/ref/heads/ao2%2Fissue-8-bounded-repair",
                "refs/heads/ao2/issue-8-bounded-repair",
                action["subject"]["repository"]["head_commit"]
                    .as_str()
                    .expect("head commit"),
                "head_ref",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let (mut get, _) = listener.accept().expect("accept ref read");
            let request = read_request(&mut get);
            assert!(request.starts_with(&format!("GET {path} HTTP/1.1")));
            let mut response =
                bound_ref_response(&action, &binding, &request, reference, commit, outcome);
            if missing_from == "ref" && index == 0 {
                response["fixture_exchange_attestation"]
                    .as_object_mut()
                    .expect("exchange attestation")
                    .remove("pull_number");
                respond(&mut get, "200 OK", &response.to_string(), "");
                return;
            }
            respond(&mut get, "200 OK", &response.to_string(), "");
        }

        let (mut discover, _) = listener.accept().expect("accept discovery");
        let request = read_request(&mut discover);
        let mut response = bound_pulls_response(&action, &binding, &request, serde_json::json!([]));
        if missing_from == "empty_discovery" {
            response["fixture_exchange_attestation"]
                .as_object_mut()
                .expect("exchange attestation")
                .remove("pull_number");
            respond(&mut discover, "200 OK", &response.to_string(), "");
            return;
        }
        respond(&mut discover, "200 OK", &response.to_string(), "");
        drop(discover);

        let (mut post, _) = listener.accept().expect("accept POST");
        let request = read_request(&mut post);
        let mut response = bound_conflict_response(&action, &binding, &request);
        let attestation = match missing_from.as_str() {
            "conflict_exchange" => "fixture_exchange_attestation",
            "conflict_write" => "fixture_write_attestation",
            other => panic!("unsupported missing pull_number case: {other}"),
        };
        response[attestation]
            .as_object_mut()
            .expect("attestation")
            .remove("pull_number");
        respond(&mut post, "409 Conflict", &response.to_string(), "");
    });

    rejected(
        &[
            "issue",
            "draft-pr",
            "fixture-publish",
            "--action",
            action_path.to_str().expect("utf8"),
            "--expected-action-digest",
            &digest,
            "--fixture-api",
            &endpoint,
            "--json",
        ],
        "missing field `pull_number`",
    );
    server.join().expect("server");
}

fn exact_draft_response(action: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "number": 9,
        "state": "open",
        "draft": true,
        "title": action["subject"]["request"]["body"]["title"],
        "body": action["subject"]["request"]["body"]["body"],
        "base": {
            "ref": action["subject"]["repository"]["base_branch"],
            "sha": action["subject"]["repository"]["base_commit"]
        },
        "head": {
            "ref": action["subject"]["repository"]["head_branch"],
            "sha": action["subject"]["repository"]["head_commit"],
            "repo": {
                "full_name": action["subject"]["repository"]["head_repository"]
            }
        }
    })
}

#[test]
fn preview_emits_digest_bound_strict_draft_action_and_verify_reproduces_digest() {
    let (_temp, action_path, action) = preview(&fixture_path());
    assert_eq!(action["schema_version"], "ao2.github-draft-pr-action.v1");
    assert_eq!(action["approval"]["algorithm"], "sha256-ao2-canonical-v1");
    let digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest");
    assert_eq!(digest.len(), 64);
    assert_eq!(
        digest,
        "31a559df1b0491c7bc292dd350706f5885fda4325ef1d1816e1e1feae3f14a44"
    );
    let golden_subject: serde_json::Value =
        serde_json::from_slice(&fs::read(named_fixture(SUBJECT_FIXTURE)).unwrap()).unwrap();
    assert_eq!(action["subject"], golden_subject);
    assert_eq!(action["subject"]["request"]["method"], "POST");
    assert_eq!(
        action["subject"]["request"]["path"],
        "/repos/uesugitorachiyo/ao-crucible/pulls"
    );
    assert_eq!(action["subject"]["request"]["body"]["draft"], true);
    assert_eq!(
        action["subject"]["request"]["body"]["preconditions"]["base_commit"],
        "2222222222222222222222222222222222222222"
    );
    assert_eq!(
        action["subject"]["request"]["body"]["preconditions"]["head_commit"],
        "3333333333333333333333333333333333333333"
    );
    assert_eq!(
        action["subject"]["request"]["body"]["body"],
        "Repairs the verified issue #8 reproduction.\n\nEvidence is digest-bound and this pull request remains draft-only.\n\nAO2-Evidence: issue_url=https://github.com/uesugitorachiyo/ao-crucible/issues/8 snapshot_sha256=1111111111111111111111111111111111111111111111111111111111111111"
    );
    assert_eq!(action["subject"]["safety"]["issue_write"], false);
    assert_eq!(action["subject"]["safety"]["merge"], false);

    let output = ao2(&[
        "issue",
        "draft-pr",
        "verify",
        "--action",
        action_path.to_str().expect("utf8"),
        "--expected-action-digest",
        digest,
        "--json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("verify readback");
    assert_eq!(readback["status"], "passed");
    assert_eq!(readback["action_digest"], digest);
    assert_eq!(readback["action_verified"], true);
    assert_eq!(readback["fixture_write_observed"], false);
    assert_eq!(readback["client_contact_scope"], "none");
    assert_eq!(
        readback["fixture_exchange_attestation_status"],
        "not_checked"
    );
    assert_eq!(readback["fixture_write_attestation_status"], "not_checked");
    assert_eq!(readback["fixture_claims_authenticated"], false);
    assert_eq!(
        readback["external_write_observability"],
        "not_observable_from_client"
    );
    assert_eq!(
        readback["behavior_outside_client_observable_boundary"],
        "not_claimed"
    );
    assert!(readback.get("safe_to_publish_fixture").is_none());
    assert!(readback.get("fixture_write_performed").is_none());
    assert!(readback.get("github_write_performed").is_none());
    assert!(readback.get("public_github_write_performed").is_none());
}

#[test]
fn preview_rejects_identity_repair_safety_schema_and_size_failures() {
    let cases = [
        (
            "/issue/classification",
            serde_json::json!("feature_request"),
            "authentic_bug",
        ),
        (
            "/issue/canonical_url",
            serde_json::json!("https://github.com/other/repo/issues/8"),
            "canonical_url",
        ),
        (
            "/repository/target",
            serde_json::json!("other/repo"),
            "canonical_url",
        ),
        (
            "/repository/head_repository",
            serde_json::json!("other/repo"),
            "head_repository",
        ),
        (
            "/repository/base_commit",
            serde_json::json!("ABC"),
            "base_commit",
        ),
        (
            "/repository/head_commit",
            serde_json::json!("ABC"),
            "head_commit",
        ),
        ("/repair/status", serde_json::json!("failed"), "verified"),
        (
            "/repair/provenance/worker_source_commit",
            serde_json::json!(""),
            "worker_source_commit",
        ),
        (
            "/safety/prompt_injection_detected",
            serde_json::json!(true),
            "prompt_injection_detected",
        ),
        (
            "/safety/security_sensitive",
            serde_json::json!(true),
            "security_sensitive",
        ),
        (
            "/safety/policy_blocked",
            serde_json::json!(true),
            "policy_blocked",
        ),
        (
            "/safety/issue_write",
            serde_json::json!(true),
            "issue_write",
        ),
        (
            "/safety/ready_for_review",
            serde_json::json!(true),
            "ready_for_review",
        ),
        (
            "/safety/review_approval",
            serde_json::json!(true),
            "review_approval",
        ),
        ("/safety/merge", serde_json::json!(true), "merge"),
        ("/safety/release", serde_json::json!(true), "release"),
    ];
    for (pointer, replacement, needle) in cases {
        let mut value = valid_evidence();
        *value.pointer_mut(pointer).expect("pointer") = replacement;
        let (_temp, path) = write_json(&value);
        rejected(
            &[
                "issue",
                "draft-pr",
                "preview",
                "--evidence",
                path.to_str().expect("utf8"),
                "--out",
                path.with_extension("action").to_str().expect("utf8"),
                "--json",
            ],
            needle,
        );
    }

    let mut unknown = valid_evidence();
    unknown["unexpected"] = serde_json::json!(true);
    let (_temp, path) = write_json(&unknown);
    rejected(
        &[
            "issue",
            "draft-pr",
            "preview",
            "--evidence",
            path.to_str().expect("utf8"),
            "--out",
            path.with_extension("action").to_str().expect("utf8"),
            "--json",
        ],
        "unknown field",
    );

    let temp = tempfile::tempdir().expect("tempdir");
    let malformed = temp.path().join("malformed.json");
    fs::write(&malformed, b"{").expect("write malformed");
    rejected(
        &[
            "issue",
            "draft-pr",
            "preview",
            "--evidence",
            malformed.to_str().expect("utf8"),
            "--out",
            temp.path()
                .join("malformed-action.json")
                .to_str()
                .expect("utf8"),
            "--json",
        ],
        "parse strict JSON",
    );

    let oversized = temp.path().join("oversized.json");
    fs::write(&oversized, vec![b' '; 65_537]).expect("write oversized");
    rejected(
        &[
            "issue",
            "draft-pr",
            "preview",
            "--evidence",
            oversized.to_str().expect("utf8"),
            "--out",
            temp.path().join("action.json").to_str().expect("utf8"),
            "--json",
        ],
        "65536",
    );
}

#[test]
fn preview_rejects_generated_action_over_input_limit_without_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let evidence_path = temp.path().join("large-valid-evidence.json");
    let output_path = temp.path().join("action.json");
    let mut evidence = valid_evidence();
    evidence["repair"]["changed_files"] = serde_json::json!((0..62)
        .map(|index| format!("src/{index:03}-{}", "x".repeat(1_000)))
        .collect::<Vec<_>>());
    let evidence_bytes = serde_json::to_vec(&evidence).expect("serialize compact evidence");
    assert!(
        evidence_bytes.len() <= 65_536,
        "test evidence must remain valid input"
    );
    fs::write(&evidence_path, evidence_bytes).expect("write evidence");

    rejected(
        &[
            "issue",
            "draft-pr",
            "preview",
            "--evidence",
            evidence_path.to_str().expect("utf8"),
            "--out",
            output_path.to_str().expect("utf8"),
            "--json",
        ],
        "action exceeds the 65536-byte limit",
    );
    assert!(
        !output_path.exists(),
        "oversized action rejection must not create output"
    );
}

#[cfg(unix)]
#[test]
fn preview_refuses_existing_symlink_output_without_changing_target() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("sentinel.json");
    let output = temp.path().join("action.json");
    fs::write(&target, b"sentinel").expect("write sentinel");
    symlink(&target, &output).expect("create output symlink");

    rejected(
        &[
            "issue",
            "draft-pr",
            "preview",
            "--evidence",
            fixture_path().to_str().expect("utf8"),
            "--out",
            output.to_str().expect("utf8"),
            "--json",
        ],
        "create new draft PR action",
    );
    assert_eq!(fs::read(&target).expect("read sentinel"), b"sentinel");
}

#[cfg(unix)]
#[test]
fn preview_rejects_symlink_input() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let evidence = temp.path().join("evidence.json");
    symlink(fixture_path(), &evidence).expect("create input symlink");

    rejected(
        &[
            "issue",
            "draft-pr",
            "preview",
            "--evidence",
            evidence.to_str().expect("utf8"),
            "--out",
            temp.path().join("action.json").to_str().expect("utf8"),
            "--json",
        ],
        "regular file",
    );
}

#[cfg(unix)]
#[test]
fn preview_path_replacement_never_follows_symlink_or_blocks_on_fifo() {
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let temp = tempfile::tempdir().expect("tempdir");
    let stable_evidence = temp.path().join("stable-evidence.json");
    fs::copy(fixture_path(), &stable_evidence).expect("copy stable evidence");
    let mut raced_evidence = valid_evidence();
    raced_evidence["draft"]["title"] = serde_json::json!("raced symlink content");
    let raced_target = temp.path().join("raced-target.json");
    fs::write(
        &raced_target,
        serde_json::to_vec(&raced_evidence).expect("serialize raced evidence"),
    )
    .expect("write raced evidence");

    let input = temp.path().join("evidence.json");
    let regular_stage = temp.path().join("regular-stage");
    let symlink_stage = temp.path().join("symlink-stage");
    let fifo_source = temp.path().join("fifo-source");
    let fifo_stage = temp.path().join("fifo-stage");
    fs::copy(&stable_evidence, &input).expect("copy initial regular input");
    fs::hard_link(&stable_evidence, &regular_stage).expect("link regular stage");
    symlink(&raced_target, &symlink_stage).expect("create symlink stage");
    assert!(Command::new("mkfifo")
        .arg(&fifo_source)
        .status()
        .expect("create source FIFO")
        .success());
    fs::hard_link(&fifo_source, &fifo_stage).expect("link FIFO stage");

    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_racer = Arc::clone(&stop);
    let input_for_racer = input.clone();
    let stable_for_racer = stable_evidence.clone();
    let raced_for_racer = raced_target.clone();
    let fifo_for_racer = fifo_source.clone();
    let racer = std::thread::spawn(move || {
        while !stop_for_racer.load(Ordering::Relaxed) {
            fs::rename(&regular_stage, &input_for_racer).expect("install regular input");
            fs::hard_link(&stable_for_racer, &regular_stage).expect("restore regular stage");
            fs::rename(&symlink_stage, &input_for_racer).expect("install symlink input");
            symlink(&raced_for_racer, &symlink_stage).expect("restore symlink stage");
            fs::rename(&fifo_stage, &input_for_racer).expect("install FIFO input");
            fs::hard_link(&fifo_for_racer, &fifo_stage).expect("restore FIFO stage");
        }
    });

    let mut violation = None;
    for attempt in 0..100 {
        let output_path = temp.path().join(format!("action-{attempt}.json"));
        let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
            .args([
                "issue",
                "draft-pr",
                "preview",
                "--evidence",
                input.to_str().expect("utf8"),
                "--out",
                output_path.to_str().expect("utf8"),
                "--json",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn ao2");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if child.try_wait().expect("poll ao2").is_some() {
                let output = child.wait_with_output().expect("collect ao2 output");
                if output.status.success() {
                    let action: serde_json::Value =
                        serde_json::from_slice(&output.stdout).expect("parse preview");
                    if action["subject"]["request"]["body"]["title"]
                        == serde_json::json!("raced symlink content")
                    {
                        violation = Some("preview followed a raced input symlink");
                    }
                }
                break;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().expect("kill blocked ao2");
                let _ = child.wait();
                violation = Some("preview blocked on a raced FIFO");
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if violation.is_some() {
            break;
        }
    }
    stop.store(true, Ordering::Relaxed);
    racer.join().expect("input racer");
    assert!(violation.is_none(), "{}", violation.unwrap_or_default());
}

#[cfg(unix)]
#[test]
fn preview_rejects_fifo_without_opening_or_blocking() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fifo = temp.path().join("evidence.fifo");
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("run mkfifo");
    assert!(status.success());
    let mut child = Command::new(env!("CARGO_BIN_EXE_ao2"))
        .args([
            "issue",
            "draft-pr",
            "preview",
            "--evidence",
            fifo.to_str().expect("utf8"),
            "--out",
            temp.path().join("action.json").to_str().expect("utf8"),
            "--json",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ao2");
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll ao2") {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().expect("kill blocked ao2");
            panic!("ao2 blocked while opening a non-regular input");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success());
    let output = child.wait_with_output().expect("collect ao2 output");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("regular file"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn verify_rejects_altered_digest_and_unknown_action_fields() {
    let (_temp, action_path, action) = preview(&fixture_path());
    rejected(
        &[
            "issue",
            "draft-pr",
            "verify",
            "--action",
            action_path.to_str().expect("utf8"),
            "--expected-action-digest",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--json",
        ],
        "digest",
    );

    let mut altered = action;
    altered["subject"]["request"]["body"]["draft"] = serde_json::json!(false);
    altered["unknown"] = serde_json::json!(true);
    let (_changed_temp, changed) = write_json(&altered);
    rejected(
        &[
            "issue",
            "draft-pr",
            "verify",
            "--action",
            changed.to_str().expect("utf8"),
            "--expected-action-digest",
            altered["approval"]["action_digest"]
                .as_str()
                .expect("digest"),
            "--json",
        ],
        "unknown field",
    );

    let mut wrong_binding =
        serde_json::from_slice::<serde_json::Value>(&fs::read(&action_path).expect("read action"))
            .expect("parse action");
    wrong_binding["subject"]["request"]["body"]["body"] = serde_json::json!(
        "Repairs the verified issue #8 reproduction.\n\nEvidence is digest-bound and this pull request remains draft-only.\n\nAO2-Evidence: issue_url=https://github.com/uesugitorachiyo/ao-crucible/issues/8 snapshot_sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    let (_binding_temp, binding_path) = write_json(&wrong_binding);
    rejected(
        &[
            "issue",
            "draft-pr",
            "verify",
            "--action",
            binding_path.to_str().expect("utf8"),
            "--expected-action-digest",
            wrong_binding["approval"]["action_digest"]
                .as_str()
                .expect("digest"),
            "--json",
        ],
        "evidence footer",
    );

    let mut wrong_precondition =
        serde_json::from_slice::<serde_json::Value>(&fs::read(&action_path).expect("read action"))
            .expect("parse action");
    wrong_precondition["subject"]["request"]["body"]["preconditions"]["base_commit"] =
        serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let (_precondition_temp, precondition_path) = write_json(&wrong_precondition);
    rejected(
        &[
            "issue",
            "draft-pr",
            "verify",
            "--action",
            precondition_path.to_str().expect("utf8"),
            "--expected-action-digest",
            wrong_precondition["approval"]["action_digest"]
                .as_str()
                .expect("digest"),
            "--json",
        ],
        "commit preconditions",
    );
}

#[test]
fn verify_rejects_changed_files_and_diff_digest_after_approval() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let expected_digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest")
        .to_owned();

    for (pointer, replacement) in [
        (
            "/subject/repair/changed_files",
            serde_json::json!(["src/lib.rs"]),
        ),
        (
            "/subject/repair/diff_sha256",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
    ] {
        let mut altered = action.clone();
        *altered.pointer_mut(pointer).expect("repair binding") = replacement;
        let (_changed_temp, changed) = write_json(&altered);
        rejected(
            &[
                "issue",
                "draft-pr",
                "verify",
                "--action",
                changed.to_str().expect("utf8"),
                "--expected-action-digest",
                &expected_digest,
                "--json",
            ],
            "digest",
        );
    }

    assert!(action_path.is_file());
}

#[test]
fn fixture_publish_accepts_strict_challenge_bound_exchange_and_write_attestations() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest")
        .to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let server = std::thread::spawn(move || {
        let binding = serve_bound_prewrite_checks(&listener, &action);

        let (mut discover, _) = listener.accept().expect("accept discovery");
        let request = read_request(&mut discover);
        assert_bound_fixture_request(&action, &binding, &request);
        respond(
            &mut discover,
            "200 OK",
            &bound_pulls_response(&action, &binding, &request, serde_json::json!([])).to_string(),
            "",
        );
        drop(discover);

        let (mut post, _) = listener.accept().expect("accept POST");
        let request = read_request(&mut post);
        assert_bound_fixture_request(&action, &binding, &request);
        let body = request.split_once("\r\n\r\n").expect("POST body").1;
        assert_eq!(sha256_hex(body.as_bytes()), binding.request_body_sha256);
        respond(
            &mut post,
            "201 Created",
            &bound_created_response(&action, &binding, &request).to_string(),
            "",
        );
    });

    let output = ao2(&[
        "issue",
        "draft-pr",
        "fixture-publish",
        "--action",
        action_path.to_str().expect("utf8"),
        "--expected-action-digest",
        &digest,
        "--fixture-api",
        &endpoint,
        "--json",
    ]);
    server.join().expect("server");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("publish readback");
    assert_eq!(readback["status"], "fixture_reported_created");
    assert_eq!(readback["post_performed"], true);
    assert_eq!(readback["fixture_write_observed"], false);
    assert_eq!(readback["client_contact_scope"], "numeric_loopback_only");
    assert_eq!(
        readback["fixture_exchange_attestation_status"],
        "strict_challenge_bound_self_attestation"
    );
    assert_eq!(
        readback["fixture_write_attestation_status"],
        "strict_challenge_bound_self_attestation"
    );
    assert_eq!(readback["fixture_claims_authenticated"], false);
    assert_eq!(
        readback["external_write_observability"],
        "not_observable_from_client"
    );
    assert_eq!(
        readback["behavior_outside_client_observable_boundary"],
        "not_claimed"
    );
    assert!(readback.get("fixture_write_performed").is_none());
    assert!(readback.get("external_github_endpoint_contacted").is_none());
}

#[test]
fn fixture_publish_uses_a_fresh_client_challenge_per_invocation() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest")
        .to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let (sender, receiver) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut request, _) = listener.accept().expect("accept attestation");
            let request_text = read_request(&mut request);
            sender
                .send(request_header(&request_text, "X-AO2-Client-Challenge").map(str::to_owned))
                .expect("send challenge");
            respond(&mut request, "500 Internal Server Error", "{}", "");
        }
    });

    for _ in 0..2 {
        rejected(
            &[
                "issue",
                "draft-pr",
                "fixture-publish",
                "--action",
                action_path.to_str().expect("utf8"),
                "--expected-action-digest",
                &digest,
                "--fixture-api",
                &endpoint,
                "--json",
            ],
            "attestation",
        );
    }
    server.join().expect("server");
    let first = receiver.recv().expect("first challenge");
    let second = receiver.recv().expect("second challenge");
    let first = first.expect("first invocation challenge");
    let second = second.expect("second invocation challenge");
    assert!(is_lower_sha256(&first));
    assert!(is_lower_sha256(&second));
    assert_ne!(first, second);
}

#[test]
fn fixture_publish_enforces_a_total_http_exchange_deadline() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest")
        .to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let server = std::thread::spawn(move || {
        let (mut request, _) = listener.accept().expect("accept attestation");
        let _ = read_request(&mut request);
        write!(
            request,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4\r\nConnection: close\r\n\r\n"
        )
        .expect("write response headers");
        request.flush().expect("flush response headers");
        for byte in b"null" {
            std::thread::sleep(Duration::from_millis(1_100));
            if request.write_all(&[*byte]).is_err() || request.flush().is_err() {
                break;
            }
        }
    });

    let started = std::time::Instant::now();
    rejected(
        &[
            "issue",
            "draft-pr",
            "fixture-publish",
            "--action",
            action_path.to_str().expect("utf8"),
            "--expected-action-digest",
            &digest,
            "--fixture-api",
            &endpoint,
            "--json",
        ],
        "total deadline",
    );
    let elapsed = started.elapsed();
    server.join().expect("server");
    assert!(
        elapsed < Duration::from_secs(4),
        "slow drip exceeded the total deadline after {elapsed:?}"
    );
}

#[test]
fn fixture_publish_rejects_every_exchange_attestation_binding_mismatch() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest")
        .to_owned();
    let cases = [
        (
            "/client_challenge",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        (
            "/action_digest",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        (
            "/request_body_sha256",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        ("/repository", serde_json::json!("other/repository")),
        (
            "/action_request_path",
            serde_json::json!("/repos/other/repository/pulls"),
        ),
        ("/draft", serde_json::json!(false)),
        (
            "/base_commit",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        (
            "/head_commit",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        ("/exchange_method", serde_json::json!("POST")),
        ("/exchange_path", serde_json::json!("/other")),
        ("/outcome", serde_json::json!("other")),
        ("/pull_number", serde_json::json!(9)),
        (
            "/fixture_instance_id",
            serde_json::json!("other-fixture-instance"),
        ),
    ];

    for (pointer, replacement) in cases {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let action_for_server = action.clone();
        let server = std::thread::spawn(move || {
            let (mut attest, _) = listener.accept().expect("accept attestation");
            let request = read_request(&mut attest);
            let binding = capture_fixture_binding(&action_for_server, &request);
            let mut response = bound_local_attestation(&action_for_server, &binding, &request);
            *response["fixture_exchange_attestation"]
                .pointer_mut(pointer)
                .expect("attestation pointer") = replacement;
            respond(&mut attest, "200 OK", &response.to_string(), "");
        });

        rejected(
            &[
                "issue",
                "draft-pr",
                "fixture-publish",
                "--action",
                action_path.to_str().expect("utf8"),
                "--expected-action-digest",
                &digest,
                "--fixture-api",
                &endpoint,
                "--json",
            ],
            "fixture exchange self-attestation binding mismatch",
        );
        server.join().expect("server");
    }
}

#[test]
fn fixture_publish_rejects_every_write_attestation_binding_mismatch() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest")
        .to_owned();
    let cases = [
        (
            "/client_challenge",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        (
            "/action_digest",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        (
            "/request_body_sha256",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        ("/repository", serde_json::json!("other/repository")),
        (
            "/action_request_path",
            serde_json::json!("/repos/other/repository/pulls"),
        ),
        ("/draft", serde_json::json!(false)),
        ("/outcome", serde_json::json!("conflict")),
        ("/pull_number", serde_json::json!(10)),
        (
            "/fixture_instance_id",
            serde_json::json!("other-fixture-instance"),
        ),
        (
            "/base_commit",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        (
            "/head_commit",
            serde_json::json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        ("/preconditions_enforced", serde_json::json!(false)),
        (
            "/claims_external_endpoint_contacted",
            serde_json::json!(true),
        ),
        ("/claims_forwarded", serde_json::json!(true)),
    ];

    for (pointer, replacement) in cases {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let action_for_server = action.clone();
        let server = std::thread::spawn(move || {
            let binding = serve_bound_prewrite_checks(&listener, &action_for_server);
            let (mut discover, _) = listener.accept().expect("accept discovery");
            let request = read_request(&mut discover);
            assert_bound_fixture_request(&action_for_server, &binding, &request);
            respond(
                &mut discover,
                "200 OK",
                &bound_pulls_response(
                    &action_for_server,
                    &binding,
                    &request,
                    serde_json::json!([]),
                )
                .to_string(),
                "",
            );
            drop(discover);

            let (mut post, _) = listener.accept().expect("accept POST");
            let request = read_request(&mut post);
            assert_bound_fixture_request(&action_for_server, &binding, &request);
            let mut response = bound_created_response(&action_for_server, &binding, &request);
            *response["fixture_write_attestation"]
                .pointer_mut(pointer)
                .expect("write attestation pointer") = replacement;
            respond(&mut post, "201 Created", &response.to_string(), "");
        });

        rejected(
            &[
                "issue",
                "draft-pr",
                "fixture-publish",
                "--action",
                action_path.to_str().expect("utf8"),
                "--expected-action-digest",
                &digest,
                "--fixture-api",
                &endpoint,
                "--json",
            ],
            "fixture write self-attestation binding mismatch",
        );
        server.join().expect("server");
    }
}

#[test]
fn fixture_publish_rejects_missing_attestation_pull_number() {
    assert_missing_pull_number_rejected("attestation");
}

#[test]
fn fixture_publish_rejects_missing_ref_pull_number() {
    assert_missing_pull_number_rejected("ref");
}

#[test]
fn fixture_publish_rejects_missing_empty_discovery_pull_number() {
    assert_missing_pull_number_rejected("empty_discovery");
}

#[test]
fn fixture_publish_rejects_missing_conflict_exchange_pull_number() {
    assert_missing_pull_number_rejected("conflict_exchange");
}

#[test]
fn fixture_publish_rejects_missing_conflict_write_pull_number() {
    assert_missing_pull_number_rejected("conflict_write");
}

#[test]
fn fixture_publish_rejects_created_exchange_pull_number_drift() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest")
        .to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let server = std::thread::spawn(move || {
        let binding = serve_bound_prewrite_checks(&listener, &action);
        let (mut discover, _) = listener.accept().expect("accept discovery");
        let request = read_request(&mut discover);
        respond(
            &mut discover,
            "200 OK",
            &bound_pulls_response(&action, &binding, &request, serde_json::json!([])).to_string(),
            "",
        );
        drop(discover);
        let (mut post, _) = listener.accept().expect("accept POST");
        let request = read_request(&mut post);
        let mut response = bound_created_response(&action, &binding, &request);
        response["fixture_exchange_attestation"]["pull_number"] = serde_json::json!(10);
        respond(&mut post, "201 Created", &response.to_string(), "");
    });

    rejected(
        &[
            "issue",
            "draft-pr",
            "fixture-publish",
            "--action",
            action_path.to_str().expect("utf8"),
            "--expected-action-digest",
            &digest,
            "--fixture-api",
            &endpoint,
            "--json",
        ],
        "fixture exchange self-attestation binding mismatch",
    );
    server.join().expect("server");
}

#[test]
fn fixture_publish_rejects_zero_pull_numbers_on_every_result_path() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest")
        .to_owned();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let action_for_created = action.clone();
    let server = std::thread::spawn(move || {
        let binding = serve_bound_prewrite_checks(&listener, &action_for_created);
        let (mut discover, _) = listener.accept().expect("accept discovery");
        let request = read_request(&mut discover);
        respond(
            &mut discover,
            "200 OK",
            &bound_pulls_response(
                &action_for_created,
                &binding,
                &request,
                serde_json::json!([]),
            )
            .to_string(),
            "",
        );
        drop(discover);
        let (mut post, _) = listener.accept().expect("accept POST");
        let request = read_request(&mut post);
        let mut response = bound_created_response(&action_for_created, &binding, &request);
        response["pull"]["number"] = serde_json::json!(0);
        response["fixture_exchange_attestation"]["pull_number"] = serde_json::json!(0);
        response["fixture_write_attestation"]["pull_number"] = serde_json::json!(0);
        respond(&mut post, "201 Created", &response.to_string(), "");
    });
    rejected(
        &[
            "issue",
            "draft-pr",
            "fixture-publish",
            "--action",
            action_path.to_str().expect("utf8"),
            "--expected-action-digest",
            &digest,
            "--fixture-api",
            &endpoint,
            "--json",
        ],
        "pull number must be positive",
    );
    server.join().expect("created server");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let action_for_existing = action.clone();
    let server = std::thread::spawn(move || {
        let binding = serve_bound_prewrite_checks(&listener, &action_for_existing);
        let (mut discover, _) = listener.accept().expect("accept discovery");
        let request = read_request(&mut discover);
        let mut pull = exact_draft_response(&action_for_existing);
        pull["number"] = serde_json::json!(0);
        respond(
            &mut discover,
            "200 OK",
            &bound_pulls_response(
                &action_for_existing,
                &binding,
                &request,
                serde_json::json!([pull]),
            )
            .to_string(),
            "",
        );
    });
    rejected(
        &[
            "issue",
            "draft-pr",
            "fixture-publish",
            "--action",
            action_path.to_str().expect("utf8"),
            "--expected-action-digest",
            &digest,
            "--fixture-api",
            &endpoint,
            "--json",
        ],
        "pull number must be positive",
    );
    server.join().expect("existing server");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let action_for_conflict = action.clone();
    let server = std::thread::spawn(move || {
        let binding = serve_bound_prewrite_checks(&listener, &action_for_conflict);
        let (mut discover, _) = listener.accept().expect("accept discovery");
        let request = read_request(&mut discover);
        respond(
            &mut discover,
            "200 OK",
            &bound_pulls_response(
                &action_for_conflict,
                &binding,
                &request,
                serde_json::json!([]),
            )
            .to_string(),
            "",
        );
        drop(discover);
        let (mut post, _) = listener.accept().expect("accept POST");
        let request = read_request(&mut post);
        respond(
            &mut post,
            "409 Conflict",
            &bound_conflict_response(&action_for_conflict, &binding, &request).to_string(),
            "",
        );
        drop(post);
        let (mut reread, _) = listener.accept().expect("accept reread");
        let request = read_request(&mut reread);
        let mut pull = exact_draft_response(&action_for_conflict);
        pull["number"] = serde_json::json!(0);
        respond(
            &mut reread,
            "200 OK",
            &bound_pulls_response(
                &action_for_conflict,
                &binding,
                &request,
                serde_json::json!([pull]),
            )
            .to_string(),
            "",
        );
    });
    rejected(
        &[
            "issue",
            "draft-pr",
            "fixture-publish",
            "--action",
            action_path.to_str().expect("utf8"),
            "--expected-action-digest",
            &digest,
            "--fixture-api",
            &endpoint,
            "--json",
        ],
        "pull number must be positive",
    );
    server.join().expect("conflict server");
}

#[test]
fn conflict_recovery_reports_post_without_claiming_fixture_write_observation() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .expect("digest")
        .to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let server = std::thread::spawn(move || {
        let binding = serve_bound_prewrite_checks(&listener, &action);
        let (mut discover, _) = listener.accept().expect("accept discovery");
        let request = read_request(&mut discover);
        respond(
            &mut discover,
            "200 OK",
            &bound_pulls_response(&action, &binding, &request, serde_json::json!([])).to_string(),
            "",
        );
        drop(discover);
        let (mut post, _) = listener.accept().expect("accept POST");
        let request = read_request(&mut post);
        respond(
            &mut post,
            "422 Unprocessable Entity",
            &bound_conflict_response(&action, &binding, &request).to_string(),
            "",
        );
        drop(post);
        let (mut reread, _) = listener.accept().expect("accept reread");
        let request = read_request(&mut reread);
        respond(
            &mut reread,
            "200 OK",
            &bound_pulls_response(
                &action,
                &binding,
                &request,
                serde_json::json!([exact_draft_response(&action)]),
            )
            .to_string(),
            "",
        );
    });

    let output = ao2(&[
        "issue",
        "draft-pr",
        "fixture-publish",
        "--action",
        action_path.to_str().expect("utf8"),
        "--expected-action-digest",
        &digest,
        "--fixture-api",
        &endpoint,
        "--json",
    ]);
    server.join().expect("server");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("conflict readback");
    assert_eq!(
        readback["status"],
        "idempotent_readback_after_create_conflict"
    );
    assert_eq!(readback["post_performed"], true);
    assert_eq!(readback["fixture_write_observed"], false);
    assert_eq!(
        readback["fixture_write_attestation_status"],
        "strict_challenge_bound_self_attestation"
    );
    assert_eq!(
        readback["external_write_observability"],
        "not_observable_from_client"
    );
    assert_eq!(
        readback["behavior_outside_client_observable_boundary"],
        "not_claimed"
    );
    assert!(readback.get("fixture_write_performed").is_none());
}

#[test]
fn fixture_publish_creates_exactly_one_digest_bound_draft() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let server = std::thread::spawn(move || {
        let binding = serve_bound_prewrite_checks(&listener, &action);
        let (mut get, _) = listener.accept().expect("accept GET");
        let get_request = read_request(&mut get);
        assert!(get_request.starts_with(
            "GET /repos/uesugitorachiyo/ao-crucible/pulls?state=all&head=uesugitorachiyo%3Aao2%2Fissue-8-bounded-repair&base=main HTTP/1.1"
        ));
        assert!(!get_request.to_ascii_lowercase().contains("authorization:"));
        assert_bound_fixture_request(&action, &binding, &get_request);
        respond(
            &mut get,
            "200 OK",
            &bound_pulls_response(&action, &binding, &get_request, serde_json::json!([]))
                .to_string(),
            "",
        );
        drop(get);

        let (mut post, _) = listener.accept().expect("accept POST");
        let post_request = read_request(&mut post);
        assert!(post_request.starts_with("POST /repos/uesugitorachiyo/ao-crucible/pulls HTTP/1.1"));
        assert_bound_fixture_request(&action, &binding, &post_request);
        let body = post_request.split_once("\r\n\r\n").unwrap().1;
        assert_eq!(sha256_hex(body.as_bytes()), binding.request_body_sha256);
        let posted: serde_json::Value = serde_json::from_str(body).expect("posted json");
        assert_eq!(posted["draft"], true);
        assert_eq!(posted["head"], "uesugitorachiyo:ao2/issue-8-bounded-repair");
        assert_eq!(
            posted["preconditions"]["base_commit"],
            "2222222222222222222222222222222222222222"
        );
        assert_eq!(
            posted["preconditions"]["head_commit"],
            "3333333333333333333333333333333333333333"
        );
        let created = bound_created_response(&action, &binding, &post_request);
        respond(&mut post, "201 Created", &created.to_string(), "");
    });

    let output = ao2(&[
        "issue",
        "draft-pr",
        "fixture-publish",
        "--action",
        action_path.to_str().unwrap(),
        "--expected-action-digest",
        &digest,
        "--fixture-api",
        &endpoint,
        "--json",
    ]);
    server.join().expect("server");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(readback["status"], "fixture_reported_created");
    assert_eq!(readback["post_performed"], true);
    assert_eq!(readback["fixture_write_observed"], false);
    assert_eq!(readback["client_contact_scope"], "numeric_loopback_only");
    assert_eq!(
        readback["fixture_exchange_attestation_status"],
        "strict_challenge_bound_self_attestation"
    );
    assert_eq!(
        readback["fixture_write_attestation_status"],
        "strict_challenge_bound_self_attestation"
    );
    assert_eq!(readback["fixture_claims_authenticated"], false);
    assert_eq!(
        readback["external_write_observability"],
        "not_observable_from_client"
    );
    assert_eq!(
        readback["behavior_outside_client_observable_boundary"],
        "not_claimed"
    );
    assert!(readback.get("github_write_performed").is_none());
    assert!(readback.get("public_github_write_performed").is_none());
    assert!(readback.get("fixture_write_performed").is_none());
    assert_eq!(readback["client_issue_write_performed"], false);
}

#[test]
fn fixture_publish_returns_exact_matching_draft_without_post() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let existing: serde_json::Value =
        serde_json::from_slice(&fs::read(named_fixture(EXISTING_FIXTURE)).unwrap()).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let server = std::thread::spawn(move || {
        let binding = serve_bound_prewrite_checks(&listener, &action);
        let (mut get, _) = listener.accept().expect("accept GET");
        let request = read_request(&mut get);
        assert_bound_fixture_request(&action, &binding, &request);
        respond(
            &mut get,
            "200 OK",
            &bound_pulls_response(&action, &binding, &request, existing).to_string(),
            "",
        );
        listener.set_nonblocking(true).expect("nonblocking");
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            listener.accept().is_err(),
            "idempotent readback must not POST"
        );
    });

    let output = ao2(&[
        "issue",
        "draft-pr",
        "fixture-publish",
        "--action",
        action_path.to_str().unwrap(),
        "--expected-action-digest",
        &digest,
        "--fixture-api",
        &endpoint,
        "--json",
    ]);
    server.join().expect("server");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(readback["status"], "idempotent_readback");
    assert_eq!(readback["post_performed"], false);
    assert_eq!(readback["fixture_write_observed"], false);
    assert_eq!(readback["client_contact_scope"], "numeric_loopback_only");
    assert_eq!(
        readback["fixture_exchange_attestation_status"],
        "strict_challenge_bound_self_attestation"
    );
    assert_eq!(
        readback["fixture_write_attestation_status"],
        "not_applicable"
    );
    assert_eq!(readback["fixture_claims_authenticated"], false);
    assert_eq!(
        readback["external_write_observability"],
        "not_observable_from_client"
    );
    assert_eq!(
        readback["behavior_outside_client_observable_boundary"],
        "not_claimed"
    );
    assert!(readback.get("fixture_write_performed").is_none());
    assert_eq!(readback["pull_number"], 9);
}

#[test]
fn fixture_publish_rejects_matching_refs_and_title_with_missing_or_wrong_issue_binding() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    for body in [
        "Repairs the verified issue #8 reproduction.\n\nEvidence is digest-bound and this pull request remains draft-only.",
        "Repairs the verified issue #8 reproduction.\n\nEvidence is digest-bound and this pull request remains draft-only.\n\nAO2-Evidence: issue_url=https://github.com/other/repository/issues/8 snapshot_sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        let mut unbound = exact_draft_response(&action);
        unbound["body"] = serde_json::json!(body);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let action_for_server = action.clone();
        let server = std::thread::spawn(move || {
            let binding = serve_bound_prewrite_checks(&listener, &action_for_server);
            let (mut get, _) = listener.accept().expect("accept GET");
            let request = read_request(&mut get);
            respond(
                &mut get,
                "200 OK",
                &bound_pulls_response(
                    &action_for_server,
                    &binding,
                    &request,
                    serde_json::json!([unbound]),
                )
                .to_string(),
                "",
            );
        });

        rejected(
            &[
                "issue",
                "draft-pr",
                "fixture-publish",
                "--action",
                action_path.to_str().unwrap(),
                "--expected-action-digest",
                &digest,
                "--fixture-api",
                &endpoint,
                "--json",
            ],
            "identity drift",
        );
        server.join().expect("server");
    }
}

#[test]
fn fixture_publish_rejects_missing_or_forwarding_capable_fixture_attestation() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    for (malformed, needle) in [(true, "attestation"), (false, "forwarding")] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let action_for_server = action.clone();
        let server = std::thread::spawn(move || {
            let (mut attest, _) = listener.accept().expect("accept attestation");
            let request = read_request(&mut attest);
            assert!(request.starts_with("GET /ao2/fixture-attestation HTTP/1.1"));
            let body = if malformed {
                serde_json::json!({})
            } else {
                let binding = capture_fixture_binding(&action_for_server, &request);
                let mut attestation =
                    bound_local_attestation(&action_for_server, &binding, &request);
                attestation["claims_forwarding_capable"] = serde_json::json!(true);
                attestation
            };
            respond(&mut attest, "200 OK", &body.to_string(), "");
            drop(attest);
            listener.set_nonblocking(true).expect("nonblocking");
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                listener.accept().is_err(),
                "unsafe attestation must stop before refs or writes"
            );
        });
        rejected(
            &[
                "issue",
                "draft-pr",
                "fixture-publish",
                "--action",
                action_path.to_str().unwrap(),
                "--expected-action-digest",
                &digest,
                "--fixture-api",
                &endpoint,
                "--json",
            ],
            needle,
        );
        server.join().expect("server");
    }
}

#[test]
fn fixture_publish_rejects_ref_drift_before_discovery_or_post() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let server = std::thread::spawn(move || {
        let (mut attest, _) = listener.accept().expect("accept attestation");
        let request = read_request(&mut attest);
        let binding = capture_fixture_binding(&action, &request);
        respond(
            &mut attest,
            "200 OK",
            &bound_local_attestation(&action, &binding, &request).to_string(),
            "",
        );
        drop(attest);

        let (mut base, _) = listener.accept().expect("accept base ref");
        let request = read_request(&mut base);
        assert!(request.contains("/git/ref/heads/main"));
        assert_bound_fixture_request(&action, &binding, &request);
        respond(
            &mut base,
            "200 OK",
            &bound_ref_response(
                &action,
                &binding,
                &request,
                "refs/heads/main",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "base_ref",
            )
            .to_string(),
            "",
        );
        drop(base);
        listener.set_nonblocking(true).expect("nonblocking");
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            listener.accept().is_err(),
            "ref drift must stop before discovery or POST"
        );
    });
    rejected(
        &[
            "issue",
            "draft-pr",
            "fixture-publish",
            "--action",
            action_path.to_str().unwrap(),
            "--expected-action-digest",
            &digest,
            "--fixture-api",
            &endpoint,
            "--json",
        ],
        "base ref commit drift",
    );
    server.join().expect("server");
}

#[test]
fn fixture_publish_requires_write_attestation_and_recovers_exact_create_race() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .unwrap()
        .to_owned();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let action_without_attestation = action.clone();
    let server = std::thread::spawn(move || {
        let binding = serve_bound_prewrite_checks(&listener, &action_without_attestation);
        let (mut discover, _) = listener.accept().expect("accept discovery");
        let request = read_request(&mut discover);
        respond(
            &mut discover,
            "200 OK",
            &bound_pulls_response(
                &action_without_attestation,
                &binding,
                &request,
                serde_json::json!([]),
            )
            .to_string(),
            "",
        );
        drop(discover);
        let (mut post, _) = listener.accept().expect("accept POST");
        let request = read_request(&mut post);
        respond(
            &mut post,
            "201 Created",
            &serde_json::json!({
                "pull": exact_draft_response(&action_without_attestation),
                "fixture_exchange_attestation": exchange_attestation(
                    &action_without_attestation,
                    &binding,
                    &request,
                    "created",
                    Some(9)
                )
            })
            .to_string(),
            "",
        );
    });
    rejected(
        &[
            "issue",
            "draft-pr",
            "fixture-publish",
            "--action",
            action_path.to_str().unwrap(),
            "--expected-action-digest",
            &digest,
            "--fixture-api",
            &endpoint,
            "--json",
        ],
        "write attestation",
    );
    server.join().expect("server");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let action_for_race = action.clone();
    let server = std::thread::spawn(move || {
        let binding = serve_bound_prewrite_checks(&listener, &action_for_race);
        let (mut discover, _) = listener.accept().expect("accept discovery");
        let request = read_request(&mut discover);
        respond(
            &mut discover,
            "200 OK",
            &bound_pulls_response(&action_for_race, &binding, &request, serde_json::json!([]))
                .to_string(),
            "",
        );
        drop(discover);
        let (mut post, _) = listener.accept().expect("accept POST");
        let request = read_request(&mut post);
        assert!(request.starts_with("POST /repos/uesugitorachiyo/ao-crucible/pulls "));
        respond(
            &mut post,
            "409 Conflict",
            &bound_conflict_response(&action_for_race, &binding, &request).to_string(),
            "",
        );
        drop(post);
        let (mut reread, _) = listener.accept().expect("accept race reread");
        let request = read_request(&mut reread);
        assert!(request.starts_with("GET /repos/uesugitorachiyo/ao-crucible/pulls?state=all"));
        respond(
            &mut reread,
            "200 OK",
            &bound_pulls_response(
                &action_for_race,
                &binding,
                &request,
                serde_json::json!([exact_draft_response(&action_for_race)]),
            )
            .to_string(),
            "",
        );
    });
    let output = ao2(&[
        "issue",
        "draft-pr",
        "fixture-publish",
        "--action",
        action_path.to_str().unwrap(),
        "--expected-action-digest",
        &digest,
        "--fixture-api",
        &endpoint,
        "--json",
    ]);
    server.join().expect("server");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let readback: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        readback["status"],
        "idempotent_readback_after_create_conflict"
    );
    assert_eq!(readback["post_performed"], true);
    assert_eq!(readback["fixture_write_observed"], false);
    assert_eq!(
        readback["fixture_exchange_attestation_status"],
        "strict_challenge_bound_self_attestation"
    );
    assert_eq!(
        readback["fixture_write_attestation_status"],
        "strict_challenge_bound_self_attestation"
    );
    assert_eq!(readback["fixture_claims_authenticated"], false);
    assert_eq!(readback["client_contact_scope"], "numeric_loopback_only");
    assert_eq!(
        readback["external_write_observability"],
        "not_observable_from_client"
    );
    assert!(readback.get("fixture_write_performed").is_none());
}

#[test]
fn fixture_publish_fails_closed_on_drift_ambiguity_and_unsafe_transport() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .unwrap()
        .to_owned();

    for response in [
        serde_json::json!([{
            "number": 9, "state": "open", "draft": false,
            "title": "drift", "body": "drift",
            "base": {"ref": "main", "sha": "2222222222222222222222222222222222222222"},
            "head": {"ref": "ao2/issue-8-bounded-repair", "sha": "3333333333333333333333333333333333333333",
                     "repo": {"full_name": "uesugitorachiyo/ao-crucible"}}
        }]),
        serde_json::json!([exact_draft_response(&action), exact_draft_response(&action)]),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let action_for_server = action.clone();
        let server = std::thread::spawn(move || {
            let binding = serve_bound_prewrite_checks(&listener, &action_for_server);
            let (mut get, _) = listener.accept().expect("accept GET");
            let request = read_request(&mut get);
            respond(
                &mut get,
                "200 OK",
                &bound_pulls_response(&action_for_server, &binding, &request, response).to_string(),
                "",
            );
        });
        rejected(
            &[
                "issue",
                "draft-pr",
                "fixture-publish",
                "--action",
                action_path.to_str().unwrap(),
                "--expected-action-digest",
                &digest,
                "--fixture-api",
                &endpoint,
                "--json",
            ],
            "fail",
        );
        server.join().expect("server");
    }

    for endpoint in [
        "http://localhost:1234",
        "http://user:pass@127.0.0.1:1234",
        "http://192.0.2.1:1234",
        "https://127.0.0.1:1234",
        "http://127.0.0.1:1234/extra",
    ] {
        rejected(
            &[
                "issue",
                "draft-pr",
                "fixture-publish",
                "--action",
                action_path.to_str().unwrap(),
                "--expected-action-digest",
                &digest,
                "--fixture-api",
                endpoint,
                "--json",
            ],
            "fixture",
        );
    }
}

#[test]
fn fixture_publish_rejects_redirects_and_oversized_responses() {
    let (_temp, action_path, action) = preview(&fixture_path());
    let digest = action["approval"]["action_digest"]
        .as_str()
        .unwrap()
        .to_owned();

    for (status, body, headers, needle) in [
        (
            "302 Found",
            "",
            "Location: http://127.0.0.1:1/\r\n",
            "redirect",
        ),
        ("200 OK", &" ".repeat(262_145), "", "262144"),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let body = body.to_string();
        let headers = headers.to_string();
        let server = std::thread::spawn(move || {
            let (mut get, _) = listener.accept().expect("accept GET");
            let _ = read_request(&mut get);
            respond(&mut get, status, &body, &headers);
        });
        rejected(
            &[
                "issue",
                "draft-pr",
                "fixture-publish",
                "--action",
                action_path.to_str().unwrap(),
                "--expected-action-digest",
                &digest,
                "--fixture-api",
                &endpoint,
                "--json",
            ],
            needle,
        );
        server.join().expect("server");
    }
}
