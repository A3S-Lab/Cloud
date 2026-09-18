use super::RevokePartnerSubjectLink;
use crate::modules::identity::application::commands::link_partner_subject::PartnerSubjectLinkMutationResult;
use crate::modules::identity::domain::events::ExternalIdentityChanged;
use crate::modules::identity::domain::repositories::{
    IPartnerSubjectLinkRepository, RevokePartnerSubjectLinkWrite,
};
use crate::modules::identity::domain::value_objects::{
    parse_partner_directory_issuer, parse_partner_directory_subject, parse_partner_provider_key,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct RevokePartnerSubjectLinkHandler {
    links: Arc<dyn IPartnerSubjectLinkRepository>,
}

impl RevokePartnerSubjectLinkHandler {
    pub fn new(links: Arc<dyn IPartnerSubjectLinkRepository>) -> Self {
        Self { links }
    }
}

impl CommandHandler<RevokePartnerSubjectLink> for RevokePartnerSubjectLinkHandler {
    fn execute(
        &self,
        command: RevokePartnerSubjectLink,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<PartnerSubjectLinkMutationResult>>,
    > {
        let links = Arc::clone(&self.links);
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
            let existing = match links
                .find_active_partner_subject_link(&issuer, &subject)
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let Some(link) = existing.filter(|link| link.provider_key == provider_key) else {
                return Ok(Err(ApplicationError::NotFound(
                    "partner subject link was not found".into(),
                )));
            };
            if link.aggregate_version != command.expected_version {
                return Ok(Err(ApplicationError::Conflict(
                    "partner subject link version conflict".into(),
                )));
            }
            let now = Utc::now();
            let mut revoked = link.clone();
            if !revoked.revoke(now) {
                return Ok(Err(ApplicationError::Conflict(
                    "partner subject link is already revoked".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "providerKey": provider_key.as_str(),
                "issuer": issuer.as_str(),
                "subject": subject.as_str(),
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/partner-subject-links/revocation",
                    command.organization_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = ExternalIdentityChanged::revoked(
                &revoked,
                command.organization_id,
                command.request_id,
            )
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match links
                .revoke_partner_subject_link(RevokePartnerSubjectLinkWrite {
                    organization_id: command.organization_id,
                    provider_key,
                    issuer,
                    subject,
                    expected_version: command.expected_version,
                    revoked_at: now,
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
