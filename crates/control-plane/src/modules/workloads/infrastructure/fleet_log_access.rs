use crate::modules::fleet::application::{NodeLogReadQuery, NodeLogReader};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::fleet::domain::services::ILogChunkStore;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workloads::application::{
    IWorkloadLogAccess, WorkloadLogReadQuery, WorkloadLogReadResult,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for Fleet Runtime log pages.
#[derive(Clone)]
pub struct FleetWorkloadLogAccessAdapter {
    logs: NodeLogReader,
}

impl FleetWorkloadLogAccessAdapter {
    pub fn new(
        metadata: Arc<dyn INodeControlRepository>,
        objects: Arc<dyn ILogChunkStore>,
    ) -> Self {
        Self {
            logs: NodeLogReader::new(metadata, objects),
        }
    }
}

#[async_trait]
impl IWorkloadLogAccess for FleetWorkloadLogAccessAdapter {
    async fn read(&self, query: WorkloadLogReadQuery) -> ApplicationResult<WorkloadLogReadResult> {
        let page = self
            .logs
            .read(NodeLogReadQuery {
                node_id: query.node_id,
                unit_id: query.unit_id,
                generation: query.generation,
                after_sequence: query.after_sequence,
                limit: query.limit,
                stream: query.stream,
            })
            .await?;
        Ok(WorkloadLogReadResult {
            node_id: page.node_id,
            unit_id: page.unit_id,
            generation: page.generation,
            records: page.records,
            next_after_sequence: page.next_after_sequence,
        })
    }
}
