use super::{encode, ObjectNamespaceRecoveryFlowRuntime, RecoveryStepOutput};
use crate::modules::data::application::{
    DeleteObjectNamespaceOperationInput, DeleteObjectNamespaceOperationOutput,
    RestoreObjectNamespaceOperationInput, RestoreObjectNamespaceOperationOutput,
    SealObjectNamespaceOperationInput, SealObjectNamespaceOperationOutput,
};
use crate::modules::data::domain::ObjectNamespaceError;
use crate::modules::shared_kernel::domain::canonical_timestamp;
use a3s_flow::FlowError;
use chrono::Utc;

pub(super) async fn seal(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: SealObjectNamespaceOperationInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.validate() {
        return encode(rejected::<SealObjectNamespaceOperationOutput>(error));
    }
    let (source, recovery) = match runtime.resolver.source_and_recovery(&input.source).await {
        Ok(access) => access,
        Err(error) => return resolve::<SealObjectNamespaceOperationOutput>(error),
    };
    match runtime
        .executor
        .seal(
            &source,
            &recovery,
            input.previous_recovery_point.as_ref(),
            input.writer_epoch,
            input.writer_fence_receipt_digest,
            input.sealed_at,
        )
        .await
    {
        Ok(recovery_point) => encode(RecoveryStepOutput::Completed {
            output: SealObjectNamespaceOperationOutput { recovery_point },
        }),
        Err(error) => resolve::<SealObjectNamespaceOperationOutput>(error),
    }
}

pub(super) async fn restore(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: RestoreObjectNamespaceOperationInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.validate() {
        return encode(rejected::<RestoreObjectNamespaceOperationOutput>(error));
    }
    let (_, recovery) = match runtime.resolver.source_and_recovery(&input.source).await {
        Ok(access) => access,
        Err(error) => return resolve::<RestoreObjectNamespaceOperationOutput>(error),
    };
    let target = match runtime.resolver.access(&input.target).await {
        Ok(access) => access,
        Err(error) => return resolve::<RestoreObjectNamespaceOperationOutput>(error),
    };
    let verified_at = canonical_timestamp(Utc::now()).max(input.restore_plan.spec().requested_at);
    match runtime
        .executor
        .restore(
            &recovery,
            &target,
            &input.recovery_point,
            &input.restore_plan,
            &input.retention_policy,
            verified_at,
        )
        .await
    {
        Ok(restore_evidence) => encode(RecoveryStepOutput::Completed {
            output: RestoreObjectNamespaceOperationOutput { restore_evidence },
        }),
        Err(error) => resolve::<RestoreObjectNamespaceOperationOutput>(error),
    }
}

pub(super) async fn delete(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: DeleteObjectNamespaceOperationInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.validate() {
        return encode(rejected::<DeleteObjectNamespaceOperationOutput>(error));
    }
    let (source, recovery) = match runtime.resolver.source_and_recovery(&input.source).await {
        Ok(access) => access,
        Err(error) => return resolve::<DeleteObjectNamespaceOperationOutput>(error),
    };
    let retained_restore = match runtime.resolver.access(&input.retained_restore).await {
        Ok(access) => access,
        Err(error) => return resolve::<DeleteObjectNamespaceOperationOutput>(error),
    };
    let completed_at = canonical_timestamp(Utc::now()).max(input.deletion_plan.spec().not_before);
    match runtime
        .executor
        .delete(
            &source,
            &recovery,
            &retained_restore,
            &input.recovery_point,
            &input.restore_plan,
            &input.restore_evidence,
            &input.deletion_plan,
            &input.retention_policy,
            completed_at,
        )
        .await
    {
        Ok(deletion_evidence) => encode(RecoveryStepOutput::Completed {
            output: DeleteObjectNamespaceOperationOutput { deletion_evidence },
        }),
        Err(error) => resolve::<DeleteObjectNamespaceOperationOutput>(error),
    }
}

fn rejected<T>(reason: String) -> RecoveryStepOutput<T> {
    RecoveryStepOutput::Rejected { reason }
}

fn resolve<T: serde::Serialize>(
    error: ObjectNamespaceError,
) -> a3s_flow::Result<serde_json::Value> {
    match error {
        ObjectNamespaceError::Unavailable(message) => Err(FlowError::Runtime(format!(
            "object namespace provider is temporarily unavailable: {message}"
        ))),
        ObjectNamespaceError::Invalid(message) => encode(rejected::<T>(format!(
            "invalid object namespace recovery request: {message}"
        ))),
        ObjectNamespaceError::Precondition(message) => encode(rejected::<T>(format!(
            "object namespace recovery precondition failed: {message}"
        ))),
        ObjectNamespaceError::Corrupt(message) => encode(rejected::<T>(format!(
            "object namespace recovery evidence is corrupt: {message}"
        ))),
        ObjectNamespaceError::Unsupported(message) => encode(rejected::<T>(format!(
            "object namespace provider is unsupported: {message}"
        ))),
    }
}
