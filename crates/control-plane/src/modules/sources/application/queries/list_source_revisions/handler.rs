use super::ListSourceRevisions;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::sources::application::ISourceEnvironmentAccess;
use crate::modules::sources::domain::{ExternalSourceRevision, ISourceRevisionRepository};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListSourceRevisionsHandler {
    environment_access: Arc<dyn ISourceEnvironmentAccess>,
    sources: Arc<dyn ISourceRevisionRepository>,
}

impl ListSourceRevisionsHandler {
    pub(in crate::modules::sources) fn from_environment_access(
        environment_access: Arc<dyn ISourceEnvironmentAccess>,
        sources: Arc<dyn ISourceRevisionRepository>,
    ) -> Self {
        Self {
            environment_access,
            sources,
        }
    }
}

impl QueryHandler<ListSourceRevisions> for ListSourceRevisionsHandler {
    fn execute(
        &self,
        query: ListSourceRevisions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<ExternalSourceRevision>>>,
    > {
        let environment_access = Arc::clone(&self.environment_access);
        let sources = Arc::clone(&self.sources);
        Box::pin(async move {
            if let Err(error) = environment_access
                .require_environment(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
            {
                return Ok(Err(error));
            }
            Ok(sources
                .list(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
                .map_err(Into::into))
        })
    }
}
