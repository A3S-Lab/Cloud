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
    controller, delete, get, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootError,
    BootRequest, BootResponse, CommandBus, ControllerDefinition, QueryBus, Result,
};
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

pub fn api_token_controller(
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
) -> Result<ControllerDefinition> {
    Arc::new(ApiTokenController {
        command_bus,
        query_bus,
    })
    .controller()
}

#[derive(Debug, Clone)]
struct ApiTokenController {
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::TOKEN_WRITE])]
impl ApiTokenController {
    #[get("/{organization_id}/api-tokens", raw)]
    async fn list_tokens(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let request_id = request_id(&request)?;
        match self
            .query_bus
            .execute(ListApiTokens { organization_id })
            .await?
        {
            Ok(tokens) => BootResponse::json(
                &tokens
                    .into_iter()
                    .map(ApiTokenReadResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get("/{organization_id}/api-tokens/{token_id}", raw)]
    async fn get_token(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let token_id = ApiTokenId::from_uuid(request.param_as::<Uuid>("token_id")?);
        let request_id = request_id(&request)?;
        match self
            .query_bus
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

    #[post("/{organization_id}/api-tokens", raw)]
    async fn create_token(&self, request: BootRequest) -> Result<BootResponse> {
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
        match self
            .command_bus
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

    #[delete("/{organization_id}/api-tokens/{token_id}", raw)]
    async fn revoke_token(&self, request: BootRequest) -> Result<BootResponse> {
        let organization_id =
            OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
        let token_id = ApiTokenId::from_uuid(request.param_as::<Uuid>("token_id")?);
        let (idempotency_key, request_id) = mutation_identity(&request)?;
        match self
            .command_bus
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
}

#[cfg(test)]
mod nest_macro_api_token_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::BTreeSet;

    #[test]
    fn api_token_controller_registers_scoped_guarded_routes_via_nest_macros() {
        let controller = api_token_controller(Arc::new(CommandBus::new()), Arc::new(QueryBus::new()))
            .expect("api token nest controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 4);
        let paths: BTreeSet<_> = routes
            .iter()
            .map(|route| (route.method(), route.path().to_string()))
            .collect();
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/api-tokens".into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Get,
            "/organizations/{organization_id}/api-tokens/{token_id}".into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Post,
            "/organizations/{organization_id}/api-tokens".into()
        )));
        assert!(paths.contains(&(
            HttpMethod::Delete,
            "/organizations/{organization_id}/api-tokens/{token_id}".into()
        )));
        for route in routes {
            assert_eq!(
                route
                    .metadata()
                    .get(AUTH_SCOPES_METADATA)
                    .cloned()
                    .expect("auth.scopes"),
                serde_json::json!([ApiTokenScope::TOKEN_WRITE])
            );
        }
    }
}
