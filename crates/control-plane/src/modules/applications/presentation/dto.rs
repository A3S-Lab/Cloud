use crate::modules::applications::application::ApplicationMutationResult;
use crate::modules::applications::domain::{Application, ApplicationRecord, ApplicationRelease};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateApplicationRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub release_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishApplicationReleaseRequest {
    pub expected_version: u64,
    pub release_acl: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub application_id: Uuid,
    pub name: String,
    pub description: String,
    pub experience: String,
    pub current_release_id: Uuid,
    pub current_release_number: u64,
    pub current_release_digest: String,
    pub aggregate_version: u64,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Application> for ApplicationResponse {
    fn from(application: Application) -> Self {
        Self {
            organization_id: application.organization_id.as_uuid(),
            project_id: application.project_id.as_uuid(),
            application_id: application.id.as_uuid(),
            name: application.name.as_str().to_owned(),
            description: application.description,
            experience: application.experience.as_str().to_owned(),
            current_release_id: application.current_release_id.as_uuid(),
            current_release_number: application.current_release_number,
            current_release_digest: application.current_release_digest.as_str().to_owned(),
            aggregate_version: application.aggregate_version,
            created_by: application.created_by.as_uuid(),
            created_at: application.created_at,
            updated_at: application.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationReleaseResponse {
    pub organization_id: Uuid,
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
    pub contract_acl: String,
    pub contract_digest: String,
    pub workflow_definition_id: Uuid,
    pub workflow_revision_id: Uuid,
    pub workflow_contract_digest: String,
    pub workflow_payload_set_digest: String,
    pub workflow_semantic_contract_set_digest: String,
    pub input_schema_digest: String,
    pub output_schema_digest: String,
    pub presentation_digest: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<ApplicationRelease> for ApplicationReleaseResponse {
    fn from(release: ApplicationRelease) -> Self {
        let spec = release.contract.spec();
        Self {
            organization_id: release.organization_id.as_uuid(),
            project_id: release.project_id.as_uuid(),
            application_id: release.application_id.as_uuid(),
            release_id: release.id.as_uuid(),
            release_number: release.release_number,
            parent_release_id: release.parent_release_id.map(|value| value.as_uuid()),
            parent_digest: release.parent_digest.map(|value| value.as_str().to_owned()),
            experience: spec.experience.as_str().to_owned(),
            audience: spec.audience.as_str().to_owned(),
            interaction_mode: spec.delivery.interaction_mode.as_str().to_owned(),
            response_modes: spec
                .delivery
                .response_modes
                .iter()
                .map(|mode| mode.as_str().to_owned())
                .collect(),
            contract_schema:
                crate::modules::applications::domain::APPLICATION_RELEASE_CONTRACT_SCHEMA.into(),
            contract_acl: release.contract.canonical_acl().to_owned(),
            contract_digest: release.contract.digest().as_str().to_owned(),
            workflow_definition_id: spec.workflow.workflow_definition_id.as_uuid(),
            workflow_revision_id: spec.workflow.workflow_revision_id.as_uuid(),
            workflow_contract_digest: spec.workflow.workflow_contract_digest.as_str().to_owned(),
            workflow_payload_set_digest: spec
                .workflow
                .workflow_payload_set_digest
                .as_str()
                .to_owned(),
            workflow_semantic_contract_set_digest: spec
                .workflow
                .workflow_semantic_contract_set_digest
                .as_str()
                .to_owned(),
            input_schema_digest: spec.workflow.input_schema_digest.as_str().to_owned(),
            output_schema_digest: spec.workflow.output_schema_digest.as_str().to_owned(),
            presentation_digest: spec.presentation_digest.as_str().to_owned(),
            created_by: release.created_by.as_uuid(),
            created_at: release.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRecordResponse {
    pub application: ApplicationResponse,
    pub release: ApplicationReleaseResponse,
}

impl From<ApplicationRecord> for ApplicationRecordResponse {
    fn from(record: ApplicationRecord) -> Self {
        Self {
            application: record.application.into(),
            release: record.release.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationMutationResponse {
    pub record: ApplicationRecordResponse,
    pub replayed: bool,
}

impl From<ApplicationMutationResult> for ApplicationMutationResponse {
    fn from(result: ApplicationMutationResult) -> Self {
        Self {
            record: result.record.into(),
            replayed: result.replayed,
        }
    }
}
