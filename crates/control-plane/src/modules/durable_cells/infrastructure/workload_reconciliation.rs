use crate::modules::durable_cells::application::{
    project_durable_cell_provider_workload, DurableCellWorkloadDeployment,
    DurableCellWorkloadDeploymentRequest, DurableCellWorkloadDeploymentStatus,
    DurableCellWorkloadPrestartProjection, DurableCellWorkloadPrestartRequest,
    DurableCellWorkloadReconciliationRequest, DurableCellWorkloadRevisionGenerationRequest,
    DurableCellWorkloadTemplate, DurableCellWorkloadWriterFenceProjection,
    DurableCellWorkloadWriterFenceRequest, IDurableCellWorkloadPort,
};
use crate::modules::durable_cells::domain::{
    DurableCellApplicationDesiredState, DurableCellProjectionIdentity,
    IDurableCellApplicationRepository, DURABLE_CELL_MANAGED_OWNER_KIND,
};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, RepositoryError, ResourceName, Sha256Digest,
};
use crate::modules::workloads::application::project_runtime_secrets;
use crate::modules::workloads::{
    CreateDeploymentBundle, Deployment, DeploymentRequested, DeploymentStatus, IWorkloadRepository,
    ManagedOwnerKind, ManagedOwnerReference, ReconfigureReplicaSetWrite, ServiceTemplate, Workload,
    WorkloadControlSpec, WorkloadDesiredState, WorkloadReplica, WorkloadRevision,
};
use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::modules::workloads::application::{
    DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION,
};

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

    async fn load_prestart_publication_projection(
        &self,
        request: &DurableCellWorkloadPrestartRequest,
    ) -> ApplicationResult<DurableCellWorkloadPrestartProjection> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let deployment = self
            .workloads
            .find_deployment(request.organization_id, request.deployment_id)
            .await
            .map_err(ApplicationError::from)?;
        if deployment.id != request.deployment_id
            || deployment.organization_id != request.organization_id
            || deployment.workload_id != request.workload_id
            || deployment.revision_id != request.workload_revision_id
            || deployment.operation_id != request.operation_id
            || deployment.node_id != Some(request.node_id)
        {
            return Err(ApplicationError::Conflict(
                "Durable Cell pre-start request changed its Workload Deployment".into(),
            ));
        }

        let control = self
            .workloads
            .find_workload_control(request.organization_id, request.workload_id)
            .await
            .map_err(ApplicationError::from)?;
        let expected_owner = ManagedOwnerReference::new(
            ManagedOwnerKind::parse(DURABLE_CELL_MANAGED_OWNER_KIND)
                .map_err(ApplicationError::Internal)?,
            request.application_id.as_uuid(),
            request.application_revision_number,
            request.application_definition_digest.as_str(),
        )
        .map_err(ApplicationError::Internal)?;
        if control.organization_id != request.organization_id
            || control.project_id != request.project_id
            || control.environment_id != request.environment_id
            || control.workload_id != request.workload_id
            || control.spec.managed_owner.as_ref() != Some(&expected_owner)
            || control.spec.placement_policy.members_per_replica() != 1
        {
            return Err(ApplicationError::Conflict(
                "Durable Cell pre-start request changed its managed Workload control".into(),
            ));
        }

        let binding = self
            .workloads
            .find_deployment_replica_binding(request.organization_id, request.deployment_id)
            .await
            .map_err(ApplicationError::from)?;
        let canonical_replica_id = WorkloadReplica::deterministic_id(request.workload_id, 0)
            .map_err(ApplicationError::Invalid)?;
        let replica = self
            .workloads
            .find_workload_replica(
                request.organization_id,
                request.workload_id,
                canonical_replica_id,
            )
            .await
            .map_err(ApplicationError::from)?;
        if binding.deployment_id != request.deployment_id
            || binding.organization_id != request.organization_id
            || binding.project_id != request.project_id
            || binding.environment_id != request.environment_id
            || binding.workload_id != request.workload_id
            || binding.revision_id != request.workload_revision_id
            || binding.replica_id != canonical_replica_id
            || binding.replica_generation == 0
            || binding.runtime_generation != binding.replica_generation
            || binding.node_id != Some(request.node_id)
            || replica.id != binding.replica_id
            || replica.ordinal != 0
            || replica.revision_id != binding.revision_id
            || replica.revision_generation != request.workload_generation
            || replica.generation != binding.replica_generation
        {
            return Err(ApplicationError::Conflict(
                "Durable Cell pre-start request changed its canonical writer binding".into(),
            ));
        }

        let revision = self
            .workloads
            .find_revision(request.organization_id, request.workload_revision_id)
            .await
            .map_err(ApplicationError::from)?;
        if revision.id != request.workload_revision_id
            || revision.workload_id != request.workload_id
            || revision.generation != request.workload_generation
            || revision.external_build.is_some()
            || !revision.skill_bindings().is_empty()
        {
            return Err(ApplicationError::Conflict(
                "Durable Cell pre-start request changed its Workload revision".into(),
            ));
        }
        let template = revision
            .resolved_template()
            .map_err(ApplicationError::Internal)?;
        template.validate().map_err(ApplicationError::Internal)?;
        let template_digest = Sha256Digest::parse(
            template
                .digest()
                .map_err(ApplicationError::Internal)?
                .as_str(),
        )
        .map_err(ApplicationError::Internal)?;
        let template_bytes = serde_json::to_vec(template)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let service_template = DurableCellWorkloadTemplate::new(template_bytes, template_digest)
            .map_err(ApplicationError::Internal)?;
        let provider_workload = project_durable_cell_provider_workload(&revision)
            .map_err(ApplicationError::Internal)?;
        let runtime_secrets =
            project_runtime_secrets(&revision).map_err(ApplicationError::Internal)?;
        let projection = DurableCellWorkloadPrestartProjection {
            deployment_id: request.deployment_id,
            operation_id: request.operation_id,
            workload_id: request.workload_id,
            workload_revision_id: request.workload_revision_id,
            node_id: request.node_id,
            writer_epoch: binding.replica_generation,
            provider_workload,
            service_template,
            runtime_secrets,
        };
        projection
            .validate_against(request)
            .map_err(ApplicationError::Internal)?;
        Ok(projection)
    }

    async fn load_writer_fence_admission_projection(
        &self,
        request: &DurableCellWorkloadWriterFenceRequest,
    ) -> ApplicationResult<Option<DurableCellWorkloadWriterFenceProjection>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let control = self
            .workloads
            .find_workload_control(request.organization_id, request.workload_id)
            .await
            .map_err(ApplicationError::from)?;
        if control.organization_id != request.organization_id
            || control.project_id != request.project_id
            || control.environment_id != request.environment_id
            || control.workload_id != request.workload_id
        {
            return Err(ApplicationError::Internal(
                "Durable Cell writer-fence Workload control crossed its tenant scope".into(),
            ));
        }
        let expected_owner = ManagedOwnerReference::new(
            ManagedOwnerKind::parse(DURABLE_CELL_MANAGED_OWNER_KIND)
                .map_err(ApplicationError::Internal)?,
            request.application_id.as_uuid(),
            request.application_revision_number,
            request.application_definition_digest.as_str(),
        )
        .map_err(ApplicationError::Internal)?;
        if control.spec.placement_policy.desired_replicas() != 0
            || control.spec.placement_policy.members_per_replica() != 1
            || control.spec.managed_owner.as_ref() != Some(&expected_owner)
        {
            return Ok(None);
        }
        let projection = DurableCellWorkloadWriterFenceProjection {
            workload_id: request.workload_id,
            workload_revision_id: request.workload_revision_id,
            workload_generation: request.workload_generation,
            replica_id: request.replica_id,
            replica_generation: request.replica_generation,
            replica_ordinal: request.replica_ordinal,
        };
        projection
            .validate_against(request)
            .map_err(ApplicationError::Internal)?;
        Ok(Some(projection))
    }

    async fn validate_control(
        &self,
        request: &DurableCellWorkloadDeploymentRequest,
        exact: bool,
    ) -> ApplicationResult<()> {
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
                    "Durable Cell application disappeared while validating its managed Workload"
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
                "Durable Cell application crossed its managed Workload validation scope".into(),
            ));
        }
        let control = self
            .workloads
            .find_workload_control(request.organization_id, request.workload_id)
            .await
            .map_err(ApplicationError::from)?;
        if control.organization_id != request.organization_id
            || control.project_id != request.project_id
            || control.environment_id != request.environment_id
            || control.workload_id != request.workload_id
            || exact
                && control.spec.placement_policy.digest()
                    != request.placement_policy_digest.as_str()
        {
            return Err(ApplicationError::Internal(
                "Durable Cell managed Workload control projection drifted".into(),
            ));
        }
        let owner = control.spec.managed_owner.as_ref().ok_or_else(|| {
            ApplicationError::Internal(
                "Durable Cell managed Workload lost its owner projection".into(),
            )
        })?;
        if owner.kind().as_str() != DURABLE_CELL_MANAGED_OWNER_KIND
            || owner.owner_id() != request.application_id.as_uuid()
            || owner.owner_generation() > application.current_revision_number
            || owner.owner_generation() == application.current_revision_number
                && owner.owner_spec_digest() != application.current_definition_digest.as_str()
            || exact
                && (owner.owner_generation() != request.application_revision_number
                    || owner.owner_spec_digest() != request.application_definition_digest.as_str())
        {
            return Err(ApplicationError::Internal(
                "Durable Cell managed Workload owner projection drifted".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl IDurableCellWorkloadPort for WorkloadsDurableCellWorkloadAdapter {
    async fn load_prestart_publication(
        &self,
        request: &DurableCellWorkloadPrestartRequest,
    ) -> ApplicationResult<DurableCellWorkloadPrestartProjection> {
        self.load_prestart_publication_projection(request).await
    }

    async fn load_writer_fence_admission(
        &self,
        request: &DurableCellWorkloadWriterFenceRequest,
    ) -> ApplicationResult<Option<DurableCellWorkloadWriterFenceProjection>> {
        self.load_writer_fence_admission_projection(request).await
    }

    async fn replay_managed_deployment(
        &self,
        request: &DurableCellWorkloadDeploymentRequest,
    ) -> ApplicationResult<Option<DurableCellWorkloadDeployment>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let template = decode_template(request)?;
        let bundle = match self.workloads.replay_deployment(&request.idempotency).await {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let Some(bundle) = bundle else {
            return Ok(None);
        };
        validate_bundle(request, &template, &bundle).map_err(ApplicationError::Internal)?;
        self.validate_control(request, false).await?;
        Ok(Some(project_bundle(bundle)?))
    }

    async fn create_managed_deployment(
        &self,
        request: &DurableCellWorkloadDeploymentRequest,
    ) -> ApplicationResult<DurableCellWorkloadDeployment> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let template = decode_template(request)?;
        let projection = request;
        let workload = match self
            .workloads
            .find_workload(projection.organization_id, projection.workload_id)
            .await
        {
            Ok(value) => {
                validate_existing_workload(&value, projection)?;
                value
            }
            Err(RepositoryError::NotFound) => Workload::create(
                projection.workload_id,
                projection.organization_id,
                projection.project_id,
                projection.environment_id,
                managed_workload_name(projection.application_id)?,
                projection.requested_at,
            ),
            Err(error) => return Err(error.into()),
        };
        let requested_at =
            std::cmp::max(Utc::now(), workload.updated_at).max(projection.requested_at);
        let revision = WorkloadRevision::create(
            projection.workload_revision_id,
            projection.workload_id,
            projection.workload_generation,
            template.clone(),
            requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let control = managed_control(projection)?;
        if control.placement_policy.digest() != projection.placement_policy_digest.as_str() {
            return Err(ApplicationError::Conflict(
                "Durable Cell placement projection changed before Workloads creation".into(),
            ));
        }
        let deployment = Deployment::create(
            projection.deployment_id,
            projection.organization_id,
            projection.workload_id,
            projection.workload_revision_id,
            projection.operation_id,
            requested_at,
        );
        let operation = OperationRequest::new(
            projection.operation_id,
            projection.organization_id,
            OperationSubject::new("deployment", projection.deployment_id.as_uuid())
                .map_err(ApplicationError::Internal)?,
            WorkflowIdentity::new(DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION)
                .map_err(ApplicationError::Internal)?,
            serde_json::json!({
                "deploymentId": projection.deployment_id,
                "organizationId": projection.organization_id,
                "revisionId": projection.workload_revision_id,
                "workloadId": projection.workload_id,
            }),
            requested_at,
        );
        let event = DeploymentRequested::envelope(&deployment, &revision, projection.request_id)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let bundle = self
            .workloads
            .create_deployment(CreateDeploymentBundle {
                workload,
                control,
                revision,
                deployment,
                operation,
                idempotency: projection.idempotency.clone(),
                event,
            })
            .await
            .map_err(ApplicationError::from)?;
        validate_bundle(projection, &template, &bundle).map_err(ApplicationError::Internal)?;
        self.validate_control(projection, !bundle.replayed).await?;
        project_bundle(bundle)
    }

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

fn decode_template(
    request: &DurableCellWorkloadDeploymentRequest,
) -> ApplicationResult<ServiceTemplate> {
    let template = serde_json::from_slice::<ServiceTemplate>(request.service_template.bytes())
        .map_err(|error| {
            ApplicationError::Internal(format!(
                "Workloads template could not be decoded at its owner boundary: {error}"
            ))
        })?;
    template.validate().map_err(|error| {
        ApplicationError::Invalid(format!("invalid Workloads template: {error}"))
    })?;
    let template_digest =
        Sha256Digest::parse(template.digest().map_err(ApplicationError::Internal)?)
            .map_err(ApplicationError::Internal)?;
    if template_digest != *request.service_template.digest() {
        return Err(ApplicationError::Conflict(
            "Durable Cell Workloads template digest changed at the owner boundary".into(),
        ));
    }
    let artifact_digest =
        Sha256Digest::parse(&template.artifact.digest).map_err(ApplicationError::Internal)?;
    if artifact_digest != request.provider_artifact_digest {
        return Err(ApplicationError::Conflict(
            "Durable Cell Workloads artifact digest changed at the owner boundary".into(),
        ));
    }
    Ok(template)
}

fn validate_existing_workload(
    workload: &Workload,
    request: &DurableCellWorkloadDeploymentRequest,
) -> ApplicationResult<()> {
    if workload.id != request.workload_id
        || workload.organization_id != request.organization_id
        || workload.project_id != request.project_id
        || workload.environment_id != request.environment_id
        || workload.name != managed_workload_name(request.application_id)?
        || workload.desired_state != WorkloadDesiredState::Running
    {
        return Err(ApplicationError::Conflict(
            "Durable Cell managed Workload identity or desired state drifted".into(),
        ));
    }
    Ok(())
}

fn managed_workload_name(
    application_id: crate::modules::shared_kernel::domain::DurableCellApplicationId,
) -> ApplicationResult<ResourceName> {
    ResourceName::parse(format!(
        "durable-cell-{}",
        application_id.as_uuid().simple()
    ))
    .map_err(ApplicationError::Internal)
}

fn managed_control(
    request: &DurableCellWorkloadDeploymentRequest,
) -> ApplicationResult<WorkloadControlSpec> {
    let owner = ManagedOwnerReference::new(
        ManagedOwnerKind::parse(DURABLE_CELL_MANAGED_OWNER_KIND)
            .map_err(ApplicationError::Internal)?,
        request.application_id.as_uuid(),
        request.application_revision_number,
        request.application_definition_digest.as_str(),
    )
    .map_err(ApplicationError::Internal)?;
    WorkloadControlSpec::managed_replica_set_in_pool(
        owner,
        request.workload_generation,
        1,
        request.node_pool_id,
    )
    .map_err(ApplicationError::Invalid)
}

fn validate_bundle(
    request: &DurableCellWorkloadDeploymentRequest,
    template: &ServiceTemplate,
    bundle: &crate::modules::workloads::DeploymentBundle,
) -> Result<(), String> {
    if bundle.workload.id != request.workload_id
        || bundle.workload.organization_id != request.organization_id
        || bundle.workload.project_id != request.project_id
        || bundle.workload.environment_id != request.environment_id
        || bundle.revision.id != request.workload_revision_id
        || bundle.revision.workload_id != request.workload_id
        || bundle.revision.generation != request.workload_generation
        || bundle.revision.resolved_template()? != template
        || bundle.deployment.id != request.deployment_id
        || bundle.deployment.organization_id != request.organization_id
        || bundle.deployment.workload_id != request.workload_id
        || bundle.deployment.revision_id != request.workload_revision_id
        || bundle.deployment.operation_id != request.operation_id
        || bundle.operation.id != request.operation_id
        || bundle.operation.organization_id != request.organization_id
        || bundle.operation.requested_at != bundle.deployment.requested_at
        || bundle.revision.external_build.is_some()
        || !bundle.revision.skill_bindings().is_empty()
    {
        return Err("Durable Cell managed Workload replay drifted".into());
    }
    Ok(())
}

fn project_bundle(
    bundle: crate::modules::workloads::DeploymentBundle,
) -> ApplicationResult<DurableCellWorkloadDeployment> {
    let revision = &bundle.revision;
    let template = revision
        .resolved_template()
        .map_err(ApplicationError::Internal)?;
    let expected_artifact_digest = revision
        .request
        .artifact
        .expected_digest
        .as_deref()
        .map(|digest| Sha256Digest::parse(digest).map_err(ApplicationError::Internal))
        .transpose()?;
    let artifact_digest =
        Sha256Digest::parse(&template.artifact.digest).map_err(ApplicationError::Internal)?;
    let template_digest =
        Sha256Digest::parse(revision.template_digest.as_deref().ok_or_else(|| {
            ApplicationError::Internal(
                "resolved Workloads revision lost its template digest".into(),
            )
        })?)
        .map_err(ApplicationError::Internal)?;
    let request_digest =
        Sha256Digest::parse(&revision.request_digest).map_err(ApplicationError::Internal)?;
    let result = DurableCellWorkloadDeployment {
        organization_id: bundle.workload.organization_id,
        project_id: bundle.workload.project_id,
        environment_id: bundle.workload.environment_id,
        workload_id: bundle.workload.id,
        revision_id: revision.id,
        deployment_id: bundle.deployment.id,
        operation_id: bundle.operation.id,
        generation: revision.generation,
        status: project_status(bundle.deployment.status),
        deployment_aggregate_version: bundle.deployment.aggregate_version,
        artifact_source_uri: revision.request.artifact.uri.clone(),
        expected_artifact_digest,
        request_digest,
        artifact_digest: Some(artifact_digest),
        template_digest: Some(template_digest),
        requested_at: bundle.deployment.requested_at,
        replayed: bundle.replayed,
    };
    result.validate().map_err(ApplicationError::Internal)?;
    Ok(result)
}

const fn project_status(status: DeploymentStatus) -> DurableCellWorkloadDeploymentStatus {
    match status {
        DeploymentStatus::Queued => DurableCellWorkloadDeploymentStatus::Queued,
        DeploymentStatus::Resolving => DurableCellWorkloadDeploymentStatus::Resolving,
        DeploymentStatus::Scheduled => DurableCellWorkloadDeploymentStatus::Scheduled,
        DeploymentStatus::Applying => DurableCellWorkloadDeploymentStatus::Applying,
        DeploymentStatus::Verifying => DurableCellWorkloadDeploymentStatus::Verifying,
        DeploymentStatus::Retiring => DurableCellWorkloadDeploymentStatus::Retiring,
        DeploymentStatus::Cancelling => DurableCellWorkloadDeploymentStatus::Cancelling,
        DeploymentStatus::CleanupPending => DurableCellWorkloadDeploymentStatus::CleanupPending,
        DeploymentStatus::Active => DurableCellWorkloadDeploymentStatus::Active,
        DeploymentStatus::Failed => DurableCellWorkloadDeploymentStatus::Failed,
        DeploymentStatus::Orphaned => DurableCellWorkloadDeploymentStatus::Orphaned,
        DeploymentStatus::Cancelled => DurableCellWorkloadDeploymentStatus::Cancelled,
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
