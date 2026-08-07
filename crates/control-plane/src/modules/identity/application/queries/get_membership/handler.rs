use super::GetMembership;
use crate::modules::identity::domain::repositories::{IMembershipRepository, MembershipRecord};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetMembershipHandler {
    repository: Arc<dyn IMembershipRepository>,
}

impl GetMembershipHandler {
    pub fn new(repository: Arc<dyn IMembershipRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetMembership> for GetMembershipHandler {
    fn execute(
        &self,
        query: GetMembership,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<MembershipRecord>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .find_membership(query.organization_id, query.membership_id)
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "membership not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
