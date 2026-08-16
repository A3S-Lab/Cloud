use crate::modules::durable_cells::domain::{
    CreateDurableCellDeploymentWrite, DurableCellDeployment, IDurableCellDeploymentRepository,
};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, IdempotencyRequest,
    IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

type DeploymentKey = (
    OrganizationId,
    DurableCellApplicationId,
    DurableCellApplicationRevisionId,
);

#[derive(Default)]
pub struct InMemoryDurableCellDeploymentRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    deployments: BTreeMap<DeploymentKey, DurableCellDeployment>,
    idempotency: BTreeMap<(String, String), (String, DurableCellDeployment)>,
}

impl InMemoryDurableCellDeploymentRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IDurableCellDeploymentRepository for InMemoryDurableCellDeploymentRepository {
    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DurableCellDeployment>, RepositoryError> {
        idempotency.validate().map_err(RepositoryError::Storage)?;
        let state = self.state.read().await;
        let Some((digest, deployment)) = state
            .idempotency
            .get(&(idempotency.scope.clone(), idempotency.key.clone()))
        else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        Ok(Some(deployment.clone()))
    }

    async fn create(
        &self,
        write: CreateDurableCellDeploymentWrite,
    ) -> Result<IdempotentWrite<DurableCellDeployment>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            write.idempotency.scope.clone(),
            write.idempotency.key.clone(),
        );
        if let Some((digest, deployment)) = state.idempotency.get(&idempotency_key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: deployment.clone(),
                replayed: true,
            });
        }
        let deployment_key = key(&write.deployment);
        if state.deployments.contains_key(&deployment_key)
            || state.deployments.values().any(|existing| {
                existing.projection.workload_revision_id
                    == write.deployment.projection.workload_revision_id
                    || existing.projection.deployment_id
                        == write.deployment.projection.deployment_id
                    || existing.projection.operation_id == write.deployment.projection.operation_id
            })
        {
            return Err(RepositoryError::Conflict(
                "Durable Cell deployment correlation identity is already in use".into(),
            ));
        }
        state
            .deployments
            .insert(deployment_key, write.deployment.clone());
        state.idempotency.insert(
            idempotency_key,
            (write.idempotency.request_digest, write.deployment.clone()),
        );
        Ok(IdempotentWrite {
            value: write.deployment,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        application_revision_id: DurableCellApplicationRevisionId,
    ) -> Result<Option<DurableCellDeployment>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .deployments
            .get(&(organization_id, application_id, application_revision_id))
            .filter(|deployment| {
                deployment.projection.project_id == project_id
                    && deployment.projection.environment_id == environment_id
            })
            .cloned())
    }
}

fn key(deployment: &DurableCellDeployment) -> DeploymentKey {
    (
        deployment.projection.organization_id,
        deployment.projection.application_id,
        deployment.projection.application_revision_id,
    )
}
