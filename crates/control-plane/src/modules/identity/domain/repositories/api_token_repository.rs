use crate::modules::identity::domain::entities::{ApiToken, IdentityBootstrap};
use crate::modules::identity::domain::value_objects::ApiTokenDigest;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, IdempotentWrite, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait IApiTokenRepository: Send + Sync {
    async fn bootstrap(
        &self,
        bootstrap: IdentityBootstrap,
        digest: ApiTokenDigest,
        events: [DomainEventEnvelope; 2],
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<IdentityBootstrap>, RepositoryError>;

    async fn create(
        &self,
        token: ApiToken,
        digest: ApiTokenDigest,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        token_id: ApiTokenId,
    ) -> Result<Option<ApiToken>, RepositoryError>;

    async fn authenticate(
        &self,
        digest: &ApiTokenDigest,
        now: DateTime<Utc>,
    ) -> Result<Option<ApiToken>, RepositoryError>;

    async fn revoke(
        &self,
        token: ApiToken,
        event: Option<DomainEventEnvelope>,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError>;
}
