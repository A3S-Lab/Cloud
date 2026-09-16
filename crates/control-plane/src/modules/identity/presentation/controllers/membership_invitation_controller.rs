use crate::modules::identity::application::commands::accept_membership_invitation::AcceptMembershipInvitation;
use crate::modules::identity::application::commands::create_membership_invitation::CreateMembershipInvitation;
use crate::modules::identity::application::commands::revoke_membership_invitation::RevokeMembershipInvitation;
use crate::modules::identity::application::queries::get_membership_invitation::GetMembershipInvitation;
use crate::modules::identity::application::queries::list_membership_invitations::ListMembershipInvitations;
use crate::modules::identity::application::queries::list_my_membership_invitations::ListMyMembershipInvitations;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    CreateMembershipInvitationRequest, MembershipInvitationAcceptanceResponse,
    MembershipInvitationMutationResponse, MembershipInvitationResponse,
    MembershipInvitationVersionRequest,
};
use crate::modules::identity::presentation::request_context::{
    actor, mutation_identity, request_id,
};
use crate::modules::identity::presentation::{
    OrganizationAdministratorGuard, OrganizationTenantGuard,
};
use crate::modules::shared_kernel::domain::{MembershipInvitationId, OrganizationId, PrincipalId};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn membership_invitation_administration_controller(
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(MembershipInvitationAdministrationController {
        command_bus,
        query_bus,
    })
    .controller()
}

pub fn membership_invitation_self_query_controller(
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(MembershipInvitationSelfQueryController { query_bus }).controller()
}

pub fn membership_invitation_acceptance_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    Arc::new(MembershipInvitationAcceptanceController { command_bus }).controller()
}

#[derive(Debug, Clone)]
struct MembershipInvitationAdministrationController {
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[use_guard(OrganizationAdministratorGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::IDENTITY_WRITE])]
impl MembershipInvitationAdministrationController {
    #[get("/{organization_id}/membership-invitations", raw)]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(ListMembershipInvitations { organization_id })
            .await?
        {
            Ok(invitations) => BootResponse::json(
                &invitations
                    .into_iter()
                    .map(MembershipInvitationResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/membership-invitations/{invitation_id}", raw)]
    async fn get(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let invitation_id =
            MembershipInvitationId::from_uuid(request.param_as::<Uuid>("invitation_id")?);
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(GetMembershipInvitation {
                organization_id,
                invitation_id,
            })
            .await?
        {
            Ok(invitation) => BootResponse::json(&MembershipInvitationResponse::from(invitation)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/membership-invitations", raw)]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateMembershipInvitationRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(CreateMembershipInvitation {
                organization_id,
                principal_id: PrincipalId::from_uuid(body.principal_id),
                role: body.role,
                expires_at: body.expires_at,
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
                    &MembershipInvitationMutationResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post(
        "/{organization_id}/membership-invitations/{invitation_id}/revocation",
        raw
    )]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        let body: MembershipInvitationVersionRequest = request.json_with_content_type()?;
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let invitation_id =
            MembershipInvitationId::from_uuid(request.param_as::<Uuid>("invitation_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(RevokeMembershipInvitation {
                organization_id,
                invitation_id,
                expected_version: body.expected_version,
                actor_principal_id: actor.principal_id,
                idempotency_key,
                request_id,
            })
            .await?
        {
            Ok(result) => {
                BootResponse::json(&MembershipInvitationMutationResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[derive(Debug, Clone)]
struct MembershipInvitationSelfQueryController {
    query_bus: Arc<QueryBus>,
}

#[controller("/membership-invitations")]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl MembershipInvitationSelfQueryController {
    #[get("/", raw)]
    async fn list_mine(&self, request: BootRequest) -> Result<BootResponse> {
        let actor = actor(&request)?;
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(ListMyMembershipInvitations {
                principal_id: actor.principal_id,
            })
            .await?
        {
            Ok(invitations) => BootResponse::json(
                &invitations
                    .into_iter()
                    .map(MembershipInvitationResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[derive(Debug, Clone)]
struct MembershipInvitationAcceptanceController {
    command_bus: Arc<CommandBus>,
}

#[controller("/membership-invitations")]
#[metadata("auth.scopes", vec![ApiTokenScope::IDENTITY_WRITE])]
impl MembershipInvitationAcceptanceController {
    #[post("/{invitation_id}/acceptance", raw)]
    async fn accept(&self, request: BootRequest) -> Result<BootResponse> {
        let body: MembershipInvitationVersionRequest = request.json_with_content_type()?;
        let invitation_id =
            MembershipInvitationId::from_uuid(request.param_as::<Uuid>("invitation_id")?);
        let actor = actor(&request)?;
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
            .execute(AcceptMembershipInvitation {
                invitation_id,
                expected_version: body.expected_version,
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
                    &MembershipInvitationAcceptanceResponse::from(result),
                )
            }
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_membership_invitation_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn membership_invitation_controllers_register_scoped_routes_via_nest_macros() {
        let admin = membership_invitation_administration_controller(
            Arc::new(CommandBus::new()),
            Arc::new(QueryBus::new()),
        )
        .expect("membership invitation admin nest controller");
        assert_eq!(admin.prefix(), "/organizations");
        let admin_routes = admin.routes();
        assert_eq!(admin_routes.len(), 4);
        assert_eq!(admin_routes[0].method(), HttpMethod::Get);
        assert_eq!(
            admin_routes[0].path(),
            "/organizations/{organization_id}/membership-invitations"
        );
        assert_eq!(
            admin_routes[3].path(),
            "/organizations/{organization_id}/membership-invitations/{invitation_id}/revocation"
        );

        let mine = membership_invitation_self_query_controller(Arc::new(QueryBus::new()))
            .expect("membership invitation self nest controller");
        assert_eq!(mine.prefix(), "/membership-invitations");
        assert_eq!(mine.routes().len(), 1);
        assert_eq!(mine.routes()[0].path(), "/membership-invitations");

        let accept = membership_invitation_acceptance_controller(Arc::new(CommandBus::new()))
            .expect("membership invitation acceptance nest controller");
        assert_eq!(accept.prefix(), "/membership-invitations");
        assert_eq!(accept.routes().len(), 1);
        assert_eq!(
            accept.routes()[0].path(),
            "/membership-invitations/{invitation_id}/acceptance"
        );
        assert_eq!(
            admin_routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::IDENTITY_WRITE])
        );
    }
}
