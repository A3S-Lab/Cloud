pub mod commands;
pub mod queries;
mod workflow_run_reconciler;

pub use workflow_run_reconciler::{
    WorkflowRunReconcileFailure, WorkflowRunReconcileReport, WorkflowRunReconciler,
};

use crate::modules::workflow::domain::{
    OntologyDiff, OntologyRecord, WorkflowDefinitionRecord, WorkflowGoalRecord, WorkflowPayloadKind,
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
    pub record: crate::modules::workflow::domain::WorkflowRunRecord,
    pub replayed: bool,
}
