use super::GetWorkloadLogs;
use crate::modules::fleet::application::{NodeLogReadQuery, NodeLogReader};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::fleet::domain::services::ILogChunkStore;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::application::queries::WorkloadLogPage;
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkloadLogsHandler {
    workloads: Arc<dyn IWorkloadRepository>,
    logs: NodeLogReader,
}

impl GetWorkloadLogsHandler {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        metadata: Arc<dyn INodeControlRepository>,
        objects: Arc<dyn ILogChunkStore>,
    ) -> Self {
        Self {
            workloads,
            logs: NodeLogReader::new(metadata, objects),
        }
    }
}

impl QueryHandler<GetWorkloadLogs> for GetWorkloadLogsHandler {
    fn execute(
        &self,
        query: GetWorkloadLogs,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkloadLogPage>>> {
        let workloads = Arc::clone(&self.workloads);
        let logs = self.logs.clone();
        Box::pin(async move {
            if query.limit == 0 || query.limit > 256 {
                return Ok(Err(ApplicationError::Invalid(
                    "workload log limit must be between 1 and 256".into(),
                )));
            }
            let workload = match workloads
                .find_workload(query.organization_id, query.workload_id)
                .await
            {
                Ok(workload) => workload,
                Err(RepositoryError::NotFound) => return Ok(Err(logs_not_found())),
                Err(error) => return Ok(Err(error.into())),
            };
            let revision = match workloads
                .find_revision(query.organization_id, query.revision_id)
                .await
            {
                Ok(revision) if revision.workload_id == workload.id => revision,
                Ok(_) | Err(RepositoryError::NotFound) => return Ok(Err(logs_not_found())),
                Err(error) => return Ok(Err(error.into())),
            };
            let deployments = match workloads
                .list_deployments(query.organization_id, workload.id)
                .await
            {
                Ok(deployments) => deployments,
                Err(error) => return Ok(Err(error.into())),
            };
            let node_id = deployments
                .into_iter()
                .find(|deployment| {
                    deployment.revision_id == revision.id && deployment.node_id.is_some()
                })
                .and_then(|deployment| deployment.node_id);
            let unit_id = revision.runtime_unit_id();
            let Some(node_id) = node_id else {
                return Ok(Ok(WorkloadLogPage {
                    workload_id: workload.id,
                    revision_id: revision.id,
                    node_id: None,
                    unit_id,
                    generation: revision.generation,
                    records: Vec::new(),
                    next_after_sequence: None,
                }));
            };
            let page = match logs
                .read(NodeLogReadQuery {
                    node_id,
                    unit_id,
                    generation: revision.generation,
                    after_sequence: query.after_sequence,
                    limit: query.limit,
                    stream: query.stream,
                })
                .await
            {
                Ok(page) => page,
                Err(error) => return Ok(Err(error)),
            };
            Ok(Ok(WorkloadLogPage {
                workload_id: workload.id,
                revision_id: revision.id,
                node_id: Some(page.node_id),
                unit_id: page.unit_id,
                generation: page.generation,
                records: page.records,
                next_after_sequence: page.next_after_sequence,
            }))
        })
    }
}

fn logs_not_found() -> ApplicationError {
    ApplicationError::NotFound("workload revision logs not found".into())
}
