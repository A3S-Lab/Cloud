use super::ListWorkloads;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workloads::application::queries::{
    reader::WorkloadQueryReader, WorkloadQueryResult,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListWorkloadsHandler {
    workloads: Arc<dyn IWorkloadRepository>,
    reader: WorkloadQueryReader,
}

impl ListWorkloadsHandler {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        operations: Arc<dyn IOperationRepository>,
        node_control: Arc<dyn INodeControlRepository>,
    ) -> Self {
        Self {
            reader: WorkloadQueryReader::new(Arc::clone(&workloads), operations, node_control),
            workloads,
        }
    }
}

impl QueryHandler<ListWorkloads> for ListWorkloadsHandler {
    fn execute(
        &self,
        query: ListWorkloads,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<WorkloadQueryResult>>>>
    {
        let workloads = Arc::clone(&self.workloads);
        let reader = self.reader.clone();
        Box::pin(async move {
            let workloads = match workloads
                .list_workloads(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
            {
                Ok(workloads) => workloads,
                Err(error) => return Ok(Err(error.into())),
            };
            let mut results = Vec::with_capacity(workloads.len());
            for workload in workloads {
                match reader.view(query.organization_id, workload).await {
                    Ok(result) => results.push(result),
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            Ok(Ok(results))
        })
    }
}
