use super::*;
use crate::modules::edge::domain::events::GatewayRouteCutoverStaged;
use crate::modules::edge::domain::repositories::{
    GatewayRouteCutoverResult, IEdgeRepository, StageGatewayRouteCutover, StageRoutePublication,
};
use crate::modules::edge::domain::{
    DomainNamePattern, GatewayCertificate, GatewayCertificateMaterial, GatewayPublication,
    GatewayRouteCutover, GatewayRouteCutoverState, Route, RouteHostname, RoutePath, RoutePortName,
    RouteState, UpstreamEndpoint,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId, IdempotencyRequest,
    NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    DomainEventEnvelope, GatewayAckState, GatewayCertificateRequest, GatewaySnapshot,
    NodeGatewayAck,
};
use chrono::{Duration, Utc};
use uuid::Uuid;

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
    let mut route = Route::create(
        RouteId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        node_id,
        RouteHostname::parse(hostname).expect("hostname"),
        RoutePath::parse(path).expect("path"),
        domain_claim_id,
        DomainNamePattern::parse(hostname).expect("domain pattern"),
        certificate_id,
        WorkloadId::new(),
        WorkloadRevisionId::new(),
        RoutePortName::parse("http").expect("port"),
        UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("endpoint"),
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
    let mut candidates = active_routes
        .iter()
        .map(|route| {
            route
                .prepare_cutover(
                    candidate_revision_id,
                    UpstreamEndpoint::parse("http://127.0.0.1:49153").expect("candidate endpoint"),
                    certificate_id,
                    now,
                )
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
        first.workload_revision_id,
        candidate_revision_id,
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
    }
}

#[tokio::test]
async fn enforces_one_owner_for_hostname_path_inside_gateway_scope() {
    let repository = InMemoryEdgeRepository::new();
    let node_id = NodeId::new();
    let first = staged(node_id, 1, None, "api.example.com", "/v1", "first");
    let stored = repository
        .stage_route_publication(first)
        .await
        .expect("first route");
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
    assert!(repository.stage_route_publication(duplicate).await.is_err());

    let other_scope = staged(
        NodeId::new(),
        1,
        None,
        "api.example.com",
        "/v1",
        "other-scope",
    );
    repository
        .stage_route_publication(other_scope)
        .await
        .expect("same tuple in another Gateway scope");
}

#[tokio::test]
async fn serializes_complete_snapshots_and_projects_exact_acknowledgements() {
    let repository = InMemoryEdgeRepository::new();
    let node_id = NodeId::new();
    let first = repository
        .stage_route_publication(staged(node_id, 1, None, "api.example.com", "/v1", "first"))
        .await
        .expect("stage first");
    let second_pending = staged(node_id, 2, None, "web.example.com", "/", "second-pending");
    assert!(repository
        .stage_route_publication(second_pending)
        .await
        .is_err());

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
    let second = repository
        .stage_route_publication(second)
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
    let failed = repository
        .stage_route_publication(staged(
            NodeId::new(),
            1,
            None,
            "failed.example.com",
            "/",
            "failed-certificate",
        ))
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

    let ready = repository
        .stage_route_publication(staged(
            NodeId::new(),
            1,
            None,
            "ready.example.com",
            "/",
            "ready-certificate",
        ))
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
    let first = repository
        .stage_route_publication(staged(
            node_id,
            1,
            None,
            "update.example.com",
            "/",
            "update-first",
        ))
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
    let previous_revision_id = active.workload_revision_id;
    let previous_upstream = active.upstream.clone();
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
    assert_eq!(pending_route.workload_revision_id, previous_revision_id);
    assert_eq!(pending_route.upstream, previous_upstream);
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
    assert_eq!(updated.workload_revision_id, candidate_revision_id);
    assert_eq!(updated.upstream.as_str(), "http://127.0.0.1:49153/");
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
    let first = repository
        .stage_route_publication(staged(
            node_id,
            1,
            None,
            "reject-update.example.com",
            "/",
            "reject-update-first",
        ))
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
        GatewayRouteCutoverState::Rejected
    );
}
