use a3s_boot::{BootRequest, CommandHandler, CqrsContext, HttpMethod, ModuleRef};
use a3s_cloud_contracts::{
    DomainEventEnvelope, GatewayAckState, GatewayCertificateRequest, GatewaySnapshot,
    NodeCommandPayload, NodeGatewayAck, NodeHeartbeatV2, NodeObservationBatchV2,
    RuntimeObservationReport, RuntimeServiceEndpoint,
};
use a3s_cloud_control_plane::modules::edge::domain::events::{
    DomainClaimChanged, GatewayRouteCutoverStaged, GatewayScopeCreated,
};
use a3s_cloud_control_plane::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, IEdgeRepository, StageGatewayRouteCutover,
    StageRoutePublication, TransitionDomainClaim,
};
use a3s_cloud_control_plane::modules::edge::infrastructure::persistence::PostgresEdgeRepository;
use a3s_cloud_control_plane::modules::edge::{
    DomainClaim, DomainNamePattern, EdgeGatewayAcknowledgementProjector, GatewayCertificate,
    GatewayCertificateMaterial, GatewayPublication, GatewayRouteCutover, GatewayRouteCutoverState,
    GatewayScope, Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget,
    UpstreamEndpoint,
};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, RuntimeObservationRecord,
};
use a3s_cloud_control_plane::modules::fleet::{
    IGatewayAcknowledgementProjector, PostgresNodeRepository, RecordGatewayAcknowledgement,
    RecordGatewayAcknowledgementHandler,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId,
    IdempotencyRequest, NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId, WorkloadId,
    WorkloadRevisionId,
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
    pub revision_generation: u64,
    pub candidate_revision_id: WorkloadRevisionId,
    pub candidate_generation: u64,
    pub candidate_deployment_id: DeploymentId,
}

pub struct EdgeApiFixture<'a> {
    pub organization_id: &'a str,
    pub project_id: &'a str,
    pub environment_id: &'a str,
    pub workload_id: &'a str,
    pub workload_revision_id: &'a str,
    pub runtime_generation: u64,
    pub node_id: NodeId,
    pub token: &'a str,
}

pub async fn exercise_edge_api(
    app: &ControlPlane,
    executor: &PostgresExecutor,
    fixture: EdgeApiFixture<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let domain_collection_path = format!(
        "/api/v1/organizations/{}/projects/{}/environments/{}/domain-claims",
        fixture.organization_id, fixture.project_id, fixture.environment_id
    );
    let created_claim_response = app
        .call(post_json(
            &domain_collection_path,
            "edge-api-domain-claim",
            json!({"pattern": "api.integration.example"}),
            fixture.token,
        ))
        .await?;
    let replayed_claim_response = app
        .call(post_json(
            &domain_collection_path,
            "edge-api-domain-claim",
            json!({"pattern": "api.integration.example"}),
            fixture.token,
        ))
        .await?;
    assert_eq!(created_claim_response.status(), 201);
    assert_eq!(replayed_claim_response.status(), 200);
    let created_claim = response_json(&created_claim_response)?;
    let replayed_claim = response_json(&replayed_claim_response)?;
    assert_eq!(created_claim["data"]["replayed"], false);
    assert_eq!(replayed_claim["data"]["replayed"], true);
    assert_eq!(created_claim["data"]["id"], replayed_claim["data"]["id"]);
    let domain_claim_id = field_str(&created_claim["data"], "id")?.to_owned();
    let proof = field_str(&created_claim["data"], "challengeValue")?.to_owned();
    let verify_path = format!(
        "/api/v1/organizations/{}/domain-claims/{}/verify",
        fixture.organization_id, domain_claim_id
    );
    let verified_claim = app
        .call(post_json(
            &verify_path,
            "edge-api-domain-verification",
            json!({"proof": proof.clone()}),
            fixture.token,
        ))
        .await?;
    let replayed_verification = app
        .call(post_json(
            &verify_path,
            "edge-api-domain-verification",
            json!({"proof": proof}),
            fixture.token,
        ))
        .await?;
    assert_eq!(verified_claim.status(), 202);
    assert_eq!(replayed_verification.status(), 200);
    let verified_claim = response_json(&verified_claim)?;
    let replayed_verification = response_json(&replayed_verification)?;
    assert_eq!(verified_claim["data"]["state"], "verified");
    assert_eq!(verified_claim["data"]["replayed"], false);
    assert_eq!(replayed_verification["data"]["replayed"], true);

    let gateway_scope_collection_path = format!(
        "/api/v1/organizations/{}/projects/{}/environments/{}/gateway-scopes",
        fixture.organization_id, fixture.project_id, fixture.environment_id
    );
    let gateway_scope_request = json!({"nodeId": fixture.node_id});
    let created_scope = app
        .call(post_json(
            &gateway_scope_collection_path,
            "edge-api-gateway-scope",
            gateway_scope_request,
            fixture.token,
        ))
        .await?;
    let replayed_scope = app
        .call(post_json(
            &gateway_scope_collection_path,
            "edge-api-gateway-scope",
            json!({
                "nodeIds": [fixture.node_id],
                "minReady": 1,
                "maxUnavailable": 0
            }),
            fixture.token,
        ))
        .await?;
    assert_eq!(created_scope.status(), 201);
    assert_eq!(replayed_scope.status(), 200);
    let created_scope = response_json(&created_scope)?;
    let replayed_scope = response_json(&replayed_scope)?;
    assert_eq!(created_scope["data"]["replayed"], false);
    assert_eq!(replayed_scope["data"]["replayed"], true);
    let gateway_scope_id = field_str(&created_scope["data"], "id")?.to_owned();
    assert_eq!(replayed_scope["data"]["id"], gateway_scope_id);
    let listed_scopes = app
        .call(get_json(&gateway_scope_collection_path, fixture.token))
        .await?;
    assert_eq!(listed_scopes.status(), 200);
    assert_eq!(
        response_json(&listed_scopes)?["data"][0]["nodeId"],
        fixture.node_id.to_string()
    );

    let runtime_unit_id = format!(
        "workload:{}:revision:{}",
        fixture.workload_id, fixture.workload_revision_id
    );
    let node_repository = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let target_observation = refresh_runtime_target_observation(
        node_repository.as_ref(),
        OrganizationId::from_uuid(Uuid::parse_str(fixture.organization_id)?),
        fixture.node_id,
        &runtime_unit_id,
        fixture.runtime_generation,
    )
    .await?;
    let nodes: Arc<dyn INodeControlRepository> = node_repository;
    let persisted_target_observation = nodes
        .latest_runtime_observation(
            fixture.node_id,
            &runtime_unit_id,
            fixture.runtime_generation,
        )
        .await?
        .ok_or("route fixture has no current Runtime observation")?;
    assert_eq!(persisted_target_observation, target_observation);
    let endpoint =
        RuntimeServiceEndpoint::from_observation(&target_observation.observation, "http")?;
    let expected_upstream = format!("http://{}/", endpoint.socket_addr());

    let collection_path = format!(
        "/api/v1/organizations/{}/projects/{}/environments/{}/routes",
        fixture.organization_id, fixture.project_id, fixture.environment_id
    );
    let request_body = json!({
        "gatewayScopeId": gateway_scope_id,
        "workloadRevisionId": fixture.workload_revision_id,
        "domainClaimId": domain_claim_id,
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
    assert_eq!(
        first.status(),
        202,
        "unexpected first publication: {first_body}"
    );
    assert_eq!(replay.status(), 200, "unexpected replay: {replay_body}");
    assert_eq!(first_body["data"]["replayed"], false);
    assert_eq!(replay_body["data"]["replayed"], true);
    assert_eq!(first_body["data"]["route"], replay_body["data"]["route"]);
    assert_eq!(replay_body["data"]["commandReplayed"], true);
    let route = &first_body["data"]["route"];
    assert_eq!(route["state"], "publishing");
    assert_eq!(route["gatewayScopeId"], gateway_scope_id);
    assert_eq!(route["runtimeUnitId"], runtime_unit_id);
    assert_eq!(
        route["runtimeGeneration"],
        json!(fixture.runtime_generation)
    );
    assert_eq!(route["upstreamOrigin"], expected_upstream);
    assert_eq!(
        route["targetObservedAt"],
        json!(target_observation.received_at)
    );
    let route_id = field_uuid(route, "id")?;
    let node_id = NodeId::from_uuid(field_uuid(route, "gatewayNodeId")?);
    let command_id = NodeCommandId::from_uuid(field_uuid(route, "gatewayCommandId")?);
    let certificate_id =
        GatewayCertificateId::from_uuid(field_uuid(&first_body["data"]["certificate"], "id")?);
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
    let certificate = routes
        .find_gateway_certificate(node_id, certificate_id)
        .await?;
    issue_certificate(routes.as_ref(), &certificate, Utc::now()).await?;
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
        gateway_id: node_id.as_uuid(),
        revision,
        snapshot_digest,
        expires_at: snapshot.expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at,
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
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

    let removable_claim = app
        .call(post_json(
            &domain_collection_path,
            "edge-api-removable-domain-claim",
            json!({"pattern": "removable.integration.example"}),
            fixture.token,
        ))
        .await?;
    assert_eq!(removable_claim.status(), 201);
    let removable_claim = response_json(&removable_claim)?;
    let removable_claim_id = field_str(&removable_claim["data"], "id")?.to_owned();
    let removable_proof = field_str(&removable_claim["data"], "challengeValue")?.to_owned();
    let removable_verify_path = format!(
        "/api/v1/organizations/{}/domain-claims/{removable_claim_id}/verify",
        fixture.organization_id
    );
    let removable_verified = app
        .call(post_json(
            &removable_verify_path,
            "edge-api-removable-domain-verification",
            json!({"proof": removable_proof}),
            fixture.token,
        ))
        .await?;
    assert_eq!(removable_verified.status(), 202);
    let removable_revoke_path = format!(
        "/api/v1/organizations/{}/domain-claims/{removable_claim_id}/revoke",
        fixture.organization_id
    );
    let removable_revoked = app
        .call(post_json(
            &removable_revoke_path,
            "edge-api-removable-domain-revocation",
            json!({"reason": "customer removed ownership"}),
            fixture.token,
        ))
        .await?;
    let removable_replay = app
        .call(post_json(
            &removable_revoke_path,
            "edge-api-removable-domain-revocation",
            json!({"reason": "customer removed ownership"}),
            fixture.token,
        ))
        .await?;
    assert_eq!(removable_revoked.status(), 202);
    assert_eq!(removable_replay.status(), 200);
    let removable_revoked = response_json(&removable_revoked)?;
    let removable_replay = response_json(&removable_replay)?;
    assert_eq!(removable_revoked["data"]["state"], "revoked");
    assert_eq!(removable_revoked["data"]["replayed"], false);
    assert_eq!(removable_replay["data"]["state"], "revoked");
    assert_eq!(removable_replay["data"]["replayed"], true);
    Ok(())
}

async fn refresh_runtime_target_observation(
    repository: &PostgresNodeRepository,
    organization_id: OrganizationId,
    node_id: NodeId,
    runtime_unit_id: &str,
    runtime_generation: u64,
) -> Result<RuntimeObservationRecord, Box<dyn std::error::Error>> {
    let current = repository
        .latest_runtime_observation(node_id, runtime_unit_id, runtime_generation)
        .await?
        .ok_or("route fixture has no Runtime observation to refresh")?;
    let command_id = current
        .command_id
        .ok_or("route fixture Runtime observation is not command-bound")?;
    let node = repository.find(organization_id, node_id).await?;
    let inventory = repository
        .current_resource_inventory(node_id)
        .await?
        .ok_or("route fixture node has no current inventory")?;
    let runtime_capabilities: a3s_runtime::contract::RuntimeCapabilities =
        serde_json::from_value(node.capabilities.document().clone())?;
    let observed_at = Utc::now();
    let observed_at_ms = u64::try_from(observed_at.timestamp_millis())
        .map_err(|_| "route fixture clock predates the Unix epoch")?;
    let mut observation = current.observation;
    observation.observed_at_ms = observed_at_ms;
    if let Some(health) = observation.health.as_mut() {
        health.checked_at_ms = observed_at_ms;
    }
    let report_id = Uuid::now_v7();
    let receipt = repository
        .record_observations(
            NodeObservationBatchV2 {
                schema: NodeObservationBatchV2::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id: node.agent_instance_id,
                sent_at: observed_at,
                heartbeat: NodeHeartbeatV2 {
                    schema: NodeHeartbeatV2::SCHEMA.into(),
                    node_id: node_id.as_uuid(),
                    agent_instance_id: node.agent_instance_id,
                    observed_at,
                    agent_version: node.agent_version,
                    runtime_capabilities,
                    inventory: inventory.inventory.reference(),
                },
                observations: vec![RuntimeObservationReport {
                    report_id,
                    command_id: Some(command_id.as_uuid()),
                    observed_at,
                    observation,
                }],
            }
            .into(),
            observed_at,
        )
        .await?;
    assert_eq!(receipt.accepted_reports, 1);
    assert_eq!(receipt.replayed_reports, 0);
    let refreshed = repository
        .latest_runtime_observation(node_id, runtime_unit_id, runtime_generation)
        .await?
        .ok_or("route fixture refresh did not persist a Runtime observation")?;
    assert_eq!(refreshed.report_id, report_id);
    assert_eq!(refreshed.command_id, Some(command_id));
    Ok(refreshed)
}

pub async fn exercise_edge(
    executor: &PostgresExecutor,
    fixture: EdgeFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = PostgresEdgeRepository::new(executor.clone());
    let now = Utc::now();
    let gateway_scope = match repository
        .list_gateway_scopes(
            fixture.organization_id,
            fixture.project_id,
            fixture.environment_id,
        )
        .await?
        .into_iter()
        .find(|scope| scope.node_id == fixture.node_id)
    {
        Some(scope) => scope,
        None => {
            let scope = GatewayScope::create(
                GatewayScopeId::new(),
                fixture.organization_id,
                fixture.project_id,
                fixture.environment_id,
                fixture.node_id,
                now,
            )?;
            repository
                .create_gateway_scope(CreateGatewayScopeWrite {
                    scope: scope.clone(),
                    idempotency: IdempotencyRequest::new(
                        "postgres-edge-gateway-scopes",
                        scope.id.to_string(),
                        scope.node_id.to_string().as_bytes(),
                    )?,
                    event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())?,
                })
                .await?
                .value
        }
    };
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
    let domain_claim = verified_claim(
        &repository,
        &fixture,
        "*.example.com",
        now - Duration::seconds(1),
    )
    .await?;
    let first_revision = initial_scope.next_revision()?;
    let mut first = staged(
        &fixture,
        &gateway_scope,
        first_revision,
        initial_scope.installed_revision,
        "api.example.com",
        "/v1",
        "edge-first",
        now,
        &domain_claim,
    )?;
    first.expected_scope_version = initial_scope.aggregate_version;
    let first_route_id = first.route.id;
    let first_idempotency = first.idempotency.clone();
    let (stored, replay) = tokio::join!(
        repository.stage_route_publication(first.clone()),
        repository.stage_route_publication(first)
    );
    let stored = stored?;
    let replay = replay?;
    assert_ne!(stored.replayed, replay.replayed);
    assert_eq!(stored.route.id, replay.route.id);
    let preflight = repository
        .replay_route_publication(&first_idempotency)
        .await?
        .ok_or("stored route publication has no idempotency replay")?;
    assert!(preflight.replayed);
    assert_eq!(preflight.route.id, stored.route.id);
    let changed_idempotency = IdempotencyRequest::new(
        first_idempotency.scope,
        first_idempotency.key,
        b"changed route publication",
    )?;
    assert!(repository
        .replay_route_publication(&changed_idempotency)
        .await
        .is_err());
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
    issue_certificate(
        &repository,
        &stored.certificate,
        now + Duration::milliseconds(1),
    )
    .await?;

    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: stored.publication.command_id.as_uuid(),
        node_id: fixture.node_id.as_uuid(),
        gateway_id: fixture.node_id.as_uuid(),
        revision: first_revision,
        snapshot_digest: stored.publication.snapshot_digest.clone(),
        expires_at: stored.publication.snapshot_expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at: now + Duration::seconds(1),
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
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
        &gateway_scope,
        scope.next_revision()?,
        scope.installed_revision,
        "API.EXAMPLE.COM",
        "/v1",
        "edge-duplicate",
        now + Duration::seconds(3),
        &domain_claim,
    )?;
    duplicate.expected_scope_version = scope.aggregate_version;
    assert!(repository.stage_route_publication(duplicate).await.is_err());

    let mut second = staged(
        &fixture,
        &gateway_scope,
        scope.next_revision()?,
        scope.installed_revision,
        "web.example.com",
        "/",
        "edge-second",
        now + Duration::seconds(4),
        &domain_claim,
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
    issue_certificate(
        &repository,
        &second.certificate,
        second.publication.command_issued_at + Duration::milliseconds(1),
    )
    .await?;
    let second_ack = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: second.publication.command_id.as_uuid(),
        node_id: fixture.node_id.as_uuid(),
        gateway_id: fixture.node_id.as_uuid(),
        revision: second.publication.revision,
        snapshot_digest: second.publication.snapshot_digest.clone(),
        expires_at: second.publication.snapshot_expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at: second.publication.command_issued_at + Duration::seconds(1),
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    };
    repository
        .project_gateway_acknowledgement(
            &second_ack,
            second_ack.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;

    let before_cutover = repository
        .active_routes(fixture.node_id)
        .await?
        .into_iter()
        .filter(|route| {
            route.workload_id == fixture.workload_id
                && route.target.workload_revision_id == fixture.revision_id
        })
        .collect::<Vec<_>>();
    assert_eq!(before_cutover.len(), 2);
    assert!(before_cutover.iter().all(|route| {
        route.target.runtime_generation == fixture.revision_generation
            && route.target.runtime_unit_id
                == format!(
                    "workload:{}:revision:{}",
                    fixture.workload_id, fixture.revision_id
                )
    }));
    let scope = repository.gateway_scope(fixture.node_id).await?;
    let request = staged_cutover(
        &fixture,
        &before_cutover,
        scope.next_revision()?,
        scope.installed_revision,
        scope.aggregate_version,
        now + Duration::seconds(6),
    )?;
    let idempotency = request.idempotency.clone();
    let (cutover, replay) = tokio::join!(
        repository.stage_gateway_route_cutover(request.clone()),
        repository.stage_gateway_route_cutover(request)
    );
    let cutover = cutover?;
    let replay = replay?;
    assert_ne!(cutover.replayed, replay.replayed);
    assert_eq!(cutover.cutover, replay.cutover);
    let restarted_repository = PostgresEdgeRepository::new(executor.clone());
    let restarted_pending = restarted_repository
        .find_gateway_route_cutover(fixture.organization_id, fixture.candidate_deployment_id)
        .await?
        .ok_or("restarted PostgreSQL repository lost the pending route cutover")?;
    assert_eq!(
        restarted_pending.previous_generation,
        fixture.revision_generation
    );
    assert_eq!(
        restarted_pending.candidate_generation,
        fixture.candidate_generation
    );
    assert!(restarted_pending.routes.iter().all(|route| {
        route.target.runtime_generation == fixture.candidate_generation
            && route.target.runtime_unit_id
                == format!(
                    "workload:{}:revision:{}",
                    fixture.workload_id, fixture.candidate_revision_id
                )
    }));
    assert_eq!(
        repository
            .replay_gateway_route_cutover(&idempotency)
            .await?
            .ok_or("PostgreSQL cutover has no idempotency replay")?
            .cutover,
        cutover.cutover
    );
    assert!(repository
        .replay_gateway_route_cutover(&IdempotencyRequest::new(
            idempotency.scope,
            idempotency.key,
            b"changed cutover",
        )?)
        .await
        .is_err());
    for route in &before_cutover {
        assert_eq!(
            repository
                .find_route(fixture.organization_id, route.id)
                .await?,
            *route
        );
    }
    let mut wrong = cutover_acknowledgement(&cutover.cutover, GatewayAckState::Applied);
    wrong.snapshot_digest = format!("sha256:{}", "f".repeat(64));
    assert!(repository
        .project_gateway_acknowledgement(&wrong, wrong.acknowledged_at)
        .await
        .is_err());
    for route in &before_cutover {
        assert_eq!(
            repository
                .find_route(fixture.organization_id, route.id)
                .await?,
            *route
        );
    }
    let cutover_issued_at = cutover.publication.command_issued_at + Duration::milliseconds(1);
    issue_certificate_until(
        &repository,
        &cutover.certificate,
        cutover_issued_at,
        cutover_issued_at + Duration::days(30),
    )
    .await?;
    let applied = cutover_acknowledgement(&cutover.cutover, GatewayAckState::Applied);
    repository
        .project_gateway_acknowledgement(
            &applied,
            applied.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    let restarted_repository = PostgresEdgeRepository::new(executor.clone());
    let stored_cutover = restarted_repository
        .find_gateway_route_cutover(fixture.organization_id, fixture.candidate_deployment_id)
        .await?
        .ok_or("PostgreSQL route cutover disappeared")?;
    assert_eq!(stored_cutover.state, GatewayRouteCutoverState::Applied);
    assert_eq!(
        stored_cutover.candidate_generation,
        fixture.candidate_generation
    );
    for route in stored_cutover.routes {
        let active = restarted_repository
            .find_route(fixture.organization_id, route.id)
            .await?;
        assert_eq!(active, route);
        assert_eq!(
            active.target.workload_revision_id,
            fixture.candidate_revision_id
        );
        assert_eq!(active.target.upstream.as_str(), "http://127.0.0.1:49153/");
        assert_eq!(
            active.target.runtime_generation,
            fixture.candidate_generation
        );
        assert_eq!(
            active.target.runtime_unit_id,
            format!(
                "workload:{}:revision:{}",
                fixture.workload_id, fixture.candidate_revision_id
            )
        );
    }
    super::edge_certificate_lifecycle_support::exercise(
        executor,
        super::edge_certificate_lifecycle_support::GatewayCertificateLifecycleScenario {
            organization_id: fixture.organization_id,
            node_id: fixture.node_id,
            domain_claim,
            started_at: now + Duration::days(24),
        },
    )
    .await?;
    Ok(())
}

async fn verified_claim(
    repository: &PostgresEdgeRepository,
    fixture: &EdgeFixture,
    pattern: &str,
    now: chrono::DateTime<Utc>,
) -> Result<DomainClaim, Box<dyn std::error::Error>> {
    let mut claim = DomainClaim::create(
        DomainClaimId::new(),
        fixture.organization_id,
        fixture.project_id,
        fixture.environment_id,
        DomainNamePattern::parse(pattern)?,
        format!("a3s-cloud-verification={}", Uuid::now_v7()),
        now,
    )?;
    let created = DomainClaimChanged::envelope(&claim, Uuid::now_v7())?;
    repository
        .create_domain_claim(CreateDomainClaimWrite {
            claim: claim.clone(),
            idempotency: IdempotencyRequest::new(
                "postgres-edge-domain-claims",
                claim.id.to_string(),
                pattern.as_bytes(),
            )?,
            event: created,
        })
        .await?;
    let expected_version = claim.aggregate_version;
    claim.verify(now + Duration::milliseconds(1))?;
    let verified = DomainClaimChanged::envelope(&claim, Uuid::now_v7())?;
    repository
        .transition_domain_claim(TransitionDomainClaim {
            claim: claim.clone(),
            expected_version,
            idempotency: IdempotencyRequest::new(
                "postgres-edge-domain-verifications",
                claim.id.to_string(),
                b"verified",
            )?,
            event: verified,
        })
        .await?;
    Ok(claim)
}

async fn issue_certificate(
    repository: &dyn IEdgeRepository,
    certificate: &GatewayCertificate,
    now: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    issue_certificate_until(repository, certificate, now, now + Duration::days(30)).await
}

async fn issue_certificate_until(
    repository: &dyn IEdgeRepository,
    certificate: &GatewayCertificate,
    now: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut issued = certificate.clone();
    let expected_version = issued.aggregate_version;
    issued.record_issued(
        format!("sha256:{}", "b".repeat(64)),
        GatewayCertificateMaterial {
            serial_number: issued.id.to_string(),
            fingerprint: format!("sha256:{}", "a".repeat(64)),
            certificate_pem: "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n"
                .into(),
            ca_bundle_pem: "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n"
                .into(),
            issued_at: now,
            expires_at,
        },
        now,
    )?;
    repository
        .transition_gateway_certificate(issued, expected_version)
        .await?;
    Ok(())
}

fn staged_cutover(
    fixture: &EdgeFixture,
    active_routes: &[Route],
    gateway_revision: u64,
    expected_revision: Option<u64>,
    expected_scope_version: u64,
    staged_at: chrono::DateTime<Utc>,
) -> Result<StageGatewayRouteCutover, Box<dyn std::error::Error>> {
    let certificate_id = GatewayCertificateId::new();
    let command_id = NodeCommandId::new();
    let correlation_id = Uuid::now_v7();
    let mut dns_names = active_routes
        .iter()
        .filter_map(|route| route.domain_pattern.as_ref())
        .map(|pattern| pattern.as_str().to_owned())
        .collect::<Vec<_>>();
    dns_names.sort();
    dns_names.dedup();
    let certificate_request = GatewayCertificateRequest::new(
        certificate_id.as_uuid(),
        dns_names,
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )?;
    let snapshot = GatewaySnapshot::new_with_certificate(
        fixture.node_id.as_uuid(),
        gateway_revision,
        expected_revision,
        staged_at,
        staged_at + Duration::minutes(3),
        format!(
            "# PostgreSQL route cutover {}\nentrypoints \"https\" {{ tls {{ cert_file = \"{}\"; key_file = \"{}\" }} }}\n",
            fixture.candidate_deployment_id,
            certificate_request.certificate_file,
            certificate_request.private_key_file
        ),
        Some(certificate_request.clone()),
    )?;
    let mut candidates = active_routes
        .iter()
        .map(|route| {
            let target = RouteTarget::new(
                fixture.workload_id,
                fixture.candidate_revision_id,
                format!(
                    "workload:{}:revision:{}",
                    fixture.workload_id, fixture.candidate_revision_id
                ),
                fixture.candidate_generation,
                route.target.port_name.clone(),
                UpstreamEndpoint::parse("http://127.0.0.1:49153")?,
                staged_at,
            )?;
            route.prepare_cutover(target, certificate_id, staged_at)
        })
        .collect::<Result<Vec<_>, String>>()?;
    for route in &mut candidates {
        route.stage(
            gateway_revision,
            command_id,
            snapshot.snapshot_digest.clone(),
            staged_at,
        )?;
    }
    let publication = GatewayPublication::stage(
        fixture.node_id,
        command_id,
        correlation_id,
        snapshot,
        staged_at,
        staged_at + Duration::minutes(3),
    )?;
    let mut domain_claim_ids = candidates
        .iter()
        .filter_map(|route| route.domain_claim_id)
        .collect::<Vec<_>>();
    domain_claim_ids.sort();
    domain_claim_ids.dedup();
    let certificate = GatewayCertificate::provision(
        certificate_id,
        fixture.organization_id,
        fixture.node_id,
        domain_claim_ids,
        gateway_revision,
        command_id,
        publication.snapshot_digest.clone(),
        certificate_request,
        staged_at,
    )?;
    let cutover = GatewayRouteCutover::stage(
        fixture.candidate_deployment_id,
        fixture.organization_id,
        fixture.workload_id,
        fixture.revision_id,
        fixture.candidate_revision_id,
        fixture.revision_generation,
        fixture.candidate_generation,
        fixture.node_id,
        gateway_revision,
        command_id,
        certificate_id,
        publication.snapshot_digest.clone(),
        publication.snapshot_expires_at,
        candidates,
        staged_at,
    )?;
    let event = GatewayRouteCutoverStaged::envelope(&cutover, &publication)?;
    Ok(StageGatewayRouteCutover {
        cutover,
        certificate,
        publication,
        expected_scope_version,
        idempotency: IdempotencyRequest::new(
            format!(
                "deployments/{}/route-cutover",
                fixture.candidate_deployment_id
            ),
            "postgres-route-cutover",
            fixture.candidate_revision_id.to_string().as_bytes(),
        )?,
        event,
    })
}

fn cutover_acknowledgement(
    cutover: &GatewayRouteCutover,
    state: GatewayAckState,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: cutover.gateway_command_id.as_uuid(),
        node_id: cutover.node_id.as_uuid(),
        gateway_id: cutover.node_id.as_uuid(),
        revision: cutover.gateway_revision,
        snapshot_digest: cutover.snapshot_digest.clone(),
        expires_at: cutover.snapshot_expires_at,
        state,
        ready: state == GatewayAckState::Applied,
        message: (state == GatewayAckState::Rejected).then(|| "candidate rejected".into()),
        acknowledged_at: cutover.staged_at + Duration::seconds(1),
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    }
}

#[allow(clippy::too_many_arguments)]
fn staged(
    fixture: &EdgeFixture,
    gateway_scope: &GatewayScope,
    revision: u64,
    expected_revision: Option<u64>,
    hostname: &str,
    path: &str,
    idempotency_key: &str,
    now: chrono::DateTime<Utc>,
    domain_claim: &DomainClaim,
) -> Result<StageRoutePublication, Box<dyn std::error::Error>> {
    let command_id = NodeCommandId::new();
    let correlation_id = Uuid::now_v7();
    let certificate_id = GatewayCertificateId::new();
    let certificate_request = GatewayCertificateRequest::new(
        certificate_id.as_uuid(),
        vec![domain_claim.pattern.as_str().into()],
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )?;
    let snapshot = GatewaySnapshot::new_with_certificate(
        fixture.node_id.as_uuid(),
        revision,
        expected_revision,
        now,
        now + Duration::minutes(3),
        format!(
            "# route {hostname}{path}\nentrypoints \"https\" {{ tls {{ cert_file = \"{}\"; key_file = \"{}\" }} }}\n",
            certificate_request.certificate_file, certificate_request.private_key_file
        ),
        Some(certificate_request.clone()),
    )?;
    let mut route = Route::create(
        RouteId::new(),
        fixture.organization_id,
        fixture.project_id,
        fixture.environment_id,
        gateway_scope.id,
        fixture.node_id,
        RouteHostname::parse(hostname)?,
        RoutePath::parse(path)?,
        domain_claim.id,
        domain_claim.pattern.clone(),
        certificate_id,
        fixture.workload_id,
        RouteTarget::new(
            fixture.workload_id,
            fixture.revision_id,
            format!(
                "workload:{}:revision:{}",
                fixture.workload_id, fixture.revision_id
            ),
            fixture.revision_generation,
            RoutePortName::parse("http")?,
            UpstreamEndpoint::parse("http://127.0.0.1:49152")?,
            now,
        )?,
        now,
    )?;
    route.stage(revision, command_id, snapshot.snapshot_digest.clone(), now)?;
    let publication = GatewayPublication::stage(
        fixture.node_id,
        command_id,
        correlation_id,
        snapshot,
        now,
        now + Duration::minutes(3),
    )?;
    let certificate = GatewayCertificate::provision(
        certificate_id,
        fixture.organization_id,
        fixture.node_id,
        vec![domain_claim.id],
        revision,
        command_id,
        publication.snapshot_digest.clone(),
        certificate_request,
        now,
    )?;
    let canonical = format!("{hostname}{path}");
    Ok(StageRoutePublication {
        route: route.clone(),
        gateway_scope: gateway_scope.clone(),
        certificate,
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
