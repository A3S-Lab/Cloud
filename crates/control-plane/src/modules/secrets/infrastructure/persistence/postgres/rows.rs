use crate::modules::secrets::domain::{
    EncryptedSecretValue, Secret, SecretState, SecretVersion, SecretVersionState,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, ResourceName, SecretId,
};
use a3s_orm::{DecodeError, FromRow, FromValue, Row};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) const SELECT_SECRETS: &str = "select s.id, s.organization_id, s.project_id, s.environment_id, s.name, s.state, s.current_version, s.aggregate_version, s.created_at, s.updated_at, s.revoked_at from secrets s";
pub(super) const SELECT_SECRET_VERSIONS: &str = "select v.secret_id, v.version, v.key_id, v.ciphertext, v.state, v.aggregate_version, v.created_at, v.revoked_at from secret_versions v";

pub(super) struct SecretRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    name: String,
    state: String,
    current_version: u64,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

pub(super) struct SecretVersionRow {
    secret_id: Uuid,
    version: u64,
    key_id: String,
    ciphertext: String,
    state: String,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for SecretRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            name: decode(row, 4)?,
            state: decode(row, 5)?,
            current_version: decode(row, 6)?,
            aggregate_version: decode(row, 7)?,
            created_at: decode(row, 8)?,
            updated_at: decode(row, 9)?,
            revoked_at: decode(row, 10)?,
        })
    }
}

impl FromRow for SecretVersionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            secret_id: decode(row, 0)?,
            version: decode(row, 1)?,
            key_id: decode(row, 2)?,
            ciphertext: decode(row, 3)?,
            state: decode(row, 4)?,
            aggregate_version: decode(row, 5)?,
            created_at: decode(row, 6)?,
            revoked_at: decode(row, 7)?,
        })
    }
}

impl SecretRow {
    pub(super) fn secret(self) -> Result<Secret, RepositoryError> {
        let value = Secret {
            id: SecretId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            name: ResourceName::parse(self.name).map_err(stored("Secret name"))?,
            state: SecretState::parse(&self.state).map_err(stored("Secret state"))?,
            current_version: self.current_version,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            revoked_at: self.revoked_at,
        };
        value
            .validate()
            .map_err(stored("Secret state transition"))?;
        Ok(value)
    }
}

impl SecretVersionRow {
    pub(super) fn version(self) -> Result<SecretVersion, RepositoryError> {
        let value = SecretVersion {
            secret_id: SecretId::from_uuid(self.secret_id),
            version: self.version,
            encrypted_value: EncryptedSecretValue::new(self.key_id, self.ciphertext)
                .map_err(stored("Secret encrypted value"))?,
            state: SecretVersionState::parse(&self.state)
                .map_err(stored("Secret version state"))?,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            revoked_at: self.revoked_at,
        };
        value
            .validate()
            .map_err(stored("Secret version transition"))?;
        Ok(value)
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored {label} is invalid: {error}"))
}
