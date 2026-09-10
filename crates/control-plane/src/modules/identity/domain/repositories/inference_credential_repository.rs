use crate::modules::identity::domain::entities::{
    InferenceCredential, InferenceCredentialDeliveryReceipt,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, InferenceCredentialId, OrganizationId, ProjectId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[async_trait]
pub trait IInferenceCredentialRepository: Send + Sync {
    async fn create_inference_credential(
        &self,
        credential: InferenceCredential,
    ) -> Result<InferenceCredential, RepositoryError>;

    async fn update_inference_credential(
        &self,
        credential: InferenceCredential,
        expected_aggregate_version: u64,
    ) -> Result<InferenceCredential, RepositoryError>;

    async fn find_inference_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: InferenceCredentialId,
    ) -> Result<Option<InferenceCredential>, RepositoryError>;

    async fn list_inference_credentials_by_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<InferenceCredential>, RepositoryError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceCredentialWriteReference {
    pub credential_id: InferenceCredentialId,
    pub generation: u64,
}

#[derive(Debug, Clone)]
pub struct InferenceCredentialWrite {
    pub credential: InferenceCredential,
    pub receipt: Option<InferenceCredentialDeliveryReceipt>,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct CreateInferenceCredentialWrite {
    pub credential: InferenceCredential,
    pub receipt: InferenceCredentialDeliveryReceipt,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct RotateInferenceCredentialWrite {
    pub credential: InferenceCredential,
    pub receipt: InferenceCredentialDeliveryReceipt,
    pub expected_aggregate_version: u64,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct RevokeInferenceCredentialWrite {
    pub credential: InferenceCredential,
    pub expected_aggregate_version: u64,
    pub idempotency: IdempotencyRequest,
    pub request_id: uuid::Uuid,
    pub event: Option<DomainEventEnvelope>,
}

impl CreateInferenceCredentialWrite {
    pub fn validate(&self) -> Result<(), String> {
        if self.credential.generation() != 1
            || self.credential.aggregate_version() != 1
            || self.credential.created_at() != self.credential.updated_at()
            || self.credential.revoked_at().is_some()
        {
            return Err("new inference credential is not at its initial generation".into());
        }
        self.receipt.validate_against(&self.credential)?;
        validate_event(
            &self.credential,
            &self.event,
            "identity.inference-credential.created",
        )
    }
}

impl RotateInferenceCredentialWrite {
    pub fn validate(&self) -> Result<(), String> {
        if self.expected_aggregate_version == 0
            || self.credential.revoked_at().is_some()
            || self.expected_aggregate_version.checked_add(1)
                != Some(self.credential.aggregate_version())
            || self.credential.generation() <= 1
        {
            return Err("inference credential rotation is invalid".into());
        }
        self.receipt.validate_against(&self.credential)?;
        validate_event(
            &self.credential,
            &self.event,
            "identity.inference-credential.rotated",
        )
    }
}

impl RevokeInferenceCredentialWrite {
    pub fn validate(&self) -> Result<(), String> {
        if self.expected_aggregate_version == 0
            || self.request_id.is_nil()
            || self.credential.revoked_at().is_none()
        {
            return Err("inference credential revocation is invalid".into());
        }
        match &self.event {
            Some(event)
                if self.expected_aggregate_version.checked_add(1)
                    == Some(self.credential.aggregate_version()) =>
            {
                validate_event(
                    &self.credential,
                    event,
                    "identity.inference-credential.revoked",
                )
            }
            None if self.expected_aggregate_version == self.credential.aggregate_version() => {
                Ok(())
            }
            _ => Err("inference credential revocation event version is invalid".into()),
        }
    }
}

fn validate_event(
    credential: &InferenceCredential,
    event: &DomainEventEnvelope,
    event_key: &str,
) -> Result<(), String> {
    if event.event_key != event_key
        || event.aggregate_id != credential.id.as_uuid()
        || event.aggregate_version != credential.aggregate_version()
        || event.occurred_at != credential.updated_at()
        || event.correlation_id.is_nil()
    {
        return Err(format!("{event_key} event does not match its credential"));
    }
    Ok(())
}

#[async_trait]
pub trait IInferenceCredentialLifecycleRepository: IInferenceCredentialRepository {
    async fn replay_inference_credential_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<InferenceCredentialWrite>, RepositoryError>;

    async fn create_inference_credential_delivery(
        &self,
        bundle: CreateInferenceCredentialWrite,
    ) -> Result<InferenceCredentialWrite, RepositoryError>;

    async fn rotate_inference_credential(
        &self,
        bundle: RotateInferenceCredentialWrite,
    ) -> Result<InferenceCredentialWrite, RepositoryError>;

    async fn revoke_inference_credential(
        &self,
        bundle: RevokeInferenceCredentialWrite,
    ) -> Result<InferenceCredentialWrite, RepositoryError>;

    /// Permanently removes expired one-time delivery material while leaving
    /// the credential aggregate and its idempotency record intact.
    async fn sweep_expired_inference_credential_delivery_receipts(
        &self,
        expired_at: chrono::DateTime<chrono::Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError>;
}
