use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SANDBOX_PATCH_APPROVAL_SUBJECT_SCHEMA: &str = "ao2.sandbox-patch-approval-subject.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxPatchApprovalSubject {
    pub schema_version: String,
    pub repository_identity: String,
    pub base_commit: String,
    pub operation_type: String,
    pub operations: Vec<SandboxPatchOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxPatchOperation {
    pub order: u32,
    pub path: String,
    pub kind: SandboxPatchOperationKind,
    pub before: Option<SandboxFileState>,
    pub after: Option<SandboxFileState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPatchOperationKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxFileState {
    pub kind: SandboxFileKind,
    pub content_sha256: Option<String>,
    pub symlink_target_sha256: Option<String>,
    pub unix_mode: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxFileKind {
    RegularFile,
    Symlink,
}

impl SandboxPatchApprovalSubject {
    pub fn action_digest(&self) -> Result<String> {
        let bytes = serde_json::to_vec(self)?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Ok(format!("{:x}", hasher.finalize()))
    }
}
