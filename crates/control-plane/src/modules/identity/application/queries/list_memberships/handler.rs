use super::ListMemberships;
use crate::modules::identity::domain::repositories::{IMembershipRepository, MembershipRecord};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListMembershipsHandler {
    repository: Arc<dyn IMembershipRepository>,
}

impl ListMembershipsHandler {
    pub fn new(repository: Arc<dyn IMembershipRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListMemberships> for ListMembershipsHandler {
    fn execute(
        &self,
        query: ListMemberships,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<MembershipRecord>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository.list_memberships(query.organization_id).await {
                Ok(value) => Ok(Ok(value)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
