use a3s_cloud_contracts::{
    GatewayAckState, GatewayManagementProtocol, GatewaySnapshot, NodeGatewayAck,
};
use a3s_cloud_control_plane::modules::edge::domain::events::{
    GatewayRolloutStaged, GatewayScopeCreated,
};
use a3s_cloud_control_plane::modules::edge::domain::repositories::{
    CreateGatewayScopeWrite, IEdgeRepository, StageGatewayRollout,
};
use a3s_cloud_control_plane::modules::edge::infrastructure::persistence::PostgresEdgeRepository;
use a3s_cloud_control_plane::modules::edge::{
    FleetGatewayCommandQueue, GatewayPublication, GatewayRollout, GatewayRolloutPolicy,
    GatewayRolloutReconciler, GatewayRolloutState, GatewayScope,
};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnvironmentId, GatewayRolloutId, GatewayScopeId, IdempotencyRequest, NodeCommandId, NodeId,
    OrganizationId, ProjectId,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

pub struct GatewayRolloutFixture {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

pub async fn exercise_replicated_gateway_rollout(
    executor: &PostgresExecutor,
    fixture: GatewayRolloutFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = PostgresEdgeRepository::new(executor.clone());
    let database = Database::new(PostgresDialect, executor.clone());
    let now = Utc::now();
    let members = [NodeId::new(), NodeId::new(), NodeId::new()];
    for (ordinal, node_id) in members.iter().enumerate() {
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

    let degraded = restarted
        .mark_gateway_rollout_replica_unavailable(
            fixture.organization_id,
            rollout.id,
            members[2],
            ready.aggregate_version,
            "Gateway missed the rollout readiness deadline",
            now + Duration::seconds(3),
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
            &acknowledgement(&publications[2], now + Duration::seconds(4)),
            now + Duration::seconds(4) + Duration::milliseconds(1),
        )
        .await
        .is_err());
    assert_eq!(
        restarted
            .find_gateway_rollout(fixture.organization_id, rollout.id)
            .await?,
        degraded
    );
    Ok(())
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
