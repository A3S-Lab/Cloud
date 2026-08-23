use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::projects::domain::entities::{Environment, Project, ProjectAttributionProfile};
use crate::modules::projects::domain::repositories::{
    IEnvironmentRepository, IProjectRepository, ProjectAttributionRecord,
    UpdateProjectAttributionWrite,
};
use crate::modules::projects::domain::value_objects::{
    BusinessOwnerReference, CostAttributionCode, EnvironmentName, ProjectAttributionLabels,
    ProjectName,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId,
    ProjectAttributionProfileId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

type ProjectRow = (Uuid, Uuid, String, u64, Option<Uuid>, DateTime<Utc>);
struct ProjectAttributionProfileRow {
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
    previous_profile_id: Option<Uuid>,
    business_owner_reference: String,
    cost_attribution_code: Option<String>,
    labels: serde_json::Value,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl FromRow for ProjectAttributionProfileRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode_column(row, 0)?,
            project_id: decode_column(row, 1)?,
            id: decode_column(row, 2)?,
            previous_profile_id: decode_column(row, 3)?,
            business_owner_reference: decode_column(row, 4)?,
            cost_attribution_code: decode_column(row, 5)?,
            labels: decode_column(row, 6)?,
            created_by: decode_column(row, 7)?,
            created_at: decode_column(row, 8)?,
        })
    }
}

fn decode_column<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

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
                sql_query::<ProjectRow>(
                    "select organization_id, id, name, aggregate_version, current_attribution_profile_id, created_at from projects where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(project_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        row.map(decode_project).transpose()
    }

    async fn list(&self, organization_id: OrganizationId) -> Result<Vec<Project>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<ProjectRow>(
                    "select organization_id, id, name, aggregate_version, current_attribution_profile_id, created_at from projects where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_project)
            .collect()
    }

    async fn replay_attribution_update(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<ProjectAttributionRecord>>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    idempotency_replay::<ProjectAttributionRecord>(transaction, &idempotency).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn update_attribution(
        &self,
        write: UpdateProjectAttributionWrite,
    ) -> Result<IdempotentWrite<ProjectAttributionRecord>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = idempotency_replay::<ProjectAttributionRecord>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(replayed);
                    }
                    let existing = fetch_optional::<ProjectRow, _>(
                        transaction,
                        sql_query::<ProjectRow>(
                            "select organization_id, id, name, aggregate_version, current_attribution_profile_id, created_at from projects where organization_id = ",
                        )
                        .bind(write.record.project.organization_id.as_uuid())
                        .append(" and id = ")
                        .bind(write.record.project.id.as_uuid())
                        .append(" for update"),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)
                    .and_then(decode_project)?;
                    write.validate_against(&existing).map_err(|_| {
                        RepositoryError::Conflict(
                            "project changed while updating its attribution profile".into(),
                        )
                    })?;

                    let profile = &write.record.attribution_profile;
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into project_attribution_profiles (organization_id, project_id, id, previous_profile_id, business_owner_reference, cost_attribution_code, labels, created_by, created_at) values (",
                        )
                        .bind(profile.organization_id.as_uuid())
                        .append(", ")
                        .bind(profile.project_id.as_uuid())
                        .append(", ")
                        .bind(profile.id.as_uuid())
                        .append(", ")
                        .bind(profile.previous_profile_id.map(|id| id.as_uuid()))
                        .append(", ")
                        .bind(profile.business_owner_reference.as_str())
                        .append(", ")
                        .bind(
                            profile
                                .cost_attribution_code
                                .as_ref()
                                .map(|code| code.as_str()),
                        )
                        .append(", ")
                        .bind(serde_json::to_value(profile.labels.as_map())?)
                        .append(", ")
                        .bind(profile.created_by.as_uuid())
                        .append(", ")
                        .bind(profile.created_at)
                        .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating project attribution profile affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "project attribution profile already exists".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    let updated = execute(
                        transaction,
                        sql_query::<()>(
                            "update projects set aggregate_version = ",
                        )
                        .bind(write.record.project.aggregate_version)
                        .append(", current_attribution_profile_id = ")
                        .bind(profile.id.as_uuid())
                        .append(" where organization_id = ")
                        .bind(write.record.project.organization_id.as_uuid())
                        .append(" and id = ")
                        .bind(write.record.project.id.as_uuid())
                        .append(" and aggregate_version = ")
                        .bind(write.expected_project_version)
                        .append(" and current_attribution_profile_id is not distinct from ")
                        .bind(profile.previous_profile_id.map(|id| id.as_uuid())),
                    )
                    .await?;
                    if updated != 1 {
                        return Err(RepositoryError::Conflict(
                            "project changed while updating its attribution profile".into(),
                        )
                        .into());
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: profile.organization_id.as_uuid(),
                            actor_id: Some(profile.created_by.as_uuid()),
                            action: "project.attribution-profile.updated",
                            aggregate_id: profile.project_id.as_uuid(),
                            occurred_at: profile.created_at,
                            request_id: write.request_id,
                            attribution_scope: AuditWrite::project_attribution(
                                profile.project_id,
                                None,
                            ),
                            details: serde_json::json!({
                                "projectId": profile.project_id,
                                "attributionProfileId": profile.id,
                                "previousAttributionProfileId": profile.previous_profile_id,
                                "aggregateVersion": write.record.project.aggregate_version,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &write.record).await?;
                    Ok(IdempotentWrite {
                        value: write.record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_attribution_profile(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        attribution_profile_id: ProjectAttributionProfileId,
    ) -> Result<Option<ProjectAttributionProfile>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<ProjectAttributionProfileRow>(
                    "select organization_id, project_id, id, previous_profile_id, business_owner_reference, cost_attribution_code, labels, created_by, created_at from project_attribution_profiles where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and id = ")
                .bind(attribution_profile_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_attribution_profile)
            .transpose()
    }
}

fn decode_project(row: ProjectRow) -> Result<Project, RepositoryError> {
    let (organization_id, id, name, aggregate_version, attribution_profile_id, created_at) = row;
    let name = ProjectName::parse(name).map_err(|error| {
        RepositoryError::Storage(format!("stored project name is invalid: {error}"))
    })?;
    Ok(Project {
        organization_id: OrganizationId::from_uuid(organization_id),
        id: ProjectId::from_uuid(id),
        name,
        aggregate_version,
        current_attribution_profile_id: attribution_profile_id
            .map(ProjectAttributionProfileId::from_uuid),
        created_at,
    })
}

fn decode_attribution_profile(
    row: ProjectAttributionProfileRow,
) -> Result<ProjectAttributionProfile, RepositoryError> {
    let labels: BTreeMap<String, String> = serde_json::from_value(row.labels).map_err(|error| {
        RepositoryError::Storage(format!(
            "stored project attribution labels are invalid: {error}"
        ))
    })?;
    ProjectAttributionProfile::create(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        ProjectAttributionProfileId::from_uuid(row.id),
        row.previous_profile_id
            .map(ProjectAttributionProfileId::from_uuid),
        BusinessOwnerReference::parse(row.business_owner_reference).map_err(|error| {
            RepositoryError::Storage(format!(
                "stored project business owner reference is invalid: {error}"
            ))
        })?,
        row.cost_attribution_code
            .map(CostAttributionCode::parse)
            .transpose()
            .map_err(|error| {
                RepositoryError::Storage(format!(
                    "stored project cost attribution code is invalid: {error}"
                ))
            })?,
        ProjectAttributionLabels::parse(labels).map_err(|error| {
            RepositoryError::Storage(format!(
                "stored project attribution labels are invalid: {error}"
            ))
        })?,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
    )
    .map_err(|error| {
        RepositoryError::Storage(format!(
            "stored project attribution profile is invalid: {error}"
        ))
    })
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
