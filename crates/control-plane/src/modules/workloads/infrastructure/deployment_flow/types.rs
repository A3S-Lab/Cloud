use crate::modules::shared_kernel::domain::{
    DeploymentId, NodeCommandId, NodeId, OrganizationId, WorkloadId, WorkloadRevisionId,
};
use a3s_runtime::contract::RuntimeUnitSpec;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeploymentFlowInput {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub revision_id: WorkloadRevisionId,
    pub workload_id: WorkloadId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResolveStepOutput {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub revision_id: WorkloadRevisionId,
    pub workload_id: WorkloadId,
    pub spec: RuntimeUnitSpec,
    pub convergence_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResolveCancellationOutput {
    pub cleaned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum ResolveStepResult {
    Resolved(Box<ResolveStepOutput>),
    CancellationRequested(ResolveCancellationOutput),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ScheduleStepInput {
    pub resolved: ResolveStepOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ScheduleStepOutput {
    Ready {
        node_id: NodeId,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchStepInput {
    pub resolved: ResolveStepOutput,
    pub node_id: NodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchedRuntime {
    pub node_id: NodeId,
    pub command_id: NodeCommandId,
    pub result_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum DispatchStepOutput {
    Ready { dispatched: DispatchedRuntime },
    Failed { reason: String },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ObserveStepInput {
    pub resolved: ResolveStepOutput,
    pub dispatched: DispatchedRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ObserveStepOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Ready {
        observed_at: DateTime<Utc>,
        received_at: DateTime<Utc>,
        spec_digest: String,
    },
    Failed {
        reason: String,
    },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct VerifyStepInput {
    pub resolved: ResolveStepOutput,
    pub observation: ObserveStepOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum VerifyStepOutput {
    Verified { verified_at: DateTime<Utc> },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ActivateStepInput {
    pub resolved: ResolveStepOutput,
    pub verification: VerifyStepOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum ActivateStepOutput {
    Active {
        deployment_id: DeploymentId,
        workload_id: WorkloadId,
        revision_id: WorkloadRevisionId,
        activated_at: DateTime<Utc>,
    },
    CancellationRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CleanupDispatchStepInput {
    pub resolved: ResolveStepOutput,
    pub attempt: u32,
    pub issued_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchedCleanup {
    pub node_id: NodeId,
    pub command_id: NodeCommandId,
    pub result_deadline: DateTime<Utc>,
    pub cleanup_deadline: DateTime<Utc>,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum CleanupDispatchStepOutput {
    NotRequired {
        cleaned_at: DateTime<Utc>,
    },
    Ready {
        dispatched: DispatchedCleanup,
    },
    Retry {
        reason: String,
        next_attempt_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CleanupObserveStepInput {
    pub resolved: ResolveStepOutput,
    pub dispatched: DispatchedCleanup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum CleanupObserveStepOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Ready {
        cleaned_at: DateTime<Utc>,
    },
    Retry {
        reason: String,
        next_attempt_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompleteCancellationStepInput {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub cleaned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompleteCancellationStepOutput {
    pub deployment_id: DeploymentId,
    pub cancelled_at: DateTime<Utc>,
    pub operation_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FailStepInput {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FailStepOutput {
    pub deployment_id: DeploymentId,
    pub failed_at: DateTime<Utc>,
    pub reason: String,
}
