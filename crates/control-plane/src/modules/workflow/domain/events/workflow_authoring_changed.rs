use crate::modules::workflow::domain::{
    WorkflowAuthoringEntry, WorkflowAuthoringJournalKey, WorkflowAuthoringSnapshot,
};
use a3s_cloud_contracts::{CloudScopeRef, DomainEventEnvelope};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Payload emitted when a hosted authoring journal is created.
///
/// Snapshot and operation bodies are intentionally absent. Consumers can use
/// the digest as a projection invalidation key and fetch the authorized
/// snapshot through the owning application port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAuthoringJournalCreated {
    pub project_id: Uuid,
    pub workflow_definition_id: Uuid,
    pub snapshot_digest: String,
}

impl WorkflowAuthoringJournalCreated {
    pub fn envelope(
        key: WorkflowAuthoringJournalKey,
        snapshot: &WorkflowAuthoringSnapshot,
        correlation_id: Uuid,
        occurred_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            project_id: key.project_id.as_uuid(),
            workflow_definition_id: key.workflow_definition_id.as_uuid(),
            snapshot_digest: snapshot.snapshot_digest().as_str().to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workflow.authoring.created".into(),
            schema_version: 1,
            scope: CloudScopeRef::Organization {
                organization_id: key.organization_id.as_uuid(),
            },
            aggregate_id: key.workflow_definition_id.as_uuid(),
            aggregate_version: 1,
            occurred_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}

/// Payload emitted after one new authoring operation is durably appended.
///
/// The event is a bounded invalidation/notification fact. It does not carry
/// opaque DSL bytes or a complete snapshot, so an event consumer cannot become
/// a second authoring-state authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowAuthoringOperationAppended {
    pub project_id: Uuid,
    pub workflow_definition_id: Uuid,
    pub sequence: u64,
    pub operation_id: String,
    pub operation_digest: String,
    pub base_snapshot_digest: String,
    pub result_snapshot_digest: String,
}

impl WorkflowAuthoringOperationAppended {
    pub fn envelope(
        key: WorkflowAuthoringJournalKey,
        entry: &WorkflowAuthoringEntry,
        aggregate_version: u64,
        correlation_id: Uuid,
        occurred_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            project_id: key.project_id.as_uuid(),
            workflow_definition_id: key.workflow_definition_id.as_uuid(),
            sequence: entry.sequence(),
            operation_id: entry.operation_id().to_owned(),
            operation_digest: entry.operation_digest().as_str().to_owned(),
            base_snapshot_digest: entry.base_snapshot_digest().as_str().to_owned(),
            result_snapshot_digest: entry.result_snapshot_digest().as_str().to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workflow.authoring.operation-appended".into(),
            schema_version: 1,
            scope: CloudScopeRef::Organization {
                organization_id: key.organization_id.as_uuid(),
            },
            aggregate_id: key.workflow_definition_id.as_uuid(),
            aggregate_version,
            occurred_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        OrganizationId, ProjectId, Sha256Digest, WorkflowDefinitionId,
    };
    use crate::modules::workflow::domain::WorkflowAuthoringOperation;
    use chrono::Utc;

    fn key() -> WorkflowAuthoringJournalKey {
        WorkflowAuthoringJournalKey::new(
            OrganizationId::new(),
            ProjectId::new(),
            WorkflowDefinitionId::new(),
        )
    }

    #[test]
    fn envelopes_are_valid_and_payloads_are_bounded_to_digests() {
        let key = key();
        let snapshot =
            WorkflowAuthoringSnapshot::try_from_bytes(b"initial".to_vec()).expect("snapshot");
        let created =
            WorkflowAuthoringJournalCreated::envelope(key, &snapshot, Uuid::now_v7(), Utc::now())
                .expect("created event");
        assert_eq!(created.validate(), Ok(()));
        assert_eq!(
            created.payload["snapshotDigest"],
            snapshot.snapshot_digest().as_str()
        );
        assert!(!created.payload.to_string().contains("initial"));

        let operation = WorkflowAuthoringOperation::try_new(
            "op-1",
            snapshot.snapshot_digest().clone(),
            b"opaque".to_vec(),
        )
        .expect("operation");
        let entry = WorkflowAuthoringEntry::try_from_parts(
            1,
            operation.operation_id().to_owned(),
            operation.operation_digest().clone(),
            operation.base_snapshot_digest().clone(),
            Sha256Digest::from_bytes(b"next"),
            operation.operation_bytes().to_vec(),
        )
        .expect("entry");
        let appended = WorkflowAuthoringOperationAppended::envelope(
            key,
            &entry,
            2,
            Uuid::now_v7(),
            Utc::now(),
        )
        .expect("append event");
        assert_eq!(appended.validate(), Ok(()));
        assert!(!appended.payload.to_string().contains("opaque"));
    }
}
