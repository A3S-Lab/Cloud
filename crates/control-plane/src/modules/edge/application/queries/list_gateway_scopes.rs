use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::GatewayScope;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListGatewayScopes {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl Query for ListGatewayScopes {
    type Output = ApplicationResult<Vec<GatewayScope>>;
}

pub struct ListGatewayScopesHandler {
    edge: Arc<dyn IEdgeRepository>,
}

impl ListGatewayScopesHandler {
    pub fn new(edge: Arc<dyn IEdgeRepository>) -> Self {
        Self { edge }
    }
}

impl QueryHandler<ListGatewayScopes> for ListGatewayScopesHandler {
    fn execute(
        &self,
        query: ListGatewayScopes,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<GatewayScope>>>> {
        let edge = Arc::clone(&self.edge);
        Box::pin(async move {
            Ok(edge
                .list_gateway_scopes(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
                .map_err(ApplicationError::from))
        })
    }
}
