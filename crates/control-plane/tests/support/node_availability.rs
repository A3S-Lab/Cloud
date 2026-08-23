use a3s_cloud_contracts::{NodeHeartbeat, NodeObservationBatch};
use a3s_cloud_control_plane::modules::fleet::domain::events::{
    NodeAvailabilityChanged, NodeAvailabilityFactStatus, NodeAvailabilityResolutionReason,
};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeAvailabilityRepository, INodeControlRepository, INodeRepository, NodeStateChange,
};
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::{NodeCapabilities, NodeState};
use a3s_cloud_control_plane::modules::fleet::{NodeAvailabilityReconciler, PostgresNodeRepository};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    IdempotencyRequest, NodeId, OrganizationId,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::{
    IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities, RuntimeFeature,
    RuntimeUnitClass,
};
use chrono::{DateTime, Duration, Timelike, Utc};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
struct SeededNode {
    organization_id: OrganizationId,
    node_id: NodeId,
    agent_instance_id: Uuid,
    last_observed_at: DateTime<Utc>,
}

pub async fn exercise_node_availability_facts(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = super::migrate_and_connect_for_test(&url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let primary_organization = OrganizationId::new();
    let secondary_organization = OrganizationId::new();
    seed_organization(&executor, primary_organization, "availability-primary").await?;
    seed_organization(&executor, secondary_organization, "availability-secondary").await?;

    let heartbeat_timeout = Duration::seconds(30);
    let base = canonical_timestamp(Utc::now() - Duration::hours(2));
    let primary = seed_node(
        &executor,
        primary_organization,
        NodeState::Ready,
        2,
        base,
        "primary",
    )
    .await?;
    let repository = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let equality_worker = reconciler(repository.clone(), heartbeat_timeout, 1)?;

    let equality = equality_worker.run_once(base + heartbeat_timeout).await?;
    assert_eq!(equality.processed_nodes, 1);
    assert_eq!(equality.initialized_heads, 1);
    assert_eq!(equality.unavailable_facts, 0);
    assert_eq!(fact_count(&executor, primary.node_id).await?, 0);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<(String, DateTime<Utc>)>(
                    "select state, timeout_deadline_at from fleet_node_availability_fact_heads where organization_id = ",
                )
                .bind(primary.organization_id.as_uuid())
                .append(" and node_id = ")
                .bind(primary.node_id.as_uuid()),
            )
            .await?,
        ("observed".into(), base + heartbeat_timeout)
    );

    let current = repository
        .find(primary_organization, primary.node_id)
        .await?;
    let mut projected = current.clone();
    projected.drain()?;
    let state_changed_at = base + heartbeat_timeout;
    let state_event =
        a3s_cloud_control_plane::modules::fleet::domain::events::NodeStateChanged::envelope(
            &projected,
            NodeState::Draining,
            state_changed_at,
            Uuid::now_v7(),
        )?;
    let state_change = repository
        .set_state(NodeStateChange {
            organization_id: primary_organization,
            node_id: primary.node_id,
            state: NodeState::Draining,
            expected_version: current.aggregate_version,
            changed_at: state_changed_at,
            event: state_event,
            idempotency: IdempotencyRequest::new(
                "availability/state",
                "ready-to-draining",
                b"ready-to-draining",
            )?,
        })
        .await?;
    assert_eq!(state_change.value.aggregate_version, 3);
    assert_eq!(fact_count(&executor, primary.node_id).await?, 0);

    let first_firing = equality_worker
        .run_once(base + heartbeat_timeout + Duration::microseconds(1))
        .await?;
    assert_eq!(first_firing.processed_nodes, 1);
    assert_eq!(first_firing.unavailable_facts, 1);
    let first_events = availability_events(&executor, primary.node_id).await?;
    assert_eq!(first_events.len(), 1);
    assert_eq!(first_events[0].1, "fleet.node.unavailable");
    assert_eq!(first_events[0].2, 1);
    assert_eq!(first_events[0].3, 6);
    assert_eq!(first_events[0].4, first_events[0].0);
    assert_eq!(first_events[0].5, None);
    let first_payload: NodeAvailabilityChanged = serde_json::from_value(first_events[0].6.clone())?;
    assert_eq!(
        first_payload.status,
        NodeAvailabilityFactStatus::Unavailable
    );
    assert_eq!(first_payload.organization_id, primary_organization);
    assert_eq!(first_payload.node_id, primary.node_id);
    assert_eq!(first_payload.timeout_deadline_at, base + heartbeat_timeout);

    let duplicate_left = reconciler(repository.clone(), heartbeat_timeout, 8)?;
    let duplicate_right = reconciler(repository.clone(), heartbeat_timeout, 8)?;
    let (left, right) = tokio::join!(
        duplicate_left.run_once(base + Duration::minutes(2)),
        duplicate_right.run_once(base + Duration::minutes(2))
    );
    assert_eq!(left?.unavailable_facts + right?.unavailable_facts, 0);
    assert_eq!(fact_count(&executor, primary.node_id).await?, 1);

    let restored_observation = base + Duration::minutes(5);
    let restored_received_at = canonical_timestamp(Utc::now());
    let restored_batch = NodeObservationBatch {
        schema: NodeObservationBatch::SCHEMA.into(),
        node_id: primary.node_id.as_uuid(),
        agent_instance_id: primary.agent_instance_id,
        sent_at: restored_observation,
        heartbeat: NodeHeartbeat {
            schema: NodeHeartbeat::SCHEMA.into(),
            node_id: primary.node_id.as_uuid(),
            agent_instance_id: primary.agent_instance_id,
            observed_at: restored_observation,
            agent_version: "availability-agent-2".into(),
            runtime_capabilities: runtime_capabilities(),
        },
        observations: Vec::new(),
    };
    database
        .execute(sql_query::<()>(
            "alter table outbox_events add constraint reject_node_availability_heartbeat_resolution_probe check (event_key <> 'fleet.node.availability-resolved') not valid",
        ))
        .await?;
    assert!(repository
        .record_observations(restored_batch.clone().into(), restored_received_at)
        .await
        .is_err());
    let rolled_back_heartbeat = repository
        .find(primary_organization, primary.node_id)
        .await?;
    assert_eq!(rolled_back_heartbeat.aggregate_version, 3);
    assert_eq!(rolled_back_heartbeat.last_observed_at, base);
    assert_eq!(fact_count(&executor, primary.node_id).await?, 1);
    database
        .execute(sql_query::<()>(
            "alter table outbox_events drop constraint reject_node_availability_heartbeat_resolution_probe",
        ))
        .await?;
    let restored_receipt = repository
        .record_observations(restored_batch.clone().into(), restored_received_at)
        .await?;
    assert_eq!(restored_receipt.accepted_reports, 0);
    assert_eq!(restored_receipt.replayed_reports, 0);
    let restored = repository
        .find(primary_organization, primary.node_id)
        .await?;
    assert_eq!(restored.aggregate_version, 4);
    let after_recovery = availability_events(&executor, primary.node_id).await?;
    assert_eq!(after_recovery.len(), 2);
    assert_eq!(after_recovery[1].1, "fleet.node.availability-resolved");
    assert_eq!(after_recovery[1].3, 7);
    assert_eq!(after_recovery[1].4, after_recovery[0].0);
    assert_eq!(after_recovery[1].5, Some(after_recovery[0].0));
    let recovered_payload: NodeAvailabilityChanged =
        serde_json::from_value(after_recovery[1].6.clone())?;
    assert_eq!(
        recovered_payload.status,
        NodeAvailabilityFactStatus::Resolved
    );
    assert_eq!(
        recovered_payload.resolution_reason,
        Some(NodeAvailabilityResolutionReason::HeartbeatRestored)
    );
    assert_eq!(recovered_payload.last_observed_at, restored_observation);

    let replayed_receipt = repository
        .record_observations(
            restored_batch.into(),
            restored_received_at + Duration::microseconds(1),
        )
        .await?;
    assert_eq!(replayed_receipt.accepted_reports, 0);
    assert_eq!(replayed_receipt.replayed_reports, 0);
    let replayed = repository
        .find(primary_organization, primary.node_id)
        .await?;
    assert_eq!(replayed, restored);
    assert_eq!(fact_count(&executor, primary.node_id).await?, 2);

    let anchored = equality_worker
        .run_once(restored_observation + heartbeat_timeout)
        .await?;
    assert_eq!(anchored.processed_nodes, 1);
    assert_eq!(anchored.unavailable_facts, 0);
    let drifted_timeout = reconciler(repository.clone(), Duration::seconds(1), 8)?;
    assert_eq!(
        drifted_timeout
            .run_once(restored_observation + Duration::seconds(5))
            .await?
            .processed_nodes,
        0,
        "timeout policy drift without another heartbeat must stay silent"
    );
    let second_firing = drifted_timeout
        .run_once(restored_observation + heartbeat_timeout + Duration::microseconds(1))
        .await?;
    assert_eq!(second_firing.unavailable_facts, 1);
    let repeated = availability_events(&executor, primary.node_id).await?;
    assert_eq!(repeated.len(), 3);
    assert_eq!(repeated[2].1, "fleet.node.unavailable");
    assert_eq!(repeated[2].3, 8);
    assert!(repeated[0].3 < repeated[1].3 && repeated[1].3 < repeated[2].3);
    assert_ne!(repeated[0].0, repeated[2].0);

    let revoked = seed_node(
        &executor,
        primary_organization,
        NodeState::Draining,
        2,
        base + Duration::seconds(1),
        "revoked",
    )
    .await?;
    seed_certificate(&executor, &revoked, base).await?;
    let revoked_deadline = revoked.last_observed_at + heartbeat_timeout;
    let revoked_baseline = equality_worker
        .run_once(revoked_deadline + Duration::microseconds(1))
        .await?;
    assert_eq!(revoked_baseline.initialized_heads, 1);
    assert_eq!(revoked_baseline.unavailable_facts, 0);
    assert_eq!(fact_count(&executor, revoked.node_id).await?, 0);
    assert_eq!(
        equality_worker
            .run_once(revoked_deadline + Duration::microseconds(1))
            .await?
            .unavailable_facts,
        1
    );
    let current = repository
        .find(primary_organization, revoked.node_id)
        .await?;
    let mut projected = current.clone();
    projected.revoke();
    let revoked_at = canonical_timestamp(Utc::now());
    let state_event =
        a3s_cloud_control_plane::modules::fleet::domain::events::NodeStateChanged::envelope(
            &projected,
            NodeState::Revoked,
            revoked_at,
            Uuid::now_v7(),
        )?;
    let revoke_idempotency = IdempotencyRequest::new(
        "availability/revoke",
        "revoke-open-firing",
        b"revoke-open-firing",
    )?;
    database
        .execute(sql_query::<()>(
            "alter table outbox_events add constraint reject_node_availability_revoke_resolution_probe check (event_key <> 'fleet.node.availability-resolved') not valid",
        ))
        .await?;
    assert!(repository
        .set_state(NodeStateChange {
            organization_id: primary_organization,
            node_id: revoked.node_id,
            state: NodeState::Revoked,
            expected_version: current.aggregate_version,
            changed_at: revoked_at,
            event: state_event.clone(),
            idempotency: revoke_idempotency.clone(),
        })
        .await
        .is_err());
    let rolled_back_revoke = repository
        .find(primary_organization, revoked.node_id)
        .await?;
    assert_eq!(rolled_back_revoke.state, NodeState::Draining);
    assert_eq!(rolled_back_revoke.aggregate_version, 2);
    assert_eq!(fact_count(&executor, revoked.node_id).await?, 1);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_certificates where node_id = ")
                    .bind(revoked.node_id.as_uuid())
                    .append(" and revoked_at is null"),
            )
            .await?,
        1
    );
    database
        .execute(sql_query::<()>(
            "alter table outbox_events drop constraint reject_node_availability_revoke_resolution_probe",
        ))
        .await?;
    let revoked_result = repository
        .set_state(NodeStateChange {
            organization_id: primary_organization,
            node_id: revoked.node_id,
            state: NodeState::Revoked,
            expected_version: current.aggregate_version,
            changed_at: revoked_at,
            event: state_event.clone(),
            idempotency: revoke_idempotency.clone(),
        })
        .await?;
    assert!(!revoked_result.replayed);
    let revoked_replay = repository
        .set_state(NodeStateChange {
            organization_id: primary_organization,
            node_id: revoked.node_id,
            state: NodeState::Revoked,
            expected_version: current.aggregate_version,
            changed_at: revoked_at,
            event: state_event,
            idempotency: revoke_idempotency,
        })
        .await?;
    assert!(revoked_replay.replayed);
    let revoked_events = availability_events(&executor, revoked.node_id).await?;
    assert_eq!(revoked_events.len(), 2);
    assert_eq!(revoked_events[0].3, 4);
    assert_eq!(revoked_events[1].3, 5);
    let revoked_payload: NodeAvailabilityChanged =
        serde_json::from_value(revoked_events[1].6.clone())?;
    assert_eq!(
        revoked_payload.resolution_reason,
        Some(NodeAvailabilityResolutionReason::NodeRevoked)
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from node_certificates where node_id = ",)
                    .bind(revoked.node_id.as_uuid())
                    .append(" and revoked_at = ")
                    .bind(revoked_at),
            )
            .await?,
        1
    );

    let bounded_one = seed_node(
        &executor,
        primary_organization,
        NodeState::Ready,
        2,
        base + Duration::seconds(2),
        "bounded-one",
    )
    .await?;
    let bounded_two = seed_node(
        &executor,
        secondary_organization,
        NodeState::Ready,
        2,
        base + Duration::seconds(3),
        "bounded-two",
    )
    .await?;
    let bounded_three = seed_node(
        &executor,
        primary_organization,
        NodeState::Draining,
        4,
        base + Duration::seconds(4),
        "bounded-three",
    )
    .await?;
    let pending = seed_node(
        &executor,
        secondary_organization,
        NodeState::Pending,
        1,
        base + Duration::seconds(5),
        "pending",
    )
    .await?;
    let baseline_left_worker = reconciler(repository.clone(), heartbeat_timeout, 2)?;
    let baseline_right_worker = reconciler(repository.clone(), heartbeat_timeout, 2)?;
    let baseline_time = base + Duration::seconds(5);
    let (baseline_left, baseline_right) = tokio::join!(
        baseline_left_worker.run_once(baseline_time),
        baseline_right_worker.run_once(baseline_time)
    );
    let baseline_left = baseline_left?;
    let baseline_right = baseline_right?;
    assert_eq!(
        baseline_left.processed_nodes + baseline_right.processed_nodes,
        3
    );
    assert_eq!(
        baseline_left.initialized_heads + baseline_right.initialized_heads,
        3
    );
    assert_eq!(
        baseline_left.unavailable_facts + baseline_right.unavailable_facts,
        0
    );
    assert!(baseline_left.processed_nodes <= 2 && baseline_right.processed_nodes <= 2);

    let page_left_worker = reconciler(repository.clone(), heartbeat_timeout, 2)?;
    let page_right_worker = reconciler(repository.clone(), heartbeat_timeout, 2)?;
    let page_time = base + Duration::minutes(10);
    let (page_left, page_right) = tokio::join!(
        page_left_worker.run_once(page_time),
        page_right_worker.run_once(page_time)
    );
    let page_left = page_left?;
    let page_right = page_right?;
    assert_eq!(page_left.processed_nodes + page_right.processed_nodes, 3);
    assert_eq!(
        page_left.unavailable_facts + page_right.unavailable_facts,
        3
    );
    assert!(page_left.processed_nodes <= 2 && page_right.processed_nodes <= 2);
    for node in [&bounded_one, &bounded_two, &bounded_three] {
        assert_eq!(fact_count(&executor, node.node_id).await?, 1);
    }
    assert_eq!(fact_count(&executor, pending.node_id).await?, 0);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from fleet_node_availability_fact_heads where node_id = ",
                )
                .bind(pending.node_id.as_uuid()),
            )
            .await?,
        0
    );
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from outbox_events e join nodes n on n.id = e.aggregate_id where e.event_key in ('fleet.node.unavailable', 'fleet.node.availability-resolved') and (e.organization_id <> n.organization_id or e.payload ->> 'organizationId' <> n.organization_id::text or e.payload ->> 'nodeId' <> n.id::text)",
            ))
            .await?,
        0,
        "owner facts must retain their exact tenant and Node identity"
    );

    let rollback = seed_node(
        &executor,
        primary_organization,
        NodeState::Ready,
        2,
        base + Duration::seconds(6),
        "rollback",
    )
    .await?;
    let rollback_worker = reconciler(repository.clone(), heartbeat_timeout, 1)?;
    let rollback_baseline = rollback_worker.run_once(page_time).await?;
    assert_eq!(rollback_baseline.initialized_heads, 1);
    assert_eq!(rollback_baseline.unavailable_facts, 0);
    assert_eq!(fact_count(&executor, rollback.node_id).await?, 0);
    database
        .execute(sql_query::<()>(
            "alter table outbox_events add constraint reject_node_availability_outbox_probe check (event_key <> 'fleet.node.unavailable') not valid",
        ))
        .await?;
    assert!(rollback_worker.run_once(page_time).await.is_err());
    assert_eq!(fact_count(&executor, rollback.node_id).await?, 0);
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from fleet_node_availability_fact_heads where node_id = ",
                )
                .bind(rollback.node_id.as_uuid()),
            )
            .await?,
        1,
        "Outbox rejection must preserve the pre-existing owner fact head"
    );
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>(
                    "select count(*) from fleet_node_availability_fact_heads where node_id = ",
                )
                .bind(rollback.node_id.as_uuid())
                .append(" and state = 'observed' and latest_event_id is null"),
            )
            .await?,
        1,
        "Outbox rejection must roll back the unavailable transition"
    );
    database
        .execute(sql_query::<()>(
            "alter table outbox_events drop constraint reject_node_availability_outbox_probe",
        ))
        .await?;
    assert_eq!(
        rollback_worker.run_once(page_time).await?.unavailable_facts,
        1
    );

    let reopened = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let restarted_worker = reconciler(reopened, heartbeat_timeout, 10)?;
    let restart = restarted_worker
        .run_once(page_time + Duration::hours(1))
        .await?;
    assert_eq!(restart.processed_nodes, 0);
    assert_eq!(restart.unavailable_facts, 0);

    let all_payloads = database
        .fetch_all_as(
            sql_query::<Value>(
                "select payload from outbox_events where event_key in ('fleet.node.unavailable', 'fleet.node.availability-resolved') order by aggregate_id, aggregate_version",
            ),
        )
        .await?
        .rows;
    assert_eq!(all_payloads.len(), 9);
    let persisted = serde_json::to_string(&all_payloads)?.to_ascii_lowercase();
    for forbidden in [
        "capabilities",
        "inventory",
        "command",
        "metric",
        "provider",
        "credential",
        "diagnostic",
        "agentversion",
    ] {
        assert!(
            !persisted.contains(forbidden),
            "persisted Node availability evidence leaked {forbidden}"
        );
    }

    println!(
        "A3S_CLOUD_C0_3_N4H_POSTGRES_CERTIFIED migration=139 initial_silence=1 strict_boundary=1 state_change_silence=1 firings=7 resolutions=2 production_heartbeat=1 revoke_replay=1 concurrent_pages=2 atomic_rollbacks=3 restart_silence=1 tenant_isolation=1 private_fields=0"
    );
    Ok(())
}

fn reconciler(
    repository: Arc<PostgresNodeRepository>,
    heartbeat_timeout: Duration,
    batch_size: usize,
) -> Result<NodeAvailabilityReconciler, String> {
    let repository: Arc<dyn INodeAvailabilityRepository> = repository;
    NodeAvailabilityReconciler::new(
        repository,
        std::time::Duration::from_millis(10),
        heartbeat_timeout,
        batch_size,
    )
}

async fn seed_organization(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    Database::new(PostgresDialect, executor.clone())
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(key)
            .append(", ")
            .bind(key)
            .append(", 1, ")
            .bind(canonical_timestamp(Utc::now()))
            .append(")"),
        )
        .await?;
    Ok(())
}

async fn seed_node(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    state: NodeState,
    aggregate_version: u64,
    last_observed_at: DateTime<Utc>,
    label: &str,
) -> Result<SeededNode, Box<dyn std::error::Error>> {
    let node_id = NodeId::new();
    let agent_instance_id = Uuid::now_v7();
    let capabilities = NodeCapabilities::new(
        "availability-runtime",
        "availability-build-1",
        serde_json::json!({"runtime": "bounded-test"}),
    )?;
    let name_key = format!("availability-{label}-{}", node_id.as_uuid().simple());
    Database::new(PostgresDialect, executor.clone())
        .execute(
            sql_query::<()>(
                "insert into nodes (organization_id, id, name, name_key, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, last_sequence, aggregate_version) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(node_id.as_uuid())
            .append(", ")
            .bind(name_key.as_str())
            .append(", ")
            .bind(name_key.as_str())
            .append(", ")
            .bind(state.as_str())
            .append(", ")
            .bind(agent_instance_id)
            .append(", 'availability-agent-1', ")
            .bind(capabilities.provider_id())
            .append(", ")
            .bind(capabilities.provider_build())
            .append(", ")
            .bind(capabilities.digest())
            .append(", ")
            .bind(capabilities.document().clone())
            .append(", ")
            .bind(last_observed_at - Duration::seconds(1))
            .append(", ")
            .bind(last_observed_at)
            .append(", 0, ")
            .bind(aggregate_version)
            .append(")"),
        )
        .await?;
    Ok(SeededNode {
        organization_id,
        node_id,
        agent_instance_id,
        last_observed_at,
    })
}

async fn seed_certificate(
    executor: &PostgresExecutor,
    node: &SeededNode,
    issued_at: DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let fingerprint_hex = node.node_id.as_uuid().simple().to_string().repeat(2);
    assert_eq!(fingerprint_hex.len(), 64);
    assert!(fingerprint_hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    Database::new(PostgresDialect, executor.clone())
        .execute(
            sql_query::<()>(
                "insert into node_certificates (id, node_id, serial_number, fingerprint, certificate_pem, ca_bundle_pem, issued_at, expires_at) values (",
            )
            .bind(Uuid::now_v7())
            .append(", ")
            .bind(node.node_id.as_uuid())
            .append(", ")
            .bind(format!("availability-{}", node.node_id))
            .append(", ")
            .bind(format!("sha256:{fingerprint_hex}"))
            .append(", 'certificate', 'ca', ")
            .bind(issued_at)
            .append(", ")
            .bind(issued_at + Duration::days(1))
            .append(")"),
        )
        .await?;
    Ok(())
}

async fn fact_count(
    executor: &PostgresExecutor,
    node_id: NodeId,
) -> Result<i64, Box<dyn std::error::Error>> {
    Ok(Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from outbox_events where aggregate_id = ",
            )
            .bind(node_id.as_uuid())
            .append(" and event_key in ('fleet.node.unavailable', 'fleet.node.availability-resolved')"),
        )
        .await?)
}

type AvailabilityEventRow = (Uuid, String, u32, u64, Uuid, Option<Uuid>, Value);

async fn availability_events(
    executor: &PostgresExecutor,
    node_id: NodeId,
) -> Result<Vec<AvailabilityEventRow>, Box<dyn std::error::Error>> {
    Ok(Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<AvailabilityEventRow>(
                "select event_id, event_key, schema_version, aggregate_version, correlation_id, causation_id, payload from outbox_events where aggregate_id = ",
            )
            .bind(node_id.as_uuid())
            .append(" and event_key in ('fleet.node.unavailable', 'fleet.node.availability-resolved') order by aggregate_version"),
        )
        .await?
        .rows)
}

fn runtime_capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("box").expect("valid availability provider ID"),
        provider_build: "availability-box-build".into(),
        unit_classes: vec![RuntimeUnitClass::Task, RuntimeUnitClass::Service],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Sandbox],
        network_modes: vec![NetworkMode::None, NetworkMode::Service],
        mount_kinds: Vec::new(),
        health_check_kinds: Vec::new(),
        resource_controls: vec![
            ResourceControl::Cpu,
            ResourceControl::Memory,
            ResourceControl::Pids,
            ResourceControl::EphemeralStorage,
        ],
        features: vec![
            RuntimeFeature::DurableIdentity,
            RuntimeFeature::Stop,
            RuntimeFeature::Remove,
            RuntimeFeature::ServiceTcp,
        ],
    }
}

fn canonical_timestamp(value: DateTime<Utc>) -> DateTime<Utc> {
    value
        .with_nanosecond(value.nanosecond() / 1_000 * 1_000)
        .expect("database-precision timestamp")
}
