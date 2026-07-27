use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ao2_core::{atomic_write, new_id, sha256_hex, ArtifactRef};

pub mod hosted_release_contract;

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn put_text(
        &self,
        artifact_type: &str,
        producer: &str,
        file_name: &str,
        media_type: &str,
        content: &str,
        input_refs: Vec<String>,
    ) -> Result<ArtifactRef> {
        let artifact_id = new_id("art");
        let dir = self.root.join(&artifact_id);
        fs::create_dir_all(&dir)
            .with_context(|| format!("create artifact dir {}", dir.display()))?;
        let content_path = dir.join(file_name);
        atomic_write(&content_path, content)
            .with_context(|| format!("write artifact content {}", content_path.display()))?;
        let digest = sha256_hex(content.as_bytes());
        let artifact = ArtifactRef {
            artifact_id,
            artifact_type: artifact_type.to_string(),
            uri: content_path.to_string_lossy().to_string(),
            media_type: media_type.to_string(),
            digest,
            producer: producer.to_string(),
            input_refs,
            sensitivity: "internal".to_string(),
        };
        let manifest_path = dir.join("artifact.json");
        atomic_write(&manifest_path, serde_json::to_string_pretty(&artifact)?)
            .with_context(|| format!("write artifact manifest {}", manifest_path.display()))?;
        Ok(artifact)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
