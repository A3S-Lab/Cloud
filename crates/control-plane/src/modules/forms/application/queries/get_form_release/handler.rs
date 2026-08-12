use super::GetFormRelease;
use crate::modules::forms::application::resource_access::FormResourceAccess;
use crate::modules::forms::domain::{FormRelease, IFormRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetFormReleaseHandler {
    forms: Arc<dyn IFormRepository>,
}

impl GetFormReleaseHandler {
    pub fn new(forms: Arc<dyn IFormRepository>) -> Self {
        Self { forms }
    }
}

impl QueryHandler<GetFormRelease> for GetFormReleaseHandler {
    fn execute(
        &self,
        query: GetFormRelease,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<FormRelease>>> {
        let forms = Arc::clone(&self.forms);
        Box::pin(async move {
            if let Err(error) = FormResourceAccess::new(Arc::clone(&forms))
                .draft(query.organization_id, query.form_id, &query.resource_access)
                .await
            {
                return Ok(Err(error));
            }
            match forms
                .find_release(query.organization_id, query.form_id, query.release_id)
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "Form release not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
