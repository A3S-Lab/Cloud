use super::{deployment_group_bindings, placement_groups, queries, replicas, transitions};
use crate::infrastructure::{transaction_error, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{IdempotentWrite, RepositoryError};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentReplicaBinding, DeploymentStatus, WorkloadPlacementGroup,
    WorkloadReplicaMember,
};
use crate::modules::workloads::domain::repositories::{
    PlacementGroupCancellationWrite, PlacementGroupPlacement, PlacementGroupSchedulingWrite,
};
use crate::modules::workloads::infrastructure::replica_deployment_materialization::{
    validate_existing_group_materialization_context, PlacementGroupDeploymentContext,
};
use a3s_orm::{PostgresExecutor, PostgresTransaction};
use std::collections::BTreeSet;

pub(super) async fn schedule(
    executor: &PostgresExecutor,
    write: PlacementGroupSchedulingWrite,
) -> Result<IdempotentWrite<PlacementGroupPlacement>, RepositoryError> {
    write.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| Box::pin(schedule_in_transaction(transaction, write)))
        .await
        .map_err(transaction_error)
}

pub(super) async fn cancel(
    executor: &PostgresExecutor,
    write: PlacementGroupCancellationWrite,
) -> Result<IdempotentWrite<PlacementGroupPlacement>, RepositoryError> {
    write.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| Box::pin(cancel_in_transaction(transaction, write)))
        .await
        .map_err(transaction_error)
}

async fn schedule_in_transaction(
    transaction: &PostgresTransaction,
    write: PlacementGroupSchedulingWrite,
) -> Result<IdempotentWrite<PlacementGroupPlacement>, PostgresPersistenceError> {
    let mut deployment = queries::deployment_in_transaction(transaction, write.deployment_id, true)
        .await?
        .filter(|deployment| deployment.organization_id == write.organization_id)
        .ok_or(RepositoryError::NotFound)?;
    let group_binding = deployment_group_bindings::find_group_binding_in_transaction(
        transaction,
        write.organization_id,
        write.deployment_id,
        true,
    )
    .await?
    .ok_or_else(|| invariant("placement-group Deployment is missing its group binding"))?;
    require_group_identity(
        group_binding.group_id,
        &group_binding.group_plan_digest,
        group_binding.member_count,
        write.group_id,
        &write.group_plan_digest,
        write.placements.len(),
    )?;
    let group = placement_groups::find_for_replica_generation_in_transaction(
        transaction,
        write.organization_id,
        group_binding.replica_id,
        group_binding.replica_generation,
        true,
    )
    .await?
    .ok_or_else(|| invariant("placement-group plan is missing"))?;
    if group.id != write.group_id || group.plan_digest != write.group_plan_digest {
        return Err(RepositoryError::Conflict(
            "placement-group scheduling write changed the immutable plan".into(),
        )
        .into());
    }
    if deployment.status == DeploymentStatus::Scheduled {
        let bindings = ordered_member_bindings(transaction, &deployment, &group).await?;
        let members = ordered_members(transaction, &deployment, &group).await?;
        require_exact_schedule(&deployment, &bindings, &members, &write)?;
        return Ok(IdempotentWrite {
            value: PlacementGroupPlacement {
                deployment,
                member_bindings: bindings,
            },
            replayed: true,
        });
    }
    if deployment.status != DeploymentStatus::Resolving {
        return Err(RepositoryError::Conflict(format!(
            "placement-group Deployment cannot schedule from {}",
            deployment.status.as_str()
        ))
        .into());
    }
    require_expected_version(&deployment, write.expected_deployment_version)?;
    replicas::require_current_desired_deployment(transaction, &deployment).await?;
    let mut placement_nodes = write
        .placements
        .iter()
        .map(|placement| placement.node_id)
        .collect::<Vec<_>>();
    placement_nodes.sort_unstable();
    for node_id in placement_nodes {
        super::resource_claims::require_node_pool_placement_eligible(
            transaction,
            deployment.organization_id,
            deployment.workload_id,
            node_id,
        )
        .await?;
    }
    let mut bindings = ordered_member_bindings(transaction, &deployment, &group).await?;
    let mut members = ordered_members(transaction, &deployment, &group).await?;

    let workload = queries::workload_in_transaction(
        transaction,
        deployment.organization_id,
        deployment.workload_id,
        false,
    )
    .await?
    .ok_or_else(|| invariant("placement-group Workload is missing"))?;
    let control = replicas::control_for_update(
        transaction,
        deployment.organization_id,
        deployment.workload_id,
    )
    .await?
    .ok_or_else(|| invariant("placement-group Workload control is missing"))?;
    let revision = queries::revision_in_transaction(
        transaction,
        deployment.organization_id,
        deployment.revision_id,
        false,
    )
    .await?
    .ok_or_else(|| invariant("placement-group Workload revision is missing"))?;
    let replica = replicas::replica_for_update(
        transaction,
        deployment.organization_id,
        deployment.workload_id,
        group.replica_id,
    )
    .await?
    .ok_or_else(|| invariant("placement-group replica is missing"))?;
    validate_existing_group_materialization_context(
        &deployment,
        PlacementGroupDeploymentContext {
            workload: &workload,
            policy: &control.spec.placement_policy,
            revision: &revision,
            replica: &replica,
            group: &group,
            members: &members,
        },
        &bindings,
        &group_binding,
    )
    .map_err(RepositoryError::Conflict)?;

    let previous_deployment_version = deployment.aggregate_version;
    let leader_node_id = write
        .placements
        .first()
        .map(|placement| placement.node_id)
        .ok_or_else(|| invariant("placement-group leader is missing"))?;
    deployment
        .schedule(leader_node_id, write.scheduled_at)
        .map_err(RepositoryError::Conflict)?;
    let mut previous_member_versions = Vec::with_capacity(members.len());
    for (((plan, placement), member), binding) in group
        .members
        .iter()
        .zip(&write.placements)
        .zip(&mut members)
        .zip(&mut bindings)
    {
        if plan.ordinal != placement.ordinal || plan.member_id != placement.member_id {
            return Err(RepositoryError::Conflict(
                "placement-group scheduling member changed the immutable plan".into(),
            )
            .into());
        }
        previous_member_versions.push(member.aggregate_version);
        member
            .place(placement.node_id, write.scheduled_at)
            .map_err(RepositoryError::Conflict)?;
        binding
            .assign_placement_group_member(&deployment, member, plan)
            .map_err(RepositoryError::Conflict)?;
    }
    transitions::persist_deployment(transaction, &deployment, previous_deployment_version).await?;
    for ((member, previous_version), binding) in
        members.iter().zip(previous_member_versions).zip(&bindings)
    {
        replicas::persist_member_placement(transaction, member, previous_version).await?;
        deployment_group_bindings::persist_member_assignment(transaction, binding).await?;
    }
    let leader_binding = bindings
        .first()
        .ok_or_else(|| invariant("placement-group leader binding is missing"))?;
    replicas::persist_canonical_binding_assignment(transaction, leader_binding).await?;
    Ok(IdempotentWrite {
        value: PlacementGroupPlacement {
            deployment,
            member_bindings: bindings,
        },
        replayed: false,
    })
}

async fn cancel_in_transaction(
    transaction: &PostgresTransaction,
    write: PlacementGroupCancellationWrite,
) -> Result<IdempotentWrite<PlacementGroupPlacement>, PostgresPersistenceError> {
    let mut deployment = queries::deployment_in_transaction(transaction, write.deployment_id, true)
        .await?
        .filter(|deployment| deployment.organization_id == write.organization_id)
        .ok_or(RepositoryError::NotFound)?;
    let group_binding = deployment_group_bindings::find_group_binding_in_transaction(
        transaction,
        write.organization_id,
        write.deployment_id,
        true,
    )
    .await?
    .ok_or_else(|| invariant("placement-group Deployment is missing its group binding"))?;
    if group_binding.group_id != write.group_id
        || group_binding.group_plan_digest != write.group_plan_digest
    {
        return Err(RepositoryError::Conflict(
            "placement-group cancellation changed the immutable plan".into(),
        )
        .into());
    }
    let group = placement_groups::find_for_replica_generation_in_transaction(
        transaction,
        write.organization_id,
        group_binding.replica_id,
        group_binding.replica_generation,
        true,
    )
    .await?
    .ok_or_else(|| invariant("placement-group plan is missing"))?;
    require_group_identity(
        group_binding.group_id,
        &group_binding.group_plan_digest,
        group_binding.member_count,
        write.group_id,
        &write.group_plan_digest,
        group.members.len(),
    )?;
    if group.id != write.group_id || group.plan_digest != write.group_plan_digest {
        return Err(RepositoryError::Conflict(
            "placement-group cancellation changed the immutable plan".into(),
        )
        .into());
    }
    let bindings = ordered_member_bindings(transaction, &deployment, &group).await?;
    let mut members = ordered_members(transaction, &deployment, &group).await?;
    if deployment.status == DeploymentStatus::Cancelled {
        require_exact_cancellation(&bindings, &members)?;
        return Ok(IdempotentWrite {
            value: PlacementGroupPlacement {
                deployment,
                member_bindings: bindings,
            },
            replayed: true,
        });
    }
    require_expected_version(&deployment, write.expected_deployment_version)?;
    if deployment.status != DeploymentStatus::Cancelling
        || deployment.command_id.is_some()
        || deployment.cleanup_command_id.is_some()
    {
        return Err(RepositoryError::Conflict(
            "placement-group cancellation is not safe before Agent preparation".into(),
        )
        .into());
    }
    let mut previous_versions = Vec::with_capacity(members.len());
    for ((plan, binding), member) in group.members.iter().zip(&bindings).zip(&mut members) {
        if plan.member_id != binding.member_id || plan.member_id != member.id {
            return Err(invariant(
                "placement-group cancellation member is inconsistent",
            ));
        }
        previous_versions.push(member.aggregate_version);
        match binding.node_id {
            Some(node_id) => member
                .release_after_fencing(node_id, write.cancelled_at)
                .map_err(RepositoryError::Conflict)?,
            None if member.node_id.is_none() => {}
            None => {
                return Err(RepositoryError::Conflict(
                    "unassigned placement-group binding has a placed member".into(),
                )
                .into())
            }
        }
    }
    let previous_deployment_version = deployment.aggregate_version;
    deployment
        .cancel(write.cancelled_at)
        .map_err(RepositoryError::Conflict)?;
    transitions::persist_deployment(transaction, &deployment, previous_deployment_version).await?;
    for (member, previous_version) in members.iter().zip(previous_versions) {
        replicas::persist_member_placement(transaction, member, previous_version).await?;
    }
    Ok(IdempotentWrite {
        value: PlacementGroupPlacement {
            deployment,
            member_bindings: bindings,
        },
        replayed: false,
    })
}

async fn ordered_member_bindings(
    transaction: &PostgresTransaction,
    deployment: &Deployment,
    group: &WorkloadPlacementGroup,
) -> Result<Vec<DeploymentReplicaBinding>, PostgresPersistenceError> {
    let bindings = deployment_group_bindings::list_member_bindings_in_transaction(
        transaction,
        deployment.organization_id,
        deployment.id,
        true,
    )
    .await?;
    group
        .members
        .iter()
        .map(|plan| {
            bindings
                .iter()
                .find(|binding| binding.member_id == plan.member_id)
                .cloned()
                .ok_or_else(|| invariant("placement-group Deployment member binding is missing"))
        })
        .collect()
}

async fn ordered_members(
    transaction: &PostgresTransaction,
    deployment: &Deployment,
    group: &WorkloadPlacementGroup,
) -> Result<Vec<WorkloadReplicaMember>, PostgresPersistenceError> {
    let mut members = Vec::with_capacity(group.members.len());
    for plan in &group.members {
        members.push(
            replicas::member_for_update(
                transaction,
                deployment.organization_id,
                group.replica_id,
                plan.member_id,
            )
            .await?
            .ok_or_else(|| invariant("placement-group member is missing"))?,
        );
    }
    Ok(members)
}

fn require_exact_schedule(
    deployment: &Deployment,
    bindings: &[DeploymentReplicaBinding],
    members: &[WorkloadReplicaMember],
    write: &PlacementGroupSchedulingWrite,
) -> Result<(), PostgresPersistenceError> {
    if deployment.node_id != write.placements.first().map(|placement| placement.node_id)
        || bindings.len() != write.placements.len()
        || members.len() != write.placements.len()
        || bindings.iter().zip(&write.placements).zip(members).any(
            |((binding, placement), member)| {
                binding.member_id != placement.member_id
                    || binding.node_id != Some(placement.node_id)
                    || member.id != placement.member_id
                    || member.ordinal != placement.ordinal
                    || member.node_id != Some(placement.node_id)
                    || member.placement_generation != binding.placement_generation
            },
        )
    {
        return Err(RepositoryError::IdempotencyConflict.into());
    }
    let node_ids = bindings
        .iter()
        .filter_map(|binding| binding.node_id)
        .collect::<BTreeSet<_>>();
    if node_ids.len() != bindings.len() {
        return Err(invariant(
            "stored placement-group schedule does not use distinct nodes",
        ));
    }
    Ok(())
}

fn require_exact_cancellation(
    bindings: &[DeploymentReplicaBinding],
    members: &[WorkloadReplicaMember],
) -> Result<(), PostgresPersistenceError> {
    if bindings.len() != members.len()
        || bindings.iter().zip(members).any(|(binding, member)| {
            binding.member_id != member.id
                || member.node_id.is_some()
                || binding.placement_generation != member.placement_generation
        })
    {
        return Err(invariant(
            "cancelled placement-group member state is inconsistent",
        ));
    }
    Ok(())
}

fn require_group_identity(
    stored_group_id: crate::modules::shared_kernel::domain::WorkloadPlacementGroupId,
    stored_plan_digest: &str,
    stored_member_count: u32,
    expected_group_id: crate::modules::shared_kernel::domain::WorkloadPlacementGroupId,
    expected_plan_digest: &str,
    expected_member_count: usize,
) -> Result<(), PostgresPersistenceError> {
    if stored_group_id != expected_group_id
        || stored_plan_digest != expected_plan_digest
        || usize::try_from(stored_member_count).ok() != Some(expected_member_count)
    {
        return Err(RepositoryError::Conflict(
            "placement-group scheduling write changed the immutable plan".into(),
        )
        .into());
    }
    Ok(())
}

fn require_expected_version(
    deployment: &Deployment,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    if deployment.aggregate_version != expected_version {
        return Err(RepositoryError::Conflict(format!(
            "deployment changed from expected version {expected_version} to {}",
            deployment.aggregate_version
        ))
        .into());
    }
    Ok(())
}

fn invariant(message: impl Into<String>) -> PostgresPersistenceError {
    PostgresPersistenceError::Invariant(message.into())
}
