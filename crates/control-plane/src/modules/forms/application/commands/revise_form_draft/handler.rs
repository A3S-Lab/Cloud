use super::ReviseFormDraft;
use crate::modules::forms::application::resource_access::FormResourceAccess;
use crate::modules::forms::application::FormDraftMutationResult;
use crate::modules::forms::domain::{
    FormDocument, FormDraftChanged, IFormRepository, ReviseFormDraftWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct ReviseFormDraftHandler {
    forms: Arc<dyn IFormRepository>,
}

impl ReviseFormDraftHandler {
    pub fn new(forms: Arc<dyn IFormRepository>) -> Self {
        Self { forms }
    }
}

impl CommandHandler<ReviseFormDraft> for ReviseFormDraftHandler {
    fn execute(
        &self,
        command: ReviseFormDraft,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<FormDraftMutationResult>>>
    {
        let forms = Arc::clone(&self.forms);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected Form draft version must be positive".into(),
                )));
            }
            let document = match FormDocument::parse(command.document_json.as_bytes()) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let current = match FormResourceAccess::new(Arc::clone(&forms))
                .draft(
                    command.organization_id,
                    command.form_id,
                    &command.resource_access,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "formId": command.form_id,
                "expectedVersion": command.expected_version,
                "name": command.name,
                "description": command.description,
                "documentDigest": document.digest(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/forms/{}/draft-revisions",
                    command.organization_id, command.form_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match forms.replay_draft_write(&idempotency).await {
                Ok(Some(replay)) => {
                    return Ok(Ok(FormDraftMutationResult {
                        draft: replay.value,
                        replayed: true,
                    }))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            if current.aggregate_version != command.expected_version {
                return Ok(Err(ApplicationError::Conflict(
                    "Form draft was changed from a stale aggregate version".into(),
                )));
            }
            let revised = match current.revise(
                command.expected_version,
                command.name,
                command.description,
                document,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = FormDraftChanged::revised(&revised, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match forms
                .revise_draft(ReviseFormDraftWrite {
                    draft: revised,
                    expected_version: command.expected_version,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(FormDraftMutationResult {
                    draft: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
