use super::renewal_subject_id;
use crate::modules::edge::domain::{
    GatewayCertificate, GatewayCertificateConvergence, GatewayCertificateConvergenceReason,
    GatewayCertificateConvergenceState, GatewayCertificateState, GatewayPublication,
    GatewayPublicationState, Route, RouteHostname, RoutePath, RouteState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GatewayCertificateId, NodeId, OrganizationId, ProjectId,
    RouteId, WorkloadId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayCertificateExpiryStatus {
    Expiring,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayCertificateExpiryChanged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub route_id: RouteId,
    pub workload_id: WorkloadId,
    pub node_id: NodeId,
    pub hostname: String,
    pub path_prefix: String,
    pub certificate_gateway_revision: u64,
    pub renewal_gateway_revision: u64,
    pub previous_certificate_id: GatewayCertificateId,
    pub replacement_certificate_id: GatewayCertificateId,
    pub active_certificate_id: GatewayCertificateId,
    pub active_certificate_expires_at: DateTime<Utc>,
    pub status: GatewayCertificateExpiryStatus,
}

impl GatewayCertificateExpiryChanged {
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
            return Err("Gateway certificate expiry fact omitted its replacement identity".into());
        };
        let (event_key, status, expected_publication_state, expected_active_id, occurred_at) =
            match convergence.state {
                GatewayCertificateConvergenceState::Pending => (
                    "edge.gateway-certificate.expiring",
                    GatewayCertificateExpiryStatus::Expiring,
                    GatewayPublicationState::Pending,
                    convergence.previous_certificate_id,
                    convergence.staged_at,
                ),
                GatewayCertificateConvergenceState::Applied => (
                    "edge.gateway-certificate.expiry-resolved",
                    GatewayCertificateExpiryStatus::Resolved,
                    GatewayPublicationState::Applied,
                    replacement_certificate_id,
                    convergence.acknowledged_at.ok_or_else(|| {
                        "resolved Gateway certificate expiry omitted its observation time"
                            .to_owned()
                    })?,
                ),
                GatewayCertificateConvergenceState::Rejected
                | GatewayCertificateConvergenceState::Unavailable => return Ok(Vec::new()),
            };
        let material = active_certificate.material.as_ref().ok_or_else(|| {
            "active Gateway certificate expiry fact omitted certificate material".to_owned()
        })?;
        material.validate()?;
        let expected_acknowledged_at =
            if convergence.state == GatewayCertificateConvergenceState::Pending {
                None
            } else {
                Some(occurred_at)
            };
        let certificate_revision_is_valid = match status {
            GatewayCertificateExpiryStatus::Expiring => {
                active_certificate.gateway_revision < convergence.gateway_revision
            }
            GatewayCertificateExpiryStatus::Resolved => {
                active_certificate.gateway_revision == convergence.gateway_revision
            }
        };
        if convergence.organization_id != active_certificate.organization_id
            || convergence.node_id != active_certificate.node_id
            || active_certificate.id != expected_active_id
            || active_certificate.state != GatewayCertificateState::Ready
            || !certificate_revision_is_valid
            || publication.node_id != convergence.node_id
            || publication.revision != convergence.gateway_revision
            || publication.command_id != convergence.gateway_command_id
            || publication.snapshot_digest != convergence.snapshot_digest
            || publication.state != expected_publication_state
            || publication.acknowledged_at != expected_acknowledged_at
            || routes.len() != convergence.retained_routes.len()
        {
            return Err("Gateway certificate expiry fact identity is inconsistent".into());
        }

        let mut events = Vec::with_capacity(routes.len());
        for (route, version) in routes.iter().zip(&convergence.retained_routes) {
            if route.domain_claim_id.is_none() {
                return Err("Gateway certificate expiry Route lost its domain Claim".into());
            }
            if route.id != version.route_id
                || route.aggregate_version != version.aggregate_version
                || route.organization_id != convergence.organization_id
                || route.gateway_node_id != convergence.node_id
                || route.state != RouteState::Active
            {
                return Err("Gateway certificate expiry Route scope is inconsistent".into());
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
                certificate_gateway_revision: active_certificate.gateway_revision,
                renewal_gateway_revision: convergence.gateway_revision,
                previous_certificate_id: convergence.previous_certificate_id,
                replacement_certificate_id,
                active_certificate_id: active_certificate.id,
                active_certificate_expires_at: material.expires_at,
                status,
            };
            events.push(DomainEventEnvelope {
                event_id: Self::deterministic_event_id(
                    aggregate_id,
                    event_key,
                    active_certificate.id,
                ),
                event_key: event_key.into(),
                schema_version: 1,
                scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                    organization_id: convergence.organization_id.as_uuid(),
                },
                aggregate_id,
                aggregate_version: certificate_expiry_aggregate_version(
                    active_certificate.gateway_revision,
                    status,
                )?,
                occurred_at,
                correlation_id: publication.command_correlation_id,
                causation_id: None,
                payload: serde_json::to_value(payload).map_err(|error| error.to_string())?,
            });
        }
        Ok(events)
    }

    pub(crate) fn same_firing_identity(
        existing: &DomainEventEnvelope,
        candidate: &DomainEventEnvelope,
    ) -> Result<bool, String> {
        let existing_payload = Self::decode_envelope(existing)?;
        let candidate_payload = Self::decode_envelope(candidate)?;
        if existing_payload.status != GatewayCertificateExpiryStatus::Expiring
            || candidate_payload.status != GatewayCertificateExpiryStatus::Expiring
        {
            return Err("Gateway certificate expiry retry selected a non-firing event".into());
        }
        Ok(existing.event_id == candidate.event_id
            && existing.organization_id() == candidate.organization_id()
            && existing.aggregate_id == candidate.aggregate_id
            && existing.aggregate_version == candidate.aggregate_version
            && existing_payload.organization_id == candidate_payload.organization_id
            && existing_payload.project_id == candidate_payload.project_id
            && existing_payload.environment_id == candidate_payload.environment_id
            && existing_payload.route_id == candidate_payload.route_id
            && existing_payload.workload_id == candidate_payload.workload_id
            && existing_payload.node_id == candidate_payload.node_id
            && existing_payload.certificate_gateway_revision
                == candidate_payload.certificate_gateway_revision
            && existing_payload.previous_certificate_id
                == candidate_payload.previous_certificate_id
            && existing_payload.active_certificate_id == candidate_payload.active_certificate_id
            && existing_payload.active_certificate_expires_at
                == candidate_payload.active_certificate_expires_at)
    }

    pub(crate) fn decode_envelope(event: &DomainEventEnvelope) -> Result<Self, String> {
        let expected_status = match event.event_key.as_str() {
            "edge.gateway-certificate.expiring" => GatewayCertificateExpiryStatus::Expiring,
            "edge.gateway-certificate.expiry-resolved" => GatewayCertificateExpiryStatus::Resolved,
            _ => return Err("Gateway certificate expiry event key is unsupported".into()),
        };
        let payload: Self = serde_json::from_value(event.payload.clone())
            .map_err(|error| format!("Gateway certificate expiry payload is invalid: {error}"))?;
        let hostname = RouteHostname::parse(payload.hostname.clone())?;
        let path_prefix = RoutePath::parse(payload.path_prefix.clone())?;
        let aggregate_id = renewal_subject_id(payload.route_id, payload.node_id);
        let (expected_active_certificate_id, valid_revisions) = match expected_status {
            GatewayCertificateExpiryStatus::Expiring => (
                payload.previous_certificate_id,
                payload.certificate_gateway_revision < payload.renewal_gateway_revision,
            ),
            GatewayCertificateExpiryStatus::Resolved => (
                payload.replacement_certificate_id,
                payload.certificate_gateway_revision == payload.renewal_gateway_revision,
            ),
        };
        if event.schema_version != 1
            || event.event_id.is_nil()
            || event.organization_id().is_none()
            || event.aggregate_id.is_nil()
            || event.correlation_id.is_nil()
            || event.causation_id.is_some()
            || canonical_timestamp(event.occurred_at) != event.occurred_at
            || canonical_timestamp(payload.active_certificate_expires_at)
                != payload.active_certificate_expires_at
            || Some(payload.organization_id.as_uuid()) != event.organization_id()
            || payload.organization_id.as_uuid().is_nil()
            || payload.project_id.as_uuid().is_nil()
            || payload.environment_id.as_uuid().is_nil()
            || payload.route_id.as_uuid().is_nil()
            || payload.workload_id.as_uuid().is_nil()
            || payload.node_id.as_uuid().is_nil()
            || payload.previous_certificate_id.as_uuid().is_nil()
            || payload.replacement_certificate_id.as_uuid().is_nil()
            || payload.active_certificate_id.as_uuid().is_nil()
            || hostname.as_str() != payload.hostname
            || path_prefix.as_str() != payload.path_prefix
            || payload.replacement_certificate_id == payload.previous_certificate_id
            || payload.status != expected_status
            || payload.active_certificate_id != expected_active_certificate_id
            || !valid_revisions
            || certificate_expiry_aggregate_version(
                payload.certificate_gateway_revision,
                payload.status,
            )? != event.aggregate_version
            || aggregate_id != event.aggregate_id
            || Self::deterministic_event_id(
                aggregate_id,
                &event.event_key,
                payload.active_certificate_id,
            ) != event.event_id
        {
            return Err("Gateway certificate expiry event identity is inconsistent".into());
        }
        Ok(payload)
    }

    pub(crate) fn deterministic_event_id(
        aggregate_id: Uuid,
        event_key: &str,
        active_certificate_id: GatewayCertificateId,
    ) -> Uuid {
        Uuid::new_v5(
            &aggregate_id,
            format!("{event_key}:{active_certificate_id}").as_bytes(),
        )
    }
}

pub fn certificate_expiry_aggregate_version(
    certificate_gateway_revision: u64,
    status: GatewayCertificateExpiryStatus,
) -> Result<u64, String> {
    let doubled = certificate_gateway_revision.checked_mul(2).ok_or_else(|| {
        "Gateway certificate expiry aggregate version exceeds supported range".to_owned()
    })?;
    match status {
        GatewayCertificateExpiryStatus::Expiring if doubled > 0 => Ok(doubled),
        GatewayCertificateExpiryStatus::Resolved => doubled.checked_sub(1).ok_or_else(|| {
            "Gateway certificate expiry aggregate version must be positive".to_owned()
        }),
        GatewayCertificateExpiryStatus::Expiring => {
            Err("Gateway certificate expiry aggregate version must be positive".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_encoded_versions_order_repeated_certificate_lifecycles() {
        let first_firing =
            certificate_expiry_aggregate_version(1, GatewayCertificateExpiryStatus::Expiring)
                .expect("first firing version");
        let first_resolution =
            certificate_expiry_aggregate_version(3, GatewayCertificateExpiryStatus::Resolved)
                .expect("first resolution version");
        let second_firing =
            certificate_expiry_aggregate_version(3, GatewayCertificateExpiryStatus::Expiring)
                .expect("second firing version");
        let second_resolution =
            certificate_expiry_aggregate_version(5, GatewayCertificateExpiryStatus::Resolved)
                .expect("second resolution version");

        assert!(first_firing < first_resolution);
        assert!(first_resolution < second_firing);
        assert!(second_firing < second_resolution);
        assert!(
            certificate_expiry_aggregate_version(0, GatewayCertificateExpiryStatus::Expiring)
                .is_err()
        );
        assert!(certificate_expiry_aggregate_version(
            u64::MAX,
            GatewayCertificateExpiryStatus::Resolved
        )
        .is_err());
    }
}
