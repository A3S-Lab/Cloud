use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::workloads::application::{
    DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION, STOP_WORKFLOW_NAME,
    STOP_WORKFLOW_VERSION,
};
use crate::modules::workloads::domain::{
    WorkloadDeploymentOperationIntent, WorkloadStopOperationIntent,
};

/// Sole anti-corruption composition from Workloads operation intents to
/// Operations aggregates. Persistence and Flow adapters call these helpers;
/// Application command handlers never construct `OperationRequest`.
pub fn compose_deployment_operation(
    intent: &WorkloadDeploymentOperationIntent,
) -> Result<OperationRequest, String> {
    Ok(OperationRequest::new(
        intent.operation_id,
        intent.organization_id,
        OperationSubject::new("deployment", intent.deployment_id.as_uuid())?,
        WorkflowIdentity::new(DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION)?,
        serde_json::json!({
            "deploymentId": intent.deployment_id,
            "organizationId": intent.organization_id,
            "revisionId": intent.revision_id,
            "workloadId": intent.workload_id,
        }),
        intent.requested_at,
    ))
}

pub fn compose_stop_operation(
    intent: &WorkloadStopOperationIntent,
) -> Result<OperationRequest, String> {
    Ok(OperationRequest::new(
        intent.operation_id,
        intent.organization_id,
        OperationSubject::new("workload", intent.workload_id.as_uuid())?,
        WorkflowIdentity::new(STOP_WORKFLOW_NAME, STOP_WORKFLOW_VERSION)?,
        serde_json::json!({
            "operationId": intent.operation_id,
            "organizationId": intent.organization_id,
            "requestedAt": intent.requested_at,
            "workloadId": intent.workload_id,
        }),
        intent.requested_at,
    ))
}
