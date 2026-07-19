use crate::modules::shared_kernel::domain::{canonical_timestamp, NodeCommandId, NodeId};
use a3s_cloud_contracts::{
    GatewayAckState, GatewayCertificateRequest, GatewaySnapshot, NodeGatewayAck,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayPublicationState {
    Pending,
    Applied,
    Rejected,
}

impl GatewayPublicationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            _ => Err(format!("unsupported Gateway publication state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayPublication {
    pub node_id: NodeId,
    pub revision: u64,
    pub expected_revision: Option<u64>,
    pub command_id: NodeCommandId,
    pub command_correlation_id: Uuid,
    pub snapshot_digest: String,
    pub acl: String,
    pub certificate_request: Option<GatewayCertificateRequest>,
    pub state: GatewayPublicationState,
    pub failure: Option<String>,
    pub command_issued_at: DateTime<Utc>,
    pub command_not_after: DateTime<Utc>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

impl GatewayPublication {
    pub fn stage(
        node_id: NodeId,
        command_id: NodeCommandId,
        command_correlation_id: Uuid,
        snapshot: GatewaySnapshot,
        command_issued_at: DateTime<Utc>,
        command_not_after: DateTime<Utc>,
    ) -> Result<Self, String> {
        snapshot.validate()?;
        if command_correlation_id.is_nil() {
            return Err("Gateway publication command correlation ID must not be nil".into());
        }
        let command_issued_at = canonical_timestamp(command_issued_at);
        let command_not_after = canonical_timestamp(command_not_after);
        if command_not_after <= command_issued_at {
            return Err("Gateway publication command expiry must follow its issue time".into());
        }
        Ok(Self {
            node_id,
            revision: snapshot.revision,
            expected_revision: snapshot.expected_revision,
            command_id,
            command_correlation_id,
            snapshot_digest: snapshot.snapshot_digest,
            acl: snapshot.acl,
            certificate_request: snapshot.certificate_request,
            state: GatewayPublicationState::Pending,
            failure: None,
            command_issued_at,
            command_not_after,
            acknowledged_at: None,
        })
    }

    pub fn snapshot(&self) -> Result<GatewaySnapshot, String> {
        let snapshot = GatewaySnapshot::new_with_certificate(
            self.revision,
            self.expected_revision,
            self.acl.clone(),
            self.certificate_request.clone(),
        )?;
        if snapshot.snapshot_digest != self.snapshot_digest {
            return Err("stored Gateway publication digest does not match its ACL".into());
        }
        Ok(snapshot)
    }

    pub fn acknowledge(&mut self, acknowledgement: &NodeGatewayAck) -> Result<(), String> {
        acknowledgement.validate_for(
            self.command_id.as_uuid(),
            self.node_id.as_uuid(),
            &self.snapshot()?,
        )?;
        let acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
        if acknowledged_at < self.command_issued_at {
            return Err("Gateway acknowledgement predates its publication".into());
        }
        let next = match acknowledgement.state {
            GatewayAckState::Applied => GatewayPublicationState::Applied,
            GatewayAckState::Rejected => GatewayPublicationState::Rejected,
        };
        if self.state == next
            && self.failure == acknowledgement.message
            && self.acknowledged_at == Some(acknowledged_at)
        {
            return Ok(());
        }
        if self.state != GatewayPublicationState::Pending {
            return Err("Gateway publication already has a different terminal outcome".into());
        }
        self.state = next;
        self.failure = acknowledgement.message.clone();
        self.acknowledged_at = Some(acknowledged_at);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayScopeState {
    pub node_id: NodeId,
    pub last_issued_revision: u64,
    pub installed_revision: Option<u64>,
    pub aggregate_version: u64,
}

impl GatewayScopeState {
    pub const fn empty(node_id: NodeId) -> Self {
        Self {
            node_id,
            last_issued_revision: 0,
            installed_revision: None,
            aggregate_version: 0,
        }
    }

    pub fn next_revision(&self) -> Result<u64, String> {
        self.last_issued_revision
            .checked_add(1)
            .ok_or_else(|| "Gateway revision space is exhausted".into())
    }
}
