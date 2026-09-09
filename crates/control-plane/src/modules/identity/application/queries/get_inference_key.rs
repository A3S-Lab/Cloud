use crate::modules::identity::domain::entities::InferenceCredential;
use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{InferenceCredentialId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetInferenceKey {
    pub organization_id: OrganizationId,
    pub credential_id: InferenceCredentialId,
}

impl Query for GetInferenceKey {
    type Output = ApplicationResult<InferenceCredential>;
}

pub struct GetInferenceKeyHandler {
    credentials: Arc<dyn IInferenceCredentialRepository>,
}

impl GetInferenceKeyHandler {
    pub fn new(credentials: Arc<dyn IInferenceCredentialRepository>) -> Self {
        Self { credentials }
    }
}

impl QueryHandler<GetInferenceKey> for GetInferenceKeyHandler {
    fn execute(
        &self,
        query: GetInferenceKey,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<InferenceCredential>>>
    {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            match credentials
                .find_inference_credential(query.organization_id, query.credential_id)
                .await
            {
                Ok(Some(credential)) => Ok(Ok(credential)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "inference key not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
