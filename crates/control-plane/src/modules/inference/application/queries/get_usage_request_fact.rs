use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::inference::domain::{
    IInferenceUsageRepository, InferenceUsageRequestFact,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GetUsageRequestFact {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub request_id: Uuid,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetUsageRequestFact {
    type Output = ApplicationResult<InferenceUsageRequestFact>;
}

pub struct GetUsageRequestFactHandler {
    usage: Arc<dyn IInferenceUsageRepository>,
}

impl GetUsageRequestFactHandler {
    pub fn new(usage: Arc<dyn IInferenceUsageRepository>) -> Self {
        Self { usage }
    }
}

impl QueryHandler<GetUsageRequestFact> for GetUsageRequestFactHandler {
    fn execute(
        &self,
        query: GetUsageRequestFact,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<InferenceUsageRequestFact>>>
    {
        let usage = Arc::clone(&self.usage);
        Box::pin(async move {
            if !query
                .resource_access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "environment not found in organization".into(),
                )));
            }
            match usage
                .get_request_fact(query.organization_id, query.request_id)
                .await
            {
                Ok(Some(fact)) if fact.environment_id == query.environment_id.as_uuid() => {
                    Ok(Ok(fact))
                }
                Ok(_) => Ok(Err(ApplicationError::NotFound(
                    "inference usage request fact not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
