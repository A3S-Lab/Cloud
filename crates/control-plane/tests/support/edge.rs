use a3s_boot::{BootRequest, CommandHandler, CqrsContext, HttpMethod, ModuleRef};
use a3s_cloud_contracts::{
    DomainEventEnvelope, GatewayAckState, GatewaySnapshot, NodeCommandPayload, NodeGatewayAck,
};
use a3s_cloud_control_plane::modules::edge::domain::repositories::{
    IEdgeRepository, StageRoutePublication,
};
use a3s_cloud_control_plane::modules::edge::infrastructure::persistence::PostgresEdgeRepository;
use a3s_cloud_control_plane::modules::edge::{
    EdgeGatewayAcknowledgementProjector, GatewayPublication, Route, RouteHostname, RoutePath,
    RoutePortName, RouteState, UpstreamEndpoint,
};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::{
    IGatewayAcknowledgementProjector, PostgresNodeRepository, RecordGatewayAcknowledgement,
    RecordGatewayAcknowledgementHandler,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId,
    WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_control_plane::ControlPlane;
use a3s_orm::PostgresExecutor;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub struct EdgeFixture {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub node_id: NodeId,
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
}

pub struct EdgeApiFixture<'a> {
    pub organization_id: &'a str,
    pub project_id: &'a str,
    pub environment_id: &'a str,
    pub workload_revision_id: &'a str,
    pub token: &'a str,
}

pub async fn exercise_edge_api(
    app: &ControlPlane,
    executor: &PostgresExecutor,
    fixture: EdgeApiFixture<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let collection_path = format!(
        "/api/v1/organizations/{}/projects/{}/environments/{}/routes",
        fixture.organization_id, fixture.project_id, fixture.environment_id
    );
    let request_body = json!({
        "workloadRevisionId": fixture.workload_revision_id,
        "hostname": "api.integration.example",
        "pathPrefix": "/v1",
        "portName": "http"
    });
    let first = app
        .call(post_json(
            &collection_path,
            "edge-api-publication",
            request_body.clone(),
            fixture.token,
        ))
        .await?;
    let replay = app
        .call(post_json(
            &collection_path,
            "edge-api-publication",
            request_body,
            fixture.token,
        ))
        .await?;
    let first_body = response_json(&first)?;
    let replay_body = response_json(&replay)?;
    assert_eq!(first.status(), 202);
    assert_eq!(replay.status(), 200, "unexpected replay: {replay_body}");
    assert_eq!(first_body["data"]["replayed"], false);
    assert_eq!(replay_body["data"]["replayed"], true);
    assert_eq!(first_body["data"]["route"], replay_body["data"]["route"]);
    assert_eq!(replay_body["data"]["commandReplayed"], true);
    let route = &first_body["data"]["route"];
    assert_eq!(route["state"], "publishing");
    let route_id = field_uuid(route, "id")?;
    let node_id = NodeId::from_uuid(field_uuid(route, "gatewayNodeId")?);
    let command_id = NodeCommandId::from_uuid(field_uuid(route, "gatewayCommandId")?);
    let revision = field_u64(route, "gatewayRevision")?;
    let snapshot_digest = field_str(route, "snapshotDigest")?.to_owned();

    let listed = app.call(get_json(&collection_path, fixture.token)).await?;
    assert_eq!(listed.status(), 200);
    let listed_body = response_json(&listed)?;
    assert_eq!(listed_body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed_body["data"][0]["id"], route_id.to_string());

    let route_path = format!(
        "/api/v1/organizations/{}/routes/{route_id}",
        fixture.organization_id
    );
    let detail = app.call(get_json(&route_path, fixture.token)).await?;
    assert_eq!(detail.status(), 200);
    assert_eq!(response_json(&detail)?["data"]["state"], "publishing");

    let nodes: Arc<dyn INodeControlRepository> =
        Arc::new(PostgresNodeRepository::new(executor.clone()));
    let issued = nodes
        .find_command(node_id, command_id)
        .await?
        .ok_or("route publication did not enqueue a Fleet command")?;
    let NodeCommandPayload::GatewaySnapshotInstall { snapshot } = &issued.payload else {
        return Err("route publication enqueued a non-Gateway Fleet command".into());
    };
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.snapshot_digest, snapshot_digest);

    let routes: Arc<dyn IEdgeRepository> = Arc::new(PostgresEdgeRepository::new(executor.clone()));
    let projector: Arc<dyn IGatewayAcknowledgementProjector> = Arc::new(
        EdgeGatewayAcknowledgementProjector::new(Arc::clone(&routes)),
    );
    let handler = RecordGatewayAcknowledgementHandler::new(Arc::clone(&nodes), projector);
    let acknowledged_at = Utc::now();
    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: command_id.as_uuid(),
        node_id: node_id.as_uuid(),
        revision,
        snapshot_digest,
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at,
    };
    let mut wrong_revision = acknowledgement.clone();
    wrong_revision.acknowledgement_id = Uuid::now_v7();
    wrong_revision.revision += 1;
    let rejected = handler
        .execute(
            RecordGatewayAcknowledgement {
                authenticated_node_id: node_id,
                acknowledgement: wrong_revision,
                received_at: acknowledged_at,
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await?;
    assert!(rejected.is_err());
    let unchanged = app.call(get_json(&route_path, fixture.token)).await?;
    assert_eq!(response_json(&unchanged)?["data"]["state"], "publishing");

    let receipt = handler
        .execute(
            RecordGatewayAcknowledgement {
                authenticated_node_id: node_id,
                acknowledgement: acknowledgement.clone(),
                received_at: acknowledged_at,
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await??;
    assert!(!receipt.replayed);
    let replayed_receipt = handler
        .execute(
            RecordGatewayAcknowledgement {
                authenticated_node_id: node_id,
                acknowledgement,
                received_at: acknowledged_at + Duration::milliseconds(1),
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await??;
    assert!(replayed_receipt.replayed);

    let active = app.call(get_json(&route_path, fixture.token)).await?;
    assert_eq!(active.status(), 200);
    let active_body = response_json(&active)?;
    assert_eq!(active_body["data"]["state"], "active");
    assert!(active_body["data"]["activatedAt"].is_string());
    assert_eq!(routes.active_routes(node_id).await?.len(), 1);
    Ok(())
}

pub async fn exercise_edge(
    executor: &PostgresExecutor,
    fixture: EdgeFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = PostgresEdgeRepository::new(executor.clone());
    let now = Utc::now();
    let initial_scope = repository.gateway_scope(fixture.node_id).await?;
    let initial_active = repository.active_routes(fixture.node_id).await?.len();
    let initial_routes = repository
        .list_routes(
            fixture.organization_id,
            fixture.project_id,
            fixture.environment_id,
        )
        .await?
        .len();
    let first_revision = initial_scope.next_revision()?;
    let mut first = staged(
        &fixture,
        first_revision,
        initial_scope.installed_revision,
        "api.example.com",
        "/v1",
        "edge-first",
        now,
    )?;
    first.expected_scope_version = initial_scope.aggregate_version;
    let first_route_id = first.route.id;
    let (stored, replay) = tokio::join!(
        repository.stage_route_publication(first.clone()),
        repository.stage_route_publication(first)
    );
    let stored = stored?;
    let replay = replay?;
    assert_ne!(stored.replayed, replay.replayed);
    assert_eq!(stored.route.id, replay.route.id);
    assert_eq!(
        repository
            .find_route(fixture.organization_id, first_route_id)
            .await?,
        stored.route
    );
    assert_eq!(
        repository.active_routes(fixture.node_id).await?.len(),
        initial_active
    );

    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: stored.publication.command_id.as_uuid(),
        node_id: fixture.node_id.as_uuid(),
        revision: first_revision,
        snapshot_digest: stored.publication.snapshot_digest.clone(),
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at: now + Duration::seconds(1),
    };
    assert!(
        repository
            .project_gateway_acknowledgement(
                &acknowledgement,
                acknowledgement.acknowledged_at + Duration::seconds(1),
            )
            .await?
    );
    assert!(
        repository
            .project_gateway_acknowledgement(
                &acknowledgement,
                acknowledgement.acknowledged_at + Duration::seconds(2),
            )
            .await?
    );
    let active = repository.active_routes(fixture.node_id).await?;
    assert_eq!(active.len(), initial_active + 1);
    assert_eq!(
        active
            .iter()
            .find(|route| route.id == first_route_id)
            .map(|route| route.state),
        Some(RouteState::Active)
    );
    let scope = repository.gateway_scope(fixture.node_id).await?;
    assert_eq!(scope.last_issued_revision, first_revision);
    assert_eq!(scope.installed_revision, Some(first_revision));
    assert_eq!(scope.aggregate_version, initial_scope.aggregate_version + 2);

    let mut duplicate = staged(
        &fixture,
        scope.next_revision()?,
        scope.installed_revision,
        "API.EXAMPLE.COM",
        "/v1",
        "edge-duplicate",
        now + Duration::seconds(3),
    )?;
    duplicate.expected_scope_version = scope.aggregate_version;
    assert!(repository.stage_route_publication(duplicate).await.is_err());

    let mut second = staged(
        &fixture,
        scope.next_revision()?,
        scope.installed_revision,
        "web.example.com",
        "/",
        "edge-second",
        now + Duration::seconds(4),
    )?;
    second.expected_scope_version = scope.aggregate_version;
    let second = repository.stage_route_publication(second).await?;
    assert_eq!(second.publication.revision, scope.next_revision()?);
    assert_eq!(
        repository
            .list_routes(
                fixture.organization_id,
                fixture.project_id,
                fixture.environment_id,
            )
            .await?
            .len(),
        initial_routes + 2
    );
    Ok(())
}

fn staged(
    fixture: &EdgeFixture,
    revision: u64,
    expected_revision: Option<u64>,
    hostname: &str,
    path: &str,
    idempotency_key: &str,
    now: chrono::DateTime<Utc>,
) -> Result<StageRoutePublication, Box<dyn std::error::Error>> {
    let command_id = NodeCommandId::new();
    let correlation_id = Uuid::now_v7();
    let snapshot = GatewaySnapshot::new(
        revision,
        expected_revision,
        format!("# route {hostname}{path}\nmanagement {{ enabled = true }}\n"),
    )?;
    let mut route = Route::create(
        RouteId::new(),
        fixture.organization_id,
        fixture.project_id,
        fixture.environment_id,
        fixture.node_id,
        RouteHostname::parse(hostname)?,
        RoutePath::parse(path)?,
        fixture.workload_id,
        fixture.revision_id,
        RoutePortName::parse("http")?,
        UpstreamEndpoint::parse("http://127.0.0.1:49152")?,
        now,
    );
    route.stage(revision, command_id, snapshot.snapshot_digest.clone(), now)?;
    let publication = GatewayPublication::stage(
        fixture.node_id,
        command_id,
        correlation_id,
        snapshot,
        now,
        now + Duration::minutes(3),
    )?;
    let canonical = format!("{hostname}{path}");
    Ok(StageRoutePublication {
        route: route.clone(),
        publication,
        expected_scope_version: 0,
        idempotency: IdempotencyRequest::new(
            "postgres-edge-routes",
            idempotency_key,
            canonical.as_bytes(),
        )?,
        event: DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "edge.route.publication-staged".into(),
            schema_version: 1,
            organization_id: fixture.organization_id.as_uuid(),
            aggregate_id: route.id.as_uuid(),
            aggregate_version: route.aggregate_version,
            occurred_at: now,
            correlation_id,
            causation_id: None,
            payload: serde_json::json!({"route_id": route.id}),
        },
    })
}

fn post_json(path: &str, idempotency_key: &str, body: Value, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path)
        .with_header("content-type", "application/json")
        .with_header("idempotency-key", idempotency_key)
        .with_header("authorization", format!("Bearer {token}"))
        .with_body(body.to_string().into_bytes())
}

fn get_json(path: &str, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Get, path)
        .with_header("accept", "application/json")
        .with_header("authorization", format!("Bearer {token}"))
}

fn response_json(response: &a3s_boot::BootResponse) -> a3s_boot::Result<Value> {
    response.body_json()
}

fn field_uuid(value: &Value, field: &str) -> Result<Uuid, Box<dyn std::error::Error>> {
    Ok(Uuid::parse_str(field_str(value, field)?)?)
}

fn field_str<'a>(value: &'a Value, field: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("response field {field:?} is not a string").into())
}

fn field_u64(value: &Value, field: &str) -> Result<u64, Box<dyn std::error::Error>> {
    value[field]
        .as_u64()
        .ok_or_else(|| format!("response field {field:?} is not an unsigned integer").into())
}
