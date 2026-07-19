use crate::modules::secrets::domain::EncryptedSecretValue;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, OrganizationId, ProjectId, ResourceName, SecretId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretState {
    Active,
    Revoked,
}

impl SecretState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "revoked" => Ok(Self::Revoked),
            _ => Err(format!("unsupported Secret state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretVersionState {
    Active,
    Revoked,
}

impl SecretVersionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "revoked" => Ok(Self::Revoked),
            _ => Err(format!("unsupported Secret version state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Secret {
    pub id: SecretId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub name: ResourceName,
    pub state: SecretState,
    pub current_version: u64,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl Secret {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: SecretId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        name: ResourceName,
        encrypted_value: EncryptedSecretValue,
        created_at: DateTime<Utc>,
    ) -> Result<(Self, SecretVersion), String> {
        validate_identity(id, organization_id, project_id, environment_id)?;
        encrypted_value.validate()?;
        let created_at = canonical_timestamp(created_at);
        let secret = Self {
            id,
            organization_id,
            project_id,
            environment_id,
            name,
            state: SecretState::Active,
            current_version: 1,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            revoked_at: None,
        };
        let version = SecretVersion::create(id, 1, encrypted_value, created_at)?;
        Ok((secret, version))
    }

    pub fn rotate(
        &mut self,
        encrypted_value: EncryptedSecretValue,
        created_at: DateTime<Utc>,
    ) -> Result<SecretVersion, String> {
        if self.state != SecretState::Active {
            return Err("revoked Secret cannot create another version".into());
        }
        encrypted_value.validate()?;
        let created_at = canonical_timestamp(created_at);
        self.ensure_time(created_at)?;
        let next_version = self
            .current_version
            .checked_add(1)
            .ok_or_else(|| "Secret version overflowed".to_owned())?;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Secret aggregate version overflowed".to_owned())?;
        self.current_version = next_version;
        self.updated_at = created_at;
        SecretVersion::create(self.id, next_version, encrypted_value, created_at)
    }

    pub fn revoke(&mut self, revoked_at: DateTime<Utc>) -> Result<(), String> {
        let revoked_at = canonical_timestamp(revoked_at);
        self.ensure_time(revoked_at)?;
        if self.state == SecretState::Revoked {
            return Ok(());
        }
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Secret aggregate version overflowed".to_owned())?;
        self.state = SecretState::Revoked;
        self.updated_at = revoked_at;
        self.revoked_at = Some(revoked_at);
        Ok(())
    }

    fn ensure_time(&self, at: DateTime<Utc>) -> Result<(), String> {
        if at < self.updated_at {
            return Err("Secret transition time regressed".into());
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretVersion {
    pub secret_id: SecretId,
    pub version: u64,
    pub encrypted_value: EncryptedSecretValue,
    pub state: SecretVersionState,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl std::fmt::Debug for SecretVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretVersion")
            .field("secret_id", &self.secret_id)
            .field("version", &self.version)
            .field("encrypted_value", &"<redacted-ciphertext>")
            .field("state", &self.state)
            .field("aggregate_version", &self.aggregate_version)
            .field("created_at", &self.created_at)
            .field("revoked_at", &self.revoked_at)
            .finish()
    }
}

impl SecretVersion {
    fn create(
        secret_id: SecretId,
        version: u64,
        encrypted_value: EncryptedSecretValue,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if secret_id.as_uuid().is_nil() || version == 0 {
            return Err("Secret version identity is invalid".into());
        }
        encrypted_value.validate()?;
        Ok(Self {
            secret_id,
            version,
            encrypted_value,
            state: SecretVersionState::Active,
            aggregate_version: 1,
            created_at: canonical_timestamp(created_at),
            revoked_at: None,
        })
    }

    pub fn revoke(&mut self, revoked_at: DateTime<Utc>) -> Result<(), String> {
        let revoked_at = canonical_timestamp(revoked_at);
        if revoked_at < self.created_at {
            return Err("Secret version transition time regressed".into());
        }
        if self.state == SecretVersionState::Revoked {
            return Ok(());
        }
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Secret version aggregate overflowed".to_owned())?;
        self.state = SecretVersionState::Revoked;
        self.revoked_at = Some(revoked_at);
        Ok(())
    }

    pub fn is_materializable(&self, secret: &Secret) -> bool {
        self.secret_id == secret.id
            && secret.state == SecretState::Active
            && self.state == SecretVersionState::Active
    }
}

fn validate_identity(
    id: SecretId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> Result<(), String> {
    if id.as_uuid().is_nil()
        || organization_id.as_uuid().is_nil()
        || project_id.as_uuid().is_nil()
        || environment_id.as_uuid().is_nil()
    {
        return Err("Secret identity is invalid".into());
    }
    Ok(())
}
