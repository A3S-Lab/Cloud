use super::{BootstrapIdentity, BootstrapIdentityResult};
use crate::modules::identity::domain::entities::{
    ApiToken, IdentityBootstrap, IdentityPrincipal, IdentityPrincipalKind, Membership, Organization,
};
use crate::modules::identity::domain::events::{
    ApiTokenCreated, MembershipChanged, OrganizationCreated, PrincipalCreated,
};
use crate::modules::identity::domain::repositories::IApiTokenRepository;
use crate::modules::identity::domain::value_objects::{
    ApiTokenName, ApiTokenScope, ApiTokenSecret, MembershipRole, OrganizationName,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, MembershipId, OrganizationId, PrincipalId, ResourceName,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct BootstrapIdentityHandler {
    repository: Arc<dyn IApiTokenRepository>,
}

impl BootstrapIdentityHandler {
    pub fn new(repository: Arc<dyn IApiTokenRepository>) -> Self {
        Self { repository }
    }
}

impl CommandHandler<BootstrapIdentity> for BootstrapIdentityHandler {
    fn execute(
        &self,
        command: BootstrapIdentity,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<BootstrapIdentityResult>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let organization_name = match OrganizationName::parse(command.organization_name) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let token_name = match ApiTokenName::parse(command.token_name) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let token_secret = match ApiTokenSecret::parse(command.token_secret) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let digest = token_secret.digest();
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationName": organization_name.as_str(),
                "tokenName": token_name.as_str(),
                "tokenDigest": digest.as_str(),
                "expiresAt": command.expires_at,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                "identity.bootstrap",
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let now = Utc::now();
            let organization = Organization::create(OrganizationId::new(), organization_name, now);
            let principal = IdentityPrincipal::create(
                PrincipalId::new(),
                IdentityPrincipalKind::Service,
                ResourceName::parse(token_name.as_str()).map_err(BootError::Internal)?,
                now,
            );
            let membership = Membership::create(
                MembershipId::new(),
                organization.id,
                principal.id,
                MembershipRole::Owner,
                now,
            );
            let token = match ApiToken::issue(
                ApiTokenId::new(),
                organization.id,
                principal.id,
                token_name,
                ApiTokenScope::bootstrap_scopes(),
                now,
                command.expires_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let organization_event =
                OrganizationCreated::envelope(&organization, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let principal_event =
                PrincipalCreated::envelope(organization.id, &principal, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            let membership_event = MembershipChanged::created(&membership, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let token_event = ApiTokenCreated::envelope(&token, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match repository
                .bootstrap(
                    IdentityBootstrap {
                        organization,
                        principal,
                        membership,
                        api_token: token,
                    },
                    digest,
                    [
                        organization_event,
                        principal_event,
                        membership_event,
                        token_event,
                    ],
                    idempotency,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(BootstrapIdentityResult {
                identity: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
