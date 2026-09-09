//! Collect Identity inference-credential scopes from ordinary Gateway routes.

use crate::modules::edge::domain::Route;
use crate::modules::identity::application::InferenceCredentialEnvironmentScope;
use std::collections::BTreeSet;

/// Deduplicate environment scopes carried by ordinary Routes for Identity
/// inference credential ACL projection.
pub fn inference_credential_scopes_from_routes(
    routes: &[Route],
) -> Result<Vec<InferenceCredentialEnvironmentScope>, String> {
    let mut scopes = BTreeSet::new();
    for route in routes {
        scopes.insert(InferenceCredentialEnvironmentScope::new(
            route.organization_id,
            route.project_id,
            route.environment_id,
        )?);
    }
    Ok(scopes.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::{
        DomainNamePattern, RouteHostname, RoutePath, RoutePortName, RouteTarget, UpstreamEndpoint,
    };
    use crate::modules::shared_kernel::domain::{
        DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId, NodeId, OrganizationId,
        ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use chrono::Utc;

    fn route(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Route {
        let node_id = NodeId::new();
        let workload_id = WorkloadId::new();
        let workload_revision_id = WorkloadRevisionId::new();
        let now = Utc::now();
        Route::create(
            RouteId::new(),
            organization_id,
            project_id,
            environment_id,
            GatewayScopeId::new(),
            node_id,
            RouteHostname::parse("api.example.com").unwrap(),
            RoutePath::parse("/").unwrap(),
            DomainClaimId::new(),
            DomainNamePattern::parse("api.example.com").unwrap(),
            GatewayCertificateId::new(),
            workload_id,
            RouteTarget::new(
                workload_id,
                workload_revision_id,
                format!("workload:{workload_id}:revision:{workload_revision_id}"),
                1,
                RoutePortName::parse("http").unwrap(),
                UpstreamEndpoint::parse("http://127.0.0.1:8080").unwrap(),
                now,
            )
            .unwrap(),
            now,
        )
        .unwrap()
    }

    #[test]
    fn dedupes_identical_environment_scopes() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let scopes = inference_credential_scopes_from_routes(&[
            route(organization_id, project_id, environment_id),
            route(organization_id, project_id, environment_id),
        ])
        .unwrap();
        assert_eq!(scopes.len(), 1);
    }
}
