use super::ListEnvironments;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListEnvironmentsHandler {
    repository: Arc<dyn IEnvironmentRepository>,
}

impl ListEnvironmentsHandler {
    pub fn new(repository: Arc<dyn IEnvironmentRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListEnvironments> for ListEnvironmentsHandler {
    fn execute(
        &self,
        query: ListEnvironments,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<Environment>>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            Ok(repository
                .list(query.organization_id, query.project_id)
                .await
                .map_err(Into::into))
        })
    }
}
