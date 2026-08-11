use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentExecutionId, NodeId, OrganizationId,
};
use a3s_cloud_contracts::AgentProtocolChangeSetV1;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Immutable workspace result captured from the exact terminal A3S Code run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionChangeSet {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub batch_id: Uuid,
    pub node_id: NodeId,
    pub change_set: AgentProtocolChangeSetV1,
    pub recorded_at: DateTime<Utc>,
}

impl AgentExecutionChangeSet {
    pub fn new(
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        batch_id: Uuid,
        node_id: NodeId,
        change_set: AgentProtocolChangeSetV1,
        recorded_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            execution_id,
            batch_id,
            node_id,
            change_set,
            recorded_at: canonical_timestamp(recorded_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.recorded_at = canonical_timestamp(self.recorded_at);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.change_set
            .validate()
            .map_err(|error| format!("invalid A3S Code change set ({})", error.code()))?;
        if self.organization_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.batch_id.is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.recorded_at != canonical_timestamp(self.recorded_at)
        {
            return Err("Agent execution change set identity is invalid".into());
        }
        Ok(())
    }
}
