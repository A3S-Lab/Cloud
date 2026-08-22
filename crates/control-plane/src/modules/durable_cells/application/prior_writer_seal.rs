use crate::modules::data::{
    ObjectNamespaceRecoveryOperationRequest, ObjectNamespaceRecoveryPoint,
    SealObjectNamespaceOperationInput, SealObjectNamespaceOperationOutput,
};
use crate::modules::durable_cells::domain::DurableCellDeployment;
use crate::modules::operations::{IOperationRepository, OperationStatus};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::IWorkloadWriterFenceRepository;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct DurableCellPriorWriterSeal {
    writer_fences: Arc<dyn IWorkloadWriterFenceRepository>,
    operations: Arc<dyn IOperationRepository>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DurableCellPriorWriterSealStatus {
    Ready {
        recovery_point: Option<ObjectNamespaceRecoveryPoint>,
    },
    Pending {
        reason: String,
    },
    Failed {
        reason: String,
    },
}

impl DurableCellPriorWriterSeal {
    pub(crate) fn new(
        writer_fences: Arc<dyn IWorkloadWriterFenceRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self {
            writer_fences,
            operations,
        }
    }

    /// Reconciles the sole prior-writer admission authority for a Durable Cell.
    /// A missing receipt means this is the first writer. Once Workloads has
    /// issued a receipt, only the exact successful S0 seal may admit a later
    /// writer epoch.
    pub(super) async fn reconcile(
        &self,
        correlation: &DurableCellDeployment,
        next_writer_epoch: u64,
    ) -> Result<DurableCellPriorWriterSealStatus, RepositoryError> {
        correlation
            .validate()
            .map_err(|error| conflict("validate Durable Cell deployment correlation", error))?;
        if next_writer_epoch == 0 {
            return Err(RepositoryError::Conflict(
                "Durable Cell next writer epoch must be positive".into(),
            ));
        }
        let Some(receipt) = self
            .writer_fences
            .latest_writer_fence(
                correlation.projection.organization_id,
                correlation.projection.workload_id,
            )
            .await?
        else {
            return Ok(DurableCellPriorWriterSealStatus::Ready {
                recovery_point: None,
            });
        };
        receipt.validate().map_err(|error| {
            RepositoryError::Storage(format!(
                "stored Durable Cell writer-fence receipt is invalid: {error}"
            ))
        })?;
        let receipt_spec = receipt.spec();
        let expected_owner = correlation
            .projection
            .managed_owner_reference()
            .map_err(|error| conflict("restore Durable Cell managed owner", error))?;
        let previous_owner = &receipt_spec.managed_owner;
        if receipt_spec.organization_id != correlation.projection.organization_id
            || receipt_spec.project_id != correlation.projection.project_id
            || receipt_spec.environment_id != correlation.projection.environment_id
            || receipt_spec.workload_id != correlation.projection.workload_id
            || receipt_spec.workload_revision_generation > correlation.provider.workload_generation
            || receipt_spec.replica_ordinal != 0
            || receipt_spec.writer_epoch >= next_writer_epoch
            || previous_owner.kind() != expected_owner.kind()
            || previous_owner.owner_id() != expected_owner.owner_id()
            || previous_owner.owner_generation() > expected_owner.owner_generation()
            || previous_owner.owner_generation() == expected_owner.owner_generation()
                && previous_owner != &expected_owner
        {
            return Err(RepositoryError::Conflict(
                "Durable Cell prior-writer receipt changed its exact lineage".into(),
            ));
        }

        let request = self
            .operations
            .find_request(receipt_spec.continuation_operation_id)
            .await?
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "Durable Cell prior-writer seal Operation request is missing".into(),
                )
            })?;
        let input: SealObjectNamespaceOperationInput =
            serde_json::from_value(request.input.clone()).map_err(|error| {
                RepositoryError::Storage(format!(
                    "Durable Cell prior-writer seal input is invalid: {error}"
                ))
            })?;
        input
            .validate()
            .map_err(|error| conflict("validate Durable Cell prior-writer seal", error))?;
        input
            .source
            .credentials
            .validate_scope(
                correlation.projection.organization_id,
                correlation.projection.project_id,
                correlation.projection.environment_id,
                correlation.storage.storage_namespace_id,
            )
            .map_err(|error| conflict("validate Durable Cell prior-writer seal scope", error))?;
        let expected_request = ObjectNamespaceRecoveryOperationRequest::seal(input.clone())
            .map_err(|error| conflict("rebuild Durable Cell prior-writer seal", error))?;
        if !request.has_same_definition(&expected_request)
            || request.requested_at != expected_request.requested_at
            || input.operation_id != receipt_spec.continuation_operation_id
            || input.organization_id != correlation.projection.organization_id
            || input.writer_epoch != receipt_spec.writer_epoch
            || input.writer_fence_receipt_digest != *receipt.digest()
            || input.sealed_at != receipt_spec.fenced_at
            || input.source.credentials.spec().namespace_id
                != correlation.storage.storage_namespace_id
            || input.source.provider_profile.digest()
                != &correlation.storage.provider_profile_digest
        {
            return Err(RepositoryError::Storage(
                "Durable Cell prior-writer seal Operation drifted from its receipt".into(),
            ));
        }

        let Some(projection) = self
            .operations
            .find_projection(receipt_spec.continuation_operation_id)
            .await?
        else {
            return Ok(DurableCellPriorWriterSealStatus::Pending {
                reason: "Durable Cell prior-writer seal is queued".into(),
            });
        };
        if projection.operation_id != receipt_spec.continuation_operation_id
            || projection.last_sequence == 0
            || projection.updated_at < receipt_spec.fenced_at
        {
            return Err(RepositoryError::Storage(
                "Durable Cell prior-writer seal projection changed its exact identity".into(),
            ));
        }
        match projection.status {
            OperationStatus::Queued
            | OperationStatus::Running
            | OperationStatus::Suspended
            | OperationStatus::Cancelling => Ok(DurableCellPriorWriterSealStatus::Pending {
                reason: format!(
                    "Durable Cell prior-writer seal is {}",
                    projection.status.as_str()
                ),
            }),
            OperationStatus::Failed | OperationStatus::Cancelled => {
                Ok(DurableCellPriorWriterSealStatus::Failed {
                    reason: format!(
                        "Durable Cell prior-writer seal {}",
                        projection.status.as_str()
                    ),
                })
            }
            OperationStatus::Succeeded => {
                if projection.error.is_some() {
                    return Err(RepositoryError::Storage(
                        "succeeded Durable Cell prior-writer seal retained an error".into(),
                    ));
                }
                let output: SealObjectNamespaceOperationOutput =
                    serde_json::from_value(projection.output.ok_or_else(|| {
                        RepositoryError::Storage(
                            "succeeded Durable Cell prior-writer seal omitted its output".into(),
                        )
                    })?)
                    .map_err(|error| {
                        RepositoryError::Storage(format!(
                            "Durable Cell prior-writer seal output is invalid: {error}"
                        ))
                    })?;
                output.recovery_point.validate().map_err(|error| {
                    conflict("validate Durable Cell prior recovery point", error)
                })?;
                if let Some(previous) = &input.previous_recovery_point {
                    output
                        .recovery_point
                        .validate_successor_of(previous)
                        .map_err(|error| {
                            conflict("validate Durable Cell recovery-point successor", error)
                        })?;
                }
                let point = output.recovery_point.spec();
                if point.namespace_id != correlation.storage.storage_namespace_id
                    || point.provider_profile_digest != correlation.storage.provider_profile_digest
                    || point.writer_epoch != receipt_spec.writer_epoch
                    || point.sealed_at < receipt_spec.fenced_at
                {
                    return Err(RepositoryError::Storage(
                        "Durable Cell prior recovery point changed its sealed lineage".into(),
                    ));
                }
                Ok(DurableCellPriorWriterSealStatus::Ready {
                    recovery_point: Some(output.recovery_point),
                })
            }
        }
    }
}

fn conflict(context: &str, error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Conflict(format!("{context}: {error}"))
}
