use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::DomainClaim;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListDomainClaims {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl Query for ListDomainClaims {
    type Output = ApplicationResult<Vec<DomainClaim>>;
}

pub struct ListDomainClaimsHandler {
    edge: Arc<dyn IEdgeRepository>,
}

impl ListDomainClaimsHandler {
    pub fn new(edge: Arc<dyn IEdgeRepository>) -> Self {
        Self { edge }
    }
}

impl QueryHandler<ListDomainClaims> for ListDomainClaimsHandler {
    fn execute(
        &self,
        query: ListDomainClaims,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<DomainClaim>>>> {
        let edge = Arc::clone(&self.edge);
        Box::pin(async move {
            Ok(edge
                .list_domain_claims(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
                .map_err(ApplicationError::from))
        })
    }
}
