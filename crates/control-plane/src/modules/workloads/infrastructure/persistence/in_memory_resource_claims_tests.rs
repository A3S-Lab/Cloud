use super::InMemoryResourceClaimRepository;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError,
    ResourceClaimId, WorkloadId, WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    AtomicResourceClaimReservation, DeploymentReplicaBinding, ResourceAllocation,
    ResourceClaimBindingEvidence, ResourceClaimReleaseEvidence, ResourceClaimReservation,
    ResourceClaimState, ResourceKind, ResourceSlotRequest, ResourceUnit,
};
use crate::modules::workloads::domain::repositories::{
    is_placement_unavailable, IResourceClaimRepository,
};
use a3s_cloud_contracts::{NodeResourceInventory, NodeResourceSlot};
use chrono::{Duration, Utc};
use std::sync::Arc;
use tokio::sync::Barrier;

#[test]
fn atomic_reservation_rejects_empty_duplicate_and_cross_tenant_batches() {
    assert!(AtomicResourceClaimReservation::new(Vec::new()).is_err());

    let now = Utc::now();
    let first = reservation(ResourceClaimId::new(), "gpu/GPU-validation", now);
    assert!(AtomicResourceClaimReservation::new(vec![first.clone(), first.clone()]).is_err());

    let cross_tenant = reservation(ResourceClaimId::new(), "gpu/GPU-cross-tenant", now);
    assert_ne!(
        first.binding.organization_id,
        cross_tenant.binding.organization_id
    );
    assert!(AtomicResourceClaimReservation::new(vec![first, cross_tenant]).is_err());

    let organization_id = OrganizationId::new();
    let left_node = NodeId::new();
    let right_node = NodeId::new();
    let left = shared_reservation(organization_id, left_node, 1, 1_000, now);
    let right = shared_reservation(organization_id, right_node, 1, 1_000, now);
    let ordered = AtomicResourceClaimReservation::new(vec![right, left])
        .expect("canonical atomic reservation");
    assert!(ordered
        .reservations()
        .windows(2)
        .all(|members| members[0].node_id < members[1].node_id));
}

#[tokio::test]
async fn atomic_reservation_rolls_back_every_claim_and_slot_after_a_member_conflict() {
    let repository = InMemoryResourceClaimRepository::new();
    let now = Utc::now();
    let first = reservation(ResourceClaimId::new(), "gpu/GPU-atomic", now);
    let mut conflicting = first.clone();
    conflicting.id = ResourceClaimId::new();
    conflicting.binding.deployment_id = DeploymentId::new();
    conflicting.binding.workload_id = WorkloadId::new();
    conflicting.binding.replica_id =
        WorkloadReplicaId::from_uuid(conflicting.binding.workload_id.as_uuid());
    conflicting.binding.member_id =
        WorkloadReplicaMemberId::from_uuid(conflicting.binding.replica_id.as_uuid());
    conflicting.binding.revision_id = WorkloadRevisionId::new();
    conflicting.binding.runtime_unit_id = format!(
        "workload:{}:revision:{}",
        conflicting.binding.workload_id, conflicting.binding.revision_id
    );
    let batch = AtomicResourceClaimReservation::new(vec![first.clone(), conflicting.clone()])
        .expect("atomic reservation batch");

    assert!(matches!(
        repository.reserve_atomically(batch).await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        repository
            .find(first.binding.organization_id, first.id)
            .await,
        Err(RepositoryError::NotFound)
    );
    assert_eq!(
        repository
            .find(conflicting.binding.organization_id, conflicting.id)
            .await,
        Err(RepositoryError::NotFound)
    );

    let recovered = repository
        .reserve(first)
        .await
        .expect("reservation after atomic rollback")
        .value;
    assert_eq!(recovered.slots[0].slot_generation, 1);
}

#[tokio::test]
async fn exact_atomic_reservation_replay_requires_the_complete_batch() {
    let repository = InMemoryResourceClaimRepository::new();
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let first = shared_reservation(organization_id, NodeId::new(), 400, 1_000, now);
    let second = shared_reservation(organization_id, NodeId::new(), 400, 1_000, now);
    let batch = AtomicResourceClaimReservation::new(vec![second.clone(), first.clone()])
        .expect("atomic reservation batch");

    let created = repository
        .reserve_atomically(batch.clone())
        .await
        .expect("atomic reservation");
    let replayed = repository
        .reserve_atomically(batch)
        .await
        .expect("exact atomic replay");
    assert!(!created.replayed);
    assert!(replayed.replayed);
    assert_eq!(created.value, replayed.value);

    let third = shared_reservation(organization_id, NodeId::new(), 1, 1_000, now);
    let partial = AtomicResourceClaimReservation::new(vec![first, third.clone()])
        .expect("partial replay batch");
    assert_eq!(
        repository.reserve_atomically(partial).await,
        Err(RepositoryError::IdempotencyConflict)
    );
    assert_eq!(
        repository.find(organization_id, third.id).await,
        Err(RepositoryError::NotFound)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_atomic_reservations_have_one_complete_winner() {
    let repository = Arc::new(InMemoryResourceClaimRepository::new());
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let first_nodes = [NodeId::new(), NodeId::new()];
    let first_members = first_nodes
        .into_iter()
        .map(|node_id| shared_reservation(organization_id, node_id, 1_000, 1_000, now))
        .collect::<Vec<_>>();
    let second_members = first_nodes
        .into_iter()
        .map(|node_id| shared_reservation(organization_id, node_id, 1_000, 1_000, now))
        .collect::<Vec<_>>();
    let batches = [
        AtomicResourceClaimReservation::new(first_members.clone()).expect("first atomic batch"),
        AtomicResourceClaimReservation::new(second_members.clone()).expect("second atomic batch"),
    ];
    let barrier = Arc::new(Barrier::new(batches.len()));
    let mut tasks = Vec::new();
    for batch in batches {
        let repository = Arc::clone(&repository);
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            repository.reserve_atomically(batch).await
        }));
    }

    let mut winner = None;
    let mut conflicts = 0;
    for task in tasks {
        match task.await.expect("atomic reservation task") {
            Ok(result) if winner.is_none() => winner = Some(result.value),
            Ok(_) => panic!("two atomic batches reserved the same complete capacity"),
            Err(RepositoryError::Conflict(_)) => conflicts += 1,
            Err(error) => panic!("unexpected atomic reservation outcome: {error}"),
        }
    }
    let winner = winner.expect("one atomic reservation winner");
    assert_eq!(winner.len(), 2);
    assert_eq!(conflicts, 1);

    let winner_ids = winner.iter().map(|claim| claim.id).collect::<Vec<_>>();
    for candidate in first_members.iter().chain(&second_members) {
        let stored = repository.find(organization_id, candidate.id).await;
        if winner_ids.contains(&candidate.id) {
            assert!(stored.is_ok());
        } else {
            assert_eq!(stored, Err(RepositoryError::NotFound));
        }
    }
}

#[tokio::test]
async fn exact_reservation_replay_does_not_rotate_fencing_identity() {
    let repository = InMemoryResourceClaimRepository::new();
    let now = Utc::now();
    let reservation = reservation(ResourceClaimId::new(), "gpu/GPU-1", now);

    let first = repository
        .reserve(reservation.clone())
        .await
        .expect("first reservation");
    let replay = repository
        .reserve(reservation.clone())
        .await
        .expect("exact replay");

    assert!(!first.replayed);
    assert!(replay.replayed);
    assert_eq!(first.value, replay.value);

    let mut changed = reservation;
    changed.inventory.generation += 1;
    assert_eq!(
        repository.reserve(changed).await,
        Err(RepositoryError::IdempotencyConflict)
    );
}

#[tokio::test]
async fn concurrent_sibling_replicas_cannot_claim_the_same_node() {
    let repository = Arc::new(InMemoryResourceClaimRepository::new());
    let now = Utc::now();
    let first = shared_reservation(OrganizationId::new(), NodeId::new(), 100, 1_000, now);
    let mut sibling = first.clone();
    sibling.id = ResourceClaimId::new();
    sibling.binding.deployment_id = DeploymentId::new();
    sibling.binding.replica_id = WorkloadReplicaId::from_uuid(uuid::Uuid::now_v7());
    sibling.binding.member_id =
        WorkloadReplicaMemberId::from_uuid(sibling.binding.replica_id.as_uuid());
    sibling.binding.runtime_unit_id = format!(
        "workload:{}:replica:{}:revision:{}",
        sibling.binding.workload_id, sibling.binding.replica_id, sibling.binding.revision_id
    );

    let barrier = Arc::new(Barrier::new(2));
    let mut tasks = Vec::new();
    for reservation in [first, sibling] {
        let repository = Arc::clone(&repository);
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            repository.reserve(reservation).await
        }));
    }
    let mut winners = 0;
    let mut anti_affinity_conflicts = 0;
    for task in tasks {
        match task.await.expect("reservation task") {
            Ok(_) => winners += 1,
            Err(error) if is_placement_unavailable(&error) => anti_affinity_conflicts += 1,
            Err(error) => panic!("unexpected sibling reservation outcome: {error}"),
        }
    }
    assert_eq!(winners, 1);
    assert_eq!(anti_affinity_conflicts, 1);
}

#[tokio::test]
async fn overlapping_generations_of_one_stable_replica_can_share_its_node() {
    let repository = InMemoryResourceClaimRepository::new();
    let now = Utc::now();
    let first = shared_reservation(OrganizationId::new(), NodeId::new(), 100, 1_000, now);
    repository
        .reserve(first.clone())
        .await
        .expect("initial replica generation");

    let mut update = first;
    update.id = ResourceClaimId::new();
    update.binding.deployment_id = DeploymentId::new();
    update.binding.replica_generation = 2;
    update.binding.runtime_generation = 2;
    repository
        .reserve(update)
        .await
        .expect("rolling generation on the stable replica node");
}

#[tokio::test]
async fn database_only_cancellation_cannot_release_prepared_or_orphaned_claims() {
    let repository = InMemoryResourceClaimRepository::new();
    let now = Utc::now();
    let prepared = repository
        .reserve(reservation(ResourceClaimId::new(), "gpu/GPU-prepared", now))
        .await
        .expect("reserve prepared claim")
        .value;
    let command_id = NodeCommandId::new();
    let preparing = repository
        .begin_preparation(
            prepared.organization_id,
            prepared.id,
            prepared.aggregate_version,
            command_id,
            now + Duration::seconds(1),
        )
        .await
        .expect("begin preparation");
    let prepared = repository
        .record_prepared(
            preparing.organization_id,
            preparing.id,
            preparing.aggregate_version,
            command_id,
            digest('a'),
            now + Duration::seconds(2),
        )
        .await
        .expect("record preparation");
    assert!(matches!(
        repository
            .cancel_database_reservation(
                prepared.organization_id,
                prepared.id,
                prepared.aggregate_version,
                now + Duration::seconds(3),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        repository
            .find(prepared.organization_id, prepared.id)
            .await
            .expect("find prepared claim")
            .state,
        ResourceClaimState::PreparedOnAgent
    );

    let orphaned = repository
        .reserve(reservation(ResourceClaimId::new(), "gpu/GPU-orphaned", now))
        .await
        .expect("reserve orphaned claim")
        .value;
    let orphaned = repository
        .orphan(
            orphaned.organization_id,
            orphaned.id,
            orphaned.aggregate_version,
            "agent state is unknown".into(),
            now + Duration::seconds(1),
        )
        .await
        .expect("orphan claim");
    assert!(matches!(
        repository
            .cancel_database_reservation(
                orphaned.organization_id,
                orphaned.id,
                orphaned.aggregate_version,
                now + Duration::seconds(2),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        repository
            .find(orphaned.organization_id, orphaned.id)
            .await
            .expect("find orphaned claim")
            .state,
        ResourceClaimState::Orphaned
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_reservations_have_one_slot_winner() {
    const CONCURRENCY: usize = 100;
    let repository = Arc::new(InMemoryResourceClaimRepository::new());
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let now = Utc::now();
    let base_reservation = reservation(ResourceClaimId::new(), "gpu/GPU-shared", now);
    let mut tasks = Vec::with_capacity(CONCURRENCY);

    for _ in 0..CONCURRENCY {
        let repository = Arc::clone(&repository);
        let barrier = Arc::clone(&barrier);
        let mut reservation = base_reservation.clone();
        reservation.id = ResourceClaimId::new();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            repository.reserve(reservation).await
        }));
    }

    let mut winners = Vec::new();
    let mut conflicts = 0;
    for task in tasks {
        match task.await.expect("reservation task") {
            Ok(reservation) => winners.push(reservation.value),
            Err(RepositoryError::Conflict(_)) => conflicts += 1,
            Err(error) => panic!("unexpected reservation result: {error}"),
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(conflicts, CONCURRENCY - 1);
    assert_eq!(winners[0].slots[0].slot_generation, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_shared_reservations_fill_capacity_without_exclusive_slot_conflicts() {
    const CONCURRENCY: usize = 20;
    let repository = Arc::new(InMemoryResourceClaimRepository::new());
    let barrier = Arc::new(Barrier::new(CONCURRENCY));
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let node_id = NodeId::new();
    let mut tasks = Vec::with_capacity(CONCURRENCY);

    for _ in 0..CONCURRENCY {
        let repository = Arc::clone(&repository);
        let barrier = Arc::clone(&barrier);
        let reservation = shared_reservation(organization_id, node_id, 250, 1_000, now);
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            repository.reserve(reservation).await
        }));
    }

    let mut winners = Vec::new();
    let mut conflicts = 0;
    for task in tasks {
        match task.await.expect("reservation task") {
            Ok(reservation) => winners.push(reservation.value),
            Err(RepositoryError::Conflict(_)) => conflicts += 1,
            Err(error) => panic!("unexpected reservation result: {error}"),
        }
    }
    assert_eq!(winners.len(), 4);
    assert_eq!(conflicts, CONCURRENCY - 4);
    let mut generations = winners
        .iter()
        .map(|claim| claim.slots[0].slot_generation)
        .collect::<Vec<_>>();
    generations.sort_unstable();
    assert_eq!(generations, vec![1, 2, 3, 4]);
    assert!(winners
        .iter()
        .all(|claim| claim.slots[0].stable_resource_id == "cpu/shared"));
}

#[tokio::test]
async fn orphan_keeps_slot_active_until_trusted_compute_fence() {
    let repository = InMemoryResourceClaimRepository::new();
    let now = Utc::now();
    let first_reservation = reservation(ResourceClaimId::new(), "gpu/GPU-2", now);
    let first = repository
        .reserve(first_reservation.clone())
        .await
        .expect("first reservation")
        .value;
    let orphaned = repository
        .orphan(
            first.organization_id,
            first.id,
            first.aggregate_version,
            "release timed out without fencing evidence".into(),
            now + Duration::seconds(1),
        )
        .await
        .expect("orphan claim");
    assert_eq!(orphaned.state, ResourceClaimState::Orphaned);

    let mut blocked_replacement = first_reservation.clone();
    blocked_replacement.id = ResourceClaimId::new();
    blocked_replacement.reserved_at = now + Duration::seconds(2);
    assert!(matches!(
        repository.reserve(blocked_replacement).await,
        Err(RepositoryError::Conflict(_))
    ));

    let released = repository
        .record_released(
            orphaned.organization_id,
            orphaned.id,
            orphaned.aggregate_version,
            ResourceClaimReleaseEvidence::ComputeFenced {
                instance_generation: 2,
                slots: orphaned.slot_evidence(),
                evidence_digest: digest('f'),
                observed_at: now + Duration::seconds(3),
            },
            now + Duration::seconds(3),
        )
        .await
        .expect("trusted compute fence");
    assert_eq!(released.state, ResourceClaimState::Released);

    let mut replacement_reservation = first_reservation;
    replacement_reservation.id = ResourceClaimId::new();
    replacement_reservation.reserved_at = now + Duration::seconds(4);
    let replacement = repository
        .reserve(replacement_reservation)
        .await
        .expect("replacement reservation")
        .value;
    assert!(replacement.slots[0].slot_generation > first.slots[0].slot_generation);
    assert_ne!(replacement.slots[0].fence_token, first.slots[0].fence_token);
}

#[tokio::test]
async fn repository_persists_exact_agent_lifecycle_evidence() {
    let repository = InMemoryResourceClaimRepository::new();
    let now = Utc::now();
    let claim = repository
        .reserve(reservation(ResourceClaimId::new(), "gpu/GPU-3", now))
        .await
        .expect("reserve")
        .value;
    let prepare_command_id = NodeCommandId::new();
    let preparing = repository
        .begin_preparation(
            claim.organization_id,
            claim.id,
            claim.aggregate_version,
            prepare_command_id,
            now + Duration::seconds(1),
        )
        .await
        .expect("begin preparation");
    let binding_digest = digest('b');
    let prepared = repository
        .record_prepared(
            claim.organization_id,
            claim.id,
            preparing.aggregate_version,
            prepare_command_id,
            binding_digest.clone(),
            now + Duration::seconds(2),
        )
        .await
        .expect("record prepared");
    let bound = repository
        .bind(
            claim.organization_id,
            claim.id,
            prepared.aggregate_version,
            ResourceClaimBindingEvidence {
                runtime_unit_id: prepared.runtime_unit_id.clone(),
                runtime_generation: prepared.runtime_generation,
                binding_digest,
                slots: prepared.slot_evidence(),
                observed_at: now + Duration::seconds(3),
            },
            now + Duration::seconds(3),
        )
        .await
        .expect("bind runtime");
    let release_command_id = NodeCommandId::new();
    let releasing = repository
        .begin_release(
            claim.organization_id,
            claim.id,
            bound.aggregate_version,
            release_command_id,
            now + Duration::seconds(4),
        )
        .await
        .expect("begin release");
    let released = repository
        .record_released(
            claim.organization_id,
            claim.id,
            releasing.aggregate_version,
            ResourceClaimReleaseEvidence::AgentReleased {
                command_id: release_command_id,
                slots: releasing.slot_evidence(),
                evidence_digest: digest('d'),
                observed_at: now + Duration::seconds(5),
            },
            now + Duration::seconds(5),
        )
        .await
        .expect("record release");

    assert_eq!(released.state, ResourceClaimState::Released);
    assert_eq!(
        repository
            .find(claim.organization_id, claim.id)
            .await
            .expect("find claim"),
        released
    );
}

fn reservation(
    id: ResourceClaimId,
    stable_resource_id: &str,
    reserved_at: chrono::DateTime<Utc>,
) -> ResourceClaimReservation {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    let node_id = NodeId::new();
    let allocation = ResourceAllocation::Scalar {
        amount: 1,
        unit: ResourceUnit::Count,
    };
    ResourceClaimReservation {
        id,
        binding: DeploymentReplicaBinding {
            deployment_id: DeploymentId::new(),
            organization_id,
            project_id,
            environment_id,
            workload_id,
            revision_id,
            replica_id: WorkloadReplicaId::from_uuid(workload_id.as_uuid()),
            replica_generation: 1,
            member_id: WorkloadReplicaMemberId::from_uuid(workload_id.as_uuid()),
            node_id: Some(node_id),
            placement_generation: 1,
            runtime_unit_id: format!("workload:{workload_id}:revision:{revision_id}"),
            runtime_generation: 1,
            created_at: reserved_at,
            updated_at: reserved_at,
        },
        node_id,
        inventory: NodeResourceInventory::new(
            node_id.as_uuid(),
            uuid::Uuid::now_v7(),
            1,
            reserved_at,
            vec![NodeResourceSlot::new(
                ResourceKind::Accelerator,
                stable_resource_id,
                allocation.clone(),
            )
            .expect("inventory slot")],
        )
        .expect("inventory"),
        topology_digest: digest('b'),
        slots: vec![ResourceSlotRequest::new(
            ResourceKind::Accelerator,
            stable_resource_id,
            allocation,
        )
        .expect("slot request")],
        reserved_at,
    }
}

fn shared_reservation(
    organization_id: OrganizationId,
    node_id: NodeId,
    amount: u64,
    capacity: u64,
    reserved_at: chrono::DateTime<Utc>,
) -> ResourceClaimReservation {
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let workload_id = WorkloadId::new();
    let revision_id = WorkloadRevisionId::new();
    let allocation = ResourceAllocation::Scalar {
        amount,
        unit: ResourceUnit::MilliCpu,
    };
    ResourceClaimReservation {
        id: ResourceClaimId::new(),
        binding: DeploymentReplicaBinding {
            deployment_id: DeploymentId::new(),
            organization_id,
            project_id,
            environment_id,
            workload_id,
            revision_id,
            replica_id: WorkloadReplicaId::from_uuid(workload_id.as_uuid()),
            replica_generation: 1,
            member_id: WorkloadReplicaMemberId::from_uuid(workload_id.as_uuid()),
            node_id: Some(node_id),
            placement_generation: 1,
            runtime_unit_id: format!("workload:{workload_id}:revision:{revision_id}"),
            runtime_generation: 1,
            created_at: reserved_at,
            updated_at: reserved_at,
        },
        node_id,
        inventory: NodeResourceInventory::new(
            node_id.as_uuid(),
            uuid::Uuid::now_v7(),
            1,
            reserved_at,
            vec![NodeResourceSlot::new(
                ResourceKind::Cpu,
                "cpu/shared",
                ResourceAllocation::Scalar {
                    amount: capacity,
                    unit: ResourceUnit::MilliCpu,
                },
            )
            .expect("inventory slot")],
        )
        .expect("inventory"),
        topology_digest: digest('c'),
        slots: vec![
            ResourceSlotRequest::new(ResourceKind::Cpu, "cpu/shared", allocation)
                .expect("slot request"),
        ],
        reserved_at,
    }
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
