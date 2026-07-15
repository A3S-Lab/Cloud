use super::GetWorkload;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::application::queries::{
    reader::WorkloadQueryReader, WorkloadQueryResult,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkloadHandler {
    reader: WorkloadQueryReader,
}

impl GetWorkloadHandler {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        operations: Arc<dyn IOperationRepository>,
        node_control: Arc<dyn INodeControlRepository>,
    ) -> Self {
        Self {
            reader: WorkloadQueryReader::new(workloads, operations, node_control),
        }
    }
}

impl QueryHandler<GetWorkload> for GetWorkloadHandler {
    fn execute(
        &self,
        query: GetWorkload,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkloadQueryResult>>>
    {
        let reader = self.reader.clone();
        Box::pin(async move {
            match reader
                .workload(query.organization_id, query.workload_id)
                .await
            {
                Ok(workload) => Ok(Ok(workload)),
                Err(RepositoryError::NotFound) => {
                    Ok(Err(ApplicationError::NotFound("workload not found".into())))
                }
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
