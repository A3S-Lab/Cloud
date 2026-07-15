use crate::modules::workloads::application::CancelDeploymentResult;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelDeploymentResponse {
    pub deployment_id: Uuid,
    pub operation_id: Uuid,
    pub status: String,
    pub replayed: bool,
}

impl From<CancelDeploymentResult> for CancelDeploymentResponse {
    fn from(result: CancelDeploymentResult) -> Self {
        Self {
            deployment_id: result.deployment.id.as_uuid(),
            operation_id: result.deployment.operation_id.as_uuid(),
            status: result.deployment.status.as_str().into(),
            replayed: result.replayed,
        }
    }
}
