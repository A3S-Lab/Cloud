use super::CreateFormDraft;
use crate::modules::forms::application::{
    FormDraftMutationResult, FormProjectScope, IFormProjectAccess,
};
use crate::modules::forms::domain::{
    CreateFormDraftWrite, FormDocument, FormDraft, FormDraftChanged, IFormRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{FormId, IdempotencyRequest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateFormDraftHandler {
    projects: Arc<dyn IFormProjectAccess>,
    forms: Arc<dyn IFormRepository>,
}

impl CreateFormDraftHandler {
    pub fn new(projects: Arc<dyn IFormProjectAccess>, forms: Arc<dyn IFormRepository>) -> Self {
        Self { projects, forms }
    }
}

impl CommandHandler<CreateFormDraft> for CreateFormDraftHandler {
    fn execute(
        &self,
        command: CreateFormDraft,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<FormDraftMutationResult>>>
    {
        let projects = Arc::clone(&self.projects);
        let forms = Arc::clone(&self.forms);
        Box::pin(async move {
            let document = match FormDocument::parse(command.document_json.as_bytes()) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "name": command.name,
                "description": command.description,
                "documentDigest": document.digest(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/forms",
                    command.organization_id, command.project_id
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
            match projects
                .project_exists(FormProjectScope {
                    organization_id: command.organization_id,
                    project_id: command.project_id,
                })
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    return Ok(Err(ApplicationError::NotFound("project not found".into())))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let draft = match FormDraft::create(
                command.organization_id,
                command.project_id,
                FormId::new(),
                command.name,
                command.description,
                document,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = FormDraftChanged::created(&draft, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match forms
                .create_draft(CreateFormDraftWrite {
                    draft,
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
