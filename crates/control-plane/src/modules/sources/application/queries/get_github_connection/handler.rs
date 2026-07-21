use super::GetGithubConnection;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::sources::domain::{GithubConnection, IGithubConnectionRepository};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetGithubConnectionHandler {
    connections: Arc<dyn IGithubConnectionRepository>,
}

impl GetGithubConnectionHandler {
    pub fn new(connections: Arc<dyn IGithubConnectionRepository>) -> Self {
        Self { connections }
    }
}

impl QueryHandler<GetGithubConnection> for GetGithubConnectionHandler {
    fn execute(
        &self,
        query: GetGithubConnection,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<GithubConnection>>> {
        let connections = Arc::clone(&self.connections);
        Box::pin(async move {
            match connections.find(query.organization_id).await {
                Ok(Some(connection)) => Ok(Ok(connection)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "GitHub source connection not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
