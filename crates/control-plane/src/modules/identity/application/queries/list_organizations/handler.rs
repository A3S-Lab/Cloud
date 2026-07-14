use super::ListOrganizations;
use crate::modules::identity::domain::repositories::IOrganizationRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListOrganizationsHandler {
    repository: Arc<dyn IOrganizationRepository>,
}

impl ListOrganizationsHandler {
    pub fn new(repository: Arc<dyn IOrganizationRepository>) -> Self {
        Self { repository }
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
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let organizations = match query.organization_id {
                Some(organization_id) => repository
                    .find(organization_id)
                    .await
                    .map(|organization| organization.into_iter().collect()),
                None => repository.list().await,
            };
            Ok(organizations.map_err(Into::into))
        })
    }
}
