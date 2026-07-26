use a3s_cloud_contracts::{
    GatewayAckState, GatewayManagementProtocol, GatewaySnapshot, NodeGatewayAck,
};
use a3s_cloud_control_plane::modules::edge::domain::events::{
    DomainClaimChanged, GatewayRolloutStaged, GatewayScopeCreated,
};
use a3s_cloud_control_plane::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, IEdgeRepository, StageGatewayRollout,
    TransitionDomainClaim,
};
use a3s_cloud_control_plane::modules::edge::domain::services::{
    ResolvedRouteTarget, ResolvedRouteTargetSet,
};
use a3s_cloud_control_plane::modules::edge::infrastructure::{
    persistence::PostgresEdgeRepository, CompileGatewayRouteRollout, GatewayMemberSnapshotContext,
    GatewayRolloutRollbackCompiler, GatewayRolloutRollbackReconciler, GatewayRouteRolloutCompiler,
    GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
};
use a3s_cloud_control_plane::modules::edge::{
    DomainClaim, DomainNamePattern, FleetGatewayCommandQueue, GatewayCertificate,
    GatewayCertificateMaterial, GatewayCertificateState, GatewayPublication, GatewayRollout,
    GatewayRolloutPolicy, GatewayRolloutReconciler, GatewayRolloutRollbackState,
    GatewayRolloutState, GatewayScope, RouteHostname, RoutePath, RoutePortName, RouteState,
    RouteTarget, UpstreamEndpoint,
};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayRolloutId, GatewayScopeId, IdempotencyRequest,
    NodeCommandId, NodeId, OrganizationId, ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

#[path = "gateway_rollouts/certificate_convergence.rs"]
mod certificate_convergence;
#[path = "gateway_rollouts/certificate_convergence_failures.rs"]
mod certificate_convergence_failures;
#[path = "gateway_rollouts/rollback_failures.rs"]
mod rollback_failures;

#[derive(Debug, Clone, Copy)]
pub struct GatewayRolloutFixture {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub workload_revision_generation: u64,
}

pub async fn exercise_replicated_gateway_rollout(
    executor: &PostgresExecutor,
    fixture: GatewayRolloutFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = PostgresEdgeRepository::new(executor.clone());
    let database = Database::new(PostgresDialect, executor.clone());
    let now = Utc::now();
    let members = [NodeId::new(), NodeId::new(), NodeId::new()];
    let route_members = [NodeId::new(), NodeId::new()];
    let conflict_members = [NodeId::new(), NodeId::new()];
    let rebind_members = [NodeId::new(), NodeId::new()];
    let rollback_members = [NodeId::new(), NodeId::new()];
    let rejected_rollback_members = [NodeId::new(), NodeId::new()];
    let unavailable_rollback_members = [NodeId::new(), NodeId::new()];
    let certificate_convergence_members = [NodeId::new(), NodeId::new()];
    let rejected_convergence_members = [NodeId::new(), NodeId::new()];
    let unavailable_convergence_members = [NodeId::new(), NodeId::new()];
    for (ordinal, node_id) in members
        .iter()
        .chain(route_members.iter())
        .chain(conflict_members.iter())
        .chain(rebind_members.iter())
        .chain(rollback_members.iter())
        .chain(rejected_rollback_members.iter())
        .chain(unavailable_rollback_members.iter())
        .chain(certificate_convergence_members.iter())
        .chain(rejected_convergence_members.iter())
        .chain(unavailable_convergence_members.iter())
        .enumerate()
    {
        let name = format!("Gateway rollout fixture {}", ordinal + 1);
        let name_key = format!("gateway-rollout-fixture-{}", ordinal + 1);
        database
            .execute(
                sql_query::<()>(
                    "insert into nodes (organization_id, id, name, name_key, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, aggregate_version) values (",
                )
                .bind(fixture.organization_id.as_uuid())
                .append(", ")
                .bind(node_id.as_uuid())
                .append(", ")
                .bind(name)
                .append(", ")
                .bind(name_key)
                .append(", 'ready', ")
                .bind(Uuid::now_v7())
                .append(", 'test', 'test-runtime', 'gateway-rollout-test', ")
                .bind(format!("sha256:{}", "a".repeat(64)))
                .append(", ")
                .bind(serde_json::json!({}))
                .append(", ")
                .bind(now)
                .append(", ")
                .bind(now)
                .append(", 1)"),
            )
            .await?;
    }

    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        fixture.organization_id,
        fixture.project_id,
        fixture.environment_id,
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(2, 1, members.len())?,
        now,
    )?;
    let create_scope = CreateGatewayScopeWrite {
        scope: scope.clone(),
        idempotency: IdempotencyRequest::new(
            "postgres-gateway-rollout-scopes",
            scope.id.to_string(),
            serde_json::to_vec(&scope.member_node_ids)?.as_slice(),
        )?,
        event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())?,
    };
    let created_scope = repository
        .create_gateway_scope(create_scope.clone())
        .await?;
    let replayed_scope = repository.create_gateway_scope(create_scope).await?;
    assert!(!created_scope.replayed);
    assert!(replayed_scope.replayed);
    assert_eq!(created_scope.value, scope);
    assert_eq!(
        repository
            .find_gateway_scope(fixture.organization_id, scope.id)
            .await?,
        scope
    );

    let correlation_id = Uuid::now_v7();
    let publications = members
        .iter()
        .map(|node_id| publication(*node_id, correlation_id, now))
        .collect::<Result<Vec<_>, _>>()?;
    let rollout = GatewayRollout::stage(GatewayRolloutId::new(), &scope, 1, &publications, now)?;
    let stage = StageGatewayRollout {
        scope: scope.clone(),
        rollout: rollout.clone(),
        route_replicas: Vec::new(),
        publications: publications.clone(),
        certificates: Vec::new(),
        expected_scope_versions: members
            .iter()
            .map(|node_id| (*node_id, 0))
            .collect::<BTreeMap<_, _>>(),
        idempotency: IdempotencyRequest::new(
            format!("gateway-scopes/{}/rollouts", scope.id),
            "postgres-replicated-rollout",
            rollout.id.to_string().as_bytes(),
        )?,
        event: GatewayRolloutStaged::envelope(&scope, &rollout)?,
        route_event: None,
    };
    let staged = repository.stage_gateway_rollout(stage.clone()).await?;
    let replayed = repository.stage_gateway_rollout(stage).await?;
    assert!(!staged.replayed);
    assert!(replayed.replayed);
    assert_eq!(staged.rollout, rollout);

    let restarted = PostgresEdgeRepository::new(executor.clone());
    assert_eq!(
        restarted
            .find_gateway_rollout(fixture.organization_id, rollout.id)
            .await?,
        rollout
    );
    let dispatches = restarted.pending_gateway_rollout_dispatches(10).await?;
    assert_eq!(dispatches.len(), 1);
    dispatches[0].validate()?;
    assert_eq!(dispatches[0].rollout, rollout);
    assert_eq!(dispatches[0].publications.len(), 3);
    let node_commands: Arc<dyn INodeControlRepository> =
        Arc::new(PostgresNodeRepository::new(executor.clone()));
    let commands = Arc::new(FleetGatewayCommandQueue::new(Arc::clone(&node_commands)));
    let dispatch_repository: Arc<dyn IEdgeRepository> =
        Arc::new(PostgresEdgeRepository::new(executor.clone()));
    let dispatched = GatewayRolloutReconciler::new(
        Arc::clone(&dispatch_repository),
        commands.clone(),
        std::time::Duration::from_secs(1),
        10,
    )?
    .run_once(now + Duration::milliseconds(1))
    .await?;
    assert_eq!(dispatched.dispatched_commands, 3);
    assert_eq!(dispatched.replayed_commands, 0);
    assert!(dispatched.failures.is_empty());
    let replayed = GatewayRolloutReconciler::new(
        dispatch_repository,
        commands,
        std::time::Duration::from_secs(1),
        10,
    )?
    .run_once(now + Duration::milliseconds(2))
    .await?;
    assert_eq!(replayed.dispatched_commands, 3);
    assert_eq!(replayed.replayed_commands, 3);
    assert!(replayed.failures.is_empty());
    for publication in &publications {
        let command = node_commands
            .find_command(publication.node_id, publication.command_id)
            .await?
            .ok_or("Gateway rollout command was not durably enqueued")?;
        assert_eq!(command.id, publication.command_id);
        assert_eq!(command.node_id, publication.node_id);
    }

    for (index, publication) in publications.iter().take(2).enumerate() {
        let acknowledged_at = now + Duration::seconds(i64::try_from(index + 1)?);
        assert!(
            restarted
                .project_gateway_acknowledgement(
                    &acknowledgement(publication, acknowledged_at),
                    acknowledged_at + Duration::milliseconds(1),
                )
                .await?
        );
    }
    let ready = restarted
        .find_gateway_rollout(fixture.organization_id, rollout.id)
        .await?;
    assert_eq!(ready.state, GatewayRolloutState::Ready);
    assert_eq!(ready.ready_replicas, 2);
    assert_eq!(ready.unavailable_replicas, 0);
    assert!(ready.serves_traffic()?);
    assert_eq!(
        restarted.pending_gateway_rollout_dispatches(10).await?[0].publications,
        vec![publications[2].clone()]
    );

    let unavailable_at = publications[2].command_not_after + Duration::seconds(1);
    let degraded = restarted
        .mark_gateway_rollout_replica_unavailable(
            fixture.organization_id,
            rollout.id,
            members[2],
            ready.aggregate_version,
            "Gateway missed the rollout readiness deadline",
            unavailable_at,
        )
        .await?;
    assert_eq!(degraded.state, GatewayRolloutState::Degraded);
    assert_eq!(degraded.ready_replicas, 2);
    assert_eq!(degraded.unavailable_replicas, 1);
    assert!(degraded.completed_at.is_some());
    assert!(degraded.serves_traffic()?);
    assert!(restarted
        .pending_gateway_rollout_dispatches(10)
        .await?
        .is_empty());
    let unavailable_publication: (String, Option<String>, Option<chrono::DateTime<Utc>>) = database
        .fetch_one_as(
            sql_query::<(String, Option<String>, Option<chrono::DateTime<Utc>>)>(
                "select state, failure, acknowledged_at from gateway_publications where node_id = ",
            )
            .bind(publications[2].node_id.as_uuid())
            .append(" and revision = ")
            .bind(publications[2].revision),
        )
        .await?;
    assert_eq!(unavailable_publication.0, "unavailable");
    assert_eq!(
        unavailable_publication.1.as_deref(),
        Some("Gateway missed the rollout readiness deadline")
    );
    assert_eq!(unavailable_publication.2, Some(unavailable_at));
    assert!(
        database
            .execute(
                sql_query::<()>(
                    "update gateway_rollouts set state = 'ready', completed_at = null where id = "
                )
                .bind(rollout.id.as_uuid()),
            )
            .await
            .is_err(),
        "a fully terminal rollout must not satisfy the ready-state constraint"
    );
    assert_eq!(
        PostgresEdgeRepository::new(executor.clone())
            .find_gateway_rollout(fixture.organization_id, rollout.id)
            .await?,
        degraded
    );

    assert!(restarted
        .project_gateway_acknowledgement(
            &acknowledgement(&publications[2], unavailable_at + Duration::seconds(1)),
            unavailable_at + Duration::seconds(1) + Duration::milliseconds(1),
        )
        .await
        .is_err());
    assert_eq!(
        restarted
            .find_gateway_rollout(fixture.organization_id, rollout.id)
            .await?,
        degraded
    );

    exercise_atomic_route_rollout_staging(
        &restarted,
        &database,
        executor,
        &fixture,
        route_members,
        conflict_members,
        rebind_members,
        rollback_members,
        rejected_rollback_members,
        unavailable_rollback_members,
        certificate_convergence_members,
        rejected_convergence_members,
        unavailable_convergence_members,
        now + Duration::seconds(5),
    )
    .await?;
    Ok(())
}

#[path = "gateway_rollouts/staging.rs"]
mod staging;
use staging::exercise_atomic_route_rollout_staging;

#[path = "gateway_rollouts/cutover.rs"]
mod cutover;
use cutover::{exercise_exact_route_rollback, exercise_retained_route_rebinding};

async fn issue_certificate(
    repository: &PostgresEdgeRepository,
    certificate: &GatewayCertificate,
    issued_at: chrono::DateTime<Utc>,
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
            issued_at,
            expires_at: issued_at + Duration::days(30),
        },
        issued_at,
    )?;
    repository
        .transition_gateway_certificate(issued, expected_version)
        .await?;
    Ok(())
}

async fn verified_claim(
    repository: &PostgresEdgeRepository,
    fixture: &GatewayRolloutFixture,
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
    repository
        .create_domain_claim(CreateDomainClaimWrite {
            claim: claim.clone(),
            idempotency: IdempotencyRequest::new(
                "postgres-route-rollout-domain-claims",
                claim.id.to_string(),
                pattern.as_bytes(),
            )?,
            event: DomainClaimChanged::envelope(&claim, Uuid::now_v7())?,
        })
        .await?;
    let expected_version = claim.aggregate_version;
    claim.verify(now + Duration::microseconds(1))?;
    repository
        .transition_domain_claim(TransitionDomainClaim {
            claim: claim.clone(),
            expected_version,
            idempotency: IdempotencyRequest::new(
                "postgres-route-rollout-domain-verifications",
                claim.id.to_string(),
                b"verified",
            )?,
            event: DomainClaimChanged::envelope(&claim, Uuid::now_v7())?,
        })
        .await?;
    Ok(claim)
}

async fn persisted_route_scope(
    repository: &PostgresEdgeRepository,
    fixture: &GatewayRolloutFixture,
    members: [NodeId; 2],
    key: &str,
    now: chrono::DateTime<Utc>,
) -> Result<GatewayScope, Box<dyn std::error::Error>> {
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        fixture.organization_id,
        fixture.project_id,
        fixture.environment_id,
        members[0],
        members.to_vec(),
        GatewayRolloutPolicy::new(1, 1, members.len())?,
        now,
    )?;
    repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "postgres-route-rollout-scopes",
                key,
                serde_json::to_vec(&scope.member_node_ids)?.as_slice(),
            )?,
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7())?,
        })
        .await?;
    Ok(scope)
}

#[allow(clippy::too_many_arguments)]
async fn route_rollout_bundle(
    repository: &PostgresEdgeRepository,
    fixture: &GatewayRolloutFixture,
    claim: &DomainClaim,
    scope: &GatewayScope,
    route_id: RouteId,
    hostname: &str,
    key: &str,
    now: chrono::DateTime<Utc>,
) -> Result<StageGatewayRollout, Box<dyn std::error::Error>> {
    route_rollout_bundle_with_validity(
        repository,
        fixture,
        claim,
        scope,
        route_id,
        hostname,
        key,
        now,
        Duration::hours(24),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn route_rollout_bundle_with_validity(
    repository: &PostgresEdgeRepository,
    fixture: &GatewayRolloutFixture,
    claim: &DomainClaim,
    scope: &GatewayScope,
    route_id: RouteId,
    hostname: &str,
    key: &str,
    now: chrono::DateTime<Utc>,
    snapshot_validity: Duration,
) -> Result<StageGatewayRollout, Box<dyn std::error::Error>> {
    let generation = repository
        .next_gateway_rollout_generation(fixture.organization_id, scope.id)
        .await?;
    let target_set = ResolvedRouteTargetSet::new(
        &scope.member_node_ids,
        scope
            .member_node_ids
            .iter()
            .enumerate()
            .map(|(index, node_id)| {
                Ok(ResolvedRouteTarget {
                    workload_id: fixture.workload_id,
                    node_id: *node_id,
                    target: RouteTarget::new(
                        fixture.workload_id,
                        fixture.workload_revision_id,
                        format!(
                            "workload:{}:revision:{}",
                            fixture.workload_id, fixture.workload_revision_id
                        ),
                        fixture.workload_revision_generation,
                        RoutePortName::parse("http")?,
                        UpstreamEndpoint::parse(format!(
                            "http://127.0.0.1:{}",
                            51_000 + u16::try_from(index)?
                        ))?,
                        now,
                    )?,
                })
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
    )?;
    let mut member_contexts = Vec::with_capacity(scope.member_node_ids.len());
    for node_id in &scope.member_node_ids {
        member_contexts.push(GatewayMemberSnapshotContext {
            scope: repository.gateway_scope(*node_id).await?,
            active_routes: repository.active_routes(*node_id).await?,
        });
    }
    GatewayRouteRolloutCompiler::new(
        GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            upstream_request_timeout_ms: 30_000,
            certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
            managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
        })?,
        Duration::minutes(3),
        snapshot_validity,
    )?
    .compile(CompileGatewayRouteRollout {
        scope: scope.clone(),
        rollout_id: GatewayRolloutId::new(),
        generation,
        correlation_id: Uuid::now_v7(),
        route_id,
        hostname: RouteHostname::parse(hostname)?,
        path_prefix: RoutePath::parse("/")?,
        domain_claim_id: claim.id,
        domain_pattern: claim.pattern.clone(),
        target_set,
        member_contexts,
        issued_at: now,
    })?
    .stage_bundle(IdempotencyRequest::new(
        format!("gateway-scopes/{}/route-rollouts", scope.id),
        key,
        hostname.as_bytes(),
    )?)
    .map_err(Into::into)
}

fn publication(
    node_id: NodeId,
    correlation_id: Uuid,
    now: chrono::DateTime<Utc>,
) -> Result<GatewayPublication, String> {
    let snapshot = GatewaySnapshot::new(
        node_id.as_uuid(),
        1,
        None,
        now,
        now + Duration::hours(1),
        format!("# complete Gateway snapshot for {node_id}"),
    )?;
    GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        correlation_id,
        snapshot,
        now,
        now + Duration::minutes(3),
    )
}

fn acknowledgement(
    publication: &GatewayPublication,
    acknowledged_at: chrono::DateTime<Utc>,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: publication.command_id.as_uuid(),
        node_id: publication.node_id.as_uuid(),
        gateway_id: publication.node_id.as_uuid(),
        revision: publication.revision,
        snapshot_digest: publication.snapshot_digest.clone(),
        expires_at: publication.snapshot_expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at,
        management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
    }
}

fn route_acknowledgement(
    publication: &GatewayPublication,
    state: GatewayAckState,
    acknowledged_at: chrono::DateTime<Utc>,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: publication.command_id.as_uuid(),
        node_id: publication.node_id.as_uuid(),
        gateway_id: publication.node_id.as_uuid(),
        revision: publication.revision,
        snapshot_digest: publication.snapshot_digest.clone(),
        expires_at: publication.snapshot_expires_at,
        state,
        ready: state == GatewayAckState::Applied,
        message: (state == GatewayAckState::Rejected)
            .then(|| "Gateway rejected the complete snapshot".into()),
        acknowledged_at,
        management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
    }
}
