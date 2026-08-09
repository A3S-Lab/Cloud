mod capability_reference;
pub mod entities;
pub mod events;
mod flow_resume;
mod ontology_contract;
pub mod repositories;
pub mod services;
mod validation;
pub mod value_objects;
mod workflow_contract;
mod workflow_goal_contract;
mod workflow_graph;
mod workflow_payload;

pub use capability_reference::{CapabilityOwner, CapabilityReference, CapabilityType};
pub use entities::{
    HumanTask, HumanTaskStatus, NewHumanTask, Ontology, OntologyRevision, PlanRevision,
    WorkflowDecision, WorkflowDecisionOutcome, WorkflowDefinition, WorkflowGoal, WorkflowPlan,
    WorkflowPlanStep, WorkflowRevision, ONTOLOGY_COMPILER_SCHEMA_VERSION,
    WORKFLOW_COMPILER_SCHEMA_VERSION, WORKFLOW_PLAN_COMPILER_REVISION, WORKFLOW_PLAN_MAX_BYTES,
    WORKFLOW_PLAN_SCHEMA, WORKFLOW_REVISION_MAX_PAYLOADS, WORKFLOW_REVISION_MAX_PAYLOAD_BYTES,
};
pub use events::{OntologyRevisionPublished, WorkflowGoalCompiled, WorkflowRevisionPublished};
pub use flow_resume::{
    FlowResumePayload, FlowResumeReceipt, FLOW_RESUME_PAYLOAD_API_VERSION,
    FLOW_RESUME_RECEIPT_API_VERSION,
};
pub use ontology_contract::{
    OntologyContract, OntologyContractQuotas, OntologyObjectType, OntologyRelationCardinality,
    OntologyRelationType, OntologyRule, OntologyRuleKind, OntologySpec, ONTOLOGY_MAX_ACL_BYTES,
    ONTOLOGY_SCHEMA,
};
pub use repositories::{
    CreateOntologyWrite, CreateWorkflowDefinitionWrite, CreateWorkflowGoalWrite,
    IOntologyRepository, IWorkflowDefinitionRepository, IWorkflowGoalRepository, OntologyRecord,
    ReviseOntologyWrite, ReviseWorkflowDefinitionWrite, WorkflowDefinitionRecord,
    WorkflowGoalRecord,
};
pub use services::{
    diff_ontology_contracts, resolve_migration_policy, CompiledWorkflowGoal, OntologyChange,
    OntologyChangeCompatibility, OntologyChangeKind, OntologyDiff, OntologyResourceKind,
    WorkflowPlanCompiler,
};
pub use value_objects::{AssignmentPolicyRef, OntologyMigrationPolicy, OntologyName};
pub use workflow_contract::{
    WorkflowContract, WorkflowContractQuotas, WorkflowEdgeSpec, WorkflowSpec, WorkflowStepKind,
    WorkflowStepSpec, WORKFLOW_DEFINITION_SCHEMA,
};
pub use workflow_goal_contract::{
    WorkflowGoalContract, WorkflowGoalSpec, WORKFLOW_GOAL_MAX_ACL_BYTES,
    WORKFLOW_GOAL_MAX_INPUT_BYTES, WORKFLOW_GOAL_SCHEMA,
};
pub use workflow_payload::{
    WorkflowBranchRoute, WorkflowDataField, WorkflowDataSchema, WorkflowDataType, WorkflowPayload,
    WorkflowPayloadContent, WorkflowPayloadKind, WorkflowPolicy, WorkflowPolicyCandidate,
    WorkflowPolicyMode, WorkflowStepConfiguration, WORKFLOW_CONFIGURATION_SCHEMA,
    WORKFLOW_DATA_SCHEMA, WORKFLOW_PAYLOAD_MAX_ACL_BYTES, WORKFLOW_POLICY_SCHEMA,
};

#[cfg(test)]
mod authority_tests;
#[cfg(test)]
mod human_task_contract_tests;
