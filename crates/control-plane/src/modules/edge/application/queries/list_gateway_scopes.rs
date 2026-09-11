use crate::modules::edge::application::resource_access::EdgeAccess;
use crate::modules::edge::domain::GatewayScope;
use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListGatewayScopes {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub access: EdgeAccess,
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
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "gateway scopes not found".into(),
                )));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::InMemoryEdgeRepository;
    use crate::modules::edge::application::resource_access::EdgeAccessScope;
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_environment() {
        let project_id = ProjectId::new();
        let handler = ListGatewayScopesHandler::new(Arc::new(InMemoryEdgeRepository::new()));
        let result = handler
            .execute(
                ListGatewayScopes {
                    organization_id: OrganizationId::new(),
                    project_id,
                    environment_id: EnvironmentId::new(),
                    access: EdgeAccess::restricted([EdgeAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("execute list query");

        assert_eq!(
            result,
            Err(ApplicationError::NotFound(
                "gateway scopes not found".into()
            ))
        );
    }
}
