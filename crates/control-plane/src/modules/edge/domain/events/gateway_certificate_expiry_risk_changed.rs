use crate::modules::edge::domain::{
    GatewayCertificateExpiryRisk, GatewayCertificateExpiryRiskState, Route, RouteState,
    GATEWAY_CERTIFICATE_EXPIRY_RISK_WINDOW_SECONDS,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayCertificateId, NodeId, OrganizationId, ProjectId, RouteId, WorkloadId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayCertificateExpiryRiskChanged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub route_id: RouteId,
    pub workload_id: WorkloadId,
    pub node_id: NodeId,
    pub hostname: String,
    pub path_prefix: String,
    pub active_gateway_revision: u64,
    pub active_certificate_id: GatewayCertificateId,
    pub active_certificate_expires_at: DateTime<Utc>,
    pub risk_window_seconds: u64,
    pub state: GatewayCertificateExpiryRiskState,
    pub generation: u64,
    pub previous_at_risk_certificate_id: Option<GatewayCertificateId>,
    pub previous_at_risk_certificate_expires_at: Option<DateTime<Utc>>,
}

impl GatewayCertificateExpiryRiskChanged {
    pub fn envelope(
        previous: Option<&GatewayCertificateExpiryRisk>,
        current: &GatewayCertificateExpiryRisk,
        route: &Route,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, String> {
        current.validate()?;
        validate_transition(previous, current)?;
        if correlation_id.is_nil()
            || route.state != RouteState::Active
            || route.organization_id != current.organization_id
            || route.id != current.route_id
            || route.gateway_node_id != current.node_id
            || route.gateway_certificate_id != Some(current.active_certificate_id)
            || route.gateway_revision != Some(current.gateway_revision)
        {
            return Err("Gateway certificate expiry-risk fact scope is inconsistent".into());
        }
        let event_key = match current.state {
            GatewayCertificateExpiryRiskState::AtRisk => "edge.gateway-certificate.expiry-at-risk",
            GatewayCertificateExpiryRiskState::Clear => {
                "edge.gateway-certificate.expiry-risk-cleared"
            }
        };
        let payload = Self {
            organization_id: current.organization_id,
            project_id: route.project_id,
            environment_id: route.environment_id,
            route_id: current.route_id,
            workload_id: route.workload_id,
            node_id: current.node_id,
            hostname: route.hostname.as_str().into(),
            path_prefix: route.path_prefix.as_str().into(),
            active_gateway_revision: current.gateway_revision,
            active_certificate_id: current.active_certificate_id,
            active_certificate_expires_at: current.active_certificate_expires_at,
            risk_window_seconds: GATEWAY_CERTIFICATE_EXPIRY_RISK_WINDOW_SECONDS,
            state: current.state,
            generation: current.generation,
            previous_at_risk_certificate_id: current.previous_at_risk_certificate_id,
            previous_at_risk_certificate_expires_at: current
                .previous_at_risk_certificate_expires_at,
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: current.organization_id.as_uuid(),
            aggregate_id: expiry_risk_subject_id(current.route_id, current.node_id),
            aggregate_version: current.generation,
            occurred_at: current.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload).map_err(|error| error.to_string())?,
        })
    }
}

pub fn expiry_risk_subject_id(route_id: RouteId, node_id: NodeId) -> Uuid {
    let mut name = b"a3s-cloud:gateway-certificate-expiry-risk:".to_vec();
    name.extend_from_slice(node_id.as_uuid().as_bytes());
    Uuid::new_v5(&route_id.as_uuid(), &name)
}

fn validate_transition(
    previous: Option<&GatewayCertificateExpiryRisk>,
    current: &GatewayCertificateExpiryRisk,
) -> Result<(), String> {
    match previous {
        None if current.generation == 1
            && current.state == GatewayCertificateExpiryRiskState::AtRisk => {}
        Some(previous) => {
            previous.validate()?;
            if previous.organization_id != current.organization_id
                || previous.route_id != current.route_id
                || previous.node_id != current.node_id
                || previous.generation.checked_add(1) != Some(current.generation)
                || previous.updated_at > current.updated_at
            {
                return Err("Gateway certificate expiry-risk fact generation is invalid".into());
            }
            match current.state {
                GatewayCertificateExpiryRiskState::AtRisk
                    if previous.state == GatewayCertificateExpiryRiskState::Clear
                        || previous.active_certificate_id != current.active_certificate_id => {}
                GatewayCertificateExpiryRiskState::Clear
                    if previous.state == GatewayCertificateExpiryRiskState::AtRisk
                        && current.previous_at_risk_certificate_id
                            == Some(previous.active_certificate_id)
                        && current.previous_at_risk_certificate_expires_at
                            == Some(previous.active_certificate_expires_at) => {}
                _ => {
                    return Err("Gateway certificate expiry-risk fact transition is invalid".into())
                }
            }
        }
        _ => return Err("Gateway certificate expiry-risk fact generation is invalid".into()),
    }
    Ok(())
}
