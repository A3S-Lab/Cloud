use crate::access_projection::edge_access;
use crate::modules::edge::application::{
    GetDomainClaim, ListDomainClaims, ListGatewayCertificates,
};
use crate::modules::edge::presentation::dto::{DomainClaimResponse, GatewayCertificateResponse};
use crate::modules::identity::presentation::{
    DeferredResourceScope, OrganizationTenantGuard, resource_access_evaluator,
    with_deferred_resource_scope,
};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn domain_claim_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_bus = Arc::clone(&bus);
    let certificate_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/domain-claims",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let environment_id =
                        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
                    let access = edge_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    match bus
                        .execute(ListDomainClaims {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id,
                            environment_id,
                            access,
                        })
                        .await?
                    {
                        Ok(claims) => BootResponse::json(
                            &claims
                                .into_iter()
                                .map(DomainClaimResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .route(with_deferred_resource_scope(
            RouteDefinition::get(
                "/{organization_id}/domain-claims/{claim_id}",
                move |request: BootRequest| {
                    let bus = Arc::clone(&get_bus);
                    async move {
                        let request_id = request_id(&request)?;
                        let access = edge_access(&resource_access_evaluator(
                            &request.require_auth_principal()?,
                        )?);
                        match bus
                            .execute(GetDomainClaim {
                                organization_id: OrganizationId::from_uuid(
                                    request.param_as::<Uuid>("organization_id")?,
                                ),
                                claim_id: DomainClaimId::from_uuid(
                                    request.param_as::<Uuid>("claim_id")?,
                                ),
                                access,
                            })
                            .await?
                        {
                            Ok(claim) => BootResponse::json(&DomainClaimResponse::from(claim)),
                            Err(error) => application_error_response(error, request_id),
                        }
                    }
                },
            )?,
            DeferredResourceScope::Project,
        )?)?
        .get(
            "/{organization_id}/gateway-certificates",
            move |request: BootRequest| {
                let bus = Arc::clone(&certificate_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let access = edge_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    match bus
                        .execute(ListGatewayCertificates {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            access,
                        })
                        .await?
                    {
                        Ok(certificates) => BootResponse::json(
                            &certificates
                                .into_iter()
                                .map(GatewayCertificateResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

use super::request::request_id;
