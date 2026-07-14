use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, is_unique_violation, store_idempotency,
    store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::projects::domain::entities::{Environment, Project};
use crate::modules::projects::domain::repositories::{IEnvironmentRepository, IProjectRepository};
use crate::modules::projects::domain::value_objects::{EnvironmentName, ProjectName};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresProjectsRepository {
    executor: PostgresExecutor,
}

impl PostgresProjectsRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IProjectRepository for PostgresProjectsRepository {
    async fn create(
        &self,
        project: Project,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Project>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<Project>(transaction, &idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
                        )
                        .bind(project.organization_id.as_uuid())
                        .append(", ")
                        .bind(project.id.as_uuid())
                        .append(", ")
                        .bind(project.name.as_str())
                        .append(", ")
                        .bind(project.name.key())
                        .append(", ")
                        .bind(project.aggregate_version)
                        .append(", ")
                        .bind(project.created_at)
                        .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating project affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "project name is already in use".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &event).await?;
                    store_idempotency(transaction, &idempotency, &project).await?;
                    Ok(IdempotentWrite {
                        value: project,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Option<Project>, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<(Uuid, Uuid, String, u64, DateTime<Utc>)>(
                    "select organization_id, id, name, aggregate_version, created_at from projects where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(project_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        row.map(
            |(organization_id, id, name, aggregate_version, created_at)| {
                let name = ProjectName::parse(name).map_err(|error| {
                    RepositoryError::Storage(format!("stored project name is invalid: {error}"))
                })?;
                Ok(Project {
                    organization_id: OrganizationId::from_uuid(organization_id),
                    id: ProjectId::from_uuid(id),
                    name,
                    aggregate_version,
                    created_at,
                })
            },
        )
        .transpose()
    }

    async fn list(&self, organization_id: OrganizationId) -> Result<Vec<Project>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<(Uuid, Uuid, String, u64, DateTime<Utc>)>(
                    "select organization_id, id, name, aggregate_version, created_at from projects where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(|(organization_id, id, name, aggregate_version, created_at)| {
                let name = ProjectName::parse(name).map_err(|error| {
                    RepositoryError::Storage(format!("stored project name is invalid: {error}"))
                })?;
                Ok(Project {
                    organization_id: OrganizationId::from_uuid(organization_id),
                    id: ProjectId::from_uuid(id),
                    name,
                    aggregate_version,
                    created_at,
                })
            })
            .collect()
    }
}

#[async_trait]
impl IEnvironmentRepository for PostgresProjectsRepository {
    async fn create(
        &self,
        environment: Environment,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Environment>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<Environment>(transaction, &idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (",
                        )
                        .bind(environment.organization_id.as_uuid())
                        .append(", ")
                        .bind(environment.project_id.as_uuid())
                        .append(", ")
                        .bind(environment.id.as_uuid())
                        .append(", ")
                        .bind(environment.name.as_str())
                        .append(", ")
                        .bind(environment.name.key())
                        .append(", ")
                        .bind(environment.aggregate_version)
                        .append(", ")
                        .bind(environment.created_at)
                        .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating environment affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "environment name is already in use".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &event).await?;
                    store_idempotency(transaction, &idempotency, &environment).await?;
                    Ok(IdempotentWrite {
                        value: environment,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Option<Environment>, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<(Uuid, Uuid, Uuid, String, u64, DateTime<Utc>)>(
                    "select organization_id, project_id, id, name, aggregate_version, created_at from environments where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and id = ")
                .bind(environment_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        row.map(
            |(organization_id, project_id, id, name, aggregate_version, created_at)| {
                let name = EnvironmentName::parse(name).map_err(|error| {
                    RepositoryError::Storage(format!("stored environment name is invalid: {error}"))
                })?;
                Ok(Environment {
                    organization_id: OrganizationId::from_uuid(organization_id),
                    project_id: ProjectId::from_uuid(project_id),
                    id: EnvironmentId::from_uuid(id),
                    name,
                    aggregate_version,
                    created_at,
                })
            },
        )
        .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> Result<Vec<Environment>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<(Uuid, Uuid, Uuid, String, u64, DateTime<Utc>)>(
                    "select organization_id, project_id, id, name, aggregate_version, created_at from environments where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(
                |(organization_id, project_id, id, name, aggregate_version, created_at)| {
                    let name = EnvironmentName::parse(name).map_err(|error| {
                        RepositoryError::Storage(format!(
                            "stored environment name is invalid: {error}"
                        ))
                    })?;
                    Ok(Environment {
                        organization_id: OrganizationId::from_uuid(organization_id),
                        project_id: ProjectId::from_uuid(project_id),
                        id: EnvironmentId::from_uuid(id),
                        name,
                        aggregate_version,
                        created_at,
                    })
                },
            )
            .collect()
    }
}
