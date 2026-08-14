mod human_task_response;
mod workflow_node_catalog_response;
mod workflow_response;
mod workflow_run_response;

use crate::modules::workflow::application::queries::diff_ontology_revisions::OntologyRevisionDiff;
use crate::modules::workflow::application::OntologyMutationResult;
use crate::modules::workflow::domain::{
    Ontology, OntologyDiff, OntologyMigrationPolicy, OntologyRevision,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub use human_task_response::{
    HumanTaskMutationResponse, HumanTaskResponse, HumanTaskSummaryResponse,
};
pub use workflow_node_catalog_response::WorkflowNodeCatalogResponse;
pub use workflow_response::{
    PlanRevisionResponse, WorkflowDefinitionMutationResponse, WorkflowDefinitionResponse,
    WorkflowGoalMutationResponse, WorkflowGoalResponse, WorkflowRevisionResponse,
    WorkflowRevisionSummaryResponse,
};
pub use workflow_run_response::{
    WorkflowRunMutationResponse, WorkflowRunOutputResponse, WorkflowRunResponse,
    WorkflowRunVariableInspectionResponse,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub current_revision_id: Uuid,
    pub current_revision_number: u64,
    pub current_revision_digest: String,
    pub aggregate_version: u64,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Ontology> for OntologyResponse {
    fn from(value: Ontology) -> Self {
        Self {
            organization_id: value.organization_id.as_uuid(),
            project_id: value.project_id.as_uuid(),
            id: value.id.as_uuid(),
            name: value.name.as_str().to_owned(),
            description: value.description,
            current_revision_id: value.current_revision_id.as_uuid(),
            current_revision_number: value.current_revision_number,
            current_revision_digest: value.current_revision_digest.as_str().to_owned(),
            aggregate_version: value.aggregate_version,
            created_by: value.created_by.as_uuid(),
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyMigrationPolicyResponse {
    pub kind: String,
    pub rule_id: Option<String>,
    pub expression_digest: Option<String>,
}

impl From<&OntologyMigrationPolicy> for OntologyMigrationPolicyResponse {
    fn from(value: &OntologyMigrationPolicy) -> Self {
        Self {
            kind: value.kind().to_owned(),
            rule_id: value.rule_id().map(str::to_owned),
            expression_digest: value
                .expression_digest()
                .map(|digest| digest.as_str().to_owned()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyRevisionSummaryResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub ontology_id: Uuid,
    pub id: Uuid,
    pub revision_number: u64,
    pub parent_revision_id: Option<Uuid>,
    pub parent_digest: Option<String>,
    pub contract_schema: String,
    pub compiler_schema_version: u32,
    pub content_digest: String,
    pub migration_policy: OntologyMigrationPolicyResponse,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<&OntologyRevision> for OntologyRevisionSummaryResponse {
    fn from(value: &OntologyRevision) -> Self {
        Self {
            organization_id: value.organization_id.as_uuid(),
            project_id: value.project_id.as_uuid(),
            ontology_id: value.ontology_id.as_uuid(),
            id: value.id.as_uuid(),
            revision_number: value.revision_number,
            parent_revision_id: value.parent_revision_id.map(|id| id.as_uuid()),
            parent_digest: value.parent_digest.as_ref().map(ToString::to_string),
            contract_schema: value.contract_schema().to_owned(),
            compiler_schema_version: value.compiler_schema_version,
            content_digest: value.contract.digest().as_str().to_owned(),
            migration_policy: OntologyMigrationPolicyResponse::from(&value.migration_policy),
            created_by: value.created_by.as_uuid(),
            created_at: value.created_at,
        }
    }
}

impl From<OntologyRevision> for OntologyRevisionSummaryResponse {
    fn from(value: OntologyRevision) -> Self {
        Self::from(&value)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyRevisionResponse {
    #[serde(flatten)]
    pub summary: OntologyRevisionSummaryResponse,
    pub canonical_acl: String,
}

impl From<OntologyRevision> for OntologyRevisionResponse {
    fn from(value: OntologyRevision) -> Self {
        Self {
            summary: OntologyRevisionSummaryResponse::from(&value),
            canonical_acl: value.contract.canonical_acl().to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyMutationResponse {
    pub ontology: OntologyResponse,
    pub revision: OntologyRevisionResponse,
    pub diff: Option<OntologyDiff>,
    pub replayed: bool,
}

impl From<OntologyMutationResult> for OntologyMutationResponse {
    fn from(value: OntologyMutationResult) -> Self {
        Self {
            ontology: OntologyResponse::from(value.record.ontology),
            revision: OntologyRevisionResponse::from(value.record.revision),
            diff: value.diff,
            replayed: value.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OntologyDiffResponse {
    pub ontology_id: Uuid,
    pub from_revision_id: Uuid,
    pub to_revision_id: Uuid,
    #[serde(flatten)]
    pub diff: OntologyDiff,
}

impl From<OntologyRevisionDiff> for OntologyDiffResponse {
    fn from(value: OntologyRevisionDiff) -> Self {
        Self {
            ontology_id: value.ontology_id.as_uuid(),
            from_revision_id: value.from_revision_id.as_uuid(),
            to_revision_id: value.to_revision_id.as_uuid(),
            diff: value.diff,
        }
    }
}
