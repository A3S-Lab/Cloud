use crate::modules::fleet::domain::value_objects::EnrollmentTokenCredential;
use crate::modules::shared_kernel::domain::{EnrollmentTokenId, OrganizationId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentToken {
    pub id: EnrollmentTokenId,
    pub organization_id: OrganizationId,
    pub name: String,
    pub name_key: String,
    pub credential: EnrollmentTokenCredential,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub aggregate_version: u64,
}

impl EnrollmentToken {
    pub fn new(
        id: EnrollmentTokenId,
        organization_id: OrganizationId,
        name: impl Into<String>,
        credential: EnrollmentTokenCredential,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let name = name.into().trim().to_owned();
        let name_key = normalize_name(&name)?;
        if expires_at <= created_at || expires_at > created_at + chrono::Duration::hours(24) {
            return Err("enrollment token lifetime must be positive and at most 24 hours".into());
        }
        Ok(Self {
            id,
            organization_id,
            name,
            name_key,
            credential,
            created_at,
            expires_at,
            used_at: None,
            revoked_at: None,
            aggregate_version: 1,
        })
    }

    pub fn is_usable_at(&self, now: DateTime<Utc>) -> bool {
        self.used_at.is_none()
            && self.revoked_at.is_none()
            && now >= self.created_at
            && now < self.expires_at
    }
}

fn normalize_name(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > 255
        || trimmed.contains('\0')
        || trimmed.contains(['\r', '\n'])
    {
        return Err("enrollment token name is invalid".into());
    }
    Ok(trimmed.to_lowercase())
}
