use crate::modules::identity::domain::entities::{ApiToken, AuthenticatedApiToken};
use crate::modules::identity::domain::value_objects::ApiTokenDigest;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct CreateApiTokenWrite {
    pub token: ApiToken,
    pub digest: ApiTokenDigest,
    pub event: DomainEventEnvelope,
    pub issuer_principal_id: PrincipalId,
    pub issuer_is_platform_admin: bool,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IApiTokenRepository: Send + Sync {
    async fn create(
        &self,
        write: CreateApiTokenWrite,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        token_id: ApiTokenId,
    ) -> Result<Option<ApiToken>, RepositoryError>;

    async fn list(&self, organization_id: OrganizationId)
        -> Result<Vec<ApiToken>, RepositoryError>;

    async fn authenticate(
        &self,
        digest: &ApiTokenDigest,
        now: DateTime<Utc>,
    ) -> Result<Option<AuthenticatedApiToken>, RepositoryError>;

    async fn revoke(
        &self,
        token: ApiToken,
        event: Option<DomainEventEnvelope>,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError>;
}
