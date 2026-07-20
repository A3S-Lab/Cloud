use super::GetWorkloadLogs;
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, NodeLogChunkMetadata, NodeLogChunkQuery,
};
use crate::modules::fleet::domain::services::{
    ILogChunkStore, LogChunkStoreError, RetrievedLogChunk,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::application::queries::{
    WorkloadLogGapReason, WorkloadLogPage, WorkloadLogRecord,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use a3s_boot::{CqrsContext, QueryHandler};
use a3s_cloud_contracts::NodeLogChunkReport;
use std::sync::Arc;

pub struct GetWorkloadLogsHandler {
    workloads: Arc<dyn IWorkloadRepository>,
    metadata: Arc<dyn INodeControlRepository>,
    objects: Arc<dyn ILogChunkStore>,
}

impl GetWorkloadLogsHandler {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        metadata: Arc<dyn INodeControlRepository>,
        objects: Arc<dyn ILogChunkStore>,
    ) -> Self {
        Self {
            workloads,
            metadata,
            objects,
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
        let metadata = Arc::clone(&self.metadata);
        let objects = Arc::clone(&self.objects);
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
            let fetch_limit = usize::from(query.limit) + 1;
            let mut chunks = match metadata
                .list_log_chunks(NodeLogChunkQuery {
                    node_id,
                    unit_id: unit_id.clone(),
                    generation: revision.generation,
                    after_sequence: query.after_sequence,
                    limit: fetch_limit,
                    stream: query.stream,
                })
                .await
            {
                Ok(chunks) => chunks,
                Err(error) => return Ok(Err(error.into())),
            };
            let has_more = chunks.len() > usize::from(query.limit);
            if has_more {
                chunks.truncate(usize::from(query.limit));
            }
            let next_after_sequence = has_more
                .then(|| chunks.last().map(|chunk| chunk.sequence))
                .flatten();
            let mut records = Vec::with_capacity(chunks.len());
            for chunk in chunks {
                if chunk.retained_at.is_some() {
                    records.push(WorkloadLogRecord::Gap {
                        metadata: chunk,
                        reason: WorkloadLogGapReason::Retained,
                    });
                    continue;
                }
                let object = match objects.get(&chunk.object_key, &chunk.checksum).await {
                    Ok(object) => object,
                    Err(LogChunkStoreError::Unavailable(_)) => {
                        return Ok(Err(ApplicationError::Internal(
                            "workload log object storage is unavailable".into(),
                        )))
                    }
                    Err(LogChunkStoreError::Invalid(_) | LogChunkStoreError::Conflict(_)) => {
                        RetrievedLogChunk::Corrupt
                    }
                };
                records.push(match object {
                    RetrievedLogChunk::Found(report) if report_matches(&report, &chunk) => {
                        WorkloadLogRecord::Data(report.chunk)
                    }
                    RetrievedLogChunk::Found(_) | RetrievedLogChunk::Corrupt => {
                        WorkloadLogRecord::Gap {
                            metadata: chunk,
                            reason: WorkloadLogGapReason::Corrupt,
                        }
                    }
                    RetrievedLogChunk::Missing => WorkloadLogRecord::Gap {
                        metadata: chunk,
                        reason: WorkloadLogGapReason::Missing,
                    },
                });
            }
            Ok(Ok(WorkloadLogPage {
                workload_id: workload.id,
                revision_id: revision.id,
                node_id: Some(node_id),
                unit_id,
                generation: revision.generation,
                records,
                next_after_sequence,
            }))
        })
    }
}

fn report_matches(report: &NodeLogChunkReport, metadata: &NodeLogChunkMetadata) -> bool {
    report.unit_id == metadata.unit_id
        && report.generation == metadata.generation
        && report.chunk.cursor == metadata.cursor
        && report.chunk.sequence == metadata.sequence
        && report.chunk.observed_at_ms == metadata.observed_at_ms
        && report.chunk.stream == metadata.stream
        && report.checksum == metadata.checksum
}

fn logs_not_found() -> ApplicationError {
    ApplicationError::NotFound("workload revision logs not found".into())
}
