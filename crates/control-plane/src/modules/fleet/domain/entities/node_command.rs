use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId};
use a3s_cloud_contracts::{NodeCommandEnvelope, NodeCommandMetadata, NodeCommandPayload};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCommand {
    pub id: NodeCommandId,
    pub node_id: NodeId,
    pub sequence: u64,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

impl NodeCommand {
    pub fn issue(draft: NodeCommandDraft, sequence: u64) -> Result<Self, String> {
        if draft.proposed_command_id.as_uuid().is_nil()
            || draft.node_id.as_uuid().is_nil()
            || draft.aggregate_id.is_nil()
            || draft.correlation_id.is_nil()
        {
            return Err("node command identity must not contain nil UUIDs".into());
        }
        if sequence == 0 {
            return Err("node command sequence must be positive".into());
        }
        let issued_at = canonical_timestamp("node command issue", draft.issued_at)?;
        let not_after = canonical_timestamp("node command expiry", draft.not_after)?;
        if not_after <= issued_at {
            return Err("node command expiry must follow issue time".into());
        }
        draft.payload.validate()?;
        Ok(Self {
            id: draft.proposed_command_id,
            node_id: draft.node_id,
            sequence,
            aggregate_id: draft.aggregate_id,
            payload: draft.payload,
            issued_at,
            not_after,
            correlation_id: draft.correlation_id,
        })
    }

    pub fn kind(&self) -> &'static str {
        match self.payload {
            NodeCommandPayload::RuntimeApply { .. } => "runtime_apply",
            NodeCommandPayload::RuntimeInspect { .. } => "runtime_inspect",
            NodeCommandPayload::RuntimeStop { .. } => "runtime_stop",
            NodeCommandPayload::RuntimeRemove { .. } => "runtime_remove",
            NodeCommandPayload::GatewaySnapshotInstall { .. } => "gateway_snapshot_install",
        }
    }

    pub fn generation(&self) -> u64 {
        self.payload.generation()
    }

    pub fn payload_schema(&self) -> &'static str {
        self.payload.schema()
    }

    pub fn payload_digest(&self) -> Result<String, String> {
        self.payload.digest()
    }

    pub fn envelope(&self, lease_id: Uuid) -> Result<NodeCommandEnvelope, String> {
        NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id: self.id.as_uuid(),
                lease_id,
                node_id: self.node_id.as_uuid(),
                sequence: self.sequence,
                aggregate_id: self.aggregate_id,
                issued_at: self.issued_at,
                not_after: self.not_after,
                correlation_id: self.correlation_id,
            },
            self.payload.clone(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCommandDraft {
    pub proposed_command_id: NodeCommandId,
    pub node_id: NodeId,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::NodeCommandPayload;
    use chrono::{TimeZone, Timelike};

    #[test]
    fn command_timestamps_are_canonical_at_database_precision() {
        let issued_at = Utc
            .timestamp_opt(1_700_000_000, 123_456_789)
            .single()
            .expect("timestamp");
        let draft = NodeCommandDraft {
            proposed_command_id: NodeCommandId::new(),
            node_id: NodeId::new(),
            aggregate_id: Uuid::now_v7(),
            payload: NodeCommandPayload::RuntimeInspect {
                unit_id: "timestamp-fixture".into(),
                generation: 1,
            },
            issued_at,
            not_after: issued_at + chrono::Duration::minutes(1),
            correlation_id: Uuid::now_v7(),
        };

        let command = NodeCommand::issue(draft.clone(), 1).expect("issue command");
        let replay = NodeCommand::issue(draft, 1).expect("replay command");

        assert_eq!(command, replay);
        assert_eq!(command.issued_at.nanosecond(), 123_456_000);
        assert_eq!(command.not_after.nanosecond(), 123_456_000);
    }
}
