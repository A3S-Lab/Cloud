//! Empty Applications publication route intent ACL projection port.
//!
//! Production Gateway snapshot compile may pass an empty intent vector until a
//! later slice loads projections through the desired-state planner. This empty
//! port keeps composition honest without inventing Applications authority.

use super::{
    ApplicationPublicationRouteIntentAclScope, IApplicationPublicationRouteIntentAclProjectionPort,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::ApplicationPublicationRouteIntentAclProjection;
use async_trait::async_trait;
use std::sync::Arc;

/// Intentional empty projection authority for compile-only fixtures and C16.
#[derive(Debug, Default, Clone, Copy)]
pub struct EmptyApplicationPublicationRouteIntentAclProjectionPort;

pub fn empty_application_publication_route_intent_acl_projections(
) -> Arc<dyn IApplicationPublicationRouteIntentAclProjectionPort> {
    Arc::new(EmptyApplicationPublicationRouteIntentAclProjectionPort)
}

#[async_trait]
impl IApplicationPublicationRouteIntentAclProjectionPort
    for EmptyApplicationPublicationRouteIntentAclProjectionPort
{
    async fn list_publication_route_intent_acl_projections(
        &self,
        _scopes: &[ApplicationPublicationRouteIntentAclScope],
    ) -> Result<Vec<ApplicationPublicationRouteIntentAclProjection>, RepositoryError> {
        Ok(Vec::new())
    }
}
