use super::rows::{
    self, DeploymentRow, RevisionRow, WorkloadRow, SELECT_DEPLOYMENTS, SELECT_REVISIONS,
    SELECT_WORKLOADS,
};
use crate::infrastructure::{fetch_optional, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, OrganizationId, ProjectId, RepositoryError, WorkloadId,
    WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{Deployment, Workload, WorkloadRevision};
use crate::modules::workloads::domain::repositories::ActiveRuntimeTarget;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor, PostgresTransaction};
use uuid::Uuid;

pub(super) async fn find_workload(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<Workload, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<WorkloadRow>(SELECT_WORKLOADS)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(workload_id.as_uuid()),
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
            sql_query::<WorkloadRow>(SELECT_WORKLOADS)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and environment_id = ")
                .bind(environment_id.as_uuid())
                .append(" order by name_key asc, id asc"),
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
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<RevisionRow>(SELECT_REVISIONS)
                .append(" join workloads w on w.id = r.workload_id where w.organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and r.id = ")
                .bind(revision_id.as_uuid()),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)
        .and_then(rows::revision)
}

pub(super) async fn list_revisions(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<Vec<WorkloadRevision>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<RevisionRow>(SELECT_REVISIONS)
                .append(" join workloads w on w.id = r.workload_id where w.organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and r.workload_id = ")
                .bind(workload_id.as_uuid())
                .append(" order by r.generation desc, r.id desc"),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(rows::revision)
        .collect()
}

pub(super) async fn find_deployment(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> Result<Deployment, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<DeploymentRow>(SELECT_DEPLOYMENTS)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(deployment_id.as_uuid()),
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
            sql_query::<DeploymentRow>(SELECT_DEPLOYMENTS)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and workload_id = ")
                .bind(workload_id.as_uuid())
                .append(" order by requested_at desc, id desc"),
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
    let limit = i64::try_from(limit)
        .map_err(|_| RepositoryError::Conflict("active Runtime target limit is invalid".into()))?;
    let identities = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<(Uuid, Uuid, Uuid, Uuid)>(
                "select w.organization_id, w.id, w.active_revision_id, d.id from workloads w join deployments d on d.workload_id = w.id and d.revision_id = w.active_revision_id where w.desired_state = 'running' and w.active_revision_id is not null and d.status in ('retiring', 'active') order by w.updated_at asc, w.id asc limit ",
            )
            .bind(limit),
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
            deployment,
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
    let mut query = sql_query::<WorkloadRow>(SELECT_WORKLOADS)
        .append(" where organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and id = ")
        .bind(workload_id.as_uuid());
    if lock {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(rows::workload)
        .transpose()
        .map_err(Into::into)
}

pub(super) async fn deployment_in_transaction(
    transaction: &PostgresTransaction,
    deployment_id: DeploymentId,
    lock: bool,
) -> Result<Option<Deployment>, PostgresPersistenceError> {
    let mut query = sql_query::<DeploymentRow>(SELECT_DEPLOYMENTS)
        .append(" where id = ")
        .bind(deployment_id.as_uuid());
    if lock {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(rows::deployment)
        .transpose()
        .map_err(Into::into)
}

pub(super) async fn revision_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    revision_id: WorkloadRevisionId,
    lock: bool,
) -> Result<Option<WorkloadRevision>, PostgresPersistenceError> {
    let mut query = sql_query::<RevisionRow>(SELECT_REVISIONS)
        .append(" join workloads w on w.id = r.workload_id where w.organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and r.id = ")
        .bind(revision_id.as_uuid());
    if lock {
        query = query.append(" for update of r");
    }
    fetch_optional(transaction, query)
        .await?
        .map(rows::revision)
        .transpose()
        .map_err(Into::into)
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
