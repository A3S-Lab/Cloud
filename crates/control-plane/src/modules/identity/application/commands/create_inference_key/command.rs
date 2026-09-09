use crate::modules::identity::application::InferenceCredentialDeliveryResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Identity-owned create for an environment inference key (`CreateInferenceKey`).
#[derive(Debug, Clone)]
pub struct CreateInferenceKey {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub expires_at: DateTime<Utc>,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CreateInferenceKey {
    type Output = ApplicationResult<InferenceCredentialDeliveryResult>;
}
