use crate::modules::edge::domain::{DomainNamePattern, RouteHostname, RoutePath, RouteTarget};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId,
    NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId, WorkloadId,
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
    pub gateway_scope_id: GatewayScopeId,
    pub gateway_node_id: NodeId,
    pub hostname: RouteHostname,
    pub path_prefix: RoutePath,
    pub domain_claim_id: Option<DomainClaimId>,
    pub domain_pattern: Option<DomainNamePattern>,
    pub gateway_certificate_id: Option<GatewayCertificateId>,
    pub workload_id: WorkloadId,
    #[serde(flatten)]
    pub target: RouteTarget,
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
        gateway_scope_id: GatewayScopeId,
        gateway_node_id: NodeId,
        hostname: RouteHostname,
        path_prefix: RoutePath,
        domain_claim_id: DomainClaimId,
        domain_pattern: DomainNamePattern,
        gateway_certificate_id: GatewayCertificateId,
        workload_id: WorkloadId,
        target: RouteTarget,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if !domain_pattern.covers(&hostname) {
            return Err("domain claim pattern does not cover the route hostname".into());
        }
        let created_at = canonical_timestamp(created_at);
        target.validate_for(workload_id)?;
        if target.observed_at > created_at {
            return Err("route target observation is newer than route creation".into());
        }
        Ok(Self {
            id,
            organization_id,
            project_id,
            environment_id,
            gateway_scope_id,
            gateway_node_id,
            hostname,
            path_prefix,
            domain_claim_id: Some(domain_claim_id),
            domain_pattern: Some(domain_pattern),
            gateway_certificate_id: Some(gateway_certificate_id),
            workload_id,
            target,
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
        target: RouteTarget,
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
        target.validate_for(self.workload_id)?;
        if target.workload_revision_id == self.target.workload_revision_id
            || target.runtime_generation <= self.target.runtime_generation
            || target.port_name != self.target.port_name
            || target.observed_at > prepared_at
        {
            return Err(
                "route cutover must select a newer generation of a different immutable revision"
                    .into(),
            );
        }
        self.ensure_time(prepared_at)?;

        let mut candidate = self.clone();
        candidate.target = target;
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

    pub fn validate_target_binding(&self) -> Result<(), String> {
        self.target.validate_for(self.workload_id)?;
        if self.target.observed_at > self.updated_at {
            return Err("route target observation is newer than the route projection".into());
        }
        Ok(())
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

    pub fn bind_gateway_certificate(
        &mut self,
        revision: u64,
        command_id: NodeCommandId,
        snapshot_digest: String,
        certificate_id: GatewayCertificateId,
        bound_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        let bound_at = canonical_timestamp(bound_at);
        if self.state != RouteState::Active
            || self.failure.is_some()
            || self.activated_at.is_none()
            || self.domain_claim_id.is_none()
            || self.domain_pattern.is_none()
            || revision == 0
            || !valid_sha256(&snapshot_digest)
        {
            return Err("only a complete active TLS route can bind a Gateway certificate".into());
        }
        self.ensure_time(bound_at)?;
        if self.gateway_revision == Some(revision)
            && self.gateway_command_id == Some(command_id)
            && self.snapshot_digest.as_deref() == Some(snapshot_digest.as_str())
            && self.gateway_certificate_id == Some(certificate_id)
        {
            return Ok(false);
        }
        self.gateway_revision = Some(revision);
        self.gateway_command_id = Some(command_id);
        self.snapshot_digest = Some(snapshot_digest);
        self.gateway_certificate_id = Some(certificate_id);
        self.aggregate_version += 1;
        self.updated_at = bound_at;
        Ok(true)
    }

    pub fn reject_for_domain_revocation(
        &mut self,
        revision: u64,
        command_id: NodeCommandId,
        snapshot_digest: String,
        rejected_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let rejected_at = canonical_timestamp(rejected_at);
        if self.state != RouteState::Active
            || self.failure.is_some()
            || self.activated_at.is_none()
            || revision == 0
            || !valid_sha256(&snapshot_digest)
        {
            return Err("only an active route can converge revoked domain ownership".into());
        }
        self.ensure_time(rejected_at)?;
        self.state = RouteState::Rejected;
        self.gateway_revision = Some(revision);
        self.gateway_command_id = Some(command_id);
        self.snapshot_digest = Some(snapshot_digest);
        self.failure = Some("domain ownership is no longer verified".into());
        self.aggregate_version += 1;
        self.updated_at = rejected_at;
        self.activated_at = None;
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
