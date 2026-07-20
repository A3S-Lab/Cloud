use crate::modules::secrets::domain::{
    CreateSecretWrite, ISecretRepository, RotateSecretWrite, Secret, SecretVersion, SecretWrite,
    SecretWriteReference, TransitionSecretVersion,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId, RepositoryError, SecretId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemorySecretRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    secrets: BTreeMap<SecretId, Secret>,
    versions: BTreeMap<(SecretId, u64), SecretVersion>,
    names: BTreeMap<(OrganizationId, ProjectId, EnvironmentId, String), SecretId>,
    idempotency: BTreeMap<(String, String), (String, SecretWriteReference)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemorySecretRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }

    pub async fn idempotency_references(&self) -> Vec<SecretWriteReference> {
        self.state
            .read()
            .await
            .idempotency
            .values()
            .map(|(_, reference)| *reference)
            .collect()
    }
}

#[async_trait]
impl ISecretRepository for InMemorySecretRepository {
    async fn replay_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<SecretWrite>, RepositoryError> {
        let state = self.state.read().await;
        replay(&state, organization_id, idempotency)
    }

    async fn create(&self, bundle: CreateSecretWrite) -> Result<SecretWrite, RepositoryError> {
        bundle.validate().map_err(invalid_write)?;
        let mut state = self.state.write().await;
        if let Some(replay) = replay(&state, bundle.secret.organization_id, &bundle.idempotency)? {
            return Ok(replay);
        }
        let name_key = (
            bundle.secret.organization_id,
            bundle.secret.project_id,
            bundle.secret.environment_id,
            bundle.secret.name.key().to_owned(),
        );
        if state.names.contains_key(&name_key) {
            return Err(RepositoryError::Conflict(
                "Secret name is already in use".into(),
            ));
        }
        if state.secrets.contains_key(&bundle.secret.id)
            || state
                .versions
                .contains_key(&(bundle.version.secret_id, bundle.version.version))
        {
            return Err(RepositoryError::Conflict(
                "Secret identity already exists".into(),
            ));
        }
        let reference = reference(&bundle.version);
        state.names.insert(name_key, bundle.secret.id);
        state
            .secrets
            .insert(bundle.secret.id, bundle.secret.clone());
        state.versions.insert(
            (bundle.version.secret_id, bundle.version.version),
            bundle.version.clone(),
        );
        store_idempotency(&mut state, bundle.idempotency, reference);
        state.outbox.push(bundle.event);
        Ok(SecretWrite {
            secret: bundle.secret,
            version: bundle.version,
            replayed: false,
        })
    }

    async fn rotate(&self, bundle: RotateSecretWrite) -> Result<SecretWrite, RepositoryError> {
        bundle.validate().map_err(invalid_write)?;
        let mut state = self.state.write().await;
        if let Some(replay) = replay(&state, bundle.secret.organization_id, &bundle.idempotency)? {
            return Ok(replay);
        }
        let existing = state
            .secrets
            .get(&bundle.secret.id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        bundle.validate_against(&existing).map_err(invalid_write)?;
        if state
            .versions
            .contains_key(&(bundle.version.secret_id, bundle.version.version))
        {
            return Err(RepositoryError::Conflict(
                "Secret version already exists".into(),
            ));
        }
        let reference = reference(&bundle.version);
        state
            .secrets
            .insert(bundle.secret.id, bundle.secret.clone());
        state.versions.insert(
            (bundle.version.secret_id, bundle.version.version),
            bundle.version.clone(),
        );
        store_idempotency(&mut state, bundle.idempotency, reference);
        state.outbox.push(bundle.event);
        Ok(SecretWrite {
            secret: bundle.secret,
            version: bundle.version,
            replayed: false,
        })
    }

    async fn transition_version(
        &self,
        bundle: TransitionSecretVersion,
    ) -> Result<SecretWrite, RepositoryError> {
        bundle.validate().map_err(invalid_write)?;
        let mut state = self.state.write().await;
        if let Some(replay) = replay(&state, bundle.secret.organization_id, &bundle.idempotency)? {
            return Ok(replay);
        }
        let existing_secret = state
            .secrets
            .get(&bundle.secret.id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let existing_version = state
            .versions
            .get(&(bundle.version.secret_id, bundle.version.version))
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        bundle
            .validate_against(&existing_secret, &existing_version)
            .map_err(invalid_write)?;
        let reference = reference(&bundle.version);
        state
            .secrets
            .insert(bundle.secret.id, bundle.secret.clone());
        state.versions.insert(
            (bundle.version.secret_id, bundle.version.version),
            bundle.version.clone(),
        );
        store_idempotency(&mut state, bundle.idempotency, reference);
        state.outbox.push(bundle.event);
        Ok(SecretWrite {
            secret: bundle.secret,
            version: bundle.version,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
    ) -> Result<Secret, RepositoryError> {
        self.state
            .read()
            .await
            .secrets
            .get(&secret_id)
            .filter(|secret| secret.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn find_version(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
        version: u64,
    ) -> Result<SecretVersion, RepositoryError> {
        let state = self.state.read().await;
        let secret = state
            .secrets
            .get(&secret_id)
            .filter(|secret| secret.organization_id == organization_id)
            .ok_or(RepositoryError::NotFound)?;
        state
            .versions
            .get(&(secret.id, version))
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Secret>, RepositoryError> {
        let mut secrets = self
            .state
            .read()
            .await
            .secrets
            .values()
            .filter(|secret| {
                secret.organization_id == organization_id
                    && secret.project_id == project_id
                    && secret.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        secrets.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(secrets)
    }

    async fn list_versions(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
    ) -> Result<Vec<SecretVersion>, RepositoryError> {
        let state = self.state.read().await;
        let secret = state
            .secrets
            .get(&secret_id)
            .filter(|secret| secret.organization_id == organization_id)
            .ok_or(RepositoryError::NotFound)?;
        Ok(state
            .versions
            .range((secret.id, 0)..=(secret.id, u64::MAX))
            .map(|(_, version)| version.clone())
            .collect())
    }
}

fn replay(
    state: &State,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<SecretWrite>, RepositoryError> {
    let Some((digest, reference)) = state
        .idempotency
        .get(&(idempotency.scope.clone(), idempotency.key.clone()))
    else {
        return Ok(None);
    };
    if digest != &idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict);
    }
    let secret = state
        .secrets
        .get(&reference.secret_id)
        .filter(|secret| secret.organization_id == organization_id)
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("Secret idempotency reference is invalid".into())
        })?;
    let version = state
        .versions
        .get(&(reference.secret_id, reference.version))
        .cloned()
        .ok_or_else(|| {
            RepositoryError::Storage("Secret version idempotency reference is invalid".into())
        })?;
    Ok(Some(SecretWrite {
        secret,
        version,
        replayed: true,
    }))
}

fn store_idempotency(
    state: &mut State,
    idempotency: IdempotencyRequest,
    reference: SecretWriteReference,
) {
    state.idempotency.insert(
        (idempotency.scope, idempotency.key),
        (idempotency.request_digest, reference),
    );
}

fn reference(version: &SecretVersion) -> SecretWriteReference {
    SecretWriteReference {
        secret_id: version.secret_id,
        version: version.version,
    }
}

fn invalid_write(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!("Secret write is invalid: {error}"))
}
