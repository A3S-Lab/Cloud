use crate::modules::forms::domain::{FormDraft, IFormRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{FormId, OrganizationId, RepositoryError};
use std::sync::Arc;

/// Resolves indirect Form identifiers through the Forms authority before authorization.
///
/// Forms owns the canonical draft-to-project relationship. Releases inherit that relationship
/// from their draft, while Form submissions remain owned by their Workflow/HumanTask aggregate.
/// Missing and denied draft identifiers intentionally share the same not-found contract.
#[derive(Clone)]
pub(crate) struct FormResourceAccess {
    forms: Arc<dyn IFormRepository>,
}

impl FormResourceAccess {
    pub fn new(forms: Arc<dyn IFormRepository>) -> Self {
        Self { forms }
    }

    pub async fn draft(
        &self,
        organization_id: OrganizationId,
        form_id: FormId,
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<FormDraft> {
        let draft = match self.forms.find_draft(organization_id, form_id).await {
            Ok(Some(draft)) => draft,
            Ok(None) => return Err(form_not_found()),
            Err(RepositoryError::NotFound) => return Err(form_not_found()),
            Err(error) => return Err(error.into()),
        };
        if !evaluator.allows(ResourceGrantScope::Project {
            project_id: draft.project_id,
        }) {
            return Err(form_not_found());
        }
        Ok(draft)
    }
}

fn form_not_found() -> ApplicationError {
    ApplicationError::NotFound("Form not found".into())
}
