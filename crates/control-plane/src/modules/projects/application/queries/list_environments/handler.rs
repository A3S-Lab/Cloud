use super::ListEnvironments;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::{IEnvironmentRepository, IProjectRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListEnvironmentsHandler {
    projects: Arc<dyn IProjectRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
}

impl ListEnvironmentsHandler {
    pub fn new(
        projects: Arc<dyn IProjectRepository>,
        environments: Arc<dyn IEnvironmentRepository>,
    ) -> Self {
        Self {
            projects,
            environments,
        }
    }
}

impl QueryHandler<ListEnvironments> for ListEnvironmentsHandler {
    fn execute(
        &self,
        query: ListEnvironments,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<Environment>>>> {
        let projects = Arc::clone(&self.projects);
        let environments = Arc::clone(&self.environments);
        Box::pin(async move {
            match projects.find(query.organization_id, query.project_id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "project not found in organization".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            Ok(environments
                .list(query.organization_id, query.project_id)
                .await
                .map_err(Into::into))
        })
    }
}
