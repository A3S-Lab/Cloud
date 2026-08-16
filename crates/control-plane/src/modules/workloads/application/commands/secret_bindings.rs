use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};
use crate::modules::workloads::domain::entities::RequestedServiceTemplate;
use crate::modules::workloads::SecretBinding;

pub(in crate::modules::workloads::application) async fn validate_secret_bindings(
    secrets: &dyn ISecretRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    template: &RequestedServiceTemplate,
) -> ApplicationResult<()> {
    validate_secret_binding_references(
        secrets,
        organization_id,
        project_id,
        environment_id,
        &template.secrets,
    )
    .await
}

/// Shared exact Secret admission for internally composed managed Workloads.
/// Product modules pass only their projected bindings; Secrets remains the
/// sole active/version/scope authority.
pub(crate) async fn validate_secret_binding_references(
    secrets: &dyn ISecretRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    bindings: &[SecretBinding],
) -> ApplicationResult<()> {
    for binding in bindings {
        let secret = secrets
            .find(organization_id, binding.secret_id)
            .await
            .map_err(binding_repository_error)?;
        if secret.project_id != project_id || secret.environment_id != environment_id {
            return Err(invalid_binding());
        }
        let version = secrets
            .find_version(organization_id, binding.secret_id, binding.version)
            .await
            .map_err(binding_repository_error)?;
        if !version.is_materializable(&secret) {
            return Err(invalid_binding());
        }
    }
    Ok(())
}

fn binding_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => invalid_binding(),
        other => other.into(),
    }
}

fn invalid_binding() -> ApplicationError {
    ApplicationError::Invalid(
        "workload Secret binding does not reference an active version in this environment".into(),
    )
}
