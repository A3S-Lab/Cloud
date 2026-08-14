use crate::modules::connectors::domain::ConnectorDefinition;
use crate::modules::secrets::application::ExactSecretVersionAccess;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};

pub(super) async fn validate_definition_secret_references(
    access: &ExactSecretVersionAccess,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    definition: &ConnectorDefinition,
) -> ApplicationResult<()> {
    for binding in definition.secret_bindings() {
        match access
            .require(
                organization_id,
                project_id,
                environment_id,
                binding.reference.secret_id,
                binding.reference.version,
            )
            .await
        {
            Ok(()) => {}
            Err(ApplicationError::Forbidden(_)) | Err(ApplicationError::NotFound(_)) => {
                return Err(ApplicationError::Invalid(
                    "Connector Secret reference is not active in this environment".into(),
                ))
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
