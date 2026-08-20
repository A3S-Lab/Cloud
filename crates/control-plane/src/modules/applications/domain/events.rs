use super::{Application, ApplicationRelease, APPLICATION_RELEASE_CONTRACT_SCHEMA};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationReleasePublished {
    pub project_id: Uuid,
    pub application_id: Uuid,
    pub release_id: Uuid,
    pub release_number: u64,
    pub parent_release_id: Option<Uuid>,
    pub experience: String,
    pub contract_schema: String,
    pub contract_digest: String,
    pub workflow_definition_id: Uuid,
    pub workflow_revision_id: Uuid,
    pub workflow_contract_digest: String,
    pub workflow_payload_set_digest: String,
    pub workflow_semantic_contract_set_digest: String,
    pub input_schema_digest: String,
    pub output_schema_digest: String,
    pub presentation_digest: String,
}

impl ApplicationReleasePublished {
    pub fn created(
        application: &Application,
        release: &ApplicationRelease,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(
            "application.release.created",
            application,
            release,
            correlation_id,
        )
    }

    pub fn published(
        application: &Application,
        release: &ApplicationRelease,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        Self::envelope(
            "application.release.published",
            application,
            release,
            correlation_id,
        )
    }

    fn envelope(
        event_key: &'static str,
        application: &Application,
        release: &ApplicationRelease,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        if correlation_id.is_nil() {
            return Err("Application event correlation identity is invalid".into());
        }
        application.validate()?;
        release.validate()?;
        let workflow = &release.contract.spec().workflow;
        let payload = Self {
            project_id: application.project_id.as_uuid(),
            application_id: application.id.as_uuid(),
            release_id: release.id.as_uuid(),
            release_number: release.release_number,
            parent_release_id: release.parent_release_id.map(|id| id.as_uuid()),
            experience: application.experience.as_str().into(),
            contract_schema: APPLICATION_RELEASE_CONTRACT_SCHEMA.into(),
            contract_digest: release.contract.digest().as_str().into(),
            workflow_definition_id: workflow.workflow_definition_id.as_uuid(),
            workflow_revision_id: workflow.workflow_revision_id.as_uuid(),
            workflow_contract_digest: workflow.workflow_contract_digest.as_str().into(),
            workflow_payload_set_digest: workflow.workflow_payload_set_digest.as_str().into(),
            workflow_semantic_contract_set_digest: workflow
                .workflow_semantic_contract_set_digest
                .as_str()
                .into(),
            input_schema_digest: workflow.input_schema_digest.as_str().into(),
            output_schema_digest: workflow.output_schema_digest.as_str().into(),
            presentation_digest: release.contract.spec().presentation_digest.as_str().into(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: application.organization_id.as_uuid(),
            aggregate_id: application.id.as_uuid(),
            aggregate_version: application.aggregate_version,
            occurred_at: application.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)
                .map_err(|error| format!("serialize Application release event: {error}"))?,
        })
    }
}
