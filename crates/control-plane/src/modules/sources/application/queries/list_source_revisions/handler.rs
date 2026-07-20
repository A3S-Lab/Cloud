use super::ListSourceRevisions;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::sources::domain::{ExternalSourceRevision, ISourceRevisionRepository};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListSourceRevisionsHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    sources: Arc<dyn ISourceRevisionRepository>,
}

impl ListSourceRevisionsHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        sources: Arc<dyn ISourceRevisionRepository>,
    ) -> Self {
        Self {
            environments,
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
        let environments = Arc::clone(&self.environments);
        let sources = Arc::clone(&self.sources);
        Box::pin(async move {
            match environments
                .find(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
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
