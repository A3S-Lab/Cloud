use crate::modules::sources::presentation::dto::GithubConnectionResponse;
use crate::modules::sources::{CompleteGithubConnection, PrepareGithubConnectionOauth};
use crate::presentation::{
    application_error_response, bounded_oauth_query_pairs, oauth_callback_query, oauth_no_store,
    OAuthNoStoreErrorFilter,
};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, CookieOptions,
    CookieSameSite, Result, AUTH_PUBLIC_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;
use zeroize::Zeroizing;

const PKCE_COOKIE: &str = "a3s_github_oauth_pkce";
const CALLBACK_PATH: &str = "/api/v1/source-connections/github/callback";

struct GithubSetupQuery {
    installation_id: u64,
    state: Zeroizing<String>,
    setup_action: Option<String>,
}

pub fn github_connection_callbacks_controller(
    commands: Arc<CommandBus>,
) -> Result<ControllerDefinition> {
    let setup_commands = Arc::clone(&commands);
    ControllerDefinition::new("/source-connections")?
        .with_filter(OAuthNoStoreErrorFilter)
        .with_metadata(AUTH_PUBLIC_METADATA, true)?
        .get("/github/setup", move |request: BootRequest| {
            let commands = Arc::clone(&setup_commands);
            async move {
                let query = setup_query(&request)?;
                validate_setup_action(query.setup_action.as_deref())?;
                let request_id = request_id(&request)?;
                match commands
                    .execute(PrepareGithubConnectionOauth {
                        installation_id: query.installation_id,
                        installation_state: query.state,
                        requested_at: Utc::now(),
                    })
                    .await?
                {
                    Ok(result) => {
                        let max_age = (result.expires_at - Utc::now())
                            .to_std()
                            .unwrap_or(Duration::from_secs(1))
                            .max(Duration::from_secs(1));
                        Ok(oauth_no_store(
                            BootResponse::see_other(result.authorization_url).with_cookie(
                                PKCE_COOKIE,
                                result.pkce_verifier.as_str(),
                                cookie_options().with_max_age(max_age),
                            )?,
                        ))
                    }
                    Err(error) => Ok(oauth_no_store(application_error_response(
                        error, request_id,
                    )?)),
                }
            }
        })?
        .get("/github/callback", move |request: BootRequest| {
            let commands = Arc::clone(&commands);
            async move {
                let query = oauth_callback_query(&request, "GitHub OAuth")?;
                if query.has_error {
                    return Err(BootError::BadRequest(
                        "GitHub authorization was not completed".into(),
                    ));
                }
                let code = query
                    .code
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| BootError::BadRequest("GitHub OAuth code is required".into()))?;
                let state = query
                    .state
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        BootError::BadRequest("GitHub OAuth state is required".into())
                    })?;
                let verifier = Zeroizing::new(request.cookie(PKCE_COOKIE)?.ok_or_else(|| {
                    BootError::BadRequest("GitHub OAuth PKCE cookie is required".into())
                })?);
                let request_id = request_id(&request)?;
                match commands
                    .execute(CompleteGithubConnection {
                        oauth_state: state,
                        code,
                        pkce_verifier: verifier,
                        request_id,
                        completed_at: Utc::now(),
                    })
                    .await?
                {
                    Ok(connection) => Ok(oauth_no_store(
                        BootResponse::json_with_status(
                            201,
                            &GithubConnectionResponse::from(connection),
                        )?
                        .delete_cookie(PKCE_COOKIE, cookie_options())?,
                    )),
                    Err(error) => Ok(oauth_no_store(application_error_response(
                        error, request_id,
                    )?)),
                }
            }
        })
}

fn setup_query(request: &BootRequest) -> Result<GithubSetupQuery> {
    let mut installation_id = None;
    let mut state = None;
    let mut setup_action = None;
    for (name, value) in bounded_oauth_query_pairs(request, "GitHub connection")? {
        match name.as_str() {
            "installation_id" => set_once(&mut installation_id, value, "installation ID")?,
            "state" => set_once(&mut state, Zeroizing::new(value), "installation state")?,
            "setup_action" => set_once(&mut setup_action, value, "setup action")?,
            _ => {}
        }
    }
    let installation_id = installation_id
        .ok_or_else(|| BootError::BadRequest("GitHub installation ID is required".into()))?
        .parse()
        .map_err(|_| BootError::BadRequest("GitHub installation ID is invalid".into()))?;
    let state = state
        .ok_or_else(|| BootError::BadRequest("GitHub installation state is required".into()))?;
    Ok(GithubSetupQuery {
        installation_id,
        state,
        setup_action,
    })
}

fn set_once<T>(slot: &mut Option<T>, value: T, label: &str) -> Result<()> {
    if slot.replace(value).is_some() {
        return Err(BootError::BadRequest(format!(
            "GitHub {label} parameter is duplicated"
        )));
    }
    Ok(())
}

fn cookie_options() -> CookieOptions {
    CookieOptions::new()
        .with_path(CALLBACK_PATH)
        .with_http_only(true)
        .with_secure(true)
        .with_same_site(CookieSameSite::Lax)
}

fn validate_setup_action(action: Option<&str>) -> Result<()> {
    if action.is_some_and(|value| {
        value.len() > 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    }) {
        return Err(BootError::BadRequest(
            "GitHub setup action is invalid".into(),
        ));
    }
    Ok(())
}

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}
