use crate::modules::executions::domain::{ExecutionOutcome, ExecutionStatus};
use crate::modules::shared_kernel::domain::{ExecutionId, NodeCommandId, NodeId, OrganizationId};
use a3s_runtime::contract::RuntimeUnitSpec;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ExecutionFlowInput {
    pub organization_id: OrganizationId,
    pub execution_id: ExecutionId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ScheduledExecution {
    pub organization_id: OrganizationId,
    pub execution_id: ExecutionId,
    pub node_id: NodeId,
    pub spec: Box<RuntimeUnitSpec>,
    pub convergence_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ScheduleOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Ready {
        scheduled: Box<ScheduledExecution>,
    },
    Terminal {
        terminal: TerminalExecution,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchInput {
    pub scheduled: Box<ScheduledExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchedExecution {
    pub scheduled: Box<ScheduledExecution>,
    pub command_id: NodeCommandId,
    pub result_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum DispatchOutput {
    Ready {
        dispatched: Box<DispatchedExecution>,
    },
    Terminal {
        terminal: TerminalExecution,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ObserveInput {
    pub dispatched: Box<DispatchedExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ObserveOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Terminal {
        terminal: TerminalExecution,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TerminalExecution {
    pub organization_id: OrganizationId,
    pub execution_id: ExecutionId,
    pub outcome: ExecutionOutcome,
    pub terminal_at: DateTime<Utc>,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CleanupDispatchInput {
    pub terminal: TerminalExecution,
    pub attempt: u32,
    pub issued_at: DateTime<Utc>,
    pub cleanup_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchedCleanup {
    pub terminal: TerminalExecution,
    pub node_id: NodeId,
    pub command_id: NodeCommandId,
    pub result_deadline: DateTime<Utc>,
    pub cleanup_deadline: DateTime<Utc>,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum CleanupDispatchOutput {
    Completed {
        execution: CompletedExecution,
    },
    Ready {
        dispatched: DispatchedCleanup,
    },
    Retry {
        reason: String,
        next_attempt_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CleanupObserveInput {
    pub dispatched: DispatchedCleanup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum CleanupObserveOutput {
    Completed {
        execution: CompletedExecution,
    },
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Retry {
        reason: String,
        next_attempt_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompletedExecution {
    pub execution_id: ExecutionId,
    pub status: ExecutionStatus,
    pub outcome: ExecutionOutcome,
    pub finished_at: DateTime<Utc>,
}
