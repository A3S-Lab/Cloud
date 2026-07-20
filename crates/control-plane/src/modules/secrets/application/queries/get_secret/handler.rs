use super::GetSecret;
use crate::modules::secrets::application::{SecretDetails, SecretVersionResult};
use crate::modules::secrets::domain::ISecretRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetSecretHandler {
    secrets: Arc<dyn ISecretRepository>,
}

impl GetSecretHandler {
    pub fn new(secrets: Arc<dyn ISecretRepository>) -> Self {
        Self { secrets }
    }
}

impl QueryHandler<GetSecret> for GetSecretHandler {
    fn execute(
        &self,
        query: GetSecret,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<SecretDetails>>> {
        let secrets = Arc::clone(&self.secrets);
        Box::pin(async move {
            let secret = match secrets.find(query.organization_id, query.secret_id).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let versions = match secrets
                .list_versions(query.organization_id, query.secret_id)
                .await
            {
                Ok(value) => value
                    .iter()
                    .map(SecretVersionResult::from)
                    .collect::<Vec<_>>(),
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(SecretDetails { secret, versions }))
        })
    }
}
