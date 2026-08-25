use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
    SourceRevisionId,
};
use crate::modules::sources::domain::{ExternalSourceRevision, GitProvider, WebhookDeliveryId};
use crate::modules::sources::published::{
    SourceRevisionAcceptedFact, SOURCE_REVISION_ACCEPTED_EVENT_KEY,
    SOURCE_REVISION_ACCEPTED_SCHEMA_VERSION,
};
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

impl AcceptSourceRevision {
    pub fn validate(&self) -> Result<(), String> {
        self.revision.clone().validate()?;
        let event = &self.event;
        if event.event_id.is_nil()
            || event.event_key != SOURCE_REVISION_ACCEPTED_EVENT_KEY
            || event.schema_version != SOURCE_REVISION_ACCEPTED_SCHEMA_VERSION
            || event.organization_id != self.revision.organization_id.as_uuid()
            || event.aggregate_id != self.revision.id.as_uuid()
            || event.aggregate_version != self.revision.aggregate_version
            || event.occurred_at != self.revision.accepted_at
            || event.correlation_id.is_nil()
            || event
                .causation_id
                .is_some_and(|causation_id| causation_id.is_nil())
        {
            return Err("Source revision acceptance event metadata is inconsistent".into());
        }
        let fact: SourceRevisionAcceptedFact = serde_json::from_value(event.payload.clone())
            .map_err(|error| format!("Source revision acceptance fact is invalid: {error}"))?;
        fact.validate()?;
        if fact.organization_id() == self.revision.organization_id
            && fact.project_id() == self.revision.project_id
            && fact.environment_id() == self.revision.environment_id
            && fact.source_revision_id() == self.revision.id
            && fact.repository_identity() == self.revision.repository.identity()
            && fact.commit_sha() == self.revision.commit_sha.as_str()
            && fact.recipe_digest() == self.revision.recipe_digest.as_str()
        {
            Ok(())
        } else {
            Err("Source revision acceptance fact and aggregate differ".into())
        }
    }
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
