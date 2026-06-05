//! P2 acceptance tests for the context shrinker (README §10 P2).

use std::collections::HashSet;

use sdd_planner::context::{count_tokens, shrink};
use sdd_planner::schema::{SurfaceFile, SurfaceMap};
use sdd_planner::surface::canonical_json;

const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

fn file(path: &str, kind: &str, symbols: &[&str]) -> SurfaceFile {
    SurfaceFile {
        kind: kind.to_string(),
        path: path.to_string(),
        public_symbols: symbols.iter().map(|s| s.to_string()).collect(),
        sha256: "deadbeef".repeat(8),
    }
}

fn json_tokens(map: &SurfaceMap) -> usize {
    count_tokens(&canonical_json(&serde_json::to_value(map).unwrap()))
}

#[test]
fn within_token_budget() {
    let files: Vec<SurfaceFile> = (0..40)
        .map(|i| file(&format!("src/mod_{i:02}.rs"), "rust", &[]))
        .collect();
    let map = SurfaceMap {
        head_sha: ZERO_SHA.to_string(),
        files,
    };
    let budget = 200usize;
    let result = shrink(&map, "auth login session token rotation", budget);
    assert!(
        json_tokens(&result) <= budget,
        "shrink produced {} tokens, budget {budget}",
        json_tokens(&result)
    );
}

#[test]
fn force_includes_known_manifests() {
    let map = SurfaceMap {
        head_sha: ZERO_SHA.to_string(),
        files: vec![
            file("Cargo.toml", "toml", &[]),
            file("README.md", "markdown", &[]),
            file("package.json", "json", &[]),
            file("src/unrelated.rs", "rust", &["Unrelated"]),
        ],
    };
    let result = shrink(&map, "totally unrelated prompt about pancakes", 8000);
    let paths: HashSet<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains("Cargo.toml"), "Cargo.toml missing");
    assert!(paths.contains("README.md"), "README.md missing");
    assert!(paths.contains("package.json"), "package.json missing");
}

#[test]
fn manifests_force_included_overrides_tight_budget() {
    let map = SurfaceMap {
        head_sha: ZERO_SHA.to_string(),
        files: vec![
            file("Cargo.toml", "toml", &[]),
            file("src/unrelated.rs", "rust", &[]),
        ],
    };
    // Budget so tight that no candidate could fit on top of forced.
    let result = shrink(&map, "anything", 1);
    let paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"Cargo.toml"), "forced file dropped");
    assert!(
        !paths.contains(&"src/unrelated.rs"),
        "non-forced file should be cut at tight budget"
    );
}

#[test]
fn forced_manifests_only_when_present() {
    // None of the forced basenames appear in input — output must not invent them.
    let map = SurfaceMap {
        head_sha: ZERO_SHA.to_string(),
        files: vec![file("src/a.rs", "rust", &[]), file("src/b.rs", "rust", &[])],
    };
    let result = shrink(&map, "test", 8000);
    let paths: HashSet<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert!(!paths.contains("Cargo.toml"));
    assert!(!paths.contains("README.md"));
    assert!(!paths.contains("package.json"));
}

#[test]
fn higher_jaccard_chosen_when_only_one_fits() {
    // Two candidates, identical shape; auth-related symbol matches the prompt.
    let map = SurfaceMap {
        head_sha: ZERO_SHA.to_string(),
        files: vec![
            file("src/pancake.rs", "rust", &["Pancake"]),
            file("src/auth.rs", "rust", &["AuthSession"]),
        ],
    };
    let prompt = "auth login session";

    // Compute the token cost of including BOTH, then ask for budget = (both - 1).
    // Greedy: high-jaccard goes first; adding pancake then pushes over, gets popped.
    let both = shrink(&map, prompt, 100_000);
    assert_eq!(both.files.len(), 2, "wide budget should keep both");
    let both_tokens = json_tokens(&both);
    assert!(both_tokens > 1, "sanity");

    let tight = shrink(&map, prompt, both_tokens - 1);
    let paths: Vec<&str> = tight.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths.len(), 1, "expected exactly one file to be cut");
    assert_eq!(paths[0], "src/auth.rs", "high-jaccard file must win");
}

#[test]
fn output_sorted_by_path() {
    let map = SurfaceMap {
        head_sha: ZERO_SHA.to_string(),
        files: vec![
            file("src/z.rs", "rust", &[]),
            file("src/a.rs", "rust", &[]),
            file("Cargo.toml", "toml", &[]),
            file("src/m.rs", "rust", &[]),
        ],
    };
    let result = shrink(&map, "irrelevant", 8000);
    let paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    let mut expected = paths.clone();
    expected.sort();
    assert_eq!(paths, expected, "files must be sorted by path ASC");
}

#[test]
fn deterministic_byte_equal_output() {
    let files: Vec<SurfaceFile> = (0..30)
        .map(|i| {
            file(
                &format!("src/handler_{i:02}.rs"),
                "rust",
                &[&format!("Handler{i:02}")],
            )
        })
        .collect();
    let map = SurfaceMap {
        head_sha: ZERO_SHA.to_string(),
        files,
    };
    let prompt = "handler auth dispatch";
    let r1 = shrink(&map, prompt, 400);
    let r2 = shrink(&map, prompt, 400);
    let j1 = canonical_json(&serde_json::to_value(&r1).unwrap());
    let j2 = canonical_json(&serde_json::to_value(&r2).unwrap());
    assert_eq!(j1, j2, "shrink is non-deterministic");
}
