use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};
use crate::modules::workloads::domain::entities::RequestedServiceTemplate;

pub(in crate::modules::workloads::application) async fn validate_secret_bindings(
    secrets: &dyn ISecretRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    template: &RequestedServiceTemplate,
) -> ApplicationResult<()> {
    for binding in &template.secrets {
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
