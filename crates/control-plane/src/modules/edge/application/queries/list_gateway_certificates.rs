use crate::modules::edge::application::resource_access::EdgeAccess;
use crate::modules::edge::domain::GatewayCertificate;
use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListGatewayCertificates {
    pub organization_id: OrganizationId,
    pub access: EdgeAccess,
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
            // Certificates are node-bound org inventory. EdgeAccess discards Node
            // grants, so only organization-wide callers may list this surface.
            if !query.access.is_organization_wide() {
                return Ok(Err(ApplicationError::Forbidden(
                    "gateway certificate inventory requires organization-wide access".into(),
                )));
            }
            Ok(edge
                .list_gateway_certificates(query.organization_id)
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
    use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_organization_inventory() {
        let handler = ListGatewayCertificatesHandler::new(Arc::new(InMemoryEdgeRepository::new()));
        let result = handler
            .execute(
                ListGatewayCertificates {
                    organization_id: OrganizationId::new(),
                    access: EdgeAccess::restricted([EdgeAccessScope::Environment {
                        project_id: ProjectId::new(),
                        environment_id: EnvironmentId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("execute list query");

        assert_eq!(
            result,
            Err(ApplicationError::Forbidden(
                "gateway certificate inventory requires organization-wide access".into()
            ))
        );
    }
}
