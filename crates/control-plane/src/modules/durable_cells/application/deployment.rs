use super::managed_replica_lifecycle::converge_current_managed_replicas;
use super::resource_access::{application_not_found, environment, revision_not_found};
use crate::modules::data::{
    ObjectNamespaceCredentialAdmission, ObjectNamespaceCredentialBinding,
    ObjectNamespaceProviderProfile, ObjectNamespaceRetentionPolicy,
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
    secrets: Arc<dyn ISecretRepository>,
    node_pools: Arc<dyn INodePoolRepository>,
}

impl DeployDurableCellApplicationHandler {
    pub fn new(
        applications: Arc<dyn IDurableCellApplicationRepository>,
        deployments: Arc<dyn IDurableCellDeploymentRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
        secrets: Arc<dyn ISecretRepository>,
        node_pools: Arc<dyn INodePoolRepository>,
    ) -> Self {
        Self {
            applications,
            deployments,
            workloads,
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
                    if let Err(error) =
                        admit_external_bindings(&secrets, node_pools.as_ref(), &command).await
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
                admit_external_bindings(&secrets, node_pools.as_ref(), &command).await
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
            projection.managed_owner_reference()?,
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
    secrets: &Arc<dyn ISecretRepository>,
    node_pools: &dyn INodePoolRepository,
    command: &DeployDurableCellApplication,
) -> ApplicationResult<()> {
    ObjectNamespaceCredentialAdmission::new(Arc::clone(secrets))
        .require_active(&command.storage_credentials)
        .await?;
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
    let provider = DurableCellProviderBinding::for_current_revision(
        &record.application,
        &record.revision,
        &projection,
        &prepared.service_profile,
        &workload_revision,
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
    correlation
        .provider
        .validate_workload_revision(&prepared.service_profile, &revision)
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
    correlation
        .provider
        .validate_workload_revision(service_profile, &bundle.revision)
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
        projection
            .managed_owner_reference()
            .map_err(ApplicationError::Internal)?,
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
mod tests {
    use super::super::commands::{
        StartDurableCellApplication, StartDurableCellApplicationHandler,
        StopDurableCellApplication, StopDurableCellApplicationHandler,
    };
    use super::*;
    use crate::modules::data::{
        ObjectNamespaceCredentialBindingSpec, ObjectNamespaceRetentionPolicySpec,
    };
    use crate::modules::durable_cells::domain::{
        CreateDurableCellApplicationWrite, DurableCellApplication, DurableCellApplicationChanged,
        DurableCellApplicationDefinition, DurableCellApplicationDefinitionSpec,
        DurableCellApplicationRevision, DurableCellClassSpec, DurableCellRollbackPolicy,
        DurableCellStateSchema, RequestDurableCellApplicationStateWrite,
        ReviseDurableCellApplicationWrite,
    };
    use crate::modules::durable_cells::infrastructure::{
        InMemoryDurableCellApplicationRepository, InMemoryDurableCellDeploymentRepository,
    };
    use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::secrets::domain::{
        CreateSecretWrite, EncryptedSecretValue, ISecretRepository, Secret, SecretChanged,
    };
    use crate::modules::secrets::infrastructure::InMemorySecretRepository;
    use crate::modules::shared_kernel::domain::{
        BuildRunId, DurableCellApplicationRevisionId, SecretId, SecretVersionReference,
    };
    use crate::modules::workloads::infrastructure::InMemoryWorkloadRepository;
    use crate::modules::workloads::{
        HttpHealthCheck, IWorkloadReplicaRetirementRepository, OciArtifact,
        ReplicaRetirementCompletion, SecretBinding, SecretBindingTarget, ServicePort,
        ServiceProcess, ServiceResources, WorkloadReplicaLifecycle,
    };
    use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
    use serde::Serialize;
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn persisted_intents_recover_through_the_existing_managed_workload_lifecycle() {
        let now = Utc::now() - chrono::Duration::seconds(5);
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let actor_principal_id = PrincipalId::new();
        let profile = service_profile();
        let applications = Arc::new(InMemoryDurableCellApplicationRepository::new());
        let record = application_record(
            organization_id,
            project_id,
            environment_id,
            actor_principal_id,
            &profile,
            now,
        );
        let application_request_id = Uuid::now_v7();
        applications
            .create(CreateDurableCellApplicationWrite {
                event: DurableCellApplicationChanged::created(
                    &record.application,
                    &record.revision,
                    application_request_id,
                )
                .expect("application event"),
                actor_principal_id,
                request_id: application_request_id,
                idempotency: IdempotencyRequest::new(
                    "durable-cell-deployment-test/application",
                    "create",
                    record.application.id.as_uuid().as_bytes(),
                )
                .expect("application idempotency"),
                record: record.clone(),
            })
            .await
            .expect("store application");

        let projection = DurableCellProjectionIdentity::for_current_revision(
            &record.application,
            &record.revision,
        )
        .expect("projection");
        let secrets = Arc::new(InMemorySecretRepository::new());
        let access_key_id = store_secret(
            secrets.as_ref(),
            organization_id,
            project_id,
            environment_id,
            "S0 access key",
            now,
        )
        .await;
        let secret_access_key = store_secret(
            secrets.as_ref(),
            organization_id,
            project_id,
            environment_id,
            "S0 secret key",
            now,
        )
        .await;
        let storage_provider_profile =
            ObjectNamespaceProviderProfile::parse_acl(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../contracts/s0.1/object-namespace-provider-profile.acl"
            )))
            .expect("storage provider profile");
        let storage_credentials =
            ObjectNamespaceCredentialBinding::from_spec(ObjectNamespaceCredentialBindingSpec {
                organization_id,
                project_id,
                environment_id,
                namespace_id: projection.storage_namespace_id,
                generation: 1,
                provider_profile_digest: storage_provider_profile.digest().clone(),
                access_key_id,
                secret_access_key,
                session_token: None,
            })
            .expect("storage credentials");
        let retention_policy =
            ObjectNamespaceRetentionPolicy::from_spec(ObjectNamespaceRetentionPolicySpec {
                minimum_sealed_recovery_points: 2,
                maximum_sealed_recovery_points: 24,
                maximum_recovery_point_age_seconds: 30 * 24 * 60 * 60,
                deletion_grace_period_seconds: 24 * 60 * 60,
            })
            .expect("retention policy");
        let command = DeployDurableCellApplication {
            organization_id,
            project_id,
            environment_id,
            application_id: record.application.id,
            application_revision_id: record.revision.id,
            service_profile_acl: profile.canonical_acl().into(),
            storage_provider_profile_acl: Some(storage_provider_profile.canonical_acl().into()),
            workload_template: service_template(&profile, access_key_id, secret_access_key),
            storage_credentials,
            retention_policy,
            node_pool_id: None,
            actor_principal_id,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            idempotency_key: "deploy-counters".into(),
            request_id: Uuid::now_v7(),
        };
        let deployments = Arc::new(InMemoryDurableCellDeploymentRepository::new());
        let workloads = Arc::new(InMemoryWorkloadRepository::new());
        let node_pools = Arc::new(InMemoryNodeRepository::new());
        let secret_port: Arc<dyn ISecretRepository> = secrets.clone();

        // Persist the exact intent without invoking Workloads, modeling a
        // process death at the cross-owner boundary.
        let prepared = PreparedDeployment::new(&command).expect("prepared deployment");
        let correlation_idempotency = prepared
            .idempotency(&command.idempotency_key)
            .expect("correlation idempotency");
        let workload_idempotency = prepared
            .workload_idempotency(&command.idempotency_key)
            .expect("Workload idempotency");
        assert!(correlation_idempotency.scope.starts_with(&format!(
            "organizations/{organization_id}/durable-cell-applications/{}/revisions/{}/",
            record.application.id, record.revision.id,
        )));
        assert_ne!(correlation_idempotency.scope, workload_idempotency.scope);
        admit_external_bindings(&secret_port, node_pools.as_ref(), &command)
            .await
            .expect("external admission");
        let correlation = prepare_correlation(workloads.as_ref(), &record, &command, &prepared)
            .await
            .expect("correlation");
        deployments
            .create(CreateDurableCellDeploymentWrite {
                deployment: correlation.clone(),
                idempotency: correlation_idempotency,
            })
            .await
            .expect("persist correlation");
        assert!(matches!(
            workloads
                .find_workload(organization_id, projection.workload_id)
                .await,
            Err(RepositoryError::NotFound)
        ));

        let handler = DeployDurableCellApplicationHandler::new(
            applications.clone(),
            deployments.clone(),
            workloads.clone(),
            secret_port,
            node_pools,
        );
        let recovered = handler
            .execute(command.clone(), CqrsContext::new(ModuleRef::new()))
            .await
            .expect("command framework")
            .expect("recover deployment");
        assert!(recovered.replayed);
        assert_eq!(recovered.correlation, correlation);
        assert_eq!(recovered.workload.workload.id, projection.workload_id);
        assert_eq!(
            recovered.workload.revision.id,
            projection.workload_revision_id
        );
        assert_eq!(recovered.workload.deployment.id, projection.deployment_id);
        assert_eq!(recovered.workload.operation.id, projection.operation_id);
        let control = workloads
            .find_workload_control(organization_id, projection.workload_id)
            .await
            .expect("managed control");
        let owner = control.spec.managed_owner.expect("managed owner");
        assert_eq!(owner.kind().as_str(), "durable-cell.application");
        assert_eq!(owner.owner_id(), record.application.id.as_uuid());
        assert_eq!(workloads.outbox_events().await.len(), 1);

        let replay = handler
            .execute(command.clone(), CqrsContext::new(ModuleRef::new()))
            .await
            .expect("command framework")
            .expect("exact replay");
        assert!(replay.replayed);
        assert_eq!(replay.correlation, recovered.correlation);
        assert_eq!(replay.workload, recovered.workload);
        assert_eq!(workloads.outbox_events().await.len(), 1);

        workloads
            .fail(
                projection.deployment_id,
                recovered.workload.deployment.aggregate_version,
                "complete the first fixture generation".into(),
                Utc::now(),
            )
            .await
            .expect("terminal first deployment");
        let mut second_definition = record.revision.definition.spec().clone();
        second_definition.build_run_id = BuildRunId::new();
        second_definition.bundle_digest = digest('b');
        let second_revision = DurableCellApplicationRevision::successor(
            &record.revision,
            DurableCellApplicationRevisionId::new(),
            DurableCellApplicationDefinition::from_spec(second_definition)
                .expect("second definition"),
            actor_principal_id,
            now + chrono::Duration::seconds(1),
        )
        .expect("second revision");
        let second_application = record
            .application
            .advance(record.application.aggregate_version, &second_revision)
            .expect("second application head");
        let second_record =
            DurableCellApplicationRecord::new(second_application.clone(), second_revision.clone())
                .expect("second record");
        store_application_revision(
            applications.as_ref(),
            &record,
            second_record.clone(),
            actor_principal_id,
            "revise-to-undeployed-two",
        )
        .await;

        let mut third_definition = second_revision.definition.spec().clone();
        third_definition.build_run_id = BuildRunId::new();
        third_definition.bundle_digest = digest('e');
        let third_revision = DurableCellApplicationRevision::successor(
            &second_revision,
            DurableCellApplicationRevisionId::new(),
            DurableCellApplicationDefinition::from_spec(third_definition)
                .expect("third definition"),
            actor_principal_id,
            now + chrono::Duration::seconds(2),
        )
        .expect("third revision");
        let third_application = second_application
            .advance(second_application.aggregate_version, &third_revision)
            .expect("third application head");
        let third_record =
            DurableCellApplicationRecord::new(third_application, third_revision.clone())
                .expect("third record");
        store_application_revision(
            applications.as_ref(),
            &second_record,
            third_record,
            actor_principal_id,
            "revise-to-deployed-three",
        )
        .await;

        let third = handler
            .execute(
                DeployDurableCellApplication {
                    application_revision_id: third_revision.id,
                    idempotency_key: "deploy-counters-third-revision".into(),
                    request_id: Uuid::now_v7(),
                    ..command.clone()
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("command framework")
            .expect("deploy third application revision");
        assert!(!third.replayed);
        assert_eq!(third.correlation.projection.application_revision_number, 3);
        assert_eq!(third.correlation.provider.workload_generation, 2);
        assert_eq!(workloads.outbox_events().await.len(), 2);
        let advanced_control = workloads
            .find_workload_control(organization_id, projection.workload_id)
            .await
            .expect("advanced managed control");
        let advanced_owner = advanced_control
            .spec
            .managed_owner
            .as_ref()
            .expect("advanced owner");
        assert_eq!(advanced_owner.owner_generation(), 3);
        assert_eq!(advanced_control.spec.placement_policy.generation(), 2);
        assert_eq!(advanced_control.aggregate_version, 2);

        // Model process death after the Durable Cell desired-state transaction
        // commits but before the Workloads-owned replica transaction begins.
        let stop_key = "stop-deployed-counters";
        let stop_request_id = Uuid::now_v7();
        let current_application = applications
            .find(
                organization_id,
                project_id,
                environment_id,
                record.application.id,
            )
            .await
            .expect("current application query")
            .expect("current application");
        assert_eq!(current_application.aggregate_version, 3);
        let stopped_application = current_application
            .request_state(
                current_application.aggregate_version,
                DurableCellApplicationDesiredState::Stopped,
                Utc::now(),
            )
            .expect("stopped application intent");
        let stopped_record =
            DurableCellApplicationRecord::new(stopped_application.clone(), third_revision.clone())
                .expect("stopped record");
        let stop_idempotency = state_idempotency(
            &stopped_record,
            current_application.aggregate_version,
            stop_key,
        );
        applications
            .request_state(RequestDurableCellApplicationStateWrite {
                event: DurableCellApplicationChanged::state_requested(
                    &stopped_application,
                    &third_revision,
                    stop_request_id,
                )
                .expect("stop event"),
                record: stopped_record.clone(),
                expected_version: current_application.aggregate_version,
                actor_principal_id,
                request_id: stop_request_id,
                idempotency: stop_idempotency,
            })
            .await
            .expect("persist stopped intent");
        assert_eq!(
            workloads
                .find_workload_control(organization_id, projection.workload_id)
                .await
                .expect("control before recovery")
                .spec
                .placement_policy
                .desired_replicas(),
            1
        );
        assert_eq!(workloads.outbox_events().await.len(), 2);

        let stop_command = StopDurableCellApplication {
            organization_id,
            project_id,
            environment_id,
            application_id: record.application.id,
            expected_version: current_application.aggregate_version,
            actor_principal_id,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            idempotency_key: stop_key.into(),
            request_id: stop_request_id,
        };
        let stop_handler =
            StopDurableCellApplicationHandler::new(applications.clone(), workloads.clone());
        let recovered_stop = stop_handler
            .execute(stop_command.clone(), CqrsContext::new(ModuleRef::new()))
            .await
            .expect("command framework")
            .expect("recover stopped replica intent");
        assert!(recovered_stop.replayed);
        assert_eq!(recovered_stop.record, stopped_record);
        let stopped_control = workloads
            .find_workload_control(organization_id, projection.workload_id)
            .await
            .expect("stopped control");
        assert_eq!(stopped_control.spec.placement_policy.desired_replicas(), 0);
        let stopped_replicas = workloads
            .list_workload_replicas(organization_id, projection.workload_id)
            .await
            .expect("stopped replicas");
        assert_eq!(stopped_replicas.len(), 1);
        assert_eq!(
            stopped_replicas[0].lifecycle,
            WorkloadReplicaLifecycle::Retiring
        );
        assert_eq!(workloads.outbox_events().await.len(), 3);
        assert!(
            stop_handler
                .execute(stop_command, CqrsContext::new(ModuleRef::new()),)
                .await
                .expect("command framework")
                .expect("exact stop replay")
                .replayed
        );
        assert_eq!(workloads.outbox_events().await.len(), 3);

        // The existing Workloads retirement authority performs cleanup. Once
        // that exact cleanup is terminal, start reactivates the same replica;
        // Durable Cells does not create a cleanup worker or a second rollout.
        let mut retirements = workloads
            .pending_replica_retirements(10)
            .await
            .expect("pending retirement");
        assert_eq!(retirements.len(), 1);
        let retirement = retirements.remove(0);
        assert!(retirement.member.node_id.is_none());
        assert!(retirement
            .deployment
            .as_ref()
            .is_some_and(|deployment| deployment.command_id.is_none()));
        let retired = workloads
            .complete_replica_retirement(ReplicaRetirementCompletion {
                organization_id,
                workload_id: projection.workload_id,
                replica_id: retirement.replica.id,
                replica_generation: retirement.replica.generation,
                expected_replica_version: retirement.replica.aggregate_version,
                member_id: retirement.member.id,
                expected_member_version: retirement.member.aggregate_version,
                fenced_node_id: None,
                completed_at: Utc::now(),
                correlation_id: Uuid::now_v7(),
            })
            .await
            .expect("complete existing Workloads retirement");
        assert_eq!(retired.value.lifecycle, WorkloadReplicaLifecycle::Retired);
        assert_eq!(workloads.outbox_events().await.len(), 4);

        let start_command = StartDurableCellApplication {
            organization_id,
            project_id,
            environment_id,
            application_id: record.application.id,
            expected_version: stopped_application.aggregate_version,
            actor_principal_id,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            idempotency_key: "restart-deployed-counters".into(),
            request_id: Uuid::now_v7(),
        };
        let start_handler =
            StartDurableCellApplicationHandler::new(applications.clone(), workloads.clone());
        let restarted = start_handler
            .execute(start_command.clone(), CqrsContext::new(ModuleRef::new()))
            .await
            .expect("command framework")
            .expect("restart retired replica");
        assert!(!restarted.replayed);
        assert_eq!(
            restarted.record.application.desired_state,
            DurableCellApplicationDesiredState::Running
        );
        let restarted_control = workloads
            .find_workload_control(organization_id, projection.workload_id)
            .await
            .expect("restarted control");
        assert_eq!(
            restarted_control.spec.placement_policy.desired_replicas(),
            1
        );
        let restarted_replicas = workloads
            .list_workload_replicas(organization_id, projection.workload_id)
            .await
            .expect("restarted replicas");
        assert_eq!(restarted_replicas.len(), 1);
        assert_eq!(
            restarted_replicas[0].lifecycle,
            WorkloadReplicaLifecycle::Desired
        );
        assert_eq!(workloads.outbox_events().await.len(), 5);
        assert!(
            start_handler
                .execute(start_command, CqrsContext::new(ModuleRef::new()),)
                .await
                .expect("command framework")
                .expect("exact start replay")
                .replayed
        );
        assert_eq!(workloads.outbox_events().await.len(), 5);

        let denied = handler
            .execute(
                DeployDurableCellApplication {
                    resource_access: ResourceAccessEvaluator::restricted([
                        ResourceGrantScope::Environment {
                            project_id,
                            environment_id: EnvironmentId::new(),
                        },
                    ]),
                    ..command
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("command framework");
        assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
    }

    fn state_idempotency(
        record: &DurableCellApplicationRecord,
        expected_version: u64,
        key: &str,
    ) -> IdempotencyRequest {
        let application = &record.application;
        let canonical = serde_json::to_vec(&CanonicalStateRequest {
            organization_id: application.organization_id,
            project_id: application.project_id,
            environment_id: application.environment_id,
            application_id: application.id,
            expected_version,
            desired_state: application.desired_state.as_str(),
        })
        .expect("canonical state request");
        IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/environments/{}/durable-cell-applications/{}/desired-state",
                application.organization_id,
                application.project_id,
                application.environment_id,
                application.id
            ),
            key,
            &canonical,
        )
        .expect("state idempotency")
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CanonicalStateRequest<'a> {
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        application_id: DurableCellApplicationId,
        expected_version: u64,
        desired_state: &'a str,
    }

    async fn store_application_revision(
        applications: &InMemoryDurableCellApplicationRepository,
        previous: &DurableCellApplicationRecord,
        next: DurableCellApplicationRecord,
        actor_principal_id: PrincipalId,
        idempotency_key: &str,
    ) {
        let request_id = Uuid::now_v7();
        let event =
            DurableCellApplicationChanged::revised(&next.application, &next.revision, request_id)
                .expect("revision event");
        applications
            .revise(ReviseDurableCellApplicationWrite {
                record: next,
                expected_version: previous.application.aggregate_version,
                event,
                actor_principal_id,
                request_id,
                idempotency: IdempotencyRequest::new(
                    "durable-cell-deployment-test/application-revisions",
                    idempotency_key,
                    request_id.as_bytes(),
                )
                .expect("revision idempotency"),
            })
            .await
            .expect("store application revision");
    }

    fn application_record(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        actor: PrincipalId,
        profile: &DurableCellServiceProfile,
        at: chrono::DateTime<Utc>,
    ) -> DurableCellApplicationRecord {
        let application_id = DurableCellApplicationId::new();
        let definition =
            DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
                build_run_id: BuildRunId::new(),
                bundle_digest: digest('a'),
                bundle_size_bytes: 4096,
                main_module: "worker.mjs".into(),
                compatibility_date: "2026-08-16".into(),
                compatibility_flags: Vec::new(),
                cell_classes: vec![DurableCellClassSpec {
                    name: "Counter".into(),
                    state_schema: DurableCellStateSchema {
                        minimum_readable_version: 1,
                        maximum_readable_version: 1,
                        write_version: 1,
                    },
                }],
                service_profile_digest: profile.digest().clone(),
                rollback_policy: DurableCellRollbackPolicy::Compatible,
            })
            .expect("application definition");
        let revision = DurableCellApplicationRevision::initial(
            organization_id,
            project_id,
            environment_id,
            application_id,
            DurableCellApplicationRevisionId::new(),
            definition,
            actor,
            at,
        )
        .expect("application revision");
        let application = DurableCellApplication::create(
            application_id,
            ResourceName::parse("Tenant counters").expect("application name"),
            &revision,
        )
        .expect("application");
        DurableCellApplicationRecord::new(application, revision).expect("application record")
    }

    fn service_profile() -> DurableCellServiceProfile {
        DurableCellServiceProfile::from_spec(
            crate::modules::durable_cells::domain::DurableCellServiceProfileSpec {
                public_runtime_port: "cell-public".into(),
                internal_runtime_port: "cell-internal".into(),
                health_path: "/__celld/health".into(),
                max_cell_name_bytes: 512,
                max_request_bytes: 16 * 1024 * 1024,
                max_response_bytes: 64 * 1024 * 1024,
                max_websocket_message_bytes: 1024 * 1024,
            },
        )
        .expect("Service profile")
    }

    fn service_template(
        profile: &DurableCellServiceProfile,
        access_key_id: SecretVersionReference,
        secret_access_key: SecretVersionReference,
    ) -> ServiceTemplate {
        let publisher = crate::modules::durable_cells::domain::DurableCellPublisherProfile::pinned_celld_v0_2_1()
            .expect("pinned celld publisher profile");
        let artifact_digest = publisher.image_digest().clone();
        ServiceTemplate {
            artifact: OciArtifact {
                uri: publisher.image_uri().into(),
                digest: artifact_digest.to_string(),
                media_type: "application/vnd.oci.image.index.v1+json".into(),
            },
            process: ServiceProcess {
                command: vec!["/usr/local/bin/celld".into()],
                args: vec![
                    "--listen".into(),
                    "0.0.0.0:8080".into(),
                    "--internal-listen".into(),
                    "0.0.0.0:8081".into(),
                ],
                working_directory: Some("/".into()),
                environment: BTreeMap::new(),
            },
            secrets: vec![
                SecretBinding {
                    name: "s0-access-key-id".into(),
                    secret_id: access_key_id.secret_id,
                    version: access_key_id.version,
                    target: SecretBindingTarget::Environment {
                        variable: "S0_ACCESS_KEY_ID".into(),
                    },
                },
                SecretBinding {
                    name: "s0-secret-access-key".into(),
                    secret_id: secret_access_key.secret_id,
                    version: secret_access_key.version,
                    target: SecretBindingTarget::Environment {
                        variable: "S0_SECRET_ACCESS_KEY".into(),
                    },
                },
            ],
            resources: ServiceResources {
                cpu_millis: 1000,
                memory_bytes: 512 * 1024 * 1024,
                pids: 256,
                ephemeral_storage_bytes: Some(512 * 1024 * 1024),
            },
            ports: vec![
                ServicePort {
                    name: profile.spec().public_runtime_port.clone(),
                    container_port: 8080,
                },
                ServicePort {
                    name: profile.spec().internal_runtime_port.clone(),
                    container_port: 8081,
                },
            ],
            health: Some(HttpHealthCheck {
                port_name: profile.spec().public_runtime_port.clone(),
                path: profile.spec().health_path.clone(),
                interval_ms: 1000,
                timeout_ms: 500,
                healthy_threshold: 1,
                unhealthy_threshold: 3,
                stabilization_window_ms: 5000,
            }),
        }
    }

    async fn store_secret(
        repository: &InMemorySecretRepository,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        name: &str,
        at: chrono::DateTime<Utc>,
    ) -> SecretVersionReference {
        let secret_id = SecretId::new();
        let (secret, version) = Secret::create(
            secret_id,
            organization_id,
            project_id,
            environment_id,
            ResourceName::parse(name).expect("Secret name"),
            EncryptedSecretValue::new("test-key", format!("ciphertext-{secret_id}"))
                .expect("ciphertext"),
            at,
        )
        .expect("Secret");
        repository
            .create(CreateSecretWrite {
                event: SecretChanged::created(&secret, &version, Uuid::now_v7())
                    .expect("Secret event"),
                idempotency: IdempotencyRequest::new(
                    "durable-cell-deployment-test/secrets",
                    secret_id.to_string(),
                    secret_id.as_uuid().as_bytes(),
                )
                .expect("Secret idempotency"),
                secret,
                version,
            })
            .await
            .expect("store Secret");
        SecretVersionReference::new(secret_id, 1).expect("Secret reference")
    }

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
    }
}
