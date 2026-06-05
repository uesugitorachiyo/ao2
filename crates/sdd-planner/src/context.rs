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
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::OnceLock;
use tiktoken_rs::{cl100k_base, CoreBPE};

/// Hard cap from README §11 D7 — fits codex/claude context with room
/// for the prompt itself and the candidate-plan output.
pub const DEFAULT_BUDGET_TOKENS: usize = 8000;

/// Basenames that are pinned into the result whenever present.
pub const FORCED_BASENAMES: &[&str] = &["Cargo.toml", "package.json", "README.md"];

fn bpe() -> &'static CoreBPE {
    static BPE: OnceLock<CoreBPE> = OnceLock::new();
    BPE.get_or_init(|| cl100k_base().expect("cl100k_base init"))
}

/// Count cl100k_base tokens in `text`.
pub fn count_tokens(text: &str) -> usize {
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

    // Partition: forced first (unconditional), candidates ranked next.
    let mut forced: Vec<SurfaceFile> = Vec::new();
    let mut scored: Vec<(f64, SurfaceFile)> = Vec::new();
    for f in &map.files {
        if is_forced(&f.path) {
            forced.push(f.clone());
        } else {
            let score = jaccard(&prompt_tokens, &file_tokens(f));
            scored.push((score, f.clone()));
        }
    }

    // Score desc, path asc (stable, deterministic).
    scored.sort_by(
        |a, b| match b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal) {
            Ordering::Equal => a.1.path.cmp(&b.1.path),
            ord => ord,
        },
    );

    // Greedy fill: forced always in; candidates added while under budget.
    let mut chosen: Vec<SurfaceFile> = forced;
    for (_, f) in scored {
        chosen.push(f);
        let candidate = SurfaceMap {
            files: sorted_by_path(&chosen),
            head_sha: map.head_sha.clone(),
        };
        let json = canonical_json(&serde_json::to_value(&candidate).expect("serialize"));
        if count_tokens(&json) > budget {
            chosen.pop();
            break;
        }
    }

    SurfaceMap {
        files: sorted_by_path(&chosen),
        head_sha: map.head_sha.clone(),
    }
}

fn sorted_by_path(files: &[SurfaceFile]) -> Vec<SurfaceFile> {
    let mut out = files.to_vec();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}
