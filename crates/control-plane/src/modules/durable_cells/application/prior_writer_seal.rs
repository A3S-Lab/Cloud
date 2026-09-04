use super::workload_port::{DurableCellWorkloadPriorWriterFenceRequest, IDurableCellWorkloadPort};
use crate::modules::data::{
    ObjectNamespaceRecoveryOperationRequest, ObjectNamespaceRecoveryPoint,
    SealObjectNamespaceOperationInput, SealObjectNamespaceOperationOutput,
};
use crate::modules::durable_cells::domain::DurableCellDeployment;
use crate::modules::operations::{IOperationRepository, OperationStatus};
use crate::modules::shared_kernel::domain::{RepositoryError, WorkloadReplicaId};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct DurableCellPriorWriterSeal {
    workload_port: Arc<dyn IDurableCellWorkloadPort>,
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
        workload_port: Arc<dyn IDurableCellWorkloadPort>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self {
            workload_port,
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
        let prior_writer_request = DurableCellWorkloadPriorWriterFenceRequest::new(
            correlation.projection.organization_id,
            correlation.projection.project_id,
            correlation.projection.environment_id,
            correlation.projection.application_id,
            correlation.projection.application_revision_id,
            correlation.projection.application_revision_number,
            correlation.projection.application_definition_digest.clone(),
            correlation.projection.workload_id,
            correlation.projection.workload_revision_id,
            correlation.provider.workload_generation,
            WorkloadReplicaId::from_uuid(correlation.projection.workload_id.as_uuid()),
            0,
            next_writer_epoch,
        );
        let Some(prior_writer) = self
            .workload_port
            .load_prior_writer_fence(&prior_writer_request)
            .await
            .map_err(application_repository_error)?
        else {
            return Ok(DurableCellPriorWriterSealStatus::Ready {
                recovery_point: None,
            });
        };

        let request = self
            .operations
            .find_request(prior_writer.continuation_operation_id)
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
            || input.operation_id != prior_writer.continuation_operation_id
            || input.organization_id != correlation.projection.organization_id
            || input.writer_epoch != prior_writer.writer_epoch
            || input.writer_fence_receipt_digest != prior_writer.receipt_digest
            || input.sealed_at != prior_writer.fenced_at
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
            .find_projection(prior_writer.continuation_operation_id)
            .await?
        else {
            return Ok(DurableCellPriorWriterSealStatus::Pending {
                reason: "Durable Cell prior-writer seal is queued".into(),
            });
        };
        if projection.operation_id != prior_writer.continuation_operation_id
            || projection.last_sequence == 0
            || projection.updated_at < prior_writer.fenced_at
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
                    || point.writer_epoch != prior_writer.writer_epoch
                    || point.sealed_at < prior_writer.fenced_at
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

fn application_repository_error(
    error: crate::modules::shared_kernel::application::ApplicationError,
) -> RepositoryError {
    match error {
        crate::modules::shared_kernel::application::ApplicationError::NotFound(_) => {
            RepositoryError::NotFound
        }
        crate::modules::shared_kernel::application::ApplicationError::Internal(reason)
        | crate::modules::shared_kernel::application::ApplicationError::Unavailable(reason) => {
            RepositoryError::Storage(reason)
        }
        crate::modules::shared_kernel::application::ApplicationError::Invalid(reason)
        | crate::modules::shared_kernel::application::ApplicationError::Conflict(reason)
        | crate::modules::shared_kernel::application::ApplicationError::Forbidden(reason) => {
            RepositoryError::Conflict(reason)
        }
    }
}
