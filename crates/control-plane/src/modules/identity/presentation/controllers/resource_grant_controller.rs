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
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn resource_grant_controller(
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
            "/{organization_id}/memberships/{membership_id}/resource-grants",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let membership_id =
                        MembershipId::from_uuid(request.param_as::<Uuid>("membership_id")?);
                    let request_id = request_id(&request)?;
                    match bus
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
            },
        )?
        .get(
            "/{organization_id}/resource-grants/{resource_grant_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let resource_grant_id =
                        ResourceGrantId::from_uuid(request.param_as::<Uuid>("resource_grant_id")?);
                    let request_id = request_id(&request)?;
                    match bus
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
            },
        )?
        .post(
            "/{organization_id}/memberships/{membership_id}/resource-grants",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateResourceGrantRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let membership_id =
                        MembershipId::from_uuid(request.param_as::<Uuid>("membership_id")?);
                    let scope = body.scope.try_into().map_err(BootError::BadRequest)?;
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
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
            },
        )?
        .post(
            "/{organization_id}/resource-grants/{resource_grant_id}/revocation",
            move |request: BootRequest| {
                let bus = Arc::clone(&command_bus);
                async move {
                    let body: RevokeResourceGrantRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let resource_grant_id =
                        ResourceGrantId::from_uuid(request.param_as::<Uuid>("resource_grant_id")?);
                    let actor = actor(&request)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
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
                        Ok(result) => {
                            BootResponse::json(&ResourceGrantMutationResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
