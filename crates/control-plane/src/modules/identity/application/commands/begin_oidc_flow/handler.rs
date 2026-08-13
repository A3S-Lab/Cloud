use super::{BeginOidcFlow, BeginOidcFlowResult};
use crate::modules::identity::application::commands::map_oidc_provider_error;
use crate::modules::identity::domain::entities::{OidcFlow, OidcFlowPurpose};
use crate::modules::identity::domain::repositories::{
    IMembershipRepository, IOidcIdentityRepository, IOrganizationRepository,
};
use crate::modules::identity::domain::services::{IOidcProviderService, OidcAuthorizationRequest};
use crate::modules::shared_kernel::application::{
    generate_oauth_flow_secret, oauth_flow_digest, ApplicationError, ApplicationResult,
};
use crate::modules::shared_kernel::domain::OidcFlowId;
use a3s_boot::{CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct BeginOidcFlowHandler {
    organizations: Arc<dyn IOrganizationRepository>,
    memberships: Arc<dyn IMembershipRepository>,
    oidc_identity: Arc<dyn IOidcIdentityRepository>,
    oidc_provider: Arc<dyn IOidcProviderService>,
}

impl BeginOidcFlowHandler {
    pub fn new(
        organizations: Arc<dyn IOrganizationRepository>,
        memberships: Arc<dyn IMembershipRepository>,
        oidc_identity: Arc<dyn IOidcIdentityRepository>,
        oidc_provider: Arc<dyn IOidcProviderService>,
    ) -> Self {
        Self {
            organizations,
            memberships,
            oidc_identity,
            oidc_provider,
        }
    }
}

impl CommandHandler<BeginOidcFlow> for BeginOidcFlowHandler {
    fn execute(
        &self,
        command: BeginOidcFlow,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<BeginOidcFlowResult>>>
    {
        let organizations = Arc::clone(&self.organizations);
        let memberships = Arc::clone(&self.memberships);
        let oidc_identity = Arc::clone(&self.oidc_identity);
        let oidc_provider = Arc::clone(&self.oidc_provider);
        Box::pin(async move {
            if matches!(command.purpose, OidcFlowPurpose::Link) != command.principal_id.is_some() {
                return Ok(Err(ApplicationError::Invalid(
                    "OIDC link flow must bind exactly one authenticated principal".into(),
                )));
            }
            match organizations.find(command.organization_id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound("resource not found".into())))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            if let Some(principal_id) = command.principal_id {
                match memberships
                    .find_active_membership_by_principal(command.organization_id, principal_id)
                    .await
                {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        return Ok(Err(ApplicationError::Forbidden(
                            "OIDC link flow requires an active organization member".into(),
                        )))
                    }
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            let state = match generate_oauth_flow_secret("OIDC state") {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let nonce = match generate_oauth_flow_secret("OIDC nonce") {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let pkce_verifier = match generate_oauth_flow_secret("OIDC PKCE verifier") {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let authorization = match oidc_provider
                .authorization_url(OidcAuthorizationRequest {
                    provider_key: command.provider_key.clone(),
                    state: state.clone(),
                    nonce: nonce.clone(),
                    pkce_verifier: pkce_verifier.clone(),
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(map_oidc_provider_error(error))),
            };
            if authorization.provider_key != command.provider_key {
                return Ok(Err(ApplicationError::Internal(
                    "OIDC provider returned a mismatched identity".into(),
                )));
            }
            let created_at = Utc::now();
            let flow = match OidcFlow::begin(
                OidcFlowId::new(),
                command.organization_id,
                command.provider_key,
                authorization.issuer,
                authorization.provider_config_digest,
                command.purpose,
                command.principal_id,
                oauth_flow_digest(&state),
                oauth_flow_digest(&nonce),
                oauth_flow_digest(&pkce_verifier),
                created_at,
                created_at + authorization.flow_lifetime,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let flow = match oidc_identity.begin_oidc_flow(flow).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(BeginOidcFlowResult {
                authorization_url: authorization.authorization_url,
                state,
                nonce,
                pkce_verifier,
                expires_at: flow.expires_at,
            }))
        })
    }
}
