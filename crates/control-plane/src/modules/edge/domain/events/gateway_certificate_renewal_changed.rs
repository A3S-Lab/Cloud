use crate::modules::edge::domain::{
    GatewayCertificate, GatewayCertificateConvergence, GatewayCertificateConvergenceReason,
    GatewayCertificateConvergenceState, GatewayCertificateState, GatewayPublication,
    GatewayPublicationState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayCertificateId, NodeId, OrganizationId, ProjectId, RouteId, WorkloadId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayCertificateRenewalStatus {
    Failed,
    Renewed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayCertificateRenewalFailureKind {
    Rejected,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayCertificateRenewalChanged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub route_id: RouteId,
    pub workload_id: WorkloadId,
    pub node_id: NodeId,
    pub hostname: String,
    pub path_prefix: String,
    pub gateway_revision: u64,
    pub previous_certificate_id: GatewayCertificateId,
    pub replacement_certificate_id: GatewayCertificateId,
    pub active_certificate_id: GatewayCertificateId,
    pub active_certificate_expires_at: DateTime<Utc>,
    pub status: GatewayCertificateRenewalStatus,
    pub failure_kind: Option<GatewayCertificateRenewalFailureKind>,
}

impl GatewayCertificateRenewalChanged {
    pub fn envelopes(
        convergence: &GatewayCertificateConvergence,
        publication: &GatewayPublication,
        active_certificate: &GatewayCertificate,
        routes: &[Route],
    ) -> Result<Vec<DomainEventEnvelope>, String> {
        if convergence.reason != GatewayCertificateConvergenceReason::Renewal {
            return Ok(Vec::new());
        }
        let Some(replacement_certificate_id) = convergence.replacement_certificate_id else {
            return Err("Gateway certificate renewal omitted its replacement identity".into());
        };
        let (event_key, status, failure_kind, expected_publication_state, expected_active_id) =
            match convergence.state {
                GatewayCertificateConvergenceState::Pending => return Ok(Vec::new()),
                GatewayCertificateConvergenceState::Applied => (
                    "edge.gateway-certificate.renewed",
                    GatewayCertificateRenewalStatus::Renewed,
                    None,
                    GatewayPublicationState::Applied,
                    replacement_certificate_id,
                ),
                GatewayCertificateConvergenceState::Rejected => (
                    "edge.gateway-certificate.renewal-failed",
                    GatewayCertificateRenewalStatus::Failed,
                    Some(GatewayCertificateRenewalFailureKind::Rejected),
                    GatewayPublicationState::Rejected,
                    convergence.previous_certificate_id,
                ),
                GatewayCertificateConvergenceState::Unavailable => (
                    "edge.gateway-certificate.renewal-failed",
                    GatewayCertificateRenewalStatus::Failed,
                    Some(GatewayCertificateRenewalFailureKind::Unavailable),
                    GatewayPublicationState::Unavailable,
                    convergence.previous_certificate_id,
                ),
            };
        let occurred_at = convergence.acknowledged_at.ok_or_else(|| {
            "terminal Gateway certificate renewal omitted its observation time".to_owned()
        })?;
        let material = active_certificate.material.as_ref().ok_or_else(|| {
            "active Gateway certificate renewal fact omitted certificate material".to_owned()
        })?;
        if convergence.organization_id != active_certificate.organization_id
            || convergence.node_id != active_certificate.node_id
            || active_certificate.id != expected_active_id
            || active_certificate.state != GatewayCertificateState::Ready
            || publication.node_id != convergence.node_id
            || publication.revision != convergence.gateway_revision
            || publication.command_id != convergence.gateway_command_id
            || publication.snapshot_digest != convergence.snapshot_digest
            || publication.state != expected_publication_state
            || publication.acknowledged_at != Some(occurred_at)
            || routes.len() != convergence.retained_routes.len()
        {
            return Err("Gateway certificate renewal fact identity is inconsistent".into());
        }

        let mut events = Vec::with_capacity(routes.len());
        for (route, version) in routes.iter().zip(&convergence.retained_routes) {
            if route.domain_claim_id.is_none() {
                return Err("Gateway certificate renewal Route lost its domain Claim".into());
            }
            if route.id != version.route_id
                || route.aggregate_version != version.aggregate_version
                || route.organization_id != convergence.organization_id
                || route.gateway_node_id != convergence.node_id
                || route.state != RouteState::Active
            {
                return Err("Gateway certificate renewal Route scope is inconsistent".into());
            }
            let aggregate_id = renewal_subject_id(route.id, convergence.node_id);
            let payload = Self {
                organization_id: convergence.organization_id,
                project_id: route.project_id,
                environment_id: route.environment_id,
                route_id: route.id,
                workload_id: route.workload_id,
                node_id: convergence.node_id,
                hostname: route.hostname.as_str().into(),
                path_prefix: route.path_prefix.as_str().into(),
                gateway_revision: convergence.gateway_revision,
                previous_certificate_id: convergence.previous_certificate_id,
                replacement_certificate_id,
                active_certificate_id: active_certificate.id,
                active_certificate_expires_at: material.expires_at,
                status,
                failure_kind,
            };
            events.push(DomainEventEnvelope {
                event_id: Uuid::now_v7(),
                event_key: event_key.into(),
                schema_version: 1,
                scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                    organization_id: convergence.organization_id.as_uuid(),
                },
                aggregate_id,
                aggregate_version: convergence.gateway_revision,
                occurred_at,
                correlation_id: publication.command_correlation_id,
                causation_id: None,
                payload: serde_json::to_value(payload).map_err(|error| error.to_string())?,
            });
        }
        Ok(events)
    }
}

pub fn renewal_subject_id(route_id: RouteId, node_id: NodeId) -> Uuid {
    Uuid::new_v5(&route_id.as_uuid(), node_id.as_uuid().as_bytes())
}
