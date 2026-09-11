use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::{NodeId, RepositoryError};
use crate::modules::workloads::application::{
    IWorkloadRuntimeObservationAccess, WorkloadRuntimeObservationProjection,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for Fleet Runtime observation enrichment.
#[derive(Clone)]
pub struct FleetWorkloadRuntimeObservationAccessAdapter {
    node_control: Arc<dyn INodeControlRepository>,
}

impl FleetWorkloadRuntimeObservationAccessAdapter {
    pub fn new(node_control: Arc<dyn INodeControlRepository>) -> Self {
        Self { node_control }
    }
}

#[async_trait]
impl IWorkloadRuntimeObservationAccess for FleetWorkloadRuntimeObservationAccessAdapter {
    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<WorkloadRuntimeObservationProjection>, RepositoryError> {
        Ok(self
            .node_control
            .latest_runtime_observation(node_id, unit_id, generation)
            .await?
            .map(|record| {
                let observation = record.observation;
                let (health_state, health_message) = observation
                    .health
                    .map(|health| (Some(health.state), health.message))
                    .unwrap_or((None, None));
                let (failure_code, failure_message) = observation
                    .failure
                    .map(|failure| (Some(failure.code), Some(failure.message)))
                    .unwrap_or((None, None));
                WorkloadRuntimeObservationProjection {
                    report_id: record.report_id,
                    node_id: record.node_id,
                    command_id: record.command_id,
                    unit_id: observation.unit_id,
                    generation: observation.generation,
                    spec_digest: observation.spec_digest,
                    state: observation.state,
                    health_state,
                    health_message,
                    provider_resource_id: observation.provider_resource_id,
                    provider_build: observation.provider_build,
                    failure_code,
                    failure_message,
                    observed_at: record.observed_at,
                    received_at: record.received_at,
                }
            }))
    }
}
