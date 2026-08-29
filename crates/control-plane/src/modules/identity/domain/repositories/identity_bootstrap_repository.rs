use crate::modules::identity::domain::entities::IdentityBootstrap;
use crate::modules::identity::domain::value_objects::ApiTokenDigest;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, InstallationId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BootstrapIdentityWrite {
    pub bootstrap: IdentityBootstrap,
    pub token_digest: ApiTokenDigest,
    pub identity_events: [DomainEventEnvelope; 4],
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IIdentityBootstrapRepository: Send + Sync {
    /// Returns the immutable identity of the Cloud installation being bootstrapped.
    async fn installation_id(&self) -> Result<InstallationId, RepositoryError>;

    /// Atomically creates tenant identity and its Installation authorization root.
    async fn bootstrap_identity(
        &self,
        write: BootstrapIdentityWrite,
    ) -> Result<IdempotentWrite<IdentityBootstrap>, RepositoryError>;
}
