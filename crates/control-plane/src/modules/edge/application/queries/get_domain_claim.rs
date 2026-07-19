use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::DomainClaim;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{DomainClaimId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetDomainClaim {
    pub organization_id: OrganizationId,
    pub claim_id: DomainClaimId,
}

impl Query for GetDomainClaim {
    type Output = ApplicationResult<DomainClaim>;
}

pub struct GetDomainClaimHandler {
    edge: Arc<dyn IEdgeRepository>,
}

impl GetDomainClaimHandler {
    pub fn new(edge: Arc<dyn IEdgeRepository>) -> Self {
        Self { edge }
    }
}

impl QueryHandler<GetDomainClaim> for GetDomainClaimHandler {
    fn execute(
        &self,
        query: GetDomainClaim,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<DomainClaim>>> {
        let edge = Arc::clone(&self.edge);
        Box::pin(async move {
            Ok(edge
                .find_domain_claim(query.organization_id, query.claim_id)
                .await
                .map_err(ApplicationError::from))
        })
    }
}
