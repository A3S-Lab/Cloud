use super::GetRecipientContact;
use crate::modules::identity::domain::entities::RecipientContactRecord;
use crate::modules::identity::domain::repositories::IRecipientContactRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetRecipientContactHandler {
    repository: Arc<dyn IRecipientContactRepository>,
}

impl GetRecipientContactHandler {
    pub fn new(repository: Arc<dyn IRecipientContactRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetRecipientContact> for GetRecipientContactHandler {
    fn execute(
        &self,
        query: GetRecipientContact,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<RecipientContactRecord>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .find_recipient_contact(
                    query.organization_id,
                    query.actor_principal_id,
                    query.contact_id,
                )
                .await
            {
                Ok(Some(contact)) => Ok(Ok(contact)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "recipient contact was not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
