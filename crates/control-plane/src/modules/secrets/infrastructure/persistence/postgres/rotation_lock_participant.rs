//! Secrets-owned same-transaction participant for rotation locks.
//!
//! Workloads secret-rotation reconcile locks one exact Secret + version row
//! inside its own Postgres transaction through this helper instead of mapping
//! the Secrets tables.

use crate::infrastructure::{PostgresPersistenceError, fetch_optional};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, SecretId,
};
use a3s_orm::{PostgresTransaction, sql_query};
use uuid::Uuid;

/// Admission facts retained after locking one Secret version for rotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecretVersionRotationLock {
    pub current_version: u64,
    pub secret_state: String,
    pub version_state: String,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

type LockRow = (u64, String, String, Uuid, Uuid);

/// Lock one exact Secret + version row inside a caller-owned transaction.
pub(crate) async fn lock_secret_version_for_rotation(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    secret_id: SecretId,
    version: u64,
) -> Result<Option<SecretVersionRotationLock>, PostgresPersistenceError> {
    fetch_optional::<LockRow, _>(
        transaction,
        sql_query::<LockRow>(
            "select secrets.current_version, secrets.state, secret_versions.state, secrets.project_id, secrets.environment_id from secrets inner join secret_versions on secrets.id = secret_versions.secret_id where secret_versions.version = ",
        )
        .bind(version)
        .append(" and secrets.organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and secrets.id = ")
        .bind(secret_id.as_uuid())
        .append(" for update of secrets, secret_versions"),
    )
    .await?
    .map(decode)
    .transpose()
    .map_err(Into::into)
}

fn decode(row: LockRow) -> Result<SecretVersionRotationLock, RepositoryError> {
    let (current_version, secret_state, version_state, project_id, environment_id) = row;
    Ok(SecretVersionRotationLock {
        current_version,
        secret_state,
        version_state,
        project_id: ProjectId::from_uuid(project_id),
        environment_id: EnvironmentId::from_uuid(environment_id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_orm::{PostgresDialect, Query};

    #[test]
    fn rotation_lock_query_targets_owner_tables_with_for_update() {
        let query = sql_query::<LockRow>(
            "select secrets.current_version, secrets.state, secret_versions.state, secrets.project_id, secrets.environment_id from secrets inner join secret_versions on secrets.id = secret_versions.secret_id where secret_versions.version = ",
        )
        .bind(3_u64)
        .append(" and secrets.organization_id = ")
        .bind(Uuid::nil())
        .append(" and secrets.id = ")
        .bind(Uuid::nil())
        .append(" for update of secrets, secret_versions")
        .compile(&PostgresDialect)
        .expect("Secret version rotation lock query");
        assert!(
            query
                .sql
                .ends_with("for update of secrets, secret_versions")
        );
        assert!(query.sql.contains(" from secrets "));
        assert!(query.sql.contains(" join secret_versions "));
        assert_eq!(query.parameters.len(), 3);
    }
}
