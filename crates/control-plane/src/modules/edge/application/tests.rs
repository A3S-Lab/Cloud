use super::{PublishRoute, PublishRouteHandler};
use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::services::{
    GatewayCommandDispatch, IGatewayCommandQueue, IRouteTargetReader, RouteTarget,
};
use crate::modules::edge::domain::{GatewayPublication, RoutePortName, UpstreamEndpoint};
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::edge::infrastructure::{
    GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, ProjectId, RepositoryError, WorkloadId,
    WorkloadRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
struct FixedTargetReader {
    target: RouteTarget,
}

#[async_trait]
impl IRouteTargetReader for FixedTargetReader {
    async fn resolve_healthy_target(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        _port_name: &RoutePortName,
        _now: chrono::DateTime<Utc>,
    ) -> Result<RouteTarget, RepositoryError> {
        if revision_id != self.target.workload_revision_id {
            return Err(RepositoryError::NotFound);
        }
        Ok(self.target.clone())
    }
}

#[derive(Default)]
struct RecordingGatewayQueue {
    commands: Mutex<Vec<GatewayPublication>>,
}

#[async_trait]
impl IGatewayCommandQueue for RecordingGatewayQueue {
    async fn enqueue(
        &self,
        publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError> {
        publication.snapshot().map_err(RepositoryError::Conflict)?;
        let mut commands = self.commands.lock().await;
        let replayed = commands
            .iter()
            .any(|existing| existing.command_id == publication.command_id);
        if !replayed {
            commands.push(publication.clone());
        }
        Ok(GatewayCommandDispatch { replayed })
    }
}

fn compiler() -> GatewaySnapshotCompiler {
    GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
    })
    .expect("compiler")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn command(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    revision_id: WorkloadRevisionId,
    hostname: &str,
    key: &str,
    requested_at: chrono::DateTime<Utc>,
) -> PublishRoute {
    PublishRoute {
        organization_id,
        project_id,
        environment_id,
        workload_revision_id: revision_id,
        hostname: hostname.into(),
        path_prefix: "/v1".into(),
        port_name: "http".into(),
        idempotency_key: key.into(),
        request_id: Uuid::now_v7(),
        requested_at,
    }
}

#[tokio::test]
async fn publishes_one_exact_command_and_replays_the_same_route_intent() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let revision_id = WorkloadRevisionId::new();
    let node_id = NodeId::new();
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let queue = Arc::new(RecordingGatewayQueue::default());
    let handler = PublishRouteHandler::new(
        routes.clone(),
        Arc::new(FixedTargetReader {
            target: RouteTarget {
                workload_id: WorkloadId::new(),
                workload_revision_id: revision_id,
                node_id,
                upstream: UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
            },
        }),
        queue.clone(),
        compiler(),
        Duration::minutes(3),
    )
    .expect("handler");
    let request = command(
        organization_id,
        project_id,
        environment_id,
        revision_id,
        "api.example.com",
        "publish-api",
        Utc::now(),
    );
    let first = handler
        .execute(request.clone(), context())
        .await
        .expect("command bus")
        .expect("publish route");
    assert!(!first.publication.replayed);
    assert!(!first.command_replayed);
    assert_eq!(first.publication.publication.revision, 1);
    assert_eq!(
        first
            .publication
            .publication
            .acl
            .matches("routers \"")
            .count(),
        1
    );

    let original_correlation_id = request.request_id;
    let mut replay_request = request;
    replay_request.request_id = Uuid::now_v7();
    assert_ne!(replay_request.request_id, original_correlation_id);
    let replay = handler
        .execute(replay_request, context())
        .await
        .expect("command bus")
        .expect("replay route");
    assert!(replay.publication.replayed);
    assert!(replay.command_replayed);
    assert_eq!(replay.publication.route.id, first.publication.route.id);
    assert_eq!(
        replay.publication.publication.command_correlation_id,
        original_correlation_id
    );
    assert_eq!(queue.commands.lock().await.len(), 1);
}

#[tokio::test]
async fn next_publication_contains_every_active_route_in_the_scope() {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let revision_id = WorkloadRevisionId::new();
    let node_id = NodeId::new();
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let queue = Arc::new(RecordingGatewayQueue::default());
    let handler = PublishRouteHandler::new(
        routes.clone(),
        Arc::new(FixedTargetReader {
            target: RouteTarget {
                workload_id: WorkloadId::new(),
                workload_revision_id: revision_id,
                node_id,
                upstream: UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("upstream"),
            },
        }),
        queue,
        compiler(),
        Duration::minutes(3),
    )
    .expect("handler");
    let now = Utc::now();
    let first = handler
        .execute(
            command(
                organization_id,
                project_id,
                environment_id,
                revision_id,
                "api.example.com",
                "first",
                now,
            ),
            context(),
        )
        .await
        .expect("command bus")
        .expect("first route");
    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: first.publication.publication.command_id.as_uuid(),
        node_id: node_id.as_uuid(),
        revision: 1,
        snapshot_digest: first.publication.publication.snapshot_digest.clone(),
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at: now + Duration::seconds(1),
    };
    assert!(routes
        .project_gateway_acknowledgement(
            &acknowledgement,
            acknowledgement.acknowledged_at + Duration::seconds(1),
        )
        .await
        .expect("project acknowledgement"));

    let second = handler
        .execute(
            command(
                organization_id,
                project_id,
                environment_id,
                revision_id,
                "web.example.com",
                "second",
                now + Duration::seconds(2),
            ),
            context(),
        )
        .await
        .expect("command bus")
        .expect("second route");
    assert_eq!(second.publication.publication.revision, 2);
    assert_eq!(
        second
            .publication
            .publication
            .acl
            .matches("routers \"")
            .count(),
        2
    );
    assert!(second
        .publication
        .publication
        .acl
        .contains("Host(`api.example.com`)"));
    assert!(second
        .publication
        .publication
        .acl
        .contains("Host(`web.example.com`)"));
}
