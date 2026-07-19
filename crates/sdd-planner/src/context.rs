//! Context shrinker — phase P2.
//!
//! `shrink(surface_map, prompt, budget)` returns a [`SurfaceMap`] sized
//! to fit `budget` cl100k_base tokens of canonical JSON, per README §10
//! P2 acceptance:
//!
//! 1. ≤ `budget` cl100k_base tokens.
//! 2. Force-includes `Cargo.toml`, `package.json`, `README.md` (basename
//!    match) when they appear in the input map — these override budget.
//! 3. Remaining files ranked by jaccard(prompt_tokens, path_tokens ∪
//!    public_symbols) descending, with path ASC as the tie-breaker.
//! 4. Deterministic: identical input → byte-equal canonical JSON.
//!
//! The output map's `head_sha` and `files[].sha256` are copied verbatim
//! from the input; the scanner is the only producer of those values.

use crate::schema::{SurfaceFile, SurfaceMap};
use crate::surface::canonical_json;
#[cfg(test)]
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::OnceLock;
use tiktoken_rs::{cl100k_base, CoreBPE};

/// Hard cap from README §11 D7 — fits codex/claude context with room
/// for the prompt itself and the candidate-plan output.
pub const DEFAULT_BUDGET_TOKENS: usize = 8000;

/// Basenames that are pinned into the result whenever present.
pub const FORCED_BASENAMES: &[&str] = &["Cargo.toml", "package.json", "README.md"];

#[cfg(test)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextShrinkMetrics {
    pub file_clones: usize,
    pub path_sort_calls: usize,
    pub serialized_candidate_maps: usize,
    pub serialized_candidate_bytes: usize,
    pub tokenization_calls: usize,
    pub tokenized_bytes: usize,
}

#[cfg(test)]
thread_local! {
    static CONTEXT_SHRINK_METRICS: RefCell<ContextShrinkMetrics> =
        RefCell::new(ContextShrinkMetrics::default());
}

#[cfg(test)]
pub fn reset_context_shrink_metrics() {
    CONTEXT_SHRINK_METRICS.with(|metrics| {
        *metrics.borrow_mut() = ContextShrinkMetrics::default();
    });
}

#[cfg(test)]
pub fn context_shrink_metrics() -> ContextShrinkMetrics {
    CONTEXT_SHRINK_METRICS.with(|metrics| metrics.borrow().clone())
}

#[cfg(test)]
fn record_file_clone(count: usize) {
    CONTEXT_SHRINK_METRICS.with(|metrics| {
        metrics.borrow_mut().file_clones += count;
    });
}

#[cfg(not(test))]
fn record_file_clone(_count: usize) {}

#[cfg(test)]
fn record_path_sort(count: usize) {
    CONTEXT_SHRINK_METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        metrics.path_sort_calls += 1;
        metrics.file_clones += count;
    });
}

#[cfg(not(test))]
fn record_path_sort(_count: usize) {}

#[cfg(test)]
fn record_candidate_serialization(bytes: usize) {
    CONTEXT_SHRINK_METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        metrics.serialized_candidate_maps += 1;
        metrics.serialized_candidate_bytes += bytes;
    });
}

#[cfg(not(test))]
fn record_candidate_serialization(_bytes: usize) {}

#[cfg(test)]
fn record_tokenization(bytes: usize) {
    CONTEXT_SHRINK_METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        metrics.tokenization_calls += 1;
        metrics.tokenized_bytes += bytes;
    });
}

#[cfg(not(test))]
fn record_tokenization(_bytes: usize) {}

fn bpe() -> &'static CoreBPE {
    static BPE: OnceLock<CoreBPE> = OnceLock::new();
    BPE.get_or_init(|| cl100k_base().expect("cl100k_base init"))
}

/// Count cl100k_base tokens in `text`.
pub fn count_tokens(text: &str) -> usize {
    record_tokenization(text.len());
    bpe().encode_with_special_tokens(text).len()
}

/// Tokenize a string into a lowercased, deduped set of alphanumeric
/// chunks. Used for prompt tokens and path tokens — same recipe both
/// sides, so jaccard is symmetric.
fn word_tokens(s: &str) -> HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

fn file_tokens(f: &SurfaceFile) -> HashSet<String> {
    let mut tokens = word_tokens(&f.path);
    for sym in &f.public_symbols {
        tokens.insert(sym.to_ascii_lowercase());
    }
    tokens
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.len() + b.len() - intersection;
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

fn is_forced(path: &str) -> bool {
    FORCED_BASENAMES.contains(&basename(path))
}

/// Trim `map` down to ≤ `budget` cl100k_base tokens of canonical JSON
/// while honouring forced inclusions and jaccard ranking.
pub fn shrink(map: &SurfaceMap, prompt: &str, budget: usize) -> SurfaceMap {
    let prompt_tokens = word_tokens(prompt);

    #[derive(Clone, Copy)]
    struct PathCandidate<'a> {
        file: &'a SurfaceFile,
        rank: Option<usize>,
    }

    // Partition: forced files are unconditional; candidates are ranked once.
    let mut forced: Vec<&SurfaceFile> = Vec::new();
    let mut scored: Vec<(f64, &SurfaceFile)> = Vec::new();
    for f in &map.files {
        if is_forced(&f.path) {
            forced.push(f);
        } else {
            let score = jaccard(&prompt_tokens, &file_tokens(f));
            scored.push((score, f));
        }
    }

    // Score desc, path asc (stable, deterministic).
    scored.sort_by(
        |a, b| match b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal) {
            Ordering::Equal => a.1.path.cmp(&b.1.path),
            ord => ord,
        },
    );

    let mut path_ordered: Vec<PathCandidate<'_>> = forced
        .iter()
        .map(|file| PathCandidate { file, rank: None })
        .collect();
    path_ordered.extend(
        scored
            .iter()
            .enumerate()
            .map(|(rank, (_, file))| PathCandidate {
                file,
                rank: Some(rank),
            }),
    );
    record_path_sort(path_ordered.len());
    path_ordered.sort_by(|a, b| a.file.path.cmp(&b.file.path));

    let candidate_for_prefix = |prefix_len: usize| -> SurfaceMap {
        let mut files = Vec::new();
        for candidate in &path_ordered {
            if candidate.rank.is_none_or(|rank| rank < prefix_len) {
                record_file_clone(1);
                files.push(candidate.file.clone());
            }
        }
        SurfaceMap {
            files,
            head_sha: map.head_sha.clone(),
        }
    };

    let candidate_token_count = |candidate: &SurfaceMap| -> usize {
        let json = canonical_json(&serde_json::to_value(candidate).expect("serialize"));
        record_candidate_serialization(json.len());
        count_tokens(&json)
    };

    // Forced files still override budget. Otherwise candidate prefixes are
    // monotonic in serialized size, so a bounded binary search finds the same
    // prefix the greedy loop selected while avoiding repeated full scans.
    let forced_only = candidate_for_prefix(0);
    if candidate_token_count(&forced_only) > budget {
        return forced_only;
    }

    let mut low = 0usize;
    let mut high = scored.len();
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        let candidate = candidate_for_prefix(mid);
        if candidate_token_count(&candidate) <= budget {
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    candidate_for_prefix(low)
}

#[cfg(test)]
mod tests {
    use super::{
        context_shrink_metrics, count_tokens, file_tokens, is_forced, jaccard,
        reset_context_shrink_metrics, shrink, word_tokens, ContextShrinkMetrics,
    };
    use crate::schema::{SurfaceFile, SurfaceMap};
    use crate::surface::canonical_json;
    use std::cmp::Ordering;
    use std::time::Instant;

    const ZERO_SHA: &str = "0000000000000000000000000000000000000000";

    fn fixture_file(path: String, symbols: Vec<String>) -> SurfaceFile {
        SurfaceFile {
            kind: "rust".to_string(),
            path,
            public_symbols: symbols,
            sha256: "0123456789abcdef".repeat(4),
        }
    }

    fn fixture_map(file_count: usize) -> SurfaceMap {
        let mut files = vec![
            SurfaceFile {
                kind: "toml".to_string(),
                path: "Cargo.toml".to_string(),
                public_symbols: vec![],
                sha256: "0123456789abcdef".repeat(4),
            },
            SurfaceFile {
                kind: "markdown".to_string(),
                path: "README.md".to_string(),
                public_symbols: vec![],
                sha256: "0123456789abcdef".repeat(4),
            },
        ];
        files.extend((0..file_count).map(|i| {
            fixture_file(
                format!("src/context/module_{i:04}.rs"),
                vec![format!("ContextSymbol{i:04}")],
            )
        }));
        SurfaceMap {
            files,
            head_sha: ZERO_SHA.to_string(),
        }
    }

    fn json_tokens(map: &SurfaceMap) -> usize {
        count_tokens(&canonical_json(
            &serde_json::to_value(map).expect("serialize"),
        ))
    }

    fn reference_shrink(map: &SurfaceMap, prompt: &str, budget: usize) -> SurfaceMap {
        let prompt_tokens = word_tokens(prompt);
        let mut forced: Vec<SurfaceFile> = Vec::new();
        let mut scored: Vec<(f64, SurfaceFile)> = Vec::new();
        for f in &map.files {
            if is_forced(&f.path) {
                forced.push(f.clone());
            } else {
                scored.push((jaccard(&prompt_tokens, &file_tokens(f)), f.clone()));
            }
        }
        scored.sort_by(
            |a, b| match b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal) {
                Ordering::Equal => a.1.path.cmp(&b.1.path),
                ord => ord,
            },
        );

        let mut chosen = forced;
        for (_, file) in scored {
            chosen.push(file);
            let candidate = SurfaceMap {
                files: reference_sorted_by_path(&chosen),
                head_sha: map.head_sha.clone(),
            };
            if json_tokens(&candidate) > budget {
                chosen.pop();
                break;
            }
        }

        SurfaceMap {
            files: reference_sorted_by_path(&chosen),
            head_sha: map.head_sha.clone(),
        }
    }

    fn reference_sorted_by_path(files: &[SurfaceFile]) -> Vec<SurfaceFile> {
        let mut out = files.to_vec();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    fn measurement(file_count: usize, budget: usize) -> (usize, usize, ContextShrinkMetrics) {
        let map = fixture_map(file_count);
        reset_context_shrink_metrics();
        let start = Instant::now();
        let shrunk = shrink(
            &map,
            "context shrink planner serialization token budget module",
            budget,
        );
        let elapsed = start.elapsed().as_micros() as usize;
        let metrics = context_shrink_metrics();
        println!(
            "context_shrink_baseline algorithm=rank_prefix_binary_search files={} selected={} budget={} elapsed_us={} clones={} path_sorts={} serializations={} serialized_bytes={} tokenizations={} tokenized_bytes={}",
            file_count,
            shrunk.files.len(),
            budget,
            elapsed,
            metrics.file_clones,
            metrics.path_sort_calls,
            metrics.serialized_candidate_maps,
            metrics.serialized_candidate_bytes,
            metrics.tokenization_calls,
            metrics.tokenized_bytes,
        );
        (shrunk.files.len(), elapsed, metrics)
    }

    #[test]
    fn context_shrink_preserves_determinism_and_budget_at_600_file_scale() {
        let map = fixture_map(600);
        let budget = 4000;
        let first = shrink(
            &map,
            "context shrink planner serialization token budget module",
            budget,
        );
        let second = shrink(
            &map,
            "context shrink planner serialization token budget module",
            budget,
        );
        assert_eq!(
            canonical_json(&serde_json::to_value(&first).expect("serialize")),
            canonical_json(&serde_json::to_value(&second).expect("serialize")),
            "600-file shrink output must be byte-identical across runs"
        );
        assert!(
            json_tokens(&first) <= budget,
            "600-file shrink output exceeded token budget"
        );
    }

    #[test]
    fn context_shrink_matches_greedy_reference_at_budget_boundaries() {
        let map = fixture_map(100);
        let prompt = "context shrink planner serialization token budget module";
        for budget in [1, 500, 2_000, 8_000, 100_000] {
            let optimized = shrink(&map, prompt, budget);
            let reference = reference_shrink(&map, prompt, budget);
            assert_eq!(
                canonical_json(&serde_json::to_value(&optimized).expect("serialize")),
                canonical_json(&serde_json::to_value(&reference).expect("serialize")),
                "optimized shrink diverged from greedy reference at budget {budget}"
            );
        }
    }

    #[test]
    fn context_shrink_avoids_repeated_candidate_resort_serialization_and_tokenization() {
        let (_selected, _elapsed, metrics) = measurement(600, 8_000);
        assert!(
            metrics.path_sort_calls <= 2,
            "path sorting should be reused, got {} sort calls",
            metrics.path_sort_calls
        );
        assert!(
            metrics.serialized_candidate_maps <= 20,
            "candidate serialization should use prefix accounting or search, got {} serializations",
            metrics.serialized_candidate_maps
        );
        assert!(
            metrics.tokenization_calls <= 20,
            "BPE tokenization should use prefix accounting or search, got {} calls",
            metrics.tokenization_calls
        );
    }

    #[test]
    #[ignore]
    fn context_shrink_records_baseline_scaling() {
        for (files, budget) in [(100, 2_000), (100, 8_000), (600, 2_000), (600, 8_000)] {
            let (_selected, _elapsed, metrics) = measurement(files, budget);
            assert!(metrics.file_clones >= files);
            assert!(metrics.serialized_candidate_maps > 0);
            assert!(metrics.tokenization_calls > 0);
        }
    }
}
