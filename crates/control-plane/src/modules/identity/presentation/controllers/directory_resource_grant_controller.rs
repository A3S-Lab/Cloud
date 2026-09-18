use crate::modules::identity::application::commands::create_directory_resource_grant::CreateDirectoryResourceGrant;
use crate::modules::identity::application::commands::revoke_directory_resource_grant::RevokeDirectoryResourceGrant;
use crate::modules::identity::application::queries::get_directory_resource_grant::GetDirectoryResourceGrant;
use crate::modules::identity::application::queries::list_directory_resource_grants::ListDirectoryResourceGrants;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    CreateDirectoryResourceGrantRequest, DirectoryResourceGrantMutationResponse,
    DirectoryResourceGrantResponse, RevokeResourceGrantRequest,
};
use crate::modules::identity::presentation::request_context::{
    actor, mutation_identity, request_id,
};
use crate::modules::identity::presentation::{
    OrganizationAdministratorGuard, OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{OrganizationId, ResourceGrantId};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError, BootRequest,
    BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn directory_resource_grant_controller(
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(DirectoryResourceGrantController {
        command_bus,
        query_bus,
    })
    .controller()
}

#[derive(Debug, Clone)]
struct DirectoryResourceGrantController {
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[use_guard(OrganizationAdministratorGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::IDENTITY_WRITE])]
impl DirectoryResourceGrantController {
    #[get("/{organization_id}/directory-resource-grants", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(ListDirectoryResourceGrants { organization_id })
            .await?
        {
            Ok(grants) => BootResponse::json(
                &grants
                    .into_iter()
                    .map(DirectoryResourceGrantResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/directory-resource-grants/{directory_resource_grant_id}",
        raw
    )]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let resource_grant_id =
            ResourceGrantId::from_uuid(request.param_as::<Uuid>("directory_resource_grant_id")?);
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(GetDirectoryResourceGrant {
                organization_id,
                resource_grant_id,
            })
            .await?
        {
            Ok(grant) => BootResponse::json(&DirectoryResourceGrantResponse::from(grant)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/directory-resource-grants", raw)]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateDirectoryResourceGrantRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let scope = body.scope.try_into().map_err(BootError::BadRequest)?;
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(CreateDirectoryResourceGrant {
                organization_id,
                subject_ref: body.subject_ref,
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
                    &DirectoryResourceGrantMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/directory-resource-grants/{directory_resource_grant_id}/revocation",
        raw
    )]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RevokeResourceGrantRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let resource_grant_id =
            ResourceGrantId::from_uuid(request.param_as::<Uuid>("directory_resource_grant_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(RevokeDirectoryResourceGrant {
                organization_id,
                resource_grant_id,
                expected_version: body.expected_version,
                actor_principal_id: actor.principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                BootResponse::json(&DirectoryResourceGrantMutationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_directory_resource_grant_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn directory_resource_grant_controller_registers_scoped_routes_via_nest_macros() {
        let controller = directory_resource_grant_controller(
            Arc::new(CommandBus::new()),
            Arc::new(QueryBus::new()),
        )
        .expect("directory resource grant nest controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 4);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/directory-resource-grants"
        );
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/directory-resource-grants/{directory_resource_grant_id}"
        );
        assert_eq!(routes[2].method(), HttpMethod::Post);
        assert_eq!(
            routes[2].path(),
            "/organizations/{organization_id}/directory-resource-grants"
        );
        assert_eq!(
            routes[3].path(),
            "/organizations/{organization_id}/directory-resource-grants/{directory_resource_grant_id}/revocation"
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
