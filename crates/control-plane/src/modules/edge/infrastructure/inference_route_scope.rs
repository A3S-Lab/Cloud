//! Collect Inference route ACL scopes from ordinary Gateway routes.

use crate::modules::edge::domain::Route;
use crate::modules::inference::application::{
    IInferenceRouteAclProjectionPort, InferenceRouteEnvironmentScope,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::InferenceRouteAclProjection;
use std::collections::BTreeSet;

/// Deduplicate environment scopes carried by ordinary Routes for Inference
/// route ACL projection.
pub fn inference_route_scopes_from_routes(
    routes: &[Route],
) -> Result<Vec<InferenceRouteEnvironmentScope>, String> {
    let mut scopes = BTreeSet::new();
    for route in routes {
        scopes.insert(InferenceRouteEnvironmentScope::new(
            route.organization_id,
            route.project_id,
            route.environment_id,
        )?);
    }
    Ok(scopes.into_iter().collect())
}

/// Load Inference-owned route/grant ACL projections for the environments on `routes`.
pub async fn load_inference_route_projections_for_routes(
    port: &dyn IInferenceRouteAclProjectionPort,
    routes: &[Route],
) -> Result<Vec<InferenceRouteAclProjection>, RepositoryError> {
    let scopes = inference_route_scopes_from_routes(routes).map_err(RepositoryError::Conflict)?;
    port.list_inference_route_acl_projections(&scopes).await
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
    use a3s_cloud_contracts::{
        InferenceEndpointAcl, InferenceGrantAclProjection, InferenceLimitsAclProjection,
        InferenceModelAclProjection, InferenceTargetAclProjection,
    };
    use chrono::Utc;
    use uuid::Uuid;

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

    fn sample_route_projection(environment_id: EnvironmentId) -> InferenceRouteAclProjection {
        InferenceRouteAclProjection {
            route_id: Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap(),
            router: "inference".into(),
            environment_id: environment_id.as_uuid(),
            policy_revision: 11,
            models: vec![InferenceModelAclProjection {
                alias: "chat-model".into(),
                model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
                targets: vec![InferenceTargetAclProjection {
                    target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
                    service: "model-service".into(),
                    upstream_model: "internal/model-v1".into(),
                    priority: 0,
                    weight: 100,
                }],
            }],
            grants: vec![InferenceGrantAclProjection {
                credential_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
                credential_generation: 3,
                models: vec!["chat-model".into()],
                endpoints: vec![
                    InferenceEndpointAcl::Models,
                    InferenceEndpointAcl::ChatCompletions,
                ],
                limits: InferenceLimitsAclProjection {
                    max_concurrent_requests: 2,
                    requests_per_minute: 60,
                    request_burst: 2,
                    tokens_per_minute: 10_000,
                },
            }],
        }
    }

    #[test]
    fn dedupes_identical_environment_scopes() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let scopes = inference_route_scopes_from_routes(&[
            route(organization_id, project_id, environment_id),
            route(organization_id, project_id, environment_id),
        ])
        .unwrap();
        assert_eq!(scopes.len(), 1);
    }

    struct RecordingProjectionPort {
        seen: std::sync::Mutex<Vec<Vec<InferenceRouteEnvironmentScope>>>,
        projection: InferenceRouteAclProjection,
    }

    #[async_trait::async_trait]
    impl IInferenceRouteAclProjectionPort for RecordingProjectionPort {
        async fn list_inference_route_acl_projections(
            &self,
            scopes: &[InferenceRouteEnvironmentScope],
        ) -> Result<Vec<InferenceRouteAclProjection>, RepositoryError> {
            self.seen.lock().expect("seen scopes").push(scopes.to_vec());
            Ok(vec![self.projection.clone()])
        }
    }

    #[tokio::test]
    async fn loader_queries_inference_port_with_route_derived_scopes() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let first_environment = EnvironmentId::new();
        let second_environment = EnvironmentId::new();
        let routes = [
            route(organization_id, project_id, first_environment),
            route(organization_id, project_id, second_environment),
            route(organization_id, project_id, first_environment),
        ];
        let expected = inference_route_scopes_from_routes(&routes).unwrap();
        let port = RecordingProjectionPort {
            seen: std::sync::Mutex::new(Vec::new()),
            projection: sample_route_projection(first_environment),
        };
        let projections = load_inference_route_projections_for_routes(&port, &routes)
            .await
            .unwrap();
        assert_eq!(projections.len(), 1);
        assert_eq!(projections[0].router, "inference");
        let seen = port.seen.lock().expect("seen scopes");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0], expected);
        assert_eq!(expected.len(), 2);
    }
}
