use crate::modules::edge::application::PublishRouteResult;
use crate::modules::edge::domain::Route;
use crate::modules::edge::presentation::dto::GatewayCertificateResponse;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub gateway_node_id: Uuid,
    pub hostname: String,
    pub path_prefix: String,
    pub domain_claim_id: Option<Uuid>,
    pub domain_pattern: Option<String>,
    pub gateway_certificate_id: Option<Uuid>,
    pub workload_id: Uuid,
    pub workload_revision_id: Uuid,
    pub port_name: String,
    pub state: String,
    pub gateway_revision: Option<u64>,
    pub gateway_command_id: Option<Uuid>,
    pub snapshot_digest: Option<String>,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePublicationResponse {
    pub route: RouteResponse,
    pub certificate: GatewayCertificateResponse,
    pub replayed: bool,
    pub command_replayed: bool,
}

impl From<Route> for RouteResponse {
    fn from(route: Route) -> Self {
        Self {
            id: route.id.as_uuid(),
            organization_id: route.organization_id.as_uuid(),
            project_id: route.project_id.as_uuid(),
            environment_id: route.environment_id.as_uuid(),
            gateway_node_id: route.gateway_node_id.as_uuid(),
            hostname: route.hostname.as_str().to_owned(),
            path_prefix: route.path_prefix.as_str().to_owned(),
            domain_claim_id: route.domain_claim_id.map(|id| id.as_uuid()),
            domain_pattern: route.domain_pattern.map(|pattern| pattern.as_str().into()),
            gateway_certificate_id: route.gateway_certificate_id.map(|id| id.as_uuid()),
            workload_id: route.workload_id.as_uuid(),
            workload_revision_id: route.workload_revision_id.as_uuid(),
            port_name: route.port_name.as_str().to_owned(),
            state: route.state.as_str().into(),
            gateway_revision: route.gateway_revision,
            gateway_command_id: route.gateway_command_id.map(|id| id.as_uuid()),
            snapshot_digest: route.snapshot_digest,
            failure: route.failure,
            aggregate_version: route.aggregate_version,
            created_at: route.created_at,
            updated_at: route.updated_at,
            activated_at: route.activated_at,
        }
    }
}

impl From<PublishRouteResult> for RoutePublicationResponse {
    fn from(result: PublishRouteResult) -> Self {
        Self {
            route: result.publication.route.into(),
            certificate: result.publication.certificate.into(),
            replayed: result.publication.replayed,
            command_replayed: result.command_replayed,
        }
    }
}
