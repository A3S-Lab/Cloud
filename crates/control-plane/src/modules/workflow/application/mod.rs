pub mod commands;
pub mod queries;
mod workflow_run_operation;

use crate::modules::operations::domain::entities::OperationRecord;
use crate::modules::workflow::domain::{
    OntologyDiff, OntologyRecord, WorkflowDefinitionRecord, WorkflowGoalRecord,
    WorkflowPayloadKind, WorkflowRun,
};

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
    pub run: WorkflowRun,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunView {
    pub run: WorkflowRun,
    pub operation: OperationRecord,
}

pub use workflow_run_operation::{
    workflow_run_operation, WorkflowRunOperationInput, WORKFLOW_RUN_FLOW_NAME,
    WORKFLOW_RUN_FLOW_VERSION, WORKFLOW_RUN_INPUT_SCHEMA,
};
