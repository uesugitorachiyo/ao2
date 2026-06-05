//! Coverage for `ArtifactStore::put_text`, the content-addressed write path
//! every produced artifact flows through. The crate previously had no tests;
//! `put_text` is only exercised indirectly via the runtime. This pins the
//! contract directly: the content file and the manifest are both written, the
//! recorded digest matches the SHA-256 of the bytes, and the returned
//! `ArtifactRef` fields round-trip into the on-disk manifest.

use std::fs;

use ao2_artifacts::ArtifactStore;
use ao2_core::sha256_hex;

#[test]
fn put_text_writes_content_and_manifest_with_matching_digest() {
    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());

    let content = "evaluation report body\n";
    let artifact = store
        .put_text(
            "evaluation_report",
            "evaluator-closer",
            "report.txt",
            "text/plain",
            content,
            vec!["art-input-1".to_string()],
        )
        .unwrap();

    // Returned ref carries the metadata we passed plus the derived digest.
    assert_eq!(artifact.artifact_type, "evaluation_report");
    assert_eq!(artifact.producer, "evaluator-closer");
    assert_eq!(artifact.media_type, "text/plain");
    assert_eq!(artifact.sensitivity, "internal");
    assert_eq!(artifact.input_refs, vec!["art-input-1".to_string()]);
    assert_eq!(artifact.digest, sha256_hex(content.as_bytes()));

    // Content file exists at the reported URI with the exact bytes.
    let written = fs::read_to_string(&artifact.uri).unwrap();
    assert_eq!(written, content);

    // Manifest is written alongside the content under the artifact dir and
    // deserializes back to an equal ArtifactRef.
    let manifest_path = temp
        .path()
        .join(&artifact.artifact_id)
        .join("artifact.json");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    let restored: ao2_core::ArtifactRef = serde_json::from_str(&manifest).unwrap();
    assert_eq!(restored.artifact_id, artifact.artifact_id);
    assert_eq!(restored.digest, artifact.digest);
    assert_eq!(restored.uri, artifact.uri);
}

#[test]
fn put_text_gives_each_artifact_an_isolated_directory() {
    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());

    let first = store
        .put_text("note", "p", "a.txt", "text/plain", "one", vec![])
        .unwrap();
    let second = store
        .put_text("note", "p", "a.txt", "text/plain", "two", vec![])
        .unwrap();

    // Same file name, distinct artifact ids -> distinct dirs, no clobber.
    assert_ne!(first.artifact_id, second.artifact_id);
    assert_eq!(fs::read_to_string(&first.uri).unwrap(), "one");
    assert_eq!(fs::read_to_string(&second.uri).unwrap(), "two");
}

#[test]
fn root_returns_the_configured_directory() {
    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());
    assert_eq!(store.root(), temp.path());
}

#[test]
fn put_text_handles_empty_and_non_ascii_content() {
    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());

    // Empty content is a valid artifact and must hash to the empty-input digest.
    let empty = store
        .put_text("note", "p", "empty.txt", "text/plain", "", vec![])
        .unwrap();
    assert_eq!(empty.digest, sha256_hex(b""));
    assert_eq!(fs::read_to_string(&empty.uri).unwrap(), "");

    // Multi-byte UTF-8 round-trips byte-exact, and the digest is over the
    // UTF-8 bytes (not chars).
    let unicode = "日本語とemoji😀\n";
    let artifact = store
        .put_text(
            "note",
            "p",
            "u.txt",
            "text/plain; charset=utf-8",
            unicode,
            vec![],
        )
        .unwrap();
    assert_eq!(artifact.digest, sha256_hex(unicode.as_bytes()));
    assert_eq!(fs::read_to_string(&artifact.uri).unwrap(), unicode);
}
