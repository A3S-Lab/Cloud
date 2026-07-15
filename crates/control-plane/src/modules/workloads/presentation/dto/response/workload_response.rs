use crate::modules::operations::domain::entities::OperationProjection;
use crate::modules::workloads::application::{DeploymentQueryResult, WorkloadQueryResult};
use crate::modules::workloads::domain::entities::WorkloadRevision;
use a3s_runtime::contract::{RuntimeHealthState, RuntimeUnitState};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub name: String,
    pub desired_state: String,
    pub desired_revision: Option<WorkloadRevisionResponse>,
    pub active_revision: Option<WorkloadRevisionResponse>,
    pub deployments: Vec<DeploymentResponse>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadRevisionResponse {
    pub id: Uuid,
    pub generation: u64,
    pub artifact_source_uri: String,
    pub expected_artifact_digest: Option<String>,
    pub request_digest: String,
    pub artifact_uri: Option<String>,
    pub artifact_digest: Option<String>,
    pub artifact_media_type: Option<String>,
    pub template_digest: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentResponse {
    pub id: Uuid,
    pub workload_id: Uuid,
    pub revision: WorkloadRevisionResponse,
    pub operation_id: Uuid,
    pub node_id: Option<Uuid>,
    pub command_id: Option<Uuid>,
    pub cleanup_command_id: Option<Uuid>,
    pub status: String,
    pub failure: Option<String>,
    pub operation: Option<DeploymentOperationResponse>,
    pub observed_runtime: Option<ObservedRuntimeResponse>,
    pub aggregate_version: u64,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub cancellation_requested_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentOperationResponse {
    pub status: String,
    pub last_sequence: u64,
    pub error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedRuntimeResponse {
    pub report_id: Uuid,
    pub node_id: Uuid,
    pub command_id: Option<Uuid>,
    pub unit_id: String,
    pub generation: u64,
    pub spec_digest: String,
    pub state: RuntimeUnitState,
    pub health_state: Option<RuntimeHealthState>,
    pub health_message: Option<String>,
    pub provider_resource_id: Option<String>,
    pub provider_build: Option<String>,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
}

impl From<WorkloadQueryResult> for WorkloadResponse {
    fn from(result: WorkloadQueryResult) -> Self {
        let desired_revision = result.desired_revision().cloned().map(Into::into);
        let active_revision = result.active_revision().cloned().map(Into::into);
        let workload = result.workload;
        Self {
            id: workload.id.as_uuid(),
            organization_id: workload.organization_id.as_uuid(),
            project_id: workload.project_id.as_uuid(),
            environment_id: workload.environment_id.as_uuid(),
            name: workload.name.as_str().to_owned(),
            desired_state: workload.desired_state.as_str().into(),
            desired_revision,
            active_revision,
            deployments: result.deployments.into_iter().map(Into::into).collect(),
            aggregate_version: workload.aggregate_version,
            created_at: workload.created_at,
            updated_at: workload.updated_at,
        }
    }
}

impl From<WorkloadRevision> for WorkloadRevisionResponse {
    fn from(revision: WorkloadRevision) -> Self {
        let (artifact_uri, artifact_digest, artifact_media_type) = revision
            .template
            .map(|template| {
                (
                    Some(template.artifact.uri),
                    Some(template.artifact.digest),
                    Some(template.artifact.media_type),
                )
            })
            .unwrap_or((None, None, None));
        Self {
            id: revision.id.as_uuid(),
            generation: revision.generation,
            artifact_source_uri: revision.request.artifact.uri,
            expected_artifact_digest: revision.request.artifact.expected_digest,
            request_digest: revision.request_digest,
            artifact_uri,
            artifact_digest,
            artifact_media_type,
            template_digest: revision.template_digest,
            created_at: revision.created_at,
            resolved_at: revision.resolved_at,
        }
    }
}

impl From<DeploymentQueryResult> for DeploymentResponse {
    fn from(result: DeploymentQueryResult) -> Self {
        let deployment = result.deployment;
        Self {
            id: deployment.id.as_uuid(),
            workload_id: deployment.workload_id.as_uuid(),
            revision: result.revision.into(),
            operation_id: deployment.operation_id.as_uuid(),
            node_id: deployment.node_id.map(|id| id.as_uuid()),
            command_id: deployment.command_id.map(|id| id.as_uuid()),
            cleanup_command_id: deployment.cleanup_command_id.map(|id| id.as_uuid()),
            status: deployment.status.as_str().into(),
            failure: deployment.failure,
            operation: result.operation.map(Into::into),
            observed_runtime: result.observation.map(ObservedRuntimeResponse::from),
            aggregate_version: deployment.aggregate_version,
            requested_at: deployment.requested_at,
            updated_at: deployment.updated_at,
            activated_at: deployment.activated_at,
            cancellation_requested_at: deployment.cancellation_requested_at,
            cancelled_at: deployment.cancelled_at,
        }
    }
}

impl From<OperationProjection> for DeploymentOperationResponse {
    fn from(operation: OperationProjection) -> Self {
        Self {
            status: operation.status.as_str().into(),
            last_sequence: operation.last_sequence,
            error: operation.error,
            updated_at: operation.updated_at,
        }
    }
}

impl From<crate::modules::fleet::domain::repositories::RuntimeObservationRecord>
    for ObservedRuntimeResponse
{
    fn from(record: crate::modules::fleet::domain::repositories::RuntimeObservationRecord) -> Self {
        let observation = record.observation;
        let (health_state, health_message) = observation
            .health
            .map(|health| (Some(health.state), health.message))
            .unwrap_or((None, None));
        let (failure_code, failure_message) = observation
            .failure
            .map(|failure| (Some(failure.code), Some(failure.message)))
            .unwrap_or((None, None));
        Self {
            report_id: record.report_id,
            node_id: record.node_id.as_uuid(),
            command_id: record.command_id.map(|id| id.as_uuid()),
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
    }
}
