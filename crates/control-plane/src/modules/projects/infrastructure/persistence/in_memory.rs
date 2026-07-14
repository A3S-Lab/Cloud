use crate::modules::projects::domain::entities::{Environment, Project};
use crate::modules::projects::domain::repositories::{IEnvironmentRepository, IProjectRepository};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryProjectsRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    projects: BTreeMap<(OrganizationId, ProjectId), Project>,
    project_names: BTreeMap<(OrganizationId, String), ProjectId>,
    project_idempotency: BTreeMap<(String, String), (String, Project)>,
    environments: BTreeMap<(OrganizationId, ProjectId, EnvironmentId), Environment>,
    environment_names: BTreeMap<(OrganizationId, ProjectId, String), EnvironmentId>,
    environment_idempotency: BTreeMap<(String, String), (String, Environment)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryProjectsRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IProjectRepository for InMemoryProjectsRepository {
    async fn create(
        &self,
        project: Project,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Project>, RepositoryError> {
        let mut state = self.state.write().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, existing)) = state.project_idempotency.get(&key) {
            if digest != &idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        let name_key = (project.organization_id, project.name.key().to_owned());
        if state.project_names.contains_key(&name_key) {
            return Err(RepositoryError::Conflict(
                "project name is already in use".into(),
            ));
        }
        state.project_names.insert(name_key, project.id);
        state
            .projects
            .insert((project.organization_id, project.id), project.clone());
        state
            .project_idempotency
            .insert(key, (idempotency.request_digest, project.clone()));
        state.outbox.push(event);
        Ok(IdempotentWrite {
            value: project,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Option<Project>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .projects
            .get(&(organization_id, project_id))
            .cloned())
    }

    async fn list(&self, organization_id: OrganizationId) -> Result<Vec<Project>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .projects
            .values()
            .filter(|project| project.organization_id == organization_id)
            .cloned()
            .collect())
    }
}

#[async_trait]
impl IEnvironmentRepository for InMemoryProjectsRepository {
    async fn create(
        &self,
        environment: Environment,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
        let mut state = self.state.write().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, existing)) = state.environment_idempotency.get(&key) {
            if digest != &idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        let name_key = (
            environment.organization_id,
            environment.project_id,
            environment.name.key().to_owned(),
        );
        if state.environment_names.contains_key(&name_key) {
            return Err(RepositoryError::Conflict(
                "environment name is already in use".into(),
            ));
        }
        state.environment_names.insert(name_key, environment.id);
        state.environments.insert(
            (
                environment.organization_id,
                environment.project_id,
                environment.id,
            ),
            environment.clone(),
        );
        state
            .environment_idempotency
            .insert(key, (idempotency.request_digest, environment.clone()));
        state.outbox.push(event);
        Ok(IdempotentWrite {
            value: environment,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Option<Environment>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .environments
            .get(&(organization_id, project_id, environment_id))
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<Environment>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .environments
            .values()
            .filter(|environment| {
                environment.organization_id == organization_id
                    && environment.project_id == project_id
            })
            .cloned()
            .collect())
    }
}
