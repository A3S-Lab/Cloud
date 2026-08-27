use crate::modules::developer_workflows::domain::{
    AcceptBuildPlanWrite, AcceptedBuildPlan, BuildPlanWriteReference, IBuildPlanRepository,
};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId,
    RepositoryError, SourceRevisionId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

type PlanKey = (OrganizationId, BuildPlanId);
type RootKey = (
    OrganizationId,
    ProjectId,
    EnvironmentId,
    SourceRevisionId,
    String,
);
type IdempotencyKey = (String, String);

#[derive(Default)]
struct State {
    plans: BTreeMap<PlanKey, AcceptedBuildPlan>,
    roots: BTreeMap<RootKey, BuildPlanId>,
    idempotency: BTreeMap<IdempotencyKey, (String, BuildPlanWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

#[derive(Default)]
pub struct InMemoryBuildPlanRepository {
    state: RwLock<State>,
}

impl InMemoryBuildPlanRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IBuildPlanRepository for InMemoryBuildPlanRepository {
    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError> {
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
        write: AcceptBuildPlanWrite,
    ) -> Result<IdempotentWrite<AcceptedBuildPlan>, RepositoryError> {
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
        let root_key = root_key(&write.plan);
        if let Some(existing_id) = state.roots.get(&root_key).copied() {
            let existing = state
                .plans
                .get(&(write.plan.organization_id, existing_id))
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "accepted BuildPlan root points to a missing record".into(),
                    )
                })?;
            if existing.contract != write.plan.contract {
                return Err(RepositoryError::Conflict(
                    "Source revision project root already accepted another BuildPlan".into(),
                ));
            }
            state.idempotency.insert(
                idempotency_key,
                (
                    write.idempotency.request_digest,
                    BuildPlanWriteReference::from(&existing),
                ),
            );
            return Ok(IdempotentWrite {
                value: existing,
                replayed: true,
            });
        }
        if state
            .plans
            .contains_key(&(write.plan.organization_id, write.plan.id))
        {
            return Err(RepositoryError::Conflict(
                "accepted BuildPlan identity is already in use".into(),
            ));
        }
        let reference = BuildPlanWriteReference::from(&write.plan);
        state.roots.insert(root_key, write.plan.id);
        state.plans.insert(
            (write.plan.organization_id, write.plan.id),
            write.plan.clone(),
        );
        state.idempotency.insert(
            idempotency_key,
            (write.idempotency.request_digest, reference),
        );
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.plan,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        build_plan_id: BuildPlanId,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .plans
            .get(&(organization_id, build_plan_id))
            .filter(|plan| plan.project_id == project_id && plan.environment_id == environment_id)
            .cloned())
    }

    async fn find_for_source_root(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
        project_root: &str,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError> {
        let state = self.state.read().await;
        let key = (
            organization_id,
            project_id,
            environment_id,
            source_revision_id,
            project_root.to_owned(),
        );
        let Some(id) = state.roots.get(&key) else {
            return Ok(None);
        };
        state
            .plans
            .get(&(organization_id, *id))
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "accepted BuildPlan root points to a missing record".into(),
                )
            })
            .map(Some)
    }

    async fn list_for_source(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
        limit: usize,
    ) -> Result<Vec<AcceptedBuildPlan>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut plans = self
            .state
            .read()
            .await
            .plans
            .values()
            .filter(|plan| {
                plan.organization_id == organization_id
                    && plan.project_id == project_id
                    && plan.environment_id == environment_id
                    && plan.source_revision_id == source_revision_id
            })
            .cloned()
            .collect::<Vec<_>>();
        plans.sort_by(AcceptedBuildPlan::canonical_cmp);
        plans.truncate(limit);
        Ok(plans)
    }
}

fn idempotency_key(idempotency: &IdempotencyRequest) -> IdempotencyKey {
    (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    )
}

fn root_key(plan: &AcceptedBuildPlan) -> RootKey {
    (
        plan.organization_id,
        plan.project_id,
        plan.environment_id,
        plan.source_revision_id,
        plan.contract.spec().proposal.spec().project_root.clone(),
    )
}

fn load_reference(
    state: &State,
    reference: BuildPlanWriteReference,
) -> Result<AcceptedBuildPlan, RepositoryError> {
    state
        .plans
        .get(&(reference.organization_id, reference.build_plan_id))
        .filter(|plan| {
            plan.project_id == reference.project_id
                && plan.environment_id == reference.environment_id
        })
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage(
                "accepted BuildPlan idempotency points to a missing record".into(),
            )
        })
}
