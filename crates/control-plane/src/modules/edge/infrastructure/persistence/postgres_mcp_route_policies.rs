use super::postgres::PostgresEdgeRepository;
use super::postgres_schema::{McpRoutePolicies, McpServiceProfiles};
use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, require_one_row,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::assets::domain::McpServiceProfile;
use crate::modules::edge::domain::repositories::IMcpRoutePolicyRepository;
use crate::modules::edge::domain::McpRoutePolicy;
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, EnvironmentId, OrganizationId, ProjectId, RepositoryError, RouteId,
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
impl IMcpRoutePolicyRepository for PostgresEdgeRepository {
    async fn create_mcp_route_policy(
        &self,
        policy: McpRoutePolicy,
    ) -> Result<McpRoutePolicy, RepositoryError> {
        create(&self.executor, policy).await
    }

    async fn update_mcp_route_policy(
        &self,
        policy: McpRoutePolicy,
        expected_policy_revision: u64,
    ) -> Result<McpRoutePolicy, RepositoryError> {
        update(&self.executor, policy, expected_policy_revision).await
    }

    async fn find_mcp_route_policy(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
    ) -> Result<Option<McpRoutePolicy>, RepositoryError> {
        find(&self.executor, organization_id, route_id).await
    }

    async fn list_mcp_route_policies(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<McpRoutePolicy>, RepositoryError> {
        list(&self.executor, organization_id, project_id, environment_id).await
    }
}

async fn create(
    executor: &PostgresExecutor,
    policy: McpRoutePolicy,
) -> Result<McpRoutePolicy, RepositoryError> {
    if policy.policy_revision() != 1 || policy.created_at() != policy.updated_at() {
        return Err(RepositoryError::Conflict(
            "new MCP route policy is not at its initial revision".into(),
        ));
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let profile = load_profile(
                    transaction,
                    policy.spec().organization_id,
                    policy.spec().asset_id,
                    policy.spec().asset_release_id,
                    policy.spec().profile_digest.as_str(),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                validate_supplied(&policy, &profile)?;
                let result = execute(
                    transaction,
                    insert_into::<McpRoutePolicies>()
                        .value(McpRoutePolicies::id(), policy.spec().route_id.as_uuid())
                        .value(
                            McpRoutePolicies::organization_id(),
                            policy.spec().organization_id.as_uuid(),
                        )
                        .value(
                            McpRoutePolicies::project_id(),
                            policy.spec().project_id.as_uuid(),
                        )
                        .value(
                            McpRoutePolicies::environment_id(),
                            policy.spec().environment_id.as_uuid(),
                        )
                        .value(
                            McpRoutePolicies::gateway_scope_id(),
                            policy.spec().gateway_scope_id.as_uuid(),
                        )
                        .value(
                            McpRoutePolicies::workload_id(),
                            policy.spec().workload_id.as_uuid(),
                        )
                        .value(
                            McpRoutePolicies::asset_id(),
                            policy.spec().asset_id.as_uuid(),
                        )
                        .value(
                            McpRoutePolicies::asset_release_id(),
                            policy.spec().asset_release_id.as_uuid(),
                        )
                        .value(
                            McpRoutePolicies::profile_digest(),
                            policy.spec().profile_digest.as_str(),
                        )
                        .value(
                            McpRoutePolicies::hostname(),
                            policy.spec().hostname.as_str(),
                        )
                        .value(McpRoutePolicies::path(), policy.spec().path.as_str())
                        .value(
                            McpRoutePolicies::policy_revision(),
                            policy.policy_revision(),
                        )
                        .value(
                            McpRoutePolicies::policy_digest(),
                            policy.policy_digest().as_str(),
                        )
                        .value(McpRoutePolicies::acl(), policy.canonical_acl())
                        .value(McpRoutePolicies::expires_at(), policy.spec().expires_at)
                        .value(McpRoutePolicies::created_at(), policy.created_at())
                        .value(McpRoutePolicies::updated_at(), policy.updated_at()),
                )
                .await;
                match result {
                    Ok(rows) => require_one_row("MCP route policy", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "MCP route identity or exact Gateway route is already in use".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                Ok(policy)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn update(
    executor: &PostgresExecutor,
    policy: McpRoutePolicy,
    expected_policy_revision: u64,
) -> Result<McpRoutePolicy, RepositoryError> {
    if expected_policy_revision == 0
        || expected_policy_revision.checked_add(1) != Some(policy.policy_revision())
    {
        return Err(RepositoryError::Conflict(
            "MCP route policy revision transition is invalid".into(),
        ));
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let row = fetch_optional::<McpRoutePolicyRow, _>(
                    transaction,
                    policy_query(policy.spec().organization_id, policy.spec().route_id)
                        .for_update(),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                let existing_profile = load_profile(
                    transaction,
                    OrganizationId::from_uuid(row.organization_id),
                    AssetId::from_uuid(row.asset_id),
                    AssetReleaseId::from_uuid(row.asset_release_id),
                    &row.profile_digest,
                )
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "stored MCP route policy lost its Service profile".into(),
                    )
                })?;
                let existing = row.policy(&existing_profile)?;
                validate_transition(&existing, &policy, expected_policy_revision)?;
                let profile = load_profile(
                    transaction,
                    policy.spec().organization_id,
                    policy.spec().asset_id,
                    policy.spec().asset_release_id,
                    policy.spec().profile_digest.as_str(),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                validate_supplied(&policy, &profile)?;

                let result = execute(
                    transaction,
                    update_table::<McpRoutePolicies>()
                        .set(
                            McpRoutePolicies::asset_release_id(),
                            policy.spec().asset_release_id.as_uuid(),
                        )
                        .set(
                            McpRoutePolicies::profile_digest(),
                            policy.spec().profile_digest.as_str(),
                        )
                        .set(
                            McpRoutePolicies::hostname(),
                            policy.spec().hostname.as_str(),
                        )
                        .set(McpRoutePolicies::path(), policy.spec().path.as_str())
                        .set(
                            McpRoutePolicies::policy_revision(),
                            policy.policy_revision(),
                        )
                        .set(
                            McpRoutePolicies::policy_digest(),
                            policy.policy_digest().as_str(),
                        )
                        .set(McpRoutePolicies::acl(), policy.canonical_acl())
                        .set(McpRoutePolicies::expires_at(), policy.spec().expires_at)
                        .set(McpRoutePolicies::updated_at(), policy.updated_at())
                        .filter(
                            McpRoutePolicies::organization_id()
                                .eq(policy.spec().organization_id.as_uuid()),
                        )
                        .filter(McpRoutePolicies::id().eq(policy.spec().route_id.as_uuid()))
                        .filter(McpRoutePolicies::policy_revision().eq(expected_policy_revision)),
                )
                .await;
                match result {
                    Ok(rows) => require_one_row("MCP route policy update", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "MCP exact Gateway route is already in use".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                Ok(policy)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    route_id: RouteId,
) -> Result<Option<McpRoutePolicy>, RepositoryError> {
    let Some(row) = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(policy_query(organization_id, route_id))
        .await
        .map_err(storage)?
    else {
        return Ok(None);
    };
    restore_row(executor, row).await.map(Some)
}

async fn list(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> Result<Vec<McpRoutePolicy>, RepositoryError> {
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<McpRoutePolicies>()
                .select(McpRoutePolicySelection)
                .filter(McpRoutePolicies::organization_id().eq(organization_id.as_uuid()))
                .filter(McpRoutePolicies::project_id().eq(project_id.as_uuid()))
                .filter(McpRoutePolicies::environment_id().eq(environment_id.as_uuid()))
                .order_by(McpRoutePolicies::created_at(), OrderDirection::Asc)
                .order_by(McpRoutePolicies::id(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows;
    let mut policies = Vec::with_capacity(rows.len());
    for row in rows {
        policies.push(restore_row(executor, row).await?);
    }
    Ok(policies)
}

async fn restore_row(
    executor: &PostgresExecutor,
    row: McpRoutePolicyRow,
) -> Result<McpRoutePolicy, RepositoryError> {
    let profile = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(profile_query(
            OrganizationId::from_uuid(row.organization_id),
            AssetId::from_uuid(row.asset_id),
            AssetReleaseId::from_uuid(row.asset_release_id),
            &row.profile_digest,
        ))
        .await
        .map_err(storage)?
        .map(|(digest, acl)| McpServiceProfile::restore(&acl, &digest))
        .transpose()
        .map_err(stored)?
        .ok_or_else(|| {
            RepositoryError::Storage("stored MCP route policy lost its Service profile".into())
        })?;
    row.policy(&profile)
}

async fn load_profile(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    profile_digest: &str,
) -> Result<Option<McpServiceProfile>, PostgresPersistenceError> {
    fetch_optional::<(String, String), _>(
        transaction,
        profile_query(organization_id, asset_id, asset_release_id, profile_digest),
    )
    .await?
    .map(|(digest, acl)| McpServiceProfile::restore(&acl, &digest))
    .transpose()
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored MCP Service profile is invalid: {error}"
        ))
    })
}

fn policy_query(
    organization_id: OrganizationId,
    route_id: RouteId,
) -> a3s_orm::query::SelectQuery<McpRoutePolicies, McpRoutePolicyRow> {
    select_from::<McpRoutePolicies>()
        .select(McpRoutePolicySelection)
        .filter(McpRoutePolicies::organization_id().eq(organization_id.as_uuid()))
        .filter(McpRoutePolicies::id().eq(route_id.as_uuid()))
}

fn profile_query(
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    profile_digest: &str,
) -> a3s_orm::query::SelectQuery<McpServiceProfiles, (String, String)> {
    select_from::<McpServiceProfiles>()
        .select((
            McpServiceProfiles::profile_digest(),
            McpServiceProfiles::acl(),
        ))
        .filter(McpServiceProfiles::organization_id().eq(organization_id.as_uuid()))
        .filter(McpServiceProfiles::asset_id().eq(asset_id.as_uuid()))
        .filter(McpServiceProfiles::asset_release_id().eq(asset_release_id.as_uuid()))
        .filter(McpServiceProfiles::profile_digest().eq(profile_digest))
}

fn validate_supplied(
    policy: &McpRoutePolicy,
    profile: &McpServiceProfile,
) -> Result<(), PostgresPersistenceError> {
    let restored = McpRoutePolicy::restore(
        policy.canonical_acl(),
        policy.policy_digest().as_str(),
        policy.created_at(),
        policy.updated_at(),
        profile,
    )
    .map_err(|error| {
        RepositoryError::Conflict(format!("invalid MCP route policy write: {error}"))
    })?;
    if restored != *policy {
        return Err(RepositoryError::Conflict(
            "MCP route policy fields do not match its canonical ACL".into(),
        )
        .into());
    }
    Ok(())
}

fn validate_transition(
    existing: &McpRoutePolicy,
    candidate: &McpRoutePolicy,
    expected_policy_revision: u64,
) -> Result<(), PostgresPersistenceError> {
    if existing.policy_revision() != expected_policy_revision
        || candidate.policy_revision() != expected_policy_revision + 1
        || candidate.spec().route_id != existing.spec().route_id
        || candidate.spec().organization_id != existing.spec().organization_id
        || candidate.spec().project_id != existing.spec().project_id
        || candidate.spec().environment_id != existing.spec().environment_id
        || candidate.spec().gateway_scope_id != existing.spec().gateway_scope_id
        || candidate.spec().workload_id != existing.spec().workload_id
        || candidate.spec().asset_id != existing.spec().asset_id
        || candidate.created_at() != existing.created_at()
        || candidate.updated_at() < existing.updated_at()
    {
        return Err(RepositoryError::Conflict(
            "MCP route policy changed during its revision transition".into(),
        )
        .into());
    }
    Ok(())
}

struct McpRoutePolicyRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    gateway_scope_id: Uuid,
    workload_id: Uuid,
    asset_id: Uuid,
    asset_release_id: Uuid,
    profile_digest: String,
    hostname: String,
    path: String,
    policy_revision: u64,
    policy_digest: String,
    acl: String,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct McpRoutePolicySelection;

impl Selection for McpRoutePolicySelection {
    type Output = McpRoutePolicyRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            McpRoutePolicies::id().expression(),
            McpRoutePolicies::organization_id().expression(),
            McpRoutePolicies::project_id().expression(),
            McpRoutePolicies::environment_id().expression(),
            McpRoutePolicies::gateway_scope_id().expression(),
            McpRoutePolicies::workload_id().expression(),
            McpRoutePolicies::asset_id().expression(),
            McpRoutePolicies::asset_release_id().expression(),
            McpRoutePolicies::profile_digest().expression(),
            McpRoutePolicies::hostname().expression(),
            McpRoutePolicies::path().expression(),
            McpRoutePolicies::policy_revision().expression(),
            McpRoutePolicies::policy_digest().expression(),
            McpRoutePolicies::acl().expression(),
            McpRoutePolicies::expires_at().expression(),
            McpRoutePolicies::created_at().expression(),
            McpRoutePolicies::updated_at().expression(),
        ]
    }
}

impl FromRow for McpRoutePolicyRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            gateway_scope_id: decode(row, 4)?,
            workload_id: decode(row, 5)?,
            asset_id: decode(row, 6)?,
            asset_release_id: decode(row, 7)?,
            profile_digest: decode(row, 8)?,
            hostname: decode(row, 9)?,
            path: decode(row, 10)?,
            policy_revision: decode(row, 11)?,
            policy_digest: decode(row, 12)?,
            acl: decode(row, 13)?,
            expires_at: decode(row, 14)?,
            created_at: decode(row, 15)?,
            updated_at: decode(row, 16)?,
        })
    }
}

impl McpRoutePolicyRow {
    fn policy(self, profile: &McpServiceProfile) -> Result<McpRoutePolicy, RepositoryError> {
        let policy = McpRoutePolicy::restore(
            &self.acl,
            &self.policy_digest,
            self.created_at,
            self.updated_at,
            profile,
        )
        .map_err(stored)?;
        let spec = policy.spec();
        if spec.route_id.as_uuid() != self.id
            || spec.organization_id.as_uuid() != self.organization_id
            || spec.project_id.as_uuid() != self.project_id
            || spec.environment_id.as_uuid() != self.environment_id
            || spec.gateway_scope_id.as_uuid() != self.gateway_scope_id
            || spec.workload_id.as_uuid() != self.workload_id
            || spec.asset_id.as_uuid() != self.asset_id
            || spec.asset_release_id.as_uuid() != self.asset_release_id
            || spec.profile_digest.as_str() != self.profile_digest
            || spec.hostname.as_str() != self.hostname
            || spec.path != self.path
            || policy.policy_revision() != self.policy_revision
            || spec.expires_at != self.expires_at
        {
            return Err(RepositoryError::Storage(
                "stored MCP route policy columns do not match its canonical ACL".into(),
            ));
        }
        Ok(policy)
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
    RepositoryError::Storage(format!("stored MCP route policy is invalid: {error}"))
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
