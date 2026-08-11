use super::ListFormDrafts;
use crate::modules::forms::domain::{FormDraft, IFormRepository};
use crate::modules::shared_kernel::application::ApplicationResult;
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
            Ok(forms
                .list_drafts(query.organization_id, query.project_id)
                .await
                .map_err(Into::into))
        })
    }
}
