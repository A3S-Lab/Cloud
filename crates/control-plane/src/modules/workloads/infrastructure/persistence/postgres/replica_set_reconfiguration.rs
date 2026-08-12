use super::schema::WorkloadControls;
use super::{queries, replicas};
use crate::infrastructure::{
    execute, idempotency_replay, require_one_row, store_idempotency, store_outbox,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::domain::events::WorkloadReplicaSetReconfigured;
use crate::modules::workloads::domain::repositories::{
    ReconfigureReplicaSetWrite, ReplicaSetWriteResult,
};
use crate::modules::workloads::domain::services::{
    plan_replica_set_reconfiguration, ReplicaSetReconfigurationError,
};
use a3s_orm::{update_table, PostgresExecutor, PostgresTransaction};
use std::collections::BTreeMap;

pub(super) async fn reconfigure(
    executor: &PostgresExecutor,
    write: ReconfigureReplicaSetWrite,
) -> Result<ReplicaSetWriteResult, RepositoryError> {
    executor
        .transaction(move |transaction| Box::pin(reconfigure_in_transaction(transaction, write)))
        .await
        .map_err(transaction_error)
}

async fn reconfigure_in_transaction(
    transaction: &PostgresTransaction,
    write: ReconfigureReplicaSetWrite,
) -> Result<ReplicaSetWriteResult, PostgresPersistenceError> {
    if let Some(replay) =
        idempotency_replay::<ReplicaSetWriteResult>(transaction, &write.idempotency).await?
    {
        let mut response = replay.value;
        response.replayed = true;
        return Ok(response);
    }

    // Workload mutation always locks in workload -> control -> replicas order.
    // Deployment creation uses the same first lock, so a revision update and a
    // desired-count update cannot validate against different control states.
    let workload = queries::workload_in_transaction(
        transaction,
        write.organization_id,
        write.workload_id,
        true,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let current_control =
        replicas::control_for_update(transaction, write.organization_id, write.workload_id)
            .await?
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "Workload is missing its durable control record".into(),
                )
            })?;
    let current_replicas =
        replicas::replicas_for_update(transaction, write.organization_id, write.workload_id)
            .await?;
    let canonical = current_replicas
        .iter()
        .find(|replica| replica.ordinal == 0)
        .ok_or_else(|| {
            PostgresPersistenceError::Invariant("Workload is missing its canonical replica".into())
        })?;
    let revision = queries::revision_in_transaction(
        transaction,
        write.organization_id,
        canonical.revision_id,
        false,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "Workload canonical replica references a missing revision".into(),
        )
    })?;
    let previous_by_id = current_replicas
        .iter()
        .cloned()
        .map(|replica| (replica.id, replica))
        .collect::<BTreeMap<_, _>>();
    let reconfiguration = plan_replica_set_reconfiguration(
        &workload,
        current_control.clone(),
        &revision,
        current_replicas,
        write.expected_control_version,
        write.expected_policy_generation,
        write.desired_replicas,
        write.managed_owner.as_ref(),
        write.requested_at,
    )
    .map_err(persistence_error)?;
    let event = WorkloadReplicaSetReconfigured::envelope(
        &current_control,
        &reconfiguration.control,
        write.correlation_id,
    )
    .map_err(PostgresPersistenceError::Invariant)?;

    for member in &reconfiguration.members_to_create {
        if replicas::member_in_transaction(
            transaction,
            member.organization_id,
            member.replica_id,
            member.id,
        )
        .await?
        .is_some()
        {
            return Err(PostgresPersistenceError::Invariant(
                "new Workload replica member identity already exists".into(),
            ));
        }
    }

    persist_control(
        transaction,
        &reconfiguration.control,
        current_control.aggregate_version,
    )
    .await?;
    let mut new_members = reconfiguration
        .members_to_create
        .into_iter()
        .map(|member| (member.replica_id, member))
        .collect::<BTreeMap<_, _>>();
    for replica in &reconfiguration.replicas {
        match previous_by_id.get(&replica.id) {
            Some(previous) if previous != replica => {
                replicas::persist_replica(transaction, replica, previous.aggregate_version).await?;
            }
            Some(_) => {}
            None => {
                replicas::insert_replica(transaction, replica).await?;
                let member = new_members.remove(&replica.id).ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "new Workload replica omitted its canonical member".into(),
                    )
                })?;
                replicas::insert_member(transaction, &member).await?;
            }
        }
    }
    if !new_members.is_empty() {
        return Err(PostgresPersistenceError::Invariant(
            "Workload replica reconfiguration created an orphan member".into(),
        ));
    }

    let response = ReplicaSetWriteResult {
        control: reconfiguration.control,
        replicas: reconfiguration.replicas,
        replayed: false,
    };
    store_outbox(transaction, &event).await?;
    store_idempotency(transaction, &write.idempotency, &response).await?;
    Ok(response)
}

async fn persist_control(
    transaction: &PostgresTransaction,
    control: &crate::modules::workloads::domain::entities::WorkloadControl,
    previous_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let placement_policy = control
        .spec
        .placement_policy
        .document()
        .map_err(PostgresPersistenceError::Invariant)?;
    let rows = execute(
        transaction,
        update_table::<WorkloadControls>()
            .set(WorkloadControls::placement_policy(), placement_policy)
            .set(
                WorkloadControls::placement_policy_digest(),
                control.spec.placement_policy.digest(),
            )
            .set(
                WorkloadControls::aggregate_version(),
                control.aggregate_version,
            )
            .set(WorkloadControls::updated_at(), control.updated_at)
            .filter(WorkloadControls::organization_id().eq(control.organization_id.as_uuid()))
            .filter(WorkloadControls::workload_id().eq(control.workload_id.as_uuid()))
            .filter(WorkloadControls::aggregate_version().eq(previous_version)),
    )
    .await?;
    require_one_row("Workload control reconfiguration", rows)
}

fn persistence_error(error: ReplicaSetReconfigurationError) -> PostgresPersistenceError {
    match error {
        ReplicaSetReconfigurationError::Conflict(message) => {
            RepositoryError::Conflict(message).into()
        }
        ReplicaSetReconfigurationError::Invariant(message) => {
            PostgresPersistenceError::Invariant(message)
        }
    }
}
