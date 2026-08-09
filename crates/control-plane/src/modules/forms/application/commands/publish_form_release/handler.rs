use super::PublishFormRelease;
use crate::modules::forms::application::form_compilation::compile_release_content;
use crate::modules::forms::application::FormPublicationMutationResult;
use crate::modules::forms::domain::{
    FormPublicationRecord, FormRelease, FormReleasePublished, IFormRepository, IFormSemanticCore,
    PublishFormReleaseWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{FormReleaseId, IdempotencyRequest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct PublishFormReleaseHandler {
    forms: Arc<dyn IFormRepository>,
    semantic_core: Arc<dyn IFormSemanticCore>,
}

impl PublishFormReleaseHandler {
    pub fn new(forms: Arc<dyn IFormRepository>, semantic_core: Arc<dyn IFormSemanticCore>) -> Self {
        Self {
            forms,
            semantic_core,
        }
    }
}

impl CommandHandler<PublishFormRelease> for PublishFormReleaseHandler {
    fn execute(
        &self,
        command: PublishFormRelease,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<FormPublicationMutationResult>>,
    > {
        let forms = Arc::clone(&self.forms);
        let semantic_core = Arc::clone(&self.semantic_core);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected Form draft version must be positive".into(),
                )));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "formId": command.form_id,
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/forms/{}/releases",
                    command.organization_id, command.form_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match forms.replay_publication(&idempotency).await {
                Ok(Some(replay)) => {
                    return Ok(Ok(FormPublicationMutationResult {
                        publication: replay.value,
                        replayed: true,
                    }))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let draft = match forms
                .find_draft(command.organization_id, command.form_id)
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => return Ok(Err(ApplicationError::NotFound("Form not found".into()))),
                Err(error) => return Ok(Err(error.into())),
            };
            if draft.aggregate_version != command.expected_version {
                return Ok(Err(ApplicationError::Conflict(
                    "Form draft was changed from a stale aggregate version".into(),
                )));
            }
            let content = match compile_release_content(semantic_core, &draft.document).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let release = match FormRelease::publish(
                &draft,
                FormReleaseId::new(),
                content,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let published = match draft.record_release(command.expected_version, &release) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event = FormReleasePublished::envelope(&published, &release, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match forms
                .publish_release(PublishFormReleaseWrite {
                    publication: FormPublicationRecord {
                        draft: published,
                        release,
                    },
                    expected_version: command.expected_version,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(FormPublicationMutationResult {
                    publication: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
