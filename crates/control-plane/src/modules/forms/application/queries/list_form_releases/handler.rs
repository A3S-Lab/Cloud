use super::ListFormReleases;
use crate::modules::forms::application::resource_access::FormResourceAccess;
use crate::modules::forms::domain::{FormRelease, IFormRepository};
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListFormReleasesHandler {
    forms: Arc<dyn IFormRepository>,
}

impl ListFormReleasesHandler {
    pub fn new(forms: Arc<dyn IFormRepository>) -> Self {
        Self { forms }
    }
}

impl QueryHandler<ListFormReleases> for ListFormReleasesHandler {
    fn execute(
        &self,
        query: ListFormReleases,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<FormRelease>>>> {
        let forms = Arc::clone(&self.forms);
        Box::pin(async move {
            if let Err(error) = FormResourceAccess::new(Arc::clone(&forms))
                .draft(query.organization_id, query.form_id, &query.resource_access)
                .await
            {
                return Ok(Err(error));
            }
            Ok(forms
                .list_releases(query.organization_id, query.form_id)
                .await
                .map_err(Into::into))
        })
    }
}
