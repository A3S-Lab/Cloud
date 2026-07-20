use super::queries::load_write;
use super::rows::{SecretRow, SecretVersionRow, SELECT_SECRETS, SELECT_SECRET_VERSIONS};
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::secrets::domain::{
    CreateSecretWrite, RotateSecretWrite, SecretWrite, SecretWriteReference,
    TransitionSecretVersion,
};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, OrganizationId, RepositoryError};
use a3s_orm::{sql_query, PostgresExecutor, PostgresTransaction};

pub(super) async fn replay(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<SecretWrite>, RepositoryError> {
    let idempotency = idempotency.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                replay_in_transaction(transaction, organization_id, &idempotency).await
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn create(
    executor: &PostgresExecutor,
    bundle: CreateSecretWrite,
) -> Result<SecretWrite, RepositoryError> {
    bundle.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_in_transaction(
                    transaction,
                    bundle.secret.organization_id,
                    &bundle.idempotency,
                )
                .await?
                {
                    return Ok(replay);
                }
                let inserted = execute(
                    transaction,
                    sql_query::<()>(
                        "insert into secrets (id, organization_id, project_id, environment_id, name, name_key, state, current_version, aggregate_version, created_at, updated_at, revoked_at) values (",
                    )
                    .bind(bundle.secret.id.as_uuid())
                    .append(", ")
                    .bind(bundle.secret.organization_id.as_uuid())
                    .append(", ")
                    .bind(bundle.secret.project_id.as_uuid())
                    .append(", ")
                    .bind(bundle.secret.environment_id.as_uuid())
                    .append(", ")
                    .bind(bundle.secret.name.as_str())
                    .append(", ")
                    .bind(bundle.secret.name.key())
                    .append(", ")
                    .bind(bundle.secret.state.as_str())
                    .append(", ")
                    .bind(bundle.secret.current_version)
                    .append(", ")
                    .bind(bundle.secret.aggregate_version)
                    .append(", ")
                    .bind(bundle.secret.created_at)
                    .append(", ")
                    .bind(bundle.secret.updated_at)
                    .append(", ")
                    .bind(bundle.secret.revoked_at)
                    .append(")"),
                )
                .await;
                match inserted {
                    Ok(rows) => require_one_row("Secret", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "Secret name is already in use".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                insert_version(transaction, &bundle.version).await?;
                store_outbox(transaction, &bundle.event).await?;
                let reference = reference(&bundle);
                store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                Ok(SecretWrite {
                    secret: bundle.secret,
                    version: bundle.version,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn rotate(
    executor: &PostgresExecutor,
    bundle: RotateSecretWrite,
) -> Result<SecretWrite, RepositoryError> {
    bundle.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_in_transaction(
                    transaction,
                    bundle.secret.organization_id,
                    &bundle.idempotency,
                )
                .await?
                {
                    return Ok(replay);
                }
                let existing =
                    lock_secret(transaction, bundle.secret.organization_id, bundle.secret.id)
                        .await?;
                bundle.validate_against(&existing).map_err(invalid_write)?;
                require_one_row(
                    "Secret rotation",
                    execute(
                        transaction,
                        sql_query::<()>("update secrets set current_version = ")
                            .bind(bundle.secret.current_version)
                            .append(", aggregate_version = ")
                            .bind(bundle.secret.aggregate_version)
                            .append(", updated_at = ")
                            .bind(bundle.secret.updated_at)
                            .append(" where organization_id = ")
                            .bind(bundle.secret.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(bundle.secret.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(bundle.expected_secret_version),
                    )
                    .await?,
                )?;
                insert_version(transaction, &bundle.version).await?;
                store_outbox(transaction, &bundle.event).await?;
                let reference = SecretWriteReference {
                    secret_id: bundle.version.secret_id,
                    version: bundle.version.version,
                };
                store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                Ok(SecretWrite {
                    secret: bundle.secret,
                    version: bundle.version,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn transition_version(
    executor: &PostgresExecutor,
    bundle: TransitionSecretVersion,
) -> Result<SecretWrite, RepositoryError> {
    bundle.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_in_transaction(
                    transaction,
                    bundle.secret.organization_id,
                    &bundle.idempotency,
                )
                .await?
                {
                    return Ok(replay);
                }
                let existing_secret =
                    lock_secret(transaction, bundle.secret.organization_id, bundle.secret.id)
                        .await?;
                let existing_version = lock_version(
                    transaction,
                    bundle.version.secret_id,
                    bundle.version.version,
                )
                .await?;
                bundle
                    .validate_against(&existing_secret, &existing_version)
                    .map_err(invalid_write)?;
                require_one_row(
                    "Secret version revocation",
                    execute(
                        transaction,
                        sql_query::<()>("update secrets set aggregate_version = ")
                            .bind(bundle.secret.aggregate_version)
                            .append(", updated_at = ")
                            .bind(bundle.secret.updated_at)
                            .append(" where organization_id = ")
                            .bind(bundle.secret.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(bundle.secret.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(bundle.expected_secret_version),
                    )
                    .await?,
                )?;
                require_one_row(
                    "Secret version",
                    execute(
                        transaction,
                        sql_query::<()>("update secret_versions set state = ")
                            .bind(bundle.version.state.as_str())
                            .append(", aggregate_version = ")
                            .bind(bundle.version.aggregate_version)
                            .append(", revoked_at = ")
                            .bind(bundle.version.revoked_at)
                            .append(" where secret_id = ")
                            .bind(bundle.version.secret_id.as_uuid())
                            .append(" and version = ")
                            .bind(bundle.version.version)
                            .append(" and aggregate_version = ")
                            .bind(bundle.expected_version),
                    )
                    .await?,
                )?;
                store_outbox(transaction, &bundle.event).await?;
                let reference = SecretWriteReference {
                    secret_id: bundle.version.secret_id,
                    version: bundle.version.version,
                };
                store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                Ok(SecretWrite {
                    secret: bundle.secret,
                    version: bundle.version,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

async fn replay_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<SecretWrite>, PostgresPersistenceError> {
    let Some(replay) = idempotency_replay::<SecretWriteReference>(transaction, idempotency).await?
    else {
        return Ok(None);
    };
    Ok(Some(
        load_write(transaction, organization_id, replay.value, true).await?,
    ))
}

async fn lock_secret(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    secret_id: crate::modules::shared_kernel::domain::SecretId,
) -> Result<crate::modules::secrets::domain::Secret, PostgresPersistenceError> {
    fetch_optional::<SecretRow, _>(
        transaction,
        sql_query::<SecretRow>(SELECT_SECRETS)
            .append(" where s.organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and s.id = ")
            .bind(secret_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?
    .secret()
    .map_err(Into::into)
}

async fn lock_version(
    transaction: &PostgresTransaction,
    secret_id: crate::modules::shared_kernel::domain::SecretId,
    version: u64,
) -> Result<crate::modules::secrets::domain::SecretVersion, PostgresPersistenceError> {
    fetch_optional::<SecretVersionRow, _>(
        transaction,
        sql_query::<SecretVersionRow>(SELECT_SECRET_VERSIONS)
            .append(" where v.secret_id = ")
            .bind(secret_id.as_uuid())
            .append(" and v.version = ")
            .bind(version)
            .append(" for update"),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?
    .version()
    .map_err(Into::into)
}

async fn insert_version(
    transaction: &PostgresTransaction,
    version: &crate::modules::secrets::domain::SecretVersion,
) -> Result<(), PostgresPersistenceError> {
    let inserted = execute(
        transaction,
        sql_query::<()>(
            "insert into secret_versions (secret_id, version, key_id, ciphertext, state, aggregate_version, created_at, revoked_at) values (",
        )
        .bind(version.secret_id.as_uuid())
        .append(", ")
        .bind(version.version)
        .append(", ")
        .bind(version.encrypted_value.key_id())
        .append(", ")
        .bind(version.encrypted_value.ciphertext())
        .append(", ")
        .bind(version.state.as_str())
        .append(", ")
        .bind(version.aggregate_version)
        .append(", ")
        .bind(version.created_at)
        .append(", ")
        .bind(version.revoked_at)
        .append(")"),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("Secret version", rows),
        Err(error) if is_unique_violation(&error) => {
            Err(RepositoryError::Conflict("Secret version already exists".into()).into())
        }
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

fn reference(bundle: &CreateSecretWrite) -> SecretWriteReference {
    SecretWriteReference {
        secret_id: bundle.version.secret_id,
        version: bundle.version.version,
    }
}

fn invalid_write(error: String) -> PostgresPersistenceError {
    RepositoryError::Conflict(format!("Secret write is invalid: {error}")).into()
}

fn invalid_repository_write(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!("Secret write is invalid: {error}"))
}
