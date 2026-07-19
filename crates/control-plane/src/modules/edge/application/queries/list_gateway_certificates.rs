use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::GatewayCertificate;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListGatewayCertificates {
    pub organization_id: OrganizationId,
}

impl Query for ListGatewayCertificates {
    type Output = ApplicationResult<Vec<GatewayCertificate>>;
}

pub struct ListGatewayCertificatesHandler {
    edge: Arc<dyn IEdgeRepository>,
}

impl ListGatewayCertificatesHandler {
    pub fn new(edge: Arc<dyn IEdgeRepository>) -> Self {
        Self { edge }
    }
}

impl QueryHandler<ListGatewayCertificates> for ListGatewayCertificatesHandler {
    fn execute(
        &self,
        query: ListGatewayCertificates,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<GatewayCertificate>>>>
    {
        let edge = Arc::clone(&self.edge);
        Box::pin(async move {
            Ok(edge
                .list_gateway_certificates(query.organization_id)
                .await
                .map_err(ApplicationError::from))
        })
    }
}
