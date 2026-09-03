use super::managed_replica_lifecycle::converge_current_managed_replicas;
#[cfg(test)]
use super::provider_workload::compose_pinned_celld_service_process;
use super::provider_workload::{
    durable_cell_managed_owner_reference, project_durable_cell_provider_workload,
    validate_durable_cell_provider_workload_binding, validate_pinned_celld_provider_workload,
};
use super::resource_access::{application_not_found, environment, revision_not_found};
use super::storage_port::{DurableCellStorageCredentialRequest, IDurableCellStoragePort};
use crate::modules::data::{
    ObjectNamespaceCredentialBinding, ObjectNamespaceProviderProfile,
    ObjectNamespaceRetentionPolicy,
};
use crate::modules::durable_cells::domain::{
    CreateDurableCellDeploymentWrite, DurableCellApplicationDesiredState,
    DurableCellApplicationRecord, DurableCellDeployment, DurableCellDeploymentRequest,
    DurableCellProjectionIdentity, DurableCellProviderBinding, DurableCellServiceProfile,
    DurableCellStorageBinding, IDurableCellApplicationRepository, IDurableCellDeploymentRepository,
};
use crate::modules::fleet::domain::repositories::INodePoolRepository;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, IdempotencyRequest,
    NodePoolId, OrganizationId, PrincipalId, ProjectId, RepositoryError, ResourceName,
    Sha256Digest,
};
use crate::modules::workloads::application::commands::{
    validate_node_pool_selection, validate_secret_binding_references,
};
use crate::modules::workloads::application::{
    DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION,
};
use crate::modules::workloads::{
    CreateDeploymentBundle, Deployment, DeploymentBundle, DeploymentRequested, IWorkloadRepository,
    ServiceTemplate, Workload, WorkloadControlSpec, WorkloadDesiredState, WorkloadRevision,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DeployDurableCellApplication {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub application_revision_id: DurableCellApplicationRevisionId,
    pub service_profile_acl: String,
    pub storage_provider_profile_acl: Option<String>,
    /// Internal, already-resolved adapter projection. C5 must expose this
    /// through canonical A3S ACL rather than serializing this Rust value.
    pub workload_template: ServiceTemplate,
    pub storage_credentials: ObjectNamespaceCredentialBinding,
    pub retention_policy: ObjectNamespaceRetentionPolicy,
    pub node_pool_id: Option<NodePoolId>,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for DeployDurableCellApplication {
    type Output = ApplicationResult<DurableCellDeploymentMutationResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellDeploymentMutationResult {
    pub correlation: DurableCellDeployment,
    pub workload: DeploymentBundle,
    pub replayed: bool,
}

#[derive(Clone)]
pub struct DeployDurableCellApplicationHandler {
    applications: Arc<dyn IDurableCellApplicationRepository>,
    deployments: Arc<dyn IDurableCellDeploymentRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    storage: Arc<dyn IDurableCellStoragePort>,
    secrets: Arc<dyn ISecretRepository>,
    node_pools: Arc<dyn INodePoolRepository>,
}

impl DeployDurableCellApplicationHandler {
    pub fn new(
        applications: Arc<dyn IDurableCellApplicationRepository>,
        deployments: Arc<dyn IDurableCellDeploymentRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
        storage: Arc<dyn IDurableCellStoragePort>,
        secrets: Arc<dyn ISecretRepository>,
        node_pools: Arc<dyn INodePoolRepository>,
    ) -> Self {
        Self {
            applications,
            deployments,
            workloads,
            storage,
            secrets,
            node_pools,
        }
    }
}

impl CommandHandler<DeployDurableCellApplication> for DeployDurableCellApplicationHandler {
    fn execute(
        &self,
        command: DeployDurableCellApplication,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DurableCellDeploymentMutationResult>>,
    > {
        let applications = Arc::clone(&self.applications);
        let deployments = Arc::clone(&self.deployments);
        let workloads = Arc::clone(&self.workloads);
        let storage = Arc::clone(&self.storage);
        let secrets = Arc::clone(&self.secrets);
        let node_pools = Arc::clone(&self.node_pools);
        Box::pin(async move {
            if let Err(error) = environment(
                command.project_id,
                command.environment_id,
                &command.resource_access,
            ) {
                return Ok(Err(error));
            }
            let prepared = match PreparedDeployment::new(&command) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let idempotency = match prepared.idempotency(&command.idempotency_key) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };

            let (correlation, correlation_replayed) = match deployments.replay(&idempotency).await {
                Ok(Some(correlation)) => {
                    if let Err(error) = prepared.validate_correlation(&correlation) {
                        return Err(BootError::Internal(error));
                    }
                    (correlation, true)
                }
                Ok(None) => {
                    let record = match load_current_record(applications.as_ref(), &command).await {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(error)),
                    };
                    if let Err(error) = admit_external_bindings(
                        storage.as_ref(),
                        &secrets,
                        node_pools.as_ref(),
                        &command,
                    )
                    .await
                    {
                        return Ok(Err(error));
                    }
                    let correlation =
                        match prepare_correlation(workloads.as_ref(), &record, &command, &prepared)
                            .await
                        {
                            Ok(value) => value,
                            Err(error) => return Ok(Err(error)),
                        };
                    match deployments
                        .create(CreateDurableCellDeploymentWrite {
                            deployment: correlation,
                            idempotency: idempotency.clone(),
                        })
                        .await
                    {
                        Ok(write) => (write.value, write.replayed),
                        Err(error) => return Ok(Err(error.into())),
                    }
                }
                Err(error) => return Ok(Err(error.into())),
            };

            let workload_idempotency = match prepared.workload_idempotency(&command.idempotency_key)
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match workloads.replay_deployment(&workload_idempotency).await {
                Ok(Some(bundle)) => {
                    if let Err(error) = validate_workload_bundle(
                        &correlation,
                        &prepared.service_profile,
                        &command.workload_template,
                        &bundle,
                    ) {
                        return Err(BootError::Internal(error));
                    }
                    if let Err(error) = converge_current_managed_replicas(
                        applications.as_ref(),
                        workloads.as_ref(),
                        command.organization_id,
                        command.project_id,
                        command.environment_id,
                        command.application_id,
                    )
                    .await
                    {
                        return Ok(Err(error));
                    }
                    return Ok(Ok(DurableCellDeploymentMutationResult {
                        correlation,
                        workload: bundle,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }

            // A persisted intent may precede its Workload bundle when the
            // process dies. Recheck mutable external admission only while the
            // existing Workloads authority is still absent.
            if let Err(error) = load_current_record(applications.as_ref(), &command).await {
                return Ok(Err(error));
            }
            if let Err(error) =
                admit_external_bindings(storage.as_ref(), &secrets, node_pools.as_ref(), &command)
                    .await
            {
                return Ok(Err(error));
            }
            let bundle = match create_managed_workload(
                workloads.as_ref(),
                &command,
                &prepared,
                &correlation,
                workload_idempotency,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if let Err(error) = converge_current_managed_replicas(
                applications.as_ref(),
                workloads.as_ref(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                command.application_id,
            )
            .await
            {
                return Ok(Err(error));
            }
            Ok(Ok(DurableCellDeploymentMutationResult {
                correlation,
                workload: bundle,
                replayed: correlation_replayed,
            }))
        })
    }
}

struct PreparedDeployment {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    application_revision_id: DurableCellApplicationRevisionId,
    service_profile: DurableCellServiceProfile,
    storage_provider_profile: Option<ObjectNamespaceProviderProfile>,
    service_template_digest: Sha256Digest,
    provider_artifact_digest: Sha256Digest,
    credential_binding_digest: Sha256Digest,
    storage_provider_profile_digest: Sha256Digest,
    retention_policy_digest: Sha256Digest,
    node_pool_id: Option<NodePoolId>,
    canonical_request: Vec<u8>,
}

impl PreparedDeployment {
    fn new(command: &DeployDurableCellApplication) -> Result<Self, String> {
        let service_profile = DurableCellServiceProfile::parse_acl(&command.service_profile_acl)?;
        let storage_provider_profile = command
            .storage_provider_profile_acl
            .as_deref()
            .map(ObjectNamespaceProviderProfile::parse_acl)
            .transpose()?;
        command.workload_template.validate()?;
        command.storage_credentials.validate()?;
        command.retention_policy.validate()?;
        if storage_provider_profile.as_ref().is_some_and(|profile| {
            profile.digest() != &command.storage_credentials.spec().provider_profile_digest
        }) {
            return Err(
                "Durable Cell deployment S0 profile and credential binding digests differ".into(),
            );
        }
        if let Some(provider_profile) = &storage_provider_profile {
            let publisher = crate::modules::durable_cells::domain::DurableCellPublisherProfile::pinned_celld_v0_2_1()?;
            validate_pinned_celld_provider_workload(
                &command.storage_credentials,
                provider_profile,
                &service_profile,
                &command.workload_template,
                &publisher,
            )?;
        }
        let service_template_digest = Sha256Digest::parse(command.workload_template.digest()?)?;
        let provider_artifact_digest =
            Sha256Digest::parse(&command.workload_template.artifact.digest)?;
        let canonical_request = serde_json::to_vec(&CanonicalDeploymentRequest {
            organization_id: command.organization_id,
            project_id: command.project_id,
            environment_id: command.environment_id,
            application_id: command.application_id,
            application_revision_id: command.application_revision_id,
            service_profile_digest: service_profile.digest().as_str(),
            service_template_digest: service_template_digest.as_str(),
            credential_binding_digest: command.storage_credentials.digest().as_str(),
            retention_policy_digest: command.retention_policy.digest().as_str(),
            node_pool_id: command.node_pool_id,
        })
        .map_err(|error| error.to_string())?;
        Ok(Self {
            organization_id: command.organization_id,
            project_id: command.project_id,
            environment_id: command.environment_id,
            application_id: command.application_id,
            application_revision_id: command.application_revision_id,
            service_profile,
            storage_provider_profile,
            service_template_digest,
            provider_artifact_digest,
            credential_binding_digest: command.storage_credentials.digest().clone(),
            storage_provider_profile_digest: command
                .storage_credentials
                .spec()
                .provider_profile_digest
                .clone(),
            retention_policy_digest: command.retention_policy.digest().clone(),
            node_pool_id: command.node_pool_id,
            canonical_request,
        })
    }

    fn idempotency(&self, key: &str) -> Result<IdempotencyRequest, String> {
        IdempotencyRequest::new(
            format!(
                "organizations/{}/durable-cell-applications/{}/revisions/{}/deployment-correlation",
                self.organization_id, self.application_id, self.application_revision_id,
            ),
            key,
            &self.canonical_request,
        )
    }

    fn workload_idempotency(&self, key: &str) -> Result<IdempotencyRequest, String> {
        IdempotencyRequest::new(
            format!(
                "organizations/{}/durable-cell-applications/{}/revisions/{}/managed-workload-deployment",
                self.organization_id,
                self.application_id,
                self.application_revision_id,
            ),
            key,
            &self.canonical_request,
        )
    }

    fn validate_correlation(&self, correlation: &DurableCellDeployment) -> Result<(), String> {
        correlation.validate()?;
        let projection = &correlation.projection;
        if projection.organization_id != self.organization_id
            || projection.project_id != self.project_id
            || projection.environment_id != self.environment_id
            || projection.application_id != self.application_id
            || projection.application_revision_id != self.application_revision_id
            || correlation.storage.credential_binding_digest != self.credential_binding_digest
            || correlation.storage.provider_profile_digest != self.storage_provider_profile_digest
            || correlation.storage_provider_profile()? != self.storage_provider_profile
            || correlation.storage.retention_policy_digest != self.retention_policy_digest
            || correlation.provider.service_profile_digest != *self.service_profile.digest()
            || correlation.provider.service_template_digest != self.service_template_digest
            || correlation.provider.provider_artifact_digest != self.provider_artifact_digest
        {
            return Err("Durable Cell deployment replay changed its exact projection".into());
        }
        let control = WorkloadControlSpec::managed_replica_set_in_pool(
            durable_cell_managed_owner_reference(projection)?,
            correlation.provider.workload_generation,
            1,
            self.node_pool_id,
        )?;
        if control.placement_policy.digest() != correlation.placement_policy_digest.as_str() {
            return Err("Durable Cell deployment replay changed its placement projection".into());
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalDeploymentRequest<'a> {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
    application_revision_id: DurableCellApplicationRevisionId,
    service_profile_digest: &'a str,
    service_template_digest: &'a str,
    credential_binding_digest: &'a str,
    retention_policy_digest: &'a str,
    node_pool_id: Option<NodePoolId>,
}

async fn load_current_record(
    applications: &dyn IDurableCellApplicationRepository,
    command: &DeployDurableCellApplication,
) -> ApplicationResult<DurableCellApplicationRecord> {
    let application = match applications
        .find(
            command.organization_id,
            command.project_id,
            command.environment_id,
            command.application_id,
        )
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => return Err(application_not_found()),
        Err(error) => return Err(error.into()),
    };
    if application.current_revision_id != command.application_revision_id {
        return Err(ApplicationError::Conflict(
            "Durable Cell deployment requires the exact current application revision".into(),
        ));
    }
    if application.desired_state != DurableCellApplicationDesiredState::Running {
        return Err(ApplicationError::Conflict(
            "stopped Durable Cell application cannot request a deployment".into(),
        ));
    }
    let revision = match applications
        .find_revision(
            command.organization_id,
            command.project_id,
            command.environment_id,
            command.application_id,
            command.application_revision_id,
        )
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => return Err(revision_not_found()),
        Err(error) => return Err(error.into()),
    };
    DurableCellApplicationRecord::new(application, revision).map_err(ApplicationError::Internal)
}

async fn admit_external_bindings(
    storage: &dyn IDurableCellStoragePort,
    secrets: &Arc<dyn ISecretRepository>,
    node_pools: &dyn INodePoolRepository,
    command: &DeployDurableCellApplication,
) -> ApplicationResult<()> {
    let storage_request = storage_credential_request(&command.storage_credentials)?;
    storage.require_active_credentials(&storage_request).await?;
    validate_secret_binding_references(
        secrets.as_ref(),
        command.organization_id,
        command.project_id,
        command.environment_id,
        &command.workload_template.secrets,
    )
    .await?;
    require_storage_credentials_in_template(
        &command.storage_credentials,
        &command.workload_template,
    )?;
    validate_node_pool_selection(node_pools, command.organization_id, command.node_pool_id).await?;
    Ok(())
}

fn storage_credential_request(
    credentials: &ObjectNamespaceCredentialBinding,
) -> ApplicationResult<DurableCellStorageCredentialRequest> {
    credentials.validate().map_err(ApplicationError::Internal)?;
    let spec = credentials.spec();
    let request = DurableCellStorageCredentialRequest::new(
        spec.organization_id,
        spec.project_id,
        spec.environment_id,
        spec.namespace_id,
        spec.generation,
        spec.provider_profile_digest.clone(),
        spec.access_key_id,
        spec.secret_access_key,
        spec.session_token,
    )
    .map_err(ApplicationError::Internal)?;
    if request.binding_digest != *credentials.digest() {
        return Err(ApplicationError::Internal(
            "Durable Cell S0 credential digest changed at the storage boundary".into(),
        ));
    }
    Ok(request)
}

async fn prepare_correlation(
    workloads: &dyn IWorkloadRepository,
    record: &DurableCellApplicationRecord,
    command: &DeployDurableCellApplication,
    prepared: &PreparedDeployment,
) -> ApplicationResult<DurableCellDeployment> {
    record
        .revision
        .definition
        .validate_service_profile(&prepared.service_profile)
        .map_err(ApplicationError::Invalid)?;
    let projection =
        DurableCellProjectionIdentity::for_current_revision(&record.application, &record.revision)
            .map_err(ApplicationError::Internal)?;
    let workload_generation = next_workload_generation(
        workloads,
        projection.organization_id,
        projection.workload_id,
        projection.workload_revision_id,
        &command.workload_template,
    )
    .await?;
    let workload_revision = WorkloadRevision::create(
        projection.workload_revision_id,
        projection.workload_id,
        workload_generation,
        command.workload_template.clone(),
        Utc::now(),
    )
    .map_err(ApplicationError::Invalid)?;
    let provider_workload = project_durable_cell_provider_workload(&workload_revision)
        .map_err(ApplicationError::Invalid)?;
    let provider = DurableCellProviderBinding::for_current_revision(
        &record.application,
        &record.revision,
        &projection,
        &prepared.service_profile,
        &provider_workload,
    )
    .map_err(ApplicationError::Invalid)?;
    let storage = DurableCellStorageBinding::for_current_revision(
        &record.application,
        &record.revision,
        &projection,
        &command.storage_credentials,
        &command.retention_policy,
    )
    .map_err(ApplicationError::Invalid)?;
    let control = managed_control(&projection, workload_generation, command.node_pool_id)?;
    DurableCellDeployment::bind(
        projection,
        storage,
        prepared.storage_provider_profile.as_ref(),
        provider,
        Sha256Digest::parse(control.placement_policy.digest())
            .map_err(ApplicationError::Internal)?,
        DurableCellDeploymentRequest {
            requested_by: command.actor_principal_id,
            request_id: command.request_id,
            requested_at: Utc::now(),
        },
    )
    .map_err(ApplicationError::Internal)
}

async fn next_workload_generation(
    workloads: &dyn IWorkloadRepository,
    organization_id: OrganizationId,
    workload_id: crate::modules::shared_kernel::domain::WorkloadId,
    workload_revision_id: crate::modules::shared_kernel::domain::WorkloadRevisionId,
    template: &ServiceTemplate,
) -> ApplicationResult<u64> {
    let revisions = match workloads.list_revisions(organization_id, workload_id).await {
        Ok(revisions) => revisions,
        Err(RepositoryError::NotFound) => Vec::new(),
        Err(error) => return Err(ApplicationError::from(error)),
    };
    if let Some(existing) = revisions
        .iter()
        .find(|revision| revision.id == workload_revision_id)
    {
        if existing.resolved_template().ok() != Some(template) {
            return Err(ApplicationError::Conflict(
                "Durable Cell Workload revision identity already has another template".into(),
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

async fn create_managed_workload(
    workloads: &dyn IWorkloadRepository,
    command: &DeployDurableCellApplication,
    prepared: &PreparedDeployment,
    correlation: &DurableCellDeployment,
    idempotency: IdempotencyRequest,
) -> ApplicationResult<DeploymentBundle> {
    let projection = &correlation.projection;
    let workload = match workloads
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
            correlation.requested_at,
        ),
        Err(error) => return Err(error.into()),
    };
    let requested_at = std::cmp::max(Utc::now(), workload.updated_at);
    let revision = WorkloadRevision::create(
        projection.workload_revision_id,
        projection.workload_id,
        correlation.provider.workload_generation,
        command.workload_template.clone(),
        requested_at,
    )
    .map_err(ApplicationError::Invalid)?;
    validate_durable_cell_provider_workload_binding(
        &correlation.provider,
        &prepared.service_profile,
        &revision,
    )
    .map_err(ApplicationError::Invalid)?;
    let control = managed_control(
        projection,
        correlation.provider.workload_generation,
        command.node_pool_id,
    )?;
    if control.placement_policy.digest() != correlation.placement_policy_digest.as_str() {
        return Err(ApplicationError::Conflict(
            "Durable Cell placement projection changed after admission".into(),
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
    let event = DeploymentRequested::envelope(&deployment, &revision, correlation.request_id)
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
    let bundle = workloads
        .create_deployment(CreateDeploymentBundle {
            workload,
            control,
            revision,
            deployment,
            operation,
            idempotency,
            event,
        })
        .await
        .map_err(ApplicationError::from)?;
    validate_workload_bundle(
        correlation,
        &prepared.service_profile,
        &command.workload_template,
        &bundle,
    )
    .map_err(ApplicationError::Internal)?;
    Ok(bundle)
}

fn validate_workload_bundle(
    correlation: &DurableCellDeployment,
    service_profile: &DurableCellServiceProfile,
    template: &ServiceTemplate,
    bundle: &DeploymentBundle,
) -> Result<(), String> {
    let projection = &correlation.projection;
    if bundle.workload.id != projection.workload_id
        || bundle.workload.organization_id != projection.organization_id
        || bundle.workload.project_id != projection.project_id
        || bundle.workload.environment_id != projection.environment_id
        || bundle.revision.id != projection.workload_revision_id
        || bundle.revision.workload_id != projection.workload_id
        || bundle.revision.generation != correlation.provider.workload_generation
        || bundle.revision.resolved_template()? != template
        || bundle.deployment.id != projection.deployment_id
        || bundle.deployment.organization_id != projection.organization_id
        || bundle.deployment.workload_id != projection.workload_id
        || bundle.deployment.revision_id != projection.workload_revision_id
        || bundle.deployment.operation_id != projection.operation_id
        || bundle.operation.id != projection.operation_id
        || bundle.operation.organization_id != projection.organization_id
    {
        return Err("Durable Cell managed Workload replay drifted".into());
    }
    validate_durable_cell_provider_workload_binding(
        &correlation.provider,
        service_profile,
        &bundle.revision,
    )
}

fn validate_existing_workload(
    workload: &Workload,
    projection: &DurableCellProjectionIdentity,
) -> ApplicationResult<()> {
    if workload.id != projection.workload_id
        || workload.organization_id != projection.organization_id
        || workload.project_id != projection.project_id
        || workload.environment_id != projection.environment_id
        || workload.name != managed_workload_name(projection.application_id)?
        || workload.desired_state != WorkloadDesiredState::Running
    {
        return Err(ApplicationError::Conflict(
            "Durable Cell managed Workload identity or desired state drifted".into(),
        ));
    }
    Ok(())
}

fn managed_control(
    projection: &DurableCellProjectionIdentity,
    generation: u64,
    node_pool_id: Option<NodePoolId>,
) -> ApplicationResult<WorkloadControlSpec> {
    WorkloadControlSpec::managed_replica_set_in_pool(
        durable_cell_managed_owner_reference(projection).map_err(ApplicationError::Internal)?,
        generation,
        1,
        node_pool_id,
    )
    .map_err(ApplicationError::Invalid)
}

fn managed_workload_name(
    application_id: DurableCellApplicationId,
) -> ApplicationResult<ResourceName> {
    ResourceName::parse(format!(
        "durable-cell-{}",
        application_id.as_uuid().simple()
    ))
    .map_err(ApplicationError::Internal)
}

fn require_storage_credentials_in_template(
    credentials: &ObjectNamespaceCredentialBinding,
    template: &ServiceTemplate,
) -> ApplicationResult<()> {
    if credentials.spec().references().iter().any(|reference| {
        !template.secrets.iter().any(|binding| {
            binding.secret_id == reference.secret_id && binding.version == reference.version
        })
    }) {
        return Err(ApplicationError::Invalid(
            "Durable Cell provider template omitted an exact S0 credential binding".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
