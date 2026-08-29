pub mod accept_membership_invitation;
pub mod authorize_privileged_access;
pub mod begin_oidc_flow;
pub mod begin_recipient_contact_verification;
pub mod bootstrap_identity;
pub mod change_membership_role;
pub mod complete_oidc_flow;
pub mod complete_recipient_contact_verification;
pub mod create_api_token;
pub mod create_membership;
pub mod create_membership_invitation;
pub mod create_organization;
pub mod create_resource_grant;
pub mod manage_platform_rbac;
pub mod manage_tenant_support;
pub mod revoke_api_token;
pub mod revoke_membership;
pub mod revoke_membership_invitation;
pub mod revoke_recipient_contact;
pub mod revoke_resource_grant;

use crate::modules::identity::domain::services::OidcProviderError;
use crate::modules::identity::domain::services::RecipientContactProofError;
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

fn map_recipient_contact_proof_error(error: RecipientContactProofError) -> ApplicationError {
    match error {
        RecipientContactProofError::Rejected => {
            ApplicationError::Forbidden("recipient contact verification proof was rejected".into())
        }
        RecipientContactProofError::Unavailable => ApplicationError::Unavailable(
            "recipient contact verification proof service is unavailable".into(),
        ),
    }
}

#[cfg(test)]
mod oidc_flow_tests;
#[cfg(test)]
mod recipient_contact_tests;
