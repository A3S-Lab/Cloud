use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::sources::domain::GithubAppAuthorizationError;
use zeroize::Zeroizing;

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
