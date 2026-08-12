use super::replicas;
use super::rows::{self, DeploymentSelection, RevisionSelection, WorkloadSelection};
use super::schema::{
    ActiveWorkloads, DeploymentReplicaBindings, Deployments, McpServiceProfiles, WorkloadReplicas,
    WorkloadRevisionSkillBindings, WorkloadRevisions, Workloads,
};
use crate::infrastructure::{fetch_all, fetch_optional, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, DeploymentId, EnvironmentId, OrganizationId, ProjectId,
    RepositoryError, Sha256Digest, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, SkillWorkloadRevisionBinding, Workload, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::ActiveRuntimeTarget;
use a3s_orm::{
    select_from, select_from_as, Database, OrderDirection, PostgresDialect, PostgresExecutor,
    PostgresTransaction,
};
use uuid::Uuid;

pub(super) async fn find_workload(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<Workload, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<Workloads>()
                .select(WorkloadSelection)
                .filter(Workloads::organization_id().eq(organization_id.as_uuid()))
                .filter(Workloads::id().eq(workload_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)
        .and_then(rows::workload)
}

pub(super) async fn list_workloads(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> Result<Vec<Workload>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<Workloads>()
                .select(WorkloadSelection)
                .filter(Workloads::organization_id().eq(organization_id.as_uuid()))
                .filter(Workloads::project_id().eq(project_id.as_uuid()))
                .filter(Workloads::environment_id().eq(environment_id.as_uuid()))
                .order_by(Workloads::name_key(), OrderDirection::Asc)
                .order_by(Workloads::id(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(rows::workload)
        .collect()
}

pub(super) async fn find_revision(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    revision_id: WorkloadRevisionId,
) -> Result<WorkloadRevision, RepositoryError> {
    let mut revision = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<WorkloadRevisions>()
                .select(RevisionSelection)
                .inner_join::<Workloads>(
                    WorkloadRevisions::workload_id().eq_column(Workloads::id()),
                )
                .left_join::<McpServiceProfiles>(mcp_profile_join())
                .filter(Workloads::organization_id().eq(organization_id.as_uuid()))
                .filter(WorkloadRevisions::id().eq(revision_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)
        .and_then(rows::revision)?;
    let bindings = skill_bindings_executor(executor, organization_id, revision.id).await?;
    attach_skill_bindings(&mut revision, bindings)?;
    Ok(revision)
}

pub(super) async fn list_revisions(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<Vec<WorkloadRevision>, RepositoryError> {
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<WorkloadRevisions>()
                .select(RevisionSelection)
                .inner_join::<Workloads>(
                    WorkloadRevisions::workload_id().eq_column(Workloads::id()),
                )
                .left_join::<McpServiceProfiles>(mcp_profile_join())
                .filter(Workloads::organization_id().eq(organization_id.as_uuid()))
                .filter(WorkloadRevisions::workload_id().eq(workload_id.as_uuid()))
                .order_by(WorkloadRevisions::generation(), OrderDirection::Desc)
                .order_by(WorkloadRevisions::id(), OrderDirection::Desc),
        )
        .await
        .map_err(storage)?
        .rows;
    let mut revisions = Vec::with_capacity(rows.len());
    for row in rows {
        let mut revision = rows::revision(row)?;
        let bindings = skill_bindings_executor(executor, organization_id, revision.id).await?;
        attach_skill_bindings(&mut revision, bindings)?;
        revisions.push(revision);
    }
    Ok(revisions)
}

pub(super) async fn find_deployment(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> Result<Deployment, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<Deployments>()
                .select(DeploymentSelection)
                .filter(Deployments::organization_id().eq(organization_id.as_uuid()))
                .filter(Deployments::id().eq(deployment_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)
        .and_then(rows::deployment)
}

pub(super) async fn list_deployments(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<Vec<Deployment>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<Deployments>()
                .select(DeploymentSelection)
                .filter(Deployments::organization_id().eq(organization_id.as_uuid()))
                .filter(Deployments::workload_id().eq(workload_id.as_uuid()))
                .order_by(Deployments::requested_at(), OrderDirection::Desc)
                .order_by(Deployments::id(), OrderDirection::Desc),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(rows::deployment)
        .collect()
}

pub(super) async fn list_active_runtime_targets(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<ActiveRuntimeTarget>, RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "active Runtime target limit must be between 1 and 10000".into(),
        ));
    }
    let limit = u64::try_from(limit)
        .map_err(|_| RepositoryError::Conflict("active Runtime target limit is invalid".into()))?;
    let identities = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from_as::<Workloads, ActiveWorkloads>()
                .select((
                    ActiveWorkloads::organization_id(),
                    ActiveWorkloads::id(),
                    ActiveWorkloads::active_revision_id(),
                    Deployments::id(),
                ))
                .inner_join::<Deployments>(
                    Deployments::workload_id()
                        .eq_column(ActiveWorkloads::id())
                        .and(
                            Deployments::revision_id()
                                .eq_column(ActiveWorkloads::active_revision_id()),
                        ),
                )
                .inner_join::<DeploymentReplicaBindings>(
                    DeploymentReplicaBindings::deployment_id()
                        .eq_column(Deployments::id())
                        .and(
                            DeploymentReplicaBindings::revision_id()
                                .eq_column(Deployments::revision_id()),
                        ),
                )
                .inner_join::<WorkloadReplicas>(
                    WorkloadReplicas::id()
                        .eq_column(DeploymentReplicaBindings::replica_id())
                        .and(WorkloadReplicas::workload_id().eq_column(ActiveWorkloads::id()))
                        .and(
                            WorkloadReplicas::revision_id()
                                .eq_column(DeploymentReplicaBindings::revision_id()),
                        )
                        .and(
                            WorkloadReplicas::generation()
                                .eq_column(DeploymentReplicaBindings::replica_generation()),
                        ),
                )
                .filter(ActiveWorkloads::desired_state().eq("running"))
                .filter(WorkloadReplicas::lifecycle().eq("desired"))
                .filter(
                    Deployments::status()
                        .eq("retiring")
                        .or(Deployments::status().eq("active")),
                )
                .order_by(ActiveWorkloads::updated_at(), OrderDirection::Asc)
                .order_by(ActiveWorkloads::id(), OrderDirection::Asc)
                .order_by(Deployments::id(), OrderDirection::Asc)
                .limit(limit),
        )
        .await
        .map_err(storage)?
        .rows;
    let mut targets = Vec::with_capacity(identities.len());
    for (organization_id, workload_id, revision_id, deployment_id) in identities {
        let organization_id = OrganizationId::from_uuid(organization_id);
        let workload = find_workload(
            executor,
            organization_id,
            WorkloadId::from_uuid(workload_id),
        )
        .await?;
        let revision = find_revision(
            executor,
            organization_id,
            WorkloadRevisionId::from_uuid(revision_id),
        )
        .await?;
        let deployment = find_deployment(
            executor,
            organization_id,
            DeploymentId::from_uuid(deployment_id),
        )
        .await?;
        let replica_binding =
            replicas::find_binding(executor, organization_id, deployment.id).await?;
        let replica = replicas::find_replica(
            executor,
            organization_id,
            workload.id,
            replica_binding.replica_id,
        )
        .await?;
        let member = replicas::find_member(
            executor,
            organization_id,
            replica.id,
            replica_binding.member_id,
        )
        .await?;
        if workload.desired_state
            != crate::modules::workloads::domain::entities::WorkloadDesiredState::Running
            || workload.active_revision_id != Some(revision.id)
            || revision.workload_id != workload.id
            || deployment.workload_id != workload.id
            || deployment.revision_id != revision.id
            || !matches!(
                deployment.status,
                crate::modules::workloads::domain::entities::DeploymentStatus::Retiring
                    | crate::modules::workloads::domain::entities::DeploymentStatus::Active
            )
        {
            continue;
        }
        targets.push(ActiveRuntimeTarget {
            workload,
            revision,
            replica,
            member,
            deployment,
            replica_binding,
        });
    }
    Ok(targets)
}

pub(super) async fn workload_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    lock: bool,
) -> Result<Option<Workload>, PostgresPersistenceError> {
    let query = select_from::<Workloads>()
        .select(WorkloadSelection)
        .filter(Workloads::organization_id().eq(organization_id.as_uuid()))
        .filter(Workloads::id().eq(workload_id.as_uuid()));
    let query = if lock { query.for_update() } else { query };
    let row = fetch_optional(transaction, query).await?;
    row.map(rows::workload).transpose().map_err(Into::into)
}

pub(super) async fn deployment_in_transaction(
    transaction: &PostgresTransaction,
    deployment_id: DeploymentId,
    lock: bool,
) -> Result<Option<Deployment>, PostgresPersistenceError> {
    let query = select_from::<Deployments>()
        .select(DeploymentSelection)
        .filter(Deployments::id().eq(deployment_id.as_uuid()));
    let query = if lock { query.for_update() } else { query };
    let row = fetch_optional(transaction, query).await?;
    row.map(rows::deployment).transpose().map_err(Into::into)
}

pub(super) async fn revision_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    revision_id: WorkloadRevisionId,
    lock: bool,
) -> Result<Option<WorkloadRevision>, PostgresPersistenceError> {
    let query = select_from::<WorkloadRevisions>()
        .select(RevisionSelection)
        .inner_join::<Workloads>(WorkloadRevisions::workload_id().eq_column(Workloads::id()))
        .left_join::<McpServiceProfiles>(mcp_profile_join())
        .filter(Workloads::organization_id().eq(organization_id.as_uuid()))
        .filter(WorkloadRevisions::id().eq(revision_id.as_uuid()));
    let query = if lock {
        query.for_update_of::<WorkloadRevisions>()
    } else {
        query
    };
    let row = fetch_optional(transaction, query).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let mut revision = rows::revision(row)?;
    let bindings = skill_bindings_transaction(transaction, organization_id, revision.id).await?;
    attach_skill_bindings(&mut revision, bindings)?;
    Ok(Some(revision))
}

type SkillBindingRow = (Uuid, Uuid, Uuid, String, u64);

async fn skill_bindings_executor(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    revision_id: WorkloadRevisionId,
) -> Result<Vec<SkillWorkloadRevisionBinding>, RepositoryError> {
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(skill_bindings_query(organization_id, revision_id))
        .await
        .map_err(storage)?
        .rows;
    decode_skill_bindings(rows).map_err(storage)
}

async fn skill_bindings_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    revision_id: WorkloadRevisionId,
) -> Result<Vec<SkillWorkloadRevisionBinding>, PostgresPersistenceError> {
    let rows = fetch_all::<SkillBindingRow, _>(
        transaction,
        skill_bindings_query(organization_id, revision_id),
    )
    .await?;
    decode_skill_bindings(rows).map_err(PostgresPersistenceError::Invariant)
}

fn skill_bindings_query(
    organization_id: OrganizationId,
    revision_id: WorkloadRevisionId,
) -> impl a3s_orm::Query<Output = SkillBindingRow> {
    select_from::<WorkloadRevisionSkillBindings>()
        .select((
            WorkloadRevisionSkillBindings::organization_id(),
            WorkloadRevisionSkillBindings::asset_id(),
            WorkloadRevisionSkillBindings::asset_release_id(),
            WorkloadRevisionSkillBindings::artifact_digest(),
            WorkloadRevisionSkillBindings::artifact_size_bytes(),
        ))
        .filter(WorkloadRevisionSkillBindings::organization_id().eq(organization_id.as_uuid()))
        .filter(WorkloadRevisionSkillBindings::revision_id().eq(revision_id.as_uuid()))
        .order_by(
            WorkloadRevisionSkillBindings::asset_id(),
            OrderDirection::Asc,
        )
}

fn decode_skill_bindings(
    rows: Vec<SkillBindingRow>,
) -> Result<Vec<SkillWorkloadRevisionBinding>, String> {
    rows.into_iter()
        .map(
            |(organization_id, asset_id, asset_release_id, artifact_digest, size_bytes)| {
                SkillWorkloadRevisionBinding::restore(
                    OrganizationId::from_uuid(organization_id),
                    AssetId::from_uuid(asset_id),
                    AssetReleaseId::from_uuid(asset_release_id),
                    Sha256Digest::parse(artifact_digest)?,
                    size_bytes,
                )
            },
        )
        .collect()
}

fn attach_skill_bindings(
    revision: &mut WorkloadRevision,
    bindings: Vec<SkillWorkloadRevisionBinding>,
) -> Result<(), RepositoryError> {
    for binding in bindings {
        revision.restore_skill_binding(binding).map_err(|error| {
            RepositoryError::Storage(format!(
                "workload revision Skill binding is invalid: {error}"
            ))
        })?;
    }
    Ok(())
}

pub(super) async fn next_revision_generation(
    transaction: &PostgresTransaction,
    workload_id: WorkloadId,
) -> Result<u64, PostgresPersistenceError> {
    let latest = fetch_optional::<u64, _>(
        transaction,
        select_from::<WorkloadRevisions>()
            .select(WorkloadRevisions::generation())
            .filter(WorkloadRevisions::workload_id().eq(workload_id.as_uuid()))
            .order_by(WorkloadRevisions::generation(), OrderDirection::Desc)
            .limit(1),
    )
    .await?
    .unwrap_or_default();
    latest
        .checked_add(1)
        .ok_or_else(|| PostgresPersistenceError::Invariant("workload generation overflowed".into()))
}

fn mcp_profile_join() -> a3s_orm::Expression {
    McpServiceProfiles::organization_id()
        .eq_column(WorkloadRevisions::mcp_organization_id())
        .and(McpServiceProfiles::asset_id().eq_column(WorkloadRevisions::mcp_asset_id()))
        .and(
            McpServiceProfiles::asset_release_id()
                .eq_column(WorkloadRevisions::mcp_asset_release_id()),
        )
        .and(
            McpServiceProfiles::profile_digest().eq_column(WorkloadRevisions::mcp_profile_digest()),
        )
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
