use crate::modules::identity::application::InferenceCredentialDeliveryResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InferenceCredentialId, OrganizationId, ProjectId,
};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Identity-owned rotate for an environment inference key (`RotateInferenceKey`).
#[derive(Debug, Clone)]
pub struct RotateInferenceKey {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub credential_id: InferenceCredentialId,
    pub expected_aggregate_version: u64,
    pub expires_at: DateTime<Utc>,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for RotateInferenceKey {
    type Output = ApplicationResult<InferenceCredentialDeliveryResult>;
}
