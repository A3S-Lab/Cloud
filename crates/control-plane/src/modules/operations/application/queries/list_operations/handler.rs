use super::ListOperations;
use crate::modules::operations::domain::entities::OperationRecord;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListOperationsHandler {
    repository: Arc<dyn IOperationRepository>,
}

impl ListOperationsHandler {
    pub fn new(repository: Arc<dyn IOperationRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListOperations> for ListOperationsHandler {
    fn execute(
        &self,
        query: ListOperations,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<OperationRecord>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            Ok(repository
                .list(query.organization_id, query.limit.min(200))
                .await
                .map_err(Into::into))
        })
    }
}
