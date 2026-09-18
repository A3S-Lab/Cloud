use crate::modules::durable_cells::application::{
    DurableCellProviderWorkloadAclRequest, DurableCellProviderWorkloadAdmission,
    DurableCellWorkloadTemplate, IDurableCellProviderWorkloadAclPort,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{NodePoolId, Sha256Digest};
use crate::modules::workloads::application::{
    parse_workload_manifest, WorkloadManifest,
};
use crate::modules::workloads::{
    IOciArtifactResolver, OciArtifactResolutionError, OciRegistryCredentialReference,
    RequestedServiceTemplate, SecretBindingTarget,
};
use a3s_boot::BootError;
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter from Workloads ACL parsing and OCI resolution to
/// the Durable Cells consumer-owned provider-workload admission port.
///
/// Workloads presentation ACL parsing and the OCI resolver stay quarantined
/// here. Durable Cells Presentation and Application never import those types.
#[derive(Clone)]
pub struct WorkloadsDurableCellProviderWorkloadAclAdapter {
    artifacts: Arc<dyn IOciArtifactResolver>,
}

impl WorkloadsDurableCellProviderWorkloadAclAdapter {
    pub fn new(artifacts: Arc<dyn IOciArtifactResolver>) -> Self {
        Self { artifacts }
    }
}

#[async_trait]
impl IDurableCellProviderWorkloadAclPort for WorkloadsDurableCellProviderWorkloadAclAdapter {
    async fn admit(
        &self,
        request: &DurableCellProviderWorkloadAclRequest,
    ) -> ApplicationResult<DurableCellProviderWorkloadAdmission> {
        request.validate().map_err(ApplicationError::Invalid)?;

        let manifest: WorkloadManifest =
            match parse_workload_manifest(request.provider_workload_acl.as_bytes()) {
                Ok(value) => value,
                Err(BootError::BadRequest(message)) => {
                    return Err(ApplicationError::Invalid(message));
                }
                Err(error) => {
                    return Err(ApplicationError::Internal(format!(
                        "Durable Cell provider-workload ACL parse failed: {error}"
                    )));
                }
            };
        let requested_template: RequestedServiceTemplate = manifest.template.into();
        requested_template
            .validate_request()
            .map_err(ApplicationError::Invalid)?;
        let bound_digest = requested_template
            .artifact
            .bound_digest()
            .map_err(ApplicationError::Invalid)?;
        if requested_template.artifact.expected_digest.is_none() && bound_digest.is_none() {
            return Err(ApplicationError::Invalid(
                "Durable Cell provider workload ACL must pin an exact OCI digest".into(),
            ));
        }

        let registry_credential = requested_template
            .secrets
            .iter()
            .find(|binding| matches!(binding.target, SecretBindingTarget::RegistryCredential))
            .map(|binding| OciRegistryCredentialReference {
                organization_id: request.organization_id,
                project_id: request.project_id,
                environment_id: request.environment_id,
                secret_id: binding.secret_id,
                version: binding.version,
            });
        if let Some(reference) = registry_credential.as_ref() {
            reference
                .validate()
                .map_err(ApplicationError::Invalid)?;
        }
        let artifact = match self
            .artifacts
            .resolve(&requested_template.artifact, registry_credential.as_ref())
            .await
        {
            Ok(value) => value,
            Err(error) => return Err(map_artifact_error(error)),
        };
        let resolved_workload_template = requested_template
            .resolve(artifact)
            .map_err(ApplicationError::Invalid)?;
        let workload_template_digest = resolved_workload_template
            .digest()
            .and_then(Sha256Digest::parse)
            .map_err(ApplicationError::Invalid)?;
        let workload_template = DurableCellWorkloadTemplate::from_serializable(
            &resolved_workload_template,
            workload_template_digest,
        )
        .map_err(ApplicationError::Invalid)?;
        DurableCellProviderWorkloadAdmission::new(
            workload_template,
            manifest.node_pool_id.map(NodePoolId::from_uuid),
        )
        .map_err(ApplicationError::Invalid)
    }
}

fn map_artifact_error(error: OciArtifactResolutionError) -> ApplicationError {
    match error {
        OciArtifactResolutionError::InvalidReference(message)
        | OciArtifactResolutionError::Protocol(message) => ApplicationError::Invalid(message),
        OciArtifactResolutionError::NotFound => {
            ApplicationError::NotFound("Durable Cell provider OCI artifact not found".into())
        }
        OciArtifactResolutionError::Unauthorized
        | OciArtifactResolutionError::Credential(_)
        | OciArtifactResolutionError::Registry(_) => ApplicationError::Unavailable(
            "Durable Cell provider OCI artifact is unavailable".into(),
        ),
    }
}
