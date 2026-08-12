use super::schema::{WorkloadPlacementGroupMembers, WorkloadPlacementGroups};
use super::{queries, replicas};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{
    OrganizationId, RepositoryError, WorkloadPlacementGroupId, WorkloadReplicaId,
    WorkloadReplicaMemberId,
};
use crate::modules::workloads::domain::entities::{
    ServiceTemplate, WorkloadPlacementGroup, WorkloadPlacementGroupMemberPlan,
    WorkloadPlacementGroupMemberRole, WorkloadPlacementGroupState, WorkloadPlacementGroupWrite,
    WorkloadReplicaMember,
};
use crate::modules::workloads::domain::repositories::PlacementGroupMaterialization;
use a3s_orm::{insert_into, select_from, OrderDirection, PostgresExecutor, PostgresTransaction};
use chrono::{DateTime, Utc};
use uuid::Uuid;

type GroupIdentityRow = (Uuid, Uuid, Uuid, Uuid, Uuid);
type GroupRevisionRow = (Uuid, u64, Uuid, u64);
type GroupPlanRow = (u64, String, String, String, String);
type GroupStateRow = (u32, u64, DateTime<Utc>, DateTime<Utc>);
type MemberPlanRow = (Uuid, u32, String, String, serde_json::Value, String);

pub(super) async fn materialize(
    executor: &PostgresExecutor,
    write: WorkloadPlacementGroupWrite,
) -> Result<PlacementGroupMaterialization, RepositoryError> {
    write.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| Box::pin(materialize_in_transaction(transaction, write)))
        .await
        .map_err(transaction_error)
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    group_id: WorkloadPlacementGroupId,
) -> Result<WorkloadPlacementGroup, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                group_in_transaction(transaction, organization_id, group_id, false)
                    .await?
                    .ok_or_else(|| RepositoryError::NotFound.into())
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find_for_replica_generation(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    replica_id: WorkloadReplicaId,
    replica_generation: u64,
) -> Result<WorkloadPlacementGroup, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let group_id = group_id_for_replica_generation(
                    transaction,
                    organization_id,
                    replica_id,
                    replica_generation,
                    false,
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                group_in_transaction(transaction, organization_id, group_id, false)
                    .await?
                    .ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "placement-group replica index references a missing group".into(),
                        )
                    })
            })
        })
        .await
        .map_err(transaction_error)
}

async fn materialize_in_transaction(
    transaction: &PostgresTransaction,
    write: WorkloadPlacementGroupWrite,
) -> Result<PlacementGroupMaterialization, PostgresPersistenceError> {
    // Keep the replica-set mutation order: workload -> control -> replica ->
    // member. The replica row serializes competing plans for one generation.
    let workload = queries::workload_in_transaction(
        transaction,
        write.group.organization_id,
        write.group.workload_id,
        true,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let control = replicas::control_for_update(
        transaction,
        write.group.organization_id,
        write.group.workload_id,
    )
    .await?
    .ok_or_else(|| invariant("Workload is missing its durable control record"))?;
    let replica = replicas::replica_for_update(
        transaction,
        write.group.organization_id,
        write.group.workload_id,
        write.group.replica_id,
    )
    .await?
    .ok_or_else(|| invariant("placement-group replica is missing"))?;

    if let Some(existing) = group_in_transaction(
        transaction,
        write.group.organization_id,
        write.group.id,
        true,
    )
    .await?
    {
        if !existing.same_plan(&write.group) {
            return Err(RepositoryError::IdempotencyConflict.into());
        }
        let replica_members = current_members(transaction, &existing).await?;
        return Ok(PlacementGroupMaterialization {
            group: existing,
            replica_members,
            replayed: true,
        });
    }

    if group_id_for_replica_generation(
        transaction,
        write.group.organization_id,
        write.group.replica_id,
        write.group.replica_generation,
        true,
    )
    .await?
    .is_some()
    {
        return Err(RepositoryError::IdempotencyConflict.into());
    }
    let revision = queries::revision_in_transaction(
        transaction,
        write.group.organization_id,
        write.group.revision_id,
        false,
    )
    .await?
    .ok_or_else(|| invariant("placement-group revision is missing"))?;
    control.validate_against(&workload).map_err(invariant)?;
    let policy = &control.spec.placement_policy;
    write
        .group
        .validate_context(&workload, policy, &revision, &replica)
        .map_err(|_| {
            RepositoryError::Conflict(
                "Workload placement-group plan is stale or inconsistent".into(),
            )
        })?;

    let mut materialized_members = Vec::with_capacity(write.replica_members.len());
    let mut missing_members = Vec::new();
    for member in &write.replica_members {
        match replicas::member_for_update(
            transaction,
            write.group.organization_id,
            write.group.replica_id,
            member.id,
        )
        .await?
        {
            Some(existing) => {
                write
                    .group
                    .validate_available_replica_member(&existing)
                    .map_err(RepositoryError::Conflict)?;
                materialized_members.push(existing);
            }
            None => {
                missing_members.push(member);
                materialized_members.push(member.clone());
            }
        }
    }
    for member in missing_members {
        replicas::insert_member(transaction, member).await?;
    }
    insert_group(transaction, &write.group).await?;
    for member in &write.group.members {
        insert_member_plan(transaction, &write.group, member).await?;
    }
    Ok(PlacementGroupMaterialization {
        group: write.group,
        replica_members: materialized_members,
        replayed: false,
    })
}

async fn group_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    group_id: WorkloadPlacementGroupId,
    for_update: bool,
) -> Result<Option<WorkloadPlacementGroup>, PostgresPersistenceError> {
    let identity_query = select_from::<WorkloadPlacementGroups>()
        .select((
            WorkloadPlacementGroups::id(),
            WorkloadPlacementGroups::organization_id(),
            WorkloadPlacementGroups::project_id(),
            WorkloadPlacementGroups::environment_id(),
            WorkloadPlacementGroups::workload_id(),
        ))
        .filter(WorkloadPlacementGroups::organization_id().eq(organization_id.as_uuid()))
        .filter(WorkloadPlacementGroups::id().eq(group_id.as_uuid()));
    let identity = if for_update {
        fetch_optional(transaction, identity_query.for_update()).await?
    } else {
        fetch_optional(transaction, identity_query).await?
    };
    let Some(identity) = identity else {
        return Ok(None);
    };
    let revision = fetch_optional::<GroupRevisionRow, _>(
        transaction,
        select_from::<WorkloadPlacementGroups>()
            .select((
                WorkloadPlacementGroups::revision_id(),
                WorkloadPlacementGroups::revision_generation(),
                WorkloadPlacementGroups::replica_id(),
                WorkloadPlacementGroups::replica_generation(),
            ))
            .filter(WorkloadPlacementGroups::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkloadPlacementGroups::id().eq(group_id.as_uuid())),
    )
    .await?
    .ok_or_else(|| invariant("placement-group revision row disappeared during restoration"))?;
    let plan = fetch_optional::<GroupPlanRow, _>(
        transaction,
        select_from::<WorkloadPlacementGroups>()
            .select((
                WorkloadPlacementGroups::policy_generation(),
                WorkloadPlacementGroups::placement_policy_digest(),
                WorkloadPlacementGroups::plan_schema(),
                WorkloadPlacementGroups::plan_digest(),
                WorkloadPlacementGroups::state(),
            ))
            .filter(WorkloadPlacementGroups::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkloadPlacementGroups::id().eq(group_id.as_uuid())),
    )
    .await?
    .ok_or_else(|| invariant("placement-group plan row disappeared during restoration"))?;
    let state = fetch_optional::<GroupStateRow, _>(
        transaction,
        select_from::<WorkloadPlacementGroups>()
            .select((
                WorkloadPlacementGroups::member_count(),
                WorkloadPlacementGroups::aggregate_version(),
                WorkloadPlacementGroups::created_at(),
                WorkloadPlacementGroups::updated_at(),
            ))
            .filter(WorkloadPlacementGroups::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkloadPlacementGroups::id().eq(group_id.as_uuid())),
    )
    .await?
    .ok_or_else(|| invariant("placement-group state row disappeared during restoration"))?;
    let member_rows = fetch_all::<MemberPlanRow, _>(
        transaction,
        select_from::<WorkloadPlacementGroupMembers>()
            .select((
                WorkloadPlacementGroupMembers::member_id(),
                WorkloadPlacementGroupMembers::ordinal(),
                WorkloadPlacementGroupMembers::role(),
                WorkloadPlacementGroupMembers::runtime_unit_id(),
                WorkloadPlacementGroupMembers::template(),
                WorkloadPlacementGroupMembers::template_digest(),
            ))
            .filter(WorkloadPlacementGroupMembers::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkloadPlacementGroupMembers::group_id().eq(group_id.as_uuid()))
            .order_by(
                WorkloadPlacementGroupMembers::ordinal(),
                OrderDirection::Asc,
            )
            .order_by(
                WorkloadPlacementGroupMembers::member_id(),
                OrderDirection::Asc,
            ),
    )
    .await?;
    restore_group(identity, revision, plan, state, member_rows)
        .map(Some)
        .map_err(invariant)
}

async fn group_id_for_replica_generation(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    replica_id: WorkloadReplicaId,
    replica_generation: u64,
    for_update: bool,
) -> Result<Option<WorkloadPlacementGroupId>, PostgresPersistenceError> {
    let query = select_from::<WorkloadPlacementGroups>()
        .select(WorkloadPlacementGroups::id())
        .filter(WorkloadPlacementGroups::organization_id().eq(organization_id.as_uuid()))
        .filter(WorkloadPlacementGroups::replica_id().eq(replica_id.as_uuid()))
        .filter(WorkloadPlacementGroups::replica_generation().eq(replica_generation));
    let id = if for_update {
        fetch_optional(transaction, query.for_update()).await?
    } else {
        fetch_optional(transaction, query).await?
    };
    Ok(id.map(WorkloadPlacementGroupId::from_uuid))
}

async fn current_members(
    transaction: &PostgresTransaction,
    group: &WorkloadPlacementGroup,
) -> Result<Vec<WorkloadReplicaMember>, PostgresPersistenceError> {
    let mut members = Vec::with_capacity(group.members.len());
    for planned in &group.members {
        let member = replicas::member_in_transaction(
            transaction,
            group.organization_id,
            group.replica_id,
            planned.member_id,
        )
        .await?
        .filter(|member| member.ordinal == planned.ordinal)
        .ok_or_else(|| invariant("placement-group plan references a missing replica member"))?;
        group
            .validate_replica_member_identity(&member)
            .map_err(invariant)?;
        members.push(member);
    }
    Ok(members)
}

async fn insert_group(
    transaction: &PostgresTransaction,
    group: &WorkloadPlacementGroup,
) -> Result<(), PostgresPersistenceError> {
    let member_count = u32::try_from(group.members.len())
        .map_err(|_| invariant("placement-group member count overflowed"))?;
    let rows = execute(
        transaction,
        insert_into::<WorkloadPlacementGroups>()
            .value(WorkloadPlacementGroups::id(), group.id.as_uuid())
            .value(
                WorkloadPlacementGroups::organization_id(),
                group.organization_id.as_uuid(),
            )
            .value(
                WorkloadPlacementGroups::project_id(),
                group.project_id.as_uuid(),
            )
            .value(
                WorkloadPlacementGroups::environment_id(),
                group.environment_id.as_uuid(),
            )
            .value(
                WorkloadPlacementGroups::workload_id(),
                group.workload_id.as_uuid(),
            )
            .value(
                WorkloadPlacementGroups::revision_id(),
                group.revision_id.as_uuid(),
            )
            .value(
                WorkloadPlacementGroups::revision_generation(),
                group.revision_generation,
            )
            .value(
                WorkloadPlacementGroups::replica_id(),
                group.replica_id.as_uuid(),
            )
            .value(
                WorkloadPlacementGroups::replica_generation(),
                group.replica_generation,
            )
            .value(
                WorkloadPlacementGroups::policy_generation(),
                group.policy_generation,
            )
            .value(
                WorkloadPlacementGroups::placement_policy_digest(),
                group.placement_policy_digest.as_str(),
            )
            .value(
                WorkloadPlacementGroups::plan_schema(),
                group.plan_schema.as_str(),
            )
            .value(
                WorkloadPlacementGroups::plan_digest(),
                group.plan_digest.as_str(),
            )
            .value(WorkloadPlacementGroups::state(), group.state.as_str())
            .value(WorkloadPlacementGroups::member_count(), member_count)
            .value(
                WorkloadPlacementGroups::aggregate_version(),
                group.aggregate_version,
            )
            .value(WorkloadPlacementGroups::created_at(), group.created_at)
            .value(WorkloadPlacementGroups::updated_at(), group.updated_at),
    )
    .await?;
    require_one_row("Workload placement group", rows)
}

async fn insert_member_plan(
    transaction: &PostgresTransaction,
    group: &WorkloadPlacementGroup,
    member: &WorkloadPlacementGroupMemberPlan,
) -> Result<(), PostgresPersistenceError> {
    let template = serde_json::to_value(&member.template).map_err(|error| {
        invariant(format!(
            "could not encode placement-group template: {error}"
        ))
    })?;
    let rows = execute(
        transaction,
        insert_into::<WorkloadPlacementGroupMembers>()
            .value(
                WorkloadPlacementGroupMembers::organization_id(),
                group.organization_id.as_uuid(),
            )
            .value(
                WorkloadPlacementGroupMembers::group_id(),
                group.id.as_uuid(),
            )
            .value(
                WorkloadPlacementGroupMembers::workload_id(),
                group.workload_id.as_uuid(),
            )
            .value(
                WorkloadPlacementGroupMembers::replica_id(),
                group.replica_id.as_uuid(),
            )
            .value(
                WorkloadPlacementGroupMembers::member_id(),
                member.member_id.as_uuid(),
            )
            .value(WorkloadPlacementGroupMembers::ordinal(), member.ordinal)
            .value(WorkloadPlacementGroupMembers::role(), member.role.as_str())
            .value(
                WorkloadPlacementGroupMembers::runtime_unit_id(),
                member.runtime_unit_id.as_str(),
            )
            .value(WorkloadPlacementGroupMembers::template(), template)
            .value(
                WorkloadPlacementGroupMembers::template_digest(),
                member.template_digest.as_str(),
            ),
    )
    .await?;
    require_one_row("Workload placement-group member plan", rows)
}

fn restore_group(
    identity: GroupIdentityRow,
    revision: GroupRevisionRow,
    plan: GroupPlanRow,
    state: GroupStateRow,
    member_rows: Vec<MemberPlanRow>,
) -> Result<WorkloadPlacementGroup, String> {
    if member_rows.len() != state.0 as usize {
        return Err("stored placement-group member count is incomplete".into());
    }
    let members = member_rows
        .into_iter()
        .map(|row| {
            let template: ServiceTemplate = serde_json::from_value(row.4)
                .map_err(|error| format!("stored placement-group template is invalid: {error}"))?;
            Ok(WorkloadPlacementGroupMemberPlan {
                member_id: WorkloadReplicaMemberId::from_uuid(row.0),
                ordinal: row.1,
                role: WorkloadPlacementGroupMemberRole::parse(&row.2)?,
                runtime_unit_id: row.3,
                template,
                template_digest: row.5,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let group = WorkloadPlacementGroup {
        id: WorkloadPlacementGroupId::from_uuid(identity.0),
        organization_id: OrganizationId::from_uuid(identity.1),
        project_id: crate::modules::shared_kernel::domain::ProjectId::from_uuid(identity.2),
        environment_id: crate::modules::shared_kernel::domain::EnvironmentId::from_uuid(identity.3),
        workload_id: crate::modules::shared_kernel::domain::WorkloadId::from_uuid(identity.4),
        revision_id: crate::modules::shared_kernel::domain::WorkloadRevisionId::from_uuid(
            revision.0,
        ),
        revision_generation: revision.1,
        replica_id: WorkloadReplicaId::from_uuid(revision.2),
        replica_generation: revision.3,
        policy_generation: plan.0,
        placement_policy_digest: plan.1,
        plan_schema: plan.2,
        plan_digest: plan.3,
        state: WorkloadPlacementGroupState::parse(&plan.4)?,
        members,
        aggregate_version: state.1,
        created_at: state.2,
        updated_at: state.3,
    };
    group.validate()?;
    Ok(group)
}

fn invariant(error: impl Into<String>) -> PostgresPersistenceError {
    PostgresPersistenceError::Invariant(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_orm::{PostgresDialect, Query};

    #[test]
    fn placement_group_queries_use_tenant_and_generation_fences() {
        let query = select_from::<WorkloadPlacementGroups>()
            .select(WorkloadPlacementGroups::id())
            .filter(WorkloadPlacementGroups::organization_id().eq(Uuid::nil()))
            .filter(WorkloadPlacementGroups::replica_id().eq(Uuid::nil()))
            .filter(WorkloadPlacementGroups::replica_generation().eq(7_u64))
            .for_update()
            .compile(&PostgresDialect)
            .expect("placement-group lookup");
        assert!(query.sql.contains("\"organization_id\" ="));
        assert!(query.sql.contains("\"replica_generation\" ="));
        assert!(query.sql.ends_with(" for update"));
    }
}
