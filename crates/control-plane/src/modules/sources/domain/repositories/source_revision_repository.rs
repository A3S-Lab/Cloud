use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
    SourceRevisionId,
};
use crate::modules::sources::domain::{ExternalSourceRevision, GitProvider, WebhookDeliveryId};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct WebhookDeliveryReservation {
    pub organization_id: OrganizationId,
    pub provider: GitProvider,
    pub delivery_id: WebhookDeliveryId,
    pub source_identity_digest: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AcceptSourceRevision {
    pub revision: ExternalSourceRevision,
    pub webhook_delivery: Option<WebhookDeliveryReservation>,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[async_trait]
pub trait ISourceRevisionRepository: Send + Sync {
    async fn find(
        &self,
        organization_id: OrganizationId,
        source_revision_id: SourceRevisionId,
    ) -> Result<ExternalSourceRevision, RepositoryError>;

    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ExternalSourceRevision>, RepositoryError>;

    async fn accept(
        &self,
        request: AcceptSourceRevision,
    ) -> Result<IdempotentWrite<ExternalSourceRevision>, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<ExternalSourceRevision>, RepositoryError>;
}
