use super::GetHumanTask;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::application::{human_task_access, resource_access};
use crate::modules::workflow::domain::{HumanTaskRecord, IHumanTaskRepository};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetHumanTaskHandler {
    repository: Arc<dyn IHumanTaskRepository>,
}

impl GetHumanTaskHandler {
    pub fn new(repository: Arc<dyn IHumanTaskRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetHumanTask> for GetHumanTaskHandler {
    fn execute(
        &self,
        query: GetHumanTask,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<HumanTaskRecord>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let record = resource_access::human_task(
                repository.as_ref(),
                query.organization_id,
                query.human_task_id,
                &query.resource_access,
            )
            .await;
            Ok(record.and_then(|record| {
                human_task_access::public_record(record, Some(query.actor_principal_id))
            }))
        })
    }
}
