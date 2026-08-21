use crate::modules::agents::domain::{AgentCodeRunBinding, AgentExecutionStatus};
use crate::modules::shared_kernel::domain::{AgentExecutionId, NodeCommandId, OrganizationId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AgentExecutionFlowInput {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PreparedAgentExecution {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub binding: AgentCodeRunBinding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_started_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum PrepareOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Ready {
        prepared: Box<PreparedAgentExecution>,
    },
    Terminal {
        completed: CompletedAgentExecution,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchInput {
    pub prepared: Box<PreparedAgentExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DispatchedAgentExecution {
    pub prepared: Box<PreparedAgentExecution>,
    pub command_id: NodeCommandId,
    pub acknowledgement_deadline: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_checkpoint_run_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum DispatchOutput {
    Ready {
        dispatched: Box<DispatchedAgentExecution>,
    },
    Terminal {
        completed: CompletedAgentExecution,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ObserveInput {
    pub dispatched: Box<DispatchedAgentExecution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ObserveOutput {
    Pending {
        reason: String,
        next_poll_at: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatched: Option<Box<DispatchedAgentExecution>>,
    },
    Terminal {
        completed: CompletedAgentExecution,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CompletedAgentExecution {
    pub execution_id: AgentExecutionId,
    pub status: AgentExecutionStatus,
    pub finished_at: DateTime<Utc>,
}
