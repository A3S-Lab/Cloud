use super::ListProjects;
use crate::modules::projects::domain::entities::Project;
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListProjectsHandler {
    repository: Arc<dyn IProjectRepository>,
}

impl ListProjectsHandler {
    pub fn new(repository: Arc<dyn IProjectRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListProjects> for ListProjectsHandler {
    fn execute(
        &self,
        query: ListProjects,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<Project>>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let projects = match repository.list(query.organization_id).await {
                Ok(projects) => projects,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(projects
                .into_iter()
                .filter(|project| {
                    query
                        .resource_access
                        .project_is_visible_in_collection(project.id)
                })
                .collect()))
        })
    }
}
