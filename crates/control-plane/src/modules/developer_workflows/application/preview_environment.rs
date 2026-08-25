use crate::modules::developer_workflows::domain::PullRequestPreview;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, IdempotentWrite, OrganizationId, ProjectId,
    PullRequestPreviewId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// Consumer-owned Environment identity requested from Projects. It repeats no
/// Projects aggregate or repository model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEnvironmentBinding {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub preview_id: PullRequestPreviewId,
    pub environment_id: EnvironmentId,
    pub pull_request_number: u64,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl PreviewEnvironmentBinding {
    pub fn validate(&self) -> Result<(), String> {
        let expected_name =
            PullRequestPreview::environment_name_for(self.preview_id, self.pull_request_number)?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.preview_id.as_uuid().is_nil()
            || self.environment_id != PullRequestPreview::environment_id_for(self.preview_id)
            || self.name != expected_name
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err("Preview Environment binding is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsurePreviewEnvironment {
    pub binding: PreviewEnvironmentBinding,
    pub correlation_id: Uuid,
    pub causation_id: Uuid,
}

impl EnsurePreviewEnvironment {
    pub fn validate(&self) -> Result<(), String> {
        self.binding.validate()?;
        if self.correlation_id.is_nil() || self.causation_id.is_nil() {
            return Err("Preview Environment handoff causality is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewEnvironmentReceipt {
    pub binding: PreviewEnvironmentBinding,
    pub environment_aggregate_version: u64,
}

impl PreviewEnvironmentReceipt {
    pub fn validate_for(&self, expected: &PreviewEnvironmentBinding) -> Result<(), String> {
        self.binding.validate()?;
        if self.binding != *expected || self.environment_aggregate_version != 1 {
            return Err("Projects changed the exact Preview Environment handoff".into());
        }
        Ok(())
    }
}

/// Developer Workflows-owned port into the Projects Environment authority.
/// Projects remains the only aggregate, name-uniqueness, idempotency, Outbox,
/// and persistence owner.
#[async_trait]
pub trait IPreviewEnvironmentPort: Send + Sync {
    async fn ensure_preview_environment(
        &self,
        request: EnsurePreviewEnvironment,
    ) -> Result<IdempotentWrite<PreviewEnvironmentReceipt>, RepositoryError>;
}
