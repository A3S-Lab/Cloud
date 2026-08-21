use super::provider_workload::{
    restore_publisher_storage_credentials, validate_pinned_celld_provider_workload,
};
use crate::modules::data::{
    ObjectNamespaceFlowBinding, ObjectNamespaceRecoveryOperationRequest,
    SealObjectNamespaceOperationInput, SealObjectNamespaceOperationOutput,
};
use crate::modules::durable_cells::domain::{
    DurableCellApplicationDesiredState, DurableCellDeployment, DurableCellPublisherProfile,
    DurableCellServiceProfile, IDurableCellApplicationRepository, IDurableCellDeploymentRepository,
};
use crate::modules::durable_cells::infrastructure::admit_durable_cell_replica_runtime_remove;
use crate::modules::fleet::domain::entities::NodeCommand;
use crate::modules::operations::{IOperationRepository, OperationStatus};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, OperationId, RepositoryError, Sha256Digest,
};
use crate::modules::workloads::{
    IWorkloadRepository, IWorkloadWriterFenceAdapter, IWorkloadWriterFenceRepository,
    RetiringReplicaTarget, WorkloadWriterFenceCommit, WorkloadWriterFenceReceipt,
    WorkloadWriterFenceReceiptSpec,
};
use a3s_cloud_contracts::NodeCommandAck;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

const SEAL_OPERATION_NAME: &str = "a3s-cloud:durable-cell:writer-fence-seal:v1";
const MAX_ACKNOWLEDGEMENT_BYTES: usize = 128 * 1024;

/// Bridges a Durable Cell stop into the generic Workloads writer-fence hook.
/// It contributes only owner-specific validation and the S0 continuation;
/// Workloads still commits the Runtime fence, receipt, and Operation request.
#[derive(Clone)]
pub(crate) struct DurableCellWriterFenceAdapter {
    applications: Arc<dyn IDurableCellApplicationRepository>,
    deployments: Arc<dyn IDurableCellDeploymentRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    writer_fences: Arc<dyn IWorkloadWriterFenceRepository>,
    operations: Arc<dyn IOperationRepository>,
}

impl DurableCellWriterFenceAdapter {
    pub(crate) fn new(
        applications: Arc<dyn IDurableCellApplicationRepository>,
        deployments: Arc<dyn IDurableCellDeploymentRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
        writer_fences: Arc<dyn IWorkloadWriterFenceRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self {
            applications,
            deployments,
            workloads,
            writer_fences,
            operations,
        }
    }

    async fn find_stopped_correlation(
        &self,
        target: &RetiringReplicaTarget,
    ) -> Result<Option<DurableCellDeployment>, RepositoryError> {
        if target.replica.evacuation_node_id.is_some() {
            return Ok(None);
        }
        let Some(correlation) = self
            .deployments
            .find_by_workload_revision(target.replica.organization_id, target.revision.id)
            .await?
        else {
            return Ok(None);
        };
        correlation
            .validate()
            .map_err(|error| conflict("validate Durable Cell deployment correlation", error))?;
        let projection = &correlation.projection;
        if projection.organization_id != target.replica.organization_id
            || projection.project_id != target.replica.project_id
            || projection.environment_id != target.replica.environment_id
            || projection.workload_id != target.replica.workload_id
            || projection.workload_revision_id != target.revision.id
            || target.replica.revision_id != target.revision.id
            || target.replica.revision_generation != target.revision.generation
        {
            return Err(RepositoryError::Conflict(
                "Durable Cell writer fence changed its exact Workload projection".into(),
            ));
        }
        let Some(application) = self
            .applications
            .find(
                projection.organization_id,
                projection.project_id,
                projection.environment_id,
                projection.application_id,
            )
            .await?
        else {
            return Err(RepositoryError::Storage(
                "Durable Cell writer fence references a missing application".into(),
            ));
        };
        application
            .validate()
            .map_err(|error| conflict("validate Durable Cell application", error))?;
        if application.desired_state != DurableCellApplicationDesiredState::Stopped
            || application.current_revision_id != projection.application_revision_id
            || application.current_revision_number != projection.application_revision_number
            || application.current_definition_digest != projection.application_definition_digest
        {
            // Old-revision rollout/rollback retirement is deliberately outside
            // CELL0.5-C5a and must remain an ordinary Workloads retirement.
            return Ok(None);
        }
        let control = self
            .workloads
            .find_workload_control(projection.organization_id, projection.workload_id)
            .await?;
        let owner = projection
            .managed_owner_reference()
            .map_err(|error| conflict("restore Durable Cell managed owner", error))?;
        if control.spec.placement_policy.desired_replicas() != 0
            || control.spec.managed_owner.as_ref() != Some(&owner)
        {
            return Ok(None);
        }
        if target.replica.ordinal != 0 {
            return Err(RepositoryError::Conflict(
                "CELL0.5 writer fencing requires the canonical single replica".into(),
            ));
        }
        Ok(Some(correlation))
    }

    async fn previous_recovery_point(
        &self,
        correlation: &DurableCellDeployment,
        writer_epoch: u64,
    ) -> Result<Option<crate::modules::data::ObjectNamespaceRecoveryPoint>, RepositoryError> {
        let Some(receipt) = self
            .writer_fences
            .latest_writer_fence(
                correlation.projection.organization_id,
                correlation.projection.workload_id,
            )
            .await?
        else {
            return Ok(None);
        };
        let receipt_spec = receipt.spec();
        let expected_owner = correlation
            .projection
            .managed_owner_reference()
            .map_err(|error| conflict("restore Durable Cell managed owner", error))?;
        let previous_owner = &receipt_spec.managed_owner;
        if receipt_spec.writer_epoch >= writer_epoch
            || receipt_spec.organization_id != correlation.projection.organization_id
            || receipt_spec.project_id != correlation.projection.project_id
            || receipt_spec.environment_id != correlation.projection.environment_id
            || previous_owner.kind() != expected_owner.kind()
            || previous_owner.owner_id() != expected_owner.owner_id()
            || previous_owner.owner_generation() > expected_owner.owner_generation()
            || previous_owner.owner_generation() == expected_owner.owner_generation()
                && previous_owner != &expected_owner
        {
            return Err(RepositoryError::Conflict(
                "Durable Cell writer-fence lineage changed its exact predecessor".into(),
            ));
        }
        let request = self
            .operations
            .find_request(receipt_spec.continuation_operation_id)
            .await?
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "Durable Cell predecessor seal Operation request is missing".into(),
                )
            })?;
        let input: SealObjectNamespaceOperationInput =
            serde_json::from_value(request.input.clone()).map_err(|error| {
                RepositoryError::Storage(format!(
                    "Durable Cell predecessor seal input is invalid: {error}"
                ))
            })?;
        input
            .validate()
            .map_err(|error| conflict("validate Durable Cell predecessor seal", error))?;
        let expected_request = ObjectNamespaceRecoveryOperationRequest::seal(input.clone())
            .map_err(|error| conflict("rebuild Durable Cell predecessor seal", error))?;
        if !request.has_same_definition(&expected_request)
            || request.requested_at != expected_request.requested_at
            || input.writer_epoch != receipt_spec.writer_epoch
            || input.writer_fence_receipt_digest != *receipt.digest()
            || input.source.credentials.spec().namespace_id
                != correlation.storage.storage_namespace_id
            || input.source.provider_profile.digest()
                != &correlation.storage.provider_profile_digest
        {
            return Err(RepositoryError::Storage(
                "Durable Cell predecessor seal Operation drifted from its writer fence".into(),
            ));
        }
        let projection = self
            .operations
            .find_projection(receipt_spec.continuation_operation_id)
            .await?
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "Durable Cell predecessor seal has not produced a projection".into(),
                )
            })?;
        if projection.status != OperationStatus::Succeeded {
            return Err(RepositoryError::Conflict(format!(
                "Durable Cell predecessor seal is {:?}, not succeeded",
                projection.status
            )));
        }
        let output: SealObjectNamespaceOperationOutput =
            serde_json::from_value(projection.output.ok_or_else(|| {
                RepositoryError::Storage(
                    "succeeded Durable Cell predecessor seal omitted its output".into(),
                )
            })?)
            .map_err(|error| {
                RepositoryError::Storage(format!(
                    "Durable Cell predecessor seal output is invalid: {error}"
                ))
            })?;
        output
            .recovery_point
            .validate()
            .map_err(|error| conflict("validate Durable Cell predecessor recovery point", error))?;
        let point = output.recovery_point.spec();
        if point.namespace_id != correlation.storage.storage_namespace_id
            || point.provider_profile_digest != correlation.storage.provider_profile_digest
            || point.writer_epoch != receipt_spec.writer_epoch
            || point.sealed_at < receipt_spec.fenced_at
        {
            return Err(RepositoryError::Storage(
                "Durable Cell predecessor recovery point changed its sealed lineage".into(),
            ));
        }
        Ok(Some(output.recovery_point))
    }
}

#[async_trait]
impl IWorkloadWriterFenceAdapter for DurableCellWriterFenceAdapter {
    async fn prepare_replica_retirement(
        &self,
        target: &RetiringReplicaTarget,
        command: &NodeCommand,
        acknowledgement: &NodeCommandAck,
    ) -> Result<Option<WorkloadWriterFenceCommit>, RepositoryError> {
        let Some(correlation) = self.find_stopped_correlation(target).await? else {
            return Ok(None);
        };
        let replica_binding = target.replica_binding.as_ref().ok_or_else(|| {
            RepositoryError::Conflict(
                "Durable Cell writer fence omitted its deployed replica binding".into(),
            )
        })?;
        let service_profile = DurableCellServiceProfile::pinned_celld_v0_2_1()
            .map_err(|error| conflict("restore pinned Durable Cell Service profile", error))?;
        let provider_profile = correlation
            .require_storage_provider_profile()
            .map_err(|error| conflict("restore Durable Cell S0 provider profile", error))?;
        let publisher = DurableCellPublisherProfile::pinned_celld_v0_2_1()
            .map_err(|error| conflict("restore pinned Durable Cell publisher profile", error))?;
        let template = target
            .revision
            .resolved_template()
            .map_err(|error| conflict("resolve Durable Cell provider Workload", error))?;
        let credentials =
            restore_publisher_storage_credentials(&correlation.storage, template, &publisher)
                .map_err(|error| conflict("restore Durable Cell S0 credentials", error))?;
        validate_pinned_celld_provider_workload(
            &credentials,
            &provider_profile,
            &service_profile,
            template,
            &publisher,
        )
        .map_err(|error| conflict("validate Durable Cell provider Workload", error))?;
        let envelope = command
            .envelope(acknowledgement.lease_id)
            .map_err(|error| conflict("restore Fleet RuntimeRemove envelope", error))?;
        admit_durable_cell_replica_runtime_remove(
            &correlation.provider,
            &service_profile,
            &target.revision,
            replica_binding,
            &envelope,
            acknowledgement,
        )
        .map_err(|error| conflict("admit Durable Cell RuntimeRemove receipt", error))?;

        let operation_id = seal_operation_id(
            target.replica.workload_id.as_uuid(),
            target.replica.generation,
        );
        let fenced_at =
            canonical_timestamp(acknowledgement.completed_at.max(target.replica.updated_at));
        let acknowledgement_bytes = canonical_json_bounded(
            acknowledgement,
            MAX_ACKNOWLEDGEMENT_BYTES,
            "Fleet RuntimeRemove acknowledgement",
        )
        .map_err(|error| conflict("digest Fleet RuntimeRemove acknowledgement", error))?;
        let receipt = WorkloadWriterFenceReceipt::issue(WorkloadWriterFenceReceiptSpec {
            organization_id: target.replica.organization_id,
            project_id: target.replica.project_id,
            environment_id: target.replica.environment_id,
            workload_id: target.replica.workload_id,
            workload_revision_id: target.revision.id,
            workload_revision_generation: target.revision.generation,
            replica_id: target.replica.id,
            replica_ordinal: target.replica.ordinal,
            writer_epoch: target.replica.generation,
            member_id: target.member.id,
            placement_generation: target.member.placement_generation,
            managed_owner: correlation
                .projection
                .managed_owner_reference()
                .map_err(|error| conflict("restore Durable Cell managed owner", error))?,
            node_id: target.member.node_id.ok_or_else(|| {
                RepositoryError::Conflict(
                    "Durable Cell writer fence omitted its Runtime node".into(),
                )
            })?,
            runtime_unit_id: replica_binding.runtime_unit_id.clone(),
            command_id: command.id,
            command_payload_digest: Sha256Digest::parse(
                command
                    .payload_digest()
                    .map_err(|error| conflict("digest Fleet RuntimeRemove command", error))?,
            )
            .map_err(|error| conflict("parse Fleet RuntimeRemove digest", error))?,
            acknowledgement_digest: Sha256Digest::from_bytes(&acknowledgement_bytes),
            continuation_operation_id: operation_id,
            fenced_at,
        })
        .map_err(|error| conflict("issue Workload writer-fence receipt", error))?;
        let previous_recovery_point = self
            .previous_recovery_point(&correlation, target.replica.generation)
            .await?;
        let operation =
            ObjectNamespaceRecoveryOperationRequest::seal(SealObjectNamespaceOperationInput {
                operation_id,
                organization_id: target.replica.organization_id,
                source: ObjectNamespaceFlowBinding {
                    provider_profile,
                    credentials,
                },
                previous_recovery_point,
                writer_epoch: target.replica.generation,
                writer_fence_receipt_digest: receipt.digest().clone(),
                sealed_at: fenced_at,
            })
            .map_err(|error| conflict("compose Durable Cell namespace seal Operation", error))?;
        let commit = WorkloadWriterFenceCommit { receipt, operation };
        commit
            .validate()
            .map_err(|error| conflict("validate Durable Cell writer-fence handoff", error))?;
        Ok(Some(commit))
    }
}

fn seal_operation_id(workload_id: Uuid, writer_epoch: u64) -> OperationId {
    OperationId::from_uuid(Uuid::new_v5(
        &workload_id,
        format!("{SEAL_OPERATION_NAME}:{writer_epoch}").as_bytes(),
    ))
}

fn conflict(context: &str, error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Conflict(format!("{context}: {error}"))
}
