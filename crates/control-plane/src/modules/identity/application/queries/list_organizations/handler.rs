use super::ListOrganizations;
use crate::modules::identity::application::privileged_management::installation_id;
use crate::modules::identity::domain::repositories::{
    IIdentityBootstrapRepository, IOrganizationRepository, ReadOrganizationCatalog,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListOrganizationsHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn IOrganizationRepository>,
}

impl ListOrganizationsHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn IOrganizationRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl QueryHandler<ListOrganizations> for ListOrganizationsHandler {
    fn execute(
        &self,
        query: ListOrganizations,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<
            ApplicationResult<Vec<crate::modules::identity::domain::entities::Organization>>,
        >,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let organizations = repository
                .list_visible(ReadOrganizationCatalog {
                    installation_id,
                    actor_principal_id: query.actor_principal_id,
                    credential_id: query.credential_id,
                    request_id: query.request_id,
                })
                .await;
            Ok(organizations.map_err(Into::into))
        })
    }
}
