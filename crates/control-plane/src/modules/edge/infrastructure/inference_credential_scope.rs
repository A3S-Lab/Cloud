//! Collect Identity inference-credential scopes from ordinary Gateway routes.

use crate::modules::edge::domain::Route;
use crate::modules::identity::application::{
    IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::InferenceCredentialAclProjection;
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

/// Load Identity-owned inference ACL projections for the environments on `routes`.
pub async fn load_inference_credential_projections_for_routes(
    port: &dyn IInferenceCredentialAclProjectionPort,
    routes: &[Route],
) -> Result<Vec<InferenceCredentialAclProjection>, RepositoryError> {
    let scopes =
        inference_credential_scopes_from_routes(routes).map_err(RepositoryError::Conflict)?;
    port.list_inference_credential_acl_projections(&scopes)
        .await
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

    struct RecordingProjectionPort {
        seen: std::sync::Mutex<Vec<Vec<InferenceCredentialEnvironmentScope>>>,
    }

    #[async_trait::async_trait]
    impl IInferenceCredentialAclProjectionPort for RecordingProjectionPort {
        async fn list_inference_credential_acl_projections(
            &self,
            scopes: &[InferenceCredentialEnvironmentScope],
        ) -> Result<Vec<InferenceCredentialAclProjection>, RepositoryError> {
            self.seen.lock().expect("seen scopes").push(scopes.to_vec());
            Ok(vec![InferenceCredentialAclProjection::new(
                uuid::Uuid::now_v7(),
                scopes[0].environment_id().as_uuid(),
                "cloud-inference",
                "a3s_inf_aaaaaaaaaaaaaaaa",
                "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                1,
                Utc::now() + chrono::Duration::hours(1),
                false,
            )
            .expect("projection")])
        }
    }

    #[tokio::test]
    async fn loader_queries_identity_port_with_route_derived_scopes() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let first_environment = EnvironmentId::new();
        let second_environment = EnvironmentId::new();
        let routes = [
            route(organization_id, project_id, first_environment),
            route(organization_id, project_id, second_environment),
            route(organization_id, project_id, first_environment),
        ];
        let expected = inference_credential_scopes_from_routes(&routes).unwrap();
        let port = RecordingProjectionPort {
            seen: std::sync::Mutex::new(Vec::new()),
        };
        let projections = load_inference_credential_projections_for_routes(&port, &routes)
            .await
            .unwrap();
        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].audience, "cloud-inference");
        let seen = port.seen.lock().expect("seen scopes");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0], expected);
        assert_eq!(expected.len(), 2);
    }
}
