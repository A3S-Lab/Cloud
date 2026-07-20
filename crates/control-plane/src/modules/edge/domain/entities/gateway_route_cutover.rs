use crate::modules::edge::domain::{Route, RouteState};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, GatewayCertificateId, NodeCommandId, NodeId, OrganizationId,
    WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayRouteCutoverState {
    Pending,
    Applied,
    Rejected,
}

impl GatewayRouteCutoverState {
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
            _ => Err(format!("unsupported Gateway route cutover state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRouteCutover {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub previous_revision_id: WorkloadRevisionId,
    pub candidate_revision_id: WorkloadRevisionId,
    pub node_id: NodeId,
    pub gateway_revision: u64,
    pub gateway_command_id: NodeCommandId,
    pub gateway_certificate_id: GatewayCertificateId,
    pub snapshot_digest: String,
    pub routes: Vec<Route>,
    pub state: GatewayRouteCutoverState,
    pub failure: Option<String>,
    pub staged_at: DateTime<Utc>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

impl GatewayRouteCutover {
    #[allow(clippy::too_many_arguments)]
    pub fn stage(
        deployment_id: DeploymentId,
        organization_id: OrganizationId,
        workload_id: WorkloadId,
        previous_revision_id: WorkloadRevisionId,
        candidate_revision_id: WorkloadRevisionId,
        node_id: NodeId,
        gateway_revision: u64,
        gateway_command_id: NodeCommandId,
        gateway_certificate_id: GatewayCertificateId,
        snapshot_digest: String,
        mut routes: Vec<Route>,
        staged_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let staged_at = canonical_timestamp(staged_at);
        routes.sort_by_key(|route| route.id);
        let cutover = Self {
            deployment_id,
            organization_id,
            workload_id,
            previous_revision_id,
            candidate_revision_id,
            node_id,
            gateway_revision,
            gateway_command_id,
            gateway_certificate_id,
            snapshot_digest,
            routes,
            state: GatewayRouteCutoverState::Pending,
            failure: None,
            staged_at,
            acknowledged_at: None,
        };
        cutover.validate()?;
        Ok(cutover)
    }

    pub fn acknowledge(&mut self, acknowledgement: &NodeGatewayAck) -> Result<(), String> {
        acknowledgement.validate()?;
        let acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
        if acknowledgement.node_id != self.node_id.as_uuid()
            || acknowledgement.command_id != self.gateway_command_id.as_uuid()
            || acknowledgement.revision != self.gateway_revision
            || acknowledgement.snapshot_digest != self.snapshot_digest
        {
            return Err("Gateway acknowledgement does not match the staged route cutover".into());
        }
        if acknowledged_at < self.staged_at {
            return Err("Gateway route cutover acknowledgement predates staging".into());
        }
        let next_state = match acknowledgement.state {
            GatewayAckState::Applied => GatewayRouteCutoverState::Applied,
            GatewayAckState::Rejected => GatewayRouteCutoverState::Rejected,
        };
        if self.state == next_state
            && self.failure == acknowledgement.message
            && self.acknowledged_at == Some(acknowledged_at)
        {
            return Ok(());
        }
        if self.state != GatewayRouteCutoverState::Pending {
            return Err("Gateway route cutover already has a different terminal outcome".into());
        }
        for route in &mut self.routes {
            route.apply_gateway_acknowledgement(acknowledgement)?;
        }
        self.state = next_state;
        self.failure = acknowledgement.message.clone();
        self.acknowledged_at = Some(acknowledged_at);
        self.validate()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.previous_revision_id == self.candidate_revision_id
            || self.gateway_revision == 0
            || !valid_sha256(&self.snapshot_digest)
            || self.routes.is_empty()
            || self
                .routes
                .windows(2)
                .any(|routes| routes[0].id >= routes[1].id)
        {
            return Err("Gateway route cutover identity is invalid".into());
        }
        let expected_route_state = match self.state {
            GatewayRouteCutoverState::Pending => RouteState::Publishing,
            GatewayRouteCutoverState::Applied => RouteState::Active,
            GatewayRouteCutoverState::Rejected => RouteState::Rejected,
        };
        if self.routes.iter().any(|route| {
            route.organization_id != self.organization_id
                || route.workload_id != self.workload_id
                || route.workload_revision_id != self.candidate_revision_id
                || route.gateway_node_id != self.node_id
                || route.state != expected_route_state
                || route.gateway_revision != Some(self.gateway_revision)
                || route.gateway_command_id != Some(self.gateway_command_id)
                || route.snapshot_digest.as_deref() != Some(self.snapshot_digest.as_str())
                || route.gateway_certificate_id != Some(self.gateway_certificate_id)
        }) {
            return Err("Gateway route cutover routes are inconsistent".into());
        }
        let state_is_consistent = match self.state {
            GatewayRouteCutoverState::Pending => {
                self.failure.is_none()
                    && self.acknowledged_at.is_none()
                    && self
                        .routes
                        .iter()
                        .all(|route| route.failure.is_none() && route.activated_at.is_none())
            }
            GatewayRouteCutoverState::Applied => {
                self.failure.is_none()
                    && self.acknowledged_at.is_some()
                    && self
                        .routes
                        .iter()
                        .all(|route| route.failure.is_none() && route.activated_at.is_some())
            }
            GatewayRouteCutoverState::Rejected => {
                self.failure.is_some()
                    && self.acknowledged_at.is_some()
                    && self
                        .routes
                        .iter()
                        .all(|route| route.failure == self.failure && route.activated_at.is_none())
            }
        };
        if !state_is_consistent {
            return Err("Gateway route cutover state is inconsistent".into());
        }
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
