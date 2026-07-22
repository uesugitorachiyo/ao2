use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn cli_factory_pack_evidence_exports_ao2_owned_pack_for_factory_parity_oracle() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("factory-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp.path().join("request.yaml");
    fs::write(
        &request,
        r#"title: AO2 evidence handoff parity
objective: Let AO2 export the primary evidence pack while factory-v3 remains only a parity oracle.
acceptance:
  - AO2 owns the signed evidence pack source for release handoff and control-plane observation.
"#,
    )
    .unwrap();
    let runspec = temp.path().join("runspec.yaml");
    fs::write(
        &runspec,
        "id: evidence-pack-parity
verifier: python -m pytest -q
",
    )
    .unwrap();
    let plan_out = temp.path().join("pack-evidence-plan.json");
    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));

    let submit = ao2([
        "factory",
        "queue-submit",
        "--plan",
        plan_out.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "evidence-pack-parity-run",
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));

    let run_next = ao2([
        "factory",
        "queue-run-next",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(run_next.status.success(), "{}", stderr(&run_next));

    let pack_out = temp.path().join("ao2-primary-evidence-pack.json");
    let pack = ao2([
        "factory",
        "pack-evidence",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "evidence-pack-parity-run",
        "--out",
        pack_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(pack.status.success(), "{}", stderr(&pack));
    let json: serde_json::Value = serde_json::from_str(&stdout(&pack)).unwrap();
    assert_eq!(
        json["schema_version"],
        "ao2.factory-v3-compat-pack-evidence.v1"
    );
    assert_eq!(json["status"], "produced");
    assert_eq!(json["run_id"], "evidence-pack-parity-run");
    assert_eq!(json["evidence_pack_schema_version"], "ao2.evidence-pack.v1");
    assert_eq!(json["evidence_pack_execution_owner"], "ao2");
    assert_eq!(json["factory_v3_role"], "parity_oracle_only");
    assert_eq!(json["ao2_decision_owner"], "ao2-workbench-queue");
    assert_eq!(
        json["control_plane_role"],
        "read_only_observer_after_signed_evidence"
    );
    assert!(pack_out.is_file());
    let exported_pack: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&pack_out).unwrap()).unwrap();
    assert_eq!(exported_pack["schema_version"], "ao2.evidence-pack.v1");
    assert_eq!(exported_pack["runtime_contract"]["execution_owner"], "ao2");
    let exported_sha = format!("{:x}", Sha256::digest(fs::read(&pack_out).unwrap()));
    assert_eq!(json["evidence_pack_sha256"], exported_sha);

    // Unsigned baseline: deterministic_replay always runs, signature reports unsigned.
    assert_eq!(
        json["deterministic_replay"]["schema_version"],
        "ao2.factory-v3-compat-pack-evidence-deterministic-replay.v1"
    );
    assert_eq!(json["deterministic_replay"]["verified"], true);
    assert_eq!(json["deterministic_replay"]["written_sha256"], exported_sha);
    assert_eq!(json["deterministic_replay"]["replay_sha256"], exported_sha);
    assert_eq!(
        json["signature"]["schema_version"],
        "ao2.factory-v3-compat-pack-evidence-signature.v1"
    );
    assert_eq!(json["signature"]["signature_verified"], false);
    assert_eq!(json["signature"]["signature_status"], "unsigned");
    assert_eq!(json["signature"]["signed_payload"], "evidence_pack_out");
    // Unsigned runs must not write sidecar files.
    let signature_sidecar = pack_out.with_extension("json.sig");
    let public_key_sidecar = pack_out.with_extension("json.public.pem");
    assert!(
        !signature_sidecar.exists(),
        "unsigned pack-evidence must not write {}",
        signature_sidecar.display()
    );
    assert!(
        !public_key_sidecar.exists(),
        "unsigned pack-evidence must not write {}",
        public_key_sidecar.display()
    );
}

#[test]
fn cli_factory_pack_evidence_signs_evidence_pack_for_release_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let repo = pack_evidence_seed_completed_entry(temp.path(), "pack-signed-run");
    let pack_out = temp.path().join("ao2-signed-evidence-pack.json");
    let signing_key = temp.path().join("pack-evidence-signing-key.pem");
    generate_native_signing_key(&signing_key, 2048);

    let pack = ao2([
        "factory",
        "pack-evidence",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "pack-signed-run",
        "--out",
        pack_out.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "ao2-release-handoff-signer",
        "--json",
    ]);
    assert!(pack.status.success(), "{}", stderr(&pack));
    let json: serde_json::Value = serde_json::from_str(&stdout(&pack)).unwrap();

    assert_eq!(json["status"], "produced");
    assert_eq!(json["run_id"], "pack-signed-run");
    let exported_sha = format!("{:x}", Sha256::digest(fs::read(&pack_out).unwrap()));
    assert_eq!(json["evidence_pack_sha256"], exported_sha);

    // Deterministic replay must hold for the signed flow too.
    assert_eq!(json["deterministic_replay"]["verified"], true);
    assert_eq!(json["deterministic_replay"]["written_sha256"], exported_sha);
    assert_eq!(json["deterministic_replay"]["replay_sha256"], exported_sha);

    // Signature block: AO2 owns the key material and the verify call.
    assert_eq!(json["signature"]["signature_verified"], true);
    assert_eq!(json["signature"]["signature_algorithm"], "RSA/SHA-256");
    assert_eq!(json["signature"]["signer_id"], "ao2-release-handoff-signer");
    assert_eq!(json["signature"]["signed_payload"], "evidence_pack_out");
    assert_eq!(json["signature"]["signed_payload_sha256"], exported_sha);

    // Sidecars exist next to --out with predictable extensions.
    let signature_path = PathBuf::from(json["signature"]["signature_path"].as_str().unwrap());
    let public_key_path = PathBuf::from(json["signature"]["public_key_path"].as_str().unwrap());
    assert_eq!(signature_path, pack_out.with_extension("json.sig"));
    assert_eq!(public_key_path, pack_out.with_extension("json.public.pem"));
    assert!(signature_path.is_file());
    assert!(public_key_path.is_file());
    let signature_sha_disk = format!("{:x}", Sha256::digest(fs::read(&signature_path).unwrap()));
    let public_key_sha_disk = format!("{:x}", Sha256::digest(fs::read(&public_key_path).unwrap()));
    assert_eq!(json["signature"]["signature_sha256"], signature_sha_disk);
    assert_eq!(json["signature"]["public_key_sha256"], public_key_sha_disk);

    // Tampering the packed evidence breaks the signature on re-verify, proving
    // AO2 owns the trust boundary rather than re-deriving from factory-v3.
    let mut tampered: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&pack_out).unwrap()).unwrap();
    tampered["runtime_contract"]["execution_owner"] = serde_json::json!("factory-v3");
    fs::write(
        &pack_out,
        serde_json::to_string_pretty(&tampered).unwrap() + "\n",
    )
    .unwrap();
    let tampered_pack_sha = format!("{:x}", Sha256::digest(fs::read(&pack_out).unwrap()));
    assert_ne!(tampered_pack_sha, exported_sha);

    // Re-running pack-evidence on the same queue entry must overwrite the
    // tampered file with the canonical AO2-owned pack and re-sign cleanly.
    let pack_again = ao2([
        "factory",
        "pack-evidence",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "pack-signed-run",
        "--out",
        pack_out.to_str().unwrap(),
        "--signing-key",
        signing_key.to_str().unwrap(),
        "--signer-id",
        "ao2-release-handoff-signer",
        "--json",
    ]);
    assert!(pack_again.status.success(), "{}", stderr(&pack_again));
    let json_again: serde_json::Value = serde_json::from_str(&stdout(&pack_again)).unwrap();
    assert_eq!(json_again["signature"]["signature_verified"], true);
    assert_eq!(json_again["deterministic_replay"]["verified"], true);
    assert_eq!(
        json_again["evidence_pack_sha256"],
        exported_sha,
        "re-run must produce the original canonical SHA, proving deterministic replay across invocations"
    );
}

fn pack_evidence_seed_completed_entry(temp_root: &Path, run_id: &str) -> PathBuf {
    let repo = temp_root.join(format!("pack-target-{run_id}"));
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let request = temp_root.join(format!("pack-request-{run_id}.yaml"));
    fs::write(
        &request,
        "title: AO2 pack-evidence seed
objective: Provide an evidence pack the nightly producer can consume.
acceptance:
  - queue-run-next persists evidence_pack on the entry without secrets.
",
    )
    .unwrap();
    let runspec = temp_root.join(format!("pack-runspec-{run_id}.yaml"));
    fs::write(
        &runspec,
        "id: pack-evidence-seed
verifier: python -m pytest -q
",
    )
    .unwrap();
    let plan_out = temp_root.join(format!("pack-plan-{run_id}.json"));
    let plan = ao2([
        "factory",
        "plan",
        "--request",
        request.to_str().unwrap(),
        "--runspec",
        runspec.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--out",
        plan_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let submit = ao2([
        "factory",
        "queue-submit",
        "--plan",
        plan_out.to_str().unwrap(),
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        run_id,
        "--json",
    ]);
    assert!(submit.status.success(), "{}", stderr(&submit));
    let run_next = ao2([
        "factory",
        "queue-run-next",
        "--target",
        repo.to_str().unwrap(),
        "--json",
    ]);
    assert!(run_next.status.success(), "{}", stderr(&run_next));
    repo
}

#[test]
fn cli_factory_pack_evidence_selects_latest_completed_entry_when_run_id_absent() {
    let temp = tempfile::tempdir().unwrap();
    let repo = pack_evidence_seed_completed_entry(temp.path(), "pack-latest-only");
    let out = temp.path().join("packed-latest.json");
    let pack = ao2([
        "factory",
        "pack-evidence",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(pack.status.success(), "{}", stderr(&pack));
    let json: serde_json::Value = serde_json::from_str(&stdout(&pack)).unwrap();
    assert_eq!(json["run_id"], "pack-latest-only");
    assert_eq!(json["status"], "produced");
    assert_eq!(json["entry_status"], "accepted");
    assert!(out.is_file());
}

#[test]
fn cli_factory_pack_evidence_rejects_unknown_run_id() {
    let temp = tempfile::tempdir().unwrap();
    let repo = pack_evidence_seed_completed_entry(temp.path(), "pack-known");
    let out = temp.path().join("packed-unknown.json");
    let pack = ao2([
        "factory",
        "pack-evidence",
        "--target",
        repo.to_str().unwrap(),
        "--run-id",
        "pack-not-in-queue",
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        !pack.status.success(),
        "expected failure for unknown run_id"
    );
    let err = stderr(&pack);
    assert!(
        err.contains("pack-not-in-queue"),
        "stderr names the missing run_id: {err}"
    );
    assert!(
        !out.exists(),
        "no evidence pack written when run_id unknown"
    );
}

#[test]
fn cli_factory_pack_evidence_blocks_when_queue_has_no_completed_entries() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("empty-target");
    copy_git_fixture(Path::new("../../fixtures/discount-service"), &repo);
    let out = temp.path().join("packed-empty.json");
    let pack = ao2([
        "factory",
        "pack-evidence",
        "--target",
        repo.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
    ]);
    assert!(!pack.status.success(), "expected failure when queue empty");
    let err = stderr(&pack);
    assert!(
        err.contains("nothing to pack evidence for")
            || err.contains("no completed entries")
            || err.contains("no entries"),
        "stderr explains empty queue: {err}"
    );
    assert!(!out.exists());
}
