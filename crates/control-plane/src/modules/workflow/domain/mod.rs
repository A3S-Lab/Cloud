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
mod workflow_run_contract;

pub use capability_reference::{CapabilityOwner, CapabilityReference, CapabilityType};
pub use entities::{
    flow_step_id, HumanTask, HumanTaskInteractionSpec, HumanTaskRecord, HumanTaskStatus,
    NewHumanTask, Ontology, OntologyRevision, PlanRevision, WorkflowDecision,
    WorkflowDecisionOutcome, WorkflowDefinition, WorkflowGoal, WorkflowPlan, WorkflowPlanStep,
    WorkflowRevision, WorkflowRun, WorkflowRunFlowState, WorkflowRunStatus, WorkflowStepFlowState,
    WorkflowStepProjection, WorkflowStepProjectionStatus, ONTOLOGY_COMPILER_SCHEMA_VERSION,
    WORKFLOW_COMPILER_SCHEMA_VERSION, WORKFLOW_PLAN_COMPILER_REVISION, WORKFLOW_PLAN_MAX_BYTES,
    WORKFLOW_PLAN_SCHEMA, WORKFLOW_REVISION_MAX_PAYLOADS, WORKFLOW_REVISION_MAX_PAYLOAD_BYTES,
    WORKFLOW_STEP_EVIDENCE_REFERENCE_MAX_BYTES, WORKFLOW_STEP_MAX_EVIDENCE_REFERENCES,
    WORKFLOW_STEP_RESULT_MAX_BYTES,
};
pub use events::{
    HumanTaskStateChanged, OntologyRevisionPublished, WorkflowGoalCompiled,
    WorkflowRevisionPublished, WorkflowRunCancellationRequested, WorkflowRunRequested,
};
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
    CancelWorkflowRunWrite, ChangeHumanTaskWrite, CreateHumanTaskWrite, CreateOntologyWrite,
    CreateWorkflowDefinitionWrite, CreateWorkflowGoalWrite, CreateWorkflowRunWrite,
    DecideHumanTaskWrite, HumanTaskDecisionRecord, HumanTaskResumeDelivery, IHumanTaskRepository,
    IOntologyRepository, IWorkflowDefinitionRepository, IWorkflowGoalRepository,
    IWorkflowRunRepository, OntologyRecord, ReviseOntologyWrite, ReviseWorkflowDefinitionWrite,
    WorkflowDefinitionRecord, WorkflowGoalRecord, WorkflowRunRecord,
};
pub use services::{
    diff_ontology_contracts, resolve_migration_policy, CompiledWorkflowGoal, CompiledWorkflowRun,
    IWorkflowRunCoordinator, IWorkflowRunHistoryReader, OntologyChange,
    OntologyChangeCompatibility, OntologyChangeKind, OntologyDiff, OntologyResourceKind,
    WorkflowPlanCompiler, WorkflowRunCompiler, WorkflowRunCoordinationError,
    WorkflowRunHistoryEvent, WorkflowRunHistoryPage,
};
pub use value_objects::{
    AssignmentPolicyRef, OntologyMigrationPolicy, OntologyName,
    WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_DIGEST,
    WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_ID,
    WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_REVISION,
};
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
pub use workflow_run_contract::{
    workflow_run_timeout_seconds, ResolvedWorkflowPayload, ResolvedWorkflowRunStep,
    WorkflowHumanDecisionHookMetadata, WorkflowRunInput, WORKFLOW_HUMAN_DECISION_HOOK_SCHEMA,
    WORKFLOW_HUMAN_DECISION_STEP_ATTEMPT, WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS,
    WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION, WORKFLOW_RUN_INPUT_MAX_BYTES,
    WORKFLOW_RUN_INPUT_SCHEMA, WORKFLOW_RUN_MAX_TIMEOUT_SECONDS, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
};

#[cfg(test)]
mod authority_tests;
#[cfg(test)]
mod human_task_contract_tests;
