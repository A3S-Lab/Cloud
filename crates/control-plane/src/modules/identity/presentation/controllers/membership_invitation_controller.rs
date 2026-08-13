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
    BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn membership_invitation_administration_controller(
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&command_bus);
    let list_bus = Arc::clone(&query_bus);
    let get_bus = Arc::clone(&query_bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_guard(OrganizationAdministratorGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::IDENTITY_WRITE])?
        .get(
            "/{organization_id}/membership-invitations",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let request_id = request_id(&request)?;
                    match bus
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
            },
        )?
        .get(
            "/{organization_id}/membership-invitations/{invitation_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let invitation_id = MembershipInvitationId::from_uuid(
                        request.param_as::<Uuid>("invitation_id")?,
                    );
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetMembershipInvitation {
                            organization_id,
                            invitation_id,
                        })
                        .await?
                    {
                        Ok(invitation) => {
                            BootResponse::json(&MembershipInvitationResponse::from(invitation))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/membership-invitations",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateMembershipInvitationRequest =
                        request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(CreateMembershipInvitation {
                            organization_id,
                            principal_id: PrincipalId::from_uuid(body.principal_id),
                            role: body.role,
                            expires_at: body.expires_at,
                            actor_principal_id: actor.principal_id,
                            actor_is_platform_admin: actor.is_platform_admin,
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
            },
        )?
        .post(
            "/{organization_id}/membership-invitations/{invitation_id}/revocation",
            move |request: BootRequest| {
                let bus = Arc::clone(&command_bus);
                async move {
                    let body: MembershipInvitationVersionRequest =
                        request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let invitation_id = MembershipInvitationId::from_uuid(
                        request.param_as::<Uuid>("invitation_id")?,
                    );
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(RevokeMembershipInvitation {
                            organization_id,
                            invitation_id,
                            expected_version: body.expected_version,
                            actor_principal_id: actor.principal_id,
                            actor_is_platform_admin: actor.is_platform_admin,
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
            },
        )
}

pub fn membership_invitation_self_query_controller(
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/membership-invitations")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .get("/", move |request: BootRequest| {
            let bus = Arc::clone(&query_bus);
            async move {
                let actor = actor(&request)?;
                let request_id = request_id(&request)?;
                match bus
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
        })
}

pub fn membership_invitation_acceptance_controller(
    command_bus: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/membership-invitations")?
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::IDENTITY_WRITE])?
        .post(
            "/{invitation_id}/acceptance",
            move |request: BootRequest| {
                let bus = Arc::clone(&command_bus);
                async move {
                    let body: MembershipInvitationVersionRequest =
                        request.json_with_content_type()?;
                    let invitation_id = MembershipInvitationId::from_uuid(
                        request.param_as::<Uuid>("invitation_id")?,
                    );
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
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
            },
        )
}
