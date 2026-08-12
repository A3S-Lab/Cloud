use super::GetFormDraft;
use crate::modules::forms::application::resource_access::FormResourceAccess;
use crate::modules::forms::domain::{FormDraft, IFormRepository};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetFormDraftHandler {
    forms: Arc<dyn IFormRepository>,
}

impl GetFormDraftHandler {
    pub fn new(forms: Arc<dyn IFormRepository>) -> Self {
        Self { forms }
    }
}

impl QueryHandler<GetFormDraft> for GetFormDraftHandler {
    fn execute(
        &self,
        query: GetFormDraft,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<FormDraft>>> {
        let forms = Arc::clone(&self.forms);
        Box::pin(async move {
            Ok(FormResourceAccess::new(forms)
                .draft(query.organization_id, query.form_id, &query.resource_access)
                .await)
        })
    }
}
