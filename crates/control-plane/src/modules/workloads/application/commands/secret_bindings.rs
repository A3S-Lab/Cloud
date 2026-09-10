use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};
use crate::modules::workloads::application::{
    IWorkloadsSecretBindingAccess, WorkloadsSecretBindingScope,
};
use crate::modules::workloads::domain::entities::RequestedServiceTemplate;
use crate::modules::workloads::SecretBinding;

pub(in crate::modules::workloads::application) async fn validate_secret_bindings(
    secrets: &dyn IWorkloadsSecretBindingAccess,
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
/// sole active/version/scope authority behind the Workloads owner port.
pub(crate) async fn validate_secret_binding_references(
    secrets: &dyn IWorkloadsSecretBindingAccess,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    bindings: &[SecretBinding],
) -> ApplicationResult<()> {
    for binding in bindings {
        let scope = WorkloadsSecretBindingScope::new(
            organization_id,
            project_id,
            environment_id,
            binding.secret_id,
            binding.version,
        )
        .map_err(ApplicationError::Invalid)?;
        match secrets.binding_is_admissible(scope).await {
            Ok(true) => {}
            Ok(false) | Err(RepositoryError::NotFound) => return Err(invalid_binding()),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn invalid_binding() -> ApplicationError {
    ApplicationError::Invalid(
        "workload Secret binding does not reference an active version in this environment".into(),
    )
}
