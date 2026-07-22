use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use sha2::{Digest, Sha256};

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

fn read_test_http_request(stream: &mut TcpStream, buffer: &mut [u8]) -> usize {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    stream.read(buffer).unwrap()
}

#[test]
fn test_http_accept_waits_for_slow_windows_child_startup() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let client = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5500));
        TcpStream::connect(("127.0.0.1", port)).unwrap();
    });

    let stream = accept_test_connection(&listener, "delayed local test HTTP request");
    drop(stream);
    client.join().unwrap();
}

#[test]
fn cli_init_provider_profiles_and_template_run_support_fast_start() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("discount-service");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);

    let init = ao2(["init", "--target", repo.to_str().unwrap()]);
    assert!(init.status.success(), "{}", stderr(&init));
    let profiles = fs::read_to_string(repo.join(".ao2/provider-profiles.json")).unwrap();
    assert!(profiles.contains("\"codex\""));
    assert!(profiles.contains("\"claude\""));
    assert!(profiles.contains("\"scripted\""));

    let list = ao2(["provider", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert!(stdout(&list).contains("codex"));
    assert!(stdout(&list).contains("claude"));

    let doctor = ao2(["provider", "doctor", "--provider", "scripted"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let doctor_json: serde_json::Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(doctor_json["provider"], "scripted");

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
printf 'Summary: provider preset template run fixed discount validation\n'
printf 'Changed files: discount_service/discounts.py\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "--template",
        "bug-fix",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "preset-template-run",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);
    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Accepted"));
    assert!(repo.join(".ao2/generated-workflows/bug-fix.yaml").is_file());
}

#[test]
fn cli_run_provider_prompt_executes_provider_backed_risky_run() {
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
printf 'Summary: added validation around discount math\n'
printf 'Changed files: discount_service/discounts.py\n'
printf 'Input tokens: 10\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "provider-cli-run",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Accepted"));
    let evidence = fs::read_to_string(
        repo.join(".ao2/runs/provider-cli-run/evidence-pack/evidence-pack.json"),
    )
    .unwrap();
    assert!(evidence.contains("sandbox_patch_apply"));
    assert!(evidence.contains("provider_summaries"));
    assert!(evidence.contains("added validation around discount math"));
}

#[test]
fn cli_run_provider_prompt_honors_zero_repair_budget() {
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
printf 'Summary: validation without tests\n'
printf 'Changed files: discount_service/discounts.py\n'
"#,
    )
    .unwrap();

    let run = ao2([
        "run",
        "../../examples/risky-pr-run/risky-pr.yaml",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "provider-cli-budget-zero",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        prompt_path.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
    ]);

    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("status=Rejected"));
    let evidence = fs::read_to_string(
        repo.join(".ao2/runs/provider-cli-budget-zero/evidence-pack/evidence-pack.json"),
    )
    .unwrap();
    assert!(evidence.contains("repair_budget_exhausted"));
    assert!(evidence.contains("repair_attempts"));
}

#[test]
fn cli_repair_resume_uses_rejected_evidence_context_for_new_run() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("real-project-repair-resume");
    fs::create_dir_all(repo.join("docs")).unwrap();
    fs::write(repo.join("README.md"), "real project\n").unwrap();
    init_existing_git_repo(&repo);
    let workflow = temp.path().join("repair-resume.yaml");
    fs::write(
        &workflow,
        r#"id: repair-resume
version: 0.1.0
template_kind: real_project
objective: Repair a failed run from prior signed evidence context.
roles:
  - planner
  - implementer
  - reviewer
  - test-engineer
  - evaluator-closer
verifier:
  command: test -f docs/fixed.txt
acceptance:
  - Fixed artifact exists after repair resume.
  - Prior verifier context is carried into the repair prompt.
"#,
    )
    .unwrap();
    let failed_prompt = temp.path().join("failed-prompt.sh");
    fs::write(
        &failed_prompt,
        r#"printf 'first attempt\n' > docs/first-attempt.txt
printf 'Summary: failed repair source run\n'
printf 'Changed files: docs/first-attempt.txt\n'
"#,
    )
    .unwrap();

    let failed = ao2([
        "run",
        workflow.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "repair-source",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        failed_prompt.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
    ]);
    assert!(failed.status.success(), "{}", stderr(&failed));
    assert!(stdout(&failed).contains("status=Rejected"));
    let source_evidence = repo.join(".ao2/runs/repair-source/evidence-pack/evidence-pack.json");
    let source_evidence_text = fs::read_to_string(&source_evidence).unwrap();
    assert!(source_evidence_text.contains("budget_exhausted"));

    let repair_prompt = temp.path().join("repair-prompt.sh");
    fs::write(
        &repair_prompt,
        r#"if printf '%s' "$AO2_REPAIR_RUN_HEALTH" | grep -q 'budget_exhausted' \
  && printf '%s' "$AO2_REPAIR_VERIFIER_OUTPUT" | grep -q 'docs/fixed.txt' \
  && test "$AO2_REPAIR_SOURCE_RUN_ID" = "repair-source"; then
  printf 'fixed\n' > docs/fixed.txt
else
  printf 'missing carried repair context\n' >&2
  exit 2
fi
printf 'Summary: repaired from rejected AO2 evidence context\n'
printf 'Changed files: docs/fixed.txt\n'
"#,
    )
    .unwrap();

    let repaired = ao2([
        "repair",
        "resume",
        "--evidence-pack",
        source_evidence.to_str().unwrap(),
        "--workflow",
        workflow.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "repair-resumed",
        "--provider",
        "scripted",
        "--provider-prompt-file",
        repair_prompt.to_str().unwrap(),
        "--max-repair-attempts",
        "0",
        "--json",
    ]);
    assert!(repaired.status.success(), "{}", stderr(&repaired));
    let repaired_json: serde_json::Value = serde_json::from_str(&stdout(&repaired)).unwrap();
    assert_eq!(repaired_json["schema_version"], "ao2.repair-resume.v1");
    assert_eq!(repaired_json["source_run_id"], "repair-source");
    assert_eq!(repaired_json["status"], "accepted");
    assert_eq!(
        fs::read_to_string(repo.join("docs/fixed.txt")).unwrap(),
        "fixed\n"
    );

    let repaired_evidence =
        fs::read_to_string(repo.join(".ao2/runs/repair-resumed/evidence-pack/evidence-pack.json"))
            .unwrap();
    assert!(repaired_evidence.contains("repair_source_context"));
    assert!(repaired_evidence.contains("\"source_run_id\": \"repair-source\""));
    assert!(repaired_evidence.contains("docs/fixed.txt"));
    assert!(repaired_evidence.contains("repair_source"));
    assert!(repaired_evidence.contains("provider_transcript_summary"));
}

#[test]
fn cli_release_phase1_decision_publish_signs_and_posts_to_control_plane() {
    let temp = tempfile::tempdir().unwrap();
    let decision_path = temp.path().join("phase1-decision.json");
    let signing_key = temp.path().join("phase1-decision-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    fs::write(
        &decision_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "factory-v3/ao2-phase1-promotion-decision/v1",
            "status": "passed",
            "decision": "promote_phase1_candidate",
            "phase1_state": "phase1_candidate_ready",
            "checklist_sha256": "a".repeat(64),
            "operator": "release-lead",
            "rationale": "All required Phase 1 evidence is present.",
            "artifacts": {
                "phase1_promotion_checklist": "phase1-promotion-checklist.json"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let expected_decision: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&decision_path).unwrap()).unwrap();
    let expected_decision_raw = serde_json::to_string_pretty(&expected_decision).unwrap();

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
                        "timed out waiting for Phase 1 decision publish request"
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
        assert!(request.starts_with("POST /api/v1/phase1/promotion/decision/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request
            .contains("\"schema_version\":\"ao2.cp-phase1-promotion-decision-signed-upload.v1\""));
        assert!(request.contains("\"schema\":\"factory-v3/ao2-phase1-promotion-decision/v1\""));
        assert!(request.contains("\"decision\":\"promote_phase1_candidate\""));
        assert!(request.contains("\"signature_algorithm\":\"RSA/SHA-256\""));
        assert!(request.contains("\"signature_hex\""));
        assert!(request.contains("\"public_key_sha256\""));
        assert!(request.contains("\"public_key_pem\""));
        assert!(request.contains("\"signer_id\":\"release-lead\""));
        assert!(!request.contains("cp-token\""));
        let request_body = request
            .split("\r\n\r\n")
            .nth(1)
            .expect("signed phase1 decision request has body");
        let upload: serde_json::Value = serde_json::from_str(request_body).unwrap();
        let decision_b64 = upload["decision_b64"]
            .as_str()
            .expect("signed phase1 decision upload carries exact decision_b64 bytes");
        {
            use base64::prelude::{Engine as _, BASE64_STANDARD};
            let decoded = BASE64_STANDARD.decode(decision_b64).unwrap();
            assert_eq!(decoded, expected_decision_raw.as_bytes());
        }
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"decision123","stored_at":"2026-05-22T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-decision/v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "release",
        "phase1-decision-publish",
        "--decision",
        decision_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "release-lead",
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
        "ao2.phase1-promotion-decision-control-plane-publish.v1"
    );
    assert_eq!(json["signed"], true);
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/decision/signed")
    );
    assert_eq!(
        json["receipt"]["ingested_schema_version"],
        "factory-v3/ao2-phase1-promotion-decision/v1"
    );
    assert_eq!(
        json["signature"]["schema_version"],
        "ao2.cp-phase1-promotion-decision-signature.v1"
    );
}

#[test]
fn cli_release_phase1_decision_publish_posts_referenced_checklist_before_signed_decision() {
    let temp = tempfile::tempdir().unwrap();
    let decision_path = temp.path().join("phase1-decision.json");
    let checklist_path = temp.path().join("phase1-promotion-checklist.json");
    let signing_key = temp.path().join("phase1-decision-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    let checklist = serde_json::json!({
        "schema": "factory-v3/ao2-phase1-promotion-checklist/v1",
        "schema_version": "ao2.phase1-promotion-checklist.v1",
        "status": "passed",
        "phase1_state": "phase1_candidate_ready",
        "next_action": "publish signed Phase 1 promotion decision",
        "checklist": {
            "provider_readiness": {"status": "superseded_by_live_acceptance", "phase1_state": "passed"},
            "live_provider_acceptance": {"status": "passed", "state": "live_acceptance_complete"},
            "release_gate": {"status": "passed", "state": "verified"},
            "three_os_smoke": {"status": "passed", "state": "accepted"}
        }
    });
    fs::write(
        &checklist_path,
        serde_json::to_string_pretty(&checklist).unwrap(),
    )
    .unwrap();
    let checklist_sha = canonical_sha256_for_test(&checklist);
    fs::write(
        &decision_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "factory-v3/ao2-phase1-promotion-decision/v1",
            "status": "passed",
            "decision": "promote_phase1_candidate",
            "phase1_state": "phase1_candidate_ready",
            "checklist_sha256": checklist_sha,
            "operator": "release-lead",
            "rationale": "All required Phase 1 evidence is present.",
            "artifacts": {
                "phase1_promotion_checklist": checklist_path.file_name().unwrap().to_string_lossy()
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let checklist_sha_for_server = checklist_sha.clone();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let mut attempts = 0;
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        attempts += 1;
                        assert!(
                            attempts <= 100,
                            "timed out waiting for Phase 1 publish request"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            };
            let mut buffer = [0_u8; 32768];
            stream.set_nonblocking(false).unwrap();
            let read = read_test_http_request(&mut stream, &mut buffer);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let body = if request.starts_with("POST /api/v1/phase1/promotion/checklist HTTP/1.1") {
                format!(
                    r#"{{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"{checklist_sha_for_server}","stored_at":"2026-05-26T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-checklist/v1"}}"#
                )
            } else {
                r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"decision456","stored_at":"2026-05-26T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-decision/v1"}"#.to_string()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
            requests.push(request);
        }
        assert!(requests[0].starts_with("POST /api/v1/phase1/promotion/checklist HTTP/1.1"));
        assert!(requests[1].starts_with("POST /api/v1/phase1/promotion/decision/signed HTTP/1.1"));
        assert!(requests[0].contains("Authorization: Bearer cp-token"));
        assert!(requests[1].contains("Authorization: Bearer cp-token"));
        assert!(requests[0].contains("\"schema\":\"factory-v3/ao2-phase1-promotion-checklist/v1\""));
        assert!(requests[1].contains("\"checklist_sha256\""));
        assert!(!requests.join("\n").contains("cp-token\""));
    });

    let publish = ao2([
        "release",
        "phase1-decision-publish",
        "--decision",
        decision_path.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "release-lead",
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let json: serde_json::Value = serde_json::from_str(&stdout(&publish)).unwrap();
    assert_eq!(json["checklist_publish"]["status"], "posted");
    assert_eq!(
        json["checklist_publish"]["receipt"]["sha256"],
        checklist_sha
    );
    assert_eq!(json["receipt"]["sha256"], "decision456");
}

#[test]
fn cli_release_phase1_decision_build_binds_release_and_replacement_gates() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.release-gate.v1",
            "status": "verified",
            "release": {
                "provenance_verified": true,
                "archive_count": 4
            },
            "smoke": {
                "status": "verified"
            },
            "obligation_gates": {
                "status": "verified"
            },
            "obligation_gate_signing": {
                "status": "verified"
            },
            "replacement_smoke_gate": {
                "schema": "ao2.release-replacement-smoke-gate-verification.v1",
                "status": "verified",
                "gate_status": "accepted",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "reasons": []
            },
            "governed_run_evidence": {
                "schema": "ao2.release-governed-run-evidence-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "factory_project_run_readback": {
                "schema": "ao2.release-factory-project-run-readback-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "reasons": []
        }))
        .unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 owns the replacement run path and all Phase 1 gates are verified.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-decision-build.v1"
    );
    assert_eq!(json["status"], "written");
    assert_eq!(json["decision"]["decision"], "promote_phase1_candidate");
    assert_eq!(json["checklist"]["status"], "passed");
    assert_eq!(
        json["checklist"]["replacement_smoke_gate"]["accepted_os"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["checklist"]["trust_boundary"]["ao2_decision_owner"],
        "ao2-native-phase1-promotion-decision-builder"
    );
    assert!(decision_path.is_file());
    let decision: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&decision_path).unwrap()).unwrap();
    assert_eq!(
        decision["schema"],
        "factory-v3/ao2-phase1-promotion-decision/v1"
    );
    assert_eq!(decision["status"], "passed");
    assert_eq!(decision["phase1_state"], "phase1_candidate_ready");
    assert_eq!(
        decision["artifacts"]["replacement_smoke_gate"],
        replacement_gate_path.display().to_string()
    );
    assert_eq!(
        decision["trust_boundary"]["factory_v3_role"],
        "parity_oracle_only"
    );
}

#[test]
fn cli_release_phase1_decision_build_binds_three_os_governed_run_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 owns the governed run path and all Phase 1 gates are verified.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["checklist"]["three_os_governed_run"]["accepted_os"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["checklist"]["release_gate"]["governed_run_evidence_verification"]["status"],
        "verified"
    );
    assert!(json["checklist"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |check| check["id"] == "governed-run-evidence-accepted" && check["status"] == "passed"
        ));
    let decision: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&decision_path).unwrap()).unwrap();
    assert_eq!(
        decision["artifacts"]["governed_run_evidence"],
        serde_json::json!([
            governed_run_paths[0].display().to_string(),
            governed_run_paths[1].display().to_string(),
            governed_run_paths[2].display().to_string()
        ])
    );
    assert_eq!(
        decision["artifacts"]["factory_project_run_readback"],
        serde_json::json!([
            project_run_readbacks[0].display().to_string(),
            project_run_readbacks[1].display().to_string(),
            project_run_readbacks[2].display().to_string()
        ])
    );
}

#[test]
fn cli_release_phase1_decision_build_allows_governed_run_only_promotion() {
    let temp = tempfile::tempdir().unwrap();
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.release-gate.v1",
            "status": "verified",
            "release": {
                "provenance_verified": true,
                "archive_count": 4
            },
            "smoke": {
                "status": "verified"
            },
            "obligation_gates": {
                "status": "verified"
            },
            "obligation_gate_signing": {
                "status": "verified"
            },
            "governed_run_evidence": {
                "schema": "ao2.release-governed-run-evidence-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "factory_project_run_readback": {
                "schema": "ao2.release-factory-project-run-readback-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "reasons": []
        }))
        .unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 governed-run evidence supersedes the legacy replacement-smoke gate.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["checklist"]["checklist"]["three_os_smoke"]["status"],
        "superseded_by_governed_run"
    );
    assert_eq!(
        json["checklist"]["three_os_governed_run"]["accepted_os"],
        serde_json::json!(["macos", "ubuntu", "windows"])
    );
    assert_eq!(
        json["decision"]["artifacts"]["governed_run_evidence"],
        serde_json::json!([
            governed_run_paths[0].display().to_string(),
            governed_run_paths[1].display().to_string(),
            governed_run_paths[2].display().to_string()
        ])
    );
    assert!(json["decision"]["artifacts"]["replacement_smoke_gate"].is_null());
}

#[test]
fn cli_release_phase1_decision_build_rejects_missing_project_run_readback_hard_gate() {
    let temp = tempfile::tempdir().unwrap();
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 must not promote without replacement-packet readback proof.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("project-run readback"));
}

#[test]
fn cli_release_phase1_decision_build_rejects_missing_governed_run_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "Missing governed run evidence should not promote.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("governed run evidence"));
}

#[test]
fn cli_release_phase1_decision_build_binds_three_provider_acceptance_preservation() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let provider_acceptance_path = temp.path().join("provider-acceptance-preservation.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &provider_acceptance_path,
        serde_json::to_string_pretty(&accepted_provider_acceptance_preservation_fixture()).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--provider-acceptance-preservation",
        provider_acceptance_path.to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "AO2 owns the replacement run path and all Phase 1 gates are verified.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(
        json["checklist"]["provider_acceptance_preservation"]["providers"],
        serde_json::json!(["codex", "claude", "antigravity"])
    );
    assert_eq!(
        json["decision"]["artifacts"]["provider_acceptance_preservation"],
        provider_acceptance_path.display().to_string()
    );
    assert!(json["checklist"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |check| check["id"] == "provider-acceptance-preservation-verified"
                && check["status"] == "passed"
        ));
}

#[test]
fn cli_release_phase1_decision_build_rejects_incomplete_provider_acceptance_preservation() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let provider_acceptance_path = temp.path().join("provider-acceptance-preservation.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&accepted_replacement_smoke_gate_fixture()).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&verified_release_gate_with_governed_run_fixture()).unwrap(),
    )
    .unwrap();
    let mut provider_acceptance = accepted_provider_acceptance_preservation_fixture();
    provider_acceptance["providers"]
        .as_object_mut()
        .unwrap()
        .remove("antigravity");
    fs::write(
        &provider_acceptance_path,
        serde_json::to_string_pretty(&provider_acceptance).unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--provider-acceptance-preservation",
        provider_acceptance_path.to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "Provider acceptance must be complete.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("provider acceptance preservation missing antigravity"));
}

#[test]
fn cli_release_phase1_decision_build_rejects_unverified_replacement_gate() {
    let temp = tempfile::tempdir().unwrap();
    let replacement_gate_path = temp.path().join("replacement-smoke-gate.json");
    let release_gate_path = temp.path().join("release-gate.json");
    let decision_path = temp.path().join("phase1-decision.json");
    let governed_run_paths = write_phase1_governed_run_evidence(temp.path());
    let project_run_readbacks = write_factory_project_run_readbacks(temp.path());
    let mut replacement_gate = accepted_replacement_smoke_gate_fixture();
    replacement_gate["status"] = serde_json::json!("rejected");
    fs::write(
        &replacement_gate_path,
        serde_json::to_string_pretty(&replacement_gate).unwrap(),
    )
    .unwrap();
    fs::write(
        &release_gate_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.release-gate.v1",
            "status": "verified",
            "replacement_smoke_gate": {
                "schema": "ao2.release-replacement-smoke-gate-verification.v1",
                "status": "failed",
                "gate_status": "rejected",
                "accepted_os": ["macos", "ubuntu"],
                "reasons": [{"code": "replacement_smoke_gate_missing_os"}]
            },
            "governed_run_evidence": {
                "schema": "ao2.release-governed-run-evidence-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "factory_project_run_readback": {
                "schema": "ao2.release-factory-project-run-readback-verification.v1",
                "status": "verified",
                "accepted_os": ["macos", "ubuntu", "windows"],
                "missing_os": [],
                "duplicate_os": [],
                "unknown_os": [],
                "input_errors": [],
                "reasons": []
            },
            "reasons": [{"code": "replacement_smoke_gate_failed"}]
        }))
        .unwrap(),
    )
    .unwrap();

    let build = ao2([
        "release",
        "phase1-decision-build",
        "--release-gate",
        release_gate_path.to_str().unwrap(),
        "--replacement-smoke-gate",
        replacement_gate_path.to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[0].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[1].to_str().unwrap(),
        "--governed-run-evidence",
        governed_run_paths[2].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[0].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[1].to_str().unwrap(),
        "--factory-project-run-summary",
        project_run_readbacks[2].to_str().unwrap(),
        "--operator",
        "release-lead",
        "--rationale",
        "Bad gate should not promote.",
        "--out",
        decision_path.to_str().unwrap(),
        "--json",
    ]);
    assert!(!build.status.success());
    assert!(!decision_path.exists());
    assert!(stderr(&build).contains("replacement smoke gate must be accepted"));
}

fn verified_release_gate_fixture() -> serde_json::Value {
    serde_json::json!({
        "schema": "ao2.release-gate.v1",
        "status": "verified",
        "release": {
            "provenance_verified": true,
            "archive_count": 4
        },
        "smoke": {
            "status": "verified"
        },
        "obligation_gates": {
            "status": "verified"
        },
        "obligation_gate_signing": {
            "status": "verified"
        },
        "replacement_smoke_gate": {
            "schema": "ao2.release-replacement-smoke-gate-verification.v1",
            "status": "verified",
            "gate_status": "accepted",
            "accepted_os": ["macos", "ubuntu", "windows"],
            "reasons": []
        },
        "reasons": []
    })
}

fn verified_release_gate_with_governed_run_fixture() -> serde_json::Value {
    let mut release_gate = verified_release_gate_fixture();
    release_gate["governed_run_evidence"] = serde_json::json!({
        "schema": "ao2.release-governed-run-evidence-verification.v1",
        "status": "verified",
        "accepted_os": ["macos", "ubuntu", "windows"],
        "missing_os": [],
        "duplicate_os": [],
        "unknown_os": [],
        "input_errors": [],
        "reasons": []
    });
    release_gate["factory_project_run_readback"] = serde_json::json!({
        "schema": "ao2.release-factory-project-run-readback-verification.v1",
        "status": "verified",
        "accepted_os": ["macos", "ubuntu", "windows"],
        "missing_os": [],
        "duplicate_os": [],
        "unknown_os": [],
        "input_errors": [],
        "reasons": []
    });
    release_gate
}

fn accepted_governed_run_fixture(run_id: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-governed-run.v1",
        "status": "accepted",
        "run_id": run_id,
        "plan": {
            "ao2_native_plan": {
                "role_contract_discovery": {
                    "mode": "auto_discovered_from_ao_runspec_layout",
                    "loaded_count": 7
                }
            }
        },
        "run_result_verification": {
            "status": "accepted"
        },
        "pack_evidence": {
            "status": "produced",
            "signature": {
                "signature_verified": true
            }
        },
        "evaluator_decision": {
            "verdict": "accepted",
            "signature": {
                "signature_verified": true
            }
        },
        "evaluator_decision_verification": {
            "status": "accepted",
            "signature_verified": true
        },
        "governed_run_checklist": {
            "ao2_planned_factory_compat_workflow": true,
            "ao2_queue_executed_factory_compat_workflow": true,
            "ao2_verified_primary_run_result": true,
            "ao2_packed_primary_evidence": true,
            "ao2_signed_evaluator_closure": true,
            "ao2_auto_loaded_role_contracts": true,
            "factory_v3_drives_workflow": false
        },
        "artifacts": {
            "governed_run": format!("target/{run_id}/governed-run.json"),
            "run_result_verification": format!("target/{run_id}/run-result-verification.json"),
            "evidence_pack": format!("target/{run_id}/evidence-pack.json"),
            "evaluator_decision": format!("target/{run_id}/evaluator-decision.json")
        },
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-governed-run",
        "control_plane_role": "read_only_observer_after_signed_evidence"
    })
}

fn write_phase1_governed_run_evidence(root: &Path) -> Vec<PathBuf> {
    ["macos", "ubuntu", "windows"]
        .into_iter()
        .map(|os_label| {
            let dir = root.join("governed-run-evidence").join(os_label);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("governed-run.json");
            fs::write(
                &path,
                serde_json::to_string_pretty(&accepted_governed_run_fixture(&format!(
                    "real-factory-runspec-{os_label}"
                )))
                .unwrap(),
            )
            .unwrap();
            path
        })
        .collect()
}

fn accepted_factory_project_run_readback_fixture(os_label: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.factory-project-run-smoke.v1",
        "status": "passed",
        "host_os": os_label,
        "run_id": format!("factory-project-run-{os_label}"),
        "factory_project_schema": "ao2.factory-project-run.v1",
        "queued_auto_replacement_packet": format!("target/{os_label}/queued/factory-replacement-packet.json"),
        "queued_auto_replacement_packet_archive": format!("target/{os_label}/queued/factory-replacement-packet.tgz"),
        "queued_auto_replacement_packet_status": "packaged",
        "queued_auto_replacement_packet_verification": format!("target/{os_label}/queued/factory-replacement-packet-verification.json"),
        "queued_auto_replacement_packet_verification_status": "accepted",
        "queued_auto_replacement_packet_verification_checksums_verified": true,
        "queued_auto_replacement_packet_verification_trust_boundary_verified": true,
        "queued_replacement_packet": format!("target/{os_label}/factory-replacement-packet.json"),
        "queued_replacement_packet_archive": format!("target/{os_label}/factory-replacement-packet.tgz"),
        "queued_replacement_packet_schema": "ao2.factory-replacement-packet.v1",
        "queued_replacement_packet_status": "packaged",
        "queued_replacement_packet_sha256": "a".repeat(64),
        "queued_replacement_packet_ao2_replaces_factory_v3_workflow_driver": true,
        "queued_replacement_packet_factory_v3_role": "evaluator_closer_and_sampling_auditor",
        "queued_replacement_packet_verification": format!("target/{os_label}/factory-replacement-packet-verification.json"),
        "queued_replacement_packet_verification_schema": "ao2.factory-replacement-packet-verification.v1",
        "queued_replacement_packet_verification_status": "accepted",
        "queued_replacement_packet_verification_checksums_verified": true,
        "queued_replacement_packet_verification_trust_boundary_verified": true,
        "queued_replacement_packet_verification_ao2_replacement_driver_verified": true,
        "queued_replacement_packet_verification_factory_v3_evaluator_closer_verified": true
    })
}

fn write_factory_project_run_readbacks(root: &Path) -> Vec<PathBuf> {
    ["macos", "ubuntu", "windows"]
        .into_iter()
        .map(|os_label| {
            let dir = root.join("factory-project-run-readback").join(os_label);
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("factory-project-run-summary.json");
            fs::write(
                &path,
                serde_json::to_string_pretty(&accepted_factory_project_run_readback_fixture(
                    os_label,
                ))
                .unwrap(),
            )
            .unwrap();
            path
        })
        .collect()
}

fn accepted_provider_acceptance_preservation_fixture() -> serde_json::Value {
    serde_json::json!({
        "schema": "ao2.provider-pilot-acceptance-preservation.v1",
        "status": "passed",
        "tag": "v0.4.80",
        "providers": {
            "codex": {
                "schema_version": "ao2.codex-provider-pilot-acceptance.v1",
                "source_class": "live",
                "run_id": "live-codex-provider-pilot",
                "smoke_score": 100,
                "minimum_score": 90,
                "replay_status": "accepted",
                "digest_failures": 0,
                "preserved": "target/release-evidence/provider-pilot-acceptance/v0.4.80/codex/provider-pilot-acceptance.json"
            },
            "claude": {
                "schema_version": "ao2.claude-provider-pilot-acceptance.v1",
                "source_class": "live",
                "run_id": "live-claude-provider-pilot",
                "smoke_score": 100,
                "minimum_score": 90,
                "replay_status": "accepted",
                "digest_failures": 0,
                "preserved": "target/release-evidence/provider-pilot-acceptance/v0.4.80/claude/provider-pilot-acceptance.json"
            },
            "antigravity": {
                "schema_version": "ao2.antigravity-provider-pilot-acceptance.v1",
                "source_class": "live",
                "run_id": "live-antigravity-provider-pilot",
                "smoke_score": 100,
                "minimum_score": 90,
                "replay_status": "accepted",
                "digest_failures": 0,
                "preserved": "target/release-evidence/provider-pilot-acceptance/v0.4.80/antigravity/provider-pilot-acceptance.json"
            }
        }
    })
}

fn accepted_replacement_smoke_gate_fixture() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "ao2.factory-v3-compat-three-os-replacement-smoke-gate.v1",
        "status": "accepted",
        "accepted_os": ["macos", "ubuntu", "windows"],
        "missing_os": [],
        "duplicate_os": [],
        "unknown_os": [],
        "input_errors": [],
        "factory_v3_role": "parity_oracle_only",
        "ao2_decision_owner": "ao2-native-three-os-replacement-smoke-gate",
        "control_plane_role": "read_only_observer_after_signed_evidence",
        "three_os_contract": {
            "path_separator_safe_artifacts": true,
            "requires_native_windows_smoke": true,
            "requires_ubuntu_smoke": true,
            "requires_macos_smoke": true,
            "provider_auth": "local OAuth CLI only; API-key provider auth forbidden"
        }
    })
}

#[test]
fn cli_release_phase1_decision_publish_reads_api_token_from_env_without_printing_secret() {
    let temp = tempfile::tempdir().unwrap();
    let decision_path = temp.path().join("phase1-decision.json");
    let signing_key = temp.path().join("phase1-decision-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);
    fs::write(
        &decision_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "factory-v3/ao2-phase1-promotion-decision/v1",
            "status": "passed",
            "decision": "promote_phase1_candidate",
            "phase1_state": "phase1_candidate_ready",
            "checklist_sha256": "b".repeat(64),
            "operator": "release-lead",
            "rationale": "All required Phase 1 evidence is present.",
            "artifacts": {
                "phase1_promotion_checklist": "phase1-promotion-checklist.json"
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
                        "timed out waiting for Phase 1 decision publish request"
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
        assert!(request.starts_with("POST /api/v1/phase1/promotion/decision/signed HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-phase1-token"));
        assert!(request.contains("\"decision\":\"promote_phase1_candidate\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"decisionenv123","stored_at":"2026-05-22T00:00:00Z","ingested_schema_version":"factory-v3/ao2-phase1-promotion-decision/v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2_with_env(
        [
            "release",
            "phase1-decision-publish",
            "--decision",
            decision_path.to_str().unwrap(),
            "--signing-key",
            signing_key.to_str().unwrap(),
            "--signer-id",
            "release-lead",
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_TEST_PHASE1_CP_TOKEN",
            "--json",
        ],
        [("AO2_TEST_PHASE1_CP_TOKEN", "env-phase1-token")],
    );
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    let stderr = stderr(&publish);
    assert!(!stdout.contains("env-phase1-token"));
    assert!(!stderr.contains("env-phase1-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-decision-control-plane-publish.v1"
    );
    assert_eq!(json["receipt"]["sha256"], "decisionenv123");
}

#[test]
fn cli_release_phase1_three_os_smoke_build_materializes_control_plane_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let smoke_root = temp.path().join("three-os-smoke");
    fs::create_dir_all(&smoke_root).unwrap();
    let local_log = smoke_root.join("local-smoke.log");
    let windows_log = smoke_root.join("windows-smoke.log");
    let report = smoke_root.join("report.md");
    fs::write(&local_log, "local smoke passed\n").unwrap();
    fs::write(&windows_log, "windows native smoke passed\n").unwrap();
    fs::write(&report, "# report\n").unwrap();

    let summary_path = smoke_root.join("summary.enriched.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.three-os-smoke-summary.v1",
            "root": smoke_root.display().to_string(),
            "report": report.display().to_string(),
            "local_smoke": "passed",
            "linux_x86_64_remote_smoke": "passed",
            "native_windows_required": true,
            "windows_native_smoke": "passed",
            "windows_log": windows_log.display().to_string()
        }))
        .unwrap(),
    )
    .unwrap();
    let provenance_path = temp.path().join("ao2-release-provenance.json");
    fs::write(
        &provenance_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2.release-provenance.v1",
            "version": "0.4.80",
            "git_commit": "addb602d07e413ca5b565d8ebca986925a97017f",
            "git_dirty": false,
            "release_tag": "v0.4.80"
        }))
        .unwrap(),
    )
    .unwrap();
    let out = temp.path().join("phase1-three-os-release-smoke.json");

    let build = ao2([
        "release",
        "phase1-three-os-smoke-build",
        "--summary",
        summary_path.to_str().unwrap(),
        "--provenance",
        provenance_path.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let json: serde_json::Value = serde_json::from_str(&stdout(&build)).unwrap();
    assert_eq!(json["schema_version"], "ao2.phase1-three-os-smoke-build.v1");
    assert_eq!(json["status"], "written");
    assert!(out.is_file());

    let bundle: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(
        bundle["schema"],
        "ao2-control-plane.three-os-release-smoke.v1"
    );
    assert_eq!(bundle["status"], "passed");
    assert_eq!(bundle["version"], "0.4.80");
    assert_eq!(bundle["release_candidate_version"], "0.4.80");
    assert_eq!(
        bundle["source_commit"],
        "addb602d07e413ca5b565d8ebca986925a97017f"
    );
    assert_eq!(bundle["source_dirty"], false);
    assert_eq!(bundle["targets"]["macos"]["status"], "passed");
    assert_eq!(bundle["targets"]["ubuntu"]["status"], "passed");
    assert_eq!(bundle["targets"]["windows"]["status"], "passed");
    assert_eq!(
        bundle["targets"]["windows"]["log"],
        windows_log.display().to_string()
    );
    assert!(bundle["rerun_commands"]["all_required"]
        .as_str()
        .unwrap()
        .contains("<local-token>"));
}

#[test]
fn cli_release_phase1_three_os_smoke_publish_posts_bundle_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let smoke_path = temp.path().join("phase1-three-os-release-smoke.json");
    fs::write(
        &smoke_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": "ao2-control-plane.three-os-release-smoke.v1",
            "version": "0.4.80",
            "status": "passed",
            "release_candidate_version": "0.4.80",
            "source_commit": "addb602d07e413ca5b565d8ebca986925a97017f",
            "source_dirty": false,
            "targets": {
                "macos": {"status": "passed", "log": "target/three-os-smoke/run/local-smoke.log"},
                "ubuntu": {"status": "passed", "log": "target/three-os-smoke/run/local-smoke.log"},
                "windows": {"status": "passed", "log": "target/three-os-smoke/run/windows-smoke.log"}
            },
            "rerun_commands": [
                "AO2_PHASE1_CP_TOKEN=<local-token> target/release/ao2 release phase1-three-os-smoke-publish"
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream =
            accept_test_connection(&listener, "Phase 1 three-OS smoke publish request");
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/phase1/promotion/three-os-smoke HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        assert!(request.contains("\"schema\":\"ao2-control-plane.three-os-release-smoke.v1\""));
        assert!(request.contains("\"status\":\"passed\""));
        assert!(request.contains("\"source_dirty\":false"));
        assert!(!request.contains("cp-token\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"threeos123","stored_at":"2026-05-26T00:00:00Z","ingested_schema_version":"ao2-control-plane.three-os-release-smoke.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2([
        "release",
        "phase1-three-os-smoke-publish",
        "--smoke",
        smoke_path.to_str().unwrap(),
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--json",
    ]);
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    assert!(!stdout.contains("cp-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-three-os-smoke-control-plane-publish.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/three-os-smoke")
    );
    assert_eq!(json["receipt"]["sha256"], "threeos123");
}

#[test]
fn cli_release_phase1_promotion_inputs_publish_posts_verification_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let verification_path = temp.path().join("promotion-inputs-verification.json");
    fs::write(
        &verification_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "ao2.phase1-replacement-promotion-inputs-verification.v1",
            "status": "accepted",
            "mode": "decision_gate",
            "manifest_path": "/work/ao2/target/phase1-replacement-promotion/promotion-inputs.json",
            "missing_required_inputs": [],
            "failure_count": 0,
            "failures": [],
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": false,
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_approves_release": false
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut stream =
            accept_test_connection(&listener, "Phase 1 promotion inputs publish request");
        let mut buffer = [0_u8; 16384];
        stream.set_nonblocking(false).unwrap();
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("POST /api/v1/phase1/promotion/inputs-verification HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-cp-token"));
        assert!(request.contains(
            "\"schema_version\":\"ao2.phase1-replacement-promotion-inputs-verification.v1\""
        ));
        assert!(request.contains("\"status\":\"accepted\""));
        assert!(request.contains("\"control_plane_approves_release\":false"));
        assert!(!request.contains("env-cp-token\""));
        let body = r#"{"schema_version":"ao2.cp-ingest-receipt.v1","sha256":"inputs123","stored_at":"2026-05-29T00:00:00Z","ingested_schema_version":"ao2.phase1-replacement-promotion-inputs-verification.v1"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let publish = ao2_with_env(
        [
            "release",
            "phase1-promotion-inputs-publish",
            "--verification",
            verification_path.to_str().unwrap(),
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_PHASE1_CP_TOKEN",
            "--json",
        ],
        [("AO2_PHASE1_CP_TOKEN", "env-cp-token")],
    );
    server.join().unwrap();
    assert!(publish.status.success(), "{}", stderr(&publish));
    let stdout = stdout(&publish);
    assert!(!stdout.contains("env-cp-token"));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-inputs-control-plane-publish.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/inputs-verification")
    );
    assert_eq!(json["receipt"]["sha256"], "inputs123");
}

#[test]
fn cli_release_phase1_history_fetch_reads_control_plane_history_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("phase1-history.json");
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
                        "timed out waiting for Phase 1 history fetch request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("GET /api/v1/phase1/promotion/history.json HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer cp-token"));
        let body = r#"{"schema_version":"ao2.cp-phase1-promotion-history.v1","counts":{"checklists":1,"signed_decisions":1,"three_os_smokes":1},"history":{"checklists":[],"signed_decisions":[],"three_os_smokes":[]},"trust_boundary":{"role":"read_only_observer","mutates_ao_artifacts":false,"release_acceptance_owner":"factory-v3 evaluator-closer"}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let fetch = ao2([
        "release",
        "phase1-history-fetch",
        "--control-plane-url",
        &format!("http://127.0.0.1:{port}"),
        "--api-token",
        "cp-token",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    server.join().unwrap();
    assert!(fetch.status.success(), "{}", stderr(&fetch));
    let json: serde_json::Value = serde_json::from_str(&stdout(&fetch)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-history-control-plane-fetch.v1"
    );
    assert_eq!(
        json["endpoint"],
        format!("http://127.0.0.1:{port}/api/v1/phase1/promotion/history.json")
    );
    assert_eq!(json["history"]["counts"]["checklists"], 1);
    assert_eq!(json["trust_boundary"]["mutates_ao_artifacts"], false);
    assert!(out.is_file());
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out).unwrap()).unwrap();
    assert_eq!(
        written["schema_version"],
        "ao2.cp-phase1-promotion-history.v1"
    );
    assert!(!stdout(&fetch).contains("cp-token"));
}

#[test]
fn cli_release_phase1_history_fetch_accepts_api_token_env_without_token_leak() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("phase1-history-env.json");
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
                        "timed out waiting for Phase 1 history env-token fetch request"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        let mut buffer = [0_u8; 8192];
        let read = read_test_http_request(&mut stream, &mut buffer);
        let request = String::from_utf8_lossy(&buffer[..read]).to_string();
        assert!(request.starts_with("GET /api/v1/phase1/promotion/history.json HTTP/1.1"));
        assert!(request.contains("Authorization: Bearer env-cp-token"));
        let body = r#"{"schema_version":"ao2.cp-phase1-promotion-history.v1","counts":{"checklists":1,"signed_decisions":1,"three_os_smokes":1},"history":{"checklists":[],"signed_decisions":[],"three_os_smokes":[]},"trust_boundary":{"role":"read_only_observer","mutates_ao_artifacts":false,"release_acceptance_owner":"factory-v3 evaluator-closer"}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let fetch = ao2_with_env(
        [
            "release",
            "phase1-history-fetch",
            "--control-plane-url",
            &format!("http://127.0.0.1:{port}"),
            "--api-token-env",
            "AO2_TEST_CP_TOKEN",
            "--out",
            out.to_str().unwrap(),
            "--json",
        ],
        [("AO2_TEST_CP_TOKEN", "env-cp-token")],
    );
    server.join().unwrap();
    assert!(fetch.status.success(), "{}", stderr(&fetch));
    let json: serde_json::Value = serde_json::from_str(&stdout(&fetch)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.phase1-promotion-history-control-plane-fetch.v1"
    );
    assert_eq!(json["history"]["counts"]["three_os_smokes"], 1);
    assert!(out.is_file());
    assert!(!stdout(&fetch).contains("env-cp-token"));
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
