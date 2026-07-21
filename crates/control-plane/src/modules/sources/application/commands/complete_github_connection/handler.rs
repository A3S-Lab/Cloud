use super::CompleteGithubConnection;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::SourceConnectionId;
use crate::modules::sources::application::github_flow_security::{
    digest, map_authorization_error, map_state_repository_error, validate_flow_secret,
    validate_oauth_code,
};
use crate::modules::sources::domain::{
    CompleteGithubConnection as PersistGithubConnection, GithubConnection, GithubConnectionCreated,
    GithubInstallationVerificationRequest, IGithubAppAuthorizationService,
    IGithubConnectionRepository, NewGithubConnection,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CompleteGithubConnectionHandler {
    connections: Arc<dyn IGithubConnectionRepository>,
    authorization: Arc<dyn IGithubAppAuthorizationService>,
}

impl CompleteGithubConnectionHandler {
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

impl CommandHandler<CompleteGithubConnection> for CompleteGithubConnectionHandler {
    fn execute(
        &self,
        command: CompleteGithubConnection,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<GithubConnection>>> {
        let connections = Arc::clone(&self.connections);
        let authorization = Arc::clone(&self.authorization);
        Box::pin(async move {
            let oauth_state = match validate_flow_secret(command.oauth_state, "GitHub OAuth state")
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let pkce_verifier =
                match validate_flow_secret(command.pkce_verifier, "GitHub PKCE verifier") {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error)),
                };
            let code = match validate_oauth_code(command.code) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let flow = match connections
                .find_oauth_flow(
                    &digest(&oauth_state),
                    &digest(&pkce_verifier),
                    command.completed_at,
                )
                .await
            {
                Ok(flow) => flow,
                Err(error) => return Ok(Err(map_state_repository_error(error))),
            };
            let installation_id = flow.installation_id.ok_or_else(|| {
                BootError::Internal("OAuth-ready GitHub connection flow has no installation".into())
            })?;
            let verified = match authorization
                .verify_installation(GithubInstallationVerificationRequest {
                    code,
                    pkce_verifier,
                    installation_id,
                })
                .await
            {
                Ok(verified) => verified,
                Err(error) => return Ok(Err(map_authorization_error(error))),
            };
            if verified.installation_id != installation_id {
                return Ok(Err(ApplicationError::Internal(
                    "GitHub authorization verified a different installation".into(),
                )));
            }
            let connection = GithubConnection::connect(NewGithubConnection {
                id: SourceConnectionId::new(),
                organization_id: flow.organization_id,
                installation_id,
                account_id: verified.account_id,
                account_login: verified.account_login,
                account_kind: verified.account_kind,
                verified_by_user_id: verified.user_id,
                verified_by_user_login: verified.user_login,
                connected_at: command.completed_at,
            })
            .map_err(BootError::Internal)?;
            let event = GithubConnectionCreated::envelope(&connection, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match connections
                .complete(PersistGithubConnection {
                    flow_id: flow.id,
                    connection,
                    event,
                    completed_at: command.completed_at,
                })
                .await
            {
                Ok(connection) => Ok(Ok(connection)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
