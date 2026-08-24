use crate::modules::shared_kernel::domain::{OperationId, WorkflowRunId};
use crate::modules::workflow::domain::{WorkflowRunRecord, WorkflowRunStatus};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WORKFLOW_RUN_DIAGNOSTICS_SCHEMA: &str = "cloud.workflow-run.diagnostics.v1";
pub const WORKFLOW_RUN_DIAGNOSTICS_MAX_EVIDENCE_REFERENCES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunDiagnosticStatus {
    Ok,
    Attention,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunDiagnosticCode {
    FlowHistoryMissing,
    ProjectionLag,
    ProjectionAhead,
    ActiveExternalWait,
    CancellationPending,
    RetryObserved,
    RuntimeRecoveryObserved,
    StepFailureObserved,
    RunFailed,
    RunTimedOut,
    RunCancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunDiagnostic {
    pub code: WorkflowRunDiagnosticCode,
    pub severity: WorkflowRunDiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunObservedFlowStatus {
    Missing,
    Pending,
    Running,
    Suspended,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    ContinuedAsNew,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunStepStatistics {
    pub total: u64,
    pub pending: u64,
    pub running: u64,
    pub completed: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub skipped: u64,
    pub total_attempt_generations: u64,
    pub evidence_reference_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunFlowStatistics {
    pub event_count: u64,
    pub event_counts: BTreeMap<String, u64>,
    pub durable_step_count: u64,
    pub active_hook_count: u64,
    pub pending_timer_count: u64,
    pub linked_child_operation_count: u64,
    pub child_workflow_count: u64,
    pub retry_event_count: u64,
    pub host_shutdown_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunEvidenceCorrelation {
    pub step_id: String,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunDiagnostics {
    pub schema: String,
    pub workflow_run_id: WorkflowRunId,
    pub operation_id: OperationId,
    pub flow_run_id: String,
    pub run_status: WorkflowRunStatus,
    pub observed_flow_status: WorkflowRunObservedFlowStatus,
    pub flow_runtime_build_id: Option<String>,
    pub projected_flow_sequence: u64,
    pub observed_flow_sequence: Option<u64>,
    pub unprojected_event_count: u64,
    pub observed_at: DateTime<Utc>,
    pub step_statistics: WorkflowRunStepStatistics,
    pub flow_statistics: WorkflowRunFlowStatistics,
    pub evidence_correlations: Vec<WorkflowRunEvidenceCorrelation>,
    pub evidence_correlations_truncated: bool,
    pub diagnostic_status: WorkflowRunDiagnosticStatus,
    pub diagnostics: Vec<WorkflowRunDiagnostic>,
}

#[async_trait]
pub trait IWorkflowRunDiagnosticsReader: Send + Sync {
    async fn inspect(&self, record: &WorkflowRunRecord) -> Result<WorkflowRunDiagnostics, String>;
}
