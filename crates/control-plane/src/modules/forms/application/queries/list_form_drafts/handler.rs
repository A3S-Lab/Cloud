use super::ListFormDrafts;
use crate::modules::forms::domain::{FormDraft, IFormRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListFormDraftsHandler {
    forms: Arc<dyn IFormRepository>,
}

impl ListFormDraftsHandler {
    pub fn new(forms: Arc<dyn IFormRepository>) -> Self {
        Self { forms }
    }
}

impl QueryHandler<ListFormDrafts> for ListFormDraftsHandler {
    fn execute(
        &self,
        query: ListFormDrafts,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<FormDraft>>>> {
        let forms = Arc::clone(&self.forms);
        Box::pin(async move {
            if !query.access.project_is_visible(query.project_id) {
                return Ok(Err(ApplicationError::NotFound("forms not found".into())));
            }
            Ok(forms
                .list_drafts(query.organization_id, query.project_id)
                .await
                .map_err(Into::into))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::forms::application::resource_access::{FormAccess, FormAccessScope};
    use crate::modules::forms::infrastructure::InMemoryFormRepository;
    use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_project() {
        let handler = ListFormDraftsHandler::new(Arc::new(InMemoryFormRepository::new()));
        let result = handler
            .execute(
                ListFormDrafts {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    access: FormAccess::restricted([FormAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message)) if message == "forms not found"
        ));
    }
}
