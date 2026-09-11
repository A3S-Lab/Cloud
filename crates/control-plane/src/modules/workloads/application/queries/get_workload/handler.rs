use super::GetWorkload;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::application::queries::{
    reader::WorkloadQueryReader, WorkloadQueryResult,
};
use crate::modules::workloads::application::{
    IWorkloadDeploymentOperationAccess, IWorkloadRuntimeObservationAccess, WorkloadResourceResolver,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkloadHandler {
    reader: WorkloadQueryReader,
    resources: WorkloadResourceResolver,
}

impl GetWorkloadHandler {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        operations: Arc<dyn IWorkloadDeploymentOperationAccess>,
        observations: Arc<dyn IWorkloadRuntimeObservationAccess>,
    ) -> Self {
        Self {
            reader: WorkloadQueryReader::new(Arc::clone(&workloads), operations, observations),
            resources: WorkloadResourceResolver::new(workloads),
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
        let resources = self.resources.clone();
        Box::pin(async move {
            let workload = match resources
                .workload(query.organization_id, query.workload_id, &query.access)
                .await
            {
                Ok(workload) => workload,
                Err(error) => return Ok(Err(error)),
            };
            match reader.view(query.organization_id, workload).await {
                Ok(workload) => Ok(Ok(workload)),
                Err(RepositoryError::NotFound) => {
                    Ok(Err(ApplicationError::NotFound("workload not found".into())))
                }
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
