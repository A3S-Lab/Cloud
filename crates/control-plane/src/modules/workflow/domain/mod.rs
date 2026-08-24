mod capability_reference;
pub mod entities;
pub mod events;
mod flow_resume;
mod ontology_contract;
pub mod repositories;
pub mod services;
mod validation;
pub mod value_objects;
mod workflow_application_answer_hook;
mod workflow_application_frame_authority;
mod workflow_application_variable_hook;
mod workflow_composite_execution;
mod workflow_composite_frame;
mod workflow_composite_region_result;
mod workflow_composite_regions;
mod workflow_connector_execution;
mod workflow_contract;
mod workflow_execution_hook;
mod workflow_failure_routing;
mod workflow_goal_contract;
mod workflow_graph;
mod workflow_human_decision_hook;
mod workflow_list_operator_binding;
mod workflow_node_catalog;
mod workflow_payload;
mod workflow_revision_semantic_contracts;
mod workflow_run_application_projection;
mod workflow_run_contract;
mod workflow_run_variable_runtime;
mod workflow_step_descriptor;
mod workflow_step_descriptor_bindings;
mod workflow_step_evidence_reference;
mod workflow_step_failure;
mod workflow_variable_aggregate_binding;
mod workflow_variable_contract;
mod workflow_variable_defaults;
mod workflow_variable_materialization;

pub(crate) use workflow_failure_routing::{
    descriptor_failure_output, has_application_answer_failure_route,
    has_application_variable_failure_route, has_branch_failure_route, has_composite_failure_route,
    has_connector_failure_route, has_transform_failure_route, has_workflow_output_failure_route,
    validate_descriptor_failure_routes,
};
pub(crate) use workflow_list_operator_binding::validate_list_operator_binding;
pub(crate) use workflow_run_variable_runtime::{
    validate_application_runtime_variable_contract, validate_runtime_variable_contract,
    validate_typed_projection_configurations,
};
pub(crate) use workflow_step_evidence_reference::{
    composite_child_evidence_references, connector_attempt_evidence_references,
    execution_evidence_references, human_decision_evidence_references,
    validate_evidence_references,
};
pub(crate) use workflow_variable_aggregate_binding::validate_variable_aggregate_binding;

pub use capability_reference::{CapabilityOwner, CapabilityReference, CapabilityType};
pub use entities::{
    flow_step_id, HumanTask, HumanTaskInteractionSpec, HumanTaskRecord, HumanTaskStatus,
    NewHumanTask, Ontology, OntologyRevision, PlanRevision, WorkflowDecision,
    WorkflowDecisionOutcome, WorkflowDefinition, WorkflowGoal, WorkflowPlan, WorkflowPlanStep,
    WorkflowRevision, WorkflowRun, WorkflowRunFlowState, WorkflowRunStatus,
    WorkflowStepDefaultOutputContract, WorkflowStepFlowState, WorkflowStepProjection,
    WorkflowStepProjectionStatus, ONTOLOGY_COMPILER_SCHEMA_VERSION,
    WORKFLOW_COMPILER_SCHEMA_VERSION, WORKFLOW_COMPILER_SCHEMA_VERSION_V2,
    WORKFLOW_PLAN_COMPILER_REVISION, WORKFLOW_PLAN_COMPILER_REVISION_V10,
    WORKFLOW_PLAN_COMPILER_REVISION_V11, WORKFLOW_PLAN_COMPILER_REVISION_V2,
    WORKFLOW_PLAN_COMPILER_REVISION_V3, WORKFLOW_PLAN_COMPILER_REVISION_V4,
    WORKFLOW_PLAN_COMPILER_REVISION_V5, WORKFLOW_PLAN_COMPILER_REVISION_V6,
    WORKFLOW_PLAN_COMPILER_REVISION_V7, WORKFLOW_PLAN_COMPILER_REVISION_V8,
    WORKFLOW_PLAN_COMPILER_REVISION_V9, WORKFLOW_PLAN_MAX_BYTES, WORKFLOW_PLAN_SCHEMA,
    WORKFLOW_PLAN_SCHEMA_V10, WORKFLOW_PLAN_SCHEMA_V11, WORKFLOW_PLAN_SCHEMA_V2,
    WORKFLOW_PLAN_SCHEMA_V3, WORKFLOW_PLAN_SCHEMA_V4, WORKFLOW_PLAN_SCHEMA_V5,
    WORKFLOW_PLAN_SCHEMA_V6, WORKFLOW_PLAN_SCHEMA_V7, WORKFLOW_PLAN_SCHEMA_V8,
    WORKFLOW_PLAN_SCHEMA_V9, WORKFLOW_REVISION_MAX_PAYLOADS, WORKFLOW_REVISION_MAX_PAYLOAD_BYTES,
    WORKFLOW_STEP_EVIDENCE_REFERENCE_MAX_BYTES, WORKFLOW_STEP_MAX_EVIDENCE_REFERENCES,
    WORKFLOW_STEP_RESULT_MAX_BYTES,
};
pub use events::{
    HumanTaskStateChanged, OntologyRevisionPublished, WorkflowGoalCompiled,
    WorkflowRevisionPublished, WorkflowRunCancellationRequested, WorkflowRunRequested,
};
pub use flow_resume::{
    FlowResumeDisposition, FlowResumePayload, FlowResumeReceipt, FLOW_RESUME_PAYLOAD_API_VERSION,
    FLOW_RESUME_RECEIPT_API_VERSION, FLOW_RESUME_TERMINAL_RECEIPT_API_VERSION,
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
    diff_ontology_contracts, expected_human_task_expiry, resolve_migration_policy,
    CompiledWorkflowGoal, CompiledWorkflowRun, HumanTaskCancellationAuthority,
    HumanTaskDeadlineAuthority, HumanTaskParentCancellationEvidence, IWorkflowRunCoordinator,
    IWorkflowRunDiagnosticsReader, IWorkflowRunHistoryReader, IWorkflowRunVariableReader,
    OntologyChange, OntologyChangeCompatibility, OntologyChangeKind, OntologyDiff,
    OntologyResourceKind, WorkflowPlanCompiler, WorkflowRunCompiler, WorkflowRunCoordinationError,
    WorkflowRunDiagnostic, WorkflowRunDiagnosticCode, WorkflowRunDiagnosticSeverity,
    WorkflowRunDiagnosticStatus, WorkflowRunDiagnostics, WorkflowRunEvidenceCorrelation,
    WorkflowRunFlowStatistics, WorkflowRunHistoryEvent, WorkflowRunHistoryPage,
    WorkflowRunObservedFlowStatus, WorkflowRunStepStatistics, WorkflowRunVariable,
    WorkflowRunVariableInspection, WorkflowRunVariableState,
    HUMAN_TASK_CANCELLATION_AUTHORITY_API_VERSION, HUMAN_TASK_DEADLINE_AUTHORITY_API_VERSION,
    WORKFLOW_RUN_DIAGNOSTICS_MAX_EVIDENCE_REFERENCES, WORKFLOW_RUN_DIAGNOSTICS_SCHEMA,
    WORKFLOW_RUN_VARIABLE_INSPECTION_MAX_BYTES, WORKFLOW_RUN_VARIABLE_INSPECTION_SCHEMA,
};
pub(crate) use services::{
    inspect_workflow_run_variables, inspect_workflow_run_variables_with_application,
};
pub use value_objects::{
    AssignmentPolicyRef, OntologyMigrationPolicy, OntologyName,
    WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_DIGEST,
    WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_ID,
    WORKFLOW_ORGANIZATION_MEMBER_ASSIGNMENT_POLICY_REVISION,
};
pub use workflow_application_answer_hook::{
    WorkflowApplicationAnswerFailureResumePayload, WorkflowApplicationAnswerHookMetadata,
    WorkflowApplicationAnswerResumePayload, WORKFLOW_APPLICATION_ANSWER_FAILURE_RESUME_SCHEMA,
    WORKFLOW_APPLICATION_ANSWER_FAILURE_RESUME_SCHEMA_V2, WORKFLOW_APPLICATION_ANSWER_HOOK_SCHEMA,
    WORKFLOW_APPLICATION_ANSWER_HOOK_SCHEMA_V2, WORKFLOW_APPLICATION_ANSWER_RESUME_SCHEMA,
    WORKFLOW_APPLICATION_ANSWER_RESUME_SCHEMA_V2, WORKFLOW_APPLICATION_ANSWER_STEP_ATTEMPT,
};
pub use workflow_application_frame_authority::{
    WorkflowApplicationFrameAuthority, WORKFLOW_APPLICATION_FRAME_AUTHORITY_SCHEMA,
};
pub use workflow_application_variable_hook::{
    WorkflowApplicationVariableSnapshotHookMetadata,
    WorkflowApplicationVariableSnapshotResumePayload,
    WorkflowApplicationVariableWriteFailureResumePayload,
    WorkflowApplicationVariableWriteHookMetadata, WorkflowApplicationVariableWriteResumePayload,
    WORKFLOW_APPLICATION_VARIABLE_SNAPSHOT_HOOK_SCHEMA,
    WORKFLOW_APPLICATION_VARIABLE_SNAPSHOT_RESUME_SCHEMA,
    WORKFLOW_APPLICATION_VARIABLE_STEP_ATTEMPT,
    WORKFLOW_APPLICATION_VARIABLE_WRITE_FAILURE_RESUME_SCHEMA,
    WORKFLOW_APPLICATION_VARIABLE_WRITE_HOOK_SCHEMA,
    WORKFLOW_APPLICATION_VARIABLE_WRITE_RESUME_SCHEMA,
};
pub use workflow_composite_execution::{
    WorkflowCompositeChildReferenceMetadata, WorkflowCompositeHookMetadata,
    WorkflowCompositeResumePayload, WORKFLOW_COMPOSITE_CHILD_REFERENCE_SCHEMA,
    WORKFLOW_COMPOSITE_HOOK_SCHEMA, WORKFLOW_COMPOSITE_RESUME_SCHEMA,
};
pub use workflow_composite_frame::{
    WorkflowCompositeFrame, WorkflowCompositeFrameMode, WorkflowCompositeFrameRequest,
    WorkflowCompositeFrameResult, WORKFLOW_COMPOSITE_FRAME_MAX_BYTES,
    WORKFLOW_COMPOSITE_FRAME_RESULT_SCHEMA, WORKFLOW_COMPOSITE_FRAME_SCHEMA,
};
pub use workflow_composite_region_result::{
    WorkflowCompositeFrameResolution, WorkflowCompositeRegionResult,
    WorkflowCompositeRegionResultRequest, WORKFLOW_COMPOSITE_REGION_RESULT_MAX_BYTES,
    WORKFLOW_COMPOSITE_REGION_RESULT_SCHEMA,
};
pub use workflow_composite_regions::{
    WorkflowCompositeRegionPolicy, WorkflowCompositeRegions, WorkflowCompositeRegionsSpec,
    WorkflowIterationFailureMode, WorkflowIterationRegionPolicy, WorkflowLoopRegionPolicy,
    WORKFLOW_COMPOSITE_REGIONS_MAX_ACL_BYTES, WORKFLOW_COMPOSITE_REGIONS_SCHEMA,
    WORKFLOW_COMPOSITE_REGION_MAX_COUNT, WORKFLOW_COMPOSITE_REGION_MAX_TIME_BUDGET_SECONDS,
    WORKFLOW_ITERATION_MAX_CONCURRENCY, WORKFLOW_ITERATION_MAX_ITEMS, WORKFLOW_LOOP_MAX_ITERATIONS,
};
pub use workflow_connector_execution::{
    WorkflowConnectorAttemptEvidence, WorkflowConnectorAttemptOutcome,
    WorkflowConnectorHookMetadata, WorkflowConnectorResponseObjectReference,
    WorkflowConnectorResumePayload, WorkflowConnectorResumeResolution, WorkflowConnectorStepOutput,
    WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA, WORKFLOW_CONNECTOR_EVIDENCE_SCHEMA_V2,
    WORKFLOW_CONNECTOR_HOOK_SCHEMA, WORKFLOW_CONNECTOR_HOOK_SCHEMA_V2,
    WORKFLOW_CONNECTOR_HOOK_SCHEMA_V3, WORKFLOW_CONNECTOR_MAX_OBSERVATIONS_PER_ATTEMPT,
    WORKFLOW_CONNECTOR_RESPONSE_OBJECT_SCHEMA, WORKFLOW_CONNECTOR_RESULT_SCHEMA,
    WORKFLOW_CONNECTOR_RESULT_SCHEMA_V2, WORKFLOW_CONNECTOR_RESUME_SCHEMA,
    WORKFLOW_CONNECTOR_RESUME_SCHEMA_V2,
};
pub use workflow_contract::{
    WorkflowContract, WorkflowContractQuotas, WorkflowEdgeSpec, WorkflowSpec, WorkflowStepKind,
    WorkflowStepSpec, WORKFLOW_DEFINITION_SCHEMA,
};
pub use workflow_execution_hook::{
    WorkflowExecutionChildReferenceMetadata, WorkflowExecutionHookMetadata,
    WorkflowExecutionOutcome, WorkflowExecutionResumePayload, WorkflowExecutionResumeResolution,
    WorkflowExecutionStepOutput, WORKFLOW_EXECUTION_CHILD_REFERENCE_SCHEMA,
    WORKFLOW_EXECUTION_HOOK_SCHEMA, WORKFLOW_EXECUTION_RESULT_SCHEMA,
    WORKFLOW_EXECUTION_RESUME_SCHEMA, WORKFLOW_EXECUTION_STEP_ATTEMPT,
};
pub use workflow_goal_contract::{
    WorkflowGoalContract, WorkflowGoalSpec, WORKFLOW_GOAL_MAX_ACL_BYTES,
    WORKFLOW_GOAL_MAX_INPUT_BYTES, WORKFLOW_GOAL_SCHEMA,
};
pub use workflow_human_decision_hook::{
    WorkflowHumanDecisionHookMetadata, WORKFLOW_HUMAN_DECISION_HOOK_SCHEMA,
    WORKFLOW_HUMAN_DECISION_STEP_ATTEMPT,
};
pub use workflow_node_catalog::{
    WorkflowNodeCatalog, WorkflowNodeCatalogAvailability, WorkflowNodeCatalogEntry,
};
pub use workflow_payload::{
    WorkflowBranchRoute, WorkflowDataField, WorkflowDataSchema, WorkflowDataType,
    WorkflowDefaultOutput, WorkflowListOperatorConfiguration, WorkflowListOperatorExtract,
    WorkflowListOperatorFilterCondition, WorkflowListOperatorFilterOperator,
    WorkflowListOperatorOperand, WorkflowListOperatorOrder, WorkflowListOperatorOrderDirection,
    WorkflowLocalTransformConfiguration, WorkflowPayload, WorkflowPayloadContent,
    WorkflowPayloadKind, WorkflowPolicy, WorkflowPolicyCandidate, WorkflowPolicyMode,
    WorkflowRetryPolicy, WorkflowStepConfiguration, WorkflowVariableAggregateCandidate,
    WorkflowVariableAggregateConfiguration, WorkflowVariableAggregateGroup,
    WORKFLOW_CONFIGURATION_SCHEMA, WORKFLOW_DATA_SCHEMA, WORKFLOW_DEFAULT_OUTPUT_MAX_BYTES,
    WORKFLOW_LIST_OPERATOR_CONFIGURATION_SCHEMA, WORKFLOW_LIST_OPERATOR_MAX_CONDITIONS,
    WORKFLOW_LIST_OPERATOR_MAX_ITEMS, WORKFLOW_PAYLOAD_MAX_ACL_BYTES, WORKFLOW_POLICY_SCHEMA,
    WORKFLOW_POLICY_SCHEMA_V2, WORKFLOW_POLICY_SCHEMA_V3, WORKFLOW_RETRY_MAXIMUM_ATTEMPTS,
    WORKFLOW_RETRY_MAXIMUM_DEFAULT_DELAY_SECONDS, WORKFLOW_VARIABLE_AGGREGATE_CONFIGURATION_SCHEMA,
    WORKFLOW_VARIABLE_AGGREGATE_MAX_CANDIDATES,
    WORKFLOW_VARIABLE_AGGREGATE_MAX_CANDIDATES_PER_GROUP, WORKFLOW_VARIABLE_AGGREGATE_MAX_GROUPS,
};
pub use workflow_revision_semantic_contracts::{
    WorkflowRevisionSemanticContractKind, WorkflowRevisionSemanticContractRef,
    WorkflowRevisionSemanticContracts,
};
pub use workflow_run_application_projection::WorkflowRunApplicationProjection;
pub use workflow_run_contract::{
    workflow_run_timeout_seconds, ResolvedWorkflowCompositeRegions, ResolvedWorkflowPayload,
    ResolvedWorkflowRunStep, ResolvedWorkflowVariableContract, ResolvedWorkflowVariableDefaults,
    WorkflowRunInput, WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA,
    WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V2, WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V3,
    WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V4, WORKFLOW_RUN_APPLICATION_PROJECTION_SCHEMA_V5,
    WORKFLOW_RUN_DEFAULT_TIMEOUT_SECONDS, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION,
    WORKFLOW_RUN_FLOW_VERSION_V10, WORKFLOW_RUN_FLOW_VERSION_V11, WORKFLOW_RUN_FLOW_VERSION_V12,
    WORKFLOW_RUN_FLOW_VERSION_V13, WORKFLOW_RUN_FLOW_VERSION_V14, WORKFLOW_RUN_FLOW_VERSION_V15,
    WORKFLOW_RUN_FLOW_VERSION_V16, WORKFLOW_RUN_FLOW_VERSION_V17, WORKFLOW_RUN_FLOW_VERSION_V18,
    WORKFLOW_RUN_FLOW_VERSION_V19, WORKFLOW_RUN_FLOW_VERSION_V2, WORKFLOW_RUN_FLOW_VERSION_V20,
    WORKFLOW_RUN_FLOW_VERSION_V21, WORKFLOW_RUN_FLOW_VERSION_V3, WORKFLOW_RUN_FLOW_VERSION_V4,
    WORKFLOW_RUN_FLOW_VERSION_V5, WORKFLOW_RUN_FLOW_VERSION_V6, WORKFLOW_RUN_FLOW_VERSION_V7,
    WORKFLOW_RUN_FLOW_VERSION_V8, WORKFLOW_RUN_FLOW_VERSION_V9, WORKFLOW_RUN_INPUT_MAX_BYTES,
    WORKFLOW_RUN_INPUT_MAX_BYTES_V2, WORKFLOW_RUN_INPUT_SCHEMA, WORKFLOW_RUN_INPUT_SCHEMA_V10,
    WORKFLOW_RUN_INPUT_SCHEMA_V11, WORKFLOW_RUN_INPUT_SCHEMA_V12, WORKFLOW_RUN_INPUT_SCHEMA_V13,
    WORKFLOW_RUN_INPUT_SCHEMA_V14, WORKFLOW_RUN_INPUT_SCHEMA_V15, WORKFLOW_RUN_INPUT_SCHEMA_V16,
    WORKFLOW_RUN_INPUT_SCHEMA_V17, WORKFLOW_RUN_INPUT_SCHEMA_V18, WORKFLOW_RUN_INPUT_SCHEMA_V19,
    WORKFLOW_RUN_INPUT_SCHEMA_V2, WORKFLOW_RUN_INPUT_SCHEMA_V20, WORKFLOW_RUN_INPUT_SCHEMA_V21,
    WORKFLOW_RUN_INPUT_SCHEMA_V3, WORKFLOW_RUN_INPUT_SCHEMA_V4, WORKFLOW_RUN_INPUT_SCHEMA_V5,
    WORKFLOW_RUN_INPUT_SCHEMA_V6, WORKFLOW_RUN_INPUT_SCHEMA_V7, WORKFLOW_RUN_INPUT_SCHEMA_V8,
    WORKFLOW_RUN_INPUT_SCHEMA_V9, WORKFLOW_RUN_MAX_TIMEOUT_SECONDS, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V10,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V11, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V12,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V13, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V14,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V15, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V16,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V17, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V18,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V19, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V2,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V20, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V21,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V3, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V4,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V5, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V6,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V7, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V8,
    WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION_V9,
};
pub use workflow_step_descriptor::{
    WorkflowStepBindingKind, WorkflowStepDescriptorAdmission, WorkflowStepDescriptorRegistry,
    WorkflowStepDescriptorRegistrySpec, WorkflowStepDescriptorRevision, WorkflowStepDescriptorSpec,
    WorkflowStepExecutionClass, WorkflowStepFailureContract, WorkflowStepFallbackMode,
    WorkflowStepOwner, WorkflowStepPort, WorkflowStepPortCardinality, WorkflowStepPresentation,
    WorkflowStepPresentationSpec, WorkflowStepRetryClassification,
    WORKFLOW_STEP_DESCRIPTOR_REGISTRY_MAX_ACL_BYTES, WORKFLOW_STEP_DESCRIPTOR_REGISTRY_SCHEMA,
    WORKFLOW_STEP_DESCRIPTOR_SEMANTIC_SCHEMA, WORKFLOW_STEP_PRESENTATION_SCHEMA,
};
pub use workflow_step_descriptor_bindings::{
    WorkflowStepDescriptorBinding, WorkflowStepDescriptorBindings,
    WorkflowStepDescriptorBindingsSpec, WORKFLOW_STEP_DESCRIPTOR_BINDINGS_MAX_ACL_BYTES,
    WORKFLOW_STEP_DESCRIPTOR_BINDINGS_SCHEMA,
};
pub use workflow_step_failure::{
    WorkflowStepDefaultOutputEvidence, WorkflowStepFailureClassification,
    WorkflowStepFailureDetails, WorkflowStepFailureOutput,
    WORKFLOW_STEP_DEFAULT_OUTPUT_EVIDENCE_SCHEMA, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA,
    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V2, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V3,
    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V4, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V5,
    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V6, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V7,
    WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V8,
};
pub use workflow_variable_contract::{
    WorkflowVariableAssignment, WorkflowVariableContract, WorkflowVariableContractSpec,
    WorkflowVariableDeclaration, WorkflowVariableExport, WorkflowVariableMutationMode,
    WorkflowVariableRead, WorkflowVariableReadMode, WorkflowVariableScope,
    WorkflowVariableStorageClass, WORKFLOW_VARIABLE_CONTRACT_COMPILER_SCHEMA_VERSION,
    WORKFLOW_VARIABLE_CONTRACT_MAX_ACL_BYTES, WORKFLOW_VARIABLE_CONTRACT_SCHEMA,
};
pub use workflow_variable_defaults::{
    WorkflowVariableDefault, WorkflowVariableDefaults, WorkflowVariableDefaultsSpec,
    WORKFLOW_VARIABLE_DEFAULTS_MAX_ACL_BYTES, WORKFLOW_VARIABLE_DEFAULTS_SCHEMA,
    WORKFLOW_VARIABLE_DEFAULT_MAX_VALUE_BYTES,
};
pub(crate) use workflow_variable_materialization::{
    lookup_workflow_variable_path, materialize_workflow_variables_with_application,
    materialize_workflow_variables_with_composites, project_workflow_variable_reads,
    resolve_application_variable_assignment_values,
};

#[cfg(test)]
mod authority_tests;
#[cfg(test)]
mod human_task_contract_tests;
#[cfg(test)]
mod workflow_composite_execution_tests;
#[cfg(test)]
mod workflow_composite_frame_tests;
#[cfg(test)]
mod workflow_composite_region_result_tests;
#[cfg(test)]
mod workflow_composite_regions_tests;
#[cfg(test)]
mod workflow_revision_semantic_contracts_tests;
#[cfg(test)]
mod workflow_run_contract_tests;
#[cfg(test)]
mod workflow_run_variable_inspection_tests;
#[cfg(test)]
mod workflow_step_descriptor_bindings_tests;
#[cfg(test)]
mod workflow_step_descriptor_contract_tests;
#[cfg(test)]
mod workflow_variable_contract_tests;
#[cfg(test)]
mod workflow_variable_defaults_tests;
