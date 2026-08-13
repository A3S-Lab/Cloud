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
    BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn membership_controller(
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&command_bus);
    let change_bus = Arc::clone(&command_bus);
    let list_bus = Arc::clone(&query_bus);
    let get_bus = Arc::clone(&query_bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_guard(OrganizationAdministratorGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::IDENTITY_WRITE])?
        .get(
            "/{organization_id}/memberships",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let request_id = request_id(&request)?;
                    match bus.execute(ListMemberships { organization_id }).await? {
                        Ok(memberships) => BootResponse::json(
                            &memberships
                                .into_iter()
                                .map(MembershipResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/memberships/{membership_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let membership_id =
                        MembershipId::from_uuid(request.param_as::<Uuid>("membership_id")?);
                    let request_id = request_id(&request)?;
                    match bus
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
            },
        )?
        .post(
            "/{organization_id}/memberships",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateMembershipRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(CreateMembership {
                            organization_id,
                            principal_kind: body.principal_kind,
                            name: body.name,
                            role: body.role,
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
                                &MembershipMutationResponse::from(result),
                            )
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/memberships/{membership_id}/role",
            move |request: BootRequest| {
                let bus = Arc::clone(&change_bus);
                async move {
                    let body: ChangeMembershipRoleRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let membership_id =
                        MembershipId::from_uuid(request.param_as::<Uuid>("membership_id")?);
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(ChangeMembershipRole {
                            organization_id,
                            membership_id,
                            role: body.role,
                            expected_version: body.expected_version,
                            actor_principal_id: actor.principal_id,
                            actor_is_platform_admin: actor.is_platform_admin,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json(&MembershipMutationResponse::from(result)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/memberships/{membership_id}/revocation",
            move |request: BootRequest| {
                let bus = Arc::clone(&command_bus);
                async move {
                    let body: RevokeMembershipRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let membership_id =
                        MembershipId::from_uuid(request.param_as::<Uuid>("membership_id")?);
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(RevokeMembership {
                            organization_id,
                            membership_id,
                            expected_version: body.expected_version,
                            actor_principal_id: actor.principal_id,
                            actor_is_platform_admin: actor.is_platform_admin,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json(&MembershipMutationResponse::from(result)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
