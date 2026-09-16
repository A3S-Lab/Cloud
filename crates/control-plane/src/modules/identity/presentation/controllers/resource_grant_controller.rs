use crate::modules::identity::application::commands::create_resource_grant::CreateResourceGrant;
use crate::modules::identity::application::commands::revoke_resource_grant::RevokeResourceGrant;
use crate::modules::identity::application::queries::get_resource_grant::GetResourceGrant;
use crate::modules::identity::application::queries::list_resource_grants::ListResourceGrants;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    CreateResourceGrantRequest, ResourceGrantMutationResponse, ResourceGrantResponse,
    RevokeResourceGrantRequest,
};
use crate::modules::identity::presentation::request_context::{
    actor, mutation_identity, request_id,
};
use crate::modules::identity::presentation::{
    OrganizationAdministratorGuard, OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{MembershipId, OrganizationId, ResourceGrantId};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn resource_grant_controller(
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ResourceGrantController {
        command_bus,
        query_bus,
    })
    .controller()
}

#[derive(Debug, Clone)]
struct ResourceGrantController {
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[use_guard(OrganizationAdministratorGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::IDENTITY_WRITE])]
impl ResourceGrantController {
    #[get("/{organization_id}/memberships/{membership_id}/resource-grants", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let membership_id = MembershipId::from_uuid(request.param_as::<Uuid>("membership_id")?);
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(ListResourceGrants {
                organization_id,
                membership_id: Some(membership_id),
            })
            .await?
        {
            Ok(grants) => BootResponse::json(
                &grants
                    .into_iter()
                    .map(ResourceGrantResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/resource-grants/{resource_grant_id}", raw)]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let resource_grant_id =
            ResourceGrantId::from_uuid(request.param_as::<Uuid>("resource_grant_id")?);
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(GetResourceGrant {
                organization_id,
                resource_grant_id,
            })
            .await?
        {
            Ok(grant) => BootResponse::json(&ResourceGrantResponse::from(grant)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/memberships/{membership_id}/resource-grants", raw)]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateResourceGrantRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let membership_id = MembershipId::from_uuid(request.param_as::<Uuid>("membership_id")?);
        let scope = body.scope.try_into().map_err(BootError::BadRequest)?;
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(CreateResourceGrant {
                organization_id,
                membership_id,
                scope,
                actor_principal_id: actor.principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(
                    status,
                    &ResourceGrantMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/resource-grants/{resource_grant_id}/revocation", raw)]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RevokeResourceGrantRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let resource_grant_id =
            ResourceGrantId::from_uuid(request.param_as::<Uuid>("resource_grant_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(RevokeResourceGrant {
                organization_id,
                resource_grant_id,
                expected_version: body.expected_version,
                actor_principal_id: actor.principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json(&ResourceGrantMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_resource_grant_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn resource_grant_controller_registers_scoped_routes_via_nest_macros() {
        let controller = resource_grant_controller(
            Arc::new(CommandBus::new()),
            Arc::new(QueryBus::new()),
        )
        .expect("resource grant nest controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 4);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/memberships/{membership_id}/resource-grants"
        );
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/resource-grants/{resource_grant_id}"
        );
        assert_eq!(routes[2].method(), HttpMethod::Post);
        assert_eq!(
            routes[2].path(),
            "/organizations/{organization_id}/memberships/{membership_id}/resource-grants"
        );
        assert_eq!(
            routes[3].path(),
            "/organizations/{organization_id}/resource-grants/{resource_grant_id}/revocation"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::IDENTITY_WRITE])
        );
    }
}
