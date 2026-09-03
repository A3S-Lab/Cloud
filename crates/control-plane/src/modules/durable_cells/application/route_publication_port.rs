use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId,
    NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// Consumer-owned request for publishing one Durable Cell public route.
///
/// Edge remains the sole Route/Gateway authority. Durable Cells sends only
/// immutable correlation identities and the public endpoint intent through
/// this port; no Edge aggregate or handler crosses the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellRoutePublicationRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub gateway_scope_id: GatewayScopeId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub domain_claim_id: DomainClaimId,
    pub hostname: String,
    pub path_prefix: String,
    pub port_name: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl DurableCellRoutePublicationRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.gateway_scope_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.domain_claim_id.as_uuid().is_nil()
            || self.request_id.is_nil()
        {
            return Err("Durable Cell route publication identity is invalid".into());
        }
        if self.hostname.trim().is_empty()
            || self.path_prefix.trim().is_empty()
            || self.port_name.trim().is_empty()
            || self.idempotency_key.trim().is_empty()
        {
            return Err("Durable Cell route publication contains an empty boundary value".into());
        }
        Ok(())
    }
}

/// Aggregate-free Route projection returned by the Edge owner port.
///
/// The projection deliberately contains only the public route/certificate
/// evidence needed by Durable Cells and its presentation layer. It is not an
/// alias of Edge's Route or GatewayCertificate model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellRoutePublication {
    pub route: DurableCellPublishedRoute,
    pub certificate: DurableCellPublishedCertificate,
    pub replayed: bool,
    pub command_replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellPublishedRoute {
    pub id: RouteId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub gateway_scope_id: GatewayScopeId,
    pub gateway_node_id: NodeId,
    pub hostname: String,
    pub path_prefix: String,
    pub domain_claim_id: Option<DomainClaimId>,
    pub domain_pattern: Option<String>,
    pub gateway_certificate_id: Option<GatewayCertificateId>,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub port_name: String,
    pub upstream_origin: String,
    pub target_observed_at: DateTime<Utc>,
    pub state: String,
    pub gateway_revision: Option<u64>,
    pub gateway_command_id: Option<NodeCommandId>,
    pub snapshot_digest: Option<String>,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DurableCellPublishedCertificate {
    pub id: GatewayCertificateId,
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub domain_claim_ids: Vec<DomainClaimId>,
    pub dns_names: Vec<String>,
    pub gateway_revision: u64,
    pub gateway_command_id: NodeCommandId,
    pub snapshot_digest: String,
    pub state: String,
    pub serial_number: Option<String>,
    pub fingerprint: Option<String>,
    pub issued_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub ready_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl DurableCellRoutePublication {
    /// Validate the returned owner projection before it is used or exposed.
    ///
    /// This is intentionally consumer-side validation: an adapter may be
    /// replaced by another Edge implementation, so Durable Cells must never
    /// trust a foreign aggregate shape merely because it compiled.
    pub fn validate_against(
        &self,
        request: &DurableCellRoutePublicationRequest,
    ) -> Result<(), String> {
        request.validate()?;
        let route = &self.route;
        let certificate = &self.certificate;
        if route.id.as_uuid().is_nil()
            || route.organization_id.as_uuid().is_nil()
            || route.project_id.as_uuid().is_nil()
            || route.environment_id.as_uuid().is_nil()
            || route.gateway_scope_id.as_uuid().is_nil()
            || route.gateway_node_id.as_uuid().is_nil()
            || route.workload_id.as_uuid().is_nil()
            || route.workload_revision_id.as_uuid().is_nil()
            || route
                .domain_claim_id
                .is_some_and(|value| value.as_uuid().is_nil())
            || route
                .gateway_certificate_id
                .is_some_and(|value| value.as_uuid().is_nil())
            || route
                .gateway_command_id
                .is_some_and(|value| value.as_uuid().is_nil())
            || certificate.id.as_uuid().is_nil()
            || certificate.organization_id.as_uuid().is_nil()
            || certificate.node_id.as_uuid().is_nil()
            || certificate.gateway_command_id.as_uuid().is_nil()
        {
            return Err("Edge returned a route publication with an invalid identity".into());
        }
        if route.organization_id != request.organization_id
            || route.project_id != request.project_id
            || route.environment_id != request.environment_id
            || route.gateway_scope_id != request.gateway_scope_id
            || route.workload_id != request.workload_id
            || route.workload_revision_id != request.workload_revision_id
            || route.domain_claim_id != Some(request.domain_claim_id)
            || route.hostname != request.hostname
            || route.path_prefix != request.path_prefix
            || route.port_name != request.port_name
            || certificate.organization_id != request.organization_id
            || route.gateway_certificate_id != Some(certificate.id)
            || route.gateway_node_id != certificate.node_id
            || route.gateway_command_id != Some(certificate.gateway_command_id)
            || route.gateway_revision != Some(certificate.gateway_revision)
            || route.snapshot_digest.as_deref() != Some(certificate.snapshot_digest.as_str())
            || !certificate
                .domain_claim_ids
                .contains(&request.domain_claim_id)
            || !certificate
                .dns_names
                .iter()
                .any(|name| name == &request.hostname)
        {
            return Err(
                "Edge returned a Route outside the exact Durable Cell public deployment binding"
                    .into(),
            );
        }
        if route.runtime_unit_id.trim().is_empty()
            || route.runtime_generation == 0
            || route.port_name.trim().is_empty()
            || route.upstream_origin.trim().is_empty()
            || route.state.trim().is_empty()
            || route.aggregate_version == 0
            || certificate.gateway_revision == 0
            || certificate.snapshot_digest.trim().is_empty()
            || certificate.state.trim().is_empty()
            || certificate.domain_claim_ids.is_empty()
            || certificate.dns_names.is_empty()
            || certificate.aggregate_version == 0
        {
            return Err("Edge returned incomplete Route publication evidence".into());
        }
        for (label, timestamp) in [
            ("route target", route.target_observed_at),
            ("route created", route.created_at),
            ("route updated", route.updated_at),
            ("certificate created", certificate.created_at),
            ("certificate updated", certificate.updated_at),
        ] {
            if timestamp != canonical_timestamp(timestamp) {
                return Err(format!("Edge returned a non-canonical {label} timestamp"));
            }
        }
        for timestamp in route
            .activated_at
            .into_iter()
            .chain(certificate.issued_at)
            .chain(certificate.expires_at)
            .chain(certificate.ready_at)
            .chain(certificate.revoked_at)
        {
            if timestamp != canonical_timestamp(timestamp) {
                return Err("Edge returned a non-canonical Route publication timestamp".into());
            }
        }
        Ok(())
    }
}

/// Durable Cells' sole application boundary for Edge Route/Gateway
/// publication. Implementations live in an outer adapter and return only the
/// immutable projection above.
#[async_trait]
pub trait IDurableCellRoutePublicationPort: Send + Sync {
    async fn publish(
        &self,
        request: &DurableCellRoutePublicationRequest,
    ) -> ApplicationResult<DurableCellRoutePublication>;
}
