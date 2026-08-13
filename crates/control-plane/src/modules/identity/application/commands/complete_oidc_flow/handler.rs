use super::{CompleteOidcFlow, CompleteOidcFlowResult};
use crate::modules::identity::application::commands::map_oidc_provider_error;
use crate::modules::identity::domain::entities::OidcFlowPurpose;
use crate::modules::identity::domain::repositories::{
    CompleteOidcLinkWrite, CompleteOidcLoginWrite, IOidcIdentityRepository,
};
use crate::modules::identity::domain::services::{
    IOidcProviderService, OidcCodeVerificationRequest,
};
use crate::modules::identity::domain::value_objects::{ApiTokenName, ApiTokenSecret};
use crate::modules::shared_kernel::application::{
    oauth_flow_digest, validate_oauth_flow_secret, ApplicationError, ApplicationResult,
};
use crate::modules::shared_kernel::domain::ApiTokenId;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CompleteOidcFlowHandler {
    oidc_identity: Arc<dyn IOidcIdentityRepository>,
    oidc_provider: Arc<dyn IOidcProviderService>,
}

impl CompleteOidcFlowHandler {
    pub fn new(
        oidc_identity: Arc<dyn IOidcIdentityRepository>,
        oidc_provider: Arc<dyn IOidcProviderService>,
    ) -> Self {
        Self {
            oidc_identity,
            oidc_provider,
        }
    }
}

impl CommandHandler<CompleteOidcFlow> for CompleteOidcFlowHandler {
    fn execute(
        &self,
        command: CompleteOidcFlow,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CompleteOidcFlowResult>>>
    {
        let oidc_identity = Arc::clone(&self.oidc_identity);
        let oidc_provider = Arc::clone(&self.oidc_provider);
        Box::pin(async move {
            let state = match validate_oauth_flow_secret(command.state, "OIDC state") {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let nonce = match validate_oauth_flow_secret(command.nonce, "OIDC nonce") {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let pkce_verifier =
                match validate_oauth_flow_secret(command.pkce_verifier, "OIDC PKCE verifier") {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            let state_digest = oauth_flow_digest(&state);
            let nonce_digest = oauth_flow_digest(&nonce);
            let pkce_verifier_digest = oauth_flow_digest(&pkce_verifier);
            let flow = match oidc_identity
                .find_pending_oidc_flow(&state_digest, Utc::now())
                .await
            {
                Ok(Some(flow)) if flow.provider_key == command.provider_key => flow,
                Ok(Some(_)) | Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "OIDC flow was not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let verified = match oidc_provider
                .verify_code(OidcCodeVerificationRequest {
                    provider_key: flow.provider_key.clone(),
                    code: command.code,
                    nonce,
                    pkce_verifier,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(map_oidc_provider_error(error))),
            };
            if verified.provider_key != flow.provider_key
                || verified.issuer != flow.issuer
                || verified.provider_config_digest != flow.provider_config_digest
            {
                return Ok(Err(ApplicationError::Conflict(
                    "OIDC provider configuration changed during the flow".into(),
                )));
            }
            let completed_at = Utc::now();
            match flow.purpose {
                OidcFlowPurpose::Link => {
                    let link = match oidc_identity
                        .complete_oidc_link(CompleteOidcLinkWrite {
                            flow_id: flow.id,
                            provider_config_digest: verified.provider_config_digest,
                            state_digest,
                            nonce_digest,
                            pkce_verifier_digest,
                            subject: verified.subject,
                            completed_at,
                            request_id: command.request_id,
                        })
                        .await
                    {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(error.into())),
                    };
                    Ok(Ok(CompleteOidcFlowResult::Linked(link)))
                }
                OidcFlowPurpose::Login => {
                    let token_id = ApiTokenId::new();
                    let token_name = ApiTokenName::parse(format!("OIDC login {token_id}"))
                        .map_err(BootError::Internal)?;
                    let (credential, token_digest) =
                        ApiTokenSecret::generate().map_err(BootError::Internal)?;
                    let api_token = match oidc_identity
                        .complete_oidc_login(CompleteOidcLoginWrite {
                            flow_id: flow.id,
                            provider_config_digest: verified.provider_config_digest,
                            state_digest,
                            nonce_digest,
                            pkce_verifier_digest,
                            subject: verified.subject,
                            token_id,
                            token_name,
                            token_digest,
                            completed_at,
                            token_expires_at: completed_at + verified.login_token_lifetime,
                            request_id: command.request_id,
                        })
                        .await
                    {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(error.into())),
                    };
                    Ok(Ok(CompleteOidcFlowResult::LoggedIn {
                        api_token,
                        credential,
                    }))
                }
            }
        })
    }
}
