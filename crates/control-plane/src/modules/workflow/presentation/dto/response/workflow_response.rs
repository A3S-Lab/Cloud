use crate::modules::workflow::application::{
    WorkflowDefinitionMutationResult, WorkflowGoalMutationResult,
};
use crate::modules::workflow::domain::{
    CapabilityReference, PlanRevision, WorkflowDefinition, WorkflowEdgeSpec, WorkflowGoalRecord,
    WorkflowPayload, WorkflowPlan, WorkflowPlanStep, WorkflowRevision,
    WorkflowStepDescriptorBinding,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinitionResponse {
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

impl From<WorkflowDefinition> for WorkflowDefinitionResponse {
    fn from(value: WorkflowDefinition) -> Self {
        Self {
            organization_id: value.organization_id.as_uuid(),
            project_id: value.project_id.as_uuid(),
            id: value.id.as_uuid(),
            name: value.name,
            description: value.description,
            current_revision_id: value.current_revision_id.as_uuid(),
            current_revision_number: value.current_revision_number,
            current_revision_digest: value.current_revision_digest.to_string(),
            aggregate_version: value.aggregate_version,
            created_by: value.created_by.as_uuid(),
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowPayloadResponse {
    pub kind: String,
    pub schema: String,
    pub digest: String,
    pub canonical_acl: String,
}

impl From<WorkflowPayload> for WorkflowPayloadResponse {
    fn from(value: WorkflowPayload) -> Self {
        Self {
            kind: value.kind().as_str().to_owned(),
            schema: value.schema().to_owned(),
            digest: value.digest().to_string(),
            canonical_acl: value.canonical_acl().to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowCapabilityReferenceResponse {
    pub owner: String,
    #[serde(rename = "type")]
    pub capability_type: String,
    pub resource_id: Uuid,
    pub revision: String,
    pub digest: String,
    pub capability: String,
}

impl From<CapabilityReference> for WorkflowCapabilityReferenceResponse {
    fn from(value: CapabilityReference) -> Self {
        Self {
            owner: value.owner.as_str().to_owned(),
            capability_type: value.capability_type.as_str().to_owned(),
            resource_id: value.resource_id,
            revision: value.revision,
            digest: value.digest.to_string(),
            capability: value.capability,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowPlanStepResponse {
    pub id: String,
    pub kind: String,
    pub configuration_digest: String,
    pub input_schema_digest: String,
    pub output_schema_digest: String,
    pub policy_digest: Option<String>,
    pub capability: Option<WorkflowCapabilityReferenceResponse>,
    pub descriptor: Option<WorkflowStepDescriptorBindingResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStepDescriptorBindingResponse {
    pub step_id: String,
    pub descriptor_id: String,
    pub descriptor_revision: String,
    pub semantic_digest: String,
}

impl From<WorkflowStepDescriptorBinding> for WorkflowStepDescriptorBindingResponse {
    fn from(value: WorkflowStepDescriptorBinding) -> Self {
        Self {
            step_id: value.step_id,
            descriptor_id: value.descriptor_id,
            descriptor_revision: value.descriptor_revision,
            semantic_digest: value.semantic_digest.to_string(),
        }
    }
}

impl From<WorkflowPlanStep> for WorkflowPlanStepResponse {
    fn from(value: WorkflowPlanStep) -> Self {
        Self {
            id: value.id,
            kind: value.kind.as_str().to_owned(),
            configuration_digest: value.configuration_digest.to_string(),
            input_schema_digest: value.input_schema_digest.to_string(),
            output_schema_digest: value.output_schema_digest.to_string(),
            policy_digest: value.policy_digest.map(|digest| digest.to_string()),
            capability: value
                .capability
                .map(WorkflowCapabilityReferenceResponse::from),
            descriptor: value
                .descriptor
                .map(WorkflowStepDescriptorBindingResponse::from),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowPlanEdgeResponse {
    pub id: String,
    pub source: String,
    pub target: String,
    pub source_handle: Option<String>,
}

impl From<WorkflowEdgeSpec> for WorkflowPlanEdgeResponse {
    fn from(value: WorkflowEdgeSpec) -> Self {
        Self {
            id: value.id,
            source: value.source,
            target: value.target,
            source_handle: value.source_handle,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowPlanResponse {
    pub schema: String,
    pub compiler_revision: String,
    pub workflow_definition_id: Uuid,
    pub workflow_revision_id: Uuid,
    pub workflow_digest: String,
    pub workflow_payload_set_digest: String,
    pub semantic_contract_set_digest: Option<String>,
    pub variable_contract_digest: Option<String>,
    pub composite_regions_digest: Option<String>,
    pub ontology_id: Uuid,
    pub ontology_revision_id: Uuid,
    pub ontology_digest: String,
    pub environment_id: Option<Uuid>,
    pub input_digest: String,
    pub steps: Vec<WorkflowPlanStepResponse>,
    pub edges: Vec<WorkflowPlanEdgeResponse>,
}

impl From<WorkflowPlan> for WorkflowPlanResponse {
    fn from(value: WorkflowPlan) -> Self {
        Self {
            schema: value.schema,
            compiler_revision: value.compiler_revision,
            workflow_definition_id: value.workflow_definition_id.as_uuid(),
            workflow_revision_id: value.workflow_revision_id.as_uuid(),
            workflow_digest: value.workflow_digest.to_string(),
            workflow_payload_set_digest: value.workflow_payload_set_digest.to_string(),
            semantic_contract_set_digest: value
                .semantic_contract_set_digest
                .map(|digest| digest.to_string()),
            variable_contract_digest: value
                .variable_contract_digest
                .map(|digest| digest.to_string()),
            composite_regions_digest: value
                .composite_regions_digest
                .map(|digest| digest.to_string()),
            ontology_id: value.ontology_id.as_uuid(),
            ontology_revision_id: value.ontology_revision_id.as_uuid(),
            ontology_digest: value.ontology_digest.to_string(),
            environment_id: value.environment_id.map(|id| id.as_uuid()),
            input_digest: value.input_digest.to_string(),
            steps: value
                .steps
                .into_iter()
                .map(WorkflowPlanStepResponse::from)
                .collect(),
            edges: value
                .edges
                .into_iter()
                .map(WorkflowPlanEdgeResponse::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRevisionSummaryResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub workflow_definition_id: Uuid,
    pub id: Uuid,
    pub revision_number: u64,
    pub parent_revision_id: Option<Uuid>,
    pub parent_digest: Option<String>,
    pub contract_schema: String,
    pub compiler_schema_version: u32,
    pub content_digest: String,
    pub payload_set_digest: String,
    pub payload_count: usize,
    pub semantic_contract_set_digest: Option<String>,
    pub semantic_contract_count: usize,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<&WorkflowRevision> for WorkflowRevisionSummaryResponse {
    fn from(value: &WorkflowRevision) -> Self {
        Self {
            organization_id: value.organization_id.as_uuid(),
            project_id: value.project_id.as_uuid(),
            workflow_definition_id: value.workflow_definition_id.as_uuid(),
            id: value.id.as_uuid(),
            revision_number: value.revision_number,
            parent_revision_id: value.parent_revision_id.map(|id| id.as_uuid()),
            parent_digest: value.parent_digest.as_ref().map(ToString::to_string),
            contract_schema: value.contract_schema().to_owned(),
            compiler_schema_version: value.compiler_schema_version,
            content_digest: value.contract.digest().to_string(),
            payload_set_digest: value.payload_set_digest.to_string(),
            payload_count: value.payloads.len(),
            semantic_contract_set_digest: value
                .semantic_contract_set_digest()
                .map(ToString::to_string),
            semantic_contract_count: value
                .semantic_contracts
                .as_ref()
                .map_or(0, |contracts| contracts.persisted_contracts().len()),
            created_by: value.created_by.as_uuid(),
            created_at: value.created_at,
        }
    }
}

impl From<WorkflowRevision> for WorkflowRevisionSummaryResponse {
    fn from(value: WorkflowRevision) -> Self {
        Self::from(&value)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRevisionResponse {
    #[serde(flatten)]
    pub summary: WorkflowRevisionSummaryResponse,
    pub canonical_definition_acl: String,
    pub payloads: Vec<WorkflowPayloadResponse>,
    pub semantic_contracts: Vec<WorkflowSemanticContractResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSemanticContractResponse {
    pub kind: String,
    pub schema: String,
    pub digest: String,
    pub canonical_acl: String,
}

impl From<WorkflowRevision> for WorkflowRevisionResponse {
    fn from(value: WorkflowRevision) -> Self {
        let summary = WorkflowRevisionSummaryResponse::from(&value);
        let semantic_contracts = value
            .semantic_contracts
            .as_ref()
            .map(|contracts| {
                contracts
                    .persisted_contracts()
                    .into_iter()
                    .map(|contract| WorkflowSemanticContractResponse {
                        kind: contract.kind.as_str().into(),
                        schema: contract.schema.into(),
                        digest: contract.digest.to_string(),
                        canonical_acl: contract.canonical_acl.into(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            summary,
            canonical_definition_acl: value.contract.canonical_acl().to_owned(),
            payloads: value
                .payloads
                .into_iter()
                .map(WorkflowPayloadResponse::from)
                .collect(),
            semantic_contracts,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinitionMutationResponse {
    pub workflow_definition: WorkflowDefinitionResponse,
    pub revision: WorkflowRevisionResponse,
    pub replayed: bool,
}

impl From<WorkflowDefinitionMutationResult> for WorkflowDefinitionMutationResponse {
    fn from(value: WorkflowDefinitionMutationResult) -> Self {
        Self {
            workflow_definition: WorkflowDefinitionResponse::from(value.record.definition),
            revision: WorkflowRevisionResponse::from(value.record.revision),
            replayed: value.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRevisionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub workflow_goal_id: Uuid,
    pub id: Uuid,
    pub schema: String,
    pub compiler_revision: String,
    pub digest: String,
    pub canonical_plan: String,
    pub plan: WorkflowPlanResponse,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<PlanRevision> for PlanRevisionResponse {
    fn from(value: PlanRevision) -> Self {
        let plan = value.plan;
        Self {
            organization_id: value.organization_id.as_uuid(),
            project_id: value.project_id.as_uuid(),
            workflow_goal_id: value.workflow_goal_id.as_uuid(),
            id: value.id.as_uuid(),
            schema: plan.schema.clone(),
            compiler_revision: plan.compiler_revision.clone(),
            digest: value.digest.to_string(),
            canonical_plan: value.canonical_plan,
            plan: WorkflowPlanResponse::from(plan),
            created_by: value.created_by.as_uuid(),
            created_at: value.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowGoalResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub id: Uuid,
    pub name: String,
    pub contract_schema: String,
    pub contract_digest: String,
    pub input_digest: String,
    pub canonical_goal_acl: String,
    pub workflow_definition_id: Uuid,
    pub workflow_revision_id: Uuid,
    pub workflow_digest: String,
    pub ontology_id: Uuid,
    pub ontology_revision_id: Uuid,
    pub ontology_digest: String,
    pub environment_id: Option<Uuid>,
    pub input: serde_json::Value,
    pub plan_revision_id: Uuid,
    pub plan_digest: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<&WorkflowGoalRecord> for WorkflowGoalResponse {
    fn from(value: &WorkflowGoalRecord) -> Self {
        let goal = &value.goal;
        let spec = goal.contract.spec();
        Self {
            organization_id: goal.organization_id.as_uuid(),
            project_id: goal.project_id.as_uuid(),
            id: goal.id.as_uuid(),
            name: spec.name.clone(),
            contract_schema: crate::modules::workflow::domain::WORKFLOW_GOAL_SCHEMA.to_owned(),
            contract_digest: goal.contract.digest().to_string(),
            input_digest: goal.contract.input_digest().to_string(),
            canonical_goal_acl: goal.contract.canonical_acl().to_owned(),
            workflow_definition_id: spec.workflow_definition_id.as_uuid(),
            workflow_revision_id: spec.workflow_revision_id.as_uuid(),
            workflow_digest: spec.workflow_digest.to_string(),
            ontology_id: spec.ontology_id.as_uuid(),
            ontology_revision_id: spec.ontology_revision_id.as_uuid(),
            ontology_digest: spec.ontology_digest.to_string(),
            environment_id: spec.environment_id.map(|id| id.as_uuid()),
            input: spec.input.clone(),
            plan_revision_id: goal.plan_revision_id.as_uuid(),
            plan_digest: goal.plan_digest.to_string(),
            created_by: goal.created_by.as_uuid(),
            created_at: goal.created_at,
        }
    }
}

impl From<WorkflowGoalRecord> for WorkflowGoalResponse {
    fn from(value: WorkflowGoalRecord) -> Self {
        Self::from(&value)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowGoalMutationResponse {
    pub goal: WorkflowGoalResponse,
    pub plan_revision: PlanRevisionResponse,
    pub replayed: bool,
}

impl From<WorkflowGoalMutationResult> for WorkflowGoalMutationResponse {
    fn from(value: WorkflowGoalMutationResult) -> Self {
        Self {
            goal: WorkflowGoalResponse::from(&value.record),
            plan_revision: PlanRevisionResponse::from(value.record.plan_revision),
            replayed: value.replayed,
        }
    }
}
