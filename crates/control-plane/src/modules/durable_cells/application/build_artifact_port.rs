use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{BuildRunId, EnvironmentId, OrganizationId, ProjectId};
use async_trait::async_trait;
use serde::Serialize;

/// Complete identity supplied by Durable Cells when it consumes one
/// published BuildRun output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableCellBuildArtifactRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_run_id: BuildRunId,
}

impl DurableCellBuildArtifactRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
        {
            return Err("Durable Cell BuildRun identity is invalid".into());
        }
        Ok(())
    }
}

/// Aggregate-free, immutable BuildRun output admitted by Durable Cells.
///
/// Artifacts remains the authority for BuildRun lifecycle, provenance, and
/// persistence. Durable Cells receives only this bounded artifact projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellBuildArtifact {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_run_id: BuildRunId,
    pub build_run_version: u64,
    pub uri: String,
    pub digest: String,
    pub media_type: String,
    pub size_bytes: u64,
}

impl DurableCellBuildArtifact {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.build_run_id.as_uuid().is_nil()
            || self.build_run_version == 0
        {
            return Err("Durable Cell BuildRun output identity is invalid".into());
        }
        if self.uri.len() > 4096
            || self.uri.contains(['\0', '\r', '\n'])
            || !self.uri.contains("://")
            || self.media_type.trim().is_empty()
            || self.media_type.len() > 255
            || self.media_type.contains(['\0', '\r', '\n'])
            || self.size_bytes == 0
        {
            return Err("Durable Cell BuildRun output has invalid bounds".into());
        }
        let Some(digest) = self.digest.strip_prefix("sha256:") else {
            return Err("Durable Cell BuildRun output digest is not SHA-256".into());
        };
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("Durable Cell BuildRun output digest is not SHA-256".into());
        }
        Ok(())
    }
}

/// Durable Cells' sole application boundary for consuming a successful,
/// typed BuildRun output. The owner adapter performs all aggregate loading
/// and lifecycle interpretation.
#[async_trait]
pub trait IDurableCellBuildArtifactPort: Send + Sync {
    async fn find_published_bundle(
        &self,
        request: &DurableCellBuildArtifactRequest,
    ) -> ApplicationResult<DurableCellBuildArtifact>;
}
