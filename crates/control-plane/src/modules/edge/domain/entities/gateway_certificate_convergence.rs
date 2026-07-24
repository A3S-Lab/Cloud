use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayCertificateId, NodeCommandId, NodeId, OrganizationId, RouteId,
};
use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayCertificateConvergenceReason {
    Renewal,
    SnapshotRenewal,
    DomainRevocation,
    CertificateRevocation,
    ProjectionRepair,
}

impl GatewayCertificateConvergenceReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Renewal => "renewal",
            Self::SnapshotRenewal => "snapshot_renewal",
            Self::DomainRevocation => "domain_revocation",
            Self::CertificateRevocation => "certificate_revocation",
            Self::ProjectionRepair => "projection_repair",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "renewal" => Ok(Self::Renewal),
            "snapshot_renewal" => Ok(Self::SnapshotRenewal),
            "domain_revocation" => Ok(Self::DomainRevocation),
            "certificate_revocation" => Ok(Self::CertificateRevocation),
            "projection_repair" => Ok(Self::ProjectionRepair),
            _ => Err(format!(
                "unsupported Gateway certificate convergence reason {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayCertificateConvergenceState {
    Pending,
    Applied,
    Rejected,
}

impl GatewayCertificateConvergenceState {
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
            _ => Err(format!(
                "unsupported Gateway certificate convergence state {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRouteVersion {
    pub route_id: RouteId,
    pub aggregate_version: u64,
}

impl GatewayRouteVersion {
    pub fn new(route_id: RouteId, aggregate_version: u64) -> Result<Self, String> {
        if aggregate_version == 0 {
            return Err("Gateway convergence route version must be positive".into());
        }
        Ok(Self {
            route_id,
            aggregate_version,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayCertificateConvergence {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub gateway_revision: u64,
    pub gateway_command_id: NodeCommandId,
    pub previous_certificate_id: GatewayCertificateId,
    pub replacement_certificate_id: Option<GatewayCertificateId>,
    pub snapshot_digest: String,
    pub retained_routes: Vec<GatewayRouteVersion>,
    pub rejected_routes: Vec<GatewayRouteVersion>,
    pub reason: GatewayCertificateConvergenceReason,
    pub state: GatewayCertificateConvergenceState,
    pub failure: Option<String>,
    pub staged_at: DateTime<Utc>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

impl GatewayCertificateConvergence {
    #[allow(clippy::too_many_arguments)]
    pub fn stage(
        organization_id: OrganizationId,
        node_id: NodeId,
        gateway_revision: u64,
        gateway_command_id: NodeCommandId,
        previous_certificate_id: GatewayCertificateId,
        replacement_certificate_id: Option<GatewayCertificateId>,
        snapshot_digest: String,
        mut retained_routes: Vec<GatewayRouteVersion>,
        mut rejected_routes: Vec<GatewayRouteVersion>,
        reason: GatewayCertificateConvergenceReason,
        staged_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        retained_routes.sort_by_key(|route| route.route_id);
        rejected_routes.sort_by_key(|route| route.route_id);
        let convergence = Self {
            organization_id,
            node_id,
            gateway_revision,
            gateway_command_id,
            previous_certificate_id,
            replacement_certificate_id,
            snapshot_digest,
            retained_routes,
            rejected_routes,
            reason,
            state: GatewayCertificateConvergenceState::Pending,
            failure: None,
            staged_at: canonical_timestamp(staged_at),
            acknowledged_at: None,
        };
        convergence.validate()?;
        Ok(convergence)
    }

    pub fn acknowledge(&mut self, acknowledgement: &NodeGatewayAck) -> Result<(), String> {
        acknowledgement.validate()?;
        let acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
        if acknowledgement.node_id != self.node_id.as_uuid()
            || acknowledgement.command_id != self.gateway_command_id.as_uuid()
            || acknowledgement.revision != self.gateway_revision
            || acknowledgement.snapshot_digest != self.snapshot_digest
        {
            return Err("Gateway acknowledgement does not match certificate convergence".into());
        }
        if acknowledged_at < self.staged_at {
            return Err("Gateway certificate convergence acknowledgement predates staging".into());
        }
        let state = match acknowledgement.state {
            GatewayAckState::Applied => GatewayCertificateConvergenceState::Applied,
            GatewayAckState::Rejected => GatewayCertificateConvergenceState::Rejected,
        };
        if self.state == state
            && self.failure == acknowledgement.message
            && self.acknowledged_at == Some(acknowledged_at)
        {
            return Ok(());
        }
        if self.state != GatewayCertificateConvergenceState::Pending {
            return Err(
                "Gateway certificate convergence already has a different terminal outcome".into(),
            );
        }
        self.state = state;
        self.failure = acknowledgement.message.clone();
        self.acknowledged_at = Some(acknowledged_at);
        self.validate()
    }

    pub fn active_certificate_id(&self) -> Option<GatewayCertificateId> {
        if self.retained_routes.is_empty() {
            None
        } else {
            Some(
                self.replacement_certificate_id
                    .unwrap_or(self.previous_certificate_id),
            )
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let retained_ids = route_ids(&self.retained_routes)?;
        let rejected_ids = route_ids(&self.rejected_routes)?;
        let certificate_transition_is_valid = match self.reason {
            GatewayCertificateConvergenceReason::SnapshotRenewal => {
                !self.retained_routes.is_empty()
                    && self.rejected_routes.is_empty()
                    && self.replacement_certificate_id.is_none()
            }
            GatewayCertificateConvergenceReason::DomainRevocation => {
                !self.rejected_routes.is_empty()
                    && (self.retained_routes.is_empty()
                        == self.replacement_certificate_id.is_none())
            }
            GatewayCertificateConvergenceReason::Renewal
            | GatewayCertificateConvergenceReason::CertificateRevocation
            | GatewayCertificateConvergenceReason::ProjectionRepair => {
                !self.retained_routes.is_empty()
                    && self.rejected_routes.is_empty()
                    && self.replacement_certificate_id.is_some()
            }
        };
        if self.gateway_revision == 0
            || !valid_sha256(&self.snapshot_digest)
            || retained_ids.is_empty() && rejected_ids.is_empty()
            || !retained_ids.is_disjoint(&rejected_ids)
            || self.replacement_certificate_id == Some(self.previous_certificate_id)
            || !certificate_transition_is_valid
        {
            return Err("Gateway certificate convergence identity is invalid".into());
        }
        let state_is_consistent = match self.state {
            GatewayCertificateConvergenceState::Pending => {
                self.failure.is_none() && self.acknowledged_at.is_none()
            }
            GatewayCertificateConvergenceState::Applied => {
                self.failure.is_none() && self.acknowledged_at.is_some()
            }
            GatewayCertificateConvergenceState::Rejected => {
                self.failure.as_deref().is_some_and(valid_failure) && self.acknowledged_at.is_some()
            }
        };
        if !state_is_consistent
            || self
                .acknowledged_at
                .is_some_and(|acknowledged_at| acknowledged_at < self.staged_at)
        {
            return Err("Gateway certificate convergence state is inconsistent".into());
        }
        Ok(())
    }
}

fn route_ids(routes: &[GatewayRouteVersion]) -> Result<BTreeSet<RouteId>, String> {
    if routes
        .windows(2)
        .any(|routes| routes[0].route_id >= routes[1].route_id)
        || routes.iter().any(|route| route.aggregate_version == 0)
    {
        return Err("Gateway convergence routes must be sorted, unique, and versioned".into());
    }
    Ok(routes.iter().map(|route| route.route_id).collect())
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn valid_failure(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && value.trim() == value
        && !value.contains(['\0', '\r', '\n'])
}
