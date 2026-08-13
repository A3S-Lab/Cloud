use super::GetMembershipInvitation;
use crate::modules::identity::domain::repositories::IMembershipInvitationRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetMembershipInvitationHandler {
    repository: Arc<dyn IMembershipInvitationRepository>,
}

impl GetMembershipInvitationHandler {
    pub fn new(repository: Arc<dyn IMembershipInvitationRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetMembershipInvitation> for GetMembershipInvitationHandler {
    fn execute(
        &self,
        query: GetMembershipInvitation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<
            ApplicationResult<crate::modules::identity::domain::entities::MembershipInvitation>,
        >,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .find_membership_invitation(query.organization_id, query.invitation_id)
                .await
            {
                Ok(Some(invitation)) => Ok(Ok(invitation)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "membership invitation not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
