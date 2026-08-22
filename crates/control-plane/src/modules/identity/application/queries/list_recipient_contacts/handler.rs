use super::ListRecipientContacts;
use crate::modules::identity::domain::entities::RecipientContactRecord;
use crate::modules::identity::domain::repositories::IRecipientContactRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListRecipientContactsHandler {
    repository: Arc<dyn IRecipientContactRepository>,
}

impl ListRecipientContactsHandler {
    pub fn new(repository: Arc<dyn IRecipientContactRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListRecipientContacts> for ListRecipientContactsHandler {
    fn execute(
        &self,
        query: ListRecipientContacts,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<RecipientContactRecord>>>,
    > {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .list_recipient_contacts(query.organization_id, query.actor_principal_id)
                .await
            {
                Ok(contacts) => Ok(Ok(contacts)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
