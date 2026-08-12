use crate::workloads_support::{ReplicaSetFixture, WorkloadFixture};
use a3s_cloud_contracts::{NodeResourceInventory, NodeResourceSlot};
use a3s_cloud_control_plane::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository,
};
use a3s_cloud_control_plane::modules::fleet::domain::value_objects::NodeState;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    NodeId, OrganizationId, RepositoryError, ResourceClaimId, ResourceName, WorkloadId,
};
use a3s_cloud_control_plane::modules::workloads::{
    AtomicResourceClaimReservation, IResourceClaimRepository, IWorkloadRepository,
    PostgresResourceClaimRepository, PostgresWorkloadRepository, ResourceAllocation,
    ResourceClaimReleaseEvidence, ResourceClaimReservation, ResourceClaimState, ResourceKind,
    ResourceSlotRequest, ResourceUnit, Workload,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Timelike, Utc};
use std::sync::Arc;
use tokio::sync::Barrier;

pub async fn exercise_replica_anti_affinity(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    replica_set: &ReplicaSetFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    let nodes = PostgresNodeRepository::new(executor.clone());
    let (node_id, inventory) = ready_inventoried_node(&nodes, organization_id).await?;
    let binding_time = replica_set
        .bindings
        .iter()
        .map(|binding| binding.updated_at)
        .max()
        .ok_or("replica anti-affinity fixture has no bindings")?;
    let base = Utc::now()
        .max(binding_time + Duration::seconds(1))
        .max(inventory.observed_at + Duration::seconds(1));
    exercise_required_replica_anti_affinity(
        &Arc::new(PostgresResourceClaimRepository::new(executor.clone())),
        node_id,
        replica_set,
        inventory,
        base,
    )
    .await
}

pub async fn exercise_atomic_resource_claim_reservations(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    replica_set: &ReplicaSetFixture,
) -> Result<(), Box<dyn std::error::Error>> {
    let nodes = PostgresNodeRepository::new(executor.clone());
    let (node_id, inventory) = ready_inventoried_node(&nodes, organization_id).await?;
    let first_unplaced = replica_set
        .bindings
        .first()
        .ok_or("atomic Claim fixture has no replica binding")?;
    exercise_concurrent_atomic_resource_claim_candidates(
        executor,
        &nodes,
        organization_id,
        replica_set,
        node_id,
        &inventory,
    )
    .await?;
    let workload_repository = PostgresWorkloadRepository::new(executor.clone());
    let created_at = Utc::now()
        .max(first_unplaced.updated_at + Duration::seconds(1))
        .max(inventory.observed_at + Duration::seconds(1));
    let second_workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        first_unplaced.project_id,
        first_unplaced.environment_id,
        ResourceName::parse("Atomic Claim companion")?,
        created_at,
    );
    let second_bundle = crate::workloads_support::request(
        second_workload,
        1,
        'a',
        "postgres-atomic-claim-companion",
        created_at,
    )?;
    let second_deployment_id = second_bundle.deployment.id;
    workload_repository.create_deployment(second_bundle).await?;
    let second_unplaced = workload_repository
        .find_deployment_replica_binding(organization_id, second_deployment_id)
        .await?;
    let placement_at = created_at.max(second_unplaced.updated_at);
    let first_binding = first_unplaced.propose_assignment(node_id, placement_at)?;
    let second_binding = second_unplaced.propose_assignment(node_id, placement_at)?;
    let cpu_slot = inventory
        .slots
        .iter()
        .find(|slot| slot.kind == ResourceKind::Cpu && slot.stable_resource_id == "cpu/shared")
        .ok_or("atomic Claim fixture inventory has no shared CPU slot")?;
    let cpu_capacity = cpu_slot
        .allocation
        .scalar_amount()
        .ok_or("atomic Claim fixture CPU capacity is not scalar")?;
    if cpu_capacity < 2 {
        return Err("atomic Claim fixture CPU capacity is smaller than two units".into());
    }
    let database = Database::new(PostgresDialect, executor.clone());
    let lease_query = || {
        sql_query::<(u64, Option<uuid::Uuid>, chrono::DateTime<Utc>)>(
            "select slot_generation, active_claim_id, updated_at from resource_slot_leases where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and node_id = ")
        .bind(node_id.as_uuid())
        .append(" and resource_kind = 'cpu' and stable_resource_id = ")
        .bind(cpu_slot.stable_resource_id.as_str())
    };
    let baseline_lease = database.fetch_optional_as(lease_query()).await?;
    let repository = PostgresResourceClaimRepository::new(executor.clone());
    let failed_at = baseline_lease.map_or(placement_at, |lease| placement_at.max(lease.2))
        + Duration::seconds(1);
    let failed_first = shared_reservation(
        ResourceClaimId::new(),
        first_binding.clone(),
        inventory.clone(),
        cpu_capacity,
        failed_at,
    );
    let failed_second = shared_reservation(
        ResourceClaimId::new(),
        second_binding.clone(),
        inventory.clone(),
        cpu_capacity,
        failed_at,
    );
    let failed_batch =
        AtomicResourceClaimReservation::new(vec![failed_second.clone(), failed_first.clone()])?;
    assert!(matches!(
        repository.reserve_atomically(failed_batch).await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from resource_claims where id in (")
                    .bind(failed_first.id.as_uuid())
                    .append(", ")
                    .bind(failed_second.id.as_uuid())
                    .append(")"),
            )
            .await?,
        0
    );
    assert_eq!(
        database.fetch_optional_as(lease_query()).await?,
        baseline_lease
    );

    let reserved_at = failed_at + Duration::seconds(1);
    let first = shared_reservation(
        ResourceClaimId::new(),
        first_binding,
        inventory.clone(),
        1,
        reserved_at,
    );
    let second = shared_reservation(
        ResourceClaimId::new(),
        second_binding.clone(),
        inventory.clone(),
        1,
        reserved_at,
    );
    let batch = AtomicResourceClaimReservation::new(vec![second.clone(), first.clone()])?;
    let created = repository.reserve_atomically(batch.clone()).await?;
    let replayed = repository.reserve_atomically(batch).await?;
    assert!(!created.replayed);
    assert!(replayed.replayed);
    assert_eq!(created.value, replayed.value);
    let mut slot_generations = created
        .value
        .iter()
        .map(|claim| claim.slots[0].slot_generation)
        .collect::<Vec<_>>();
    slot_generations.sort_unstable();
    let baseline_generation = baseline_lease.map_or(0, |lease| lease.0);
    assert_eq!(
        slot_generations,
        vec![baseline_generation + 1, baseline_generation + 2]
    );

    let absent = shared_reservation(
        ResourceClaimId::new(),
        second_binding,
        inventory,
        1,
        reserved_at,
    );
    let partial = AtomicResourceClaimReservation::new(vec![first, absent.clone()])?;
    assert_eq!(
        repository.reserve_atomically(partial).await,
        Err(RepositoryError::IdempotencyConflict)
    );
    assert_eq!(
        repository.find(organization_id, absent.id).await,
        Err(RepositoryError::NotFound)
    );

    for claim in created.value {
        repository
            .cancel_database_reservation(
                claim.organization_id,
                claim.id,
                claim.aggregate_version,
                reserved_at + Duration::seconds(1),
            )
            .await?;
    }
    Ok(())
}

async fn ready_inventoried_node(
    nodes: &PostgresNodeRepository,
    organization_id: OrganizationId,
) -> Result<(NodeId, NodeResourceInventory), Box<dyn std::error::Error>> {
    let mut ready_without_inventory = None;
    let mut inventory_template = None;
    for node in nodes.list(organization_id).await? {
        let inventory = nodes.current_resource_inventory(node.id).await?;
        if inventory_template.is_none() {
            inventory_template = inventory.as_ref().map(|record| record.inventory.clone());
        }
        if node.state == NodeState::Ready {
            if let Some(inventory) = inventory {
                return Ok((node.id, inventory.inventory));
            }
            if ready_without_inventory.is_none() {
                ready_without_inventory = Some(node);
            }
        }
    }

    let node = ready_without_inventory.ok_or("Claim fixture has no ready node")?;
    let template = inventory_template.ok_or("Claim fixture has no inventory template")?;
    let observed_at = Utc::now()
        .max(node.enrolled_at)
        .max(node.last_observed_at)
        .max(template.observed_at);
    let observed_at = observed_at
        .with_nanosecond(observed_at.nanosecond() / 1_000 * 1_000)
        .ok_or("Claim fixture inventory timestamp is invalid")?
        + Duration::milliseconds(1);
    let inventory = NodeResourceInventory::new(
        node.id.as_uuid(),
        node.agent_instance_id,
        1,
        observed_at,
        template.slots,
    )?;
    nodes
        .record_resource_inventory(inventory.clone(), observed_at + Duration::milliseconds(1))
        .await?;
    Ok((node.id, inventory))
}

async fn exercise_concurrent_atomic_resource_claim_candidates(
    executor: &PostgresExecutor,
    nodes: &PostgresNodeRepository,
    organization_id: OrganizationId,
    replica_set: &ReplicaSetFixture,
    first_node_id: a3s_cloud_control_plane::modules::shared_kernel::domain::NodeId,
    first_inventory: &NodeResourceInventory,
) -> Result<(), Box<dyn std::error::Error>> {
    let first_binding = replica_set
        .bindings
        .first()
        .ok_or("concurrent atomic Claim fixture has no first binding")?;
    let second_binding = replica_set
        .bindings
        .get(1)
        .ok_or("concurrent atomic Claim fixture has no second binding")?;
    let second_node_id = a3s_cloud_control_plane::modules::shared_kernel::domain::NodeId::new();
    let second_agent_instance_id = uuid::Uuid::now_v7();
    let inserted_at = Utc::now()
        .max(first_binding.updated_at)
        .max(second_binding.updated_at)
        .max(first_inventory.observed_at);
    let database = Database::new(PostgresDialect, executor.clone());
    database
        .execute(
            sql_query::<()>(
                "insert into nodes (organization_id, id, name, name_key, state, agent_instance_id, agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, enrolled_at, last_observed_at, last_sequence, aggregate_version) select organization_id, ",
            )
            .bind(second_node_id.as_uuid())
            .append(", 'atomic claim worker', ")
            .bind(format!("atomic-claim-worker-{}", second_node_id.as_uuid()))
            .append(", 'ready', ")
            .bind(second_agent_instance_id)
            .append(", agent_version, runtime_provider_id, runtime_provider_build, capabilities_digest, capabilities, ")
            .bind(inserted_at)
            .append(", ")
            .bind(inserted_at)
            .append(", 0, 1 from nodes where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(first_node_id.as_uuid()),
        )
        .await?;
    let second_inventory = NodeResourceInventory::new(
        second_node_id.as_uuid(),
        second_agent_instance_id,
        1,
        inserted_at + Duration::milliseconds(1),
        first_inventory.slots.clone(),
    )?;
    nodes
        .record_resource_inventory(
            second_inventory.clone(),
            inserted_at + Duration::milliseconds(2),
        )
        .await?;

    let first_capacity = cpu_shared_capacity(first_inventory)?;
    let second_capacity = cpu_shared_capacity(&second_inventory)?;
    let slot_ledger_updated_at = database
        .fetch_one_as(
            sql_query::<Option<chrono::DateTime<Utc>>>(
                "select max(updated_at) from resource_slot_leases where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and node_id in (")
            .bind(first_node_id.as_uuid())
            .append(", ")
            .bind(second_node_id.as_uuid())
            .append(")"),
        )
        .await?;
    let reserved_at = slot_ledger_updated_at
        .map_or(inserted_at, |updated_at| inserted_at.max(updated_at))
        + Duration::seconds(1);
    let left = AtomicResourceClaimReservation::new(vec![
        shared_reservation(
            ResourceClaimId::new(),
            first_binding.propose_assignment(first_node_id, reserved_at)?,
            first_inventory.clone(),
            first_capacity,
            reserved_at,
        ),
        shared_reservation(
            ResourceClaimId::new(),
            second_binding.propose_assignment(second_node_id, reserved_at)?,
            second_inventory.clone(),
            second_capacity,
            reserved_at,
        ),
    ])?;
    let right = AtomicResourceClaimReservation::new(vec![
        shared_reservation(
            ResourceClaimId::new(),
            first_binding.propose_assignment(second_node_id, reserved_at)?,
            second_inventory,
            second_capacity,
            reserved_at,
        ),
        shared_reservation(
            ResourceClaimId::new(),
            second_binding.propose_assignment(first_node_id, reserved_at)?,
            first_inventory.clone(),
            first_capacity,
            reserved_at,
        ),
    ])?;
    let repository = Arc::new(PostgresResourceClaimRepository::new(executor.clone()));
    let barrier = Arc::new(Barrier::new(2));
    let mut tasks = Vec::new();
    for batch in [left, right] {
        let attempted_ids = batch
            .reservations()
            .iter()
            .map(|reservation| reservation.id)
            .collect::<Vec<_>>();
        let repository = Arc::clone(&repository);
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            (attempted_ids, repository.reserve_atomically(batch).await)
        }));
    }

    let mut winner = None;
    let mut loser_ids = None;
    for task in tasks {
        let (attempted_ids, result) =
            tokio::time::timeout(std::time::Duration::from_secs(10), task)
                .await
                .map_err(|_| "opposing atomic Claim candidates deadlocked")??;
        match result {
            Ok(result) if winner.is_none() => winner = Some(result.value),
            Ok(_) => return Err("opposing atomic Claim candidates both committed".into()),
            Err(RepositoryError::Conflict(_)) if loser_ids.is_none() => {
                loser_ids = Some(attempted_ids)
            }
            Err(error) => return Err(error.into()),
        }
    }
    let winner = winner.ok_or("opposing atomic Claim candidates produced no winner")?;
    let loser_ids = loser_ids.ok_or("opposing atomic Claim candidates produced no loser")?;
    assert_eq!(winner.len(), 2);
    assert_eq!(loser_ids.len(), 2);
    for claim_id in loser_ids {
        assert_eq!(
            repository.find(organization_id, claim_id).await,
            Err(RepositoryError::NotFound)
        );
    }
    for claim in winner {
        repository
            .cancel_database_reservation(
                claim.organization_id,
                claim.id,
                claim.aggregate_version,
                reserved_at + Duration::seconds(1),
            )
            .await?;
    }
    Ok(())
}

fn cpu_shared_capacity(
    inventory: &NodeResourceInventory,
) -> Result<u64, Box<dyn std::error::Error>> {
    inventory
        .slots
        .iter()
        .find(|slot| slot.kind == ResourceKind::Cpu && slot.stable_resource_id == "cpu/shared")
        .and_then(|slot| slot.allocation.scalar_amount())
        .ok_or_else(|| "atomic Claim fixture has no scalar shared CPU capacity".into())
}

pub async fn exercise_resource_claims(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload: &WorkloadFixture,
    replica_set: &ReplicaSetFixture,
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
    exercise_required_replica_anti_affinity(
        &repository,
        workload.node_id,
        replica_set,
        shared_inventory.clone(),
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

async fn exercise_required_replica_anti_affinity(
    repository: &Arc<PostgresResourceClaimRepository>,
    node_id: a3s_cloud_control_plane::modules::shared_kernel::domain::NodeId,
    replica_set: &ReplicaSetFixture,
    inventory: NodeResourceInventory,
    base: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let first_binding = replica_set
        .bindings
        .first()
        .ok_or("replica-set fixture omitted its first binding")?
        .propose_assignment(node_id, base + Duration::milliseconds(100))?;
    let second_binding = replica_set
        .bindings
        .get(1)
        .ok_or("replica-set fixture omitted its second binding")?
        .propose_assignment(node_id, base + Duration::milliseconds(100))?;
    if first_binding.replica_id == second_binding.replica_id {
        return Err("replica-set fixture reused one stable replica identity".into());
    }
    let candidates = [
        shared_reservation(
            ResourceClaimId::new(),
            first_binding,
            inventory.clone(),
            1,
            base + Duration::milliseconds(200),
        ),
        shared_reservation(
            ResourceClaimId::new(),
            second_binding,
            inventory,
            1,
            base + Duration::milliseconds(200),
        ),
    ];
    let barrier = Arc::new(Barrier::new(candidates.len()));
    let mut tasks = Vec::with_capacity(candidates.len());
    for reservation in candidates {
        let repository = Arc::clone(repository);
        let barrier = Arc::clone(&barrier);
        let attempted = reservation.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            (attempted, repository.reserve(reservation).await)
        }));
    }

    let mut winner = None;
    let mut loser = None;
    for task in tasks {
        let (reservation, outcome) = task.await?;
        match outcome {
            Ok(result) if winner.is_none() => winner = Some(result.value),
            Ok(_) => return Err("two sibling replicas claimed one node".into()),
            Err(RepositoryError::Conflict(message))
                if message.starts_with("replica placement unavailable: ") && loser.is_none() =>
            {
                loser = Some(reservation);
            }
            Err(error) => return Err(error.into()),
        }
    }
    let winner = winner.ok_or("required replica anti-affinity produced no winner")?;
    let mut loser = loser.ok_or("required replica anti-affinity produced no conflict")?;
    repository
        .cancel_database_reservation(
            winner.organization_id,
            winner.id,
            winner.aggregate_version,
            base + Duration::milliseconds(400),
        )
        .await?;

    loser.reserved_at = base + Duration::milliseconds(500);
    let retried = repository.reserve(loser).await?.value;
    repository
        .cancel_database_reservation(
            retried.organization_id,
            retried.id,
            retried.aggregate_version,
            base + Duration::milliseconds(700),
        )
        .await?;
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
