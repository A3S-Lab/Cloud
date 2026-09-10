//! Collect Inference worker ACL scopes from ordinary Gateway routes.

use crate::modules::edge::domain::Route;
use crate::modules::edge::infrastructure::inference_route_scopes_from_routes;
use crate::modules::inference::application::IInferenceWorkerAclProjectionPort;
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::InferenceWorkerAclProjection;
use chrono::{DateTime, Utc};

/// Load Inference-owned worker ACL projections for the environments on `routes`.
pub async fn load_inference_worker_projections_for_routes(
    port: &dyn IInferenceWorkerAclProjectionPort,
    routes: &[Route],
    projected_at: DateTime<Utc>,
) -> Result<Vec<InferenceWorkerAclProjection>, RepositoryError> {
    let scopes = inference_route_scopes_from_routes(routes).map_err(RepositoryError::Conflict)?;
    port.list_inference_worker_acl_projections(&scopes, projected_at)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::{
        DomainNamePattern, RouteHostname, RoutePath, RoutePortName, RouteTarget, UpstreamEndpoint,
    };
    use crate::modules::inference::application::{
        IInferenceWorkerAclProjectionPort, InferenceRouteEnvironmentScope,
    };
    use crate::modules::shared_kernel::domain::{
        DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId, NodeId, OrganizationId,
        ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use async_trait::async_trait;
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

    struct RecordingWorkerPort {
        scopes: std::sync::Mutex<Vec<Vec<InferenceRouteEnvironmentScope>>>,
    }

    #[async_trait]
    impl IInferenceWorkerAclProjectionPort for RecordingWorkerPort {
        async fn list_inference_worker_acl_projections(
            &self,
            scopes: &[InferenceRouteEnvironmentScope],
            _projected_at: DateTime<Utc>,
        ) -> Result<Vec<InferenceWorkerAclProjection>, RepositoryError> {
            self.scopes
                .lock()
                .expect("worker scopes")
                .push(scopes.to_vec());
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn loaders_query_worker_port_with_route_derived_scopes() {
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
        let port = RecordingWorkerPort {
            scopes: std::sync::Mutex::new(Vec::new()),
        };
        let workers = load_inference_worker_projections_for_routes(&port, &routes, Utc::now())
            .await
            .unwrap();
        assert!(workers.is_empty());
        assert_eq!(port.scopes.lock().unwrap().as_slice(), &[expected]);
    }
}
