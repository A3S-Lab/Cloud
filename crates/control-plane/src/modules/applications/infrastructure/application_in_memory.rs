use crate::modules::applications::domain::{
    Application, ApplicationRecord, ApplicationRelease, ApplicationWriteReference,
    CreateApplicationWrite, IApplicationRepository, PublishApplicationReleaseWrite,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, IdempotencyRequest, IdempotentWrite, OrganizationId,
    ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryApplicationRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    applications: BTreeMap<(OrganizationId, ApplicationId), Application>,
    names: BTreeMap<(OrganizationId, ProjectId, String), ApplicationId>,
    releases: BTreeMap<(OrganizationId, ApplicationId, ApplicationReleaseId), ApplicationRelease>,
    idempotency: BTreeMap<(String, String), (String, ApplicationWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryApplicationRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IApplicationRepository for InMemoryApplicationRepository {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ApplicationRecord>, RepositoryError> {
        let state = self.state.read().await;
        replay(&state, idempotency)
    }

    async fn create(
        &self,
        write: CreateApplicationWrite,
    ) -> Result<IdempotentWrite<ApplicationRecord>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        let application = &write.record.application;
        let application_key = (application.organization_id, application.id);
        let name_key = (
            application.organization_id,
            application.project_id,
            application.name.key().to_owned(),
        );
        if state.applications.contains_key(&application_key) || state.names.contains_key(&name_key)
        {
            return Err(RepositoryError::Conflict(
                "Application name is already in use in this project".into(),
            ));
        }
        let release_key = (
            application.organization_id,
            application.id,
            write.record.release.id,
        );
        if state.releases.contains_key(&release_key) {
            return Err(RepositoryError::Conflict(
                "Application release identity is already in use".into(),
            ));
        }
        state.names.insert(name_key, application.id);
        state
            .applications
            .insert(application_key, application.clone());
        state
            .releases
            .insert(release_key, write.record.release.clone());
        store_replay(&mut state, &write.idempotency, &write.record);
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn publish_release(
        &self,
        write: PublishApplicationReleaseWrite,
    ) -> Result<IdempotentWrite<ApplicationRecord>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        if let Some(record) = replay(&state, &write.idempotency)? {
            return Ok(IdempotentWrite {
                value: record,
                replayed: true,
            });
        }
        let application_key = (
            write.record.application.organization_id,
            write.record.application.id,
        );
        let current = state
            .applications
            .get(&application_key)
            .ok_or(RepositoryError::NotFound)?
            .clone();
        write
            .validate_against(&current)
            .map_err(RepositoryError::Conflict)?;
        let release_key = (
            write.record.application.organization_id,
            write.record.application.id,
            write.record.release.id,
        );
        if state.releases.contains_key(&release_key) {
            return Err(RepositoryError::Conflict(
                "Application release identity is already in use".into(),
            ));
        }
        state
            .applications
            .insert(application_key, write.record.application.clone());
        state
            .releases
            .insert(release_key, write.record.release.clone());
        store_replay(&mut state, &write.idempotency, &write.record);
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.record,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
    ) -> Result<Option<Application>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .applications
            .get(&(organization_id, application_id))
            .filter(|application| application.project_id == project_id)
            .cloned())
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<Application>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut values = self
            .state
            .read()
            .await
            .applications
            .values()
            .filter(|application| {
                application.organization_id == organization_id
                    && application.project_id == project_id
            })
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            left.name
                .key()
                .cmp(right.name.key())
                .then_with(|| left.id.cmp(&right.id))
        });
        values.truncate(limit);
        Ok(values)
    }

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        release_id: ApplicationReleaseId,
    ) -> Result<Option<ApplicationRelease>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .releases
            .get(&(organization_id, application_id, release_id))
            .filter(|release| release.project_id == project_id)
            .cloned())
    }

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        limit: usize,
    ) -> Result<Vec<ApplicationRelease>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut values = self
            .state
            .read()
            .await
            .releases
            .values()
            .filter(|release| {
                release.organization_id == organization_id
                    && release.project_id == project_id
                    && release.application_id == application_id
            })
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| {
            right
                .release_number
                .cmp(&left.release_number)
                .then_with(|| left.id.cmp(&right.id))
        });
        values.truncate(limit);
        Ok(values)
    }
}

fn replay(
    state: &State,
    idempotency: &IdempotencyRequest,
) -> Result<Option<ApplicationRecord>, RepositoryError> {
    let key = (
        idempotency.storage_key().0.to_owned(),
        idempotency.storage_key().1.to_owned(),
    );
    let Some((digest, reference)) = state.idempotency.get(&key) else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    let head = state
        .applications
        .get(&(reference.organization_id, reference.application_id))
        .filter(|application| application.project_id == reference.project_id)
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("Application replay head is missing".into()))?;
    let release = state
        .releases
        .get(&(
            reference.organization_id,
            reference.application_id,
            reference.release_id,
        ))
        .filter(|release| release.project_id == reference.project_id)
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("Application replay release is missing".into()))?;
    let application = head.at_release(&release).map_err(|error| {
        RepositoryError::Storage(format!("Application replay target is invalid: {error}"))
    })?;
    ApplicationRecord::new(application, release)
        .map(Some)
        .map_err(RepositoryError::Storage)
}

fn store_replay(state: &mut State, idempotency: &IdempotencyRequest, record: &ApplicationRecord) {
    state.idempotency.insert(
        (
            idempotency.storage_key().0.to_owned(),
            idempotency.storage_key().1.to_owned(),
        ),
        (
            idempotency.request_digest.clone(),
            ApplicationWriteReference::from(record),
        ),
    );
}
