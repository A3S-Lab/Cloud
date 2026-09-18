use crate::access_projection::edge_access;
use crate::modules::edge::application::{
    GetDomainClaim, ListDomainClaims, ListGatewayCertificates,
};
use crate::modules::edge::presentation::dto::{DomainClaimResponse, GatewayCertificateResponse};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, OrganizationId, ProjectId,
};
use crate::presentation::{DeferredResourceScope, OrganizationTenantGuard, resource_access_evaluator, with_deferred_resource_scope, application_error_response};
use a3s_boot::{
    controller, get, use_guard, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
    RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

use super::request::request_id;

pub fn domain_claim_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    // List routes use Nest macros; org-scoped claim get keeps deferred project admission.
    let mut controller = Arc::new(DomainClaimQueriesController {
        bus: Arc::clone(&bus),
    })
    .controller()?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/domain-claims/{claim_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
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
    )?)?;
    Ok(controller)
}

#[derive(Debug, Clone)]
struct DomainClaimQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl DomainClaimQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/domain-claims",
        raw
    )]
    async fn list_claims(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let environment_id =
            EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
        let access = edge_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
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

    #[get("/{organization_id}/gateway-certificates", raw)]
    async fn list_certificates(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let access = edge_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
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
}

#[cfg(test)]
mod nest_macro_domain_claim_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn domain_claim_queries_controller_registers_lists_via_nest_and_deferred_get() {
        let controller = domain_claim_queries_controller(Arc::new(QueryBus::new()))
            .expect("domain claim nest query controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/domain-claims"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path() == "/organizations/{organization_id}/domain-claims/{claim_id}"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path() == "/organizations/{organization_id}/gateway-certificates"
        }));
    }
}
