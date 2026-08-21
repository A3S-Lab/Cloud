use super::super::{encode, resolve, ObjectNamespaceRecoveryFlowRuntime, RecoveryStepOutput};
use crate::modules::data::application::{
    DeleteObjectNamespaceOperationInput, DeleteObjectNamespaceOperationOutput,
    ObjectNamespaceCleanupPageCheckpoint, ObjectNamespaceManifestPageCheckpoint,
    ObjectNamespaceObservationPageCheckpoint, ObjectNamespaceRecoveryAnchorCheckpoint,
    ObjectNamespaceSealPageCheckpoint, RestoreObjectNamespaceOperationInput,
    RestoreObjectNamespaceOperationOutput, SealObjectNamespaceOperationInput,
    SealObjectNamespaceOperationOutput,
};
use crate::modules::shared_kernel::domain::canonical_timestamp;
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SealPageInput {
    pub(super) operation: SealObjectNamespaceOperationInput,
    pub(super) page_index: u32,
    pub(super) previous: Option<ObjectNamespaceSealPageCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SealVerifyPageInput {
    pub(super) operation: SealObjectNamespaceOperationInput,
    pub(super) checkpoint: ObjectNamespaceSealPageCheckpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SealFinalizeInput {
    pub(super) operation: SealObjectNamespaceOperationInput,
    pub(super) pages: Vec<ObjectNamespaceSealPageCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RestorePreflightPageInput {
    pub(super) operation: RestoreObjectNamespaceOperationInput,
    pub(super) page_index: u32,
    pub(super) previous: Option<ObjectNamespaceObservationPageCheckpoint>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RestoreManifestPhase {
    Apply,
    Verify,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RestoreManifestPageInput {
    pub(super) operation: RestoreObjectNamespaceOperationInput,
    pub(super) phase: RestoreManifestPhase,
    pub(super) page_index: u32,
    pub(super) previous: Option<ObjectNamespaceManifestPageCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RestoreFinalizeInput {
    pub(super) operation: RestoreObjectNamespaceOperationInput,
    pub(super) preflight: ObjectNamespaceObservationPageCheckpoint,
    pub(super) verification: ObjectNamespaceManifestPageCheckpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeleteRetainedPageInput {
    pub(super) operation: DeleteObjectNamespaceOperationInput,
    pub(super) page_index: u32,
    pub(super) previous: Option<ObjectNamespaceManifestPageCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeleteSourcePreflightPageInput {
    pub(super) operation: DeleteObjectNamespaceOperationInput,
    pub(super) page_index: u32,
    pub(super) previous: Option<ObjectNamespaceObservationPageCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeleteMarkInput {
    pub(super) operation: DeleteObjectNamespaceOperationInput,
    pub(super) retained_checkpoint: ObjectNamespaceManifestPageCheckpoint,
    pub(super) source_checkpoint: ObjectNamespaceObservationPageCheckpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeleteSourcePageInput {
    pub(super) operation: DeleteObjectNamespaceOperationInput,
    pub(super) page_index: u32,
    pub(super) previous: Option<ObjectNamespaceManifestPageCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeleteRecoveryPlanPageInput {
    pub(super) operation: DeleteObjectNamespaceOperationInput,
    pub(super) page_index: u32,
    pub(super) previous: Option<ObjectNamespaceCleanupPageCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeleteRecoveryPageInput {
    pub(super) operation: DeleteObjectNamespaceOperationInput,
    pub(super) checkpoint: ObjectNamespaceCleanupPageCheckpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DeleteFinalizeInput {
    pub(super) operation: DeleteObjectNamespaceOperationInput,
    pub(super) retained_checkpoint: ObjectNamespaceManifestPageCheckpoint,
    pub(super) anchor_checkpoint: ObjectNamespaceRecoveryAnchorCheckpoint,
}

pub(super) async fn seal_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: SealPageInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<ObjectNamespaceSealPageCheckpoint>(error);
    }
    let (source, recovery) = match runtime
        .resolver
        .source_and_recovery(&input.operation.source)
        .await
    {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceSealPageCheckpoint>(error),
    };
    match runtime
        .executor
        .seal_page(
            &source,
            &recovery,
            input.operation.previous_recovery_point.as_ref(),
            input.operation.writer_epoch,
            &input.operation.writer_fence_receipt_digest,
            input.operation.sealed_at,
            input.page_index,
            input.previous.as_ref(),
        )
        .await
    {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceSealPageCheckpoint>(error),
    }
}

pub(super) async fn seal_verify_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: SealVerifyPageInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<ObjectNamespaceSealPageCheckpoint>(error);
    }
    let (source, recovery) = match runtime
        .resolver
        .source_and_recovery(&input.operation.source)
        .await
    {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceSealPageCheckpoint>(error),
    };
    match runtime
        .executor
        .verify_seal_page(
            &source,
            &recovery,
            input.operation.previous_recovery_point.as_ref(),
            input.operation.writer_epoch,
            &input.operation.writer_fence_receipt_digest,
            input.operation.sealed_at,
            &input.checkpoint,
        )
        .await
    {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceSealPageCheckpoint>(error),
    }
}

pub(super) async fn seal_finalize(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: SealFinalizeInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<SealObjectNamespaceOperationOutput>(error);
    }
    let (source, recovery) = match runtime
        .resolver
        .source_and_recovery(&input.operation.source)
        .await
    {
        Ok(access) => access,
        Err(error) => return resolve::<SealObjectNamespaceOperationOutput>(error),
    };
    match runtime
        .executor
        .finalize_seal_pages(
            &source,
            &recovery,
            input.operation.previous_recovery_point.as_ref(),
            input.operation.writer_epoch,
            input.operation.writer_fence_receipt_digest,
            input.operation.sealed_at,
            &input.pages,
        )
        .await
    {
        Ok(recovery_point) => completed(SealObjectNamespaceOperationOutput { recovery_point }),
        Err(error) => resolve::<SealObjectNamespaceOperationOutput>(error),
    }
}

pub(super) async fn restore_preflight_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: RestorePreflightPageInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<ObjectNamespaceObservationPageCheckpoint>(error);
    }
    let (recovery, target) = match restore_access(runtime, &input.operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceObservationPageCheckpoint>(error),
    };
    match runtime
        .executor
        .restore_preflight_page(
            &recovery,
            &target,
            &input.operation.recovery_point,
            &input.operation.restore_plan,
            &input.operation.retention_policy,
            input.page_index,
            input.previous.as_ref(),
        )
        .await
    {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceObservationPageCheckpoint>(error),
    }
}

pub(super) async fn restore_apply_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: RestoreManifestPageInput,
) -> a3s_flow::Result<serde_json::Value> {
    restore_manifest_page(runtime, input, RestoreManifestPhase::Apply).await
}

pub(super) async fn restore_verify_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: RestoreManifestPageInput,
) -> a3s_flow::Result<serde_json::Value> {
    restore_manifest_page(runtime, input, RestoreManifestPhase::Verify).await
}

async fn restore_manifest_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: RestoreManifestPageInput,
    expected_phase: RestoreManifestPhase,
) -> a3s_flow::Result<serde_json::Value> {
    if !matches!(
        (input.phase, expected_phase),
        (RestoreManifestPhase::Apply, RestoreManifestPhase::Apply)
            | (RestoreManifestPhase::Verify, RestoreManifestPhase::Verify)
    ) {
        return invalid::<ObjectNamespaceManifestPageCheckpoint>(
            "object namespace restore page changed its exact phase".into(),
        );
    }
    if let Err(error) = input.operation.validate() {
        return invalid::<ObjectNamespaceManifestPageCheckpoint>(error);
    }
    let (recovery, target) = match restore_access(runtime, &input.operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceManifestPageCheckpoint>(error),
    };
    let result = match input.phase {
        RestoreManifestPhase::Apply => {
            runtime
                .executor
                .restore_apply_page(
                    &recovery,
                    &target,
                    &input.operation.recovery_point,
                    &input.operation.restore_plan,
                    &input.operation.retention_policy,
                    input.page_index,
                    input.previous.as_ref(),
                )
                .await
        }
        RestoreManifestPhase::Verify => {
            runtime
                .executor
                .restore_verify_page(
                    &recovery,
                    &target,
                    &input.operation.recovery_point,
                    &input.operation.restore_plan,
                    &input.operation.retention_policy,
                    input.page_index,
                    input.previous.as_ref(),
                )
                .await
        }
    };
    match result {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceManifestPageCheckpoint>(error),
    }
}

pub(super) async fn restore_finalize(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: RestoreFinalizeInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<RestoreObjectNamespaceOperationOutput>(error);
    }
    let (recovery, target) = match restore_access(runtime, &input.operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<RestoreObjectNamespaceOperationOutput>(error),
    };
    let verified_at =
        canonical_timestamp(Utc::now()).max(input.operation.restore_plan.spec().requested_at);
    match runtime
        .executor
        .finalize_restore_pages(
            &recovery,
            &target,
            &input.operation.recovery_point,
            &input.operation.restore_plan,
            &input.operation.retention_policy,
            &input.preflight,
            &input.verification,
            verified_at,
        )
        .await
    {
        Ok(restore_evidence) => {
            completed(RestoreObjectNamespaceOperationOutput { restore_evidence })
        }
        Err(error) => resolve::<RestoreObjectNamespaceOperationOutput>(error),
    }
}

pub(super) async fn delete_retained_preflight_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: DeleteRetainedPageInput,
) -> a3s_flow::Result<serde_json::Value> {
    delete_retained_page(runtime, input, false).await
}

pub(super) async fn delete_retained_postflight_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: DeleteRetainedPageInput,
) -> a3s_flow::Result<serde_json::Value> {
    delete_retained_page(runtime, input, true).await
}

async fn delete_retained_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: DeleteRetainedPageInput,
    postflight: bool,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<ObjectNamespaceManifestPageCheckpoint>(error);
    }
    let (source, recovery, retained) = match delete_access(runtime, &input.operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceManifestPageCheckpoint>(error),
    };
    let result = if postflight {
        runtime
            .executor
            .delete_retained_postflight_page(
                &source,
                &recovery,
                &retained,
                &input.operation.recovery_point,
                &input.operation.restore_plan,
                &input.operation.restore_evidence,
                &input.operation.deletion_plan,
                &input.operation.retention_policy,
                input.page_index,
                input.previous.as_ref(),
            )
            .await
    } else {
        runtime
            .executor
            .delete_retained_preflight_page(
                &source,
                &recovery,
                &retained,
                &input.operation.recovery_point,
                &input.operation.restore_plan,
                &input.operation.restore_evidence,
                &input.operation.deletion_plan,
                &input.operation.retention_policy,
                input.page_index,
                input.previous.as_ref(),
            )
            .await
    };
    match result {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceManifestPageCheckpoint>(error),
    }
}

pub(super) async fn delete_source_preflight_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: DeleteSourcePreflightPageInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<ObjectNamespaceObservationPageCheckpoint>(error);
    }
    let (source, recovery, retained) = match delete_access(runtime, &input.operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceObservationPageCheckpoint>(error),
    };
    match runtime
        .executor
        .delete_source_preflight_page(
            &source,
            &recovery,
            &retained,
            &input.operation.recovery_point,
            &input.operation.restore_plan,
            &input.operation.restore_evidence,
            &input.operation.deletion_plan,
            &input.operation.retention_policy,
            input.page_index,
            input.previous.as_ref(),
        )
        .await
    {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceObservationPageCheckpoint>(error),
    }
}

pub(super) async fn delete_mark(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: DeleteMarkInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<()>(error);
    }
    let (source, recovery, retained) = match delete_access(runtime, &input.operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<()>(error),
    };
    match runtime
        .executor
        .mark_delete(
            &source,
            &recovery,
            &retained,
            &input.operation.recovery_point,
            &input.operation.restore_plan,
            &input.operation.restore_evidence,
            &input.operation.deletion_plan,
            &input.operation.retention_policy,
            &input.retained_checkpoint,
            &input.source_checkpoint,
        )
        .await
    {
        Ok(()) => completed(()),
        Err(error) => resolve::<()>(error),
    }
}

pub(super) async fn delete_source_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: DeleteSourcePageInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<ObjectNamespaceManifestPageCheckpoint>(error);
    }
    let (source, recovery, retained) = match delete_access(runtime, &input.operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceManifestPageCheckpoint>(error),
    };
    match runtime
        .executor
        .delete_source_page(
            &source,
            &recovery,
            &retained,
            &input.operation.recovery_point,
            &input.operation.restore_plan,
            &input.operation.restore_evidence,
            &input.operation.deletion_plan,
            &input.operation.retention_policy,
            input.page_index,
            input.previous.as_ref(),
        )
        .await
    {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceManifestPageCheckpoint>(error),
    }
}

pub(super) async fn delete_source_absence(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    operation: DeleteObjectNamespaceOperationInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = operation.validate() {
        return invalid::<ObjectNamespaceObservationPageCheckpoint>(error);
    }
    let (source, _, _) = match delete_access(runtime, &operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceObservationPageCheckpoint>(error),
    };
    match runtime
        .executor
        .confirm_source_absence(&source, &operation.deletion_plan)
        .await
    {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceObservationPageCheckpoint>(error),
    }
}

pub(super) async fn delete_recovery_plan_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: DeleteRecoveryPlanPageInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<ObjectNamespaceCleanupPageCheckpoint>(error);
    }
    let (_, recovery, _) = match delete_access(runtime, &input.operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceCleanupPageCheckpoint>(error),
    };
    match runtime
        .executor
        .plan_recovery_cleanup_page(
            &recovery,
            &input.operation.recovery_point,
            &input.operation.deletion_plan,
            &input.operation.retention_policy,
            input.page_index,
            input.previous.as_ref(),
        )
        .await
    {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceCleanupPageCheckpoint>(error),
    }
}

pub(super) async fn delete_recovery_page(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: DeleteRecoveryPageInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<ObjectNamespaceCleanupPageCheckpoint>(error);
    }
    let (_, recovery, _) = match delete_access(runtime, &input.operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceCleanupPageCheckpoint>(error),
    };
    match runtime
        .executor
        .delete_recovery_cleanup_page(&recovery, &input.operation.deletion_plan, &input.checkpoint)
        .await
    {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceCleanupPageCheckpoint>(error),
    }
}

pub(super) async fn delete_recovery_anchor(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    operation: DeleteObjectNamespaceOperationInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = operation.validate() {
        return invalid::<ObjectNamespaceRecoveryAnchorCheckpoint>(error);
    }
    let (_, recovery, _) = match delete_access(runtime, &operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<ObjectNamespaceRecoveryAnchorCheckpoint>(error),
    };
    match runtime
        .executor
        .delete_recovery_anchor(
            &recovery,
            &operation.recovery_point,
            &operation.deletion_plan,
            &operation.retention_policy,
        )
        .await
    {
        Ok(output) => completed(output),
        Err(error) => resolve::<ObjectNamespaceRecoveryAnchorCheckpoint>(error),
    }
}

pub(super) async fn delete_finalize(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    input: DeleteFinalizeInput,
) -> a3s_flow::Result<serde_json::Value> {
    if let Err(error) = input.operation.validate() {
        return invalid::<DeleteObjectNamespaceOperationOutput>(error);
    }
    let (source, recovery, retained) = match delete_access(runtime, &input.operation).await {
        Ok(access) => access,
        Err(error) => return resolve::<DeleteObjectNamespaceOperationOutput>(error),
    };
    let completed_at =
        canonical_timestamp(Utc::now()).max(input.operation.deletion_plan.spec().not_before);
    match runtime
        .executor
        .finalize_delete_pages(
            &source,
            &recovery,
            &retained,
            &input.operation.recovery_point,
            &input.operation.restore_plan,
            &input.operation.restore_evidence,
            &input.operation.deletion_plan,
            &input.operation.retention_policy,
            &input.retained_checkpoint,
            &input.anchor_checkpoint,
            completed_at,
        )
        .await
    {
        Ok(deletion_evidence) => {
            completed(DeleteObjectNamespaceOperationOutput { deletion_evidence })
        }
        Err(error) => resolve::<DeleteObjectNamespaceOperationOutput>(error),
    }
}

async fn restore_access(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    operation: &RestoreObjectNamespaceOperationInput,
) -> Result<
    (
        crate::modules::data::application::ObjectNamespaceRecoveryStore,
        crate::modules::data::application::ObjectNamespaceAccess,
    ),
    crate::modules::data::domain::ObjectNamespaceError,
> {
    let (_, recovery) = runtime
        .resolver
        .source_and_recovery(&operation.source)
        .await?;
    let target = runtime.resolver.access(&operation.target).await?;
    Ok((recovery, target))
}

async fn delete_access(
    runtime: &ObjectNamespaceRecoveryFlowRuntime,
    operation: &DeleteObjectNamespaceOperationInput,
) -> Result<
    (
        crate::modules::data::application::ObjectNamespaceAccess,
        crate::modules::data::application::ObjectNamespaceRecoveryStore,
        crate::modules::data::application::ObjectNamespaceAccess,
    ),
    crate::modules::data::domain::ObjectNamespaceError,
> {
    let (source, recovery) = runtime
        .resolver
        .source_and_recovery(&operation.source)
        .await?;
    let retained = runtime.resolver.access(&operation.retained_restore).await?;
    Ok((source, recovery, retained))
}

fn completed<T: Serialize>(output: T) -> a3s_flow::Result<serde_json::Value> {
    encode(RecoveryStepOutput::Completed { output })
}

fn invalid<T: Serialize>(reason: String) -> a3s_flow::Result<serde_json::Value> {
    encode(RecoveryStepOutput::<T>::Rejected {
        reason: format!("invalid object namespace recovery request: {reason}"),
    })
}
