use super::{PrepareGithubConnectionOauth, PrepareGithubConnectionOauthResult};
use crate::modules::shared_kernel::application::{
    generate_oauth_flow_secret, oauth_flow_digest, pkce_s256_challenge, validate_oauth_flow_secret,
    ApplicationError, ApplicationResult,
};
use crate::modules::sources::application::github_flow_security::{
    map_authorization_error, map_state_repository_error,
};
use crate::modules::sources::domain::{
    GithubInstallationId, IGithubAppAuthorizationService, IGithubConnectionRepository,
};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct PrepareGithubConnectionOauthHandler {
    connections: Arc<dyn IGithubConnectionRepository>,
    authorization: Arc<dyn IGithubAppAuthorizationService>,
}

impl PrepareGithubConnectionOauthHandler {
    pub fn new(
        connections: Arc<dyn IGithubConnectionRepository>,
        authorization: Arc<dyn IGithubAppAuthorizationService>,
    ) -> Self {
        Self {
            connections,
            authorization,
        }
    }
}

impl CommandHandler<PrepareGithubConnectionOauth> for PrepareGithubConnectionOauthHandler {
    fn execute(
        &self,
        command: PrepareGithubConnectionOauth,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<PrepareGithubConnectionOauthResult>>,
    > {
        let connections = Arc::clone(&self.connections);
        let authorization = Arc::clone(&self.authorization);
        Box::pin(async move {
            let installation_id = match GithubInstallationId::parse(command.installation_id) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let installation_state = match validate_oauth_flow_secret(
                command.installation_state,
                "GitHub installation state",
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let oauth_state = match generate_oauth_flow_secret("GitHub OAuth state") {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let pkce_verifier = match generate_oauth_flow_secret("GitHub PKCE verifier") {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let authorization_url = match authorization
                .authorization_url(&oauth_state, &pkce_s256_challenge(&pkce_verifier))
            {
                Ok(url) => url,
                Err(error) => return Ok(Err(map_authorization_error(error))),
            };
            let flow = match connections
                .prepare_oauth(
                    oauth_flow_digest(&installation_state).as_str(),
                    installation_id,
                    oauth_flow_digest(&oauth_state).to_string(),
                    oauth_flow_digest(&pkce_verifier).to_string(),
                    command.requested_at,
                )
                .await
            {
                Ok(flow) => flow,
                Err(error) => return Ok(Err(map_state_repository_error(error))),
            };
            Ok(Ok(PrepareGithubConnectionOauthResult {
                authorization_url,
                pkce_verifier,
                expires_at: flow.expires_at,
            }))
        })
    }
}
