use super::InMemoryResourceClaimRepository;
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError,
    ResourceClaimId, WorkloadId, WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    DeploymentReplicaBinding, ResourceAllocation, ResourceClaimBindingEvidence,
    ResourceClaimReleaseEvidence, ResourceClaimReservation, ResourceClaimState, ResourceKind,
    ResourceSlotRequest, ResourceUnit,
};
use crate::modules::workloads::domain::repositories::IResourceClaimRepository;
use chrono::{Duration, Utc};
use std::sync::Arc;
use tokio::sync::Barrier;

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
    changed.inventory_generation += 1;
    assert_eq!(
        repository.reserve(changed).await,
        Err(RepositoryError::IdempotencyConflict)
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
        inventory_generation: 1,
        inventory_digest: digest('a'),
        topology_digest: digest('b'),
        slots: vec![ResourceSlotRequest::new(
            ResourceKind::Accelerator,
            stable_resource_id,
            ResourceAllocation::Scalar {
                amount: 1,
                unit: ResourceUnit::Count,
            },
        )
        .expect("slot request")],
        reserved_at,
    }
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
