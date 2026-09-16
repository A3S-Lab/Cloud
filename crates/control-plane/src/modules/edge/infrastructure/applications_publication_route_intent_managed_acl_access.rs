//! Anti-corruption adapter: Applications ACL projections → Edge managed access.
//!
//! Edge must not import Applications domain aggregates. This adapter is the
//! single cross-module quarantine surface for APP0.3-C16.

use crate::modules::applications::application::{
    ApplicationPublicationRouteIntentAclScope, IApplicationPublicationRouteIntentAclProjectionPort,
};
use crate::modules::edge::application::{
    EdgeManagedApplicationPublicationRouteIntentScope,
    IEdgeManagedApplicationPublicationRouteIntentAccess,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::ApplicationPublicationRouteIntentAclProjection;
use async_trait::async_trait;
use std::sync::Arc;

/// ACA that loads Applications-owned publication route intent ACL projections.
#[derive(Clone)]
pub struct ApplicationsEdgeManagedPublicationRouteIntentAccessAdapter {
    projections: Arc<dyn IApplicationPublicationRouteIntentAclProjectionPort>,
}

impl ApplicationsEdgeManagedPublicationRouteIntentAccessAdapter {
    pub fn new(projections: Arc<dyn IApplicationPublicationRouteIntentAclProjectionPort>) -> Self {
        Self { projections }
    }
}

#[async_trait]
impl IEdgeManagedApplicationPublicationRouteIntentAccess for ApplicationsEdgeManagedPublicationRouteIntentAccessAdapter
{
    async fn list_for_scopes(
        &self,
        scopes: &[EdgeManagedApplicationPublicationRouteIntentScope],
    ) -> Result<Vec<ApplicationPublicationRouteIntentAclProjection>, RepositoryError> {
        let mut mapped = Vec::with_capacity(scopes.len());
        for scope in scopes {
            mapped.push(
                ApplicationPublicationRouteIntentAclScope::new(
                    scope.organization_id(),
                    scope.project_id(),
                )
                .map_err(RepositoryError::Conflict)?,
            );
        }
        mapped.sort();
        mapped.dedup();
        self.projections
            .list_publication_route_intent_acl_projections(&mapped)
            .await
    }
}
