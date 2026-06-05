use std::fs;

use ao2_core::{
    annotate_obligation_ledger, check_obligation_ledger, extract_obligation_ledger,
    ObligationEvidence, ObligationStatus, ObligationVerdict,
};

#[test]
fn extractor_tracks_content_preservation_obligation_with_source_hash() {
    let spec = r#"
# Math Spec

- MUST preserve `net = gross - fees` exactly in the implementation note.
- MUST NOT remove the audit evidence section.
"#;

    let ledger = extract_obligation_ledger("SPEC.md", spec);

    assert_eq!(ledger.schema_version, "ao2.obligation-ledger.v1");
    assert_eq!(ledger.source_contracts.len(), 1);
    assert_eq!(ledger.obligations.len(), 2);
    assert_eq!(ledger.obligations[0].id, "OBL-001");
    assert_eq!(ledger.obligations[0].kind, "content_preservation");
    assert_eq!(ledger.obligations[0].source_line, 4);
    assert_eq!(
        ledger.obligations[0].expected_fragments,
        vec!["net = gross - fees"]
    );
    assert_eq!(ledger.obligations[0].status, ObligationStatus::Unverified);
    assert!(ledger.obligations[0]
        .source_excerpt_hash
        .starts_with("sha256:"));
    assert_eq!(ledger.verdict, ObligationVerdict::Rejected);
}

#[test]
fn checker_blocks_missing_preserved_equation_and_passes_when_present() {
    let spec = "- MUST preserve `net = gross - fees` exactly in the implementation note.\n";
    let ledger = extract_obligation_ledger("SPEC.md", spec);
    let temp = tempfile::tempdir().unwrap();

    fs::write(
        temp.path().join("README.md"),
        "The implementation note is missing.\n",
    )
    .unwrap();
    let missing = check_obligation_ledger(&ledger, temp.path()).unwrap();
    assert_eq!(missing.summary.fail, 1);
    assert_eq!(missing.summary.pass, 0);
    assert_eq!(missing.verdict, ObligationVerdict::Rejected);
    assert_eq!(missing.obligations[0].status, ObligationStatus::Fail);
    assert!(missing.obligations[0].evidence.is_empty());

    fs::write(
        temp.path().join("README.md"),
        "The implementation note preserves: net = gross - fees\n",
    )
    .unwrap();
    let present = check_obligation_ledger(&ledger, temp.path()).unwrap();
    assert_eq!(present.summary.fail, 0);
    assert_eq!(present.summary.pass, 1);
    assert_eq!(present.verdict, ObligationVerdict::Accepted);
    assert_eq!(present.obligations[0].status, ObligationStatus::Pass);
    assert_eq!(present.obligations[0].evidence[0].path, "README.md");
}

#[test]
fn checker_does_not_count_source_contract_as_evidence() {
    let spec = "- MUST preserve `net = gross - fees` exactly in the implementation note.\n";
    let ledger = extract_obligation_ledger("SPEC.md", spec);
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("SPEC.md"), spec).unwrap();

    let checked = check_obligation_ledger(&ledger, temp.path()).unwrap();

    assert_eq!(checked.summary.fail, 1);
    assert!(checked.obligations[0].evidence.is_empty());
}

#[test]
fn annotation_claim_requires_existing_path_line_before_passing() {
    let spec = "- MUST keep the business rule understandable to operators.\n";
    let ledger = extract_obligation_ledger("SPEC.md", spec);

    let annotated = annotate_obligation_ledger(
        &ledger,
        "OBL-001",
        Some(ObligationEvidence {
            path: "README.md".to_string(),
            line: 12,
            detail: "operator-facing rule is documented".to_string(),
        }),
        None,
    )
    .unwrap();

    assert_eq!(annotated.verdict, ObligationVerdict::Rejected);
    assert_eq!(annotated.summary.unverified, 1);
    assert_eq!(
        annotated.obligations[0].status,
        ObligationStatus::Unverified
    );
    assert_eq!(annotated.obligations[0].evidence[0].path, "README.md");

    let temp = tempfile::tempdir().unwrap();
    let missing = check_obligation_ledger(&annotated, temp.path()).unwrap();
    assert_eq!(missing.verdict, ObligationVerdict::Rejected);
    assert_eq!(missing.summary.fail, 1);
    assert_eq!(missing.obligations[0].status, ObligationStatus::Fail);

    fs::write(temp.path().join("README.md"), "too short\n").unwrap();
    let out_of_range = check_obligation_ledger(&annotated, temp.path()).unwrap();
    assert_eq!(out_of_range.verdict, ObligationVerdict::Rejected);
    assert_eq!(out_of_range.summary.fail, 1);
    assert_eq!(out_of_range.obligations[0].status, ObligationStatus::Fail);

    fs::write(
        temp.path().join("README.md"),
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\noperator-facing rule is documented\n",
    )
    .unwrap();
    let checked = check_obligation_ledger(&annotated, temp.path()).unwrap();
    assert_eq!(checked.verdict, ObligationVerdict::Accepted);
    assert_eq!(checked.summary.pass, 1);
    assert_eq!(checked.obligations[0].status, ObligationStatus::Pass);
}

#[test]
fn annotation_requires_evidence_or_waiver() {
    let spec = "- MUST keep the business rule understandable to operators.\n";
    let ledger = extract_obligation_ledger("SPEC.md", spec);

    // Neither evidence nor a waiver: the annotation is a no-op claim and must
    // be rejected rather than silently leaving the obligation unverified.
    let err = annotate_obligation_ledger(&ledger, "OBL-001", None, None).unwrap_err();
    assert_eq!(err, "annotation requires evidence or waiver");

    // A whitespace-only waiver is treated as empty and likewise rejected.
    let err =
        annotate_obligation_ledger(&ledger, "OBL-001", None, Some("   ".to_string())).unwrap_err();
    assert_eq!(err, "annotation requires evidence or waiver");
}

#[test]
fn annotation_rejects_unknown_obligation_id() {
    let spec = "- MUST keep the business rule understandable to operators.\n";
    let ledger = extract_obligation_ledger("SPEC.md", spec);

    let err = annotate_obligation_ledger(
        &ledger,
        "OBL-999",
        Some(ObligationEvidence {
            path: "README.md".to_string(),
            line: 1,
            detail: "evidence for a nonexistent obligation".to_string(),
        }),
        None,
    )
    .unwrap_err();
    assert_eq!(err, "unknown obligation id: OBL-999");
}

#[test]
fn annotation_allows_explicit_waiver_for_obligation() {
    let spec = "- MUST keep the business rule understandable to operators.\n";
    let ledger = extract_obligation_ledger("SPEC.md", spec);

    let annotated = annotate_obligation_ledger(
        &ledger,
        "OBL-001",
        None,
        Some("superseded by ADR-004".to_string()),
    )
    .unwrap();

    assert_eq!(annotated.verdict, ObligationVerdict::Accepted);
    assert_eq!(annotated.summary.waived, 1);
    assert_eq!(annotated.obligations[0].status, ObligationStatus::Waived);
    assert_eq!(
        annotated.obligations[0].waiver.as_deref(),
        Some("superseded by ADR-004")
    );
}
