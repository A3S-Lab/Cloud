use super::ListMyMembershipInvitations;
use crate::modules::identity::domain::entities::MembershipInvitation;
use crate::modules::identity::domain::repositories::IMembershipInvitationRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListMyMembershipInvitationsHandler {
    repository: Arc<dyn IMembershipInvitationRepository>,
}

impl ListMyMembershipInvitationsHandler {
    pub fn new(repository: Arc<dyn IMembershipInvitationRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListMyMembershipInvitations> for ListMyMembershipInvitationsHandler {
    fn execute(
        &self,
        query: ListMyMembershipInvitations,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<MembershipInvitation>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .list_membership_invitations_for_principal(query.principal_id)
                .await
            {
                Ok(invitations) => Ok(Ok(invitations)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
