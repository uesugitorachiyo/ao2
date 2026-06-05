use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::sha256_hex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObligationLedger {
    pub schema_version: String,
    pub source_contracts: Vec<ObligationSourceContract>,
    pub obligations: Vec<Obligation>,
    pub summary: ObligationSummary,
    pub verdict: ObligationVerdict,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObligationSourceContract {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obligation {
    pub id: String,
    pub kind: String,
    pub statement: String,
    pub source_path: String,
    pub source_line: usize,
    pub source_excerpt_hash: String,
    pub expected_fragments: Vec<String>,
    pub status: ObligationStatus,
    pub evidence: Vec<ObligationEvidence>,
    pub waiver: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObligationEvidence {
    pub path: String,
    pub line: usize,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationStatus {
    Pass,
    Fail,
    Unverified,
    Waived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationVerdict {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObligationSummary {
    pub pass: usize,
    pub fail: usize,
    pub unverified: usize,
    pub waived: usize,
}

pub fn extract_obligation_ledger(source_path: &str, content: &str) -> ObligationLedger {
    let obligations = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| extract_obligation(source_path, index + 1, line))
        .enumerate()
        .map(|(index, mut obligation)| {
            obligation.id = format!("OBL-{number:03}", number = index + 1);
            obligation
        })
        .collect::<Vec<_>>();

    let mut ledger = ObligationLedger {
        schema_version: "ao2.obligation-ledger.v1".to_string(),
        source_contracts: vec![ObligationSourceContract {
            path: source_path.to_string(),
            sha256: format!("sha256:{}", sha256_hex(content.as_bytes())),
        }],
        obligations,
        summary: ObligationSummary::default(),
        verdict: ObligationVerdict::Rejected,
        created_at: Utc::now(),
    };
    refresh_summary_and_verdict(&mut ledger);
    ledger
}

pub fn check_obligation_ledger(
    ledger: &ObligationLedger,
    target_root: &Path,
) -> io::Result<ObligationLedger> {
    let mut checked = ledger.clone();
    let source_paths = ledger
        .source_contracts
        .iter()
        .map(|contract| contract.path.replace('\\', "/"))
        .collect::<Vec<_>>();
    let searchable_files = searchable_files(target_root, &source_paths)?;
    for obligation in &mut checked.obligations {
        if obligation.waiver.is_some() {
            obligation.status = ObligationStatus::Waived;
            continue;
        }
        if obligation.expected_fragments.is_empty() {
            obligation.status =
                if obligation.evidence.is_empty() {
                    ObligationStatus::Unverified
                } else if obligation.evidence.iter().all(|evidence| {
                    obligation_evidence_points_to_existing_line(target_root, evidence)
                }) {
                    ObligationStatus::Pass
                } else {
                    ObligationStatus::Fail
                };
            continue;
        }

        obligation.evidence.clear();
        let mut evidence = Vec::new();
        for fragment in &obligation.expected_fragments {
            if let Some(found) = find_fragment(target_root, &searchable_files, fragment)? {
                evidence.push(found);
            }
        }

        if evidence.len() == obligation.expected_fragments.len() {
            obligation.status = ObligationStatus::Pass;
            obligation.evidence = evidence;
        } else {
            obligation.status = ObligationStatus::Fail;
        }
    }
    checked.created_at = Utc::now();
    refresh_summary_and_verdict(&mut checked);
    Ok(checked)
}

pub fn annotate_obligation_ledger(
    ledger: &ObligationLedger,
    obligation_id: &str,
    evidence: Option<ObligationEvidence>,
    waiver: Option<String>,
) -> Result<ObligationLedger, String> {
    if evidence.is_none() && waiver.as_deref().unwrap_or("").trim().is_empty() {
        return Err("annotation requires evidence or waiver".to_string());
    }
    let mut annotated = ledger.clone();
    let Some(obligation) = annotated
        .obligations
        .iter_mut()
        .find(|obligation| obligation.id == obligation_id)
    else {
        return Err(format!("unknown obligation id: {obligation_id}"));
    };
    if let Some(waiver) = waiver {
        let waiver = waiver.trim();
        if !waiver.is_empty() {
            obligation.waiver = Some(waiver.to_string());
            obligation.status = ObligationStatus::Waived;
        }
    }
    if let Some(evidence) = evidence {
        obligation.evidence.push(evidence);
        if obligation.waiver.is_none() {
            obligation.status = ObligationStatus::Unverified;
        }
    }
    annotated.created_at = Utc::now();
    refresh_summary_and_verdict(&mut annotated);
    Ok(annotated)
}

pub fn obligation_evidence_points_to_existing_line(
    target_root: &Path,
    evidence: &ObligationEvidence,
) -> bool {
    if evidence.line == 0 {
        return false;
    }
    let evidence_path = Path::new(&evidence.path);
    if evidence_path.is_absolute()
        || evidence_path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return false;
    }
    let path = target_root.join(evidence_path);
    let Ok(content) = fs::read_to_string(&path) else {
        return false;
    };
    content.lines().nth(evidence.line - 1).is_some()
}

fn extract_obligation(source_path: &str, source_line: usize, raw_line: &str) -> Option<Obligation> {
    let statement = clean_statement(raw_line);
    if statement.is_empty() || !is_obligation_statement(&statement) {
        return None;
    }

    Some(Obligation {
        id: String::new(),
        kind: obligation_kind(&statement),
        expected_fragments: expected_fragments(&statement),
        source_excerpt_hash: format!("sha256:{}", sha256_hex(statement.as_bytes())),
        statement,
        source_path: source_path.to_string(),
        source_line,
        status: ObligationStatus::Unverified,
        evidence: Vec::new(),
        waiver: None,
    })
}

fn clean_statement(raw_line: &str) -> String {
    raw_line
        .trim()
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.')
        .trim()
        .to_string()
}

fn is_obligation_statement(statement: &str) -> bool {
    let lower = statement.to_ascii_lowercase();
    lower.contains("must")
        || lower.contains("shall")
        || lower.contains("required")
        || lower.contains("acceptance")
        || lower.contains("rubric")
        || lower.contains("preserve")
        || lower.contains("unchanged")
        || lower.contains("verbatim")
}

fn obligation_kind(statement: &str) -> String {
    let lower = statement.to_ascii_lowercase();
    if lower.contains("must not") || lower.contains("shall not") || lower.contains("forbidden") {
        "must_not".to_string()
    } else if lower.contains("preserve")
        || lower.contains("unchanged")
        || lower.contains("verbatim")
        || lower.contains("exact")
        || lower.contains("equation")
    {
        "content_preservation".to_string()
    } else if lower.contains("acceptance") {
        "acceptance".to_string()
    } else if lower.contains("rubric") {
        "rubric".to_string()
    } else {
        "must".to_string()
    }
}

fn expected_fragments(statement: &str) -> Vec<String> {
    let mut fragments = delimited_fragments(statement, '`');
    fragments.extend(delimited_fragments(statement, '$'));
    fragments.sort();
    fragments.dedup();
    fragments
        .into_iter()
        .filter(|fragment| !fragment.trim().is_empty())
        .collect()
}

fn delimited_fragments(statement: &str, delimiter: char) -> Vec<String> {
    let mut fragments = Vec::new();
    let mut current = String::new();
    let mut in_fragment = false;
    for ch in statement.chars() {
        if ch == delimiter {
            if in_fragment {
                fragments.push(current.trim().to_string());
                current.clear();
                in_fragment = false;
            } else {
                in_fragment = true;
            }
        } else if in_fragment {
            current.push(ch);
        }
    }
    fragments
}

fn searchable_files(root: &Path, source_paths: &[String]) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_searchable_files(root, root, source_paths, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_searchable_files(
    root: &Path,
    path: &Path,
    source_paths: &[String],
    files: &mut Vec<PathBuf>,
) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.is_file() {
        let relative = relative_path(root, path);
        if !source_paths.iter().any(|source| source == &relative)
            && is_searchable_file(path, metadata.len())
        {
            files.push(path.to_path_buf());
        }
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".ao2" || name == ".git" || name == "target" || name == "node_modules" {
            continue;
        }
        if should_exclude_generated_evidence_dir(root, &child) {
            continue;
        }
        collect_searchable_files(root, &child, source_paths, files)?;
    }
    Ok(())
}

fn should_exclude_generated_evidence_dir(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let parts = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    parts.starts_with(&["docs".to_string(), "status".to_string()])
        || parts.starts_with(&["docs".to_string(), "evaluations".to_string()])
}

fn is_searchable_file(path: &Path, len: u64) -> bool {
    if len > 2_000_000 {
        return false;
    }
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("md")
            | Some("txt")
            | Some("rs")
            | Some("toml")
            | Some("json")
            | Some("yaml")
            | Some("yml")
            | Some("py")
            | Some("js")
            | Some("jsx")
            | Some("ts")
            | Some("tsx")
            | Some("html")
            | Some("css")
    )
}

fn find_fragment(
    target_root: &Path,
    files: &[PathBuf],
    fragment: &str,
) -> io::Result<Option<ObligationEvidence>> {
    for path in files {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        for (index, line) in content.lines().enumerate() {
            if line.contains(fragment) {
                return Ok(Some(ObligationEvidence {
                    path: relative_path(target_root, path),
                    line: index + 1,
                    detail: format!("found expected fragment `{fragment}`"),
                }));
            }
        }
    }
    Ok(None)
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn refresh_summary_and_verdict(ledger: &mut ObligationLedger) {
    let mut summary = ObligationSummary::default();
    for obligation in &ledger.obligations {
        match obligation.status {
            ObligationStatus::Pass => summary.pass += 1,
            ObligationStatus::Fail => summary.fail += 1,
            ObligationStatus::Unverified => summary.unverified += 1,
            ObligationStatus::Waived => summary.waived += 1,
        }
    }
    ledger.summary = summary;
    ledger.verdict = if summary.fail == 0 && summary.unverified == 0 {
        ObligationVerdict::Accepted
    } else {
        ObligationVerdict::Rejected
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger_for_fragment(fragment: &str) -> ObligationLedger {
        let mut ledger = ObligationLedger {
            schema_version: "ao2.obligation-ledger.v1".to_string(),
            source_contracts: vec![ObligationSourceContract {
                path: "contract.md".to_string(),
                sha256: "sha256:test".to_string(),
            }],
            obligations: vec![Obligation {
                id: "OBL-test".to_string(),
                kind: "evidence".to_string(),
                statement: "fragment must be present in source, not generated evidence".to_string(),
                source_path: "contract.md".to_string(),
                source_line: 1,
                source_excerpt_hash: "sha256:test".to_string(),
                expected_fragments: vec![fragment.to_string()],
                status: ObligationStatus::Unverified,
                evidence: Vec::new(),
                waiver: None,
            }],
            summary: ObligationSummary::default(),
            verdict: ObligationVerdict::Rejected,
            created_at: Utc::now(),
        };
        refresh_summary_and_verdict(&mut ledger);
        ledger
    }

    #[test]
    fn obligation_check_excludes_generated_evidence_directories() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let fragment = "AO2_GENERATED_ONLY_FRAGMENT";
        fs::write(root.join("contract.md"), "contract source\n").unwrap();
        fs::create_dir_all(root.join("docs/status/nightly")).unwrap();
        fs::write(root.join("docs/status/nightly/evidence.md"), fragment).unwrap();
        fs::create_dir_all(root.join("docs/evaluations/run-1")).unwrap();
        fs::write(root.join("docs/evaluations/run-1/evidence.md"), fragment).unwrap();
        fs::create_dir_all(root.join(".ao2/runs/run-1")).unwrap();
        fs::write(root.join(".ao2/runs/run-1/evidence.md"), fragment).unwrap();

        let checked = check_obligation_ledger(&ledger_for_fragment(fragment), root).unwrap();
        assert_eq!(checked.obligations[0].status, ObligationStatus::Fail);
        assert!(checked.obligations[0].evidence.is_empty());

        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            format!("const MARKER: &str = \"{fragment}\";\n"),
        )
        .unwrap();
        let checked = check_obligation_ledger(&ledger_for_fragment(fragment), root).unwrap();
        assert_eq!(checked.obligations[0].status, ObligationStatus::Pass);
        assert_eq!(checked.obligations[0].evidence[0].path, "src/lib.rs");
    }
}
