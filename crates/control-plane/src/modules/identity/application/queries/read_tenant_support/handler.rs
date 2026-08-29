use super::GetTenantSupportGrant;
use crate::modules::identity::application::privileged_management::{installation_id, not_found};
use crate::modules::identity::domain::repositories::{
    IIdentityBootstrapRepository, ITenantSupportGrantRepository, ReadTenantSupportGrant,
    TenantSupportGrantRecord,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetTenantSupportGrantHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn ITenantSupportGrantRepository>,
}

impl GetTenantSupportGrantHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn ITenantSupportGrantRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl QueryHandler<GetTenantSupportGrant> for GetTenantSupportGrantHandler {
    fn execute(
        &self,
        query: GetTenantSupportGrant,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<TenantSupportGrantRecord>>>
    {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match repository
                .read_tenant_support_grant(ReadTenantSupportGrant {
                    installation_id,
                    grant_id: query.grant_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(not_found("tenant support grant"))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
