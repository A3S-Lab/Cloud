use super::ListMembershipInvitations;
use crate::modules::identity::domain::entities::MembershipInvitation;
use crate::modules::identity::domain::repositories::IMembershipInvitationRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListMembershipInvitationsHandler {
    repository: Arc<dyn IMembershipInvitationRepository>,
}

impl ListMembershipInvitationsHandler {
    pub fn new(repository: Arc<dyn IMembershipInvitationRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListMembershipInvitations> for ListMembershipInvitationsHandler {
    fn execute(
        &self,
        query: ListMembershipInvitations,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<MembershipInvitation>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .list_membership_invitations(query.organization_id)
                .await
            {
                Ok(invitations) => Ok(Ok(invitations)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
