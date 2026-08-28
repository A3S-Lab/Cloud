use super::{
    DurableCellApplication, DurableCellApplicationRevision, DurableCellProjectionIdentity,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellApplicationChanged {
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub application_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub definition_digest: String,
    pub desired_state: String,
    pub storage_namespace_id: Uuid,
    pub workload_id: Uuid,
    pub workload_revision_id: Uuid,
    pub deployment_id: Uuid,
    pub operation_id: Uuid,
}

impl DurableCellApplicationChanged {
    pub fn created(
        application: &DurableCellApplication,
        revision: &DurableCellApplicationRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(
            "durable-cell.application.created",
            application,
            revision,
            correlation_id,
        )
    }

    pub fn revised(
        application: &DurableCellApplication,
        revision: &DurableCellApplicationRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(
            "durable-cell.application.revised",
            application,
            revision,
            correlation_id,
        )
    }

    pub fn state_requested(
        application: &DurableCellApplication,
        revision: &DurableCellApplicationRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(
            "durable-cell.application.state-requested",
            application,
            revision,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &'static str,
        application: &DurableCellApplication,
        revision: &DurableCellApplicationRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        if correlation_id.is_nil() {
            return Err("Durable Cell event correlation identity is invalid".into());
        }
        let projection =
            DurableCellProjectionIdentity::for_current_revision(application, revision)?;
        let payload = Self {
            project_id: application.project_id.as_uuid(),
            environment_id: application.environment_id.as_uuid(),
            application_id: application.id.as_uuid(),
            revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            definition_digest: revision.definition.digest().as_str().into(),
            desired_state: application.desired_state.as_str().into(),
            storage_namespace_id: projection.storage_namespace_id.as_uuid(),
            workload_id: projection.workload_id.as_uuid(),
            workload_revision_id: projection.workload_revision_id.as_uuid(),
            deployment_id: projection.deployment_id.as_uuid(),
            operation_id: projection.operation_id.as_uuid(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: application.organization_id.as_uuid(),
            },
            aggregate_id: application.id.as_uuid(),
            aggregate_version: application.aggregate_version,
            occurred_at: application.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)
                .map_err(|error| format!("serialize Durable Cell event: {error}"))?,
        })
    }
}
