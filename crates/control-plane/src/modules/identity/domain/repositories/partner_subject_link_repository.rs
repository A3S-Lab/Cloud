use crate::modules::identity::domain::entities::ExternalIdentityLink;
use crate::modules::identity::domain::value_objects::{
    ExternalIdentitySubject, OidcIssuer, OidcProviderKey,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct LinkPartnerSubjectWrite {
    pub organization_id: OrganizationId,
    pub link: ExternalIdentityLink,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
    pub events: [DomainEventEnvelope; 1],
}

#[derive(Debug, Clone)]
pub struct RevokePartnerSubjectLinkWrite {
    pub organization_id: OrganizationId,
    pub provider_key: OidcProviderKey,
    pub issuer: OidcIssuer,
    pub subject: ExternalIdentitySubject,
    pub expected_version: u64,
    pub revoked_at: DateTime<Utc>,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
    pub events: [DomainEventEnvelope; 1],
}

/// Administrator-managed partner directory ↔ Principal mappings (SubjectLink).
#[async_trait]
pub trait IPartnerSubjectLinkRepository: Send + Sync {
    async fn link_partner_subject(
        &self,
        write: LinkPartnerSubjectWrite,
    ) -> Result<IdempotentWrite<ExternalIdentityLink>, RepositoryError>;

    async fn revoke_partner_subject_link(
        &self,
        write: RevokePartnerSubjectLinkWrite,
    ) -> Result<IdempotentWrite<ExternalIdentityLink>, RepositoryError>;

    async fn find_active_partner_subject_link(
        &self,
        issuer: &OidcIssuer,
        subject: &ExternalIdentitySubject,
    ) -> Result<Option<ExternalIdentityLink>, RepositoryError>;

    async fn list_active_partner_subject_links_for_principal(
        &self,
        principal_id: PrincipalId,
    ) -> Result<Vec<ExternalIdentityLink>, RepositoryError>;
}
