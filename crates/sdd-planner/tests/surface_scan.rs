//! P1 acceptance tests for the surface scanner (README §10 P1).

use std::fs;
use std::path::PathBuf;

use sdd_planner::surface::{canonical_json, extract_public_symbols, scan, MAX_FILE_SIZE_BYTES};
use sdd_planner::SurfaceMap;

const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

fn tiny_repo_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tiny-repo")
}

fn scan_tiny_repo() -> SurfaceMap {
    scan(&tiny_repo_path(), ZERO_SHA.to_string()).expect("scan tiny-repo")
}

fn canonicalize(map: &SurfaceMap) -> String {
    canonical_json(&serde_json::to_value(map).expect("surface map → value"))
}

fn write(path: PathBuf, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap_or_else(|e| panic!("write {path:?}: {e}"));
}

#[test]
fn matches_golden_canonical_json() {
    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tiny-repo-surface-map.json");
    let golden = fs::read_to_string(&golden_path).expect("read golden");
    let actual = canonicalize(&scan_tiny_repo());
    assert_eq!(
        actual,
        golden.trim_end_matches('\n'),
        "golden surface map drifted: regenerate tiny-repo-surface-map.json"
    );
}

#[test]
fn files_sorted_ascending_by_path() {
    let map = scan_tiny_repo();
    let paths: Vec<&str> = map.files.iter().map(|f| f.path.as_str()).collect();
    let mut expected = paths.clone();
    expected.sort();
    assert_eq!(paths, expected, "files must be sorted by path ASC");
}

#[test]
fn excludes_target_git_node_modules_dirs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    write(root.join("keep.rs"), "pub fn k() {}\n");
    write(root.join("target/build.rs"), "pub fn t() {}\n");
    write(root.join(".git/HEAD"), "ref: refs/heads/main\n");
    write(root.join("node_modules/lib.js"), "export const x = 1;\n");
    write(root.join("src/.git/x.rs"), "pub fn nested() {}\n");

    let map = scan(&root, ZERO_SHA.to_string()).expect("scan");
    let paths: Vec<&str> = map.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["keep.rs"],
        "only keep.rs should survive; got {paths:?}"
    );
}

#[test]
fn excludes_secret_basenames() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    write(root.join("ok.rs"), "pub fn ok() {}\n");
    write(root.join(".env"), "SECRET=1\n");
    write(root.join(".env.local"), "X=2\n");
    write(root.join("server.pem"), "-----PEM-----\n");
    write(root.join("id_rsa.key"), "PRIVATE\n");

    let map = scan(&root, ZERO_SHA.to_string()).expect("scan");
    let paths: Vec<&str> = map.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["ok.rs"],
        "secrets must be dropped; got {paths:?}"
    );
}

#[test]
fn drops_files_larger_than_one_mib() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    write(root.join("small.rs"), "pub fn s() {}\n");
    let oversize = vec![b'a'; (MAX_FILE_SIZE_BYTES as usize) + 1];
    fs::write(root.join("big.rs"), &oversize).unwrap();

    let map = scan(&root, ZERO_SHA.to_string()).expect("scan");
    let paths: Vec<&str> = map.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["small.rs"],
        "oversize file must be dropped; got {paths:?}"
    );
}

#[test]
fn allow_list_filters_unknown_extensions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    write(root.join("code.rs"), "pub fn r() {}\n");
    write(root.join("notes.md"), "# title\n");
    write(root.join("blob.bin"), "binary\n");
    write(root.join("image.png"), "png-bytes\n");
    write(root.join("script.exe"), "exe\n");
    write(root.join("Makefile"), "all:\n\t@echo hi\n");

    let map = scan(&root, ZERO_SHA.to_string()).expect("scan");
    let mut paths: Vec<&str> = map.files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec!["code.rs", "notes.md"],
        "allow-list violated; got {paths:?}"
    );
}

#[test]
fn public_symbols_extracted_for_rust_files() {
    let map = scan_tiny_repo();
    let lib = map
        .files
        .iter()
        .find(|f| f.path == "src/lib.rs")
        .expect("src/lib.rs present");
    assert_eq!(lib.kind, "rust");
    assert_eq!(
        lib.public_symbols,
        vec!["Config", "Mode", "Wakeup", "greet"],
        "public symbols mismatch"
    );

    let toml = map
        .files
        .iter()
        .find(|f| f.path == "Cargo.toml")
        .expect("Cargo.toml present");
    assert!(
        toml.public_symbols.is_empty(),
        "non-Rust files must have no public_symbols"
    );
}

#[test]
fn crlf_and_lf_source_hashes_match() {
    let lf_dir = tempfile::tempdir().expect("lf tempdir");
    let crlf_dir = tempfile::tempdir().expect("crlf tempdir");
    write(lf_dir.path().join("src/lib.rs"), "pub fn stable() {}\n");
    write(crlf_dir.path().join("src/lib.rs"), "pub fn stable() {}\r\n");

    let lf_map = scan(lf_dir.path(), ZERO_SHA.to_string()).expect("scan lf");
    let crlf_map = scan(crlf_dir.path(), ZERO_SHA.to_string()).expect("scan crlf");

    assert_eq!(lf_map.files.len(), 1);
    assert_eq!(lf_map.files[0].sha256, crlf_map.files[0].sha256);
    assert_eq!(
        lf_map.files[0].public_symbols,
        crlf_map.files[0].public_symbols
    );
}

#[test]
fn extract_public_symbols_handles_duplicates_and_kinds() {
    let src = "\
pub fn alpha() {}
pub fn alpha() {}
pub struct Beta;
pub enum Gamma { X }
pub trait Delta {}
fn private_one() {}
";
    let syms = extract_public_symbols(src);
    assert_eq!(
        syms,
        vec![
            "Beta".to_string(),
            "Delta".to_string(),
            "Gamma".to_string(),
            "alpha".to_string()
        ],
        "expected dedup + alphabetical (uppercase < lowercase in UTF-8)"
    );
}

#[test]
fn two_scans_produce_byte_equal_canonical_json() {
    let a = canonicalize(&scan_tiny_repo());
    let b = canonicalize(&scan_tiny_repo());
    assert_eq!(a, b, "scan is non-deterministic");
}
