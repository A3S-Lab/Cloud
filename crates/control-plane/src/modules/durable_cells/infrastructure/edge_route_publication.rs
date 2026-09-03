use crate::modules::durable_cells::application::{
    DurableCellPublishedCertificate, DurableCellPublishedRoute, DurableCellRoutePublication,
    DurableCellRoutePublicationRequest, IDurableCellRoutePublicationPort,
};
use crate::modules::edge::{PublishRoute, PublishRouteHandler, PublishRouteResult};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use async_trait::async_trait;

/// Anti-corruption adapter from the Edge Route/Gateway application authority
/// to the Durable Cells consumer port.
///
/// The adapter is the only production Durable Cells integration point that
/// knows Edge command or aggregate types. It translates the committed Edge
/// result into an aggregate-free projection before returning it to the
/// Durable Cells Application layer.
#[derive(Clone)]
pub struct EdgeDurableCellRoutePublicationAdapter {
    routes: PublishRouteHandler,
}

impl EdgeDurableCellRoutePublicationAdapter {
    pub fn new(routes: PublishRouteHandler) -> Self {
        Self { routes }
    }
}

#[async_trait]
impl IDurableCellRoutePublicationPort for EdgeDurableCellRoutePublicationAdapter {
    async fn publish(
        &self,
        request: &DurableCellRoutePublicationRequest,
    ) -> ApplicationResult<DurableCellRoutePublication> {
        request.validate().map_err(ApplicationError::Invalid)?;
        // PublishRoute currently does not read its CQRS module context. Keep
        // that framework detail inside this adapter rather than leaking it
        // through the consumer-owned port.
        let result = self
            .routes
            .execute(
                PublishRoute {
                    organization_id: request.organization_id,
                    project_id: request.project_id,
                    environment_id: request.environment_id,
                    gateway_scope_id: request.gateway_scope_id,
                    workload_revision_id: request.workload_revision_id,
                    domain_claim_id: request.domain_claim_id,
                    hostname: request.hostname.clone(),
                    path_prefix: request.path_prefix.clone(),
                    port_name: request.port_name.clone(),
                    idempotency_key: request.idempotency_key.clone(),
                    request_id: request.request_id,
                    requested_at: request.requested_at,
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .map_err(|error| ApplicationError::Internal(error.to_string()))??;
        let projection = project_edge_result(result);
        projection
            .validate_against(request)
            .map_err(ApplicationError::Conflict)?;
        Ok(projection)
    }
}

fn project_edge_result(result: PublishRouteResult) -> DurableCellRoutePublication {
    let PublishRouteResult {
        publication,
        command_replayed,
    } = result;
    let route = publication.route;
    let certificate = publication.certificate;
    let material = certificate.material;
    let (serial_number, fingerprint, issued_at, expires_at) =
        material.map_or((None, None, None, None), |material| {
            (
                Some(material.serial_number),
                Some(material.fingerprint),
                Some(material.issued_at),
                Some(material.expires_at),
            )
        });

    DurableCellRoutePublication {
        route: DurableCellPublishedRoute {
            id: route.id,
            organization_id: route.organization_id,
            project_id: route.project_id,
            environment_id: route.environment_id,
            gateway_scope_id: route.gateway_scope_id,
            gateway_node_id: route.gateway_node_id,
            hostname: route.hostname.as_str().to_owned(),
            path_prefix: route.path_prefix.as_str().to_owned(),
            domain_claim_id: route.domain_claim_id,
            domain_pattern: route
                .domain_pattern
                .map(|pattern| pattern.as_str().to_owned()),
            gateway_certificate_id: route.gateway_certificate_id,
            workload_id: route.workload_id,
            workload_revision_id: route.target.workload_revision_id,
            runtime_unit_id: route.target.runtime_unit_id,
            runtime_generation: route.target.runtime_generation,
            port_name: route.target.port_name.as_str().to_owned(),
            upstream_origin: route.target.upstream.as_str().to_owned(),
            target_observed_at: route.target.observed_at,
            state: route.state.as_str().to_owned(),
            gateway_revision: route.gateway_revision,
            gateway_command_id: route.gateway_command_id,
            snapshot_digest: route.snapshot_digest,
            failure: route.failure,
            aggregate_version: route.aggregate_version,
            created_at: route.created_at,
            updated_at: route.updated_at,
            activated_at: route.activated_at,
        },
        certificate: DurableCellPublishedCertificate {
            id: certificate.id,
            organization_id: certificate.organization_id,
            node_id: certificate.node_id,
            domain_claim_ids: certificate.domain_claim_ids,
            dns_names: certificate.request.dns_names,
            gateway_revision: certificate.gateway_revision,
            gateway_command_id: certificate.gateway_command_id,
            snapshot_digest: certificate.snapshot_digest,
            state: certificate.state.as_str().to_owned(),
            serial_number,
            fingerprint,
            issued_at,
            expires_at,
            failure: certificate.failure,
            aggregate_version: certificate.aggregate_version,
            created_at: certificate.created_at,
            updated_at: certificate.updated_at,
            ready_at: certificate.ready_at,
            revoked_at: certificate.revoked_at,
        },
        replayed: publication.replayed,
        command_replayed,
    }
}
