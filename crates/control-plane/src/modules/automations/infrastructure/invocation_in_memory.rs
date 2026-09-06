use crate::modules::automations::domain::{
    AutomationInvocationAdmission, AutomationInvocationRecord, IAutomationInvocationReader,
    IAutomationInvocationRepository,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::{
    AutomationAuditActionV1, AutomationAuditRecordV1, AutomationInvocationEnvelopeV1,
    AutomationOutboxMessageV1,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Local/test composition for exact invocation admission.
///
/// It owns no timer, queue, target execution, or mutable revision lookup. A
/// production repository can implement the same port with the shared
/// PostgreSQL transaction while retaining these replay rules.
#[derive(Default)]
pub struct InMemoryAutomationInvocationRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    by_invocation: BTreeMap<(Uuid, Uuid), AutomationInvocationRecord>,
    by_deduplication: BTreeMap<(Uuid, Uuid, String), Uuid>,
    audit: Vec<AutomationAuditRecordV1>,
    outbox: Vec<AutomationOutboxMessageV1>,
}

impl InMemoryAutomationInvocationRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn invocations(&self) -> Vec<AutomationInvocationRecord> {
        self.state
            .read()
            .await
            .by_invocation
            .values()
            .cloned()
            .collect()
    }

    pub async fn audit_records(&self) -> Vec<AutomationAuditRecordV1> {
        self.state.read().await.audit.clone()
    }

    pub async fn outbox_messages(&self) -> Vec<AutomationOutboxMessageV1> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IAutomationInvocationRepository for InMemoryAutomationInvocationRepository {
    async fn admit(
        &self,
        envelope: AutomationInvocationEnvelopeV1,
    ) -> Result<AutomationInvocationAdmission, RepositoryError> {
        let record =
            AutomationInvocationRecord::new(envelope).map_err(RepositoryError::Conflict)?;
        let key = (
            record.envelope.organization_id,
            record.envelope.invocation_id,
        );
        let deduplication_key = (
            record.envelope.organization_id,
            record.envelope.automation_id,
            record.envelope.deduplication_key.clone(),
        );
        let mut state = self.state.write().await;

        if let Some(existing) = state.by_invocation.get(&key).cloned() {
            if existing == record {
                let audit = audit_for(
                    &record.envelope,
                    AutomationAuditActionV1::InvocationReplayed,
                )?;
                state.audit.push(audit);
                return Ok(AutomationInvocationAdmission {
                    invocation: existing,
                    replayed: true,
                });
            }
            return Err(RepositoryError::Conflict(
                "Automation invocation identity was reused with different immutable evidence"
                    .into(),
            ));
        }
        if let Some(existing_id) = state.by_deduplication.get(&deduplication_key) {
            return Err(RepositoryError::Conflict(format!(
                "Automation deduplication key is already bound to invocation {existing_id}"
            )));
        }

        let audit = audit_for(
            &record.envelope,
            AutomationAuditActionV1::InvocationAdmitted,
        )?;
        let outbox = AutomationOutboxMessageV1::for_invocation(
            &record.envelope,
            Uuid::now_v7(),
            record.envelope.causation_id,
            record.envelope.requested_at,
        )
        .map_err(RepositoryError::Storage)?;
        state.by_deduplication.insert(deduplication_key, key.1);
        state.by_invocation.insert(key, record.clone());
        state.audit.push(audit);
        state.outbox.push(outbox);
        Ok(AutomationInvocationAdmission {
            invocation: record,
            replayed: false,
        })
    }
}

#[async_trait]
impl IAutomationInvocationReader for InMemoryAutomationInvocationRepository {
    async fn find(
        &self,
        organization_id: Uuid,
        invocation_id: Uuid,
    ) -> Result<Option<AutomationInvocationRecord>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .by_invocation
            .get(&(organization_id, invocation_id))
            .cloned())
    }
}

fn audit_for(
    envelope: &AutomationInvocationEnvelopeV1,
    action: AutomationAuditActionV1,
) -> Result<AutomationAuditRecordV1, RepositoryError> {
    AutomationAuditRecordV1::for_invocation(envelope, action, Uuid::now_v7(), envelope.requested_at)
        .map_err(RepositoryError::Storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::automations::application::{
        AutomationInvocationAdmissionOutcome, AutomationInvocationAdmissionService,
        IAutomationInvocationAdmission,
    };
    use crate::modules::automations::domain::{
        AutomationScheduleInvocationFactory, AutomationScheduleInvocationRequest,
    };
    use a3s_cloud_contracts::{
        AutomationDefinitionV1, AutomationInvocationAuthorizationV1, AutomationInvocationInputV1,
        AutomationRevisionV1,
    };
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    const SCHEDULE_DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-schedule.acl"
    ));

    #[tokio::test]
    async fn admits_one_exact_invocation_and_replays_without_duplicate_outbox() {
        let repository = std::sync::Arc::new(InMemoryAutomationInvocationRepository::new());
        let service = AutomationInvocationAdmissionService::new(repository.clone());
        let envelope = envelope();

        assert_eq!(
            service.admit(envelope.clone()).await.expect("admission"),
            AutomationInvocationAdmissionOutcome::Admitted
        );
        assert_eq!(
            service.admit(envelope).await.expect("replay"),
            AutomationInvocationAdmissionOutcome::AlreadyAdmitted
        );
        assert_eq!(repository.invocations().await.len(), 1);
        assert_eq!(repository.outbox_messages().await.len(), 1);
        assert_eq!(repository.audit_records().await.len(), 2);
    }

    #[tokio::test]
    async fn rejects_deduplication_key_reuse_by_another_invocation() {
        let repository = std::sync::Arc::new(InMemoryAutomationInvocationRepository::new());
        let service = AutomationInvocationAdmissionService::new(repository);
        let first = envelope();
        service.admit(first.clone()).await.expect("admission");
        let mut second = first;
        second.invocation_id = uuid::Uuid::new_v4();
        assert!(matches!(
            service.admit(second).await,
            Err(crate::modules::shared_kernel::application::ApplicationError::Conflict(message))
                if message.contains("deduplication key")
        ));
    }

    #[tokio::test]
    async fn rejects_immutable_evidence_drift_without_new_side_effects() {
        let repository = std::sync::Arc::new(InMemoryAutomationInvocationRepository::new());
        let service = AutomationInvocationAdmissionService::new(repository.clone());
        let first = envelope();
        service.admit(first.clone()).await.expect("admission");

        let mut drifted = first;
        drifted.requested_at += chrono::Duration::seconds(1);
        assert!(matches!(
            service.admit(drifted).await,
            Err(crate::modules::shared_kernel::application::ApplicationError::Conflict(message))
                if message.contains("different immutable evidence")
        ));
        assert_eq!(repository.invocations().await.len(), 1);
        assert_eq!(repository.outbox_messages().await.len(), 1);
        assert_eq!(repository.audit_records().await.len(), 1);
    }

    #[tokio::test]
    async fn rejects_invalid_envelope_before_repository_side_effects() {
        let repository = std::sync::Arc::new(InMemoryAutomationInvocationRepository::new());
        let service = AutomationInvocationAdmissionService::new(repository.clone());
        let mut invalid = envelope();
        invalid.schema = "cloud.automation.invocation.invalid".into();

        assert!(matches!(
            service.admit(invalid).await,
            Err(crate::modules::shared_kernel::application::ApplicationError::Invalid(_))
        ));
        assert!(repository.invocations().await.is_empty());
        assert!(repository.outbox_messages().await.is_empty());
        assert!(repository.audit_records().await.is_empty());
    }

    #[tokio::test]
    async fn concurrent_exact_admission_has_one_winner_and_one_outbox() {
        let repository = std::sync::Arc::new(InMemoryAutomationInvocationRepository::new());
        let service = std::sync::Arc::new(AutomationInvocationAdmissionService::new(
            repository.clone(),
        ));
        let envelope = envelope();

        let (left, right) = tokio::join!(service.admit(envelope.clone()), service.admit(envelope),);
        let outcomes = [
            left.expect("left admission"),
            right.expect("right admission"),
        ];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == AutomationInvocationAdmissionOutcome::Admitted)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| {
                    **outcome == AutomationInvocationAdmissionOutcome::AlreadyAdmitted
                })
                .count(),
            1
        );
        assert_eq!(repository.invocations().await.len(), 1);
        assert_eq!(repository.outbox_messages().await.len(), 1);
    }

    fn envelope() -> AutomationInvocationEnvelopeV1 {
        let definition =
            AutomationDefinitionV1::parse_acl(SCHEDULE_DEFINITION).expect("definition");
        let revision = AutomationRevisionV1::from_definition(
            uuid::Uuid::new_v4(),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("revision");
        AutomationScheduleInvocationFactory::build(AutomationScheduleInvocationRequest {
            revision: &revision,
            invocation_id: uuid::Uuid::new_v4(),
            scheduled_at: Utc
                .with_ymd_and_hms(2026, 9, 6, 2, 0, 0)
                .single()
                .expect("scheduled time"),
            requested_at: Utc
                .with_ymd_and_hms(2026, 9, 6, 2, 0, 0)
                .single()
                .expect("requested time"),
            input: AutomationInvocationInputV1::inline_json(json!({"source": "test"}))
                .expect("input"),
            authorization: AutomationInvocationAuthorizationV1 {
                policy_digest: revision
                    .spec()
                    .definition
                    .authorization
                    .policy_digest
                    .clone(),
                grant_snapshot_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .into(),
                principal_id: None,
            },
            correlation_id: uuid::Uuid::new_v4(),
            causation_id: None,
        })
        .expect("envelope")
    }
}
