use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, require_one_row,
    store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError, SourceConnectionId};
use crate::modules::sources::domain::{
    CompleteGithubConnection, GithubAccountId, GithubAccountKind, GithubConnection,
    GithubConnectionFlow, GithubConnectionFlowError, GithubConnectionFlowStage,
    GithubInstallationId, GithubLogin, IGithubConnectionRepository,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct GithubConnectionFlowRow {
    id: Uuid,
    organization_id: Uuid,
    stage: String,
    state_digest: String,
    installation_id: Option<i64>,
    pkce_verifier_digest: Option<String>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
}

struct GithubConnectionRow {
    organization_id: Uuid,
    id: Uuid,
    installation_id: i64,
    account_id: i64,
    account_login: String,
    account_kind: String,
    verified_by_user_id: i64,
    verified_by_user_login: String,
    aggregate_version: u64,
    connected_at: DateTime<Utc>,
}

impl FromRow for GithubConnectionFlowRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            stage: decode(row, 2)?,
            state_digest: decode(row, 3)?,
            installation_id: decode(row, 4)?,
            pkce_verifier_digest: decode(row, 5)?,
            created_at: decode(row, 6)?,
            expires_at: decode(row, 7)?,
            consumed_at: decode(row, 8)?,
        })
    }
}

impl FromRow for GithubConnectionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            id: decode(row, 1)?,
            installation_id: decode(row, 2)?,
            account_id: decode(row, 3)?,
            account_login: decode(row, 4)?,
            account_kind: decode(row, 5)?,
            verified_by_user_id: decode(row, 6)?,
            verified_by_user_login: decode(row, 7)?,
            aggregate_version: decode(row, 8)?,
            connected_at: decode(row, 9)?,
        })
    }
}

#[derive(Clone)]
pub struct PostgresGithubConnectionRepository {
    executor: PostgresExecutor,
}

impl PostgresGithubConnectionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IGithubConnectionRepository for PostgresGithubConnectionRepository {
    async fn begin_flow(
        &self,
        flow: GithubConnectionFlow,
    ) -> Result<GithubConnectionFlow, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>("select 1 from organizations where id = ")
                            .bind(flow.organization_id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .is_none()
                    {
                        return Err(RepositoryError::NotFound.into());
                    }
                    if fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from github_source_connections where organization_id = ",
                        )
                        .bind(flow.organization_id.as_uuid())
                        .append(" for update"),
                    )
                    .await?
                    .is_some()
                    {
                        return Err(RepositoryError::Conflict(
                            "organization already has a GitHub source connection".into(),
                        )
                        .into());
                    }
                    let result = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into github_connection_flows (id, organization_id, stage, state_digest, installation_id, pkce_verifier_digest, created_at, expires_at, consumed_at) values (",
                        )
                        .bind(flow.id)
                        .append(", ")
                        .bind(flow.organization_id.as_uuid())
                        .append(", ")
                        .bind(flow.stage.as_str())
                        .append(", ")
                        .bind(flow.state_digest.as_str())
                        .append(", null, null, ")
                        .bind(flow.created_at)
                        .append(", ")
                        .bind(flow.expires_at)
                        .append(", null) on conflict (organization_id) do update set id = excluded.id, stage = excluded.stage, state_digest = excluded.state_digest, installation_id = null, pkce_verifier_digest = null, created_at = excluded.created_at, expires_at = excluded.expires_at, consumed_at = null"),
                    )
                    .await;
                    match result {
                        Ok(rows) => require_one_row("GitHub connection flow", rows)?,
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "GitHub connection state collision".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    Ok(flow)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn prepare_oauth(
        &self,
        installation_state_digest: &str,
        installation_id: GithubInstallationId,
        oauth_state_digest: String,
        pkce_verifier_digest: String,
        now: DateTime<Utc>,
    ) -> Result<GithubConnectionFlow, RepositoryError> {
        let installation_state_digest = installation_state_digest.to_owned();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let row = fetch_optional::<GithubConnectionFlowRow, _>(
                        transaction,
                        flow_by_state_query(&installation_state_digest).append(" for update"),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    let mut flow = map_flow(row)?;
                    flow.prepare_oauth(
                        installation_id,
                        oauth_state_digest,
                        pkce_verifier_digest,
                        now,
                    )
                    .map_err(flow_error)?;
                    let prepared_installation_id = flow.installation_id.ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "prepared GitHub flow has no installation ID".into(),
                        )
                    })?;
                    let prepared_pkce_digest =
                        flow.pkce_verifier_digest.as_deref().ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "prepared GitHub flow has no PKCE digest".into(),
                            )
                        })?;
                    let update = execute(
                        transaction,
                        sql_query::<()>("update github_connection_flows set stage = ")
                            .bind(flow.stage.as_str())
                            .append(", state_digest = ")
                            .bind(flow.state_digest.as_str())
                            .append(", installation_id = ")
                            .bind(as_i64(prepared_installation_id)?)
                            .append(", pkce_verifier_digest = ")
                            .bind(prepared_pkce_digest)
                            .append(" where id = ")
                            .bind(flow.id),
                    )
                    .await;
                    match update {
                        Ok(rows) => {
                            require_one_row("GitHub OAuth connection flow", rows)?;
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "GitHub connection state collision".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    Ok(flow)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_oauth_flow(
        &self,
        oauth_state_digest: &str,
        pkce_verifier_digest: &str,
        now: DateTime<Utc>,
    ) -> Result<GithubConnectionFlow, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(flow_by_state_query(oauth_state_digest))
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .ok_or(RepositoryError::NotFound)?;
        let flow = map_flow(row).map_err(persistence_error)?;
        flow.require_oauth(oauth_state_digest, pkce_verifier_digest, now)
            .map_err(|error| RepositoryError::Conflict(error.to_string()))?;
        Ok(flow)
    }

    async fn complete(
        &self,
        request: CompleteGithubConnection,
    ) -> Result<GithubConnection, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>("select 1 from organizations where id = ")
                            .bind(request.connection.organization_id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .is_none()
                    {
                        return Err(RepositoryError::NotFound.into());
                    }
                    let row = fetch_optional::<GithubConnectionFlowRow, _>(
                        transaction,
                        flow_by_id_query(request.flow_id).append(" for update"),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    let mut flow = map_flow(row)?;
                    if flow.organization_id != request.connection.organization_id
                        || flow.installation_id != Some(request.connection.installation_id)
                    {
                        return Err(RepositoryError::Conflict(
                            "GitHub connection flow identity changed".into(),
                        )
                        .into());
                    }
                    flow.complete(request.completed_at).map_err(flow_error)?;
                    let insert = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into github_source_connections (organization_id, id, installation_id, account_id, account_login, account_kind, verified_by_user_id, verified_by_user_login, aggregate_version, connected_at) values (",
                        )
                        .bind(request.connection.organization_id.as_uuid())
                        .append(", ")
                        .bind(request.connection.id.as_uuid())
                        .append(", ")
                        .bind(as_i64(request.connection.installation_id)?)
                        .append(", ")
                        .bind(as_i64(request.connection.account_id)?)
                        .append(", ")
                        .bind(request.connection.account_login.as_str())
                        .append(", ")
                        .bind(request.connection.account_kind.as_str())
                        .append(", ")
                        .bind(as_i64(request.connection.verified_by_user_id)?)
                        .append(", ")
                        .bind(request.connection.verified_by_user_login.as_str())
                        .append(", ")
                        .bind(request.connection.aggregate_version)
                        .append(", ")
                        .bind(request.connection.connected_at)
                        .append(")"),
                    )
                    .await;
                    match insert {
                        Ok(rows) => require_one_row("GitHub source connection", rows)?,
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "GitHub installation or account is already connected".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    let rows = execute(
                        transaction,
                        sql_query::<()>(
                            "update github_connection_flows set stage = ",
                        )
                        .bind(GithubConnectionFlowStage::Completed.as_str())
                        .append(", consumed_at = ")
                        .bind(flow.consumed_at.ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "completed GitHub flow has no consumption timestamp".into(),
                            )
                        })?)
                        .append(" where id = ")
                        .bind(flow.id),
                    )
                    .await?;
                    require_one_row("completed GitHub connection flow", rows)?;
                    store_outbox(transaction, &request.event).await?;
                    Ok(request.connection)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<GithubConnection>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(connection_by_organization_query(organization_id))
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(map_connection)
            .transpose()
            .map_err(persistence_error)
    }
}

fn flow_by_state_query(state_digest: &str) -> a3s_orm::SqlQuery<GithubConnectionFlowRow> {
    sql_query::<GithubConnectionFlowRow>(
        "select id, organization_id, stage, state_digest, installation_id, pkce_verifier_digest, created_at, expires_at, consumed_at from github_connection_flows where state_digest = ",
    )
    .bind(state_digest)
}

fn flow_by_id_query(flow_id: Uuid) -> a3s_orm::SqlQuery<GithubConnectionFlowRow> {
    sql_query::<GithubConnectionFlowRow>(
        "select id, organization_id, stage, state_digest, installation_id, pkce_verifier_digest, created_at, expires_at, consumed_at from github_connection_flows where id = ",
    )
    .bind(flow_id)
}

fn connection_by_organization_query(
    organization_id: OrganizationId,
) -> a3s_orm::SqlQuery<GithubConnectionRow> {
    sql_query::<GithubConnectionRow>(
        "select organization_id, id, installation_id, account_id, account_login, account_kind, verified_by_user_id, verified_by_user_login, aggregate_version, connected_at from github_source_connections where organization_id = ",
    )
    .bind(organization_id.as_uuid())
}

fn map_flow(
    row: GithubConnectionFlowRow,
) -> Result<GithubConnectionFlow, PostgresPersistenceError> {
    GithubConnectionFlow::restore(GithubConnectionFlow {
        id: row.id,
        organization_id: OrganizationId::from_uuid(row.organization_id),
        stage: GithubConnectionFlowStage::parse(&row.stage)
            .map_err(PostgresPersistenceError::Invariant)?,
        state_digest: row.state_digest,
        installation_id: row
            .installation_id
            .map(|value| {
                u64::try_from(value)
                    .map_err(|_| "stored GitHub installation ID is negative".to_owned())
                    .and_then(GithubInstallationId::parse)
            })
            .transpose()
            .map_err(PostgresPersistenceError::Invariant)?,
        pkce_verifier_digest: row.pkce_verifier_digest,
        created_at: row.created_at,
        expires_at: row.expires_at,
        consumed_at: row.consumed_at,
    })
    .map_err(PostgresPersistenceError::Invariant)
}

fn map_connection(row: GithubConnectionRow) -> Result<GithubConnection, PostgresPersistenceError> {
    GithubConnection::restore(GithubConnection {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        id: SourceConnectionId::from_uuid(row.id),
        installation_id: GithubInstallationId::parse(to_u64(row.installation_id, "installation")?)
            .map_err(PostgresPersistenceError::Invariant)?,
        account_id: GithubAccountId::parse(to_u64(row.account_id, "account")?)
            .map_err(PostgresPersistenceError::Invariant)?,
        account_login: GithubLogin::parse(row.account_login)
            .map_err(PostgresPersistenceError::Invariant)?,
        account_kind: GithubAccountKind::parse(&row.account_kind)
            .map_err(PostgresPersistenceError::Invariant)?,
        verified_by_user_id: GithubAccountId::parse(to_u64(
            row.verified_by_user_id,
            "verifying user",
        )?)
        .map_err(PostgresPersistenceError::Invariant)?,
        verified_by_user_login: GithubLogin::parse(row.verified_by_user_login)
            .map_err(PostgresPersistenceError::Invariant)?,
        aggregate_version: row.aggregate_version,
        connected_at: row.connected_at,
    })
    .map_err(PostgresPersistenceError::Invariant)
}

fn as_i64(value: impl IntoGithubNumericId) -> Result<i64, PostgresPersistenceError> {
    i64::try_from(value.into_u64()).map_err(|_| {
        PostgresPersistenceError::Invariant("GitHub numeric ID exceeds PostgreSQL bigint".into())
    })
}

trait IntoGithubNumericId {
    fn into_u64(self) -> u64;
}

impl IntoGithubNumericId for GithubInstallationId {
    fn into_u64(self) -> u64 {
        GithubInstallationId::as_u64(self)
    }
}

impl IntoGithubNumericId for GithubAccountId {
    fn into_u64(self) -> u64 {
        GithubAccountId::as_u64(self)
    }
}

fn to_u64(value: i64, label: &str) -> Result<u64, PostgresPersistenceError> {
    u64::try_from(value).map_err(|_| {
        PostgresPersistenceError::Invariant(format!("stored GitHub {label} ID is negative"))
    })
}

fn flow_error(error: GithubConnectionFlowError) -> PostgresPersistenceError {
    RepositoryError::Conflict(error.to_string()).into()
}

fn persistence_error(error: PostgresPersistenceError) -> RepositoryError {
    match error {
        PostgresPersistenceError::Repository(error) => error,
        error => RepositoryError::Storage(error.to_string()),
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
