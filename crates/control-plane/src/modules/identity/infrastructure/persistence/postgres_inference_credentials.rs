use super::postgres::PostgresIdentityRepository;
use super::postgres_inference_credentials_schema::InferenceCredentials;
use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, require_one_row,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::InferenceCredential;
use crate::modules::identity::domain::repositories::IInferenceCredentialRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InferenceCredentialId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, update_table, Database, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[async_trait]
impl IInferenceCredentialRepository for PostgresIdentityRepository {
    async fn create_inference_credential(
        &self,
        credential: InferenceCredential,
    ) -> Result<InferenceCredential, RepositoryError> {
        create(&self.executor, credential).await
    }

    async fn update_inference_credential(
        &self,
        credential: InferenceCredential,
        expected_aggregate_version: u64,
    ) -> Result<InferenceCredential, RepositoryError> {
        update(&self.executor, credential, expected_aggregate_version).await
    }

    async fn find_inference_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: InferenceCredentialId,
    ) -> Result<Option<InferenceCredential>, RepositoryError> {
        find(&self.executor, organization_id, credential_id).await
    }

    async fn list_inference_credentials_by_environment(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<InferenceCredential>, RepositoryError> {
        list(&self.executor, organization_id, project_id, environment_id).await
    }
}

async fn create(
    executor: &PostgresExecutor,
    credential: InferenceCredential,
) -> Result<InferenceCredential, RepositoryError> {
    if credential.generation() != 1
        || credential.aggregate_version() != 1
        || credential.created_at() != credential.updated_at()
        || credential.revoked_at().is_some()
    {
        return Err(RepositoryError::Conflict(
            "new inference credential is not at its initial generation".into(),
        ));
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                insert_credential(transaction, &credential).await?;
                Ok(credential)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn update(
    executor: &PostgresExecutor,
    credential: InferenceCredential,
    expected_aggregate_version: u64,
) -> Result<InferenceCredential, RepositoryError> {
    if expected_aggregate_version == 0
        || expected_aggregate_version.checked_add(1) != Some(credential.aggregate_version())
    {
        return Err(RepositoryError::Conflict(
            "inference credential aggregate transition is invalid".into(),
        ));
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let existing = fetch_optional::<InferenceCredentialRow, _>(
                    transaction,
                    credential_query(credential.organization_id, credential.id).for_update(),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?
                .credential()?;
                credential
                    .validate_transition_from(&existing, expected_aggregate_version)
                    .map_err(RepositoryError::Conflict)?;
                update_credential_row(transaction, &credential, expected_aggregate_version).await?;
                Ok(credential)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    credential_id: InferenceCredentialId,
) -> Result<Option<InferenceCredential>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(credential_query(organization_id, credential_id))
        .await
        .map_err(storage)?
        .map(InferenceCredentialRow::credential)
        .transpose()
}

async fn list(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> Result<Vec<InferenceCredential>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<InferenceCredentials>()
                .select(InferenceCredentialSelection)
                .filter(InferenceCredentials::organization_id().eq(organization_id.as_uuid()))
                .filter(InferenceCredentials::project_id().eq(project_id.as_uuid()))
                .filter(InferenceCredentials::environment_id().eq(environment_id.as_uuid()))
                .order_by(InferenceCredentials::created_at(), OrderDirection::Asc)
                .order_by(InferenceCredentials::id(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(InferenceCredentialRow::credential)
        .collect()
}

fn credential_query(
    organization_id: OrganizationId,
    credential_id: InferenceCredentialId,
) -> a3s_orm::query::SelectQuery<InferenceCredentials, InferenceCredentialRow> {
    select_from::<InferenceCredentials>()
        .select(InferenceCredentialSelection)
        .filter(InferenceCredentials::organization_id().eq(organization_id.as_uuid()))
        .filter(InferenceCredentials::id().eq(credential_id.as_uuid()))
}

struct InferenceCredentialRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    prefix: String,
    verifier_hash: String,
    generation: u64,
    aggregate_version: u64,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

struct InferenceCredentialSelection;

impl Selection for InferenceCredentialSelection {
    type Output = InferenceCredentialRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            InferenceCredentials::id().expression(),
            InferenceCredentials::organization_id().expression(),
            InferenceCredentials::project_id().expression(),
            InferenceCredentials::environment_id().expression(),
            InferenceCredentials::prefix().expression(),
            InferenceCredentials::verifier_hash().expression(),
            InferenceCredentials::generation().expression(),
            InferenceCredentials::aggregate_version().expression(),
            InferenceCredentials::expires_at().expression(),
            InferenceCredentials::created_at().expression(),
            InferenceCredentials::updated_at().expression(),
            InferenceCredentials::revoked_at().expression(),
        ]
    }
}

impl FromRow for InferenceCredentialRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            prefix: decode(row, 4)?,
            verifier_hash: decode(row, 5)?,
            generation: decode(row, 6)?,
            aggregate_version: decode(row, 7)?,
            expires_at: decode(row, 8)?,
            created_at: decode(row, 9)?,
            updated_at: decode(row, 10)?,
            revoked_at: decode(row, 11)?,
        })
    }
}

impl InferenceCredentialRow {
    fn credential(self) -> Result<InferenceCredential, RepositoryError> {
        InferenceCredential::restore(
            InferenceCredentialId::from_uuid(self.id),
            OrganizationId::from_uuid(self.organization_id),
            ProjectId::from_uuid(self.project_id),
            EnvironmentId::from_uuid(self.environment_id),
            self.prefix,
            self.verifier_hash,
            self.generation,
            self.aggregate_version,
            self.expires_at,
            self.created_at,
            self.updated_at,
            self.revoked_at,
        )
        .map_err(stored)
    }
}

async fn insert_credential(
    transaction: &PostgresTransaction,
    credential: &InferenceCredential,
) -> Result<(), PostgresPersistenceError> {
    let projection = credential
        .gateway_projection()
        .map_err(|error| RepositoryError::Conflict(error))?;
    let result = execute(
        transaction,
        insert_into::<InferenceCredentials>()
            .value(InferenceCredentials::id(), credential.id.as_uuid())
            .value(
                InferenceCredentials::organization_id(),
                credential.organization_id.as_uuid(),
            )
            .value(
                InferenceCredentials::project_id(),
                credential.project_id.as_uuid(),
            )
            .value(
                InferenceCredentials::environment_id(),
                credential.environment_id.as_uuid(),
            )
            .value(InferenceCredentials::prefix(), credential.prefix())
            .value(
                InferenceCredentials::verifier_hash(),
                projection.verifier_hash(),
            )
            .value(InferenceCredentials::generation(), credential.generation())
            .value(
                InferenceCredentials::aggregate_version(),
                credential.aggregate_version(),
            )
            .value(InferenceCredentials::expires_at(), credential.expires_at())
            .value(InferenceCredentials::created_at(), credential.created_at())
            .value(InferenceCredentials::updated_at(), credential.updated_at())
            .value(InferenceCredentials::revoked_at(), credential.revoked_at()),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("inference credential", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "inference credential identity or lookup prefix is already in use".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn update_credential_row(
    transaction: &PostgresTransaction,
    credential: &InferenceCredential,
    expected_aggregate_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let projection = credential
        .gateway_projection()
        .map_err(|error| RepositoryError::Conflict(error))?;
    let result = execute(
        transaction,
        update_table::<InferenceCredentials>()
            .set(InferenceCredentials::prefix(), credential.prefix())
            .set(
                InferenceCredentials::verifier_hash(),
                projection.verifier_hash(),
            )
            .set(InferenceCredentials::generation(), credential.generation())
            .set(
                InferenceCredentials::aggregate_version(),
                credential.aggregate_version(),
            )
            .set(InferenceCredentials::expires_at(), credential.expires_at())
            .set(InferenceCredentials::updated_at(), credential.updated_at())
            .set(InferenceCredentials::revoked_at(), credential.revoked_at())
            .filter(
                InferenceCredentials::organization_id().eq(credential.organization_id.as_uuid()),
            )
            .filter(InferenceCredentials::id().eq(credential.id.as_uuid()))
            .filter(InferenceCredentials::aggregate_version().eq(expected_aggregate_version)),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("inference credential update", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "inference credential lookup prefix is already in use".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn stored(error: String) -> RepositoryError {
    RepositoryError::Storage(format!("stored inference credential is invalid: {error}"))
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_orm::Query;

    #[test]
    fn environment_list_compiles_as_one_typed_scoped_query() {
        let compiled = select_from::<InferenceCredentials>()
            .select(InferenceCredentialSelection)
            .filter(InferenceCredentials::organization_id().eq(OrganizationId::new().as_uuid()))
            .filter(InferenceCredentials::project_id().eq(ProjectId::new().as_uuid()))
            .filter(InferenceCredentials::environment_id().eq(EnvironmentId::new().as_uuid()))
            .order_by(InferenceCredentials::created_at(), OrderDirection::Asc)
            .order_by(InferenceCredentials::id(), OrderDirection::Asc)
            .compile(&PostgresDialect)
            .expect("compile");

        assert_eq!(compiled.parameters.len(), 3);
        assert!(compiled.sql.contains("\"organization_id\" = $1"));
        assert!(compiled.sql.contains("\"project_id\" = $2"));
        assert!(compiled.sql.contains("\"environment_id\" = $3"));
        assert!(compiled.sql.contains("\"inference_credentials\""));
        assert!(compiled.sql.ends_with("\"id\" asc"));
    }
}
