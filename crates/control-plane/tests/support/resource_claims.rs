use crate::workloads_support::WorkloadFixture;
use a3s_cloud_contracts::{NodeResourceInventory, NodeResourceSlot};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    OrganizationId, RepositoryError, ResourceClaimId, ResourceName, WorkloadId,
};
use a3s_cloud_control_plane::modules::workloads::{
    IResourceClaimRepository, IWorkloadRepository, PostgresResourceClaimRepository,
    PostgresWorkloadRepository, ResourceAllocation, ResourceClaimReleaseEvidence,
    ResourceClaimReservation, ResourceClaimState, ResourceKind, ResourceSlotRequest, ResourceUnit,
    Workload,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Utc};
use std::sync::Arc;
use tokio::sync::Barrier;

pub async fn exercise_resource_claims(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload: &WorkloadFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    const CONCURRENCY: usize = 100;
    let workload_repository = PostgresWorkloadRepository::new(executor.clone());
    let binding = workload_repository
        .find_deployment_replica_binding(organization_id, workload.deployment_id)
        .await?;
    let repository = Arc::new(PostgresResourceClaimRepository::new(executor.clone()));
    let node_repository = PostgresNodeRepository::new(executor.clone());
    let current_inventory = node_repository
        .current_resource_inventory(workload.node_id)
        .await?
        .ok_or("resource-claim fixture node has no resource inventory")?
        .inventory;
    let now = std::cmp::max(
        std::cmp::max(Utc::now(), binding.updated_at + Duration::seconds(1)),
        current_inventory.observed_at + Duration::seconds(1),
    );
    let mut accelerator_slots = current_inventory.slots.clone();
    accelerator_slots.push(NodeResourceSlot::new(
        ResourceKind::Accelerator,
        "gpu/GPU-H0-1",
        ResourceAllocation::Scalar {
            amount: 1,
            unit: ResourceUnit::Count,
        },
    )?);
    let accelerator_inventory = NodeResourceInventory::new(
        workload.node_id.as_uuid(),
        current_inventory.agent_instance_id,
        current_inventory
            .generation
            .checked_add(1)
            .ok_or("resource inventory generation overflowed")?,
        now,
        accelerator_slots,
    )?;
    node_repository
        .record_resource_inventory(
            accelerator_inventory.clone(),
            now + Duration::milliseconds(1),
        )
        .await?;
    let exact_reservation = reservation(
        ResourceClaimId::new(),
        binding.clone(),
        accelerator_inventory.clone(),
        "gpu/GPU-H0-1",
        now + Duration::seconds(1),
    );
    let exact_barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut exact_tasks = Vec::with_capacity(CONCURRENCY);
    for _ in 0..CONCURRENCY {
        let repository = Arc::clone(&repository);
        let barrier = Arc::clone(&exact_barrier);
        let reservation = exact_reservation.clone();
        exact_tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            repository.reserve(reservation).await
        }));
    }
    let mut exact_results = Vec::with_capacity(CONCURRENCY);
    for task in exact_tasks {
        exact_results.push(task.await??);
    }
    assert_eq!(
        exact_results
            .iter()
            .filter(|result| !result.replayed)
            .count(),
        1
    );
    assert!(exact_results
        .iter()
        .all(|result| result.value == exact_results[0].value));
    let exact_claim = exact_results.remove(0).value;
    assert_eq!(
        Database::new(PostgresDialect, executor.clone())
            .fetch_one_as(
                sql_query::<i64>("select count(*) from resource_claims where id = ")
                    .bind(exact_claim.id.as_uuid()),
            )
            .await?,
        1
    );

    let exact_orphan = repository
        .orphan(
            organization_id,
            exact_claim.id,
            exact_claim.aggregate_version,
            "fixture fences the exact-replay claim".into(),
            now + Duration::seconds(2),
        )
        .await?;
    repository
        .record_released(
            organization_id,
            exact_orphan.id,
            exact_orphan.aggregate_version,
            ResourceClaimReleaseEvidence::ComputeFenced {
                instance_generation: 2,
                slots: exact_orphan.slot_evidence(),
                evidence_digest: digest('c'),
                observed_at: now + Duration::seconds(3),
            },
            now + Duration::seconds(3),
        )
        .await?;

    let base = reservation(
        ResourceClaimId::new(),
        binding.clone(),
        accelerator_inventory.clone(),
        "gpu/GPU-H0-1",
        now + Duration::seconds(4),
    );
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let mut tasks = Vec::with_capacity(CONCURRENCY);
    for _ in 0..CONCURRENCY {
        let repository = Arc::clone(&repository);
        let barrier = Arc::clone(&barrier);
        let mut reservation = base.clone();
        reservation.id = ResourceClaimId::new();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            repository.reserve(reservation).await
        }));
    }
    let mut winners = Vec::new();
    let mut conflicts = 0;
    for task in tasks {
        match task.await? {
            Ok(result) => winners.push(result.value),
            Err(RepositoryError::Conflict(_)) => conflicts += 1,
            Err(error) => return Err(error.into()),
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(conflicts, CONCURRENCY - 1);
    let winner = winners.remove(0);
    assert!(winner.slots[0].slot_generation > exact_claim.slots[0].slot_generation);
    assert_ne!(
        winner.slots[0].fence_token,
        exact_claim.slots[0].fence_token
    );

    let orphaned = repository
        .orphan(
            organization_id,
            winner.id,
            winner.aggregate_version,
            "agent release timed out without provider fencing".into(),
            now + Duration::seconds(5),
        )
        .await?;
    assert_eq!(orphaned.state, ResourceClaimState::Orphaned);
    let mut blocked = base.clone();
    blocked.id = ResourceClaimId::new();
    blocked.reserved_at = now + Duration::seconds(6);
    assert!(matches!(
        repository.reserve(blocked).await,
        Err(RepositoryError::Conflict(_))
    ));
    let active_claim_id = Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(
            sql_query::<Option<uuid::Uuid>>(
                "select active_claim_id from resource_slot_leases where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and node_id = ")
            .bind(binding.node_id.ok_or("binding omitted its node")?.as_uuid())
            .append(" and resource_kind = 'accelerator' and stable_resource_id = 'gpu/GPU-H0-1'"),
        )
        .await?;
    assert_eq!(active_claim_id, Some(orphaned.id.as_uuid()));

    let released = repository
        .record_released(
            organization_id,
            orphaned.id,
            orphaned.aggregate_version,
            ResourceClaimReleaseEvidence::ComputeFenced {
                instance_generation: 3,
                slots: orphaned.slot_evidence(),
                evidence_digest: digest('d'),
                observed_at: now + Duration::seconds(7),
            },
            now + Duration::seconds(7),
        )
        .await?;
    assert_eq!(released.state, ResourceClaimState::Released);

    let mut replacement = base;
    replacement.id = ResourceClaimId::new();
    replacement.reserved_at = now + Duration::seconds(8);
    let replacement = repository.reserve(replacement).await?.value;
    assert!(replacement.slots[0].slot_generation > winner.slots[0].slot_generation);
    assert_ne!(
        replacement.slots[0].fence_token,
        winner.slots[0].fence_token
    );
    let replacement = repository
        .cancel_database_reservation(
            organization_id,
            replacement.id,
            replacement.aggregate_version,
            now + Duration::seconds(9),
        )
        .await?;
    assert_eq!(replacement.state, ResourceClaimState::Released);

    let candidate = workload_repository
        .mark_resolving(
            workload.candidate_deployment_id,
            1,
            now + Duration::seconds(10),
        )
        .await?;
    workload_repository
        .assign_node(
            candidate.id,
            candidate.aggregate_version,
            workload.node_id,
            now + Duration::seconds(11),
        )
        .await?;
    let candidate_binding = workload_repository
        .find_deployment_replica_binding(organization_id, candidate.id)
        .await?;

    let third_workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        binding.project_id,
        binding.environment_id,
        ResourceName::parse("Shared capacity fixture")?,
        now + Duration::seconds(12),
    );
    let third_bundle = crate::workloads_support::request(
        third_workload,
        1,
        'f',
        "resource-claim-shared-capacity",
        now + Duration::seconds(12),
    )?;
    let third_deployment_id = third_bundle.deployment.id;
    workload_repository.create_deployment(third_bundle).await?;
    let third = workload_repository
        .mark_resolving(third_deployment_id, 1, now + Duration::seconds(13))
        .await?;
    workload_repository
        .assign_node(
            third.id,
            third.aggregate_version,
            workload.node_id,
            now + Duration::seconds(14),
        )
        .await?;
    let third_binding = workload_repository
        .find_deployment_replica_binding(organization_id, third.id)
        .await?;

    let active_cpu_allocations = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<serde_json::Value>(
                "select allocation from resource_claim_slots where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and node_id = ")
            .bind(workload.node_id.as_uuid())
            .append(
                " and resource_kind = 'cpu' and stable_resource_id = 'cpu/shared' and released_at is null",
            ),
        )
        .await?
        .rows;
    let already_allocated = active_cpu_allocations.into_iter().try_fold::<_, _, Result<
        u64,
        Box<dyn std::error::Error>,
    >>(0, |total, value| {
        let allocation = serde_json::from_value::<ResourceAllocation>(value)?;
        let amount = allocation
            .scalar_amount()
            .ok_or("active CPU allocation is not scalar")?;
        Ok(total
            .checked_add(amount)
            .ok_or("active CPU allocation overflowed")?)
    })?;
    let shared_capacity = already_allocated
        .checked_add(1_000)
        .ok_or("shared CPU fixture capacity overflowed")?;
    let shared_inventory = NodeResourceInventory::new(
        workload.node_id.as_uuid(),
        accelerator_inventory.agent_instance_id,
        accelerator_inventory
            .generation
            .checked_add(1)
            .ok_or("resource inventory generation overflowed")?,
        now + Duration::seconds(15),
        vec![NodeResourceSlot::new(
            ResourceKind::Cpu,
            "cpu/shared",
            ResourceAllocation::Scalar {
                amount: shared_capacity,
                unit: ResourceUnit::MilliCpu,
            },
        )?],
    )?;
    advance_inventory_while_rejecting_a_concurrent_stale_reservation(
        executor,
        candidate_binding.clone(),
        accelerator_inventory,
        shared_inventory.clone(),
        now + Duration::seconds(15) + Duration::milliseconds(1),
        now + Duration::seconds(16),
    )
    .await?;
    let first_shared = repository
        .reserve(shared_reservation(
            ResourceClaimId::new(),
            binding,
            shared_inventory.clone(),
            400,
            now + Duration::seconds(17),
        ))
        .await?
        .value;
    let second_shared = repository
        .reserve(shared_reservation(
            ResourceClaimId::new(),
            candidate_binding,
            shared_inventory.clone(),
            400,
            now + Duration::seconds(18),
        ))
        .await?
        .value;
    assert!(second_shared.slots[0].slot_generation > first_shared.slots[0].slot_generation);
    assert!(matches!(
        repository
            .reserve(shared_reservation(
                ResourceClaimId::new(),
                third_binding.clone(),
                shared_inventory.clone(),
                400,
                now + Duration::seconds(19),
            ))
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let first_shared = repository
        .orphan(
            organization_id,
            first_shared.id,
            first_shared.aggregate_version,
            "fixture fences one shared allocation".into(),
            now + Duration::seconds(20),
        )
        .await?;
    repository
        .record_released(
            organization_id,
            first_shared.id,
            first_shared.aggregate_version,
            ResourceClaimReleaseEvidence::ComputeFenced {
                instance_generation: 4,
                slots: first_shared.slot_evidence(),
                evidence_digest: digest('e'),
                observed_at: now + Duration::seconds(21),
            },
            now + Duration::seconds(21),
        )
        .await?;
    let third_shared = repository
        .reserve(shared_reservation(
            ResourceClaimId::new(),
            third_binding,
            shared_inventory,
            400,
            now + Duration::seconds(22),
        ))
        .await?
        .value;
    assert!(third_shared.slots[0].slot_generation > second_shared.slots[0].slot_generation);
    Ok(())
}

async fn advance_inventory_while_rejecting_a_concurrent_stale_reservation(
    executor: &PostgresExecutor,
    binding: a3s_cloud_control_plane::modules::workloads::DeploymentReplicaBinding,
    stale_inventory: NodeResourceInventory,
    next_inventory: NodeResourceInventory,
    received_at: chrono::DateTime<Utc>,
    reserved_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    const LOCK_CLASS: i32 = 43_001;
    const LOCK_KEY: i32 = 43_002;

    let control = executor.pool().get().await?;
    control
        .batch_execute(
            "drop trigger if exists a3s_test_pause_inventory_slot_insert
                 on node_resource_inventory_slots;
             drop function if exists a3s_test_pause_inventory_slot_insert();
             create function a3s_test_pause_inventory_slot_insert()
             returns trigger
             language plpgsql
             as $$
             begin
                 perform pg_advisory_xact_lock(43001, 43002);
                 return new;
             end
             $$;
             create trigger a3s_test_pause_inventory_slot_insert
             before insert on node_resource_inventory_slots
             for each row execute function a3s_test_pause_inventory_slot_insert();",
        )
        .await?;
    control
        .query_one("select pg_advisory_lock($1, $2)", &[&LOCK_CLASS, &LOCK_KEY])
        .await?;

    let nodes = PostgresNodeRepository::new(executor.clone());
    let inventory_task = tokio::spawn(async move {
        nodes
            .record_resource_inventory(next_inventory, received_at)
            .await
    });
    let mut inventory_waits_in_trigger = false;
    for _ in 0..500 {
        inventory_waits_in_trigger = control
            .query_one(
                "select exists(
                     select 1
                     from pg_stat_activity
                     where datname = current_database()
                       and pid <> pg_backend_pid()
                       and wait_event_type = 'Lock'
                       and wait_event = 'advisory'
                       and query like '%node_resource_inventory_slots%'
                 )",
                &[],
            )
            .await?
            .get(0);
        if inventory_waits_in_trigger {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    if !inventory_waits_in_trigger {
        control
            .query_one(
                "select pg_advisory_unlock($1, $2)",
                &[&LOCK_CLASS, &LOCK_KEY],
            )
            .await?;
        let inventory_result = inventory_task.await;
        control
            .batch_execute(
                "drop trigger if exists a3s_test_pause_inventory_slot_insert
                     on node_resource_inventory_slots;
                 drop function if exists a3s_test_pause_inventory_slot_insert();",
            )
            .await?;
        inventory_result??;
        return Err("inventory writer did not reach the controlled concurrency boundary".into());
    }

    let claims = Arc::new(PostgresResourceClaimRepository::new(executor.clone()));
    let stale_task = tokio::spawn(async move {
        claims
            .reserve(reservation(
                ResourceClaimId::new(),
                binding,
                stale_inventory,
                "gpu/GPU-H0-1",
                reserved_at,
            ))
            .await
    });
    let mut reservation_waits_on_inventory_head = false;
    for _ in 0..500 {
        reservation_waits_on_inventory_head = control
            .query_one(
                "select exists(
                     select 1
                     from pg_stat_activity
                     where datname = current_database()
                       and pid <> pg_backend_pid()
                       and wait_event_type = 'Lock'
                       and query like '%node_resource_inventory_heads%'
                 )",
                &[],
            )
            .await?
            .get(0);
        if reservation_waits_on_inventory_head {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let unlocked: bool = control
        .query_one(
            "select pg_advisory_unlock($1, $2)",
            &[&LOCK_CLASS, &LOCK_KEY],
        )
        .await?
        .get(0);
    let inventory_result = inventory_task.await;
    let stale_result = stale_task.await;
    control
        .batch_execute(
            "drop trigger if exists a3s_test_pause_inventory_slot_insert
                 on node_resource_inventory_slots;
             drop function if exists a3s_test_pause_inventory_slot_insert();",
        )
        .await?;

    if !unlocked {
        return Err("controlled inventory concurrency lock was not held".into());
    }
    inventory_result??;
    let stale_result = stale_result?;
    if !reservation_waits_on_inventory_head {
        return Err("stale reservation did not wait on the locked inventory head".into());
    }
    assert!(matches!(
        stale_result,
        Err(RepositoryError::Conflict(message))
            if message.contains("current node resource inventory")
    ));
    Ok(())
}

fn reservation(
    id: ResourceClaimId,
    binding: a3s_cloud_control_plane::modules::workloads::DeploymentReplicaBinding,
    inventory: NodeResourceInventory,
    stable_resource_id: &str,
    reserved_at: chrono::DateTime<Utc>,
) -> ResourceClaimReservation {
    let node_id = binding
        .node_id
        .expect("fixture deployment binding is placed");
    let allocation = ResourceAllocation::Scalar {
        amount: 1,
        unit: ResourceUnit::Count,
    };
    ResourceClaimReservation {
        id,
        node_id,
        binding,
        inventory,
        topology_digest: digest('b'),
        slots: vec![ResourceSlotRequest::new(
            ResourceKind::Accelerator,
            stable_resource_id,
            allocation,
        )
        .expect("fixture resource slot")],
        reserved_at,
    }
}

fn shared_reservation(
    id: ResourceClaimId,
    binding: a3s_cloud_control_plane::modules::workloads::DeploymentReplicaBinding,
    inventory: NodeResourceInventory,
    amount: u64,
    reserved_at: chrono::DateTime<Utc>,
) -> ResourceClaimReservation {
    ResourceClaimReservation {
        id,
        node_id: binding
            .node_id
            .expect("fixture deployment binding is placed"),
        binding,
        inventory,
        topology_digest: digest('f'),
        slots: vec![ResourceSlotRequest::new(
            ResourceKind::Cpu,
            "cpu/shared",
            ResourceAllocation::Scalar {
                amount,
                unit: ResourceUnit::MilliCpu,
            },
        )
        .expect("fixture shared resource slot")],
        reserved_at,
    }
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
