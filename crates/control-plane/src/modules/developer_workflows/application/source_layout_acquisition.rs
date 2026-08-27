use crate::modules::developer_workflows::domain::SourceLayoutSnapshot;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
};
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildPlanSourceLayoutRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_revision_id: SourceRevisionId,
}

impl BuildPlanSourceLayoutRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.source_revision_id.as_uuid().is_nil()
        {
            return Err("BuildPlan source-layout request identity is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildPlanSourceLayoutError {
    #[error("BuildPlan source-layout request is invalid: {0}")]
    Invalid(String),
    #[error("BuildPlan source-layout identity conflicts with Sources authority")]
    Conflict,
    #[error("BuildPlan source layout is unavailable: {0}")]
    Unavailable(String),
    #[error("BuildPlan source layout failed integrity validation: {0}")]
    Integrity(String),
    #[error("BuildPlan source-layout storage failed: {0}")]
    Storage(String),
}

/// Developer Workflows-owned port for one exact, trusted Sources layout.
///
/// Repository connections, credentials, checkout receipts, local paths, and
/// cleanup remain behind the Sources implementation. `None` means the exact
/// accepted Source revision does not exist in the requested scope.
#[async_trait]
pub trait IBuildPlanSourceLayoutPort: Send + Sync {
    async fn acquire(
        &self,
        request: BuildPlanSourceLayoutRequest,
    ) -> Result<Option<SourceLayoutSnapshot>, BuildPlanSourceLayoutError>;
}
