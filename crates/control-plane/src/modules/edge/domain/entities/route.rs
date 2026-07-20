use crate::modules::edge::domain::{
    DomainNamePattern, RouteHostname, RoutePath, RoutePortName, UpstreamEndpoint,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, EnvironmentId, GatewayCertificateId, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteState {
    Pending,
    Publishing,
    Active,
    Rejected,
}

impl RouteState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Publishing => "publishing",
            Self::Active => "active",
            Self::Rejected => "rejected",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "publishing" => Ok(Self::Publishing),
            "active" => Ok(Self::Active),
            "rejected" => Ok(Self::Rejected),
            _ => Err(format!("unsupported route state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub id: RouteId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub gateway_node_id: NodeId,
    pub hostname: RouteHostname,
    pub path_prefix: RoutePath,
    pub domain_claim_id: Option<DomainClaimId>,
    pub domain_pattern: Option<DomainNamePattern>,
    pub gateway_certificate_id: Option<GatewayCertificateId>,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub port_name: RoutePortName,
    pub upstream: UpstreamEndpoint,
    pub state: RouteState,
    pub gateway_revision: Option<u64>,
    pub gateway_command_id: Option<NodeCommandId>,
    pub snapshot_digest: Option<String>,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
}

impl Route {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: RouteId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        gateway_node_id: NodeId,
        hostname: RouteHostname,
        path_prefix: RoutePath,
        domain_claim_id: DomainClaimId,
        domain_pattern: DomainNamePattern,
        gateway_certificate_id: GatewayCertificateId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        port_name: RoutePortName,
        upstream: UpstreamEndpoint,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if !domain_pattern.covers(&hostname) {
            return Err("domain claim pattern does not cover the route hostname".into());
        }
        let created_at = canonical_timestamp(created_at);
        Ok(Self {
            id,
            organization_id,
            project_id,
            environment_id,
            gateway_node_id,
            hostname,
            path_prefix,
            domain_claim_id: Some(domain_claim_id),
            domain_pattern: Some(domain_pattern),
            gateway_certificate_id: Some(gateway_certificate_id),
            workload_id,
            workload_revision_id,
            port_name,
            upstream,
            state: RouteState::Pending,
            gateway_revision: None,
            gateway_command_id: None,
            snapshot_digest: None,
            failure: None,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            activated_at: None,
        })
    }

    pub fn stage(
        &mut self,
        revision: u64,
        command_id: NodeCommandId,
        snapshot_digest: String,
        staged_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let staged_at = canonical_timestamp(staged_at);
        if self.state != RouteState::Pending
            || self.gateway_revision.is_some()
            || self.gateway_command_id.is_some()
            || self.snapshot_digest.is_some()
            || self.domain_claim_id.is_none()
            || self.domain_pattern.is_none()
            || self.gateway_certificate_id.is_none()
        {
            return Err("route publication has already been staged".into());
        }
        if revision == 0 || !valid_sha256(&snapshot_digest) {
            return Err("route publication identity is invalid".into());
        }
        self.ensure_time(staged_at)?;
        self.state = RouteState::Publishing;
        self.gateway_revision = Some(revision);
        self.gateway_command_id = Some(command_id);
        self.snapshot_digest = Some(snapshot_digest);
        self.aggregate_version += 1;
        self.updated_at = staged_at;
        Ok(())
    }

    pub fn prepare_cutover(
        &self,
        workload_revision_id: WorkloadRevisionId,
        upstream: UpstreamEndpoint,
        gateway_certificate_id: GatewayCertificateId,
        prepared_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let prepared_at = canonical_timestamp(prepared_at);
        if self.state != RouteState::Active
            || self.gateway_revision.is_none()
            || self.gateway_command_id.is_none()
            || self.snapshot_digest.is_none()
            || self.failure.is_some()
            || self.activated_at.is_none()
        {
            return Err("only an active route can prepare a target cutover".into());
        }
        if workload_revision_id == self.workload_revision_id {
            return Err("route cutover must select a different immutable revision".into());
        }
        self.ensure_time(prepared_at)?;

        let mut candidate = self.clone();
        candidate.workload_revision_id = workload_revision_id;
        candidate.upstream = upstream;
        candidate.state = RouteState::Pending;
        candidate.gateway_revision = None;
        candidate.gateway_command_id = None;
        candidate.snapshot_digest = None;
        candidate.gateway_certificate_id = Some(gateway_certificate_id);
        candidate.failure = None;
        candidate.updated_at = prepared_at;
        candidate.activated_at = None;
        Ok(candidate)
    }

    pub fn apply_gateway_acknowledgement(
        &mut self,
        acknowledgement: &NodeGatewayAck,
    ) -> Result<(), String> {
        acknowledgement.validate()?;
        let acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
        if acknowledgement.node_id != self.gateway_node_id.as_uuid()
            || Some(acknowledgement.command_id) != self.gateway_command_id.map(|id| id.as_uuid())
            || Some(acknowledgement.revision) != self.gateway_revision
            || self.snapshot_digest.as_deref() != Some(&acknowledgement.snapshot_digest)
        {
            return Err(
                "Gateway acknowledgement does not match the staged route publication".into(),
            );
        }
        self.ensure_time(acknowledged_at)?;
        let next_state = match acknowledgement.state {
            GatewayAckState::Applied => RouteState::Active,
            GatewayAckState::Rejected => RouteState::Rejected,
        };
        if self.state == next_state {
            return Ok(());
        }
        if self.state != RouteState::Publishing {
            return Err(
                "route cannot accept a Gateway acknowledgement in its current state".into(),
            );
        }
        self.state = next_state;
        self.failure = acknowledgement.message.clone();
        self.activated_at = (next_state == RouteState::Active).then_some(acknowledged_at);
        self.aggregate_version += 1;
        self.updated_at = acknowledged_at;
        Ok(())
    }

    fn ensure_time(&self, at: DateTime<Utc>) -> Result<(), String> {
        if at < self.updated_at {
            return Err("route transition time regressed".into());
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
