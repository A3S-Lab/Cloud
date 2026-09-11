use super::ListSecrets;
use crate::modules::secrets::domain::{ISecretRepository, Secret};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListSecretsHandler {
    secrets: Arc<dyn ISecretRepository>,
}

impl ListSecretsHandler {
    pub fn new(secrets: Arc<dyn ISecretRepository>) -> Self {
        Self { secrets }
    }
}

impl QueryHandler<ListSecrets> for ListSecretsHandler {
    fn execute(
        &self,
        query: ListSecrets,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<Secret>>>> {
        let secrets = Arc::clone(&self.secrets);
        Box::pin(async move {
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound("secrets not found".into())));
            }
            Ok(secrets
                .list(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
                .map_err(Into::into))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::secrets::application::resource_access::{SecretAccess, SecretAccessScope};
    use crate::modules::secrets::infrastructure::InMemorySecretRepository;
    use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_environment() {
        let project_id = ProjectId::new();
        let handler = ListSecretsHandler::new(Arc::new(InMemorySecretRepository::new()));
        let result = handler
            .execute(
                ListSecrets {
                    organization_id: OrganizationId::new(),
                    project_id,
                    environment_id: EnvironmentId::new(),
                    access: SecretAccess::restricted([SecretAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message)) if message == "secrets not found"
        ));
    }
}
