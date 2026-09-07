use crate::modules::automations::application::schedule_worker::{
    AutomationScheduleCandidate, IAutomationScheduleCandidateProvider,
};
use crate::modules::automations::domain::{
    AutomationDefinitionRecord, AutomationScheduleStateKey, IAutomationDefinitionRepository,
    IAutomationScheduleAuthorizationSnapshotProvider, IAutomationScheduleInputProvider,
    IAutomationScheduleStateRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_cloud_contracts::AutomationTriggerV1;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Repository-backed schedule candidate discovery.
///
/// This adapter composes only owner ports: Automations supplies the exact
/// current revision and durable schedule state, while the target/input and
/// Identity owners supply their own immutable input and authorization snapshot.
/// Missing state, input, or authorization is fail-closed and produces no
/// candidate; a persisted revision/state mismatch is surfaced as a conflict.
#[derive(Clone)]
pub struct RepositoryAutomationScheduleCandidateProvider {
    definitions: Arc<dyn IAutomationDefinitionRepository>,
    states: Arc<dyn IAutomationScheduleStateRepository>,
    inputs: Arc<dyn IAutomationScheduleInputProvider>,
    authorizations: Arc<dyn IAutomationScheduleAuthorizationSnapshotProvider>,
    max_candidates: usize,
    occurrence_limit: usize,
}

impl RepositoryAutomationScheduleCandidateProvider {
    pub fn new(
        definitions: Arc<dyn IAutomationDefinitionRepository>,
        states: Arc<dyn IAutomationScheduleStateRepository>,
        inputs: Arc<dyn IAutomationScheduleInputProvider>,
        authorizations: Arc<dyn IAutomationScheduleAuthorizationSnapshotProvider>,
        max_candidates: usize,
        occurrence_limit: usize,
    ) -> Result<Self, String> {
        if max_candidates == 0 || max_candidates > 10_000 {
            return Err("Automation schedule candidate bound is outside its limit".into());
        }
        if occurrence_limit == 0
            || occurrence_limit
                > crate::modules::automations::domain::AUTOMATION_SCHEDULE_MAX_OCCURRENCES
        {
            return Err("Automation schedule occurrence bound is outside its limit".into());
        }
        Ok(Self {
            definitions,
            states,
            inputs,
            authorizations,
            max_candidates,
            occurrence_limit,
        })
    }

    async fn candidate_for(
        &self,
        record: AutomationDefinitionRecord,
    ) -> ApplicationResult<Option<AutomationScheduleCandidate>> {
        record.validate().map_err(|error| {
            ApplicationError::Internal(format!("invalid Automation record: {error}"))
        })?;
        let definition = record.definition.spec();
        if !matches!(definition.trigger, AutomationTriggerV1::Schedule(_)) {
            return Ok(None);
        }
        let key = AutomationScheduleStateKey::new(
            OrganizationId::from_uuid(definition.organization_id),
            ProjectId::from_uuid(definition.project_id),
            EnvironmentId::from_uuid(definition.environment_id),
            definition.automation_id,
        );
        let Some(state) = self.states.find(key).await? else {
            return Ok(None);
        };
        if state.revision_id() != record.revision.spec().revision_id
            || state.revision_digest().as_str() != record.revision.digest()
        {
            return Err(ApplicationError::Conflict(
                "Automation schedule state is bound to a different definition head".into(),
            ));
        }
        let Some(authorization) = self
            .authorizations
            .resolve(&record.revision)
            .await
            .map_err(|error| {
                ApplicationError::Unavailable(format!(
                    "Automation schedule authorization resolution failed: {error}"
                ))
            })?
        else {
            return Ok(None);
        };
        let Some(input) = self
            .inputs
            .resolve(&record.revision)
            .await
            .map_err(|error| {
                ApplicationError::Unavailable(format!(
                    "Automation schedule input resolution failed: {error}"
                ))
            })?
        else {
            return Ok(None);
        };
        Ok(Some(AutomationScheduleCandidate {
            key,
            revision: record.revision,
            input,
            authorization,
            limit: self.occurrence_limit,
        }))
    }
}

#[async_trait]
impl IAutomationScheduleCandidateProvider for RepositoryAutomationScheduleCandidateProvider {
    async fn candidates(
        &self,
        _observed_at: DateTime<Utc>,
    ) -> ApplicationResult<Vec<AutomationScheduleCandidate>> {
        let records = self.definitions.list(self.max_candidates).await?;
        let mut candidates = Vec::new();
        for record in records {
            if let Some(candidate) = self.candidate_for(record).await? {
                candidates.push(candidate);
            }
        }
        candidates.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then_with(|| left.revision.digest().cmp(right.revision.digest()))
        });
        Ok(candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::automations::domain::{
        AutomationScheduleState, CreateAutomationDefinition, IAutomationScheduleStateRepository,
    };
    use crate::modules::automations::infrastructure::{
        InMemoryAutomationDefinitionRepository, InMemoryAutomationScheduleStateRepository,
    };
    use crate::modules::shared_kernel::domain::Sha256Digest;
    use a3s_cloud_contracts::{
        AutomationDefinitionV1, AutomationInvocationAuthorizationV1, AutomationInvocationInputV1,
        AutomationRevisionV1,
    };
    use chrono::DateTime;
    use serde_json::json;
    use uuid::Uuid;

    const SCHEDULE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-schedule.acl"
    ));

    struct Inputs;

    #[async_trait]
    impl IAutomationScheduleInputProvider for Inputs {
        async fn resolve(
            &self,
            _revision: &AutomationRevisionV1,
        ) -> Result<Option<AutomationInvocationInputV1>, String> {
            Ok(Some(AutomationInvocationInputV1::inline_json(json!({
                "source": "candidate-test"
            }))?))
        }
    }

    struct Authorizations;

    #[async_trait]
    impl IAutomationScheduleAuthorizationSnapshotProvider for Authorizations {
        async fn resolve(
            &self,
            revision: &AutomationRevisionV1,
        ) -> Result<Option<AutomationInvocationAuthorizationV1>, String> {
            Ok(Some(AutomationInvocationAuthorizationV1 {
                policy_digest: revision
                    .spec()
                    .definition
                    .authorization
                    .policy_digest
                    .clone(),
                grant_snapshot_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                principal_id: None,
            }))
        }
    }

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("timestamp")
    }

    #[tokio::test]
    async fn discovers_only_ready_schedule_heads_with_owner_snapshots() {
        let definition = AutomationDefinitionV1::parse_acl(SCHEDULE).expect("definition");
        let revision = AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x300),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("revision");
        let head =
            AutomationDefinitionV1::from_spec(revision.spec().definition.clone()).expect("head");
        let definitions = Arc::new(InMemoryAutomationDefinitionRepository::new());
        definitions
            .create(CreateAutomationDefinition {
                definition: head,
                revision: revision.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("definition");
        let state_key = AutomationScheduleStateKey::new(
            OrganizationId::from_uuid(revision.spec().definition.organization_id),
            ProjectId::from_uuid(revision.spec().definition.project_id),
            EnvironmentId::from_uuid(revision.spec().definition.environment_id),
            revision.spec().definition.automation_id,
        );
        let states = Arc::new(InMemoryAutomationScheduleStateRepository::new());
        states
            .create(
                AutomationScheduleState::new(
                    state_key,
                    revision.spec().revision_id,
                    Sha256Digest::parse(revision.digest()).expect("digest"),
                    timestamp(900),
                    timestamp(900),
                )
                .expect("state"),
            )
            .await
            .expect("schedule state");
        let provider = RepositoryAutomationScheduleCandidateProvider::new(
            definitions,
            states,
            Arc::new(Inputs),
            Arc::new(Authorizations),
            10,
            2,
        )
        .expect("provider");
        let candidates = provider
            .candidates(timestamp(1_001))
            .await
            .expect("candidates");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].revision, revision);
        assert_eq!(candidates[0].limit, 2);
    }

    #[tokio::test]
    async fn rejects_state_bound_to_an_old_revision() {
        let definition = AutomationDefinitionV1::parse_acl(SCHEDULE).expect("definition");
        let revision = AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x301),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("revision");
        let head =
            AutomationDefinitionV1::from_spec(revision.spec().definition.clone()).expect("head");
        let definitions = Arc::new(InMemoryAutomationDefinitionRepository::new());
        definitions
            .create(CreateAutomationDefinition {
                definition: head,
                revision: revision.clone(),
                created_at: timestamp(1_000),
            })
            .await
            .expect("definition");
        let key = AutomationScheduleStateKey::new(
            OrganizationId::from_uuid(revision.spec().definition.organization_id),
            ProjectId::from_uuid(revision.spec().definition.project_id),
            EnvironmentId::from_uuid(revision.spec().definition.environment_id),
            revision.spec().definition.automation_id,
        );
        let states = Arc::new(InMemoryAutomationScheduleStateRepository::new());
        states
            .create(
                AutomationScheduleState::new(
                    key,
                    Uuid::from_u128(0x302),
                    Sha256Digest::parse(revision.digest()).expect("digest"),
                    timestamp(900),
                    timestamp(900),
                )
                .expect("state"),
            )
            .await
            .expect("schedule state");
        let provider = RepositoryAutomationScheduleCandidateProvider::new(
            definitions,
            states,
            Arc::new(Inputs),
            Arc::new(Authorizations),
            10,
            2,
        )
        .expect("provider");
        assert!(matches!(
            provider.candidates(timestamp(1_001)).await,
            Err(ApplicationError::Conflict(message)) if message.contains("different definition head")
        ));
    }
}
