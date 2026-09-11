use super::ListWorkloads;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workloads::application::queries::{
    WorkloadQueryResult, reader::WorkloadQueryReader,
};
use crate::modules::workloads::application::{
    IWorkloadDeploymentOperationAccess, IWorkloadRuntimeObservationAccess,
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
        operations: Arc<dyn IWorkloadDeploymentOperationAccess>,
        observations: Arc<dyn IWorkloadRuntimeObservationAccess>,
    ) -> Self {
        Self {
            reader: WorkloadQueryReader::new(Arc::clone(&workloads), operations, observations),
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
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "workloads not found".into(),
                )));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodeId, OperationId, OrganizationId, ProjectId, RepositoryError,
    };
    use crate::modules::workloads::application::resource_access::{
        WorkloadAccess, WorkloadAccessScope,
    };
    use crate::modules::workloads::application::{
        IWorkloadDeploymentOperationAccess, IWorkloadRuntimeObservationAccess,
        WorkloadDeploymentOperationProjection, WorkloadRuntimeObservationProjection,
    };
    use crate::modules::workloads::infrastructure::InMemoryWorkloadRepository;
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;

    struct StubOperations;
    struct StubObservations;

    #[async_trait]
    impl IWorkloadDeploymentOperationAccess for StubOperations {
        async fn find_projection(
            &self,
            _operation_id: OperationId,
        ) -> Result<Option<WorkloadDeploymentOperationProjection>, RepositoryError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl IWorkloadRuntimeObservationAccess for StubObservations {
        async fn latest_runtime_observation(
            &self,
            _node_id: NodeId,
            _unit_id: &str,
            _generation: u64,
        ) -> Result<Option<WorkloadRuntimeObservationProjection>, RepositoryError> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_environment() {
        let project_id = ProjectId::new();
        let handler = ListWorkloadsHandler::new(
            Arc::new(InMemoryWorkloadRepository::new()),
            Arc::new(StubOperations),
            Arc::new(StubObservations),
        );
        let result = handler
            .execute(
                ListWorkloads {
                    organization_id: OrganizationId::new(),
                    project_id,
                    environment_id: EnvironmentId::new(),
                    access: WorkloadAccess::restricted([WorkloadAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message)) if message == "workloads not found"
        ));
    }
}
