use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, is_unique_violation, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::applications::domain::{
    Application, ApplicationRecord, ApplicationRelease, ApplicationWriteReference,
    CreateApplicationWrite, IApplicationRepository, PublishApplicationReleaseWrite,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, IdempotencyRequest, IdempotentWrite, OrganizationId,
    ProjectId, RepositoryError,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use async_trait::async_trait;

use super::postgres_records::{
    application_select, decode_application, decode_release, load_record, lock_application,
    release_select, storage,
};
use super::postgres_writes::{insert_application, insert_release, persist_write};

#[derive(Clone)]
pub struct PostgresApplicationRepository {
    executor: PostgresExecutor,
}

impl PostgresApplicationRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IApplicationRepository for PostgresApplicationRepository {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<ApplicationRecord>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) =
                        idempotency_replay::<ApplicationWriteReference>(transaction, &idempotency)
                            .await?
                    else {
                        return Ok(None);
                    };
                    load_record(transaction, reference.value).await.map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn create(
        &self,
        write: CreateApplicationWrite,
    ) -> Result<IdempotentWrite<ApplicationRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<ApplicationWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_record(transaction, reference.value).await?,
                            replayed: true,
                        });
                    }
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let insertion = async {
                        insert_application(transaction, &write.record.application).await?;
                        insert_release(transaction, &write.record.release).await
                    }
                    .await;
                    match insertion {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Application name or release identity is already in use".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    persist_write(
                        transaction,
                        &write.record,
                        &write.event,
                        write.actor_principal_id,
                        write.request_id,
                        &write.idempotency,
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn publish_release(
        &self,
        write: PublishApplicationReleaseWrite,
    ) -> Result<IdempotentWrite<ApplicationRecord>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<ApplicationWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_record(transaction, reference.value).await?,
                            replayed: true,
                        });
                    }
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let current = lock_application(
                        transaction,
                        write.record.application.organization_id,
                        write.record.application.project_id,
                        write.record.application.id,
                    )
                    .await?;
                    write.validate_against(&current).map_err(|error| {
                        PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                    })?;
                    match insert_release(transaction, &write.record.release).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Application release identity is already in use".into(),
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
                        sql_query::<()>("update applications set current_release_id = ")
                            .bind(write.record.application.current_release_id.as_uuid())
                            .append(", current_release_number = ")
                            .bind(write.record.application.current_release_number)
                            .append(", current_release_digest = ")
                            .bind(write.record.application.current_release_digest.as_str())
                            .append(", aggregate_version = ")
                            .bind(write.record.application.aggregate_version)
                            .append(", updated_at = ")
                            .bind(write.record.application.updated_at)
                            .append(" where organization_id = ")
                            .bind(write.record.application.organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(write.record.application.project_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.record.application.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version),
                    )
                    .await?;
                    match updated {
                        1 => {}
                        0 => {
                            return Err(RepositoryError::Conflict(
                                "Application was revised from a stale aggregate version".into(),
                            )
                            .into())
                        }
                        rows => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "publishing Application release affected {rows} rows"
                            )))
                        }
                    }
                    persist_write(
                        transaction,
                        &write.record,
                        &write.event,
                        write.actor_principal_id,
                        write.request_id,
                        &write.idempotency,
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.record,
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
        application_id: ApplicationId,
    ) -> Result<Option<Application>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                application_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and id = ")
                    .bind(application_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_application)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<Application>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                application_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" order by name_key asc, id asc limit ")
                    .bind(limit),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(decode_application)
            .collect()
    }

    async fn find_release(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        release_id: ApplicationReleaseId,
    ) -> Result<Option<ApplicationRelease>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                release_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" and id = ")
                    .bind(release_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_release)
            .transpose()
    }

    async fn list_releases(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        limit: usize,
    ) -> Result<Vec<ApplicationRelease>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                release_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and application_id = ")
                    .bind(application_id.as_uuid())
                    .append(" order by release_number desc, id asc limit ")
                    .bind(limit),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(decode_release)
            .collect()
    }
}
