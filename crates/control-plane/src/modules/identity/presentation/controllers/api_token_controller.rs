use crate::modules::identity::application::commands::create_api_token::CreateApiToken;
use crate::modules::identity::application::commands::revoke_api_token::RevokeApiToken;
use crate::modules::identity::application::queries::get_api_token::GetApiToken;
use crate::modules::identity::application::queries::list_api_tokens::ListApiTokens;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::dto::{
    ApiTokenReadResponse, ApiTokenResponse, CreateApiTokenRequest,
};
use crate::modules::identity::presentation::request_context::{
    actor, mutation_identity, request_id,
};
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{ApiTokenId, OrganizationId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
    AUTH_SCOPES_METADATA,
};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

pub fn api_token_controller(
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&command_bus);
    let list_bus = Arc::clone(&query_bus);
    let get_bus = Arc::clone(&query_bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::TOKEN_WRITE])?
        .get(
            "/{organization_id}/api-tokens",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let request_id = request_id(&request)?;
                    match bus.execute(ListApiTokens { organization_id }).await? {
                        Ok(tokens) => BootResponse::json(
                            &tokens
                                .into_iter()
                                .map(ApiTokenReadResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/api-tokens/{token_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let token_id = ApiTokenId::from_uuid(request.param_as::<Uuid>("token_id")?);
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetApiToken {
                            organization_id,
                            token_id,
                        })
                        .await?
                    {
                        Ok(token) => BootResponse::json(&ApiTokenReadResponse::from(token)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/api-tokens",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateApiTokenRequest = request.json_with_content_type()?;
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let principal = request.require_auth_principal()?;
                    let actor = actor(&request)?;
                    let issuer_scopes = principal
                        .scopes()
                        .map(ApiTokenScope::parse)
                        .collect::<std::result::Result<BTreeSet<_>, _>>()
                        .map_err(BootError::Internal)?;
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(CreateApiToken {
                            organization_id,
                            principal_id: body
                                .principal_id
                                .map(crate::modules::shared_kernel::domain::PrincipalId::from_uuid)
                                .unwrap_or(actor.principal_id),
                            issuer_principal_id: actor.principal_id,
                            name: body.name,
                            token_secret: body.token,
                            scopes: body.scopes,
                            issuer_scopes,
                            expires_at: body.expires_at,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => {
                            let status = if result.replayed { 200 } else { 201 };
                            BootResponse::json_with_status(status, &ApiTokenResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .delete(
            "/{organization_id}/api-tokens/{token_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&command_bus);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let token_id = ApiTokenId::from_uuid(request.param_as::<Uuid>("token_id")?);
                    let (idempotency_key, request_id) = mutation_identity(&request)?;
                    match bus
                        .execute(RevokeApiToken {
                            organization_id,
                            token_id,
                            idempotency_key,
                            request_id,
                        })
                        .await?
                    {
                        Ok(result) => BootResponse::json(&ApiTokenResponse::from(result)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
