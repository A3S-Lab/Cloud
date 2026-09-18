use crate::modules::data::ObjectNamespaceProviderProfile;
use crate::modules::durable_cells::DurableCellAccess;
use crate::modules::durable_cells::application::require_environment_access;
use crate::modules::durable_cells::domain::{
    DurableCellDeploymentBinding, DurableCellProjectionIdentity, DurableCellServiceProfile,
};
use crate::modules::durable_cells::{
    DeployDurableCellApplication, DeployDurableCellApplicationHandler,
    DurableCellDeploymentMutationResult, DurableCellProviderWorkloadAclRequest,
    DurableCellStorageCredentialRequest, DurableCellStorageRetentionPolicyRequest,
    DurableCellStorageRetentionPolicySpec, IDurableCellProviderWorkloadAclPort,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, OrganizationId,
    PrincipalId, ProjectId,
};
use a3s_boot::{Command, CommandHandler, CqrsContext};
use std::sync::Arc;
use uuid::Uuid;

/// ACL-native public admission command. It is shared by REST and Management
/// MCP, resolves only the existing Workloads/S0 configuration contracts, and
/// delegates the authoritative write to C3.
#[derive(Debug, Clone)]
pub struct DeployDurableCellApplicationFromAcl {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub application_revision_id: DurableCellApplicationRevisionId,
    pub service_profile_acl: String,
    pub storage_provider_profile_acl: Option<String>,
    pub provider_workload_acl: String,
    pub storage_binding_acl: String,
    pub actor_principal_id: PrincipalId,
    pub access: DurableCellAccess,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for DeployDurableCellApplicationFromAcl {
    type Output = ApplicationResult<DurableCellDeploymentMutationResult>;
}

pub struct DeployDurableCellApplicationFromAclHandler {
    provider_workloads: Arc<dyn IDurableCellProviderWorkloadAclPort>,
    deployment: DeployDurableCellApplicationHandler,
}

impl DeployDurableCellApplicationFromAclHandler {
    pub fn new(
        provider_workloads: Arc<dyn IDurableCellProviderWorkloadAclPort>,
        deployment: DeployDurableCellApplicationHandler,
    ) -> Self {
        Self {
            provider_workloads,
            deployment,
        }
    }
}

impl CommandHandler<DeployDurableCellApplicationFromAcl>
    for DeployDurableCellApplicationFromAclHandler
{
    fn execute(
        &self,
        command: DeployDurableCellApplicationFromAcl,
        context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DurableCellDeploymentMutationResult>>,
    > {
        let provider_workloads = Arc::clone(&self.provider_workloads);
        let deployment = self.deployment.clone();
        Box::pin(async move {
            if let Err(error) = require_environment_access(
                command.project_id,
                command.environment_id,
                &command.access,
            ) {
                return Ok(Err(error));
            }

            // Parse all bounded ACL before any external registry access.
            let service_profile =
                match DurableCellServiceProfile::parse_acl(&command.service_profile_acl) {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let storage_provider_profile = match command
                .storage_provider_profile_acl
                .as_deref()
                .map(ObjectNamespaceProviderProfile::parse_acl)
                .transpose()
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let storage =
                match DurableCellDeploymentBinding::parse_acl(&command.storage_binding_acl) {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let provider_admission = match provider_workloads
                .admit(&DurableCellProviderWorkloadAclRequest::new(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.provider_workload_acl,
                ))
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let namespace_id = DurableCellProjectionIdentity::storage_namespace_id_for_application(
                command.application_id,
            );
            let storage_spec = storage.spec();
            let storage_credentials = match DurableCellStorageCredentialRequest::new(
                command.organization_id,
                command.project_id,
                command.environment_id,
                namespace_id,
                storage_spec.credential_generation,
                storage_spec.provider_profile_digest.clone(),
                storage_spec.access_key_id,
                storage_spec.secret_access_key,
                storage_spec.session_token,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let retention_spec = &storage_spec.retention_policy;
            let retention_spec = DurableCellStorageRetentionPolicySpec {
                minimum_sealed_recovery_points: retention_spec.minimum_sealed_recovery_points,
                maximum_sealed_recovery_points: retention_spec.maximum_sealed_recovery_points,
                maximum_recovery_point_age_seconds: retention_spec
                    .maximum_recovery_point_age_seconds,
                deletion_grace_period_seconds: retention_spec.deletion_grace_period_seconds,
            };
            let retention_digest = match retention_spec.digest() {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let retention_policy = match DurableCellStorageRetentionPolicyRequest::new(
                retention_spec,
                retention_digest,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if storage_provider_profile.as_ref().is_some_and(|profile| {
                profile.digest() != &storage_credentials.provider_profile_digest
            }) {
                return Ok(Err(ApplicationError::Invalid(
                    "Durable Cell deployment S0 profile and binding digests differ".into(),
                )));
            }

            deployment
                .execute(
                    DeployDurableCellApplication {
                        organization_id: command.organization_id,
                        project_id: command.project_id,
                        environment_id: command.environment_id,
                        application_id: command.application_id,
                        application_revision_id: command.application_revision_id,
                        service_profile_acl: service_profile.canonical_acl().into(),
                        storage_provider_profile_acl: storage_provider_profile
                            .map(|profile| profile.canonical_acl().into()),
                        workload_template: provider_admission.workload_template,
                        storage_credentials,
                        retention_policy,
                        node_pool_id: provider_admission.node_pool_id,
                        actor_principal_id: command.actor_principal_id,
                        access: command.access,
                        idempotency_key: command.idempotency_key,
                        request_id: command.request_id,
                    },
                    context,
                )
                .await
        })
    }
}
