use crate::modules::secrets::domain::{
    Secret, SecretChanged, SecretState, SecretVersion, SecretVersionState,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId, RepositoryError, SecretId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretWriteReference {
    pub secret_id: SecretId,
    pub version: u64,
}

#[derive(Debug, Clone)]
pub struct SecretWrite {
    pub secret: Secret,
    pub version: SecretVersion,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct CreateSecretWrite {
    pub secret: Secret,
    pub version: SecretVersion,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct RotateSecretWrite {
    pub secret: Secret,
    pub version: SecretVersion,
    pub expected_secret_version: u64,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct TransitionSecretVersion {
    pub secret: Secret,
    pub version: SecretVersion,
    pub expected_secret_version: u64,
    pub expected_version: u64,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

impl CreateSecretWrite {
    pub fn validate(&self) -> Result<(), String> {
        validate_write(
            &self.secret,
            &self.version,
            &self.event,
            "secret.secret.created",
        )?;
        if self.secret.current_version != 1
            || self.secret.aggregate_version != 1
            || self.secret.state != SecretState::Active
            || self.version.version != 1
            || self.version.aggregate_version != 1
            || self.version.state != SecretVersionState::Active
            || self.secret.created_at != self.version.created_at
        {
            return Err("new Secret is not at its initial version".into());
        }
        Ok(())
    }
}

impl RotateSecretWrite {
    pub fn validate(&self) -> Result<(), String> {
        validate_write(
            &self.secret,
            &self.version,
            &self.event,
            "secret.version.created",
        )
    }

    pub fn validate_against(&self, existing: &Secret) -> Result<(), String> {
        if existing.aggregate_version != self.expected_secret_version
            || self.expected_secret_version.checked_add(1) != Some(self.secret.aggregate_version)
            || existing.current_version.checked_add(1) != Some(self.secret.current_version)
            || existing.state != SecretState::Active
            || self.secret.state != SecretState::Active
            || self.secret.updated_at < existing.updated_at
            || self.version.version != self.secret.current_version
            || self.version.aggregate_version != 1
            || self.version.state != SecretVersionState::Active
            || self.version.created_at != self.secret.updated_at
            || immutable_secret_fields_changed(existing, &self.secret)
        {
            return Err("Secret changed while creating its version".into());
        }
        Ok(())
    }
}

impl TransitionSecretVersion {
    pub fn validate(&self) -> Result<(), String> {
        validate_write(
            &self.secret,
            &self.version,
            &self.event,
            "secret.version.revoked",
        )
    }

    pub fn validate_against(
        &self,
        existing_secret: &Secret,
        existing_version: &SecretVersion,
    ) -> Result<(), String> {
        if existing_secret.aggregate_version != self.expected_secret_version
            || existing_version.aggregate_version != self.expected_version
            || self.expected_secret_version.checked_add(1) != Some(self.secret.aggregate_version)
            || self.expected_version.checked_add(1) != Some(self.version.aggregate_version)
            || existing_secret.state != SecretState::Active
            || self.secret.state != SecretState::Active
            || self.secret.updated_at < existing_secret.updated_at
            || existing_version.state != SecretVersionState::Active
            || self.version.state != SecretVersionState::Revoked
            || self.version.revoked_at != Some(self.secret.updated_at)
            || self.version.encrypted_value != existing_version.encrypted_value
            || self.version.created_at != existing_version.created_at
            || immutable_secret_fields_changed(existing_secret, &self.secret)
            || self.secret.current_version != existing_secret.current_version
            || self.secret.state != existing_secret.state
        {
            return Err("Secret changed while revoking its version".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait ISecretRepository: Send + Sync {
    async fn replay_write(
        &self,
        organization_id: OrganizationId,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<SecretWrite>, RepositoryError>;

    async fn create(&self, bundle: CreateSecretWrite) -> Result<SecretWrite, RepositoryError>;

    async fn rotate(&self, bundle: RotateSecretWrite) -> Result<SecretWrite, RepositoryError>;

    async fn transition_version(
        &self,
        bundle: TransitionSecretVersion,
    ) -> Result<SecretWrite, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
    ) -> Result<Secret, RepositoryError>;

    async fn find_version(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
        version: u64,
    ) -> Result<SecretVersion, RepositoryError>;

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Secret>, RepositoryError>;

    async fn list_versions(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
    ) -> Result<Vec<SecretVersion>, RepositoryError>;
}

fn validate_write(
    secret: &Secret,
    version: &SecretVersion,
    event: &DomainEventEnvelope,
    event_key: &str,
) -> Result<(), String> {
    secret.validate()?;
    version.validate()?;
    if secret.id != version.secret_id
        || event.event_key != event_key
        || event.organization_id != secret.organization_id.as_uuid()
        || event.aggregate_id != secret.id.as_uuid()
        || event.aggregate_version != secret.aggregate_version
        || event.occurred_at != secret.updated_at
    {
        return Err("Secret write and domain event are inconsistent".into());
    }
    let payload: SecretChanged = serde_json::from_value(event.payload.clone())
        .map_err(|error| format!("Secret domain event is invalid: {error}"))?;
    if payload.organization_id != secret.organization_id
        || payload.project_id != secret.project_id
        || payload.environment_id != secret.environment_id
        || payload.secret_id != secret.id
        || payload.name != secret.name.as_str()
        || payload.state != secret.state.as_str()
        || payload.version != version.version
        || payload.version_state != version.state.as_str()
    {
        return Err("Secret domain event payload is inconsistent".into());
    }
    Ok(())
}

fn immutable_secret_fields_changed(existing: &Secret, changed: &Secret) -> bool {
    existing.id != changed.id
        || existing.organization_id != changed.organization_id
        || existing.project_id != changed.project_id
        || existing.environment_id != changed.environment_id
        || existing.name != changed.name
        || existing.created_at != changed.created_at
        || existing.revoked_at != changed.revoked_at
}
