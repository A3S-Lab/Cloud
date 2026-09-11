use crate::modules::identity::domain::repositories::IRecipientContactRepository;
use crate::modules::notifications::application::{
    IOutboundRecipientContactAccess, OutboundVerifiedRecipientContact,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RecipientContactId};
use async_trait::async_trait;
use std::sync::Arc;

pub struct IdentityOutboundRecipientContactAccessAdapter {
    contacts: Arc<dyn IRecipientContactRepository>,
}

impl IdentityOutboundRecipientContactAccessAdapter {
    pub fn new(contacts: Arc<dyn IRecipientContactRepository>) -> Self {
        Self { contacts }
    }
}

#[async_trait]
impl IOutboundRecipientContactAccess for IdentityOutboundRecipientContactAccessAdapter {
    async fn resolve_verified(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
    ) -> ApplicationResult<Option<OutboundVerifiedRecipientContact>> {
        match self
            .contacts
            .resolve_verified_recipient_contact(organization_id, principal_id, contact_id)
            .await
        {
            Ok(Some(contact)) => Ok(Some(OutboundVerifiedRecipientContact {
                id: contact.id,
                principal_id: contact.principal_id,
                address: contact.address.as_str().to_owned(),
                aggregate_version: contact.aggregate_version,
                verified_at: contact.verified_at,
            })),
            Ok(None) => Ok(None),
            Err(error) => Err(ApplicationError::from(error)),
        }
    }
}
