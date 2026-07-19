use super::rows::{SecretRow, SecretVersionRow, SELECT_SECRETS, SELECT_SECRET_VERSIONS};
use crate::infrastructure::{fetch_optional, PostgresPersistenceError};
use crate::modules::secrets::domain::{Secret, SecretVersion, SecretWrite, SecretWriteReference};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, SecretId,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor, PostgresTransaction};

pub(super) async fn load_write(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    reference: SecretWriteReference,
    replayed: bool,
) -> Result<SecretWrite, PostgresPersistenceError> {
    let secret = fetch_optional::<SecretRow, _>(
        transaction,
        sql_query::<SecretRow>(SELECT_SECRETS)
            .append(" where s.organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and s.id = ")
            .bind(reference.secret_id.as_uuid()),
    )
    .await?
    .ok_or_else(|| invalid_reference("Secret"))?
    .secret()?;
    let version = fetch_optional::<SecretVersionRow, _>(
        transaction,
        sql_query::<SecretVersionRow>(SELECT_SECRET_VERSIONS)
            .append(" where v.secret_id = ")
            .bind(reference.secret_id.as_uuid())
            .append(" and v.version = ")
            .bind(reference.version),
    )
    .await?
    .ok_or_else(|| invalid_reference("Secret version"))?
    .version()?;
    Ok(SecretWrite {
        secret,
        version,
        replayed,
    })
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    secret_id: SecretId,
) -> Result<Secret, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<SecretRow>(SELECT_SECRETS)
                .append(" where s.organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and s.id = ")
                .bind(secret_id.as_uuid()),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)?
        .secret()
}

pub(super) async fn find_version(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    secret_id: SecretId,
    version: u64,
) -> Result<SecretVersion, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<SecretVersionRow>(SELECT_SECRET_VERSIONS)
                .append(" join secrets s on s.id = v.secret_id where s.organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and v.secret_id = ")
                .bind(secret_id.as_uuid())
                .append(" and v.version = ")
                .bind(version),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)?
        .version()
}

pub(super) async fn list(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> Result<Vec<Secret>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<SecretRow>(SELECT_SECRETS)
                .append(" where s.organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and s.project_id = ")
                .bind(project_id.as_uuid())
                .append(" and s.environment_id = ")
                .bind(environment_id.as_uuid())
                .append(" order by s.created_at, s.id"),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(SecretRow::secret)
        .collect()
}

pub(super) async fn list_versions(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    secret_id: SecretId,
) -> Result<Vec<SecretVersion>, RepositoryError> {
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<SecretVersionRow>(SELECT_SECRET_VERSIONS)
                .append(" join secrets s on s.id = v.secret_id where s.organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and v.secret_id = ")
                .bind(secret_id.as_uuid())
                .append(" order by v.version"),
        )
        .await
        .map_err(storage)?
        .rows;
    if rows.is_empty() {
        return match find(executor, organization_id, secret_id).await {
            Ok(_) => Err(RepositoryError::Storage(
                "stored Secret has no versions".into(),
            )),
            Err(error) => Err(error),
        };
    }
    rows.into_iter().map(SecretVersionRow::version).collect()
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn invalid_reference(resource: &str) -> PostgresPersistenceError {
    PostgresPersistenceError::Invariant(format!("{resource} idempotency reference is invalid"))
}
