use super::AutomationScheduleLease;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, OrganizationId, ProjectId, RepositoryError, Sha256Digest,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Durable identity for one schedule cursor. The revision digest is retained
/// as an immutable binding; the scheduler must resolve the exact revision
/// before constructing the stateless calendar and policy evaluators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AutomationScheduleStateKey {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub automation_id: Uuid,
}

impl AutomationScheduleStateKey {
    pub const fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        automation_id: Uuid,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            environment_id,
            automation_id,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.automation_id.is_nil()
        {
            return Err("Automation schedule state identity is invalid".into());
        }
        Ok(())
    }
}

/// The durable cursor and its current lease fence.
#[derive(Clone, PartialEq, Eq)]
pub struct AutomationScheduleState {
    key: AutomationScheduleStateKey,
    revision_id: Uuid,
    revision_digest: Sha256Digest,
    cursor_at: DateTime<Utc>,
    lease_generation: u64,
    lease: Option<AutomationScheduleLease>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl AutomationScheduleState {
    pub fn new(
        key: AutomationScheduleStateKey,
        revision_id: Uuid,
        revision_digest: Sha256Digest,
        cursor_at: DateTime<Utc>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        let state = Self {
            key,
            revision_id,
            revision_digest,
            cursor_at: canonical_timestamp(cursor_at),
            lease_generation: 0,
            lease: None,
            created_at,
            updated_at: created_at,
        };
        state.validate()?;
        Ok(state)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        key: AutomationScheduleStateKey,
        revision_id: Uuid,
        revision_digest: Sha256Digest,
        cursor_at: DateTime<Utc>,
        lease_generation: u64,
        lease: Option<AutomationScheduleLease>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let state = Self {
            key,
            revision_id,
            revision_digest,
            cursor_at,
            lease_generation,
            lease,
            created_at,
            updated_at,
        };
        state.validate()?;
        Ok(state)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.key.validate()?;
        if self.revision_id.is_nil()
            || Sha256Digest::parse(self.revision_digest.as_str()).is_err()
            || self.cursor_at != canonical_timestamp(self.cursor_at)
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || self.lease_generation > i64::MAX as u64
        {
            return Err("Automation schedule state is invalid".into());
        }
        if let Some(lease) = &self.lease {
            lease.validate()?;
            if self.lease_generation == 0 {
                return Err("Automation schedule lease generation is invalid".into());
            }
        } else if self.lease_generation > 0 {
            // A released state retains the monotonic generation so an old
            // worker can never reuse a previous fence.
        }
        Ok(())
    }

    pub fn validate_for_creation(&self) -> Result<(), String> {
        self.validate()?;
        if self.lease_generation != 0 || self.lease.is_some() || self.updated_at != self.created_at
        {
            return Err("Automation schedule state creation must start without a lease".into());
        }
        Ok(())
    }

    /// Acquire a new fence, or reject an active lease. A takeover is allowed
    /// only at or after the previous lease expiry and always rotates the UUID.
    pub fn reserve_lease(
        &mut self,
        owner_id: Uuid,
        lease_id: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<AutomationScheduleLease, RepositoryError> {
        let reserved_at = canonical_timestamp(reserved_at);
        if self
            .lease
            .as_ref()
            .is_some_and(|lease| reserved_at < lease.lease_expires_at())
        {
            return Err(RepositoryError::Conflict(
                "Automation schedule lease is still active".into(),
            ));
        }
        if reserved_at < self.updated_at {
            return Err(RepositoryError::Conflict(
                "Automation schedule lease reservation is older than state progress".into(),
            ));
        }
        if self
            .lease
            .as_ref()
            .is_some_and(|lease| lease.lease_id() == lease_id)
        {
            return Err(RepositoryError::Conflict(
                "Automation schedule lease takeover must rotate its fence".into(),
            ));
        }
        let lease = AutomationScheduleLease::new(owner_id, lease_id, reserved_at, lease_expires_at)
            .map_err(RepositoryError::Conflict)?;
        let lease_generation = self.lease_generation.checked_add(1).ok_or_else(|| {
            RepositoryError::Conflict("Automation schedule lease generation exhausted".into())
        })?;
        let mut next = self.clone();
        next.lease_generation = lease_generation;
        next.lease = Some(lease.clone());
        next.updated_at = reserved_at;
        next.validate().map_err(RepositoryError::Storage)?;
        *self = next;
        Ok(lease)
    }

    /// Atomically advance the cursor and release the exact fence.
    pub fn commit_cursor(
        &mut self,
        owner_id: Uuid,
        lease_id: Uuid,
        lease_generation: u64,
        evaluated_through: DateTime<Utc>,
        committed_at: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        if lease_generation != self.lease_generation {
            return Err(RepositoryError::Conflict(
                "Automation schedule lease generation is stale".into(),
            ));
        }
        let committed_at = canonical_timestamp(committed_at);
        let evaluated_through = canonical_timestamp(evaluated_through);
        let lease = self.lease.as_ref().ok_or_else(|| {
            RepositoryError::Conflict("Automation schedule has no active lease".into())
        })?;
        lease
            .authorize(owner_id, lease_id, committed_at)
            .map_err(RepositoryError::Conflict)?;
        if evaluated_through > committed_at {
            return Err(RepositoryError::Conflict(
                "Automation schedule cursor cannot advance beyond its commit observation".into(),
            ));
        }
        if evaluated_through <= self.cursor_at {
            return Err(RepositoryError::Conflict(
                "Automation schedule cursor must advance".into(),
            ));
        }
        self.cursor_at = evaluated_through;
        self.lease = None;
        self.updated_at = committed_at;
        self.validate().map_err(RepositoryError::Storage)
    }

    /// Release the exact active fence without advancing the cursor.
    ///
    /// A scheduler uses this path when a bounded evaluation finds no due
    /// occurrence or when admission fails before cursor progress can be
    /// committed. The cursor remains exclusive and the next owner may safely
    /// retry the same window.
    pub fn release_lease(
        &mut self,
        owner_id: Uuid,
        lease_id: Uuid,
        lease_generation: u64,
        released_at: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        if lease_generation != self.lease_generation {
            return Err(RepositoryError::Conflict(
                "Automation schedule lease generation is stale".into(),
            ));
        }
        let released_at = canonical_timestamp(released_at);
        if released_at < self.updated_at {
            return Err(RepositoryError::Conflict(
                "Automation schedule lease release is older than state progress".into(),
            ));
        }
        let lease = self.lease.as_ref().ok_or_else(|| {
            RepositoryError::Conflict("Automation schedule has no active lease".into())
        })?;
        lease
            .authorize(owner_id, lease_id, released_at)
            .map_err(RepositoryError::Conflict)?;
        self.lease = None;
        self.updated_at = released_at;
        self.validate().map_err(RepositoryError::Storage)
    }

    pub const fn key(&self) -> AutomationScheduleStateKey {
        self.key
    }

    pub const fn revision_id(&self) -> Uuid {
        self.revision_id
    }

    pub const fn revision_digest(&self) -> &Sha256Digest {
        &self.revision_digest
    }

    pub const fn cursor_at(&self) -> DateTime<Utc> {
        self.cursor_at
    }

    pub const fn lease_generation(&self) -> u64 {
        self.lease_generation
    }

    pub const fn lease(&self) -> Option<&AutomationScheduleLease> {
        self.lease.as_ref()
    }

    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

impl std::fmt::Debug for AutomationScheduleState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AutomationScheduleState")
            .field("key", &self.key)
            .field("revision_id", &self.revision_id)
            .field("revision_digest", &self.revision_digest)
            .field("cursor_at", &self.cursor_at)
            .field("lease_generation", &self.lease_generation)
            .field("lease", &self.lease.as_ref().map(|_| "redacted"))
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReserveAutomationScheduleLease {
    pub key: AutomationScheduleStateKey,
    pub owner_id: Uuid,
    pub lease_id: Uuid,
    pub reserved_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitAutomationScheduleCursor {
    pub key: AutomationScheduleStateKey,
    pub owner_id: Uuid,
    pub lease_id: Uuid,
    pub lease_generation: u64,
    pub evaluated_through: DateTime<Utc>,
    pub committed_at: DateTime<Utc>,
}

#[async_trait]
pub trait IAutomationScheduleStateRepository: Send + Sync {
    async fn create(
        &self,
        state: AutomationScheduleState,
    ) -> Result<AutomationScheduleState, RepositoryError>;

    async fn find(
        &self,
        key: AutomationScheduleStateKey,
    ) -> Result<Option<AutomationScheduleState>, RepositoryError>;

    async fn reserve(
        &self,
        request: ReserveAutomationScheduleLease,
    ) -> Result<AutomationScheduleState, RepositoryError>;

    async fn commit_cursor(
        &self,
        request: CommitAutomationScheduleCursor,
    ) -> Result<AutomationScheduleState, RepositoryError>;

    async fn release_lease(
        &self,
        request: ReleaseAutomationScheduleLease,
    ) -> Result<AutomationScheduleState, RepositoryError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAutomationScheduleLease {
    pub key: AutomationScheduleStateKey,
    pub owner_id: Uuid,
    pub lease_id: Uuid,
    pub lease_generation: u64,
    pub released_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::Sha256Digest;

    fn state() -> AutomationScheduleState {
        AutomationScheduleState::new(
            AutomationScheduleStateKey::new(
                OrganizationId::from_uuid(Uuid::from_u128(1)),
                ProjectId::from_uuid(Uuid::from_u128(2)),
                EnvironmentId::from_uuid(Uuid::from_u128(3)),
                Uuid::from_u128(4),
            ),
            Uuid::from_u128(5),
            Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
            timestamp(1_000),
            timestamp(900),
        )
        .expect("state")
    }

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    #[test]
    fn rotates_fence_and_commits_cursor_once() {
        let mut state = state();
        let lease = state
            .reserve_lease(
                Uuid::from_u128(6),
                Uuid::from_u128(7),
                timestamp(1_001),
                timestamp(1_101),
            )
            .expect("lease");
        assert_eq!(state.lease_generation(), 1);
        state
            .commit_cursor(
                lease.owner_id(),
                lease.lease_id(),
                1,
                timestamp(1_080),
                timestamp(1_090),
            )
            .expect("commit");
        assert_eq!(state.cursor_at(), timestamp(1_080));
        assert!(state.lease().is_none());
        assert!(state
            .commit_cursor(
                lease.owner_id(),
                lease.lease_id(),
                1,
                timestamp(1_090),
                timestamp(1_091),
            )
            .is_err());
    }

    #[test]
    fn rejects_active_and_stale_fences() {
        let mut state = state();
        state
            .reserve_lease(
                Uuid::from_u128(6),
                Uuid::from_u128(7),
                timestamp(1_001),
                timestamp(1_101),
            )
            .expect("lease");
        assert!(state
            .reserve_lease(
                Uuid::from_u128(8),
                Uuid::from_u128(9),
                timestamp(1_050),
                timestamp(1_100),
            )
            .is_err());
        assert!(state
            .commit_cursor(
                Uuid::from_u128(8),
                Uuid::from_u128(9),
                1,
                timestamp(1_080),
                timestamp(1_090),
            )
            .is_err());
    }

    #[test]
    fn releases_an_active_fence_without_advancing_the_cursor() {
        let mut state = state();
        state
            .reserve_lease(
                Uuid::from_u128(6),
                Uuid::from_u128(7),
                timestamp(1_001),
                timestamp(1_101),
            )
            .expect("lease");
        state
            .release_lease(Uuid::from_u128(6), Uuid::from_u128(7), 1, timestamp(1_050))
            .expect("release");
        assert_eq!(state.cursor_at(), timestamp(1_000));
        assert!(state.lease().is_none());
        assert!(state
            .release_lease(Uuid::from_u128(6), Uuid::from_u128(7), 1, timestamp(1_051),)
            .is_err());
    }
}
