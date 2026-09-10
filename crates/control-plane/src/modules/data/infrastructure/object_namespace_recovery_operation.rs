use crate::modules::data::application::{
    DeleteObjectNamespaceOperationInput, RestoreObjectNamespaceOperationInput,
    SealObjectNamespaceOperationInput, OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME,
    OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION, OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME,
    OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
};
use crate::modules::operations::{OperationRequest, OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{OperationId, OrganizationId, StorageNamespaceId};
use chrono::{DateTime, Utc};
use serde::Serialize;

/// Sole anti-corruption builder from Data recovery inputs to Operations requests.
///
/// Owning aggregates persist/enqueue the returned request atomically with their
/// own state; this builder does not add an S0 operation repository.
pub struct ObjectNamespaceRecoveryOperationRequest;

impl ObjectNamespaceRecoveryOperationRequest {
    pub fn seal(input: SealObjectNamespaceOperationInput) -> Result<OperationRequest, String> {
        input.validate()?;
        build_request(
            input.operation_id,
            input.organization_id,
            input.source.credentials.spec().namespace_id,
            OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
            input.sealed_at,
            &input,
        )
    }

    pub fn restore(
        input: RestoreObjectNamespaceOperationInput,
    ) -> Result<OperationRequest, String> {
        input.validate()?;
        build_request(
            input.operation_id,
            input.organization_id,
            input.restore_plan.spec().source_namespace_id,
            OBJECT_NAMESPACE_RESTORE_WORKFLOW_NAME,
            input.restore_plan.spec().requested_at,
            &input,
        )
    }

    pub fn delete(input: DeleteObjectNamespaceOperationInput) -> Result<OperationRequest, String> {
        input.validate()?;
        build_request(
            input.operation_id,
            input.organization_id,
            input.deletion_plan.spec().namespace_id,
            OBJECT_NAMESPACE_DELETE_WORKFLOW_NAME,
            input.deletion_plan.spec().requested_at,
            &input,
        )
    }
}

fn build_request<T: Serialize>(
    operation_id: OperationId,
    organization_id: OrganizationId,
    namespace_id: StorageNamespaceId,
    workflow_name: &str,
    requested_at: DateTime<Utc>,
    input: &T,
) -> Result<OperationRequest, String> {
    Ok(OperationRequest::new(
        operation_id,
        organization_id,
        OperationSubject::new("storage_namespace", namespace_id.as_uuid())?,
        WorkflowIdentity::new(workflow_name, OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION)?,
        serde_json::to_value(input)
            .map_err(|error| format!("could not encode object namespace operation: {error}"))?,
        requested_at,
    ))
}
