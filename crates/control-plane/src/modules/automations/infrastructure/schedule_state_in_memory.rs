use crate::modules::automations::domain::{
    AutomationScheduleState, AutomationScheduleStateKey, CommitAutomationScheduleCursor,
    IAutomationScheduleStateRepository, ReserveAutomationScheduleLease,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryAutomationScheduleStateRepository {
    states: RwLock<BTreeMap<AutomationScheduleStateKey, AutomationScheduleState>>,
}

impl InMemoryAutomationScheduleStateRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IAutomationScheduleStateRepository for InMemoryAutomationScheduleStateRepository {
    async fn create(
        &self,
        state: AutomationScheduleState,
    ) -> Result<AutomationScheduleState, RepositoryError> {
        state
            .validate_for_creation()
            .map_err(RepositoryError::Storage)?;
        let key = state.key();
        let mut states = self.states.write().await;
        if states.contains_key(&key)
            || states
                .keys()
                .any(|existing| existing.automation_id == key.automation_id)
        {
            return Err(RepositoryError::Conflict(
                "Automation schedule state already exists".into(),
            ));
        }
        states.insert(key, state.clone());
        Ok(state)
    }

    async fn find(
        &self,
        key: AutomationScheduleStateKey,
    ) -> Result<Option<AutomationScheduleState>, RepositoryError> {
        Ok(self.states.read().await.get(&key).cloned())
    }

    async fn reserve(
        &self,
        request: ReserveAutomationScheduleLease,
    ) -> Result<AutomationScheduleState, RepositoryError> {
        let mut states = self.states.write().await;
        let state = states
            .get_mut(&request.key)
            .ok_or(RepositoryError::NotFound)?;
        state.reserve_lease(
            request.owner_id,
            request.lease_id,
            request.reserved_at,
            request.lease_expires_at,
        )?;
        Ok(state.clone())
    }

    async fn commit_cursor(
        &self,
        request: CommitAutomationScheduleCursor,
    ) -> Result<AutomationScheduleState, RepositoryError> {
        let mut states = self.states.write().await;
        let state = states
            .get_mut(&request.key)
            .ok_or(RepositoryError::NotFound)?;
        state.commit_cursor(
            request.owner_id,
            request.lease_id,
            request.lease_generation,
            request.evaluated_through,
            request.committed_at,
        )?;
        Ok(state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, ProjectId, Sha256Digest,
    };
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    fn state(scope: u128) -> AutomationScheduleState {
        AutomationScheduleState::new(
            AutomationScheduleStateKey::new(
                OrganizationId::from_uuid(Uuid::from_u128(scope)),
                ProjectId::from_uuid(Uuid::from_u128(scope + 1)),
                EnvironmentId::from_uuid(Uuid::from_u128(scope + 2)),
                Uuid::from_u128(4),
            ),
            Uuid::from_u128(5),
            Sha256Digest::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
            timestamp(1_000),
            timestamp(900),
        )
        .expect("state")
    }

    #[tokio::test]
    async fn persists_one_fenced_cursor_and_rejects_cross_scope_automation_reuse() {
        let repository = InMemoryAutomationScheduleStateRepository::new();
        let initial = state(1);
        let key = initial.key();
        repository.create(initial).await.expect("create");
        assert!(repository.create(state(1)).await.is_err());
        assert!(repository.create(state(10)).await.is_err());

        let reserved = repository
            .reserve(ReserveAutomationScheduleLease {
                key,
                owner_id: Uuid::from_u128(6),
                lease_id: Uuid::from_u128(7),
                reserved_at: timestamp(1_001),
                lease_expires_at: timestamp(1_101),
            })
            .await
            .expect("reserve");
        assert_eq!(reserved.lease_generation(), 1);
        let committed = repository
            .commit_cursor(CommitAutomationScheduleCursor {
                key,
                owner_id: Uuid::from_u128(6),
                lease_id: Uuid::from_u128(7),
                lease_generation: 1,
                evaluated_through: timestamp(1_020),
                committed_at: timestamp(1_030),
            })
            .await
            .expect("commit");
        assert_eq!(committed.cursor_at(), timestamp(1_020));
        assert!(committed.lease().is_none());
    }
}
