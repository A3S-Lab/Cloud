use crate::modules::identity::domain::entities::InferenceCredential;
use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListInferenceKeys {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl Query for ListInferenceKeys {
    type Output = ApplicationResult<Vec<InferenceCredential>>;
}

pub struct ListInferenceKeysHandler {
    credentials: Arc<dyn IInferenceCredentialRepository>,
}

impl ListInferenceKeysHandler {
    pub fn new(credentials: Arc<dyn IInferenceCredentialRepository>) -> Self {
        Self { credentials }
    }
}

impl QueryHandler<ListInferenceKeys> for ListInferenceKeysHandler {
    fn execute(
        &self,
        query: ListInferenceKeys,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<InferenceCredential>>>>
    {
        let credentials = Arc::clone(&self.credentials);
        Box::pin(async move {
            Ok(credentials
                .list_inference_credentials_by_environment(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
                .map_err(ApplicationError::from))
        })
    }
}
