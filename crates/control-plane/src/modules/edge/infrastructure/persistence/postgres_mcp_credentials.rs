use super::postgres::PostgresEdgeRepository;
use super::postgres_schema::McpCredentials;
use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, require_one_row,
    transaction_error,
};
use crate::modules::edge::domain::repositories::IMcpCredentialRepository;
use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, update_table, Database, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[async_trait]
impl IMcpCredentialRepository for PostgresEdgeRepository {
    async fn create_mcp_credential(
        &self,
        credential: McpCredential,
    ) -> Result<McpCredential, RepositoryError> {
        create(&self.executor, credential).await
    }

    async fn update_mcp_credential(
        &self,
        credential: McpCredential,
        expected_aggregate_version: u64,
    ) -> Result<McpCredential, RepositoryError> {
        update(&self.executor, credential, expected_aggregate_version).await
    }

    async fn find_mcp_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: McpCredentialId,
    ) -> Result<Option<McpCredential>, RepositoryError> {
        find(&self.executor, organization_id, credential_id).await
    }

    async fn list_mcp_credentials(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<McpCredential>, RepositoryError> {
        list(&self.executor, organization_id, project_id, environment_id).await
    }
}

async fn create(
    executor: &PostgresExecutor,
    credential: McpCredential,
) -> Result<McpCredential, RepositoryError> {
    if credential.generation() != 1
        || credential.aggregate_version() != 1
        || credential.created_at() != credential.updated_at()
        || credential.revoked_at().is_some()
    {
        return Err(RepositoryError::Conflict(
            "new MCP credential is not at its initial generation".into(),
        ));
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let projection = credential.gateway_projection();
                let result = execute(
                    transaction,
                    insert_into::<McpCredentials>()
                        .value(McpCredentials::id(), credential.id.as_uuid())
                        .value(
                            McpCredentials::organization_id(),
                            credential.organization_id.as_uuid(),
                        )
                        .value(
                            McpCredentials::project_id(),
                            credential.project_id.as_uuid(),
                        )
                        .value(
                            McpCredentials::environment_id(),
                            credential.environment_id.as_uuid(),
                        )
                        .value(McpCredentials::prefix(), credential.prefix())
                        .value(McpCredentials::verifier_hash(), projection.verifier_hash())
                        .value(McpCredentials::generation(), credential.generation())
                        .value(
                            McpCredentials::aggregate_version(),
                            credential.aggregate_version(),
                        )
                        .value(McpCredentials::expires_at(), credential.expires_at())
                        .value(McpCredentials::created_at(), credential.created_at())
                        .value(McpCredentials::updated_at(), credential.updated_at())
                        .value(McpCredentials::revoked_at(), credential.revoked_at()),
                )
                .await;
                match result {
                    Ok(rows) => require_one_row("MCP credential", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "MCP credential identity or lookup prefix is already in use".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                Ok(credential)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn update(
    executor: &PostgresExecutor,
    credential: McpCredential,
    expected_aggregate_version: u64,
) -> Result<McpCredential, RepositoryError> {
    if expected_aggregate_version == 0
        || expected_aggregate_version.checked_add(1) != Some(credential.aggregate_version())
    {
        return Err(RepositoryError::Conflict(
            "MCP credential aggregate transition is invalid".into(),
        ));
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let existing = fetch_optional::<McpCredentialRow, _>(
                    transaction,
                    credential_query(credential.organization_id, credential.id).for_update(),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?
                .credential()?;
                credential
                    .validate_transition_from(&existing, expected_aggregate_version)
                    .map_err(RepositoryError::Conflict)?;
                let projection = credential.gateway_projection();
                let result = execute(
                    transaction,
                    update_table::<McpCredentials>()
                        .set(McpCredentials::prefix(), credential.prefix())
                        .set(McpCredentials::verifier_hash(), projection.verifier_hash())
                        .set(McpCredentials::generation(), credential.generation())
                        .set(
                            McpCredentials::aggregate_version(),
                            credential.aggregate_version(),
                        )
                        .set(McpCredentials::expires_at(), credential.expires_at())
                        .set(McpCredentials::updated_at(), credential.updated_at())
                        .set(McpCredentials::revoked_at(), credential.revoked_at())
                        .filter(
                            McpCredentials::organization_id()
                                .eq(credential.organization_id.as_uuid()),
                        )
                        .filter(McpCredentials::id().eq(credential.id.as_uuid()))
                        .filter(McpCredentials::aggregate_version().eq(expected_aggregate_version)),
                )
                .await;
                match result {
                    Ok(rows) => require_one_row("MCP credential update", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "MCP credential lookup prefix is already in use".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                Ok(credential)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    credential_id: McpCredentialId,
) -> Result<Option<McpCredential>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(credential_query(organization_id, credential_id))
        .await
        .map_err(storage)?
        .map(McpCredentialRow::credential)
        .transpose()
}

async fn list(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> Result<Vec<McpCredential>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<McpCredentials>()
                .select(McpCredentialSelection)
                .filter(McpCredentials::organization_id().eq(organization_id.as_uuid()))
                .filter(McpCredentials::project_id().eq(project_id.as_uuid()))
                .filter(McpCredentials::environment_id().eq(environment_id.as_uuid()))
                .order_by(McpCredentials::created_at(), OrderDirection::Asc)
                .order_by(McpCredentials::id(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(McpCredentialRow::credential)
        .collect()
}

fn credential_query(
    organization_id: OrganizationId,
    credential_id: McpCredentialId,
) -> a3s_orm::query::SelectQuery<McpCredentials, McpCredentialRow> {
    select_from::<McpCredentials>()
        .select(McpCredentialSelection)
        .filter(McpCredentials::organization_id().eq(organization_id.as_uuid()))
        .filter(McpCredentials::id().eq(credential_id.as_uuid()))
}

struct McpCredentialRow {
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

struct McpCredentialSelection;

impl Selection for McpCredentialSelection {
    type Output = McpCredentialRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            McpCredentials::id().expression(),
            McpCredentials::organization_id().expression(),
            McpCredentials::project_id().expression(),
            McpCredentials::environment_id().expression(),
            McpCredentials::prefix().expression(),
            McpCredentials::verifier_hash().expression(),
            McpCredentials::generation().expression(),
            McpCredentials::aggregate_version().expression(),
            McpCredentials::expires_at().expression(),
            McpCredentials::created_at().expression(),
            McpCredentials::updated_at().expression(),
            McpCredentials::revoked_at().expression(),
        ]
    }
}

impl FromRow for McpCredentialRow {
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

impl McpCredentialRow {
    fn credential(self) -> Result<McpCredential, RepositoryError> {
        McpCredential::restore(
            McpCredentialId::from_uuid(self.id),
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

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn stored(error: String) -> RepositoryError {
    RepositoryError::Storage(format!("stored MCP credential is invalid: {error}"))
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
