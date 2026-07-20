use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
    SourceRevisionId,
};
use crate::modules::sources::domain::{
    AcceptSourceRevision, ExternalSourceRevision, ISourceRevisionRepository,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemorySourceRevisionRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    revisions: BTreeMap<(OrganizationId, SourceRevisionId), ExternalSourceRevision>,
    natural_ids: BTreeMap<NaturalKey, SourceRevisionId>,
    webhook_deliveries: BTreeMap<DeliveryKey, String>,
    idempotency: BTreeMap<(String, String), (String, ExternalSourceRevision)>,
    outbox: Vec<DomainEventEnvelope>,
}

type NaturalKey = (
    OrganizationId,
    ProjectId,
    EnvironmentId,
    String,
    String,
    String,
);
type DeliveryKey = (OrganizationId, String, String);

impl InMemorySourceRevisionRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl ISourceRevisionRepository for InMemorySourceRevisionRepository {
    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ExternalSourceRevision>, RepositoryError> {
        let state = self.state.read().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        let Some((digest, revision)) = state.idempotency.get(&key) else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        Ok(Some(revision.clone()))
    }

    async fn accept(
        &self,
        request: AcceptSourceRevision,
    ) -> Result<IdempotentWrite<ExternalSourceRevision>, RepositoryError> {
        let mut state = self.state.write().await;
        let idempotency_key = (
            request.idempotency.storage_key().0.to_owned(),
            request.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, revision)) = state.idempotency.get(&idempotency_key) {
            if digest != &request.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: revision.clone(),
                replayed: true,
            });
        }
        if let Some(delivery) = &request.webhook_delivery {
            let key = (
                delivery.organization_id,
                delivery.provider.as_str().to_owned(),
                delivery.delivery_id.as_str().to_owned(),
            );
            if let Some(existing_digest) = state.webhook_deliveries.get(&key) {
                if existing_digest != &delivery.source_identity_digest {
                    return Err(RepositoryError::Conflict(
                        "webhook delivery ID was reused for another source identity".into(),
                    ));
                }
            } else {
                state
                    .webhook_deliveries
                    .insert(key, delivery.source_identity_digest.clone());
            }
        }
        let natural_key = natural_key(&request.revision);
        if let Some(existing_id) = state.natural_ids.get(&natural_key).copied() {
            let existing = state
                .revisions
                .get(&(request.revision.organization_id, existing_id))
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "source revision natural identity points to a missing revision".into(),
                    )
                })?;
            state.idempotency.insert(
                idempotency_key,
                (request.idempotency.request_digest, existing.clone()),
            );
            return Ok(IdempotentWrite {
                value: existing,
                replayed: true,
            });
        }
        state.natural_ids.insert(natural_key, request.revision.id);
        state.revisions.insert(
            (request.revision.organization_id, request.revision.id),
            request.revision.clone(),
        );
        state.idempotency.insert(
            idempotency_key,
            (request.idempotency.request_digest, request.revision.clone()),
        );
        state.outbox.push(request.event);
        Ok(IdempotentWrite {
            value: request.revision,
            replayed: false,
        })
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<ExternalSourceRevision>, RepositoryError> {
        let mut revisions = self
            .state
            .read()
            .await
            .revisions
            .values()
            .filter(|revision| {
                revision.organization_id == organization_id
                    && revision.project_id == project_id
                    && revision.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        revisions.sort_by_key(|revision| (revision.accepted_at, revision.id));
        Ok(revisions)
    }
}

fn natural_key(revision: &ExternalSourceRevision) -> NaturalKey {
    (
        revision.organization_id,
        revision.project_id,
        revision.environment_id,
        revision.repository.identity().to_owned(),
        revision.commit_sha.as_str().to_owned(),
        revision.recipe_digest.clone(),
    )
}
