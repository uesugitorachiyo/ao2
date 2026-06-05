use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod obligations;

pub use obligations::{
    annotate_obligation_ledger, check_obligation_ledger, extract_obligation_ledger,
    obligation_evidence_points_to_existing_line, Obligation, ObligationEvidence, ObligationLedger,
    ObligationSourceContract, ObligationStatus, ObligationSummary, ObligationVerdict,
};

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4())
}

pub fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes.as_ref());
    format!("{:x}", hasher.finalize())
}

static ATOMIC_WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Durably write `bytes` to `path` via a uniquely-named temp file, fsync, then
/// an atomic rename onto `path`.
///
/// A crash, power loss, or full disk during the write can never truncate or
/// corrupt an existing `path`: a reader sees either the old complete file or the
/// new complete file, never a partial one. This is the write discipline the AO2
/// evidence boundary depends on, mirroring the control-plane `write_tmp_then_rename`.
///
/// The temp name carries the pid plus a process-unique counter so concurrent
/// writers (in-process or cross-process) cannot collide on the temp path, and an
/// orphaned temp from a crashed writer never clobbers a healthy one. On any error
/// the temp file is cleaned up so failed writes leave no litter beside the target.
pub fn atomic_write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    let path = path.as_ref();
    let bytes = bytes.as_ref();
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = parent.unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("atomic");
    let counter = ATOMIC_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{file_name}.tmp.{}.{counter}", std::process::id()));

    let write = || -> io::Result<()> {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        // Flush the file's data+size metadata to disk before the rename, so the
        // renamed entry cannot point at a not-yet-durable, zero-length file.
        file.sync_all()?;
        Ok(())
    };
    if let Err(err) = write() {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Created,
    Compiled,
    Queued,
    Running,
    WaitingForApproval,
    Blocked,
    Failed,
    Rejected,
    Accepted,
    AcceptedWithConcerns,
    Canceled,
    Replaying,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub id: String,
    pub kind: String,
}

impl Actor {
    pub fn system() -> Self {
        Self {
            id: "system:ao2".to_string(),
            kind: "system".to_string(),
        }
    }

    pub fn human_local() -> Self {
        Self {
            id: "human:local-user".to_string(),
            kind: "human".to_string(),
        }
    }

    pub fn role(role: &str) -> Self {
        Self {
            id: format!("role:{role}"),
            kind: "agent_role".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AoEvent {
    pub event_id: String,
    pub event_type: String,
    pub run_id: String,
    pub workflow_id: String,
    pub role_id: Option<String>,
    pub task_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub actor: Actor,
    pub causation_id: Option<String>,
    pub correlation_id: String,
    pub trace_id: String,
    pub span_id: String,
    pub payload: serde_json::Value,
    pub payload_digest: String,
    pub schema_version: String,
    pub sensitivity: String,
}

impl AoEvent {
    pub fn new(
        run_id: &str,
        workflow_id: &str,
        event_type: &str,
        role_id: Option<&str>,
        task_id: Option<&str>,
        actor: Actor,
        payload: serde_json::Value,
    ) -> Self {
        let payload_bytes = serde_json::to_vec(&payload).unwrap_or_default();
        Self {
            event_id: new_id("evt"),
            event_type: event_type.to_string(),
            run_id: run_id.to_string(),
            workflow_id: workflow_id.to_string(),
            role_id: role_id.map(str::to_string),
            task_id: task_id.map(str::to_string),
            timestamp: Utc::now(),
            actor,
            causation_id: None,
            correlation_id: run_id.to_string(),
            trace_id: sha256_hex(format!("trace:{run_id}"))[..32].to_string(),
            span_id: sha256_hex(format!("span:{run_id}:{event_type}:{}", Utc::now()))[..16]
                .to_string(),
            payload,
            payload_digest: sha256_hex(payload_bytes),
            schema_version: "ao2.event.v1".to_string(),
            sensitivity: "internal".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub artifact_type: String,
    pub uri: String,
    pub media_type: String,
    pub digest: String,
    pub producer: String,
    pub input_refs: Vec<String>,
    pub sensitivity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub decision_id: String,
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub request_digest: String,
    pub decision: String,
    pub reason: String,
    pub policy_version: String,
    pub approval_ticket_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalTicket {
    pub ticket_id: String,
    pub run_id: String,
    pub requested_action: String,
    pub action_digest: String,
    pub risk_class: String,
    pub requester: String,
    pub approver: Option<String>,
    pub status: String,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosureReport {
    pub verdict: String,
    pub acceptance_criteria_results: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub unresolved_concerns: Vec<String>,
    pub blockers: Vec<String>,
    pub policy_exceptions: Vec<String>,
    pub cost_summary: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod atomic_write_tests {
    use super::atomic_write;
    use std::fs;

    #[test]
    fn writes_new_file_with_exact_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run-record.json");
        atomic_write(&path, b"{\"status\":\"accepted\"}").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\"status\":\"accepted\"}"
        );
    }

    #[test]
    fn overwrites_existing_file_completely() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("evidence.json");
        atomic_write(&path, "first-and-longer-content").unwrap();
        atomic_write(&path, "second").unwrap();
        // No leftover bytes from the longer first write.
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("approval.json");
        atomic_write(&path, "approved").unwrap();
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "approval.json")
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left behind: {leftovers:?}"
        );
    }
}
