use std::fs;
use std::process::Command;

#[test]
fn cli_contract_extract_and_check_blocks_missing_required_fragment() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("SPEC.md");
    let ledger = temp.path().join("obligations.json");
    let checked = temp.path().join("checked-obligations.json");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(
        &spec,
        "- MUST preserve `net = gross - fees` exactly in the implementation note.\n",
    )
    .unwrap();
    fs::write(target.join("README.md"), "No equation yet.\n").unwrap();

    let extract = ao2([
        "contract",
        "extract",
        "--spec",
        spec.to_str().unwrap(),
        "--out",
        ledger.to_str().unwrap(),
        "--json",
    ]);
    assert!(extract.status.success(), "{}", stderr(&extract));
    let extract_json: serde_json::Value = serde_json::from_str(&stdout(&extract)).unwrap();
    assert_eq!(extract_json["schema_version"], "ao2.obligation-ledger.v1");
    assert_eq!(extract_json["summary"]["unverified"], 1);

    let missing = ao2([
        "contract",
        "check",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--out",
        checked.to_str().unwrap(),
        "--json",
    ]);
    assert!(!missing.status.success());
    let missing_json: serde_json::Value = serde_json::from_str(&stdout(&missing)).unwrap();
    assert_eq!(missing_json["verdict"], "rejected");
    assert_eq!(missing_json["summary"]["fail"], 1);

    fs::write(
        target.join("README.md"),
        "The implementation note preserves: net = gross - fees\n",
    )
    .unwrap();
    let present = ao2([
        "contract",
        "check",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--out",
        checked.to_str().unwrap(),
        "--json",
    ]);
    assert!(present.status.success(), "{}", stderr(&present));
    let present_json: serde_json::Value = serde_json::from_str(&stdout(&present)).unwrap();
    assert_eq!(present_json["verdict"], "accepted");
    assert_eq!(present_json["summary"]["pass"], 1);
    assert_eq!(
        present_json["obligations"][0]["evidence"][0]["path"],
        "README.md"
    );
}

#[test]
fn cli_contract_gate_blocks_midpoint_when_required_fragment_missing() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("SPEC.md");
    let ledger = temp.path().join("obligations.json");
    let gate = temp.path().join("midpoint-gate.json");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(
        &spec,
        "- MUST preserve `net = gross - fees` exactly in the implementation note.\n",
    )
    .unwrap();
    fs::write(target.join("README.md"), "No equation yet.\n").unwrap();

    let extract = ao2([
        "contract",
        "extract",
        "--spec",
        spec.to_str().unwrap(),
        "--out",
        ledger.to_str().unwrap(),
        "--json",
    ]);
    assert!(extract.status.success(), "{}", stderr(&extract));

    let blocked = ao2([
        "contract",
        "gate",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--stage",
        "midpoint",
        "--out",
        gate.to_str().unwrap(),
        // Producer-side default-on signing is not part of this verdict test.
        "--allow-unsigned-obligation-gates",
        "--json",
    ]);
    assert!(!blocked.status.success());
    let json: serde_json::Value = serde_json::from_str(&stdout(&blocked)).unwrap();
    assert_eq!(json["schema_version"], "ao2.obligation-gate.v1");
    assert_eq!(json["stage"], "midpoint");
    assert_eq!(json["status"], "failed");
    assert_eq!(json["verdict"], "rejected");
    assert_eq!(json["summary"]["fail"], 1);
    assert_eq!(json["failed_obligations"][0]["id"], "OBL-001");

    let persisted: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&gate).unwrap()).unwrap();
    assert_eq!(persisted["schema_version"], "ao2.obligation-gate.v1");
    assert_eq!(persisted["status"], "failed");
}

#[test]
fn cli_contract_gate_accepts_closure_when_required_fragment_present() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("SPEC.md");
    let ledger = temp.path().join("obligations.json");
    let gate = temp.path().join("closure-gate.json");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(
        &spec,
        "- MUST preserve `net = gross - fees` exactly in the implementation note.\n",
    )
    .unwrap();
    fs::write(
        target.join("README.md"),
        "The implementation note preserves: net = gross - fees\n",
    )
    .unwrap();

    let extract = ao2([
        "contract",
        "extract",
        "--spec",
        spec.to_str().unwrap(),
        "--out",
        ledger.to_str().unwrap(),
        "--json",
    ]);
    assert!(extract.status.success(), "{}", stderr(&extract));

    let accepted = ao2([
        "contract",
        "gate",
        "--ledger",
        ledger.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--stage",
        "closure",
        "--out",
        gate.to_str().unwrap(),
        // Producer-side default-on signing is not part of this verdict test.
        "--allow-unsigned-obligation-gates",
        "--json",
    ]);
    assert!(accepted.status.success(), "{}", stderr(&accepted));
    let json: serde_json::Value = serde_json::from_str(&stdout(&accepted)).unwrap();
    assert_eq!(json["schema_version"], "ao2.obligation-gate.v1");
    assert_eq!(json["stage"], "closure");
    assert_eq!(json["status"], "passed");
    assert_eq!(json["verdict"], "accepted");
    assert_eq!(json["summary"]["pass"], 1);
    assert_eq!(
        json["checked_ledger"]["obligations"][0]["evidence"][0]["path"],
        "README.md"
    );
}

#[test]
fn cli_contract_annotate_adds_manual_semantic_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let spec = temp.path().join("SPEC.md");
    let ledger = temp.path().join("obligations.json");
    let annotated = temp.path().join("annotated-obligations.json");
    let checked = temp.path().join("checked-obligations.json");
    let target = temp.path().join("target");
    fs::create_dir_all(&target).unwrap();
    fs::write(
        target.join("README.md"),
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\noperator-facing rule is documented\n",
    )
    .unwrap();
    fs::write(
        &spec,
        "- MUST keep the business rule understandable to operators.\n",
    )
    .unwrap();

    let extract = ao2([
        "contract",
        "extract",
        "--spec",
        spec.to_str().unwrap(),
        "--out",
        ledger.to_str().unwrap(),
        "--json",
    ]);
    assert!(extract.status.success(), "{}", stderr(&extract));

    let annotate = ao2([
        "contract",
        "annotate",
        "--ledger",
        ledger.to_str().unwrap(),
        "--obligation-id",
        "OBL-001",
        "--evidence-path",
        "README.md",
        "--evidence-line",
        "12",
        "--detail",
        "operator-facing rule is documented",
        "--out",
        annotated.to_str().unwrap(),
        "--json",
    ]);

    assert!(annotate.status.success(), "{}", stderr(&annotate));
    let json: serde_json::Value = serde_json::from_str(&stdout(&annotate)).unwrap();
    assert_eq!(json["verdict"], "rejected");
    assert_eq!(json["summary"]["unverified"], 1);
    assert_eq!(json["obligations"][0]["evidence"][0]["path"], "README.md");

    let check = ao2([
        "contract",
        "check",
        "--ledger",
        annotated.to_str().unwrap(),
        "--target",
        target.to_str().unwrap(),
        "--out",
        checked.to_str().unwrap(),
        "--json",
    ]);
    assert!(check.status.success(), "{}", stderr(&check));
    let checked_json: serde_json::Value = serde_json::from_str(&stdout(&check)).unwrap();
    assert_eq!(checked_json["verdict"], "accepted");
    assert_eq!(checked_json["summary"]["pass"], 1);
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
