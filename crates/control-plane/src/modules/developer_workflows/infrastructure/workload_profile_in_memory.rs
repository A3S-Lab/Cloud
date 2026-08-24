use crate::modules::developer_workflows::domain::{
    AcceptWorkloadProfileRevisionWrite, AcceptedWorkloadProfileRevision,
    IWorkloadProfileRepository, WorkloadProfileRevisionWriteReference,
    MAX_WORKLOAD_PROFILE_REVISIONS_PAGE,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
    WorkloadProfileId, WorkloadProfileRevisionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

type RevisionKey = (OrganizationId, WorkloadProfileRevisionId);
type SequenceKey = (OrganizationId, WorkloadProfileId, u64);
type IdempotencyKey = (String, String);

#[derive(Default)]
struct State {
    revisions: BTreeMap<RevisionKey, AcceptedWorkloadProfileRevision>,
    sequence: BTreeMap<SequenceKey, WorkloadProfileRevisionId>,
    idempotency: BTreeMap<IdempotencyKey, (String, WorkloadProfileRevisionWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

#[derive(Default)]
pub struct InMemoryWorkloadProfileRepository {
    state: RwLock<State>,
}

impl InMemoryWorkloadProfileRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IWorkloadProfileRepository for InMemoryWorkloadProfileRepository {
    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError> {
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
        write: AcceptWorkloadProfileRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedWorkloadProfileRevision>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let idempotency_key = idempotency_key(&write.idempotency);
        if let Some((digest, reference)) = state.idempotency.get(&idempotency_key) {
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
            write.revision.profile_id,
        )?;
        if let Some(existing) = current.as_ref() {
            ensure_same_profile(existing, &write.revision)?;
            if existing.contract == write.revision.contract
                && existing.accepted_by == write.revision.accepted_by
            {
                state.idempotency.insert(
                    idempotency_key,
                    (
                        write.idempotency.request_digest,
                        WorkloadProfileRevisionWriteReference::from(existing),
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
                RepositoryError::Conflict("workload profile revision overflowed".into())
            })?;
        if actual_previous != write.expected_previous_revision_id
            || write.revision.revision_number != expected_number
        {
            return Err(RepositoryError::Conflict(
                "workload profile head advanced before acceptance".into(),
            ));
        }
        if state
            .revisions
            .contains_key(&(write.revision.organization_id, write.revision.id))
            || state.sequence.contains_key(&(
                write.revision.organization_id,
                write.revision.profile_id,
                write.revision.revision_number,
            ))
        {
            return Err(RepositoryError::Conflict(
                "workload profile revision identity is already in use".into(),
            ));
        }

        let reference = WorkloadProfileRevisionWriteReference::from(&write.revision);
        state.sequence.insert(
            (
                write.revision.organization_id,
                write.revision.profile_id,
                write.revision.revision_number,
            ),
            write.revision.id,
        );
        state.revisions.insert(
            (write.revision.organization_id, write.revision.id),
            write.revision.clone(),
        );
        state.idempotency.insert(
            idempotency_key,
            (write.idempotency.request_digest, reference),
        );
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
        environment_id: EnvironmentId,
        workload_profile_id: WorkloadProfileId,
        workload_profile_revision_id: WorkloadProfileRevisionId,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .revisions
            .get(&(organization_id, workload_profile_revision_id))
            .filter(|revision| {
                revision.project_id == project_id
                    && revision.environment_id == environment_id
                    && revision.profile_id == workload_profile_id
            })
            .cloned())
    }

    async fn find_current(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_profile_id: WorkloadProfileId,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError> {
        let state = self.state.read().await;
        Ok(
            current_revision(&state, organization_id, workload_profile_id)?.filter(|revision| {
                revision.project_id == project_id && revision.environment_id == environment_id
            }),
        )
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_profile_id: WorkloadProfileId,
        limit: usize,
    ) -> Result<Vec<AcceptedWorkloadProfileRevision>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let state = self.state.read().await;
        let mut revisions = state
            .sequence
            .range(
                (organization_id, workload_profile_id, 0)
                    ..=(organization_id, workload_profile_id, u64::MAX),
            )
            .filter_map(|(_, id)| state.revisions.get(&(organization_id, *id)))
            .filter(|revision| {
                revision.project_id == project_id && revision.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        revisions.truncate(limit.min(MAX_WORKLOAD_PROFILE_REVISIONS_PAGE));
        Ok(revisions)
    }
}

fn ensure_same_profile(
    existing: &AcceptedWorkloadProfileRevision,
    candidate: &AcceptedWorkloadProfileRevision,
) -> Result<(), RepositoryError> {
    if existing.organization_id != candidate.organization_id
        || existing.project_id != candidate.project_id
        || existing.environment_id != candidate.environment_id
        || existing.profile_id != candidate.profile_id
        || existing.contract.spec().project_root != candidate.contract.spec().project_root
        || existing.contract.spec().profile.name != candidate.contract.spec().profile.name
    {
        return Err(RepositoryError::Conflict(
            "workload profile identity collided with another logical profile".into(),
        ));
    }
    Ok(())
}

fn current_revision(
    state: &State,
    organization_id: OrganizationId,
    workload_profile_id: WorkloadProfileId,
) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError> {
    let Some((_, id)) = state
        .sequence
        .range(
            (organization_id, workload_profile_id, 0)
                ..=(organization_id, workload_profile_id, u64::MAX),
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
                "workload profile revision sequence points to a missing record".into(),
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
    reference: WorkloadProfileRevisionWriteReference,
) -> Result<AcceptedWorkloadProfileRevision, RepositoryError> {
    state
        .revisions
        .get(&(
            reference.organization_id,
            reference.workload_profile_revision_id,
        ))
        .filter(|revision| {
            revision.project_id == reference.project_id
                && revision.environment_id == reference.environment_id
                && revision.profile_id == reference.workload_profile_id
        })
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage(
                "workload profile idempotency points to a missing revision".into(),
            )
        })
}
