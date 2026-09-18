use crate::access_projection::edge_access;
use crate::modules::edge::application::{CreateDomainClaim, RevokeDomainClaim, VerifyDomainClaim};
use crate::modules::edge::presentation::dto::{
    CreateDomainClaimRequest, DomainClaimMutationResponse, RevokeDomainClaimRequest,
    VerifyDomainClaimRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, OrganizationId, ProjectId,
};
use crate::presentation::{OrganizationTenantGuard, resource_access_evaluator, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use super::request::request_identity;

pub fn domain_claim_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(DomainClaimCommandsController { bus }).controller()
}

#[derive(Debug, Clone)]
struct DomainClaimCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::ROUTE_WRITE])]
impl DomainClaimCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/domain-claims",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateDomainClaimRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let access = edge_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(CreateDomainClaim {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                access,
                pattern: body.pattern,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 201 },
                &DomainClaimMutationResponse::new(result.claim, result.replayed),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/domain-claims/{claim_id}/verify", raw)]
    async fn verify(&self, request: BootRequest) -> Result<BootResponse> {
        let body: VerifyDomainClaimRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(VerifyDomainClaim {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                claim_id: DomainClaimId::from_uuid(request.param_as::<Uuid>("claim_id")?),
                proof: body.proof,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 202 },
                &DomainClaimMutationResponse::new(result.claim, result.replayed),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/domain-claims/{claim_id}/revoke", raw)]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RevokeDomainClaimRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(RevokeDomainClaim {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                claim_id: DomainClaimId::from_uuid(request.param_as::<Uuid>("claim_id")?),
                reason: body.reason,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json_with_status(
                if result.replayed { 200 } else { 202 },
                &DomainClaimMutationResponse::new(result.claim, result.replayed),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_domain_claim_commands_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn domain_claim_commands_controller_registers_scoped_posts_via_nest_macros() {
        let controller = domain_claim_commands_controller(Arc::new(CommandBus::new()))
            .expect("domain claim nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/domain-claims"
        );
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/domain-claims/{claim_id}/verify"
        );
        assert_eq!(
            routes[2].path(),
            "/organizations/{organization_id}/domain-claims/{claim_id}/revoke"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::ROUTE_WRITE])
        );
    }
}
