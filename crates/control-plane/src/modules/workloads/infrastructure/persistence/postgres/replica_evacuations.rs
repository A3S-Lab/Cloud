use super::schema::{
    DeploymentReplicaBindings, WorkloadReplicaMembers, WorkloadReplicas, Workloads,
};
use super::{queries, replicas};
use crate::infrastructure::{store_outbox, transaction_error, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, NodeId, OrganizationId, RepositoryError, WorkloadId, WorkloadReplicaId,
    WorkloadReplicaMemberId,
};
use crate::modules::workloads::domain::entities::{WorkloadDesiredState, WorkloadReplicaLifecycle};
use crate::modules::workloads::domain::events::WorkloadReplicaEvacuationRequested;
use crate::modules::workloads::domain::repositories::{
    ReplicaEvacuationCandidate, ReplicaEvacuationRequest,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    select_from, Database, DecodeError, Expression, FromRow, FromValue, OrderDirection,
    PostgresDialect, PostgresExecutor, PostgresTransaction, Row,
};
use uuid::Uuid;

type CandidateRow = (Uuid, Uuid, Uuid, u64, u64, u64, Option<Uuid>, u64);

struct PlacementSelection;

struct PlacementRow(Uuid);

impl Selection for PlacementSelection {
    type Output = PlacementRow;

    fn expressions(self) -> Vec<Expression> {
        vec![WorkloadReplicaMembers::id().expression()]
    }
}

impl FromRow for PlacementRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self(Uuid::from_value(
            row.value(0)
                .ok_or(DecodeError::MissingColumn { index: 0 })?,
            0,
        )?))
    }
}

pub(super) async fn has_placements(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_id: NodeId,
) -> Result<bool, RepositoryError> {
    if node_id.as_uuid().is_nil() {
        return Err(RepositoryError::Conflict(
            "replica placement node is invalid".into(),
        ));
    }
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<WorkloadReplicaMembers>()
                .select(PlacementSelection)
                .filter(WorkloadReplicaMembers::organization_id().eq(organization_id.as_uuid()))
                .filter(WorkloadReplicaMembers::node_id().eq(node_id.as_uuid()))
                .limit(1),
        )
        .await
        .map(|row| row.is_some_and(|row| !row.0.is_nil()))
        .map_err(storage)
}

pub(super) async fn pending(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    source_node_id: NodeId,
    limit: usize,
) -> Result<Vec<ReplicaEvacuationCandidate>, RepositoryError> {
    let limit = checked_limit(limit)?;
    if source_node_id.as_uuid().is_nil() {
        return Err(RepositoryError::Conflict(
            "replica evacuation source node is invalid".into(),
        ));
    }
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(candidate_query(organization_id, source_node_id, limit))
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(candidate_from_row)
        .collect()
}

pub(super) async fn request(
    executor: &PostgresExecutor,
    request: ReplicaEvacuationRequest,
) -> Result<
    IdempotentWrite<crate::modules::workloads::domain::entities::WorkloadReplica>,
    RepositoryError,
> {
    executor
        .transaction(move |transaction| Box::pin(request_in_transaction(transaction, request)))
        .await
        .map_err(transaction_error)
}

async fn request_in_transaction(
    transaction: &PostgresTransaction,
    request: ReplicaEvacuationRequest,
) -> Result<
    IdempotentWrite<crate::modules::workloads::domain::entities::WorkloadReplica>,
    PostgresPersistenceError,
> {
    let candidate = request.candidate;
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
    .ok_or_else(|| invariant("evacuated Workload has no control record"))?;
    let mut replica = replicas::replica_for_update(
        transaction,
        candidate.organization_id,
        candidate.workload_id,
        candidate.replica_id,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let member = replicas::member_for_update(
        transaction,
        candidate.organization_id,
        candidate.replica_id,
        candidate.member_id,
    )
    .await?
    .ok_or_else(|| invariant("evacuated replica member is missing"))?;
    if replica.generation == candidate.replica_generation
        && replica.lifecycle == WorkloadReplicaLifecycle::Retiring
        && replica.evacuation_node_id == Some(candidate.source_node_id)
    {
        return Ok(IdempotentWrite {
            value: replica,
            replayed: true,
        });
    }
    let binding = replicas::binding_for_replica_generation(
        transaction,
        candidate.organization_id,
        candidate.replica_id,
        candidate.replica_generation,
    )
    .await?
    .ok_or_else(|| invariant("evacuated replica has no exact deployment binding"))?;
    if workload.desired_state != WorkloadDesiredState::Running
        || replica.generation != candidate.replica_generation
        || replica.aggregate_version != candidate.expected_replica_version
        || replica.lifecycle != WorkloadReplicaLifecycle::Desired
        || replica.ordinal >= control.spec.placement_policy.desired_replicas()
        || member.aggregate_version != candidate.expected_member_version
        || member.node_id != Some(candidate.source_node_id)
        || member.placement_generation != candidate.placement_generation
        || binding.organization_id != candidate.organization_id
        || binding.workload_id != candidate.workload_id
        || binding.replica_id != candidate.replica_id
        || binding.replica_generation != candidate.replica_generation
        || binding.member_id != candidate.member_id
        || binding.node_id != Some(candidate.source_node_id)
        || binding.placement_generation != candidate.placement_generation
    {
        return Err(RepositoryError::Conflict(
            "Workload replica evacuation candidate changed".into(),
        )
        .into());
    }
    let previous = replica.clone();
    let previous_version = replica.aggregate_version;
    replica
        .request_evacuation(&member, candidate.source_node_id, request.requested_at)
        .map_err(RepositoryError::Conflict)?;
    let event = WorkloadReplicaEvacuationRequested::envelope(
        &previous,
        &replica,
        &member,
        request.correlation_id,
    )
    .map_err(invariant)?;
    replicas::persist_replica(transaction, &replica, previous_version).await?;
    store_outbox(transaction, &event).await?;
    Ok(IdempotentWrite {
        value: replica,
        replayed: false,
    })
}

fn candidate_query(
    organization_id: OrganizationId,
    source_node_id: NodeId,
    limit: u64,
) -> a3s_orm::query::SelectQuery<WorkloadReplicas, CandidateRow> {
    select_from::<WorkloadReplicas>()
        .select((
            WorkloadReplicas::organization_id(),
            WorkloadReplicas::workload_id(),
            WorkloadReplicas::id(),
            WorkloadReplicas::generation(),
            WorkloadReplicas::aggregate_version(),
            WorkloadReplicaMembers::aggregate_version(),
            WorkloadReplicaMembers::node_id(),
            WorkloadReplicaMembers::placement_generation(),
        ))
        .inner_join::<WorkloadReplicaMembers>(
            WorkloadReplicaMembers::organization_id()
                .eq_column(WorkloadReplicas::organization_id())
                .and(
                    WorkloadReplicaMembers::workload_id()
                        .eq_column(WorkloadReplicas::workload_id()),
                )
                .and(WorkloadReplicaMembers::replica_id().eq_column(WorkloadReplicas::id())),
        )
        .inner_join::<DeploymentReplicaBindings>(
            DeploymentReplicaBindings::organization_id()
                .eq_column(WorkloadReplicas::organization_id())
                .and(
                    DeploymentReplicaBindings::workload_id()
                        .eq_column(WorkloadReplicas::workload_id()),
                )
                .and(DeploymentReplicaBindings::replica_id().eq_column(WorkloadReplicas::id()))
                .and(
                    DeploymentReplicaBindings::replica_generation()
                        .eq_column(WorkloadReplicas::generation()),
                )
                .and(
                    DeploymentReplicaBindings::member_id().eq_column(WorkloadReplicaMembers::id()),
                ),
        )
        .inner_join::<Workloads>(
            Workloads::organization_id()
                .eq_column(WorkloadReplicas::organization_id())
                .and(Workloads::id().eq_column(WorkloadReplicas::workload_id())),
        )
        .filter(WorkloadReplicas::organization_id().eq(organization_id.as_uuid()))
        .filter(WorkloadReplicas::lifecycle().eq("desired"))
        .filter(Workloads::desired_state().eq("running"))
        .filter(WorkloadReplicaMembers::node_id().eq(source_node_id.as_uuid()))
        .filter(DeploymentReplicaBindings::node_id().eq(source_node_id.as_uuid()))
        .filter(
            DeploymentReplicaBindings::placement_generation()
                .eq_column(WorkloadReplicaMembers::placement_generation()),
        )
        .order_by(WorkloadReplicas::updated_at(), OrderDirection::Asc)
        .order_by(WorkloadReplicas::workload_id(), OrderDirection::Asc)
        .order_by(WorkloadReplicas::ordinal(), OrderDirection::Asc)
        .order_by(WorkloadReplicas::id(), OrderDirection::Asc)
        .limit(limit)
}

fn candidate_from_row(row: CandidateRow) -> Result<ReplicaEvacuationCandidate, RepositoryError> {
    let source_node_id = row
        .6
        .map(NodeId::from_uuid)
        .ok_or_else(|| RepositoryError::Storage("evacuation source node is missing".into()))?;
    let replica_id = WorkloadReplicaId::from_uuid(row.2);
    let candidate = ReplicaEvacuationCandidate {
        organization_id: OrganizationId::from_uuid(row.0),
        workload_id: WorkloadId::from_uuid(row.1),
        replica_id,
        replica_generation: row.3,
        expected_replica_version: row.4,
        member_id: WorkloadReplicaMemberId::from_uuid(replica_id.as_uuid()),
        expected_member_version: row.5,
        source_node_id,
        placement_generation: row.7,
    };
    if candidate.organization_id.as_uuid().is_nil()
        || candidate.workload_id.as_uuid().is_nil()
        || candidate.replica_id.as_uuid().is_nil()
        || candidate.member_id.as_uuid().is_nil()
        || candidate.source_node_id.as_uuid().is_nil()
        || candidate.replica_generation == 0
        || candidate.expected_replica_version == 0
        || candidate.expected_member_version == 0
        || candidate.placement_generation == 0
    {
        return Err(RepositoryError::Storage(
            "stored replica evacuation candidate is invalid".into(),
        ));
    }
    Ok(candidate)
}

fn checked_limit(limit: usize) -> Result<u64, RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "replica evacuation candidate limit must be between 1 and 10000".into(),
        ));
    }
    u64::try_from(limit)
        .map_err(|_| RepositoryError::Conflict("replica evacuation limit is invalid".into()))
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
    fn evacuation_candidate_scan_is_typed_bounded_and_generation_exact() {
        let query = candidate_query(OrganizationId::new(), NodeId::new(), 25)
            .compile(&PostgresDialect)
            .expect("replica evacuation candidate query");
        assert!(query.sql.contains("inner join"));
        assert!(query
            .sql
            .contains("\"replica_generation\" = \"workload_replicas\".\"generation\""));
        assert!(query.sql.contains("\"lifecycle\" ="));
        assert!(query.sql.contains(" limit $"));
        assert!(query.parameters.len() >= 5);
    }
}
