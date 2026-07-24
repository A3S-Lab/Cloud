use super::*;
use crate::modules::edge::domain::events::{GatewayRouteCutoverStaged, GatewayScopeCreated};
use crate::modules::edge::domain::repositories::{
    CreateGatewayScopeWrite, EdgeRoutePublicationResult, GatewayRouteCutoverResult,
    IEdgeRepository, StageGatewayRouteCutover, StageRoutePublication,
};
use crate::modules::edge::domain::{
    DomainNamePattern, GatewayCertificate, GatewayCertificateMaterial, GatewayPublication,
    GatewayRouteCutover, GatewayRouteCutoverState, GatewayScope, Route, RouteHostname, RoutePath,
    RoutePortName, RouteState, RouteTarget, UpstreamEndpoint,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId,
    IdempotencyRequest, NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError, RouteId,
    WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    DomainEventEnvelope, GatewayAckState, GatewayCertificateRequest, GatewaySnapshot,
    NodeGatewayAck,
};
use chrono::{Duration, Utc};
use uuid::Uuid;

#[path = "tests/gateway_rollout_tests.rs"]
mod gateway_rollout_tests;

fn staged(
    node_id: NodeId,
    revision: u64,
    expected_revision: Option<u64>,
    hostname: &str,
    path: &str,
    key: &str,
) -> StageRoutePublication {
    let now = Utc::now();
    let command_id = NodeCommandId::new();
    let correlation_id = Uuid::now_v7();
    let certificate_id = GatewayCertificateId::new();
    let domain_claim_id = DomainClaimId::new();
    let certificate_request = GatewayCertificateRequest::new(
        certificate_id.as_uuid(),
        vec![hostname.to_ascii_lowercase()],
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )
    .expect("certificate request");
    let snapshot = GatewaySnapshot::new_with_certificate(
        node_id.as_uuid(),
        revision,
        expected_revision,
        now,
        now + Duration::minutes(3),
        format!(
            "# {hostname}{path}\nentrypoints \"https\" {{ tls {{ cert_file = \"{}\"; key_file = \"{}\" }} }}\n",
            certificate_request.certificate_file, certificate_request.private_key_file
        ),
        Some(certificate_request.clone()),
    )
    .expect("snapshot");
    let workload_id = WorkloadId::new();
    let workload_revision_id = WorkloadRevisionId::new();
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let gateway_scope = GatewayScope::create(
        GatewayScopeId::new(),
        organization_id,
        project_id,
        environment_id,
        node_id,
        now,
    )
    .expect("Gateway scope");
    let mut route = Route::create(
        RouteId::new(),
        organization_id,
        project_id,
        environment_id,
        gateway_scope.id,
        node_id,
        RouteHostname::parse(hostname).expect("hostname"),
        RoutePath::parse(path).expect("path"),
        domain_claim_id,
        DomainNamePattern::parse(hostname).expect("domain pattern"),
        certificate_id,
        workload_id,
        RouteTarget::new(
            workload_id,
            workload_revision_id,
            format!("workload:{workload_id}:revision:{workload_revision_id}"),
            1,
            RoutePortName::parse("http").expect("port"),
            UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("endpoint"),
            now,
        )
        .expect("target"),
        now,
    )
    .expect("route");
    route
        .stage(revision, command_id, snapshot.snapshot_digest.clone(), now)
        .expect("stage route");
    let publication = GatewayPublication::stage(
        node_id,
        command_id,
        correlation_id,
        snapshot,
        now,
        now + Duration::minutes(3),
    )
    .expect("publication");
    let certificate = GatewayCertificate::provision(
        certificate_id,
        route.organization_id,
        node_id,
        vec![domain_claim_id],
        revision,
        command_id,
        publication.snapshot_digest.clone(),
        certificate_request,
        now,
    )
    .expect("certificate");
    let canonical = format!("{hostname}{path}");
    StageRoutePublication {
        route: route.clone(),
        gateway_scope,
        certificate,
        publication,
        expected_scope_version: 0,
        idempotency: IdempotencyRequest::new("routes", key, canonical.as_bytes())
            .expect("idempotency"),
        event: DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "edge.route.publication-staged".into(),
            schema_version: 1,
            organization_id: route.organization_id.as_uuid(),
            aggregate_id: route.id.as_uuid(),
            aggregate_version: route.aggregate_version,
            occurred_at: now,
            correlation_id,
            causation_id: None,
            payload: serde_json::json!({ "route_id": route.id }),
        },
    }
}

async fn persist_gateway_scope(
    repository: &InMemoryEdgeRepository,
    scope: &GatewayScope,
) -> Result<(), RepositoryError> {
    repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "test-gateway-scopes",
                scope.id.to_string(),
                scope.node_id.to_string().as_bytes(),
            )
            .expect("scope idempotency"),
            event: GatewayScopeCreated::envelope(scope, Uuid::now_v7()).expect("scope event"),
        })
        .await
        .map(|_| ())
}

async fn stage_route(
    repository: &InMemoryEdgeRepository,
    bundle: StageRoutePublication,
) -> Result<EdgeRoutePublicationResult, RepositoryError> {
    persist_gateway_scope(repository, &bundle.gateway_scope).await?;
    repository.stage_route_publication(bundle).await
}

async fn issue(
    repository: &InMemoryEdgeRepository,
    certificate: &GatewayCertificate,
    issued_at: chrono::DateTime<Utc>,
) {
    let mut issued = certificate.clone();
    let expected_version = issued.aggregate_version;
    issued
        .record_issued(
            format!("sha256:{}", "b".repeat(64)),
            GatewayCertificateMaterial {
                serial_number: issued.id.to_string(),
                fingerprint: format!("sha256:{}", "a".repeat(64)),
                certificate_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n".into(),
                ca_bundle_pem:
                    "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n".into(),
                issued_at,
                expires_at: issued_at + Duration::days(30),
            },
            issued_at,
        )
        .expect("record issue");
    repository
        .transition_gateway_certificate(issued, expected_version)
        .await
        .expect("persist issue");
}

fn acknowledgement(
    staged: &crate::modules::edge::domain::repositories::EdgeRoutePublicationResult,
    state: GatewayAckState,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: staged.publication.command_id.as_uuid(),
        node_id: staged.publication.node_id.as_uuid(),
        gateway_id: staged.publication.node_id.as_uuid(),
        revision: staged.publication.revision,
        snapshot_digest: staged.publication.snapshot_digest.clone(),
        expires_at: staged.publication.snapshot_expires_at,
        state,
        ready: state == GatewayAckState::Applied,
        message: (state == GatewayAckState::Rejected).then(|| "invalid snapshot".into()),
        acknowledged_at: staged.publication.command_issued_at + Duration::seconds(1),
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    }
}

fn staged_cutover(
    active_routes: &[Route],
    deployment_id: DeploymentId,
    candidate_revision_id: WorkloadRevisionId,
    gateway_revision: u64,
    expected_revision: Option<u64>,
    expected_scope_version: u64,
    key: &str,
) -> StageGatewayRouteCutover {
    let first = active_routes.first().expect("active route");
    let now = active_routes
        .iter()
        .map(|route| route.updated_at)
        .max()
        .unwrap_or_else(Utc::now)
        + Duration::milliseconds(1);
    let node_id = first.gateway_node_id;
    let certificate_id = GatewayCertificateId::new();
    let command_id = NodeCommandId::new();
    let correlation_id = Uuid::now_v7();
    let certificate_request = GatewayCertificateRequest::new(
        certificate_id.as_uuid(),
        vec![first.hostname.as_str().to_owned()],
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/certificate.pem"),
        format!("/var/lib/a3s-cloud/gateway/certificates/{certificate_id}/private-key.pem"),
    )
    .expect("certificate request");
    let snapshot = GatewaySnapshot::new_with_certificate(
        node_id.as_uuid(),
        gateway_revision,
        expected_revision,
        now,
        now + Duration::minutes(3),
        format!(
            "# cutover {deployment_id}\nentrypoints \"https\" {{ tls {{ cert_file = \"{}\"; key_file = \"{}\" }} }}\n",
            certificate_request.certificate_file, certificate_request.private_key_file
        ),
        Some(certificate_request.clone()),
    )
    .expect("snapshot");
    let candidate_generation = first.target.runtime_generation + 1;
    let mut candidates = active_routes
        .iter()
        .map(|route| {
            let target = RouteTarget::new(
                route.workload_id,
                candidate_revision_id,
                format!(
                    "workload:{}:revision:{candidate_revision_id}",
                    route.workload_id
                ),
                candidate_generation,
                route.target.port_name.clone(),
                UpstreamEndpoint::parse("http://127.0.0.1:49153").expect("candidate endpoint"),
                now,
            )
            .expect("candidate target");
            route
                .prepare_cutover(target, certificate_id, now)
                .expect("prepare cutover")
        })
        .collect::<Vec<_>>();
    for route in &mut candidates {
        route
            .stage(
                gateway_revision,
                command_id,
                snapshot.snapshot_digest.clone(),
                now,
            )
            .expect("stage candidate");
    }
    let publication = GatewayPublication::stage(
        node_id,
        command_id,
        correlation_id,
        snapshot,
        now,
        now + Duration::minutes(3),
    )
    .expect("publication");
    let mut domain_claim_ids = candidates
        .iter()
        .filter_map(|route| route.domain_claim_id)
        .collect::<Vec<_>>();
    domain_claim_ids.sort();
    domain_claim_ids.dedup();
    let certificate = GatewayCertificate::provision(
        certificate_id,
        first.organization_id,
        node_id,
        domain_claim_ids,
        gateway_revision,
        command_id,
        publication.snapshot_digest.clone(),
        certificate_request,
        now,
    )
    .expect("certificate");
    let cutover = GatewayRouteCutover::stage(
        deployment_id,
        first.organization_id,
        first.workload_id,
        first.target.workload_revision_id,
        candidate_revision_id,
        first.target.runtime_generation,
        candidate_generation,
        node_id,
        gateway_revision,
        command_id,
        certificate_id,
        publication.snapshot_digest.clone(),
        publication.snapshot_expires_at,
        candidates,
        now,
    )
    .expect("cutover");
    let event = GatewayRouteCutoverStaged::envelope(&cutover, &publication).expect("cutover event");
    StageGatewayRouteCutover {
        cutover,
        certificate,
        publication,
        expected_scope_version,
        idempotency: IdempotencyRequest::new(
            format!("deployments/{deployment_id}/route-cutover"),
            key,
            candidate_revision_id.to_string().as_bytes(),
        )
        .expect("idempotency"),
        event,
    }
}

fn cutover_acknowledgement(
    staged: &GatewayRouteCutoverResult,
    state: GatewayAckState,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: staged.publication.command_id.as_uuid(),
        node_id: staged.publication.node_id.as_uuid(),
        gateway_id: staged.publication.node_id.as_uuid(),
        revision: staged.publication.revision,
        snapshot_digest: staged.publication.snapshot_digest.clone(),
        expires_at: staged.publication.snapshot_expires_at,
        state,
        ready: state == GatewayAckState::Applied,
        message: (state == GatewayAckState::Rejected).then(|| "candidate rejected".into()),
        acknowledged_at: staged.publication.command_issued_at + Duration::seconds(1),
        management_protocol: Some(a3s_cloud_contracts::GatewayManagementProtocol::advertised_v1()),
    }
}

#[tokio::test]
async fn creates_and_lists_environment_gateway_scopes_idempotently() {
    let repository = InMemoryEdgeRepository::new();
    let now = Utc::now();
    let primary_node_id = NodeId::new();
    let secondary_node_id = NodeId::new();
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        primary_node_id,
        vec![secondary_node_id, primary_node_id],
        crate::modules::edge::domain::GatewayRolloutPolicy::new(1, 1, 2).expect("rollout policy"),
        now,
    )
    .expect("replicated Gateway scope");
    let idempotency = IdempotencyRequest::new(
        "gateway-scopes",
        "bind-primary",
        serde_json::to_string(&scope.member_node_ids)
            .expect("member identities")
            .as_bytes(),
    )
    .expect("idempotency");
    let bundle = CreateGatewayScopeWrite {
        scope: scope.clone(),
        idempotency: idempotency.clone(),
        event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7()).expect("event"),
    };
    let first = repository
        .create_gateway_scope(bundle.clone())
        .await
        .expect("create scope");
    let replay = repository
        .create_gateway_scope(bundle)
        .await
        .expect("replay scope");
    assert!(!first.replayed);
    assert!(replay.replayed);
    assert_eq!(replay.value, scope);
    assert_eq!(
        repository
            .find_gateway_scope(scope.organization_id, scope.id)
            .await
            .expect("find scope"),
        scope
    );
    assert_eq!(
        repository
            .list_gateway_scopes(
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
            )
            .await
            .expect("list scopes"),
        vec![scope.clone()]
    );
    assert!(repository
        .find_gateway_scope(OrganizationId::new(), scope.id)
        .await
        .is_err());

    let duplicate = GatewayScope::create(
        GatewayScopeId::new(),
        scope.organization_id,
        scope.project_id,
        scope.environment_id,
        secondary_node_id,
        now + Duration::seconds(1),
    )
    .expect("duplicate Gateway scope");
    let duplicate_result = repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: duplicate.clone(),
            idempotency: IdempotencyRequest::new(
                "gateway-scopes",
                "duplicate-binding",
                duplicate.id.to_string().as_bytes(),
            )
            .expect("duplicate idempotency"),
            event: GatewayScopeCreated::envelope(&duplicate, Uuid::now_v7())
                .expect("duplicate event"),
        })
        .await;
    assert!(matches!(
        duplicate_result,
        Err(RepositoryError::Conflict(_))
    ));

    let changed_scope = GatewayScope::create(
        GatewayScopeId::new(),
        scope.organization_id,
        scope.project_id,
        scope.environment_id,
        NodeId::new(),
        now,
    )
    .expect("changed Gateway scope");
    let changed_request = repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: changed_scope.clone(),
            idempotency: IdempotencyRequest::new(
                idempotency.scope,
                idempotency.key,
                b"different-node",
            )
            .expect("changed idempotency"),
            event: GatewayScopeCreated::envelope(&changed_scope, Uuid::now_v7())
                .expect("changed event"),
        })
        .await;
    assert!(matches!(
        changed_request,
        Err(RepositoryError::IdempotencyConflict)
    ));
    assert_eq!(
        repository
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "edge.gateway-scope.created")
            .count(),
        1
    );
}

#[tokio::test]
async fn rejects_route_publication_without_the_authoritative_logical_scope() {
    let repository = InMemoryEdgeRepository::new();
    let bundle = staged(
        NodeId::new(),
        1,
        None,
        "unbound.example.com",
        "/",
        "unbound",
    );
    assert!(matches!(
        repository.stage_route_publication(bundle).await,
        Err(RepositoryError::NotFound)
    ));
}

#[tokio::test]
async fn enforces_one_owner_for_hostname_path_inside_gateway_scope() {
    let repository = InMemoryEdgeRepository::new();
    let node_id = NodeId::new();
    let first = staged(node_id, 1, None, "api.example.com", "/v1", "first");
    let stored = stage_route(&repository, first).await.expect("first route");
    issue(
        &repository,
        &stored.certificate,
        stored.publication.command_issued_at + Duration::milliseconds(1),
    )
    .await;
    repository
        .project_gateway_acknowledgement(
            &acknowledgement(&stored, GatewayAckState::Applied),
            stored.publication.command_issued_at + Duration::seconds(2),
        )
        .await
        .expect("acknowledge first route");
    let mut duplicate = staged(node_id, 2, Some(1), "API.EXAMPLE.COM", "/v1", "duplicate");
    duplicate.expected_scope_version = 2;
    assert!(stage_route(&repository, duplicate).await.is_err());

    let other_scope = staged(
        NodeId::new(),
        1,
        None,
        "api.example.com",
        "/v1",
        "other-scope",
    );
    stage_route(&repository, other_scope)
        .await
        .expect("same tuple in another Gateway scope");
}

#[tokio::test]
async fn serializes_complete_snapshots_and_projects_exact_acknowledgements() {
    let repository = InMemoryEdgeRepository::new();
    let node_id = NodeId::new();
    let first = stage_route(
        &repository,
        staged(node_id, 1, None, "api.example.com", "/v1", "first"),
    )
    .await
    .expect("stage first");
    let second_pending = staged(node_id, 2, None, "web.example.com", "/", "second-pending");
    assert!(stage_route(&repository, second_pending).await.is_err());

    let rejected = acknowledgement(&first, GatewayAckState::Rejected);
    assert!(repository
        .project_gateway_acknowledgement(&rejected, rejected.acknowledged_at + Duration::seconds(1))
        .await
        .expect("reject publication"));
    let stored = repository
        .find_route(first.route.organization_id, first.route.id)
        .await
        .expect("route");
    assert_eq!(stored.state, RouteState::Rejected);
    let scope = repository.gateway_scope(node_id).await.expect("scope");
    assert_eq!(scope.last_issued_revision, 1);
    assert_eq!(scope.installed_revision, None);
    assert_eq!(scope.aggregate_version, 1);

    let mut second = staged(
        node_id,
        2,
        None,
        "api.example.com",
        "/v1",
        "republish-rejected",
    );
    second.expected_scope_version = 1;
    let second = stage_route(&repository, second)
        .await
        .expect("republish ownership released by rejection");
    issue(
        &repository,
        &second.certificate,
        second.publication.command_issued_at + Duration::milliseconds(1),
    )
    .await;
    let applied = acknowledgement(&second, GatewayAckState::Applied);
    repository
        .project_gateway_acknowledgement(&applied, applied.acknowledged_at + Duration::seconds(1))
        .await
        .expect("apply publication");
    let scope = repository.gateway_scope(node_id).await.expect("scope");
    assert_eq!(scope.last_issued_revision, 2);
    assert_eq!(scope.installed_revision, Some(2));
    assert_eq!(scope.aggregate_version, 3);
}

#[tokio::test]
async fn persists_only_closed_gateway_certificate_transitions() {
    let repository = InMemoryEdgeRepository::new();
    let failed = stage_route(
        &repository,
        staged(
            NodeId::new(),
            1,
            None,
            "failed.example.com",
            "/",
            "failed-certificate",
        ),
    )
    .await
    .expect("stage failed certificate");
    let mut failed_certificate = failed.certificate.clone();
    let failed_version = failed_certificate.aggregate_version;
    failed_certificate
        .fail_provisioning(
            format!("sha256:{}", "c".repeat(64)),
            "certificate authority unavailable",
            failed.publication.command_issued_at + Duration::milliseconds(1),
        )
        .expect("fail provisioning");
    repository
        .transition_gateway_certificate(failed_certificate.clone(), failed_version)
        .await
        .expect("persist provisioning failure");
    assert_eq!(
        repository
            .find_gateway_certificate(failed_certificate.node_id, failed_certificate.id)
            .await
            .expect("failed certificate")
            .state,
        crate::modules::edge::domain::GatewayCertificateState::Failed
    );

    let ready = stage_route(
        &repository,
        staged(
            NodeId::new(),
            1,
            None,
            "ready.example.com",
            "/",
            "ready-certificate",
        ),
    )
    .await
    .expect("stage ready certificate");
    issue(
        &repository,
        &ready.certificate,
        ready.publication.command_issued_at + Duration::milliseconds(1),
    )
    .await;
    let applied = acknowledgement(&ready, GatewayAckState::Applied);
    repository
        .project_gateway_acknowledgement(
            &applied,
            applied.acknowledged_at + Duration::milliseconds(1),
        )
        .await
        .expect("ready certificate");
    let mut revoked = repository
        .find_gateway_certificate(ready.certificate.node_id, ready.certificate.id)
        .await
        .expect("ready certificate");
    let ready_version = revoked.aggregate_version;
    revoked
        .revoke(
            "domain ownership removed",
            applied.acknowledged_at + Duration::seconds(1),
        )
        .expect("revoke ready certificate");
    repository
        .transition_gateway_certificate(revoked.clone(), ready_version)
        .await
        .expect("persist revocation");
    assert_eq!(
        repository
            .find_gateway_certificate(revoked.node_id, revoked.id)
            .await
            .expect("revoked certificate")
            .state,
        crate::modules::edge::domain::GatewayCertificateState::Revoked
    );
}

#[tokio::test]
async fn route_cutover_preserves_the_active_target_until_exact_applied_acknowledgement() {
    let repository = InMemoryEdgeRepository::new();
    let node_id = NodeId::new();
    let first = stage_route(
        &repository,
        staged(node_id, 1, None, "update.example.com", "/", "update-first"),
    )
    .await
    .expect("stage initial route");
    issue(
        &repository,
        &first.certificate,
        first.publication.command_issued_at + Duration::milliseconds(1),
    )
    .await;
    let initial_ack = acknowledgement(&first, GatewayAckState::Applied);
    repository
        .project_gateway_acknowledgement(
            &initial_ack,
            initial_ack.acknowledged_at + Duration::milliseconds(1),
        )
        .await
        .expect("activate initial route");
    let active = repository
        .find_route(first.route.organization_id, first.route.id)
        .await
        .expect("active route");
    let previous_revision_id = active.target.workload_revision_id;
    let previous_generation = active.target.runtime_generation;
    let previous_upstream = active.target.upstream.clone();
    let candidate_revision_id = WorkloadRevisionId::new();
    let cutover = repository
        .stage_gateway_route_cutover(staged_cutover(
            std::slice::from_ref(&active),
            DeploymentId::new(),
            candidate_revision_id,
            2,
            Some(1),
            2,
            "update-cutover",
        ))
        .await
        .expect("stage cutover");

    let pending_route = repository
        .find_route(first.route.organization_id, first.route.id)
        .await
        .expect("pending serving route");
    assert_eq!(
        pending_route.target.workload_revision_id,
        previous_revision_id
    );
    assert_eq!(pending_route.target.runtime_generation, previous_generation);
    assert_eq!(pending_route.target.upstream, previous_upstream);
    assert_eq!(
        repository
            .find_gateway_route_cutover(
                cutover.cutover.organization_id,
                cutover.cutover.deployment_id,
            )
            .await
            .expect("cutover query")
            .expect("cutover")
            .state,
        GatewayRouteCutoverState::Pending
    );

    issue(
        &repository,
        &cutover.certificate,
        cutover.publication.command_issued_at + Duration::milliseconds(1),
    )
    .await;
    let mut wrong = cutover_acknowledgement(&cutover, GatewayAckState::Applied);
    wrong.snapshot_digest = format!("sha256:{}", "f".repeat(64));
    assert!(repository
        .project_gateway_acknowledgement(&wrong, wrong.acknowledged_at)
        .await
        .is_err());
    let applied = cutover_acknowledgement(&cutover, GatewayAckState::Applied);
    repository
        .project_gateway_acknowledgement(
            &applied,
            applied.acknowledged_at + Duration::milliseconds(1),
        )
        .await
        .expect("apply exact cutover");

    let updated = repository
        .find_route(first.route.organization_id, first.route.id)
        .await
        .expect("updated route");
    assert_eq!(updated.target.workload_revision_id, candidate_revision_id);
    assert_eq!(
        updated.target.runtime_generation,
        cutover.cutover.candidate_generation
    );
    assert_eq!(updated.target.upstream.as_str(), "http://127.0.0.1:49153/");
    assert_eq!(updated.gateway_revision, Some(2));
    assert_eq!(
        repository
            .find_gateway_route_cutover(
                cutover.cutover.organization_id,
                cutover.cutover.deployment_id,
            )
            .await
            .expect("cutover query")
            .expect("cutover")
            .state,
        GatewayRouteCutoverState::Applied
    );
}

#[tokio::test]
async fn rejected_route_cutover_keeps_the_previous_route_authoritative() {
    let repository = InMemoryEdgeRepository::new();
    let node_id = NodeId::new();
    let first = stage_route(
        &repository,
        staged(
            node_id,
            1,
            None,
            "reject-update.example.com",
            "/",
            "reject-update-first",
        ),
    )
    .await
    .expect("stage initial route");
    issue(
        &repository,
        &first.certificate,
        first.publication.command_issued_at + Duration::milliseconds(1),
    )
    .await;
    let initial_ack = acknowledgement(&first, GatewayAckState::Applied);
    repository
        .project_gateway_acknowledgement(
            &initial_ack,
            initial_ack.acknowledged_at + Duration::milliseconds(1),
        )
        .await
        .expect("activate initial route");
    let active = repository
        .find_route(first.route.organization_id, first.route.id)
        .await
        .expect("active route");
    let previous_generation = active.target.runtime_generation;
    let cutover = repository
        .stage_gateway_route_cutover(staged_cutover(
            std::slice::from_ref(&active),
            DeploymentId::new(),
            WorkloadRevisionId::new(),
            2,
            Some(1),
            2,
            "reject-update-cutover",
        ))
        .await
        .expect("stage cutover");
    let rejected = cutover_acknowledgement(&cutover, GatewayAckState::Rejected);
    repository
        .project_gateway_acknowledgement(
            &rejected,
            rejected.acknowledged_at + Duration::milliseconds(1),
        )
        .await
        .expect("reject cutover");

    assert_eq!(
        repository
            .find_route(first.route.organization_id, first.route.id)
            .await
            .expect("serving route"),
        active
    );
    let rejected_cutover = repository
        .find_gateway_route_cutover(
            cutover.cutover.organization_id,
            cutover.cutover.deployment_id,
        )
        .await
        .expect("cutover query")
        .expect("cutover");
    assert_eq!(
        repository
            .find_route(first.route.organization_id, first.route.id)
            .await
            .expect("serving route")
            .target
            .runtime_generation,
        previous_generation
    );
    assert_eq!(
        rejected_cutover.routes[0].target.runtime_generation,
        cutover.cutover.candidate_generation
    );
    assert_eq!(rejected_cutover.state, GatewayRouteCutoverState::Rejected);
}
