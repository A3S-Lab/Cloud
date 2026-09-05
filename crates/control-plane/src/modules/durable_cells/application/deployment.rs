use super::node_pool_port::{DurableCellNodePoolSelectionRequest, IDurableCellNodePoolPort};
#[cfg(test)]
use super::provider_workload::compose_pinned_celld_service_process;
use super::resource_access::{application_not_found, environment, revision_not_found};
use super::secret_binding_port::{
    DurableCellSecretBindingAdmissionRequest, IDurableCellSecretBindingPort,
};
use super::storage_port::{
    DurableCellStorageCredentialRequest, DurableCellStorageProviderProfileRequest,
    DurableCellStorageRetentionPolicyRequest, IDurableCellStoragePort,
};
use super::workload_port::{
    DurableCellWorkloadDeployment, DurableCellWorkloadDeploymentRequest,
    DurableCellWorkloadPlacementRequest, DurableCellWorkloadProviderProjectionRequest,
    DurableCellWorkloadProviderValidationRequest, DurableCellWorkloadReconciliationRequest,
    DurableCellWorkloadRevisionGenerationRequest, DurableCellWorkloadTemplate,
    IDurableCellWorkloadPort,
};
use crate::modules::durable_cells::domain::{
    CreateDurableCellDeploymentWrite, DurableCellApplicationDesiredState,
    DurableCellApplicationRecord, DurableCellDeployment, DurableCellDeploymentRequest,
    DurableCellProjectionIdentity, DurableCellProviderBinding, DurableCellServiceProfile,
    DurableCellStorageBinding, DurableCellStorageBindingInput, IDurableCellApplicationRepository,
    IDurableCellDeploymentRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, IdempotencyRequest,
    NodePoolId, OrganizationId, PrincipalId, ProjectId, RepositoryError, SecretVersionReference,
    Sha256Digest,
};
use crate::modules::workloads::ServiceTemplate;
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
    pub storage_credentials: DurableCellStorageCredentialRequest,
    pub retention_policy: DurableCellStorageRetentionPolicyRequest,
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
    pub workload: DurableCellWorkloadDeployment,
    pub replayed: bool,
}

#[derive(Clone)]
pub struct DeployDurableCellApplicationHandler {
    applications: Arc<dyn IDurableCellApplicationRepository>,
    deployments: Arc<dyn IDurableCellDeploymentRepository>,
    workload_port: Arc<dyn IDurableCellWorkloadPort>,
    storage: Arc<dyn IDurableCellStoragePort>,
    secret_bindings: Arc<dyn IDurableCellSecretBindingPort>,
    node_pool_port: Arc<dyn IDurableCellNodePoolPort>,
}

impl DeployDurableCellApplicationHandler {
    pub fn new(
        applications: Arc<dyn IDurableCellApplicationRepository>,
        deployments: Arc<dyn IDurableCellDeploymentRepository>,
        workload_port: Arc<dyn IDurableCellWorkloadPort>,
        storage: Arc<dyn IDurableCellStoragePort>,
        secret_bindings: Arc<dyn IDurableCellSecretBindingPort>,
        node_pool_port: Arc<dyn IDurableCellNodePoolPort>,
    ) -> Self {
        Self {
            applications,
            deployments,
            workload_port,
            storage,
            secret_bindings,
            node_pool_port,
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
        let workload_port = Arc::clone(&self.workload_port);
        let storage = Arc::clone(&self.storage);
        let secret_bindings = Arc::clone(&self.secret_bindings);
        let node_pool_port = Arc::clone(&self.node_pool_port);
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
                    if let Err(error) =
                        prepared.validate_correlation(&correlation, workload_port.as_ref())
                    {
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
                        workload_port.as_ref(),
                        storage.as_ref(),
                        secret_bindings.as_ref(),
                        node_pool_port.as_ref(),
                        &prepared,
                        &command,
                    )
                    .await
                    {
                        return Ok(Err(error));
                    }
                    let correlation = match prepare_correlation(
                        workload_port.as_ref(),
                        &record,
                        &command,
                        &prepared,
                    )
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
            let workload_request = match workload_deployment_request(
                &command,
                &prepared,
                &correlation,
                workload_idempotency,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match workload_port
                .replay_managed_deployment(&workload_request)
                .await
            {
                Ok(Some(workload)) => {
                    if let Err(error) =
                        validate_workload_projection(&correlation, &workload_request, &workload)
                    {
                        return Err(BootError::Internal(error));
                    }
                    if let Err(error) = workload_port
                        .converge_managed_replicas(&DurableCellWorkloadReconciliationRequest::new(
                            command.organization_id,
                            command.project_id,
                            command.environment_id,
                            command.application_id,
                        ))
                        .await
                    {
                        return Ok(Err(error));
                    }
                    return Ok(Ok(DurableCellDeploymentMutationResult {
                        correlation,
                        workload,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error)),
            }

            // A persisted intent may precede its Workload bundle when the
            // process dies. Recheck mutable external admission only while the
            // existing Workloads authority is still absent.
            if let Err(error) = load_current_record(applications.as_ref(), &command).await {
                return Ok(Err(error));
            }
            if let Err(error) = admit_external_bindings(
                workload_port.as_ref(),
                storage.as_ref(),
                secret_bindings.as_ref(),
                node_pool_port.as_ref(),
                &prepared,
                &command,
            )
            .await
            {
                return Ok(Err(error));
            }
            let workload = match workload_port
                .create_managed_deployment(&workload_request)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if let Err(error) =
                validate_workload_projection(&correlation, &workload_request, &workload)
            {
                return Err(BootError::Internal(error));
            }
            if let Err(error) = workload_port
                .converge_managed_replicas(&DurableCellWorkloadReconciliationRequest::new(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.application_id,
                ))
                .await
            {
                return Ok(Err(error));
            }
            Ok(Ok(DurableCellDeploymentMutationResult {
                correlation,
                replayed: correlation_replayed || workload.replayed,
                workload,
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
    storage_provider_profile_acl: Option<String>,
    service_template: DurableCellWorkloadTemplate,
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
        command.workload_template.validate()?;
        command.storage_credentials.validate()?;
        command.retention_policy.validate()?;
        let expected_namespace =
            DurableCellProjectionIdentity::storage_namespace_id_for_application(
                command.application_id,
            );
        if command.storage_credentials.organization_id != command.organization_id
            || command.storage_credentials.project_id != command.project_id
            || command.storage_credentials.environment_id != command.environment_id
            || command.storage_credentials.namespace_id != expected_namespace
        {
            return Err("Durable Cell S0 credential request has the wrong exact scope".into());
        }
        if let Some(acl) = command.storage_provider_profile_acl.as_deref() {
            DurableCellStorageProviderProfileRequest::new(
                acl,
                command.storage_credentials.provider_profile_digest.clone(),
            )?;
        }
        let service_template_digest = Sha256Digest::parse(command.workload_template.digest()?)?;
        let service_template = DurableCellWorkloadTemplate::from_serializable(
            &command.workload_template,
            service_template_digest,
        )?;
        let provider_artifact_digest =
            Sha256Digest::parse(&command.workload_template.artifact.digest)?;
        let canonical_request = serde_json::to_vec(&CanonicalDeploymentRequest {
            organization_id: command.organization_id,
            project_id: command.project_id,
            environment_id: command.environment_id,
            application_id: command.application_id,
            application_revision_id: command.application_revision_id,
            service_profile_digest: service_profile.digest().as_str(),
            service_template_digest: service_template.digest().as_str(),
            credential_binding_digest: command.storage_credentials.binding_digest.as_str(),
            retention_policy_digest: command.retention_policy.expected_digest.as_str(),
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
            storage_provider_profile_acl: command.storage_provider_profile_acl.clone(),
            service_template,
            provider_artifact_digest,
            credential_binding_digest: command.storage_credentials.binding_digest.clone(),
            storage_provider_profile_digest: command
                .storage_credentials
                .provider_profile_digest
                .clone(),
            retention_policy_digest: command.retention_policy.expected_digest.clone(),
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

    fn validate_correlation(
        &self,
        correlation: &DurableCellDeployment,
        workload_port: &dyn IDurableCellWorkloadPort,
    ) -> Result<(), String> {
        correlation.validate()?;
        let projection = &correlation.projection;
        if projection.organization_id != self.organization_id
            || projection.project_id != self.project_id
            || projection.environment_id != self.environment_id
            || projection.application_id != self.application_id
            || projection.application_revision_id != self.application_revision_id
            || correlation.storage.credential_binding_digest != self.credential_binding_digest
            || correlation.storage.provider_profile_digest != self.storage_provider_profile_digest
            || correlation.storage_provider_profile_acl()?
                != self.storage_provider_profile_acl.as_deref()
            || correlation.storage.retention_policy_digest != self.retention_policy_digest
            || correlation.provider.service_profile_digest != *self.service_profile.digest()
            || correlation.provider.service_template_digest != *self.service_template.digest()
            || correlation.provider.provider_artifact_digest != self.provider_artifact_digest
        {
            return Err("Durable Cell deployment replay changed its exact projection".into());
        }
        let placement_policy_digest = workload_port
            .compile_placement_policy_digest(&DurableCellWorkloadPlacementRequest::new(
                projection.clone(),
                correlation.provider.workload_generation,
                self.node_pool_id,
            ))
            .map_err(|error| error.to_string())?;
        if placement_policy_digest.as_str() != correlation.placement_policy_digest.as_str() {
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
    workload_port: &dyn IDurableCellWorkloadPort,
    storage: &dyn IDurableCellStoragePort,
    secret_bindings: &dyn IDurableCellSecretBindingPort,
    node_pool_port: &dyn IDurableCellNodePoolPort,
    prepared: &PreparedDeployment,
    command: &DeployDurableCellApplication,
) -> ApplicationResult<()> {
    storage
        .require_active_credentials(&command.storage_credentials)
        .await?;
    storage
        .project_retention_policy(&command.retention_policy)
        .await?;
    if let Some(acl) = prepared.storage_provider_profile_acl.as_deref() {
        let profile_request = DurableCellStorageProviderProfileRequest::new(
            acl,
            prepared.storage_provider_profile_digest.clone(),
        )
        .map_err(ApplicationError::Invalid)?;
        let profile = storage.project_provider_profile(&profile_request).await?;
        let publisher = crate::modules::durable_cells::domain::DurableCellPublisherProfile::pinned_celld_v0_2_1()
            .map_err(ApplicationError::Invalid)?;
        workload_port.validate_provider_workload(
            &DurableCellWorkloadProviderValidationRequest::new(
                command.storage_credentials.clone(),
                profile,
                prepared.service_profile.clone(),
                prepared.service_template.clone(),
                publisher,
            ),
        )?;
    }
    let bindings = command
        .workload_template
        .secrets
        .iter()
        .map(|binding| SecretVersionReference::new(binding.secret_id, binding.version))
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApplicationError::Invalid)?;
    secret_bindings
        .validate_active_bindings(&DurableCellSecretBindingAdmissionRequest::new(
            command.organization_id,
            command.project_id,
            command.environment_id,
            bindings,
        ))
        .await?;
    require_storage_credentials_in_template(
        &command.storage_credentials,
        &command.workload_template,
    )?;
    node_pool_port
        .validate_selection(&DurableCellNodePoolSelectionRequest::new(
            command.organization_id,
            command.project_id,
            command.environment_id,
            command.node_pool_id,
        ))
        .await?;
    Ok(())
}

async fn prepare_correlation(
    workload_port: &dyn IDurableCellWorkloadPort,
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
    let workload_generation = workload_port
        .resolve_revision_generation(&DurableCellWorkloadRevisionGenerationRequest::new(
            projection.organization_id,
            projection.workload_id,
            projection.workload_revision_id,
            prepared.service_template.digest().clone(),
        ))
        .await?;
    let provider_workload = workload_port.project_provider_workload(
        &DurableCellWorkloadProviderProjectionRequest::new(
            projection.clone(),
            workload_generation,
            prepared.service_template.clone(),
        ),
    )?;
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
        &DurableCellStorageBindingInput {
            namespace_id: command.storage_credentials.namespace_id,
            credential_binding_generation: command.storage_credentials.generation,
            credential_binding_digest: command.storage_credentials.binding_digest.clone(),
            provider_profile_digest: command.storage_credentials.provider_profile_digest.clone(),
            retention_policy_digest: command.retention_policy.expected_digest.clone(),
        },
    )
    .map_err(ApplicationError::Invalid)?;
    let placement_policy_digest = workload_port.compile_placement_policy_digest(
        &DurableCellWorkloadPlacementRequest::new(
            projection.clone(),
            workload_generation,
            command.node_pool_id,
        ),
    )?;
    DurableCellDeployment::bind(
        projection,
        storage,
        prepared.storage_provider_profile_acl.as_deref(),
        provider,
        placement_policy_digest,
        DurableCellDeploymentRequest {
            requested_by: command.actor_principal_id,
            request_id: command.request_id,
            requested_at: Utc::now(),
        },
    )
    .map_err(ApplicationError::Internal)
}

fn workload_deployment_request(
    command: &DeployDurableCellApplication,
    prepared: &PreparedDeployment,
    correlation: &DurableCellDeployment,
    idempotency: IdempotencyRequest,
) -> ApplicationResult<DurableCellWorkloadDeploymentRequest> {
    let projection = &correlation.projection;
    Ok(DurableCellWorkloadDeploymentRequest::new(
        projection.organization_id,
        projection.project_id,
        projection.environment_id,
        projection.application_id,
        projection.application_revision_id,
        projection.application_revision_number,
        projection.application_definition_digest.clone(),
        projection.workload_id,
        projection.workload_revision_id,
        projection.deployment_id,
        projection.operation_id,
        correlation.provider.workload_generation,
        correlation.provider.provider_artifact_digest.clone(),
        correlation.placement_policy_digest.clone(),
        prepared.service_template.clone(),
        command.node_pool_id,
        idempotency,
        correlation.request_id,
        correlation.requested_at,
    ))
}

fn validate_workload_projection(
    correlation: &DurableCellDeployment,
    request: &DurableCellWorkloadDeploymentRequest,
    workload: &DurableCellWorkloadDeployment,
) -> Result<(), String> {
    let projection = &correlation.projection;
    workload.validate()?;
    if request.workload_id != projection.workload_id
        || request.workload_revision_id != projection.workload_revision_id
        || request.deployment_id != projection.deployment_id
        || request.operation_id != projection.operation_id
        || workload.organization_id != projection.organization_id
        || workload.project_id != projection.project_id
        || workload.environment_id != projection.environment_id
        || workload.workload_id != projection.workload_id
        || workload.revision_id != projection.workload_revision_id
        || workload.deployment_id != projection.deployment_id
        || workload.operation_id != projection.operation_id
        || workload.generation != correlation.provider.workload_generation
        || workload.template_digest.as_ref() != Some(&correlation.provider.service_template_digest)
        || workload.artifact_digest.as_ref() != Some(&correlation.provider.provider_artifact_digest)
        || workload.expected_artifact_digest.as_ref()
            != Some(&correlation.provider.provider_artifact_digest)
        || workload.requested_at < correlation.requested_at
        || workload.deployment_aggregate_version == 0
    {
        return Err("Durable Cell managed Workload replay drifted".into());
    }
    Ok(())
}

fn require_storage_credentials_in_template(
    credentials: &DurableCellStorageCredentialRequest,
    template: &ServiceTemplate,
) -> ApplicationResult<()> {
    if credentials.references().iter().any(|reference| {
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
