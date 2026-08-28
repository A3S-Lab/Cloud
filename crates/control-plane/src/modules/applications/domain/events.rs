use super::{Application, ApplicationRelease, APPLICATION_RELEASE_CONTRACT_SCHEMA};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationReleasePublished {
    pub project_id: Uuid,
    pub application_id: Uuid,
    pub release_id: Uuid,
    pub release_number: u64,
    pub parent_release_id: Option<Uuid>,
    pub parent_digest: Option<String>,
    pub experience: String,
    pub audience: String,
    pub interaction_mode: String,
    pub response_modes: Vec<String>,
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
    pub fn published(
        application: &Application,
        release: &ApplicationRelease,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        application.validate()?;
        release.validate()?;
        if correlation_id.is_nil()
            || application.organization_id != release.organization_id
            || application.project_id != release.project_id
            || application.id != release.application_id
            || application.current_release_id != release.id
            || application.current_release_number != release.release_number
            || &application.current_release_digest != release.contract.digest()
            || application.aggregate_version != release.release_number
            || application.experience != release.contract.spec().experience
            || application.updated_at != release.created_at
        {
            return Err("Application release event does not match the current head".into());
        }
        let spec = release.contract.spec();
        let payload = Self {
            project_id: application.project_id.as_uuid(),
            application_id: application.id.as_uuid(),
            release_id: release.id.as_uuid(),
            release_number: release.release_number,
            parent_release_id: release.parent_release_id.map(|id| id.as_uuid()),
            parent_digest: release
                .parent_digest
                .as_ref()
                .map(|value| value.as_str().into()),
            experience: spec.experience.as_str().into(),
            audience: spec.audience.as_str().into(),
            interaction_mode: spec.delivery.interaction_mode.as_str().into(),
            response_modes: spec
                .delivery
                .response_modes
                .iter()
                .map(|mode| mode.as_str().to_owned())
                .collect(),
            contract_schema: APPLICATION_RELEASE_CONTRACT_SCHEMA.into(),
            contract_digest: release.contract.digest().as_str().into(),
            workflow_definition_id: spec.workflow.workflow_definition_id.as_uuid(),
            workflow_revision_id: spec.workflow.workflow_revision_id.as_uuid(),
            workflow_contract_digest: spec.workflow.workflow_contract_digest.as_str().into(),
            workflow_payload_set_digest: spec.workflow.workflow_payload_set_digest.as_str().into(),
            workflow_semantic_contract_set_digest: spec
                .workflow
                .workflow_semantic_contract_set_digest
                .as_str()
                .into(),
            input_schema_digest: spec.workflow.input_schema_digest.as_str().into(),
            output_schema_digest: spec.workflow.output_schema_digest.as_str().into(),
            presentation_digest: spec.presentation_digest.as_str().into(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "application.release.published".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: application.organization_id.as_uuid(),
            },
            aggregate_id: application.id.as_uuid(),
            aggregate_version: application.aggregate_version,
            occurred_at: release.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)
                .map_err(|error| format!("serialize Application release event: {error}"))?,
        })
    }
}
