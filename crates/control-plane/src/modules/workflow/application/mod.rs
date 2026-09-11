pub mod commands;
mod environment_access;
#[cfg(test)]
mod historical_idempotency_replay_tests;
pub(crate) mod human_task_access;
mod human_task_authorization_port;
mod human_task_form_port;
mod project_access;
pub mod queries;
pub(crate) mod resource_access;
mod workflow_authoring;
#[cfg(test)]
mod workflow_authoring_tests;
mod workflow_composite_execution_port;
#[cfg(test)]
mod workflow_composite_execution_port_tests;
mod workflow_definition_publication;
#[cfg(test)]
mod workflow_definition_publication_tests;
mod workflow_run_reconciler;

pub use environment_access::{IWorkflowEnvironmentAccess, WorkflowEnvironmentScope};
pub use human_task_authorization_port::{
    HumanTaskSubmissionAuthorization, IHumanTaskAuthorizationPort,
};
pub use human_task_form_port::{
    HumanTaskFormEvaluation, HumanTaskFormReleaseAuthority, IHumanTaskFormPort,
};
pub use project_access::{IWorkflowProjectAccess, WorkflowProjectScope};
pub use workflow_authoring::{
    AppendWorkflowAuthoringRequest, CreateWorkflowAuthoringJournalRequest,
    GetWorkflowAuthoringJournalRequest, IWorkflowAuthoringApplicationPort,
    IWorkflowAuthoringFlowPort, PageWorkflowAuthoringRequest, WorkflowAuthoringApplicationService,
};
pub use workflow_composite_execution_port::{
    IWorkflowCompositeExecutionPort, WorkflowCompositeExecutionApplicationService,
    WorkflowCompositeExecutionRequest,
};
pub use workflow_definition_publication::{
    IWorkflowDefinitionPublicationPort, WorkflowDefinitionPublicationProvenance,
    WorkflowDefinitionPublicationRequest, WorkflowDefinitionPublicationService,
};
pub use workflow_run_reconciler::{
    WorkflowRunReconcileFailure, WorkflowRunReconcileReport, WorkflowRunReconciler,
};
pub use resource_access::WorkflowAccess;
pub(crate) use resource_access::WorkflowAccessScope;

use crate::modules::workflow::domain::{
    HumanTaskRecord, OntologyDiff, OntologyRecord, WorkflowDefinitionRecord, WorkflowGoalRecord,
    WorkflowPayloadKind,
};

#[derive(Debug, Clone, PartialEq)]
pub struct HumanTaskMutationResult {
    pub record: HumanTaskRecord,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OntologyMutationResult {
    pub record: OntologyRecord,
    pub diff: Option<OntologyDiff>,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowPayloadAcl {
    pub kind: WorkflowPayloadKind,
    pub acl: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowSemanticContractAcls {
    pub descriptor_bindings_acl: String,
    pub descriptor_registry_acl: String,
    pub variable_contract_acl: String,
    pub variable_defaults_acl: Option<String>,
    pub composite_regions_acl: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinitionMutationResult {
    pub record: WorkflowDefinitionRecord,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowGoalMutationResult {
    pub record: WorkflowGoalRecord,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunMutationResult {
    pub record: crate::modules::workflow::domain::WorkflowRunRecord,
    pub replayed: bool,
}
