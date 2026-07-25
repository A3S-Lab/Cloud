use crate::workloads_support::WorkloadFixture;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    OrganizationId, RepositoryError, ResourceClaimId,
};
use a3s_cloud_control_plane::modules::workloads::{
    IResourceClaimRepository, IWorkloadRepository, PostgresResourceClaimRepository,
    PostgresWorkloadRepository, ResourceAllocation, ResourceClaimReleaseEvidence,
    ResourceClaimReservation, ResourceClaimState, ResourceKind, ResourceSlotRequest, ResourceUnit,
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
    let now = std::cmp::max(Utc::now(), binding.updated_at + Duration::seconds(1));
    let exact_reservation =
        reservation(ResourceClaimId::new(), binding.clone(), "gpu/GPU-H0-1", now);
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
            now + Duration::seconds(1),
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
                observed_at: now + Duration::seconds(2),
            },
            now + Duration::seconds(2),
        )
        .await?;

    let base = reservation(
        ResourceClaimId::new(),
        binding.clone(),
        "gpu/GPU-H0-1",
        now + Duration::seconds(3),
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
            now + Duration::seconds(4),
        )
        .await?;
    assert_eq!(orphaned.state, ResourceClaimState::Orphaned);
    let mut blocked = base.clone();
    blocked.id = ResourceClaimId::new();
    blocked.reserved_at = now + Duration::seconds(5);
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
                observed_at: now + Duration::seconds(6),
            },
            now + Duration::seconds(6),
        )
        .await?;
    assert_eq!(released.state, ResourceClaimState::Released);

    let mut replacement = base;
    replacement.id = ResourceClaimId::new();
    replacement.reserved_at = now + Duration::seconds(7);
    let replacement = repository.reserve(replacement).await?.value;
    assert!(replacement.slots[0].slot_generation > winner.slots[0].slot_generation);
    assert_ne!(
        replacement.slots[0].fence_token,
        winner.slots[0].fence_token
    );
    Ok(())
}

fn reservation(
    id: ResourceClaimId,
    binding: a3s_cloud_control_plane::modules::workloads::DeploymentReplicaBinding,
    stable_resource_id: &str,
    reserved_at: chrono::DateTime<Utc>,
) -> ResourceClaimReservation {
    ResourceClaimReservation {
        id,
        node_id: binding
            .node_id
            .expect("fixture deployment binding is placed"),
        binding,
        inventory_generation: 11,
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
        .expect("fixture resource slot")],
        reserved_at,
    }
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
