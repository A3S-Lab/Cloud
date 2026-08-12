use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::secrets::domain::{ISecretRepository, Secret};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError, SecretId};
use std::sync::Arc;

/// Resolves indirect Secret identifiers through the Secrets authority before authorization.
///
/// Secret owns its immutable project/environment identity. Identity owns grant semantics. This
/// resolver joins those authorities at the application boundary without duplicating Secret scope
/// in Identity or inferring ownership from a Workload reference.
#[derive(Clone)]
pub(crate) struct SecretResourceAccess {
    secrets: Arc<dyn ISecretRepository>,
}

impl SecretResourceAccess {
    pub fn new(secrets: Arc<dyn ISecretRepository>) -> Self {
        Self { secrets }
    }

    pub async fn secret(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<Secret> {
        let secret = self
            .secrets
            .find(organization_id, secret_id)
            .await
            .map_err(map_secret_repository_error)?;
        if !evaluator.allows(ResourceGrantScope::Environment {
            project_id: secret.project_id,
            environment_id: secret.environment_id,
        }) {
            return Err(secret_not_found());
        }
        Ok(secret)
    }
}

fn map_secret_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => secret_not_found(),
        error => error.into(),
    }
}

fn secret_not_found() -> ApplicationError {
    ApplicationError::NotFound("secret not found".into())
}
