pub mod accept_membership_invitation;
pub mod begin_oidc_flow;
pub mod bootstrap_identity;
pub mod change_membership_role;
pub mod complete_oidc_flow;
pub mod create_api_token;
pub mod create_membership;
pub mod create_membership_invitation;
pub mod create_organization;
pub mod create_resource_grant;
pub mod revoke_api_token;
pub mod revoke_membership;
pub mod revoke_membership_invitation;
pub mod revoke_resource_grant;

use crate::modules::identity::domain::services::OidcProviderError;
use crate::modules::shared_kernel::application::ApplicationError;

fn map_oidc_provider_error(error: OidcProviderError) -> ApplicationError {
    match error {
        OidcProviderError::NotConfigured => {
            ApplicationError::NotFound("OIDC provider was not found".into())
        }
        OidcProviderError::Rejected => {
            ApplicationError::Forbidden("OIDC authorization was rejected".into())
        }
        OidcProviderError::CredentialUnavailable
        | OidcProviderError::Unavailable
        | OidcProviderError::Protocol(_) => {
            ApplicationError::Unavailable("OIDC provider is unavailable".into())
        }
    }
}

#[cfg(test)]
mod oidc_flow_tests;
