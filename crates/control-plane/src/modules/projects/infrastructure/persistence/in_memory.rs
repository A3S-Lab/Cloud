use crate::modules::projects::domain::entities::{Environment, Project, ProjectAttributionProfile};
use crate::modules::projects::domain::repositories::{
    IEnvironmentRepository, IProjectRepository, ProjectAttributionRecord,
    UpdateProjectAttributionWrite,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId,
    ProjectAttributionProfileId, ProjectId, RepositoryError,
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
    attribution_profiles: BTreeMap<
        (OrganizationId, ProjectId, ProjectAttributionProfileId),
        ProjectAttributionProfile,
    >,
    attribution_idempotency: BTreeMap<(String, String), (String, ProjectAttributionRecord)>,
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

    async fn replay_attribution_update(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<ProjectAttributionRecord>>, RepositoryError> {
        idempotency.validate().map_err(RepositoryError::Storage)?;
        let state = self.state.read().await;
        let key = (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        );
        match state.attribution_idempotency.get(&key) {
            Some((digest, _)) if digest != &idempotency.request_digest => {
                Err(RepositoryError::IdempotencyConflict)
            }
            Some((_, record)) => Ok(Some(IdempotentWrite {
                value: record.clone(),
                replayed: true,
            })),
            None => Ok(None),
        }
    }

    async fn update_attribution(
        &self,
        write: UpdateProjectAttributionWrite,
    ) -> Result<IdempotentWrite<ProjectAttributionRecord>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let key = (
            write.idempotency.storage_key().0.to_owned(),
            write.idempotency.storage_key().1.to_owned(),
        );
        if let Some((digest, record)) = state.attribution_idempotency.get(&key) {
            if digest != &write.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: record.clone(),
                replayed: true,
            });
        }
        let project_key = (
            write.record.project.organization_id,
            write.record.project.id,
        );
        let existing = state
            .projects
            .get(&project_key)
            .ok_or(RepositoryError::NotFound)?;
        write.validate_against(existing).map_err(|_| {
            RepositoryError::Conflict(
                "project changed while updating its attribution profile".into(),
            )
        })?;
        let profile = &write.record.attribution_profile;
        let profile_key = (profile.organization_id, profile.project_id, profile.id);
        if state.attribution_profiles.contains_key(&profile_key) {
            return Err(RepositoryError::Conflict(
                "project attribution profile already exists".into(),
            ));
        }
        state
            .attribution_profiles
            .insert(profile_key, profile.clone());
        state
            .projects
            .insert(project_key, write.record.project.clone());
        state.attribution_idempotency.insert(
            key,
            (write.idempotency.request_digest, write.record.clone()),
        );
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn find_attribution_profile(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        attribution_profile_id: ProjectAttributionProfileId,
    ) -> Result<Option<ProjectAttributionProfile>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .attribution_profiles
            .get(&(organization_id, project_id, attribution_profile_id))
            .cloned())
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
