use crate::modules::shared_kernel::domain::{
    DeploymentId, OperationId, OrganizationId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{Deployment, WorkloadRevision};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentRequested {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
    pub deployment_id: DeploymentId,
    pub operation_id: OperationId,
    pub generation: u64,
    pub artifact_source_uri: String,
    pub expected_artifact_digest: Option<String>,
    pub request_digest: String,
    pub artifact_digest: Option<String>,
    pub template_digest: Option<String>,
}

impl DeploymentRequested {
    pub fn envelope(
        deployment: &Deployment,
        revision: &WorkloadRevision,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "workload.deployment.requested".into(),
            schema_version: 2,
            organization_id: deployment.organization_id.as_uuid(),
            aggregate_id: deployment.id.as_uuid(),
            aggregate_version: deployment.aggregate_version,
            occurred_at: deployment.requested_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: deployment.organization_id,
                workload_id: deployment.workload_id,
                revision_id: deployment.revision_id,
                deployment_id: deployment.id,
                operation_id: deployment.operation_id,
                generation: revision.generation,
                artifact_source_uri: revision.request.artifact.uri.clone(),
                expected_artifact_digest: revision.request.artifact.expected_digest.clone(),
                request_digest: revision.request_digest.clone(),
                artifact_digest: revision
                    .template
                    .as_ref()
                    .map(|template| template.artifact.digest.clone()),
                template_digest: revision.template_digest.clone(),
            })?,
        })
    }
}
