use super::{ListHumanTasks, HUMAN_TASK_LIST_MAX_LIMIT};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::application::{human_task_access, resource_access};
use crate::modules::workflow::domain::{HumanTaskRecord, IHumanTaskRepository};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListHumanTasksHandler {
    repository: Arc<dyn IHumanTaskRepository>,
}

impl ListHumanTasksHandler {
    pub fn new(repository: Arc<dyn IHumanTaskRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListHumanTasks> for ListHumanTasksHandler {
    fn execute(
        &self,
        query: ListHumanTasks,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<HumanTaskRecord>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if query.limit == 0 || query.limit > HUMAN_TASK_LIST_MAX_LIMIT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "HumanTask limit must be between 1 and {HUMAN_TASK_LIST_MAX_LIMIT}"
                ))));
            }
            if let Err(error) =
                resource_access::human_task_project(query.project_id, &query.resource_access)
            {
                return Ok(Err(error));
            }
            let records = match repository
                .list_tasks(
                    query.organization_id,
                    query.project_id,
                    query.status,
                    query.limit,
                )
                .await
            {
                Ok(records) => records,
                Err(error) => return Ok(Err(ApplicationError::from(error))),
            };
            Ok(records
                .into_iter()
                .map(|record| human_task_access::public_record(record, None))
                .collect())
        })
    }
}
