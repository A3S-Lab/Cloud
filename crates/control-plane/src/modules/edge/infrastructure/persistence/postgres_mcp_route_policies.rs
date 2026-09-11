use super::postgres::PostgresEdgeRepository;
use super::postgres_schema::{GatewayRouteScopes, McpRoutePolicies};
use crate::infrastructure::{
    AuditWrite, PostgresPersistenceError, execute, fetch_optional, idempotency_replay,
    is_foreign_key_violation, is_unique_violation, require_one_row, store_audit, store_idempotency,
    store_outbox, transaction_error,
};
use crate::modules::edge::domain::EdgeMcpServiceProfileAdmission;
use crate::modules::edge::domain::events::{McpRoutePolicyChanged, McpRoutePolicyMutationKind};
use crate::modules::edge::domain::repositories::{
    IMcpRoutePolicyRepository, MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY, McpRoutePolicyWrite,
    McpRoutePolicyWriteSnapshot, MutateMcpRoutePolicyWrite,
};
use crate::modules::edge::domain::{McpRoutePolicy, McpRoutePolicyDocument, McpRoutePolicySpec};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayScopeId, OrganizationId, ProjectId, RepositoryError, RouteId,
    Sha256Digest, canonical_timestamp,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    Database, DecodeError, Expression, FromRow, FromValue, OrderDirection, PostgresDialect,
    PostgresExecutor, PostgresTransaction, Row, insert_into, select_from, update_table,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[async_trait]
impl IMcpRoutePolicyRepository for PostgresEdgeRepository {
    async fn mutate_mcp_route_policy(
        &self,
        write: MutateMcpRoutePolicyWrite,
    ) -> Result<McpRoutePolicyWrite, RepositoryError> {
        mutate(&self.executor, write).await
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

    async fn list_active_mcp_route_policies_for_gateway(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        gateway_scope_id: GatewayScopeId,
        active_at: DateTime<Utc>,
    ) -> Result<Vec<McpRoutePolicy>, RepositoryError> {
        list_active_for_gateway(
            &self.executor,
            organization_id,
            project_id,
            environment_id,
            gateway_scope_id,
            active_at,
        )
        .await
    }
}

async fn mutate(
    executor: &PostgresExecutor,
    write: MutateMcpRoutePolicyWrite,
) -> Result<McpRoutePolicyWrite, RepositoryError> {
    write.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay(transaction, &write).await? {
                    return Ok(replay);
                }
                let (policy, changed) = match write.kind {
                    McpRoutePolicyMutationKind::Create => {
                        create_in_transaction(transaction, &write).await?
                    }
                    McpRoutePolicyMutationKind::Revise => {
                        revise_in_transaction(transaction, &write).await?
                    }
                };
                if changed {
                    let event = McpRoutePolicyChanged::envelope(
                        &policy,
                        write.kind,
                        write.request_id,
                        write.requested_at,
                    )
                    .map_err(|error| {
                        PostgresPersistenceError::Invariant(format!(
                            "could not build MCP route policy event: {error}"
                        ))
                    })?;
                    store_outbox(transaction, &event).await?;
                }
                store_policy_audit(transaction, &policy, &write, changed).await?;
                store_idempotency(
                    transaction,
                    &write.idempotency,
                    &McpRoutePolicyWriteSnapshot::from(&policy),
                )
                .await?;
                Ok(McpRoutePolicyWrite {
                    policy,
                    replayed: !changed,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

async fn create_in_transaction(
    transaction: &PostgresTransaction,
    write: &MutateMcpRoutePolicyWrite,
) -> Result<(McpRoutePolicy, bool), PostgresPersistenceError> {
    let document = &write.document;
    lock_policy_scope(transaction, document.spec()).await?;
    if let Some(existing) = find_in_transaction(
        transaction,
        document.spec().organization_id,
        document.spec().route_id,
    )
    .await?
    {
        if matches_document(&existing, document) {
            return Ok((existing, false));
        }
        return Err(RepositoryError::Conflict(
            "MCP route policy identity is already in use".into(),
        )
        .into());
    }
    let profile = &write.profile;
    let policy = document
        .materialize(write.requested_at, write.requested_at, profile)
        .map_err(|error| {
            RepositoryError::Conflict(format!("invalid MCP route policy write: {error}"))
        })?;
    validate_supplied(&policy, profile)?;
    insert_policy(transaction, &policy, profile).await?;
    Ok((policy, true))
}

async fn revise_in_transaction(
    transaction: &PostgresTransaction,
    write: &MutateMcpRoutePolicyWrite,
) -> Result<(McpRoutePolicy, bool), PostgresPersistenceError> {
    let document = &write.document;
    lock_policy_scope(transaction, document.spec()).await?;
    let existing = find_in_transaction(
        transaction,
        document.spec().organization_id,
        document.spec().route_id,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    if matches_document(&existing, document) {
        return Ok((existing, false));
    }
    let expected_policy_revision = document
        .policy_revision()
        .checked_sub(1)
        .ok_or_else(|| RepositoryError::Conflict("MCP route policy revision is invalid".into()))?;
    let profile = &write.profile;
    let policy = document
        .materialize(existing.created_at(), write.requested_at, profile)
        .map_err(|error| {
            RepositoryError::Conflict(format!("invalid MCP route policy write: {error}"))
        })?;
    validate_transition(&existing, &policy, expected_policy_revision)?;
    validate_supplied(&policy, profile)?;
    update_policy(transaction, &policy, expected_policy_revision, profile).await?;
    Ok((policy, true))
}

async fn replay(
    transaction: &PostgresTransaction,
    write: &MutateMcpRoutePolicyWrite,
) -> Result<Option<McpRoutePolicyWrite>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<McpRoutePolicyWriteSnapshot>(transaction, &write.idempotency).await?
    else {
        return Ok(None);
    };
    let policy = restore_snapshot(replay.value, &write.profile).await?;
    if !matches_document(&policy, &write.document) {
        return Err(PostgresPersistenceError::Invariant(
            "stored MCP route policy idempotency response does not match the request".into(),
        ));
    }
    Ok(Some(McpRoutePolicyWrite {
        policy,
        replayed: true,
    }))
}

async fn restore_snapshot(
    snapshot: McpRoutePolicyWriteSnapshot,
    profile: &EdgeMcpServiceProfileAdmission,
) -> Result<McpRoutePolicy, PostgresPersistenceError> {
    let document = McpRoutePolicy::parse_acl(&snapshot.canonical_acl).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored MCP route policy idempotency ACL is invalid: {error}"
        ))
    })?;
    if document.policy_digest().as_str() != snapshot.policy_digest {
        return Err(PostgresPersistenceError::Invariant(
            "stored MCP route policy idempotency digest is invalid".into(),
        ));
    }
    McpRoutePolicy::restore(
        &snapshot.canonical_acl,
        &snapshot.policy_digest,
        snapshot.created_at,
        snapshot.updated_at,
        profile,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored MCP route policy idempotency response is invalid: {error}"
        ))
    })
}

fn matches_document(policy: &McpRoutePolicy, document: &McpRoutePolicyDocument) -> bool {
    policy.spec() == document.spec()
        && policy.policy_revision() == document.policy_revision()
        && policy.canonical_acl() == document.canonical_acl()
        && policy.policy_digest() == document.policy_digest()
}

async fn store_policy_audit(
    transaction: &PostgresTransaction,
    policy: &McpRoutePolicy,
    write: &MutateMcpRoutePolicyWrite,
    changed: bool,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: None,
            action: write.kind.action(),
            aggregate_id: policy.spec().route_id.as_uuid(),
            occurred_at: write.requested_at,
            request_id: write.request_id,
            scope: AuditWrite::resource_scope(
                policy.spec().organization_id.as_uuid(),
                policy.spec().project_id,
                Some(policy.spec().environment_id),
            ),
            details: serde_json::json!({
                "projectId": policy.spec().project_id,
                "environmentId": policy.spec().environment_id,
                "policyRevision": policy.policy_revision(),
                "policyDigest": policy.policy_digest().as_str(),
                "changed": changed,
            }),
        },
    )
    .await
}

async fn insert_policy(
    transaction: &PostgresTransaction,
    policy: &McpRoutePolicy,
    profile: &EdgeMcpServiceProfileAdmission,
) -> Result<(), PostgresPersistenceError> {
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
                McpRoutePolicies::domain_claim_id(),
                policy.spec().domain_claim_id.as_uuid(),
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
                McpRoutePolicies::profile_endpoint_path(),
                profile.endpoint_path(),
            )
            .value(
                McpRoutePolicies::profile_max_request_bytes(),
                profile.max_request_bytes(),
            )
            .value(
                McpRoutePolicies::profile_max_response_bytes(),
                profile.max_response_bytes(),
            )
            .value(
                McpRoutePolicies::profile_max_stream_seconds(),
                profile.max_stream_seconds(),
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
        Ok(rows) => require_one_row("MCP route policy", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "MCP route identity or exact Gateway route is already in use".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn update_policy(
    transaction: &PostgresTransaction,
    policy: &McpRoutePolicy,
    expected_policy_revision: u64,
    profile: &EdgeMcpServiceProfileAdmission,
) -> Result<(), PostgresPersistenceError> {
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
                McpRoutePolicies::profile_endpoint_path(),
                profile.endpoint_path(),
            )
            .set(
                McpRoutePolicies::profile_max_request_bytes(),
                profile.max_request_bytes(),
            )
            .set(
                McpRoutePolicies::profile_max_response_bytes(),
                profile.max_response_bytes(),
            )
            .set(
                McpRoutePolicies::profile_max_stream_seconds(),
                profile.max_stream_seconds(),
            )
            .set(
                McpRoutePolicies::domain_claim_id(),
                policy.spec().domain_claim_id.as_uuid(),
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
            .filter(McpRoutePolicies::organization_id().eq(policy.spec().organization_id.as_uuid()))
            .filter(McpRoutePolicies::id().eq(policy.spec().route_id.as_uuid()))
            .filter(McpRoutePolicies::policy_revision().eq(expected_policy_revision)),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("MCP route policy update", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "MCP exact Gateway route is already in use".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn lock_policy_scope(
    transaction: &PostgresTransaction,
    spec: &McpRoutePolicySpec,
) -> Result<(), PostgresPersistenceError> {
    let owner = fetch_optional::<(Uuid, Uuid, Uuid), _>(
        transaction,
        select_from::<GatewayRouteScopes>()
            .select((
                GatewayRouteScopes::organization_id(),
                GatewayRouteScopes::project_id(),
                GatewayRouteScopes::environment_id(),
            ))
            .filter(GatewayRouteScopes::id().eq(spec.gateway_scope_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    if owner
        != (
            spec.organization_id.as_uuid(),
            spec.project_id.as_uuid(),
            spec.environment_id.as_uuid(),
        )
    {
        return Err(RepositoryError::NotFound.into());
    }
    Ok(())
}

async fn find_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    route_id: RouteId,
) -> Result<Option<McpRoutePolicy>, PostgresPersistenceError> {
    let Some(row) = fetch_optional::<McpRoutePolicyRow, _>(
        transaction,
        policy_query(organization_id, route_id).for_update(),
    )
    .await?
    else {
        return Ok(None);
    };
    row.policy().map(Some).map_err(Into::into)
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
    row.policy().map(Some)
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
        policies.push(row.policy()?);
    }
    Ok(policies)
}

async fn list_active_for_gateway(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    gateway_scope_id: GatewayScopeId,
    active_at: DateTime<Utc>,
) -> Result<Vec<McpRoutePolicy>, RepositoryError> {
    if organization_id.as_uuid().is_nil()
        || project_id.as_uuid().is_nil()
        || environment_id.as_uuid().is_nil()
        || gateway_scope_id.as_uuid().is_nil()
    {
        return Err(RepositoryError::Conflict(
            "active MCP Gateway route query identities must not be nil".into(),
        ));
    }
    let active_at = canonical_timestamp(active_at);
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<McpRoutePolicies>()
                .select(McpRoutePolicySelection)
                .filter(McpRoutePolicies::organization_id().eq(organization_id.as_uuid()))
                .filter(McpRoutePolicies::project_id().eq(project_id.as_uuid()))
                .filter(McpRoutePolicies::environment_id().eq(environment_id.as_uuid()))
                .filter(McpRoutePolicies::gateway_scope_id().eq(gateway_scope_id.as_uuid()))
                .filter(McpRoutePolicies::expires_at().gt(active_at))
                .order_by(McpRoutePolicies::id(), OrderDirection::Asc)
                .limit((MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY + 1) as u64),
        )
        .await
        .map_err(storage)?
        .rows;
    if rows.len() > MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY {
        return Err(RepositoryError::Conflict(format!(
            "active MCP Gateway route set exceeds {MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY} routes"
        )));
    }
    rows.into_iter().map(McpRoutePolicyRow::policy).collect()
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

fn validate_supplied(
    policy: &McpRoutePolicy,
    profile: &EdgeMcpServiceProfileAdmission,
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
    domain_claim_id: Uuid,
    workload_id: Uuid,
    asset_id: Uuid,
    asset_release_id: Uuid,
    profile_digest: String,
    profile_endpoint_path: String,
    profile_max_request_bytes: u64,
    profile_max_response_bytes: u64,
    profile_max_stream_seconds: u64,
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
            McpRoutePolicies::domain_claim_id().expression(),
            McpRoutePolicies::workload_id().expression(),
            McpRoutePolicies::asset_id().expression(),
            McpRoutePolicies::asset_release_id().expression(),
            McpRoutePolicies::profile_digest().expression(),
            McpRoutePolicies::profile_endpoint_path().expression(),
            McpRoutePolicies::profile_max_request_bytes().expression(),
            McpRoutePolicies::profile_max_response_bytes().expression(),
            McpRoutePolicies::profile_max_stream_seconds().expression(),
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
            domain_claim_id: decode(row, 5)?,
            workload_id: decode(row, 6)?,
            asset_id: decode(row, 7)?,
            asset_release_id: decode(row, 8)?,
            profile_digest: decode(row, 9)?,
            profile_endpoint_path: decode(row, 10)?,
            profile_max_request_bytes: decode(row, 11)?,
            profile_max_response_bytes: decode(row, 12)?,
            profile_max_stream_seconds: decode(row, 13)?,
            hostname: decode(row, 14)?,
            path: decode(row, 15)?,
            policy_revision: decode(row, 16)?,
            policy_digest: decode(row, 17)?,
            acl: decode(row, 18)?,
            expires_at: decode(row, 19)?,
            created_at: decode(row, 20)?,
            updated_at: decode(row, 21)?,
        })
    }
}

impl McpRoutePolicyRow {
    fn policy(self) -> Result<McpRoutePolicy, RepositoryError> {
        let profile = EdgeMcpServiceProfileAdmission::new(
            Sha256Digest::parse(&self.profile_digest).map_err(stored)?,
            &self.profile_endpoint_path,
            self.profile_max_request_bytes,
            self.profile_max_response_bytes,
            self.profile_max_stream_seconds,
        )
        .map_err(stored)?;
        let policy = McpRoutePolicy::restore(
            &self.acl,
            &self.policy_digest,
            self.created_at,
            self.updated_at,
            &profile,
        )
        .map_err(stored)?;
        let spec = policy.spec();
        if spec.route_id.as_uuid() != self.id
            || spec.organization_id.as_uuid() != self.organization_id
            || spec.project_id.as_uuid() != self.project_id
            || spec.environment_id.as_uuid() != self.environment_id
            || spec.gateway_scope_id.as_uuid() != self.gateway_scope_id
            || spec.domain_claim_id.as_uuid() != self.domain_claim_id
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
