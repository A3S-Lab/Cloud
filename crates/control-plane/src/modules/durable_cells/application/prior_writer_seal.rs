use super::operation_port::{
    DurableCellOperationLookupRequest, DurableCellOperationStatus, IDurableCellOperationPort,
};
use super::storage_port::{
    DurableCellStorageRecoveryPointProjection, DurableCellStorageSealRequest,
    IDurableCellStoragePort,
};
use super::workload_port::{DurableCellWorkloadPriorWriterFenceRequest, IDurableCellWorkloadPort};
use crate::modules::durable_cells::domain::DurableCellDeployment;
use crate::modules::shared_kernel::domain::{RepositoryError, WorkloadReplicaId};
use std::sync::Arc;

const OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME: &str = "cloud.object-namespace.seal";
const OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION: &str = "2";

#[derive(Clone)]
pub(crate) struct DurableCellPriorWriterSeal {
    workload_port: Arc<dyn IDurableCellWorkloadPort>,
    operation_port: Arc<dyn IDurableCellOperationPort>,
    storage_port: Arc<dyn IDurableCellStoragePort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DurableCellPriorWriterSealStatus {
    Ready {
        recovery_point: Option<DurableCellStorageRecoveryPointProjection>,
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
        operation_port: Arc<dyn IDurableCellOperationPort>,
        storage_port: Arc<dyn IDurableCellStoragePort>,
    ) -> Self {
        Self {
            workload_port,
            operation_port,
            storage_port,
        }
    }

    pub(crate) fn storage_port(&self) -> Arc<dyn IDurableCellStoragePort> {
        Arc::clone(&self.storage_port)
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

        let operation_lookup = DurableCellOperationLookupRequest::new(
            prior_writer.continuation_operation_id,
            correlation.projection.organization_id,
            "storage_namespace",
            correlation.storage.storage_namespace_id.as_uuid(),
            OBJECT_NAMESPACE_SEAL_WORKFLOW_NAME,
            OBJECT_NAMESPACE_RECOVERY_WORKFLOW_VERSION,
        );
        let operation = self
            .operation_port
            .load_exact(&operation_lookup)
            .await
            .map_err(operation_repository_error)?;
        let request = operation.request;
        let seal_request = DurableCellStorageSealRequest::new(
            request.operation_id,
            correlation.projection.organization_id,
            correlation.projection.project_id,
            correlation.projection.environment_id,
            correlation.storage.storage_namespace_id,
            correlation.storage.provider_profile_digest.clone(),
            prior_writer.writer_epoch,
            prior_writer.receipt_digest.clone(),
            prior_writer.fenced_at,
        );
        let input = self
            .storage_port
            .validate_seal_input(&seal_request, &request.input)
            .await
            .map_err(storage_repository_error)?;
        if request.requested_at != seal_request.sealed_at {
            return Err(RepositoryError::Storage(
                "Durable Cell prior-writer seal Operation timestamp drifted from its receipt"
                    .into(),
            ));
        }

        let Some(projection) = operation.projection else {
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
            DurableCellOperationStatus::Queued
            | DurableCellOperationStatus::Running
            | DurableCellOperationStatus::Suspended
            | DurableCellOperationStatus::Cancelling => {
                Ok(DurableCellPriorWriterSealStatus::Pending {
                    reason: format!(
                        "Durable Cell prior-writer seal is {}",
                        projection.status.as_str()
                    ),
                })
            }
            DurableCellOperationStatus::Failed | DurableCellOperationStatus::Cancelled => {
                Ok(DurableCellPriorWriterSealStatus::Failed {
                    reason: format!(
                        "Durable Cell prior-writer seal {}",
                        projection.status.as_str()
                    ),
                })
            }
            DurableCellOperationStatus::Succeeded => {
                if projection.error.is_some() {
                    return Err(RepositoryError::Storage(
                        "succeeded Durable Cell prior-writer seal retained an error".into(),
                    ));
                }
                let output = projection.output.ok_or_else(|| {
                    RepositoryError::Storage(
                        "succeeded Durable Cell prior-writer seal omitted its output".into(),
                    )
                })?;
                let recovery_point = self
                    .storage_port
                    .project_seal_output(&seal_request, &input, &output)
                    .await
                    .map_err(storage_repository_error)?;
                if recovery_point.namespace_id != correlation.storage.storage_namespace_id
                    || recovery_point.provider_profile_digest
                        != correlation.storage.provider_profile_digest
                    || recovery_point.writer_epoch != prior_writer.writer_epoch
                    || recovery_point.sealed_at < prior_writer.fenced_at
                {
                    return Err(RepositoryError::Storage(
                        "Durable Cell prior recovery point changed its sealed lineage".into(),
                    ));
                }
                Ok(DurableCellPriorWriterSealStatus::Ready {
                    recovery_point: Some(recovery_point),
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

fn storage_repository_error(
    error: crate::modules::shared_kernel::application::ApplicationError,
) -> RepositoryError {
    match error {
        crate::modules::shared_kernel::application::ApplicationError::NotFound(reason) => {
            RepositoryError::Storage(reason)
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

fn operation_repository_error(
    error: crate::modules::shared_kernel::application::ApplicationError,
) -> RepositoryError {
    match error {
        crate::modules::shared_kernel::application::ApplicationError::NotFound(_) => {
            RepositoryError::Storage(
                "Durable Cell prior-writer seal Operation request is missing".into(),
            )
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
