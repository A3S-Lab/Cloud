use super::schema::WorkloadReplicas;
use super::{operation_requests, queries, replicas, writer_fences};
use crate::infrastructure::{store_outbox, transaction_error, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, IdempotentWrite, OrganizationId, RepositoryError, WorkloadId,
    WorkloadReplicaId, WorkloadReplicaMemberId,
};
use crate::modules::workloads::domain::entities::{WorkloadReplica, WorkloadReplicaLifecycle};
use crate::modules::workloads::domain::events::{WorkloadReplicaEvacuated, WorkloadReplicaRetired};
use crate::modules::workloads::domain::repositories::{
    ReplicaRetirementCompletion, ReplicaRetirementDispatch, ReplicaRuntimeFence,
    RetiringReplicaTarget, WorkloadWriterFenceCommit,
};
use a3s_orm::{
    select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction,
};
use uuid::Uuid;

type CandidateRow = (Uuid, Uuid, Uuid, u64);

pub(super) async fn pending(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<RetiringReplicaTarget>, RepositoryError> {
    let limit = checked_limit(limit)?;
    let candidates = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(candidate_query(limit))
        .await
        .map_err(storage)?
        .rows;
    let mut targets = Vec::with_capacity(candidates.len());
    for (organization_uuid, workload_uuid, replica_uuid, generation) in candidates {
        let organization_id = OrganizationId::from_uuid(organization_uuid);
        let workload_id = WorkloadId::from_uuid(workload_uuid);
        let replica_id = WorkloadReplicaId::from_uuid(replica_uuid);
        let replica =
            replicas::find_replica(executor, organization_id, workload_id, replica_id).await?;
        if replica.lifecycle != WorkloadReplicaLifecycle::Retiring
            || replica.generation != generation
        {
            continue;
        }
        let revision =
            queries::find_revision(executor, organization_id, replica.revision_id).await?;
        let member_id = WorkloadReplicaMemberId::from_uuid(replica.id.as_uuid());
        let member =
            replicas::find_member(executor, organization_id, replica.id, member_id).await?;
        let replica_binding = replicas::find_binding_for_replica_generation(
            executor,
            organization_id,
            replica.id,
            replica.generation,
        )
        .await?;
        let deployment = match &replica_binding {
            Some(binding) => Some(
                queries::find_deployment(executor, organization_id, binding.deployment_id).await?,
            ),
            None => None,
        };
        let target = RetiringReplicaTarget {
            revision,
            replica,
            member,
            deployment,
            replica_binding,
        };
        validate_target(&target).map_err(RepositoryError::Storage)?;
        targets.push(target);
    }
    Ok(targets)
}

pub(super) async fn dispatch(
    executor: &PostgresExecutor,
    dispatch: ReplicaRetirementDispatch,
) -> Result<WorkloadReplica, RepositoryError> {
    executor
        .transaction(move |transaction| Box::pin(dispatch_in_transaction(transaction, dispatch)))
        .await
        .map_err(transaction_error)
}

async fn dispatch_in_transaction(
    transaction: &PostgresTransaction,
    dispatch: ReplicaRetirementDispatch,
) -> Result<WorkloadReplica, PostgresPersistenceError> {
    lock_workload_control(transaction, dispatch.organization_id, dispatch.workload_id).await?;
    let mut replica = replicas::replica_for_update(
        transaction,
        dispatch.organization_id,
        dispatch.workload_id,
        dispatch.replica_id,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    require_generation(&replica, dispatch.replica_generation)?;
    if replica.lifecycle == WorkloadReplicaLifecycle::Retiring
        && replica.retirement_command_id == Some(dispatch.command_id)
    {
        return Ok(replica);
    }
    require_version(&replica, dispatch.expected_replica_version)?;
    let previous_version = replica.aggregate_version;
    replica
        .dispatch_retirement(dispatch.command_id, dispatch.dispatched_at)
        .map_err(RepositoryError::Conflict)?;
    replicas::persist_replica(transaction, &replica, previous_version).await?;
    Ok(replica)
}

pub(super) async fn record_fence(
    executor: &PostgresExecutor,
    fence: ReplicaRuntimeFence,
    writer_fence: Option<WorkloadWriterFenceCommit>,
) -> Result<WorkloadReplica, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(record_fence_in_transaction(
                transaction,
                fence,
                writer_fence,
            ))
        })
        .await
        .map_err(transaction_error)
}

async fn record_fence_in_transaction(
    transaction: &PostgresTransaction,
    fence: ReplicaRuntimeFence,
    writer_fence: Option<WorkloadWriterFenceCommit>,
) -> Result<WorkloadReplica, PostgresPersistenceError> {
    let control =
        lock_workload_control(transaction, fence.organization_id, fence.workload_id).await?;
    let mut replica = replicas::replica_for_update(
        transaction,
        fence.organization_id,
        fence.workload_id,
        fence.replica_id,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    require_generation(&replica, fence.replica_generation)?;
    let replayed = replica.lifecycle == WorkloadReplicaLifecycle::Retiring
        && replica.retirement_command_id == Some(fence.command_id)
        && replica.runtime_fenced_at == Some(canonical_timestamp(fence.fenced_at));
    let mut existing_writer_fence = None;
    if let Some(commit) = &writer_fence {
        let spec = commit.receipt.spec();
        let member = replicas::member_for_update(
            transaction,
            fence.organization_id,
            fence.replica_id,
            spec.member_id,
        )
        .await?
        .ok_or_else(|| invariant("writer-fenced Workload replica member is missing"))?;
        let binding = replicas::binding_for_replica_generation(
            transaction,
            fence.organization_id,
            fence.replica_id,
            fence.replica_generation,
        )
        .await?
        .ok_or_else(|| invariant("writer-fenced Workload replica binding is missing"))?;
        commit
            .validate_replica_retirement(&fence, &control, &replica, &member, &binding)
            .map_err(RepositoryError::Conflict)?;
        existing_writer_fence = writer_fences::find_in_transaction(
            transaction,
            fence.organization_id,
            fence.workload_id,
            fence.replica_generation,
        )
        .await?;
        if existing_writer_fence
            .as_ref()
            .is_some_and(|existing| existing != &commit.receipt)
        {
            return Err(RepositoryError::Conflict(
                "Workload writer-fence receipt replay changed its exact evidence".into(),
            )
            .into());
        }
        if existing_writer_fence.is_some() && !replayed {
            return Err(invariant(
                "Workload writer-fence receipt exists before its Runtime fence",
            ));
        }
        if existing_writer_fence.is_some() {
            let existing_operation = operation_requests::find(
                transaction,
                commit.receipt.spec().continuation_operation_id,
            )
            .await?
            .ok_or_else(|| invariant("Workload writer-fence Operation is missing"))?;
            if !existing_operation.has_same_definition(&commit.operation)
                || existing_operation.requested_at != commit.operation.requested_at
            {
                return Err(RepositoryError::Conflict(
                    "Workload writer-fence Operation replay changed its definition".into(),
                )
                .into());
            }
        }
    }
    if !replayed {
        require_version(&replica, fence.expected_replica_version)?;
        let previous_version = replica.aggregate_version;
        replica
            .record_runtime_fenced(fence.command_id, fence.fenced_at)
            .map_err(RepositoryError::Conflict)?;
        replicas::persist_replica(transaction, &replica, previous_version).await?;
    }
    if existing_writer_fence.is_none() {
        if let Some(commit) = &writer_fence {
            operation_requests::insert(transaction, &commit.operation).await?;
            writer_fences::insert(transaction, &commit.receipt).await?;
        }
    }
    Ok(replica)
}

pub(super) async fn complete(
    executor: &PostgresExecutor,
    completion: ReplicaRetirementCompletion,
) -> Result<IdempotentWrite<WorkloadReplica>, RepositoryError> {
    executor
        .transaction(move |transaction| Box::pin(complete_in_transaction(transaction, completion)))
        .await
        .map_err(transaction_error)
}

async fn complete_in_transaction(
    transaction: &PostgresTransaction,
    completion: ReplicaRetirementCompletion,
) -> Result<IdempotentWrite<WorkloadReplica>, PostgresPersistenceError> {
    let replica_binding = replicas::binding_for_replica_generation(
        transaction,
        completion.organization_id,
        completion.replica_id,
        completion.replica_generation,
    )
    .await?;
    let deployment = match &replica_binding {
        Some(binding) => Some(
            queries::deployment_in_transaction(transaction, binding.deployment_id, true)
                .await?
                .ok_or_else(|| {
                    invariant("retiring replica binding references a missing deployment")
                })?,
        ),
        None => None,
    };
    let control = lock_workload_control(
        transaction,
        completion.organization_id,
        completion.workload_id,
    )
    .await?;
    let mut replica = replicas::replica_for_update(
        transaction,
        completion.organization_id,
        completion.workload_id,
        completion.replica_id,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let mut member = replicas::member_for_update(
        transaction,
        completion.organization_id,
        completion.replica_id,
        completion.member_id,
    )
    .await?
    .ok_or_else(|| invariant("retiring replica member is missing"))?;
    if completion.replica_generation.checked_add(1) == Some(replica.generation)
        && replica.lifecycle == WorkloadReplicaLifecycle::Desired
        && replica.evacuation_node_id.is_none()
    {
        return Ok(IdempotentWrite {
            value: replica,
            replayed: true,
        });
    }
    require_generation(&replica, completion.replica_generation)?;
    if replica.lifecycle == WorkloadReplicaLifecycle::Retired && member.node_id.is_none() {
        return Ok(IdempotentWrite {
            value: replica,
            replayed: true,
        });
    }
    require_version(&replica, completion.expected_replica_version)?;
    if member.aggregate_version != completion.expected_member_version {
        return Err(RepositoryError::Conflict(format!(
            "Workload replica member changed from expected version {} to {}",
            completion.expected_member_version, member.aggregate_version
        ))
        .into());
    }
    let evacuation = replica.evacuation_node_id.is_some();
    let remains_desired = replica.ordinal < control.spec.placement_policy.desired_replicas();
    if replica.lifecycle != WorkloadReplicaLifecycle::Retiring
        || evacuation != remains_desired
        || member.node_id != completion.fenced_node_id
        || replica
            .evacuation_node_id
            .is_some_and(|source| Some(source) != completion.fenced_node_id)
    {
        return Err(RepositoryError::Conflict(
            "Workload replica is no longer eligible for generation retirement completion".into(),
        )
        .into());
    }
    if (deployment
        .as_ref()
        .is_some_and(|deployment| deployment.command_id.is_some())
        || completion.fenced_node_id.is_some())
        && replica.runtime_fenced_at.is_none()
    {
        return Err(RepositoryError::Conflict(
            "Workload replica Runtime is not durably fenced".into(),
        )
        .into());
    }
    let previous_replica = replica.clone();
    let previous_member = member.clone();
    let previous_replica_version = replica.aggregate_version;
    let previous_member_version = member.aggregate_version;
    if let Some(node_id) = completion.fenced_node_id {
        member
            .release_after_fencing(node_id, completion.completed_at)
            .map_err(RepositoryError::Conflict)?;
    }
    let event = if evacuation {
        replica
            .complete_evacuation(&member, completion.completed_at)
            .map_err(RepositoryError::Conflict)?;
        WorkloadReplicaEvacuated::envelope(
            &previous_replica,
            &replica,
            &previous_member,
            &member,
            completion.correlation_id,
        )
        .map_err(invariant)?
    } else {
        replica
            .complete_retirement(&member, completion.completed_at)
            .map_err(RepositoryError::Conflict)?;
        WorkloadReplicaRetired::envelope(
            &previous_replica,
            &replica,
            &previous_member,
            &member,
            completion.correlation_id,
        )
        .map_err(invariant)?
    };
    replicas::persist_member(transaction, &member, previous_member_version).await?;
    replicas::persist_replica(transaction, &replica, previous_replica_version).await?;
    store_outbox(transaction, &event).await?;
    Ok(IdempotentWrite {
        value: replica,
        replayed: false,
    })
}

async fn lock_workload_control(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<crate::modules::workloads::domain::entities::WorkloadControl, PostgresPersistenceError>
{
    queries::workload_in_transaction(transaction, organization_id, workload_id, true)
        .await?
        .ok_or(RepositoryError::NotFound)?;
    replicas::control_for_update(transaction, organization_id, workload_id)
        .await?
        .ok_or_else(|| invariant("retiring replica Workload has no control record"))
}

fn candidate_query(limit: u64) -> a3s_orm::query::SelectQuery<WorkloadReplicas, CandidateRow> {
    select_from::<WorkloadReplicas>()
        .select((
            WorkloadReplicas::organization_id(),
            WorkloadReplicas::workload_id(),
            WorkloadReplicas::id(),
            WorkloadReplicas::generation(),
        ))
        .filter(WorkloadReplicas::lifecycle().eq("retiring"))
        .order_by(WorkloadReplicas::updated_at(), OrderDirection::Asc)
        .order_by(WorkloadReplicas::workload_id(), OrderDirection::Asc)
        .order_by(WorkloadReplicas::ordinal(), OrderDirection::Asc)
        .order_by(WorkloadReplicas::id(), OrderDirection::Asc)
        .limit(limit)
}

fn validate_target(target: &RetiringReplicaTarget) -> Result<(), String> {
    target.replica.validate()?;
    target.member.validate()?;
    if target.replica.lifecycle != WorkloadReplicaLifecycle::Retiring
        || target.revision.workload_id != target.replica.workload_id
        || target.revision.id != target.replica.revision_id
        || target.revision.generation != target.replica.revision_generation
        || target.member.organization_id != target.replica.organization_id
        || target.member.workload_id != target.replica.workload_id
        || target.member.replica_id != target.replica.id
        || target.member.id.as_uuid() != target.replica.id.as_uuid()
        || target
            .replica
            .evacuation_node_id
            .is_some_and(|node_id| target.member.node_id != Some(node_id))
        || target.deployment.is_some() != target.replica_binding.is_some()
        || target.member.node_id.is_some() && target.replica_binding.is_none()
    {
        return Err("retiring replica target is inconsistent".into());
    }
    if let (Some(deployment), Some(binding)) = (&target.deployment, &target.replica_binding) {
        if deployment.id != binding.deployment_id
            || deployment.organization_id != target.replica.organization_id
            || deployment.workload_id != target.replica.workload_id
            || deployment.revision_id != target.replica.revision_id
            || binding.organization_id != target.replica.organization_id
            || binding.workload_id != target.replica.workload_id
            || binding.revision_id != target.replica.revision_id
            || binding.replica_id != target.replica.id
            || binding.replica_generation != target.replica.generation
            || binding.member_id != target.member.id
            || binding.runtime_generation != target.replica.generation
            || binding.node_id != deployment.node_id
            || binding.node_id.is_some() && binding.node_id != target.member.node_id
        {
            return Err("retiring replica deployment binding is inconsistent".into());
        }
    }
    Ok(())
}

fn require_generation(
    replica: &WorkloadReplica,
    expected_generation: u64,
) -> Result<(), PostgresPersistenceError> {
    if replica.generation == expected_generation {
        Ok(())
    } else {
        Err(
            RepositoryError::Conflict("Workload replica retirement changed generation".into())
                .into(),
        )
    }
}

fn require_version(
    replica: &WorkloadReplica,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    if replica.aggregate_version == expected_version {
        Ok(())
    } else {
        Err(RepositoryError::Conflict(format!(
            "Workload replica changed from expected version {expected_version} to {}",
            replica.aggregate_version
        ))
        .into())
    }
}

fn checked_limit(limit: usize) -> Result<u64, RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "replica retirement target limit must be between 1 and 10000".into(),
        ));
    }
    u64::try_from(limit)
        .map_err(|_| RepositoryError::Conflict("replica retirement target limit is invalid".into()))
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
    fn retirement_candidate_scan_is_typed_bounded_and_oldest_first() {
        let query = candidate_query(25)
            .compile(&PostgresDialect)
            .expect("replica retirement candidate query");
        assert!(query.sql.contains("\"lifecycle\" ="));
        assert!(query.sql.contains("order by"));
        assert!(query.sql.contains(" limit $"));
        assert_eq!(query.parameters.len(), 2);
    }
}
