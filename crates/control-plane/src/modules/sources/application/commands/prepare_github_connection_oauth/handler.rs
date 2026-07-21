use super::{PrepareGithubConnectionOauth, PrepareGithubConnectionOauthResult};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::sources::application::github_flow_security::{
    digest, generate_flow_secret, map_authorization_error, map_state_repository_error,
    pkce_challenge, validate_flow_secret,
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
            let installation_state =
                match validate_flow_secret(command.installation_state, "GitHub installation state")
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            let oauth_state = match generate_flow_secret() {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let pkce_verifier = match generate_flow_secret() {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let authorization_url = match authorization
                .authorization_url(&oauth_state, &pkce_challenge(&pkce_verifier))
            {
                Ok(url) => url,
                Err(error) => return Ok(Err(map_authorization_error(error))),
            };
            let flow = match connections
                .prepare_oauth(
                    &digest(&installation_state),
                    installation_id,
                    digest(&oauth_state),
                    digest(&pkce_verifier),
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
