use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId,
    PullRequestPreviewId, RepositoryError, SourceConnectionId, SourceRevisionId,
    SourceSubscriptionId,
};
use crate::modules::sources::application::{
    lifecycle_event, IPreviewSourceRevisionProjectionPort, PreviewSourceRevisionProjectionOutcome,
    PreviewSourceRevisionProjectionReceipt, ProjectPreviewSourceRevision,
};
use crate::modules::sources::domain::{
    AcceptSourceRevision, AcceptSourceWebhook, CreateGithubRepositorySubscription,
    DeactivateGithubRepositorySubscription, ExternalSourceRevision, GithubRepositorySubscription,
    ISourceRevisionRepository, ISourceSubscriptionRepository, ISourceWebhookRepository,
    NewExternalSourceRevision, PullRequestChangeCommitted, SourceRevisionAccepted,
    SourceWebhookAcceptance, SourceWebhookDelivery, SourceWebhookPayload,
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
    preview_source_revision_receipts: BTreeMap<
        (OrganizationId, PullRequestPreviewId, u64),
        PreviewSourceRevisionProjectionReceipt,
    >,
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
                pull_request_changes: Vec::new(),
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
                    && subscription.branch_name() == delivery.payload.branch().value()
            })
            .cloned()
            .collect::<Vec<_>>();
        matching.sort_by_key(|subscription| (subscription.organization_id, subscription.id));
        let mut revisions = Vec::with_capacity(matching.len());
        let mut pull_request_changes = Vec::with_capacity(matching.len());
        for subscription in matching {
            match &delivery.payload {
                SourceWebhookPayload::Push(push) => {
                    let delivery_key = (
                        subscription.organization_id,
                        delivery.provider.as_str().to_owned(),
                        delivery.delivery_id.as_str().to_owned(),
                    );
                    let source_identity_digest =
                        delivery.repository.source_identity_digest(&push.commit_sha);
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
                        commit_sha: push.commit_sha.clone(),
                        recipe: subscription.recipe.clone(),
                        accepted_at: delivery.received_at,
                    })
                    .map_err(|error| {
                        RepositoryError::Storage(format!(
                            "could not create source revision from subscription: {error}"
                        ))
                    })?;
                    let revision_natural_key = natural_key(&revision);
                    if let Some(existing_id) = next.natural_ids.get(&revision_natural_key).copied()
                    {
                        let existing = next
                            .revisions
                            .get(&(revision.organization_id, existing_id))
                            .cloned()
                            .ok_or_else(|| {
                                RepositoryError::Storage(
                                    "source revision natural identity points to a missing revision"
                                        .into(),
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
                SourceWebhookPayload::PullRequest(_) => {
                    let fact = PullRequestChangeCommitted::fact(&subscription, &delivery)
                        .map_err(RepositoryError::Storage)?;
                    let event = PullRequestChangeCommitted::envelope(
                        &fact,
                        delivery.received_at,
                        request.correlation_id,
                    )
                    .map_err(|error| RepositoryError::Storage(error.to_string()))?;
                    next.outbox.push(event);
                    pull_request_changes.push(fact);
                }
            }
        }
        *state = next;
        Ok(SourceWebhookAcceptance {
            delivery,
            replayed: false,
            revisions,
            pull_request_changes,
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
        request.validate().map_err(|error| {
            RepositoryError::Storage(format!("invalid Source revision acceptance: {error}"))
        })?;
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

#[async_trait]
impl IPreviewSourceRevisionProjectionPort for InMemorySourceRevisionRepository {
    async fn project_preview_source_revision(
        &self,
        input: ProjectPreviewSourceRevision,
    ) -> Result<IdempotentWrite<PreviewSourceRevisionProjectionReceipt>, RepositoryError> {
        input.validate().map_err(|error| {
            RepositoryError::Storage(format!(
                "invalid Preview Source revision projection: {error}"
            ))
        })?;
        let mut state = self.state.write().await;
        let receipt_key = (
            input.organization_id,
            input.preview_id,
            input.preview_aggregate_version,
        );
        if let Some(existing) = state.preview_source_revision_receipts.get(&receipt_key) {
            if !existing.matches_input(&input) {
                return Err(RepositoryError::Conflict(
                    "Preview aggregate version changed lifecycle fact or Sources binding".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }

        let latest = state
            .preview_source_revision_receipts
            .iter()
            .filter(|((organization_id, preview_id, _), _)| {
                *organization_id == input.organization_id && *preview_id == input.preview_id
            })
            .max_by_key(|((_, _, version), _)| *version)
            .map(|(_, receipt)| receipt.clone());
        if latest
            .as_ref()
            .is_some_and(|receipt| !receipt.has_same_scope_as(&input))
        {
            return Err(RepositoryError::Conflict(
                "Preview lifecycle changed its Sources projection scope".into(),
            ));
        }
        if latest.as_ref().is_some_and(|receipt| {
            receipt.preview_aggregate_version > input.preview_aggregate_version
        }) {
            let receipt = PreviewSourceRevisionProjectionReceipt::from_input(
                &input,
                PreviewSourceRevisionProjectionOutcome::IgnoredStale,
                None,
            )
            .map_err(RepositoryError::Storage)?;
            state
                .preview_source_revision_receipts
                .insert(receipt_key, receipt.clone());
            return Ok(IdempotentWrite {
                value: receipt,
                replayed: false,
            });
        }

        let subscription = state
            .subscriptions
            .get(&(input.organization_id, input.source_subscription_id))
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let decision = input.decide(&subscription).map_err(|error| {
            RepositoryError::Conflict(format!(
                "Preview Source projection authority is invalid: {error}"
            ))
        })?;
        let mut next = state.clone();
        let (outcome, revision) = match decision.revision {
            Some(candidate) => {
                let key = natural_key(&candidate);
                let revision = if let Some(existing_id) = next.natural_ids.get(&key).copied() {
                    next.revisions
                        .get(&(input.organization_id, existing_id))
                        .cloned()
                        .ok_or_else(|| {
                            RepositoryError::Storage(
                                "Preview Source natural identity points to a missing revision"
                                    .into(),
                            )
                        })?
                } else {
                    next.natural_ids.insert(key, candidate.id);
                    next.revisions
                        .insert((input.organization_id, candidate.id), candidate.clone());
                    candidate
                };
                (decision.outcome, Some(revision))
            }
            None => (decision.outcome, None),
        };
        let receipt = PreviewSourceRevisionProjectionReceipt::from_input(
            &input,
            outcome,
            revision.as_ref().map(|value| value.id),
        )
        .map_err(RepositoryError::Storage)?;
        let event =
            lifecycle_event(&receipt, revision.as_ref()).map_err(RepositoryError::Storage)?;
        next.preview_source_revision_receipts
            .insert(receipt_key, receipt.clone());
        next.outbox.push(event);
        *state = next;
        Ok(IdempotentWrite {
            value: receipt,
            replayed: false,
        })
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

#[cfg(test)]
mod preview_projection_tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, GitCommitSha, PullRequestPreviewId, Sha256Digest,
        SourcePullRequestChangeId,
    };
    use crate::modules::sources::application::{
        IPreviewSourceRevisionProjectionPort, PreviewSourceRevisionDesiredState,
        PreviewSourceRevisionProjectionOutcome, ProjectPreviewSourceRevision,
    };
    use crate::modules::sources::domain::{
        BuildRecipe, GitProvider, GitReference, GitRepository, GithubInstallationId,
        GithubRepositorySubscription, NewGithubRepositorySubscription,
    };
    use crate::modules::sources::published::{
        PreviewSourceRevisionLifecycleCommittedFact, PreviewSourceRevisionLifecycleState,
        PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY,
    };
    use chrono::{Duration, TimeZone, Utc};
    use uuid::Uuid;

    #[tokio::test]
    async fn active_versions_replay_exactly_and_reuse_one_ordinary_source_revision() {
        let repository = InMemorySourceRevisionRepository::new();
        let subscription = subscription(false);
        seed(&repository, subscription.clone()).await;
        let preview_id = PullRequestPreviewId::new();
        let preview_environment_id = EnvironmentId::new();
        let first = input(
            &subscription,
            preview_id,
            preview_environment_id,
            1,
            PreviewSourceRevisionDesiredState::Active,
        );

        let accepted = repository
            .project_preview_source_revision(first.clone())
            .await
            .expect("active Preview projection");
        assert!(!accepted.replayed);
        assert_eq!(
            accepted.value.outcome,
            PreviewSourceRevisionProjectionOutcome::Projected
        );
        let revision_id = accepted.value.source_revision_id.expect("revision ID");

        let replay = repository
            .project_preview_source_revision(first.clone())
            .await
            .expect("exact replay");
        assert!(replay.replayed);
        assert_eq!(replay.value, accepted.value);
        assert_eq!(repository.outbox_events().await.len(), 1);

        let mut drifted = first.clone();
        drifted.fact_digest = Sha256Digest::from_bytes(b"drifted");
        assert!(matches!(
            repository.project_preview_source_revision(drifted).await,
            Err(RepositoryError::Conflict(_))
        ));

        let mut causality_drift = first;
        causality_drift.correlation_id = Uuid::now_v7();
        assert!(matches!(
            repository
                .project_preview_source_revision(causality_drift)
                .await,
            Err(RepositoryError::Conflict(_))
        ));

        let second = input(
            &subscription,
            preview_id,
            preview_environment_id,
            2,
            PreviewSourceRevisionDesiredState::Active,
        );
        let advanced = repository
            .project_preview_source_revision(second)
            .await
            .expect("advanced Preview projection");
        assert_eq!(advanced.value.source_revision_id, Some(revision_id));
        let revisions = ISourceRevisionRepository::list(
            &repository,
            subscription.organization_id,
            subscription.project_id,
            preview_environment_id,
        )
        .await
        .expect("Preview revisions");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].id, revision_id);

        let events = repository.outbox_events().await;
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|event| {
            event.event_key == PREVIEW_SOURCE_REVISION_LIFECYCLE_COMMITTED_EVENT_KEY
                && event.aggregate_id == preview_id.as_uuid()
        }));
        assert!(events.iter().all(|event| {
            event.event_key
                != crate::modules::sources::published::SOURCE_REVISION_ACCEPTED_EVENT_KEY
        }));
        for event in events {
            let fact: PreviewSourceRevisionLifecycleCommittedFact =
                serde_json::from_value(event.payload).expect("Preview Source lifecycle fact");
            fact.validate().expect("valid lifecycle fact");
            assert_eq!(fact.state(), PreviewSourceRevisionLifecycleState::Active);
            assert_eq!(fact.source_revision_id(), Some(revision_id));
            assert_eq!(
                fact.source_revision_accepted_at(),
                Some(revisions[0].accepted_at),
                "later Preview versions must retain the ordinary SourceRevision creation time"
            );
        }
    }

    #[tokio::test]
    async fn newer_cleanup_fences_late_active_versions_without_a_ghost_revision() {
        let repository = InMemorySourceRevisionRepository::new();
        let subscription = subscription(false);
        seed(&repository, subscription.clone()).await;
        let preview_id = PullRequestPreviewId::new();
        let preview_environment_id = EnvironmentId::new();
        let cleanup = input(
            &subscription,
            preview_id,
            preview_environment_id,
            3,
            PreviewSourceRevisionDesiredState::CleanupRequired,
        );
        let cleaned = repository
            .project_preview_source_revision(cleanup)
            .await
            .expect("cleanup projection");
        assert_eq!(
            cleaned.value.outcome,
            PreviewSourceRevisionProjectionOutcome::CleanupRequired
        );
        assert!(cleaned.value.source_revision_id.is_none());

        for version in [1, 2] {
            let late_active = input(
                &subscription,
                preview_id,
                preview_environment_id,
                version,
                PreviewSourceRevisionDesiredState::Active,
            );
            let ignored = repository
                .project_preview_source_revision(late_active)
                .await
                .expect("stale active projection");
            assert_eq!(
                ignored.value.outcome,
                PreviewSourceRevisionProjectionOutcome::IgnoredStale
            );
        }
        assert!(ISourceRevisionRepository::list(
            &repository,
            subscription.organization_id,
            subscription.project_id,
            preview_environment_id,
        )
        .await
        .expect("Preview revisions")
        .is_empty());
        assert_eq!(repository.outbox_events().await.len(), 1);
    }

    #[tokio::test]
    async fn inactive_subscription_is_a_terminal_suppression_not_a_retry_loop() {
        let repository = InMemorySourceRevisionRepository::new();
        let subscription = subscription(true);
        seed(&repository, subscription.clone()).await;
        let preview_id = PullRequestPreviewId::new();
        let preview_environment_id = EnvironmentId::new();
        let active = input(
            &subscription,
            preview_id,
            preview_environment_id,
            1,
            PreviewSourceRevisionDesiredState::Active,
        );
        let suppressed = repository
            .project_preview_source_revision(active)
            .await
            .expect("inactive subscription suppression");
        assert_eq!(
            suppressed.value.outcome,
            PreviewSourceRevisionProjectionOutcome::SuppressedInactiveSubscription
        );
        assert!(suppressed.value.source_revision_id.is_none());
        let events = repository.outbox_events().await;
        assert_eq!(events.len(), 1);
        let fact: PreviewSourceRevisionLifecycleCommittedFact =
            serde_json::from_value(events[0].payload.clone()).expect("lifecycle fact");
        fact.validate().expect("valid lifecycle fact");
        assert_eq!(
            fact.state(),
            PreviewSourceRevisionLifecycleState::SuppressedInactiveSubscription
        );
    }

    async fn seed(
        repository: &InMemorySourceRevisionRepository,
        subscription: GithubRepositorySubscription,
    ) {
        repository.state.write().await.subscriptions.insert(
            (subscription.organization_id, subscription.id),
            subscription,
        );
    }

    fn subscription(inactive: bool) -> GithubRepositorySubscription {
        let created_at = Utc
            .with_ymd_and_hms(2026, 8, 26, 4, 30, 0)
            .single()
            .expect("timestamp");
        let mut subscription =
            GithubRepositorySubscription::subscribe(NewGithubRepositorySubscription {
                id: SourceSubscriptionId::new(),
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                connection_id: SourceConnectionId::new(),
                installation_id: GithubInstallationId::parse(42).expect("installation"),
                repository: GitRepository::parse(
                    GitProvider::Github,
                    "https://github.com/a3s-lab/cloud",
                )
                .expect("repository"),
                branch: GitReference::parse("branch", "main").expect("branch"),
                recipe: BuildRecipe::dockerfile(
                    BuildRecipe::SCHEMA,
                    BuildRecipe::DOCKERFILE_KIND,
                    ".",
                    "Dockerfile",
                    None,
                    vec!["linux/amd64".into()],
                )
                .expect("recipe"),
                created_at,
            })
            .expect("subscription");
        if inactive {
            subscription
                .deactivate(created_at + Duration::seconds(1))
                .expect("deactivation");
        }
        subscription
    }

    fn input(
        subscription: &GithubRepositorySubscription,
        preview_id: PullRequestPreviewId,
        preview_environment_id: EnvironmentId,
        version: u64,
        desired_state: PreviewSourceRevisionDesiredState,
    ) -> ProjectPreviewSourceRevision {
        let occurred_at = Utc
            .with_ymd_and_hms(2026, 8, 26, 5, 0, version as u32)
            .single()
            .expect("timestamp");
        ProjectPreviewSourceRevision {
            lifecycle_event_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
            lifecycle_causation_id: Uuid::now_v7(),
            source_pull_request_change_id: SourcePullRequestChangeId::new(),
            organization_id: subscription.organization_id,
            project_id: subscription.project_id,
            source_environment_id: subscription.environment_id,
            source_subscription_id: subscription.id,
            preview_id,
            preview_aggregate_version: version,
            preview_environment_id,
            installation_id: subscription.installation_id,
            base_repository: subscription.repository.clone(),
            base_branch: subscription.branch.clone(),
            head_repository: matches!(desired_state, PreviewSourceRevisionDesiredState::Active)
                .then(|| subscription.repository.clone()),
            head_branch: GitReference::parse("branch", "feature/preview").expect("branch"),
            head_commit_sha: GitCommitSha::parse("a".repeat(40)).expect("commit"),
            pull_request_id: 42,
            pull_request_number: 7,
            desired_state,
            fact_digest: Sha256Digest::from_bytes(
                format!("fact-{version}-{}", desired_state.as_str()).as_bytes(),
            ),
            fact_occurred_at: canonical_timestamp(occurred_at),
        }
    }
}
