use crate::modules::files::domain::{
    IUserFileRepository, ReserveUserFileWrite, TransitionUserFileWrite, UserFile, UserFileQuota,
    DEFAULT_USER_FILE_ORGANIZATION_QUOTA_BYTES,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError, UserFileId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
struct StoredReplay {
    request_digest: String,
    file: UserFile,
}

#[derive(Default)]
struct State {
    files: HashMap<(OrganizationId, UserFileId), UserFile>,
    quotas: HashMap<OrganizationId, UserFileQuota>,
    replays: HashMap<(String, String), StoredReplay>,
    events: Vec<DomainEventEnvelope>,
}

pub struct InMemoryUserFileRepository {
    default_quota_bytes: u64,
    state: Mutex<State>,
}

impl Default for InMemoryUserFileRepository {
    fn default() -> Self {
        Self::new(DEFAULT_USER_FILE_ORGANIZATION_QUOTA_BYTES)
            .expect("default UserFile quota is valid")
    }
}

impl InMemoryUserFileRepository {
    pub fn new(default_quota_bytes: u64) -> Result<Self, String> {
        UserFileQuota::empty(OrganizationId::new(), default_quota_bytes)?;
        Ok(Self {
            default_quota_bytes,
            state: Mutex::new(State::default()),
        })
    }

    #[cfg(test)]
    pub async fn event_count(&self) -> usize {
        self.state.lock().await.events.len()
    }
}

#[async_trait]
impl IUserFileRepository for InMemoryUserFileRepository {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<UserFile>>, RepositoryError> {
        idempotency.validate().map_err(RepositoryError::Storage)?;
        let state = self.state.lock().await;
        replay(&state, idempotency)
    }

    async fn reserve(
        &self,
        write: ReserveUserFileWrite,
    ) -> Result<IdempotentWrite<UserFile>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.lock().await;
        if let Some(replay) = replay(&state, &write.idempotency)? {
            return Ok(replay);
        }
        let key = file_key(&write.file);
        if state.files.contains_key(&key) {
            return Err(RepositoryError::Conflict(
                "UserFile identity is already reserved".into(),
            ));
        }
        if state.files.values().any(|file| {
            file.organization_id == write.file.organization_id
                && file.upload_id == write.file.upload_id
        }) {
            return Err(RepositoryError::Conflict(
                "UserFile upload identity is already reserved".into(),
            ));
        }
        let current_quota = state
            .quotas
            .get(&write.file.organization_id)
            .cloned()
            .unwrap_or(
                UserFileQuota::empty(write.file.organization_id, self.default_quota_bytes)
                    .map_err(RepositoryError::Storage)?,
            );
        let size_bytes = write.file.contract.spec().content.size_bytes;
        if !current_quota.can_reserve(size_bytes) {
            return Err(quota_exceeded(&current_quota, size_bytes));
        }
        let next_quota = current_quota
            .reserve(size_bytes, write.file.updated_at)
            .map_err(RepositoryError::Storage)?;
        state.quotas.insert(write.file.organization_id, next_quota);
        state.files.insert(key, write.file.clone());
        state.events.push(write.event);
        store_replay(&mut state, write.idempotency, &write.file);
        Ok(IdempotentWrite {
            value: write.file,
            replayed: false,
        })
    }

    async fn transition(
        &self,
        write: TransitionUserFileWrite,
    ) -> Result<IdempotentWrite<UserFile>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.lock().await;
        if let Some(replay) = replay(&state, &write.idempotency)? {
            return Ok(replay);
        }
        let key = file_key(&write.file);
        let current = state
            .files
            .get(&key)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        write
            .validate_against(&current)
            .map_err(RepositoryError::Conflict)?;
        if current.quota_reserved() && !write.file.quota_reserved() {
            let current_quota = state
                .quotas
                .get(&write.file.organization_id)
                .cloned()
                .ok_or_else(|| {
                    RepositoryError::Storage(
                        "UserFile allocation has no organization quota row".into(),
                    )
                })?;
            let next_quota = current_quota
                .release(
                    current.contract.spec().content.size_bytes,
                    write.file.updated_at,
                )
                .map_err(RepositoryError::Storage)?;
            state.quotas.insert(write.file.organization_id, next_quota);
        }
        state.files.insert(key, write.file.clone());
        state.events.push(write.event);
        store_replay(&mut state, write.idempotency, &write.file);
        Ok(IdempotentWrite {
            value: write.file,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        user_file_id: UserFileId,
    ) -> Result<Option<UserFile>, RepositoryError> {
        let value = self
            .state
            .lock()
            .await
            .files
            .get(&(organization_id, user_file_id))
            .filter(|file| file.project_id == project_id)
            .cloned();
        value
            .map(|file| {
                file.validate()
                    .map(|()| file)
                    .map_err(RepositoryError::Storage)
            })
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<UserFile>, RepositoryError> {
        if limit == 0 {
            return Err(RepositoryError::Storage(
                "UserFile list limit must be positive".into(),
            ));
        }
        let mut files = self
            .state
            .lock()
            .await
            .files
            .values()
            .filter(|file| file.organization_id == organization_id && file.project_id == project_id)
            .cloned()
            .collect::<Vec<_>>();
        files.sort_by_key(|file| (file.created_at, file.id.as_uuid()));
        files.truncate(limit);
        for file in &files {
            file.validate().map_err(RepositoryError::Storage)?;
        }
        Ok(files)
    }

    async fn quota(
        &self,
        organization_id: OrganizationId,
    ) -> Result<UserFileQuota, RepositoryError> {
        self.state
            .lock()
            .await
            .quotas
            .get(&organization_id)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| {
                UserFileQuota::empty(organization_id, self.default_quota_bytes)
                    .map_err(RepositoryError::Storage)
            })
    }
}

fn file_key(file: &UserFile) -> (OrganizationId, UserFileId) {
    (file.organization_id, file.id)
}

fn replay(
    state: &State,
    idempotency: &IdempotencyRequest,
) -> Result<Option<IdempotentWrite<UserFile>>, RepositoryError> {
    let Some(stored) = state
        .replays
        .get(&(idempotency.scope.clone(), idempotency.key.clone()))
    else {
        return Ok(None);
    };
    if stored.request_digest != idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    stored.file.validate().map_err(RepositoryError::Storage)?;
    Ok(Some(IdempotentWrite {
        value: stored.file.clone(),
        replayed: true,
    }))
}

fn store_replay(state: &mut State, idempotency: IdempotencyRequest, file: &UserFile) {
    state.replays.insert(
        (idempotency.scope, idempotency.key),
        StoredReplay {
            request_digest: idempotency.request_digest,
            file: file.clone(),
        },
    );
}

fn quota_exceeded(quota: &UserFileQuota, requested_bytes: u64) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "UserFile organization quota exceeded: requested {requested_bytes} bytes with {} bytes available",
        quota.available_bytes()
    ))
}
