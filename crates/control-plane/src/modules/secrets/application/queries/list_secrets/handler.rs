use super::ListSecrets;
use crate::modules::secrets::domain::{ISecretRepository, Secret};
use crate::modules::shared_kernel::application::ApplicationResult;
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
