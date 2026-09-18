use super::{PartnerSubjectLinkView, ResolvePartnerSubject};
use crate::modules::identity::domain::repositories::{
    IMembershipRepository, IPartnerSubjectLinkRepository,
};
use crate::modules::identity::domain::value_objects::{
    parse_partner_directory_issuer, parse_partner_directory_subject, parse_partner_provider_key,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ResolvePartnerSubjectHandler {
    links: Arc<dyn IPartnerSubjectLinkRepository>,
    memberships: Arc<dyn IMembershipRepository>,
}

impl ResolvePartnerSubjectHandler {
    pub fn new(
        links: Arc<dyn IPartnerSubjectLinkRepository>,
        memberships: Arc<dyn IMembershipRepository>,
    ) -> Self {
        Self { links, memberships }
    }
}

impl QueryHandler<ResolvePartnerSubject> for ResolvePartnerSubjectHandler {
    fn execute(
        &self,
        query: ResolvePartnerSubject,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PartnerSubjectLinkView>>>
    {
        let links = Arc::clone(&self.links);
        let memberships = Arc::clone(&self.memberships);
        Box::pin(async move {
            let provider_key = match parse_partner_provider_key(query.provider_key) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let issuer = match parse_partner_directory_issuer(query.issuer) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let subject = match parse_partner_directory_subject(query.subject) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let link = match links
                .find_active_partner_subject_link(&issuer, &subject)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let Some(link) = link.filter(|link| link.provider_key == provider_key) else {
                return Ok(Err(ApplicationError::NotFound(
                    "partner subject link was not found".into(),
                )));
            };
            let membership = match memberships
                .find_active_membership_by_principal(query.organization_id, link.principal_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            if membership.is_none() {
                return Ok(Err(ApplicationError::NotFound(
                    "partner subject link was not found".into(),
                )));
            }
            Ok(Ok(PartnerSubjectLinkView::from(link)))
        })
    }
}
