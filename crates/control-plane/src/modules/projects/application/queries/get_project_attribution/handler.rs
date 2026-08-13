use super::GetProjectAttribution;
use crate::modules::projects::application::resource_access::ProjectResourceAccess;
use crate::modules::projects::domain::entities::ProjectAttributionProfile;
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetProjectAttributionHandler {
    projects: Arc<dyn IProjectRepository>,
}

impl GetProjectAttributionHandler {
    pub fn new(projects: Arc<dyn IProjectRepository>) -> Self {
        Self { projects }
    }
}

impl QueryHandler<GetProjectAttribution> for GetProjectAttributionHandler {
    fn execute(
        &self,
        query: GetProjectAttribution,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ProjectAttributionProfile>>>
    {
        let projects = Arc::clone(&self.projects);
        Box::pin(async move {
            let project = match ProjectResourceAccess::new(Arc::clone(&projects))
                .project(
                    query.organization_id,
                    query.project_id,
                    &query.resource_access,
                )
                .await
            {
                Ok(project) => project,
                Err(error) => return Ok(Err(error)),
            };
            let profile_id = match query
                .attribution_profile_id
                .or(project.current_attribution_profile_id)
            {
                Some(profile_id) => profile_id,
                None => return Ok(Err(attribution_not_found())),
            };
            match projects
                .find_attribution_profile(query.organization_id, query.project_id, profile_id)
                .await
            {
                Ok(Some(profile)) => Ok(Ok(profile)),
                Ok(None) => Ok(Err(attribution_not_found())),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

fn attribution_not_found() -> ApplicationError {
    ApplicationError::NotFound("project attribution profile not found".into())
}
