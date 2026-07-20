use crate::modules::workloads::application::{
    CreateWorkloadDeploymentResult, UpdateWorkloadDeploymentResult,
};
use crate::modules::workloads::domain::repositories::DeploymentBundle;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadDeploymentResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub workload_id: Uuid,
    pub revision_id: Uuid,
    pub deployment_id: Uuid,
    pub operation_id: Uuid,
    pub generation: u64,
    pub status: String,
    pub artifact_source_uri: String,
    pub expected_artifact_digest: Option<String>,
    pub request_digest: String,
    pub artifact_digest: Option<String>,
    pub template_digest: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub replayed: bool,
}

impl From<CreateWorkloadDeploymentResult> for WorkloadDeploymentResponse {
    fn from(result: CreateWorkloadDeploymentResult) -> Self {
        Self::from_bundle(result.bundle)
    }
}

impl From<UpdateWorkloadDeploymentResult> for WorkloadDeploymentResponse {
    fn from(result: UpdateWorkloadDeploymentResult) -> Self {
        Self::from_bundle(result.bundle)
    }
}

impl WorkloadDeploymentResponse {
    fn from_bundle(bundle: DeploymentBundle) -> Self {
        Self {
            organization_id: bundle.workload.organization_id.as_uuid(),
            project_id: bundle.workload.project_id.as_uuid(),
            environment_id: bundle.workload.environment_id.as_uuid(),
            workload_id: bundle.workload.id.as_uuid(),
            revision_id: bundle.revision.id.as_uuid(),
            deployment_id: bundle.deployment.id.as_uuid(),
            operation_id: bundle.operation.id.as_uuid(),
            generation: bundle.revision.generation,
            status: bundle.deployment.status.as_str().into(),
            artifact_source_uri: bundle.revision.request.artifact.uri,
            expected_artifact_digest: bundle.revision.request.artifact.expected_digest,
            request_digest: bundle.revision.request_digest,
            artifact_digest: bundle
                .revision
                .template
                .map(|template| template.artifact.digest),
            template_digest: bundle.revision.template_digest,
            requested_at: bundle.deployment.requested_at,
            replayed: bundle.replayed,
        }
    }
}
