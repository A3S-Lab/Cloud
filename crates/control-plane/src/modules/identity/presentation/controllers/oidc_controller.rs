use crate::modules::identity::domain::entities::{ExternalIdentityLink, OidcFlowPurpose};
use crate::modules::identity::domain::value_objects::{ApiTokenScope, OidcProviderKey};
use crate::modules::identity::presentation::dto::ApiTokenReadResponse;
use crate::modules::identity::presentation::request_context::{actor, request_id};
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::identity::{
    BeginOidcFlow, BeginOidcFlowResult, CompleteOidcFlow, CompleteOidcFlowResult,
};
use crate::modules::shared_kernel::application::oauth_flow_digest;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::presentation::{
    application_error_response, boot_error_response, bounded_oauth_query_pairs,
    oauth_callback_query, oauth_no_store, OAuthNoStoreErrorFilter,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, CookieOptions,
    CookieSameSite, Result, AUTH_PUBLIC_METADATA, AUTH_SCOPES_METADATA,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;
use zeroize::Zeroizing;

const OIDC_CALLBACK_BASE_PATH: &str = "/api/v1/identity/oidc";
const OIDC_COOKIE_PREFIX: &str = "a3s_oidc";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OidcLinkResponse {
    kind: &'static str,
    link_id: Uuid,
    provider_key: String,
    principal_id: Uuid,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    last_verified_at: DateTime<Utc>,
}

impl From<ExternalIdentityLink> for OidcLinkResponse {
    fn from(link: ExternalIdentityLink) -> Self {
        Self {
            kind: "linked",
            link_id: link.id.as_uuid(),
            provider_key: link.provider_key.as_str().to_owned(),
            principal_id: link.principal_id.as_uuid(),
            aggregate_version: link.aggregate_version,
            created_at: link.created_at,
            last_verified_at: link.last_verified_at,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OidcLoginResponse<'a> {
    kind: &'static str,
    token: ApiTokenReadResponse,
    credential: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OidcAuthorizationResponse<'a> {
    authorization_url: &'a str,
}

#[derive(Clone, Copy)]
enum BeginFlowTransport {
    Redirect,
    Json,
}

pub fn oidc_public_controller(commands: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let login_commands = Arc::clone(&commands);
    ControllerDefinition::new("/identity/oidc")?
        .with_filter(OAuthNoStoreErrorFilter)
        .with_metadata(AUTH_PUBLIC_METADATA, true)?
        .get("/{provider_key}/login", move |request: BootRequest| {
            let commands = Arc::clone(&login_commands);
            async move {
                let organization_id = login_organization_id(&request)?;
                let provider_key = provider_key(&request)?;
                let request_id = request_id(&request)?;
                match commands
                    .execute(BeginOidcFlow {
                        organization_id,
                        provider_key: provider_key.clone(),
                        purpose: OidcFlowPurpose::Login,
                        principal_id: None,
                    })
                    .await?
                {
                    Ok(result) => {
                        begin_flow_response(result, &provider_key, BeginFlowTransport::Redirect)
                    }
                    Err(error) => Ok(oauth_no_store(application_error_response(
                        error, request_id,
                    )?)),
                }
            }
        })?
        .get("/{provider_key}/callback", move |request: BootRequest| {
            let commands = Arc::clone(&commands);
            async move { complete_flow_response(&request, commands).await }
        })
}

pub fn oidc_link_controller(commands: Arc<CommandBus>) -> Result<ControllerDefinition> {
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_filter(OAuthNoStoreErrorFilter)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::CLOUD_READ])?
        .post(
            "/{organization_id}/identity/oidc/{provider_key}/link",
            move |request: BootRequest| {
                let commands = Arc::clone(&commands);
                async move {
                    let organization_id =
                        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?);
                    let provider_key = provider_key(&request)?;
                    let principal_id = actor(&request)?.principal_id;
                    let request_id = request_id(&request)?;
                    match commands
                        .execute(BeginOidcFlow {
                            organization_id,
                            provider_key: provider_key.clone(),
                            purpose: OidcFlowPurpose::Link,
                            principal_id: Some(principal_id),
                        })
                        .await?
                    {
                        Ok(result) => {
                            begin_flow_response(result, &provider_key, BeginFlowTransport::Json)
                        }
                        Err(error) => Ok(oauth_no_store(application_error_response(
                            error, request_id,
                        )?)),
                    }
                }
            },
        )
}

async fn complete_flow_response(
    request: &BootRequest,
    commands: Arc<CommandBus>,
) -> Result<BootResponse> {
    let request_id = request_id(request)?;
    let provider_key = provider_key(request)?;
    let query = oauth_callback_query(request, "OIDC")?;
    let Some(state) = query.state.filter(|value| !value.is_empty()) else {
        return Err(BootError::BadRequest("OIDC state is required".into()));
    };
    let cookie_names = flow_cookie_names(&state);
    if query.has_error {
        return callback_error_response(
            BootError::BadRequest("OIDC authorization was not completed".into()),
            request_id,
            &provider_key,
            &cookie_names,
        );
    }
    let Some(code) = query.code.filter(|value| !value.is_empty()) else {
        return callback_error_response(
            BootError::BadRequest("OIDC code is required".into()),
            request_id,
            &provider_key,
            &cookie_names,
        );
    };
    let nonce = match required_cookie(request, &cookie_names.nonce, "OIDC nonce") {
        Ok(value) => value,
        Err(error) => {
            return callback_error_response(error, request_id, &provider_key, &cookie_names)
        }
    };
    let pkce_verifier = match required_cookie(request, &cookie_names.pkce, "OIDC PKCE") {
        Ok(value) => value,
        Err(error) => {
            return callback_error_response(error, request_id, &provider_key, &cookie_names)
        }
    };
    let result = commands
        .execute(CompleteOidcFlow {
            provider_key: provider_key.clone(),
            code,
            state,
            nonce,
            pkce_verifier,
            request_id,
        })
        .await?;
    let response = match result {
        Ok(CompleteOidcFlowResult::Linked(link)) => {
            BootResponse::json(&OidcLinkResponse::from(link))?
        }
        Ok(CompleteOidcFlowResult::LoggedIn {
            api_token,
            credential,
        }) => BootResponse::json(&OidcLoginResponse {
            kind: "login",
            token: ApiTokenReadResponse::from(api_token),
            credential: credential.as_str(),
        })?,
        Err(error) => application_error_response(error, request_id)?,
    };
    clear_flow_cookies(oauth_no_store(response), &provider_key, &cookie_names)
}

fn begin_flow_response(
    result: BeginOidcFlowResult,
    provider_key: &OidcProviderKey,
    transport: BeginFlowTransport,
) -> Result<BootResponse> {
    let cookie_names = flow_cookie_names(&result.state);
    let max_age = (result.expires_at - Utc::now())
        .to_std()
        .unwrap_or(Duration::from_secs(1))
        .max(Duration::from_secs(1));
    let options = flow_cookie_options(provider_key).with_max_age(max_age);
    let response = match transport {
        BeginFlowTransport::Redirect => BootResponse::see_other(&result.authorization_url),
        BeginFlowTransport::Json => BootResponse::json(&OidcAuthorizationResponse {
            authorization_url: &result.authorization_url,
        })?,
    };
    Ok(oauth_no_store(
        response
            .with_cookie(&cookie_names.nonce, result.nonce.as_str(), options.clone())?
            .with_cookie(&cookie_names.pkce, result.pkce_verifier.as_str(), options)?,
    ))
}

fn callback_error_response(
    error: BootError,
    request_id: Uuid,
    provider_key: &OidcProviderKey,
    cookie_names: &FlowCookieNames,
) -> Result<BootResponse> {
    clear_flow_cookies(
        oauth_no_store(boot_error_response(error, request_id)?),
        provider_key,
        cookie_names,
    )
}

fn clear_flow_cookies(
    response: BootResponse,
    provider_key: &OidcProviderKey,
    cookie_names: &FlowCookieNames,
) -> Result<BootResponse> {
    let options = flow_cookie_options(provider_key);
    response
        .delete_cookie(&cookie_names.nonce, options.clone())?
        .delete_cookie(&cookie_names.pkce, options)
}

fn login_organization_id(request: &BootRequest) -> Result<OrganizationId> {
    let mut organization_id = None;
    for (name, value) in bounded_oauth_query_pairs(request, "OIDC login")? {
        if name == "organization_id" && organization_id.replace(value).is_some() {
            return Err(BootError::BadRequest(
                "OIDC organization_id parameter is duplicated".into(),
            ));
        }
    }
    let organization_id = organization_id
        .ok_or_else(|| BootError::BadRequest("OIDC organization_id is required".into()))?
        .parse::<Uuid>()
        .map_err(|_| BootError::BadRequest("OIDC organization_id is invalid".into()))?;
    Ok(OrganizationId::from_uuid(organization_id))
}

fn provider_key(request: &BootRequest) -> Result<OidcProviderKey> {
    OidcProviderKey::parse(request.param_as::<String>("provider_key")?)
        .map_err(BootError::BadRequest)
}

fn required_cookie(request: &BootRequest, name: &str, label: &str) -> Result<Zeroizing<String>> {
    request
        .cookie(name)?
        .filter(|value| !value.is_empty())
        .map(Zeroizing::new)
        .ok_or_else(|| BootError::BadRequest(format!("{label} cookie is required")))
}

struct FlowCookieNames {
    nonce: String,
    pkce: String,
}

fn flow_cookie_names(state: &str) -> FlowCookieNames {
    let digest = oauth_flow_digest(state);
    let hexadecimal = digest
        .as_str()
        .strip_prefix("sha256:")
        .expect("OAuth flow digest uses canonical syntax");
    FlowCookieNames {
        nonce: format!("{OIDC_COOKIE_PREFIX}_nonce_{hexadecimal}"),
        pkce: format!("{OIDC_COOKIE_PREFIX}_pkce_{hexadecimal}"),
    }
}

fn flow_cookie_options(provider_key: &OidcProviderKey) -> CookieOptions {
    CookieOptions::new()
        .with_path(format!(
            "{OIDC_CALLBACK_BASE_PATH}/{}/callback",
            provider_key.as_str()
        ))
        .with_http_only(true)
        .with_secure(true)
        .with_same_site(CookieSameSite::Lax)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_scopes_distinct_flow_cookies_without_exposing_state() {
        let left = flow_cookie_names("left-state");
        let right = flow_cookie_names("right-state");

        assert_ne!(left.nonce, right.nonce);
        assert_ne!(left.pkce, right.pkce);
        assert!(!left.nonce.contains("left-state"));
        assert!(!left.pkce.contains("left-state"));
    }

    #[test]
    fn login_query_rejects_duplicate_organization_context() {
        let request = BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/identity/oidc/workforce/login?organization_id=one&organization_id=two",
        );

        assert!(login_organization_id(&request).is_err());
    }
}
