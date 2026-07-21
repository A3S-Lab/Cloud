use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
    SourceConnectionId, SourceRevisionId, SourceSubscriptionId,
};
use crate::modules::sources::domain::{
    AcceptSourceRevision, AcceptSourceWebhook, CreateGithubRepositorySubscription,
    DeactivateGithubRepositorySubscription, ExternalSourceRevision, GithubRepositorySubscription,
    ISourceRevisionRepository, ISourceSubscriptionRepository, ISourceWebhookRepository,
    NewExternalSourceRevision, SourceRevisionAccepted, SourceWebhookAcceptance,
    SourceWebhookDelivery,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemorySourceRevisionRepository {
    state: RwLock<State>,
}

#[derive(Clone, Default)]
struct State {
    revisions: BTreeMap<(OrganizationId, SourceRevisionId), ExternalSourceRevision>,
    natural_ids: BTreeMap<NaturalKey, SourceRevisionId>,
    subscriptions: BTreeMap<(OrganizationId, SourceSubscriptionId), GithubRepositorySubscription>,
    subscription_natural_ids: BTreeMap<SubscriptionNaturalKey, SourceSubscriptionId>,
    subscription_idempotency: BTreeMap<(String, String), (String, GithubRepositorySubscription)>,
    webhook_deliveries: BTreeMap<DeliveryKey, String>,
    webhook_inbox: BTreeMap<(String, String), SourceWebhookDelivery>,
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
type SubscriptionNaturalKey = (
    OrganizationId,
    ProjectId,
    EnvironmentId,
    SourceConnectionId,
    String,
    String,
    String,
);

impl InMemorySourceRevisionRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }

    pub async fn webhook_inbox(&self) -> Vec<SourceWebhookDelivery> {
        self.state
            .read()
            .await
            .webhook_inbox
            .values()
            .cloned()
            .collect()
    }
}

#[async_trait]
impl ISourceSubscriptionRepository for InMemorySourceRevisionRepository {
    async fn create(
        &self,
        request: CreateGithubRepositorySubscription,
    ) -> Result<IdempotentWrite<GithubRepositorySubscription>, RepositoryError> {
        let mut state = self.state.write().await;
        let idempotency_key = owned_idempotency_key(&request.idempotency);
        if let Some((digest, subscription)) = state.subscription_idempotency.get(&idempotency_key) {
            if digest != &request.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: subscription.clone(),
                replayed: true,
            });
        }
        let natural_key = subscription_natural_key(&request.subscription);
        if let Some(existing_id) = state.subscription_natural_ids.get(&natural_key).copied() {
            let existing = state
                .subscriptions
                .get(&(request.subscription.organization_id, existing_id))
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "subscription natural identity points to a missing subscription".into(),
                    )
                })?;
            state.subscription_idempotency.insert(
                idempotency_key,
                (request.idempotency.request_digest, existing.clone()),
            );
            return Ok(IdempotentWrite {
                value: existing,
                replayed: true,
            });
        }
        state
            .subscription_natural_ids
            .insert(natural_key, request.subscription.id);
        state.subscriptions.insert(
            (
                request.subscription.organization_id,
                request.subscription.id,
            ),
            request.subscription.clone(),
        );
        state.subscription_idempotency.insert(
            idempotency_key,
            (
                request.idempotency.request_digest,
                request.subscription.clone(),
            ),
        );
        state.outbox.push(request.event);
        Ok(IdempotentWrite {
            value: request.subscription,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        subscription_id: SourceSubscriptionId,
    ) -> Result<Option<GithubRepositorySubscription>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .subscriptions
            .get(&(organization_id, subscription_id))
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<GithubRepositorySubscription>, RepositoryError> {
        let mut subscriptions = self
            .state
            .read()
            .await
            .subscriptions
            .values()
            .filter(|subscription| {
                subscription.organization_id == organization_id
                    && subscription.project_id == project_id
                    && subscription.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        subscriptions.sort_by_key(|subscription| (subscription.created_at, subscription.id));
        Ok(subscriptions)
    }

    async fn deactivate(
        &self,
        request: DeactivateGithubRepositorySubscription,
    ) -> Result<IdempotentWrite<GithubRepositorySubscription>, RepositoryError> {
        let mut state = self.state.write().await;
        let idempotency_key = owned_idempotency_key(&request.idempotency);
        if let Some((digest, subscription)) = state.subscription_idempotency.get(&idempotency_key) {
            if digest != &request.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: subscription.clone(),
                replayed: true,
            });
        }
        let key = (
            request.subscription.organization_id,
            request.subscription.id,
        );
        let existing = state
            .subscriptions
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if existing == request.subscription {
            state.subscription_idempotency.insert(
                idempotency_key,
                (request.idempotency.request_digest, existing.clone()),
            );
            return Ok(IdempotentWrite {
                value: existing,
                replayed: true,
            });
        }
        if existing.aggregate_version != request.previous_version
            || existing.organization_id != request.subscription.organization_id
            || existing.project_id != request.subscription.project_id
            || existing.environment_id != request.subscription.environment_id
            || existing.connection_id != request.subscription.connection_id
            || existing.installation_id != request.subscription.installation_id
            || existing.repository != request.subscription.repository
            || existing.branch != request.subscription.branch
            || existing.recipe != request.subscription.recipe
        {
            return Err(RepositoryError::Conflict(
                "GitHub repository subscription changed concurrently".into(),
            ));
        }
        state
            .subscription_natural_ids
            .remove(&subscription_natural_key(&existing));
        state
            .subscriptions
            .insert(key, request.subscription.clone());
        state.subscription_idempotency.insert(
            idempotency_key,
            (
                request.idempotency.request_digest,
                request.subscription.clone(),
            ),
        );
        state.outbox.push(request.event);
        Ok(IdempotentWrite {
            value: request.subscription,
            replayed: false,
        })
    }
}

#[async_trait]
impl ISourceWebhookRepository for InMemorySourceRevisionRepository {
    async fn accept_delivery(
        &self,
        request: AcceptSourceWebhook,
    ) -> Result<SourceWebhookAcceptance, RepositoryError> {
        let mut state = self.state.write().await;
        let delivery = request.delivery;
        let key = (
            delivery.provider.as_str().to_owned(),
            delivery.delivery_id.as_str().to_owned(),
        );
        if let Some(existing) = state.webhook_inbox.get(&key) {
            if !existing.same_payload_as(&delivery) {
                return Err(RepositoryError::Conflict(
                    "webhook delivery ID was reused with another payload".into(),
                ));
            }
            return Ok(SourceWebhookAcceptance {
                delivery: existing.clone(),
                replayed: true,
                revisions: Vec::new(),
            });
        }
        let mut next = state.clone();
        next.webhook_inbox.insert(key, delivery.clone());
        let mut matching = next
            .subscriptions
            .values()
            .filter(|subscription| {
                subscription.is_active()
                    && Some(subscription.connection_id) == request.authoritative_connection_id
                    && subscription.installation_id == delivery.installation_id
                    && subscription.repository == delivery.repository
                    && subscription.branch_name() == delivery.reference.value()
            })
            .cloned()
            .collect::<Vec<_>>();
        matching.sort_by_key(|subscription| (subscription.organization_id, subscription.id));
        let mut revisions = Vec::with_capacity(matching.len());
        for subscription in matching {
            let delivery_key = (
                subscription.organization_id,
                delivery.provider.as_str().to_owned(),
                delivery.delivery_id.as_str().to_owned(),
            );
            let source_identity_digest = delivery
                .repository
                .source_identity_digest(&delivery.commit_sha);
            if let Some(existing_digest) = next.webhook_deliveries.get(&delivery_key) {
                if existing_digest != &source_identity_digest {
                    return Err(RepositoryError::Conflict(
                        "webhook delivery ID was reused for another source identity".into(),
                    ));
                }
            } else {
                next.webhook_deliveries
                    .insert(delivery_key, source_identity_digest);
            }
            let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
                organization_id: subscription.organization_id,
                project_id: subscription.project_id,
                environment_id: subscription.environment_id,
                id: SourceRevisionId::new(),
                repository: delivery.repository.clone(),
                commit_sha: delivery.commit_sha.clone(),
                recipe: subscription.recipe.clone(),
                accepted_at: delivery.received_at,
            })
            .map_err(|error| {
                RepositoryError::Storage(format!(
                    "could not create source revision from subscription: {error}"
                ))
            })?;
            let revision_natural_key = natural_key(&revision);
            if let Some(existing_id) = next.natural_ids.get(&revision_natural_key).copied() {
                let existing = next
                    .revisions
                    .get(&(revision.organization_id, existing_id))
                    .cloned()
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "source revision natural identity points to a missing revision".into(),
                        )
                    })?;
                revisions.push(existing);
                continue;
            }
            let event = SourceRevisionAccepted::envelope(&revision, request.correlation_id)
                .map_err(|error| RepositoryError::Storage(error.to_string()))?;
            next.natural_ids.insert(revision_natural_key, revision.id);
            next.revisions
                .insert((revision.organization_id, revision.id), revision.clone());
            next.outbox.push(event);
            revisions.push(revision);
        }
        *state = next;
        Ok(SourceWebhookAcceptance {
            delivery,
            replayed: false,
            revisions,
        })
    }
}

#[async_trait]
impl ISourceRevisionRepository for InMemorySourceRevisionRepository {
    async fn find(
        &self,
        organization_id: OrganizationId,
        source_revision_id: crate::modules::shared_kernel::domain::SourceRevisionId,
    ) -> Result<ExternalSourceRevision, RepositoryError> {
        self.state
            .read()
            .await
            .revisions
            .get(&(organization_id, source_revision_id))
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

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

fn subscription_natural_key(subscription: &GithubRepositorySubscription) -> SubscriptionNaturalKey {
    (
        subscription.organization_id,
        subscription.project_id,
        subscription.environment_id,
        subscription.connection_id,
        subscription.repository.identity().to_owned(),
        subscription.branch_name().to_owned(),
        subscription.recipe_digest.clone(),
    )
}

fn owned_idempotency_key(idempotency: &IdempotencyRequest) -> (String, String) {
    (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    )
}
