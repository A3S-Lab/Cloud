use crate::modules::durable_cells::application::{
    DurableCellWorkloadReconciliationRequest, DurableCellWorkloadRevisionGenerationRequest,
    IDurableCellWorkloadPort,
};
use crate::modules::durable_cells::domain::{
    DurableCellApplicationDesiredState, DurableCellProjectionIdentity,
    IDurableCellApplicationRepository, DURABLE_CELL_MANAGED_OWNER_KIND,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use crate::modules::workloads::{IWorkloadRepository, ReconfigureReplicaSetWrite};
use async_trait::async_trait;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

/// Anti-corruption adapter from the Workloads owner to the Durable Cells
/// consumer-owned reconciliation port.
///
/// Workloads remains the sole authority for replica retirement, Runtime
/// fencing, cleanup, and restart. This adapter only translates the exact
/// Durable Cell application intent into the existing managed replica-set
/// command and validates the returned owner projection.
#[derive(Clone)]
pub struct WorkloadsDurableCellWorkloadAdapter {
    applications: Arc<dyn IDurableCellApplicationRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
}

impl WorkloadsDurableCellWorkloadAdapter {
    pub fn new(
        applications: Arc<dyn IDurableCellApplicationRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
    ) -> Self {
        Self {
            applications,
            workloads,
        }
    }
}

#[async_trait]
impl IDurableCellWorkloadPort for WorkloadsDurableCellWorkloadAdapter {
    async fn resolve_revision_generation(
        &self,
        request: &DurableCellWorkloadRevisionGenerationRequest,
    ) -> ApplicationResult<u64> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let revisions = match self
            .workloads
            .list_revisions(request.organization_id, request.workload_id)
            .await
        {
            Ok(revisions) => revisions,
            Err(RepositoryError::NotFound) => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        if revisions
            .iter()
            .any(|revision| revision.workload_id != request.workload_id)
        {
            return Err(ApplicationError::Internal(
                "Durable Cell Workloads revision crossed its workload scope".into(),
            ));
        }
        if let Some(existing) = revisions
            .iter()
            .find(|revision| revision.id == request.workload_revision_id)
        {
            let template_digest = existing
                .resolved_template()
                .and_then(|template| template.digest())
                .map_err(|error| {
                    ApplicationError::Internal(format!(
                        "Durable Cell Workloads revision could not resolve its template: {error}"
                    ))
                })?;
            if template_digest != request.service_template_digest.as_str() {
                return Err(ApplicationError::Conflict(
                    "Durable Cell Workload revision identity already has another template".into(),
                ));
            }
            if existing.generation == 0 {
                return Err(ApplicationError::Internal(
                    "Durable Cell Workloads revision has an invalid generation".into(),
                ));
            }
            return Ok(existing.generation);
        }
        revisions
            .iter()
            .map(|revision| revision.generation)
            .max()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or_else(|| ApplicationError::Internal("Workload generation is exhausted".into()))
    }

    async fn converge_managed_replicas(
        &self,
        request: &DurableCellWorkloadReconciliationRequest,
    ) -> ApplicationResult<()> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let application = match self
            .applications
            .find(
                request.organization_id,
                request.project_id,
                request.environment_id,
                request.application_id,
            )
            .await
        {
            Ok(Some(value)) => value,
            Ok(None) | Err(RepositoryError::NotFound) => {
                return Err(ApplicationError::Internal(
                    "Durable Cell application disappeared while converging its managed Workload"
                        .into(),
                ));
            }
            Err(error) => return Err(error.into()),
        };
        if application.organization_id != request.organization_id
            || application.project_id != request.project_id
            || application.environment_id != request.environment_id
            || application.id != request.application_id
        {
            return Err(ApplicationError::Internal(
                "Durable Cell application crossed its requested Workload reconciliation scope"
                    .into(),
            ));
        }
        let workload_id =
            DurableCellProjectionIdentity::workload_id_for_application(application.id);
        let control = match self
            .workloads
            .find_workload_control(application.organization_id, workload_id)
            .await
        {
            Ok(value) => value,
            // An application that has never projected a Workload has no
            // compute to stop or restart. A racing deployment invokes this
            // same composition after its Workload transaction, so a later
            // projection cannot escape an already-committed stopped intent.
            Err(RepositoryError::NotFound) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if control.organization_id != application.organization_id
            || control.project_id != application.project_id
            || control.environment_id != application.environment_id
            || control.workload_id != workload_id
        {
            return Err(ApplicationError::Internal(
                "Durable Cell managed Workload control crossed its tenant scope".into(),
            ));
        }
        let owner = control.spec.managed_owner.as_ref().ok_or_else(|| {
            ApplicationError::Internal(
                "Durable Cell deterministic Workload lost its managed owner authority".into(),
            )
        })?;
        if owner.kind().as_str() != DURABLE_CELL_MANAGED_OWNER_KIND
            || owner.owner_id() != application.id.as_uuid()
            || owner.owner_generation() > application.current_revision_number
            || owner.owner_generation() == application.current_revision_number
                && owner.owner_spec_digest() != application.current_definition_digest.as_str()
        {
            return Err(ApplicationError::Internal(
                "Durable Cell managed Workload owner does not belong to the application lineage"
                    .into(),
            ));
        }

        let desired_replicas = match application.desired_state {
            DurableCellApplicationDesiredState::Running => 1,
            DurableCellApplicationDesiredState::Stopped => 0,
        };
        if control.spec.placement_policy.desired_replicas() == desired_replicas {
            return Ok(());
        }
        let replicas = self
            .workloads
            .list_workload_replicas(application.organization_id, workload_id)
            .await
            .map_err(ApplicationError::from)?;
        let latest_replica_at = replicas
            .iter()
            .map(|replica| replica.updated_at)
            .max()
            .ok_or_else(|| {
                ApplicationError::Internal(
                    "Durable Cell managed Workload has no canonical replica".into(),
                )
            })?;

        let canonical = serde_json::to_vec(&CanonicalManagedReplicaIntent {
            organization_id: application.organization_id,
            project_id: application.project_id,
            environment_id: application.environment_id,
            application_id: application.id,
            application_state_version: application.aggregate_version,
            desired_state: application.desired_state.as_str(),
            workload_id,
            owner_kind: owner.kind().as_str(),
            owner_id: owner.owner_id(),
            owner_generation: owner.owner_generation(),
            owner_spec_digest: owner.owner_spec_digest(),
            desired_replicas,
        })
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/durable-cell-applications/{}/managed-workload-replica-set",
                application.organization_id, application.id
            ),
            format!("application-state-{}", application.aggregate_version),
            &canonical,
        )
        .map_err(ApplicationError::Internal)?;
        let correlation_name = format!(
            "a3s-cloud:durable-cell:managed-replica-intent:v1:{}",
            idempotency.request_digest
        );
        let result = self
            .workloads
            .reconfigure_replica_set(ReconfigureReplicaSetWrite {
                organization_id: application.organization_id,
                workload_id,
                expected_control_version: control.aggregate_version,
                expected_policy_generation: control.spec.placement_policy.generation(),
                desired_replicas,
                managed_owner: Some(owner.clone()),
                idempotency,
                correlation_id: Uuid::new_v5(
                    &application.id.as_uuid(),
                    correlation_name.as_bytes(),
                ),
                requested_at: application
                    .updated_at
                    .max(control.updated_at)
                    .max(latest_replica_at),
            })
            .await
            .map_err(ApplicationError::from)?;
        if result.control.organization_id != application.organization_id
            || result.control.project_id != application.project_id
            || result.control.environment_id != application.environment_id
            || result.control.workload_id != workload_id
            || result.control.spec.managed_owner.as_ref() != Some(owner)
            || result.control.spec.placement_policy.desired_replicas() != desired_replicas
        {
            return Err(ApplicationError::Internal(
                "Durable Cell managed Workload replica replay drifted".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalManagedReplicaIntent<'a> {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    application_id: crate::modules::shared_kernel::domain::DurableCellApplicationId,
    application_state_version: u64,
    desired_state: &'a str,
    workload_id: crate::modules::shared_kernel::domain::WorkloadId,
    owner_kind: &'a str,
    owner_id: Uuid,
    owner_generation: u64,
    owner_spec_digest: &'a str,
    desired_replicas: u32,
}
