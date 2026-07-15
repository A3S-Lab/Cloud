use super::GetDeployment;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::application::queries::{
    reader::WorkloadQueryReader, DeploymentQueryResult,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetDeploymentHandler {
    reader: WorkloadQueryReader,
}

impl GetDeploymentHandler {
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

impl QueryHandler<GetDeployment> for GetDeploymentHandler {
    fn execute(
        &self,
        query: GetDeployment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<DeploymentQueryResult>>>
    {
        let reader = self.reader.clone();
        Box::pin(async move {
            match reader
                .deployment(query.organization_id, query.deployment_id)
                .await
            {
                Ok(deployment) => Ok(Ok(deployment)),
                Err(RepositoryError::NotFound) => Ok(Err(ApplicationError::NotFound(
                    "deployment not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
