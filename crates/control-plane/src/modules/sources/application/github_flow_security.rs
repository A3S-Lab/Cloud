use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::sources::domain::GithubAppAuthorizationError;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const FLOW_SECRET_BYTES: usize = 32;
const FLOW_SECRET_LENGTH: usize = 43;

pub fn generate_flow_secret() -> Result<Zeroizing<String>, ApplicationError> {
    let mut random = Zeroizing::new([0_u8; FLOW_SECRET_BYTES]);
    getrandom::fill(&mut *random).map_err(|error| {
        ApplicationError::Internal(format!(
            "could not generate GitHub connection state: {error}"
        ))
    })?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(&random[..])))
}

pub fn validate_flow_secret(
    value: Zeroizing<String>,
    label: &str,
) -> Result<Zeroizing<String>, ApplicationError> {
    if value.len() != FLOW_SECRET_LENGTH
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ApplicationError::Invalid(format!("{label} is invalid")));
    }
    Ok(value)
}

pub fn validate_oauth_code(
    value: Zeroizing<String>,
) -> Result<Zeroizing<String>, ApplicationError> {
    if value.is_empty()
        || value.len() > 1024
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ApplicationError::Invalid(
            "GitHub OAuth code is invalid".into(),
        ));
    }
    Ok(value)
}

pub fn digest(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn map_authorization_error(error: GithubAppAuthorizationError) -> ApplicationError {
    match error {
        GithubAppAuthorizationError::NotConfigured | GithubAppAuthorizationError::Unavailable => {
            ApplicationError::Unavailable(error.to_string())
        }
        GithubAppAuthorizationError::Rejected => ApplicationError::Invalid(error.to_string()),
        GithubAppAuthorizationError::Forbidden => ApplicationError::Forbidden(error.to_string()),
        GithubAppAuthorizationError::Protocol(message) => ApplicationError::Internal(message),
    }
}

pub fn map_state_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => {
            ApplicationError::Invalid("GitHub connection state is invalid or expired".into())
        }
        error => error.into(),
    }
}
