use crate::modules::identity::domain::entities::{
    RecipientContactRecord, RecipientContactVerification, RecipientContactVerificationClaims,
};
use crate::modules::identity::domain::value_objects::{
    RecipientContactSigningKeyId, RecipientEmailAddress,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, RecipientContactId,
    RecipientContactVerificationId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeginRecipientContactVerificationResult {
    pub contact: RecipientContactRecord,
    pub verification: RecipientContactVerification,
}

#[derive(Debug, Clone)]
pub struct BeginRecipientContactVerificationWrite {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub contact_id: RecipientContactId,
    pub verification_id: RecipientContactVerificationId,
    pub address: RecipientEmailAddress,
    pub signing_key_id: RecipientContactSigningKeyId,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct CompleteRecipientContactVerificationWrite {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub contact_id: RecipientContactId,
    pub claims: RecipientContactVerificationClaims,
    pub completed_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct RevokeRecipientContactWrite {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub contact_id: RecipientContactId,
    pub expected_version: u64,
    pub revoked_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedRecipientContact {
    pub id: RecipientContactId,
    pub principal_id: PrincipalId,
    pub address: RecipientEmailAddress,
    pub aggregate_version: u64,
    pub verified_at: DateTime<Utc>,
}

impl std::fmt::Debug for ResolvedRecipientContact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedRecipientContact")
            .field("id", &self.id)
            .field("principal_id", &self.principal_id)
            .field("address", &"[REDACTED]")
            .field("aggregate_version", &self.aggregate_version)
            .field("verified_at", &self.verified_at)
            .finish()
    }
}

#[async_trait]
pub trait IRecipientContactRepository: Send + Sync {
    async fn begin_recipient_contact_verification(
        &self,
        write: BeginRecipientContactVerificationWrite,
    ) -> Result<IdempotentWrite<BeginRecipientContactVerificationResult>, RepositoryError>;

    async fn find_recipient_contact(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
    ) -> Result<Option<RecipientContactRecord>, RepositoryError>;

    async fn list_recipient_contacts(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Vec<RecipientContactRecord>, RepositoryError>;

    async fn find_recipient_contact_verification(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
        verification_id: RecipientContactVerificationId,
    ) -> Result<Option<RecipientContactVerification>, RepositoryError>;

    async fn complete_recipient_contact_verification(
        &self,
        write: CompleteRecipientContactVerificationWrite,
    ) -> Result<IdempotentWrite<RecipientContactRecord>, RepositoryError>;

    async fn revoke_recipient_contact(
        &self,
        write: RevokeRecipientContactWrite,
    ) -> Result<IdempotentWrite<RecipientContactRecord>, RepositoryError>;

    async fn resolve_verified_recipient_contact(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
    ) -> Result<Option<ResolvedRecipientContact>, RepositoryError>;
}
