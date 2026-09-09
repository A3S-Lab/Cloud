use crate::modules::identity::application::InferenceCredentialMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{InferenceCredentialId, OrganizationId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Identity-owned revoke for an environment inference key (`RevokeInferenceKey`).
#[derive(Debug, Clone)]
pub struct RevokeInferenceKey {
    pub organization_id: OrganizationId,
    pub credential_id: InferenceCredentialId,
    pub expected_aggregate_version: u64,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for RevokeInferenceKey {
    type Output = ApplicationResult<InferenceCredentialMutationResult>;
}
