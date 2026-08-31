use super::{BeginGithubConnection, BeginGithubConnectionResult};
use crate::modules::shared_kernel::application::{
    generate_oauth_flow_secret, oauth_flow_digest, ApplicationError, ApplicationResult,
};
use crate::modules::sources::application::{
    github_flow_security::map_authorization_error, ISourceOrganizationAccess,
};
use crate::modules::sources::domain::{
    GithubConnectionFlow, IGithubAppAuthorizationService, IGithubConnectionRepository,
};
use a3s_boot::{CommandHandler, CqrsContext};
use chrono::Duration;
use std::sync::Arc;
use uuid::Uuid;

pub struct BeginGithubConnectionHandler {
    organization_access: Arc<dyn ISourceOrganizationAccess>,
    connections: Arc<dyn IGithubConnectionRepository>,
    authorization: Arc<dyn IGithubAppAuthorizationService>,
    state_ttl: Duration,
}

impl BeginGithubConnectionHandler {
    pub(in crate::modules::sources) fn from_organization_access(
        organization_access: Arc<dyn ISourceOrganizationAccess>,
        connections: Arc<dyn IGithubConnectionRepository>,
        authorization: Arc<dyn IGithubAppAuthorizationService>,
        state_ttl: Duration,
    ) -> Result<Self, String> {
        if state_ttl < Duration::minutes(1) || state_ttl > Duration::minutes(30) {
            return Err("GitHub connection state TTL must be between 1 and 30 minutes".into());
        }
        Ok(Self {
            organization_access,
            connections,
            authorization,
            state_ttl,
        })
    }
}

impl CommandHandler<BeginGithubConnection> for BeginGithubConnectionHandler {
    fn execute(
        &self,
        command: BeginGithubConnection,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<BeginGithubConnectionResult>>,
    > {
        let organization_access = Arc::clone(&self.organization_access);
        let connections = Arc::clone(&self.connections);
        let authorization = Arc::clone(&self.authorization);
        let state_ttl = self.state_ttl;
        Box::pin(async move {
            if let Err(error) = organization_access
                .require_organization(command.organization_id)
                .await
            {
                return Ok(Err(error));
            }
            let state = match generate_oauth_flow_secret("GitHub connection state") {
                Ok(state) => state,
                Err(error) => return Ok(Err(error)),
            };
            let installation_url = match authorization.installation_url(&state) {
                Ok(url) => url,
                Err(error) => return Ok(Err(map_authorization_error(error))),
            };
            let expires_at = command.requested_at + state_ttl;
            let flow = match GithubConnectionFlow::begin(
                Uuid::now_v7(),
                command.organization_id,
                oauth_flow_digest(&state).to_string(),
                command.requested_at,
                expires_at,
            ) {
                Ok(flow) => flow,
                Err(error) => return Ok(Err(ApplicationError::Internal(error))),
            };
            let flow = match connections.begin_flow(flow).await {
                Ok(flow) => flow,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(BeginGithubConnectionResult {
                installation_url,
                expires_at: flow.expires_at,
            }))
        })
    }
}
