use super::{LinkPartnerSubject, PartnerSubjectLinkMutationResult};
use crate::modules::identity::domain::entities::{ExternalIdentityLink, IdentityPrincipalKind};
use crate::modules::identity::domain::events::ExternalIdentityChanged;
use crate::modules::identity::domain::repositories::{
    IMembershipRepository, IPartnerSubjectLinkRepository, LinkPartnerSubjectWrite,
};
use crate::modules::identity::domain::value_objects::{
    parse_partner_directory_issuer, parse_partner_directory_subject, parse_partner_provider_key,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{ExternalIdentityLinkId, IdempotencyRequest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct LinkPartnerSubjectHandler {
    links: Arc<dyn IPartnerSubjectLinkRepository>,
    memberships: Arc<dyn IMembershipRepository>,
}

impl LinkPartnerSubjectHandler {
    pub fn new(
        links: Arc<dyn IPartnerSubjectLinkRepository>,
        memberships: Arc<dyn IMembershipRepository>,
    ) -> Self {
        Self { links, memberships }
    }
}

impl CommandHandler<LinkPartnerSubject> for LinkPartnerSubjectHandler {
    fn execute(
        &self,
        command: LinkPartnerSubject,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<PartnerSubjectLinkMutationResult>>,
    > {
        let links = Arc::clone(&self.links);
        let memberships = Arc::clone(&self.memberships);
        Box::pin(async move {
            let provider_key = match parse_partner_provider_key(command.provider_key) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let issuer = match parse_partner_directory_issuer(command.issuer) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let subject = match parse_partner_directory_subject(command.subject) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let membership = match memberships
                .find_active_membership_by_principal(command.organization_id, command.principal_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let Some(membership) = membership else {
                return Ok(Err(ApplicationError::Forbidden(
                    "partner subject link requires an active organization membership".into(),
                )));
            };
            let record = match memberships
                .find_membership(command.organization_id, membership.id)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let Some(record) = record.filter(|record| {
                record.principal.is_active()
                    && record.principal.kind == IdentityPrincipalKind::Human
                    && record.principal.id == command.principal_id
            }) else {
                return Ok(Err(ApplicationError::Forbidden(
                    "partner subject links require one active human principal".into(),
                )));
            };
            let now = Utc::now();
            let link = match ExternalIdentityLink::create(
                ExternalIdentityLinkId::new(),
                provider_key.clone(),
                issuer.clone(),
                subject.clone(),
                &record.principal,
                now,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "providerKey": provider_key.as_str(),
                "issuer": issuer.as_str(),
                "subject": subject.as_str(),
                "principalId": command.principal_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/partner-subject-links",
                    command.organization_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = ExternalIdentityChanged::linked(
                &link,
                command.organization_id,
                command.request_id,
            )
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match links
                .link_partner_subject(LinkPartnerSubjectWrite {
                    organization_id: command.organization_id,
                    link,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                    events: [event],
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(PartnerSubjectLinkMutationResult {
                link: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
