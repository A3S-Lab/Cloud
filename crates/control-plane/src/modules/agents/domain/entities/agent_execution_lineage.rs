use crate::modules::shared_kernel::domain::{
    AgentExecutionCheckpointId, AgentExecutionId, Sha256Digest,
};
use serde::{Deserialize, Serialize};

pub const MAX_AGENT_EXECUTION_FORK_DEPTH: u16 = 64;

/// Immutable parent/checkpoint lineage for one logical Agent execution fork.
///
/// A fork always creates a new execution. The parent execution and its semantic
/// trajectory remain immutable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentExecutionLineage {
    pub parent_execution_id: AgentExecutionId,
    pub parent_checkpoint_id: AgentExecutionCheckpointId,
    pub parent_checkpoint_digest: Sha256Digest,
    pub depth: u16,
}

impl AgentExecutionLineage {
    pub fn new(
        parent_execution_id: AgentExecutionId,
        parent_checkpoint_id: AgentExecutionCheckpointId,
        parent_checkpoint_digest: Sha256Digest,
        depth: u16,
    ) -> Result<Self, String> {
        let lineage = Self {
            parent_execution_id,
            parent_checkpoint_id,
            parent_checkpoint_digest,
            depth,
        };
        lineage.validate()?;
        Ok(lineage)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.parent_execution_id.as_uuid().is_nil()
            || self.parent_checkpoint_id.as_uuid().is_nil()
            || self.depth == 0
            || self.depth > MAX_AGENT_EXECUTION_FORK_DEPTH
            || Sha256Digest::parse(self.parent_checkpoint_digest.as_str())
                .ok()
                .as_ref()
                != Some(&self.parent_checkpoint_digest)
        {
            return Err("Agent execution fork lineage is invalid".into());
        }
        Ok(())
    }
}
