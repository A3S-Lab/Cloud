use crate::modules::identity::application::commands::change_membership_role::ChangeMembershipRole;
use crate::modules::identity::application::commands::create_membership::CreateMembership;
use crate::modules::identity::application::commands::revoke_membership::RevokeMembership;
use crate::modules::identity::application::queries::get_membership::GetMembership;
use crate::modules::identity::application::queries::list_memberships::ListMemberships;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    ChangeMembershipRoleRequest, CreateMembershipRequest, MembershipMutationResponse,
    MembershipResponse, RevokeMembershipRequest,
};
use crate::modules::identity::presentation::request_context::{
    actor, mutation_identity, request_id,
};
use crate::modules::identity::presentation::{
    OrganizationAdministratorGuard, OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{MembershipId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn membership_controller(
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(MembershipController {
        command_bus,
        query_bus,
    })
    .controller()
}

#[derive(Debug, Clone)]
struct MembershipController {
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[use_guard(OrganizationAdministratorGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::IDENTITY_WRITE])]
impl MembershipController {
    #[get("/{organization_id}/memberships", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(ListMemberships { organization_id })
            .await?
        {
            Ok(memberships) => BootResponse::json(
                &memberships
                    .into_iter()
                    .map(MembershipResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/memberships/{membership_id}", raw)]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let membership_id = MembershipId::from_uuid(request.param_as::<Uuid>("membership_id")?);
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(GetMembership {
                organization_id,
                membership_id,
            })
            .await?
        {
            Ok(membership) => BootResponse::json(&MembershipResponse::from(membership)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/memberships", raw)]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateMembershipRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(CreateMembership {
                organization_id,
                principal_kind: body.principal_kind,
                name: body.name,
                role: body.role,
                actor_principal_id: actor.principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                BootResponse::json_with_status(status, &MembershipMutationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/memberships/{membership_id}/role", raw)]
    async fn change_role(&self, request: BootRequest) -> Result<BootResponse> {
        let body: ChangeMembershipRoleRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let membership_id = MembershipId::from_uuid(request.param_as::<Uuid>("membership_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(ChangeMembershipRole {
                organization_id,
                membership_id,
                role: body.role,
                expected_version: body.expected_version,
                actor_principal_id: actor.principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json(&MembershipMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/memberships/{membership_id}/revocation", raw)]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RevokeMembershipRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let membership_id = MembershipId::from_uuid(request.param_as::<Uuid>("membership_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(RevokeMembership {
                organization_id,
                membership_id,
                expected_version: body.expected_version,
                actor_principal_id: actor.principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => BootResponse::json(&MembershipMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_membership_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn membership_controller_registers_scoped_routes_via_nest_macros() {
        let controller = membership_controller(
            Arc::new(CommandBus::new()),
            Arc::new(QueryBus::new()),
        )
        .expect("membership nest controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 5);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/memberships"
        );
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/memberships/{membership_id}"
        );
        assert_eq!(routes[2].method(), HttpMethod::Post);
        assert_eq!(
            routes[2].path(),
            "/organizations/{organization_id}/memberships"
        );
        assert_eq!(
            routes[3].path(),
            "/organizations/{organization_id}/memberships/{membership_id}/role"
        );
        assert_eq!(
            routes[4].path(),
            "/organizations/{organization_id}/memberships/{membership_id}/revocation"
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
