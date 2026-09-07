use crate::modules::plugins::domain::entities::PluginPlanProjection;
use crate::modules::plugins::domain::repositories::IPluginPlanProjectionRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, PluginPlanProjectionId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetPluginPlanProjection {
    pub organization_id: OrganizationId,
    pub projection_id: PluginPlanProjectionId,
}

impl Query for GetPluginPlanProjection {
    type Output = ApplicationResult<PluginPlanProjection>;
}

pub struct GetPluginPlanProjectionHandler {
    projections: Arc<dyn IPluginPlanProjectionRepository>,
}

impl GetPluginPlanProjectionHandler {
    pub fn new(projections: Arc<dyn IPluginPlanProjectionRepository>) -> Self {
        Self { projections }
    }
}

impl QueryHandler<GetPluginPlanProjection> for GetPluginPlanProjectionHandler {
    fn execute(
        &self,
        query: GetPluginPlanProjection,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PluginPlanProjection>>>
    {
        let projections = Arc::clone(&self.projections);
        Box::pin(async move {
            match projections
                .find(query.organization_id, query.projection_id)
                .await
            {
                Ok(Some(projection)) => Ok(Ok(projection)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "plugin plan projection was not found".into(),
                ))),
                Err(error) => Ok(Err(ApplicationError::from(error))),
            }
        })
    }
}
