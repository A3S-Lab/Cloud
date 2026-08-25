use crate::modules::developer_workflows::domain::{
    AcceptPullRequestPreviewPolicyRevisionWrite, AcceptedPullRequestPreviewPolicyRevision,
    IPullRequestPreviewPolicyRepository, PreviewPolicyRevisionWriteReference,
    MAX_PULL_REQUEST_PREVIEW_POLICY_REVISIONS_PAGE,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId,
    PullRequestPreviewPolicyRevisionId, RepositoryError, SourceSubscriptionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

type RevisionKey = (OrganizationId, PullRequestPreviewPolicyRevisionId);
type SequenceKey = (OrganizationId, SourceSubscriptionId, u64);
type IdempotencyKey = (String, String);

#[derive(Default)]
struct State {
    revisions: BTreeMap<RevisionKey, AcceptedPullRequestPreviewPolicyRevision>,
    sequence: BTreeMap<SequenceKey, PullRequestPreviewPolicyRevisionId>,
    idempotency: BTreeMap<IdempotencyKey, (String, PreviewPolicyRevisionWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

#[derive(Default)]
pub struct InMemoryPullRequestPreviewPolicyRepository {
    state: RwLock<State>,
}

impl InMemoryPullRequestPreviewPolicyRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IPullRequestPreviewPolicyRepository for InMemoryPullRequestPreviewPolicyRepository {
    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        idempotency.validate().map_err(RepositoryError::Storage)?;
        let state = self.state.read().await;
        let key = idempotency_key(idempotency);
        let Some((digest, reference)) = state.idempotency.get(&key) else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        load_reference(&state, *reference).map(Some)
    }

    async fn accept(
        &self,
        write: AcceptPullRequestPreviewPolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let request_key = idempotency_key(&write.idempotency);
        if let Some((digest, reference)) = state.idempotency.get(&request_key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: load_reference(&state, *reference)?,
                replayed: true,
            });
        }

        let current = current_revision(
            &state,
            write.revision.organization_id,
            write.revision.source_subscription_id,
        )?;
        if let Some(existing) = current.as_ref() {
            ensure_same_policy(existing, &write.revision)?;
            if existing.contract == write.revision.contract {
                state.idempotency.insert(
                    request_key,
                    (
                        write.idempotency.request_digest,
                        PreviewPolicyRevisionWriteReference::from(existing),
                    ),
                );
                return Ok(IdempotentWrite {
                    value: existing.clone(),
                    replayed: true,
                });
            }
        }
        let actual_previous = current.as_ref().map(|revision| revision.id);
        let expected_number = current
            .as_ref()
            .map_or(Some(1), |revision| revision.revision_number.checked_add(1))
            .ok_or_else(|| {
                RepositoryError::Conflict("Preview policy revision overflowed".into())
            })?;
        if actual_previous != write.expected_previous_revision_id
            || write.revision.revision_number != expected_number
        {
            return Err(RepositoryError::Conflict(
                "Preview policy head advanced before acceptance".into(),
            ));
        }
        if state
            .revisions
            .contains_key(&(write.revision.organization_id, write.revision.id))
            || state.sequence.contains_key(&(
                write.revision.organization_id,
                write.revision.source_subscription_id,
                write.revision.revision_number,
            ))
        {
            return Err(RepositoryError::Conflict(
                "Preview policy revision identity is already in use".into(),
            ));
        }

        let reference = PreviewPolicyRevisionWriteReference::from(&write.revision);
        state.sequence.insert(
            (
                write.revision.organization_id,
                write.revision.source_subscription_id,
                write.revision.revision_number,
            ),
            write.revision.id,
        );
        state.revisions.insert(
            (write.revision.organization_id, write.revision.id),
            write.revision.clone(),
        );
        state
            .idempotency
            .insert(request_key, (write.idempotency.request_digest, reference));
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.revision,
            replayed: false,
        })
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        revision_id: PullRequestPreviewPolicyRevisionId,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .revisions
            .get(&(organization_id, revision_id))
            .filter(|revision| {
                revision.project_id == project_id
                    && revision.source_environment_id == source_environment_id
                    && revision.source_subscription_id == source_subscription_id
            })
            .cloned())
    }

    async fn find_current(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        let state = self.state.read().await;
        Ok(
            current_revision(&state, organization_id, source_subscription_id)?.filter(|revision| {
                revision.project_id == project_id
                    && revision.source_environment_id == source_environment_id
            }),
        )
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        limit: usize,
    ) -> Result<Vec<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let state = self.state.read().await;
        let mut revisions = state
            .sequence
            .range(
                (organization_id, source_subscription_id, 0)
                    ..=(organization_id, source_subscription_id, u64::MAX),
            )
            .filter_map(|(_, id)| state.revisions.get(&(organization_id, *id)))
            .filter(|revision| {
                revision.project_id == project_id
                    && revision.source_environment_id == source_environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        revisions.truncate(limit.min(MAX_PULL_REQUEST_PREVIEW_POLICY_REVISIONS_PAGE));
        Ok(revisions)
    }
}

fn ensure_same_policy(
    existing: &AcceptedPullRequestPreviewPolicyRevision,
    candidate: &AcceptedPullRequestPreviewPolicyRevision,
) -> Result<(), RepositoryError> {
    let existing_policy = existing.contract.policy();
    let candidate_policy = candidate.contract.policy();
    if existing.organization_id != candidate.organization_id
        || existing.project_id != candidate.project_id
        || existing.source_environment_id != candidate.source_environment_id
        || existing.source_subscription_id != candidate.source_subscription_id
        || existing_policy.installation_id != candidate_policy.installation_id
        || existing_policy.base_repository != candidate_policy.base_repository
        || existing_policy.base_branch != candidate_policy.base_branch
    {
        return Err(RepositoryError::Conflict(
            "Preview policy identity collided with another source binding".into(),
        ));
    }
    Ok(())
}

fn current_revision(
    state: &State,
    organization_id: OrganizationId,
    source_subscription_id: SourceSubscriptionId,
) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
    let Some((_, id)) = state
        .sequence
        .range(
            (organization_id, source_subscription_id, 0)
                ..=(organization_id, source_subscription_id, u64::MAX),
        )
        .next_back()
    else {
        return Ok(None);
    };
    state
        .revisions
        .get(&(organization_id, *id))
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage(
                "Preview policy revision sequence points to a missing record".into(),
            )
        })
        .map(Some)
}

fn idempotency_key(idempotency: &IdempotencyRequest) -> IdempotencyKey {
    (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    )
}

fn load_reference(
    state: &State,
    reference: PreviewPolicyRevisionWriteReference,
) -> Result<AcceptedPullRequestPreviewPolicyRevision, RepositoryError> {
    state
        .revisions
        .get(&(
            reference.organization_id,
            reference.preview_policy_revision_id,
        ))
        .filter(|revision| {
            revision.project_id == reference.project_id
                && revision.source_environment_id == reference.source_environment_id
                && revision.source_subscription_id == reference.source_subscription_id
        })
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage(
                "Preview policy idempotency points to a missing revision".into(),
            )
        })
}
