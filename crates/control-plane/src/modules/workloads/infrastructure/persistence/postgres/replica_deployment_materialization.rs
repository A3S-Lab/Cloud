use super::schema::{DeploymentReplicaBindings, WorkloadControls, WorkloadReplicas, Workloads};
use super::{create, operation_requests, queries, replicas};
use crate::infrastructure::{store_outbox, transaction_error, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{
    OrganizationId, RepositoryError, WorkloadId, WorkloadReplicaId, WorkloadReplicaMemberId,
    WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{WorkloadDesiredState, WorkloadReplicaLifecycle};
use crate::modules::workloads::domain::repositories::{
    ReplicaDeploymentCandidate, ReplicaDeploymentMaterialization,
};
use crate::modules::workloads::infrastructure::replica_deployment_materialization::{
    build_replica_deployment_write, created_materialization, materialization_from_existing,
};
use a3s_orm::{
    exists, not, select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor,
    PostgresTransaction,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

type CandidateRow = (Uuid, Uuid, Uuid, u32, Uuid, u64, u64);

pub(super) async fn pending(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<ReplicaDeploymentCandidate>, RepositoryError> {
    let limit = checked_limit(limit)?;
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(candidate_query(limit))
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(candidate_from_row)
        .collect()
}

pub(super) async fn materialize(
    executor: &PostgresExecutor,
    candidate: ReplicaDeploymentCandidate,
    requested_at: DateTime<Utc>,
) -> Result<Option<ReplicaDeploymentMaterialization>, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(materialize_in_transaction(
                transaction,
                candidate,
                requested_at,
            ))
        })
        .await
        .map_err(transaction_error)
}

async fn materialize_in_transaction(
    transaction: &PostgresTransaction,
    candidate: ReplicaDeploymentCandidate,
    requested_at: DateTime<Utc>,
) -> Result<Option<ReplicaDeploymentMaterialization>, PostgresPersistenceError> {
    // Replica-set mutation and deployment materialization share this lock order:
    // workload -> control -> replica -> member. This keeps scale changes and
    // concurrent materializers from observing different desired generations.
    let workload = queries::workload_in_transaction(
        transaction,
        candidate.organization_id,
        candidate.workload_id,
        true,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let control = replicas::control_for_update(
        transaction,
        candidate.organization_id,
        candidate.workload_id,
    )
    .await?
    .ok_or_else(|| invariant("Workload is missing its durable control record"))?;
    control.validate_against(&workload).map_err(invariant)?;
    let replica = replicas::replica_for_update(
        transaction,
        candidate.organization_id,
        candidate.workload_id,
        candidate.replica_id,
    )
    .await?
    .ok_or_else(|| invariant("replica deployment candidate is missing"))?;
    if replica.ordinal != candidate.replica_ordinal
        || replica.revision_id != candidate.revision_id
        || replica.revision_generation != candidate.revision_generation
        || replica.generation != candidate.replica_generation
        || replica.lifecycle != WorkloadReplicaLifecycle::Desired
        || replica.ordinal >= control.spec.placement_policy.desired_replicas()
        || control.spec.placement_policy.topology()
            != crate::modules::workloads::domain::entities::PlacementTopology::SingleNode
        || workload.desired_state != WorkloadDesiredState::Running
        || workload
            .active_revision_id
            .is_some_and(|active| active != replica.revision_id)
    {
        return Ok(None);
    }

    if let Some(binding) = replicas::binding_for_replica_generation(
        transaction,
        candidate.organization_id,
        candidate.replica_id,
        candidate.replica_generation,
    )
    .await?
    {
        let deployment =
            queries::deployment_in_transaction(transaction, binding.deployment_id, false)
                .await?
                .ok_or_else(|| invariant("replica binding references a missing deployment"))?;
        return materialization_from_existing(candidate, deployment)
            .map(Some)
            .map_err(invariant);
    }

    let revision = queries::revision_in_transaction(
        transaction,
        candidate.organization_id,
        candidate.revision_id,
        false,
    )
    .await?
    .ok_or_else(|| invariant("replica deployment revision is missing"))?;
    let member_id = WorkloadReplicaMemberId::from_uuid(replica.id.as_uuid());
    let member = replicas::member_for_update(
        transaction,
        candidate.organization_id,
        candidate.replica_id,
        member_id,
    )
    .await?
    .ok_or_else(|| invariant("replica deployment member is missing"))?;
    let write = build_replica_deployment_write(
        candidate,
        &workload,
        &revision,
        &replica,
        &member,
        requested_at,
    )
    .map_err(invariant)?;
    operation_requests::insert(transaction, &write.operation).await?;
    create::insert_deployment(transaction, &write.deployment).await?;
    replicas::insert_binding(transaction, &write.binding).await?;
    store_outbox(transaction, &write.event).await?;
    Ok(Some(created_materialization(candidate, write)))
}

fn candidate_query(limit: u64) -> a3s_orm::query::SelectQuery<WorkloadReplicas, CandidateRow> {
    let existing = select_from::<DeploymentReplicaBindings>()
        .select(DeploymentReplicaBindings::deployment_id())
        .filter(DeploymentReplicaBindings::replica_id().eq_column(WorkloadReplicas::id()))
        .filter(
            DeploymentReplicaBindings::replica_generation()
                .eq_column(WorkloadReplicas::generation()),
        );
    select_from::<WorkloadReplicas>()
        .select((
            WorkloadReplicas::organization_id(),
            WorkloadReplicas::workload_id(),
            WorkloadReplicas::id(),
            WorkloadReplicas::ordinal(),
            WorkloadReplicas::revision_id(),
            WorkloadReplicas::revision_generation(),
            WorkloadReplicas::generation(),
        ))
        .inner_join::<Workloads>(
            Workloads::organization_id()
                .eq_column(WorkloadReplicas::organization_id())
                .and(Workloads::id().eq_column(WorkloadReplicas::workload_id())),
        )
        .inner_join::<WorkloadControls>(
            WorkloadControls::organization_id()
                .eq_column(WorkloadReplicas::organization_id())
                .and(WorkloadControls::workload_id().eq_column(WorkloadReplicas::workload_id())),
        )
        .filter(WorkloadReplicas::lifecycle().eq("desired"))
        .filter(Workloads::desired_state().eq("running"))
        .filter(WorkloadControls::placement_topology().eq("single_node"))
        .filter(WorkloadControls::members_per_replica().eq(1_u32))
        .filter(
            Workloads::active_revision_id()
                .is_null()
                .or(Workloads::active_revision_id().eq_column(WorkloadReplicas::revision_id())),
        )
        .filter(not(exists(existing)))
        .order_by(WorkloadReplicas::updated_at(), OrderDirection::Asc)
        .order_by(WorkloadReplicas::workload_id(), OrderDirection::Asc)
        .order_by(WorkloadReplicas::ordinal(), OrderDirection::Asc)
        .order_by(WorkloadReplicas::id(), OrderDirection::Asc)
        .limit(limit)
}

fn candidate_from_row(row: CandidateRow) -> Result<ReplicaDeploymentCandidate, RepositoryError> {
    let candidate = ReplicaDeploymentCandidate {
        organization_id: OrganizationId::from_uuid(row.0),
        workload_id: WorkloadId::from_uuid(row.1),
        replica_id: WorkloadReplicaId::from_uuid(row.2),
        replica_ordinal: row.3,
        revision_id: WorkloadRevisionId::from_uuid(row.4),
        revision_generation: row.5,
        replica_generation: row.6,
    };
    if candidate.organization_id.as_uuid().is_nil()
        || candidate.workload_id.as_uuid().is_nil()
        || candidate.replica_id.as_uuid().is_nil()
        || candidate.revision_id.as_uuid().is_nil()
        || candidate.revision_generation == 0
        || candidate.replica_generation == 0
    {
        return Err(RepositoryError::Storage(
            "stored replica deployment candidate is invalid".into(),
        ));
    }
    Ok(candidate)
}

fn checked_limit(limit: usize) -> Result<u64, RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "replica deployment candidate limit must be between 1 and 10000".into(),
        ));
    }
    u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict("replica deployment candidate limit is invalid".into())
    })
}

fn invariant(error: impl Into<String>) -> PostgresPersistenceError {
    PostgresPersistenceError::Invariant(error.into())
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_orm::Query;

    #[test]
    fn candidate_scan_uses_only_typed_orm_and_exact_generation_anti_join() {
        let query = candidate_query(25)
            .compile(&PostgresDialect)
            .expect("replica deployment candidate query");
        assert!(query.sql.contains("not (exists"));
        assert!(query
            .sql
            .contains("\"replica_generation\" = \"workload_replicas\".\"generation\""));
        assert!(query.sql.contains("\"lifecycle\" ="));
        assert!(query.sql.contains(" limit $"));
        assert!(query.parameters.len() >= 3);
    }
}
