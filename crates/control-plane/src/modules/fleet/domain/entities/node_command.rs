use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult,
};
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
        let issued_at = canonical_timestamp(draft.issued_at);
        let not_after = canonical_timestamp(draft.not_after);
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
            NodeCommandPayload::ResourceClaimPrepare { .. } => "resource_claim_prepare",
            NodeCommandPayload::RuntimeApply { .. } => "runtime_apply",
            NodeCommandPayload::RuntimeInspect { .. } => "runtime_inspect",
            NodeCommandPayload::RuntimeStop { .. } => "runtime_stop",
            NodeCommandPayload::RuntimeRemove { .. } => "runtime_remove",
            NodeCommandPayload::ResourceClaimRelease { .. } => "resource_claim_release",
            NodeCommandPayload::GatewaySnapshotInstall { .. } => "gateway_snapshot_install",
            NodeCommandPayload::GatewaySnapshotObserve { .. } => "gateway_snapshot_observe",
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

    pub fn canonicalize_acknowledgement(mut acknowledgement: NodeCommandAck) -> NodeCommandAck {
        acknowledgement.completed_at = canonical_timestamp(acknowledgement.completed_at);
        if let NodeCommandOutcome::Succeeded { result } = &mut acknowledgement.outcome {
            match result.as_mut() {
                NodeCommandResult::ResourceClaimPrepared { prepared } => {
                    prepared.prepared_at = canonical_timestamp(prepared.prepared_at);
                }
                NodeCommandResult::ResourceClaimReleased { released } => {
                    released.released_at = canonical_timestamp(released.released_at);
                }
                NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } => {
                    acknowledgement.acknowledged_at =
                        canonical_timestamp(acknowledgement.acknowledged_at);
                }
                NodeCommandResult::GatewaySnapshotObserved { observation } => {
                    observation.observed_at = canonical_timestamp(observation.observed_at);
                    if let Some(applied) = &mut observation.applied {
                        applied.issued_at = canonical_timestamp(applied.issued_at);
                        applied.expires_at = canonical_timestamp(applied.expires_at);
                        applied.applied_at = canonical_timestamp(applied.applied_at);
                    }
                }
                NodeCommandResult::RuntimeApplied { .. }
                | NodeCommandResult::RuntimeInspected { .. }
                | NodeCommandResult::RuntimeStopped { .. }
                | NodeCommandResult::RuntimeRemoved { .. } => {}
            }
        }
        acknowledgement
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
    use a3s_cloud_contracts::{
        NodeCommandAck, NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
        NodeResourceClaimPrepared,
    };
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

    #[test]
    fn acknowledgement_evidence_is_canonicalized_with_its_completion() {
        let timestamp = Utc
            .timestamp_opt(1_700_000_000, 123_456_789)
            .single()
            .expect("timestamp");
        let acknowledgement = NodeCommand::canonicalize_acknowledgement(NodeCommandAck {
            schema: NodeCommandAck::SCHEMA.into(),
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            sequence: 1,
            payload_digest: format!("sha256:{}", "a".repeat(64)),
            completed_at: timestamp,
            outcome: NodeCommandOutcome::Succeeded {
                result: Box::new(NodeCommandResult::ResourceClaimPrepared {
                    prepared: NodeResourceClaimPrepared {
                        schema: NodeResourceClaimPrepared::SCHEMA.into(),
                        claim_id: Uuid::now_v7(),
                        claim_generation: 1,
                        claim_digest: format!("sha256:{}", "b".repeat(64)),
                        binding_digest: format!("sha256:{}", "c".repeat(64)),
                        slots: Vec::new(),
                        prepared_at: timestamp,
                    },
                }),
            },
        });

        let NodeCommandOutcome::Succeeded { result } = acknowledgement.outcome else {
            panic!("acknowledgement must succeed");
        };
        let NodeCommandResult::ResourceClaimPrepared { prepared } = result.as_ref() else {
            panic!("acknowledgement must contain prepared Claim evidence");
        };
        assert_eq!(acknowledgement.completed_at.nanosecond(), 123_456_000);
        assert_eq!(prepared.prepared_at, acknowledgement.completed_at);
    }
}
