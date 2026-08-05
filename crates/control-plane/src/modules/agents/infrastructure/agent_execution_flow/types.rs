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
