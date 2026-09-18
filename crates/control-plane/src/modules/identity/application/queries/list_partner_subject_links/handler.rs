use super::ListPartnerSubjectLinks;
use crate::modules::identity::application::queries::resolve_partner_subject::PartnerSubjectLinkView;
use crate::modules::identity::domain::repositories::{
    IMembershipRepository, IPartnerSubjectLinkRepository,
};
use crate::modules::identity::domain::value_objects::parse_partner_provider_key;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListPartnerSubjectLinksHandler {
    links: Arc<dyn IPartnerSubjectLinkRepository>,
    memberships: Arc<dyn IMembershipRepository>,
}

impl ListPartnerSubjectLinksHandler {
    pub fn new(
        links: Arc<dyn IPartnerSubjectLinkRepository>,
        memberships: Arc<dyn IMembershipRepository>,
    ) -> Self {
        Self { links, memberships }
    }
}

impl QueryHandler<ListPartnerSubjectLinks> for ListPartnerSubjectLinksHandler {
    fn execute(
        &self,
        query: ListPartnerSubjectLinks,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<PartnerSubjectLinkView>>>,
    > {
        let links = Arc::clone(&self.links);
        let memberships = Arc::clone(&self.memberships);
        Box::pin(async move {
            let provider_key = match query.provider_key {
                Some(value) => match parse_partner_provider_key(value) {
                    Ok(value) => Some(value),
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                },
                None => None,
            };
            let membership = match memberships
                .find_active_membership_by_principal(query.organization_id, query.principal_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            if membership.is_none() {
                return Ok(Err(ApplicationError::NotFound(
                    "partner subject link principal was not found".into(),
                )));
            }
            let listed = match links
                .list_active_partner_subject_links_for_principal(query.principal_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let views = listed
                .into_iter()
                .filter(|link| {
                    provider_key
                        .as_ref()
                        .map(|key| &link.provider_key == key)
                        .unwrap_or(true)
                })
                .map(PartnerSubjectLinkView::from)
                .collect();
            Ok(Ok(views))
        })
    }
}
