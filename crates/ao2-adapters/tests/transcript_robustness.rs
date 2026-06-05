//! Robustness coverage for the untrusted provider-transcript parser.
//!
//! `parse_provider_transcript` ingests **provider stdout** — text the control
//! plane does not control — and turns it into the structured summary that
//! feeds the evidence pack. The existing tests pin happy paths and specific
//! edges; this suite covers the *input space* instead: across thousands of
//! deterministically-generated pathological transcripts plus a curated
//! adversarial corpus, the parser must
//!
//!   1. never panic (the load-bearing property for an untrusted-input parser),
//!   2. emit a deduplicated, sorted `changed_files` list with no empty entries,
//!   3. keep structured output bounded by the input (no runaway duplication),
//!   4. be deterministic (same bytes in → identical summary out),
//!   5. preserve every sandbox-observed file.
//!
//! The Codex provider routes into a separate crate and is intentionally out of
//! scope here; this exercises Claude, Antigravity, and Scripted through the
//! public dispatcher, which needs no extra dependency.

use ao2_adapters::{parse_provider_transcript, ProviderKind, ProviderTranscriptSummary};

/// The non-Codex providers, all reached through the public dispatcher.
const PROVIDERS: &[ProviderKind] = &[
    ProviderKind::Claude,
    ProviderKind::Antigravity,
    ProviderKind::Scripted,
];

/// Deterministic xorshift64*-style PRNG. No external dependency and no
/// `Math.random`/clock use, so any failure is exactly reproducible from the
/// seed printed in the panic message.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len())]
    }
}

/// Label heads, value-ish junk, JSON-ish blobs, and nasty unicode — the raw
/// material the generator splices into lines.
const FRAGMENTS: &[&str] = &[
    "Summary:",
    "Summary: ",
    "Changed files:",
    "changed_files=",
    "files changed:",
    "Concern:",
    "Concern: high - ",
    "Concern: none",
    "Blocker:",
    "Blocker: none",
    "Modified:",
    "Added:",
    "Deleted:",
    "Input tokens:",
    "Output tokens:",
    "Total tokens:",
    "tokens:",
    "Cost:",
    "Cost: $",
    "session_id:",
    "thread id:",
    "stdout:",
    "stderr:",
    "a",
    "src/main.rs",
    "a,b,c,d",
    "  - x.rs  ",
    "`quoted.rs`",
    "dir\\win.rs",
    "1,200",
    "$0.04",
    "-1",
    "99999999999999999999999999",
    "NaN",
    "🎉🎉🎉",
    "—café",
    "\t\t",
    "<<EOF",
    "grep -q x",
    "$AO2_REPAIR_TOK",
    "{}",
    "{\"usage\":{\"input_tokens\":5}}",
    "{\"session_id\":\"s\"}",
    "{\"usage\":{",
    "{not json",
    "[]",
    "null",
    "Bearer super-secret-token",
    "../../etc/passwd",
];

const SEPARATORS: &[&str] = &[" ", " - ", ",", ";", " <<", ": ", "\t"];
const LINE_ENDINGS: &[&str] = &["\n", "\r\n"];

fn build_input(rng: &mut Rng) -> String {
    let mut out = String::new();
    if rng.below(4) == 0 {
        out.push_str("preamble\nstdout:\n");
    }
    let lines = 1 + rng.below(12);
    for _ in 0..lines {
        let parts = 1 + rng.below(4);
        for p in 0..parts {
            if p > 0 {
                out.push_str(rng.pick(SEPARATORS));
            }
            out.push_str(rng.pick(FRAGMENTS));
        }
        if rng.below(20) == 0 {
            out.push_str(&"x".repeat(2000));
        }
        out.push_str(rng.pick(LINE_ENDINGS));
    }
    if rng.below(5) == 0 {
        out.push_str("\nstderr:\nSummary: from stderr\nChanged files: leaked.rs\n");
    }
    out
}

fn build_sandbox_files(rng: &mut Rng) -> Vec<String> {
    let n = rng.below(4);
    (0..n)
        .map(|i| format!("sandbox/file_{}.rs", rng.below(5) + i))
        .collect()
}

/// All invariants that must hold for any input. `seed` is echoed so a failure
/// reproduces deterministically.
fn assert_invariants(
    summary: &ProviderTranscriptSummary,
    transcript: &str,
    sandbox: &[String],
    seed: u64,
) {
    // (2) changed_files: strictly ascending => sorted AND duplicate-free.
    for w in summary.changed_files.windows(2) {
        assert!(
            w[0] < w[1],
            "seed {seed}: changed_files not sorted/unique: {:?}",
            summary.changed_files
        );
    }
    // (2) no empty path tokens survive.
    assert!(
        summary.changed_files.iter().all(|f| !f.is_empty()),
        "seed {seed}: empty changed_files entry"
    );
    // (3) each parsed line yields at most one concern/blocker, so both are
    // bounded by the line count (parse body is a substring => <= transcript).
    let line_budget = transcript.lines().count() + 2;
    assert!(
        summary.concerns.len() <= line_budget,
        "seed {seed}: {} concerns over budget {line_budget}",
        summary.concerns.len()
    );
    assert!(
        summary.blockers.len() <= line_budget,
        "seed {seed}: {} blockers over budget {line_budget}",
        summary.blockers.len()
    );
    // (3) transcript ids carry non-empty kind and value (merge skips blanks).
    for id in &summary.transcript_ids {
        assert!(
            !id.kind.is_empty() && !id.value.is_empty(),
            "seed {seed}: blank transcript id"
        );
    }
    // (3) a parsed summary can only be a slice of the input.
    if let Some(s) = &summary.raw_summary {
        assert!(
            s.len() <= transcript.len(),
            "seed {seed}: raw_summary too long"
        );
    }
    // (5) every sandbox-observed file is preserved.
    for f in sandbox {
        assert!(
            summary.changed_files.iter().any(|c| c == f),
            "seed {seed}: sandbox file {f:?} dropped"
        );
    }
}

#[test]
fn parser_survives_thousands_of_pathological_inputs() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..3000 {
        let seed = rng.0;
        let transcript = build_input(&mut rng);
        let sandbox = build_sandbox_files(&mut rng);

        for &provider in PROVIDERS {
            let summary = parse_provider_transcript(provider, &transcript, &sandbox);
            assert_invariants(&summary, &transcript, &sandbox, seed);

            // (4) determinism: identical bytes parse to an identical summary.
            let again = parse_provider_transcript(provider, &transcript, &sandbox);
            assert_eq!(summary, again, "seed {seed}: parse is not deterministic");
        }
    }
}

#[test]
fn parser_survives_a_curated_adversarial_corpus() {
    let huge_summary = format!("Summary: {}", "x".repeat(100_000));
    let many_files = format!("Changed files: {}", "a,".repeat(5000));
    let corpus: Vec<String> = vec![
        String::new(),
        " ".into(),
        "\n\n\n".into(),
        "\r\n\r\n".into(),
        "Summary:".into(),       // label, no value
        "Changed files:".into(), // label, no value
        "Modified:".into(),      // label, no file
        "Concern:".into(),       // label, no value
        "Blocker:".into(),
        huge_summary,
        many_files,
        "Concern: ".to_string() + &" - ".repeat(100),
        "stdout:\n".into(),
        "x\nstdout:\n\nstderr:\n".into(),
        "{\"usage\":{\"input_tokens\":99999999999999999999999999}}".into(),
        "Cost: $$$,,,...".into(),
        "Total tokens: NaN".into(),
        "[1,2,3]".into(),
        "Summary: 🎉—café\u{0}embedded".into(),
        "Changed files: a\\b\\c, ../../x, `q`, '', \"\"".into(),
        "Bearer secret-token-123".into(),
        "session_id:\nthread id: ".into(),
        "if [ -f x ]; then printf 'y'; fi".into(),
        "\u{202e}rtl-override".into(),
    ];

    let sandbox = vec!["sandbox/a.rs".to_string(), "sandbox/b.rs".to_string()];
    for (i, transcript) in corpus.iter().enumerate() {
        for &provider in PROVIDERS {
            let summary = parse_provider_transcript(provider, transcript, &sandbox);
            // Reuse the full invariant set; seed slot carries the corpus index.
            assert_invariants(&summary, transcript, &sandbox, i as u64);
        }
    }
}
