use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Fixed admission policy for a newly observed organization. Existing quota
/// rows remain authoritative, so a future policy change cannot silently
/// rewrite already allocated tenant capacity.
pub const DEFAULT_USER_FILE_ORGANIZATION_QUOTA_BYTES: u64 = 50 * 1024 * 1024 * 1024;
pub const USER_FILE_PUBLIC_INTEGER_MAX: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserFileQuota {
    pub organization_id: OrganizationId,
    pub limit_bytes: u64,
    pub allocated_bytes: u64,
    pub revision: u64,
    pub updated_at: Option<DateTime<Utc>>,
}

impl UserFileQuota {
    pub fn empty(organization_id: OrganizationId, limit_bytes: u64) -> Result<Self, String> {
        Self::restore(organization_id, limit_bytes, 0, 0, None)
    }

    pub fn restore(
        organization_id: OrganizationId,
        limit_bytes: u64,
        allocated_bytes: u64,
        revision: u64,
        updated_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            limit_bytes,
            allocated_bytes,
            revision,
            updated_at: updated_at.map(canonical_timestamp),
        };
        value.validate()?;
        Ok(value)
    }

    pub const fn available_bytes(&self) -> u64 {
        self.limit_bytes - self.allocated_bytes
    }

    pub fn can_reserve(&self, size_bytes: u64) -> bool {
        size_bytes > 0 && size_bytes <= self.available_bytes()
    }

    pub fn reserve(&self, size_bytes: u64, at: DateTime<Utc>) -> Result<Self, String> {
        self.validate()?;
        if !self.can_reserve(size_bytes) {
            return Err("UserFile organization quota would be exceeded".into());
        }
        self.next(self.allocated_bytes + size_bytes, at)
    }

    pub fn release(&self, size_bytes: u64, at: DateTime<Utc>) -> Result<Self, String> {
        self.validate()?;
        if size_bytes == 0 || size_bytes > self.allocated_bytes {
            return Err("UserFile quota release exceeds its allocation".into());
        }
        self.next(self.allocated_bytes - size_bytes, at)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.limit_bytes == 0
            || self.limit_bytes > USER_FILE_PUBLIC_INTEGER_MAX
            || self.allocated_bytes > self.limit_bytes
            || self.revision > USER_FILE_PUBLIC_INTEGER_MAX
            || (self.revision == 0) != self.updated_at.is_none()
            || self
                .updated_at
                .is_some_and(|value| value != canonical_timestamp(value))
        {
            return Err("stored UserFile organization quota is invalid".into());
        }
        Ok(())
    }

    fn next(&self, allocated_bytes: u64, at: DateTime<Utc>) -> Result<Self, String> {
        let at = canonical_timestamp(at);
        if self.updated_at.is_some_and(|previous| at < previous) {
            return Err("UserFile quota update precedes its current revision".into());
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "UserFile quota revision is exhausted".to_owned())?;
        Self::restore(
            self.organization_id,
            self.limit_bytes,
            allocated_bytes,
            revision,
            Some(at),
        )
    }
}
