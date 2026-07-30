use super::postgres::{insert_publication, PostgresEdgeRepository, RouteRow, RouteSelection};
use super::postgres_rollout_routes;
use super::postgres_schema::{
    DomainClaims, GatewayPublications, GatewayRouteProjections, GatewayRouteScopes,
    GatewayScopeMembers, GatewayScopes, McpCredentials, McpRoutePolicies, Nodes, Routes, Workloads,
};
use super::postgres_tls::insert_certificate;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, store_outbox, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::{GatewayPublicationState, GatewayScopeState, RouteState};
use crate::modules::edge::infrastructure::{
    CompiledMcpGatewaySnapshot, IMcpGatewaySnapshotRepository, McpGatewaySnapshotStageResult,
    StageMcpGatewaySnapshot,
};
use crate::modules::shared_kernel::domain::{RepositoryError, WorkloadId, WorkloadRevisionId};
use a3s_orm::expression::{exists, not};
use a3s_orm::{insert_into, select_from, update_table, OrderDirection, PostgresTransaction};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[async_trait]
impl IMcpGatewaySnapshotRepository for PostgresEdgeRepository {
    async fn stage_mcp_gateway_snapshot(
        &self,
        stage: StageMcpGatewaySnapshot,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
        stage.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let (candidate, publication, certificate, event) = stage.into_parts();
                    let scope = candidate.mcp().scope();
                    let organization_id = fetch_optional::<Uuid, _>(
                        transaction,
                        select_from::<Nodes>()
                            .select(Nodes::organization_id())
                            .filter(Nodes::id().eq(publication.node_id.as_uuid()))
                            .for_update(),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if organization_id != scope.organization_id.as_uuid() {
                        return Err(RepositoryError::NotFound.into());
                    }

                    lock_logical_scope(transaction, &candidate).await?;
                    let physical_scope = lock_physical_scope(transaction, &candidate).await?;
                    if fetch_optional::<u64, _>(
                        transaction,
                        select_from::<GatewayPublications>()
                            .select(GatewayPublications::revision())
                            .filter(
                                GatewayPublications::node_id().eq(publication.node_id.as_uuid()),
                            )
                            .filter(GatewayPublications::state().eq("pending"))
                            .for_update(),
                    )
                    .await?
                    .is_some()
                    {
                        return Err(RepositoryError::Conflict(
                            "Gateway scope already has a pending complete snapshot".into(),
                        )
                        .into());
                    }

                    lock_ordinary_routes(transaction, &candidate).await?;
                    lock_mcp_policies(transaction, &candidate).await?;
                    lock_domain_claims(transaction, &candidate).await?;
                    lock_workloads(transaction, &candidate).await?;
                    lock_credentials(transaction, &candidate).await?;

                    insert_publication(transaction, &publication).await?;
                    if let Some(certificate) = &certificate {
                        insert_certificate(transaction, certificate).await?;
                    }
                    advance_physical_scope(transaction, &physical_scope, &publication).await?;
                    store_outbox(transaction, &event).await?;
                    Ok(McpGatewaySnapshotStageResult {
                        publication,
                        certificate,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn lock_logical_scope(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let scope = candidate.mcp().scope();
    let stored = fetch_optional::<(Uuid, Uuid, Uuid, Uuid, u64, u64), _>(
        transaction,
        select_from::<GatewayRouteScopes>()
            .select((
                GatewayRouteScopes::organization_id(),
                GatewayRouteScopes::project_id(),
                GatewayRouteScopes::environment_id(),
                GatewayRouteScopes::node_id(),
                GatewayRouteScopes::membership_generation(),
                GatewayRouteScopes::aggregate_version(),
            ))
            .filter(GatewayRouteScopes::id().eq(scope.id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    if stored
        != (
            scope.organization_id.as_uuid(),
            scope.project_id.as_uuid(),
            scope.environment_id.as_uuid(),
            scope.node_id.as_uuid(),
            scope.membership_generation,
            scope.aggregate_version,
        )
    {
        return Err(RepositoryError::Conflict(
            "logical Gateway scope changed while planning the MCP snapshot".into(),
        )
        .into());
    }
    let members = fetch_all::<(Uuid, u32, u64), _>(
        transaction,
        select_from::<GatewayScopeMembers>()
            .select((
                GatewayScopeMembers::node_id(),
                GatewayScopeMembers::ordinal(),
                GatewayScopeMembers::membership_generation(),
            ))
            .filter(GatewayScopeMembers::gateway_scope_id().eq(scope.id.as_uuid()))
            .filter(GatewayScopeMembers::organization_id().eq(scope.organization_id.as_uuid()))
            .filter(GatewayScopeMembers::project_id().eq(scope.project_id.as_uuid()))
            .filter(GatewayScopeMembers::environment_id().eq(scope.environment_id.as_uuid()))
            .order_by(GatewayScopeMembers::ordinal(), OrderDirection::Asc)
            .for_update(),
    )
    .await?;
    if members.len() != scope.member_node_ids.len()
        || members.iter().zip(&scope.member_node_ids).enumerate().any(
            |(index, ((node_id, ordinal, generation), expected_node_id))| {
                *node_id != expected_node_id.as_uuid()
                    || usize::try_from(*ordinal).ok() != Some(index)
                    || *generation != scope.membership_generation
            },
        )
    {
        return Err(RepositoryError::Conflict(
            "logical Gateway membership changed while planning the MCP snapshot".into(),
        )
        .into());
    }
    Ok(())
}

async fn lock_physical_scope(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<GatewayScopeState, PostgresPersistenceError> {
    let expected = candidate.physical_scope();
    let row = fetch_optional::<(u64, Option<u64>, u64), _>(
        transaction,
        select_from::<GatewayScopes>()
            .select((
                GatewayScopes::last_issued_revision(),
                GatewayScopes::installed_revision(),
                GatewayScopes::aggregate_version(),
            ))
            .filter(GatewayScopes::node_id().eq(expected.node_id.as_uuid()))
            .for_update(),
    )
    .await?;
    let current = row
        .map(
            |(last_issued_revision, installed_revision, aggregate_version)| GatewayScopeState {
                node_id: expected.node_id,
                last_issued_revision,
                installed_revision,
                aggregate_version,
            },
        )
        .unwrap_or_else(|| GatewayScopeState::empty(expected.node_id));
    validate_physical_scope(&current)?;
    if &current != expected {
        return Err(RepositoryError::Conflict(
            "physical Gateway scope changed while compiling the complete snapshot".into(),
        )
        .into());
    }
    Ok(current)
}

async fn lock_ordinary_routes(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let node_id = candidate.physical_scope().node_id;
    let projected_route = exists(
        select_from::<GatewayRouteProjections>()
            .select(GatewayRouteProjections::route_id())
            .filter(GatewayRouteProjections::route_id().eq_column(Routes::id())),
    );
    let mut active = fetch_all::<RouteRow, _>(
        transaction,
        select_from::<Routes>()
            .select(RouteSelection)
            .filter(Routes::gateway_node_id().eq(node_id.as_uuid()))
            .filter(Routes::state().eq("active"))
            .filter(not(projected_route))
            .order_by(Routes::id(), OrderDirection::Asc)
            .for_update(),
    )
    .await?
    .into_iter()
    .map(RouteRow::route)
    .collect::<Result<Vec<_>, _>>()?;
    active.extend(postgres_rollout_routes::lock_active(transaction, node_id).await?);
    active.sort_by_key(|route| route.id);
    let expected = candidate.active_route_versions();
    if active.len() != expected.len()
        || active.iter().zip(expected).any(|(route, expected)| {
            route.state != RouteState::Active
                || route.gateway_node_id != node_id
                || route.id != expected.route_id
                || route.aggregate_version != expected.aggregate_version
        })
    {
        return Err(RepositoryError::Conflict(
            "ordinary Gateway Route set changed while compiling the complete snapshot".into(),
        )
        .into());
    }
    Ok(())
}

async fn lock_mcp_policies(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let scope = candidate.mcp().scope();
    let rows = fetch_all::<(Uuid, u64, String, Uuid, Uuid, DateTime<Utc>), _>(
        transaction,
        select_from::<McpRoutePolicies>()
            .select((
                McpRoutePolicies::id(),
                McpRoutePolicies::policy_revision(),
                McpRoutePolicies::policy_digest(),
                McpRoutePolicies::workload_id(),
                McpRoutePolicies::domain_claim_id(),
                McpRoutePolicies::updated_at(),
            ))
            .filter(McpRoutePolicies::organization_id().eq(scope.organization_id.as_uuid()))
            .filter(McpRoutePolicies::project_id().eq(scope.project_id.as_uuid()))
            .filter(McpRoutePolicies::environment_id().eq(scope.environment_id.as_uuid()))
            .filter(McpRoutePolicies::gateway_scope_id().eq(scope.id.as_uuid()))
            .filter(McpRoutePolicies::expires_at().gt(candidate.mcp().observed_at()))
            .order_by(McpRoutePolicies::id(), OrderDirection::Asc)
            .limit(1_001)
            .for_update(),
    )
    .await?;
    let expected = candidate.mcp().route_versions();
    if rows.len() != expected.len()
        || rows.iter().zip(expected).any(
            |((route_id, revision, digest, workload_id, domain_claim_id, updated_at), expected)| {
                *route_id != expected.route_id().as_uuid()
                    || *revision != expected.policy_revision()
                    || digest != expected.policy_digest().as_str()
                    || *workload_id != expected.workload_id().as_uuid()
                    || *domain_claim_id != expected.domain_claim_id().as_uuid()
                    || *updated_at > candidate.mcp().observed_at()
            },
        )
    {
        return Err(RepositoryError::Conflict(
            "active MCP route-policy set changed while compiling the complete snapshot".into(),
        )
        .into());
    }
    Ok(())
}

async fn lock_domain_claims(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let mcp_claims = candidate
        .mcp()
        .route_versions()
        .iter()
        .map(|version| version.domain_claim_id())
        .collect::<BTreeSet<_>>();
    for expected in candidate.domain_claim_versions() {
        let row = fetch_optional::<
            (
                Uuid,
                Uuid,
                Uuid,
                String,
                Option<String>,
                u64,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
            ),
            _,
        >(
            transaction,
            select_from::<DomainClaims>()
                .select((
                    DomainClaims::organization_id(),
                    DomainClaims::project_id(),
                    DomainClaims::environment_id(),
                    DomainClaims::state(),
                    DomainClaims::failure(),
                    DomainClaims::aggregate_version(),
                    DomainClaims::updated_at(),
                    DomainClaims::revoked_at(),
                ))
                .filter(DomainClaims::id().eq(expected.domain_claim_id().as_uuid()))
                .for_update(),
        )
        .await?
        .ok_or_else(|| {
            RepositoryError::Conflict(
                "Gateway snapshot DomainClaim disappeared before staging".into(),
            )
        })?;
        let (
            organization_id,
            project_id,
            environment_id,
            state,
            failure,
            aggregate_version,
            updated_at,
            revoked_at,
        ) = row;
        let is_mcp_claim = mcp_claims.contains(&expected.domain_claim_id());
        let scope = candidate.mcp().scope();
        if organization_id != scope.organization_id.as_uuid()
            || is_mcp_claim
                && (project_id != scope.project_id.as_uuid()
                    || environment_id != scope.environment_id.as_uuid())
            || state != "verified"
            || failure.is_some()
            || aggregate_version != expected.aggregate_version()
            || updated_at > candidate.mcp().observed_at()
            || revoked_at.is_some()
        {
            return Err(RepositoryError::Conflict(
                "Gateway snapshot DomainClaim authority changed before staging".into(),
            )
            .into());
        }
    }
    Ok(())
}

async fn lock_workloads(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let mut expected = BTreeMap::<WorkloadId, (u64, WorkloadRevisionId)>::new();
    for version in candidate.mcp().route_versions() {
        match expected.get(&version.workload_id()) {
            Some(value)
                if *value
                    != (
                        version.workload_aggregate_version(),
                        version.active_revision_id(),
                    ) =>
            {
                return Err(RepositoryError::Conflict(
                    "MCP snapshot observed conflicting versions of one Workload".into(),
                )
                .into())
            }
            Some(_) => {}
            None => {
                expected.insert(
                    version.workload_id(),
                    (
                        version.workload_aggregate_version(),
                        version.active_revision_id(),
                    ),
                );
            }
        }
    }
    let scope = candidate.mcp().scope();
    for (workload_id, (aggregate_version, active_revision_id)) in expected {
        let row = fetch_optional::<(Uuid, Uuid, Uuid, String, Option<Uuid>, u64), _>(
            transaction,
            select_from::<Workloads>()
                .select((
                    Workloads::organization_id(),
                    Workloads::project_id(),
                    Workloads::environment_id(),
                    Workloads::desired_state(),
                    Workloads::active_revision_id(),
                    Workloads::aggregate_version(),
                ))
                .filter(Workloads::id().eq(workload_id.as_uuid()))
                .for_update(),
        )
        .await?
        .ok_or_else(|| {
            RepositoryError::Conflict(
                "MCP snapshot Workload disappeared before Gateway staging".into(),
            )
        })?;
        if row
            != (
                scope.organization_id.as_uuid(),
                scope.project_id.as_uuid(),
                scope.environment_id.as_uuid(),
                "running".to_owned(),
                Some(active_revision_id.as_uuid()),
                aggregate_version,
            )
        {
            return Err(RepositoryError::Conflict(
                "MCP snapshot Workload authority changed before Gateway staging".into(),
            )
            .into());
        }
    }
    Ok(())
}

async fn lock_credentials(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let Some(projection) = candidate.mcp().projection() else {
        return Ok(());
    };
    let scope = candidate.mcp().scope();
    for expected in projection.credential_versions() {
        let row = fetch_optional::<
            (
                Uuid,
                Uuid,
                Uuid,
                u64,
                u64,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
            ),
            _,
        >(
            transaction,
            select_from::<McpCredentials>()
                .select((
                    McpCredentials::organization_id(),
                    McpCredentials::project_id(),
                    McpCredentials::environment_id(),
                    McpCredentials::generation(),
                    McpCredentials::aggregate_version(),
                    McpCredentials::expires_at(),
                    McpCredentials::revoked_at(),
                ))
                .filter(McpCredentials::id().eq(expected.credential_id().as_uuid()))
                .for_update(),
        )
        .await?
        .ok_or_else(|| {
            RepositoryError::Conflict(
                "MCP snapshot credential disappeared before Gateway staging".into(),
            )
        })?;
        if row.0 != scope.organization_id.as_uuid()
            || row.1 != scope.project_id.as_uuid()
            || row.2 != scope.environment_id.as_uuid()
            || row.3 != expected.generation()
            || row.4 != expected.aggregate_version()
            || row.5 <= candidate.mcp().observed_at()
            || row.6.is_some()
        {
            return Err(RepositoryError::Conflict(
                "MCP snapshot credential authority changed before Gateway staging".into(),
            )
            .into());
        }
    }
    Ok(())
}

async fn advance_physical_scope(
    transaction: &PostgresTransaction,
    current: &GatewayScopeState,
    publication: &crate::modules::edge::domain::GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    if publication.state != GatewayPublicationState::Pending
        || publication.node_id != current.node_id
        || publication.revision != current.next_revision().map_err(RepositoryError::Conflict)?
        || publication.expected_revision != current.installed_revision
    {
        return Err(RepositoryError::Conflict(
            "MCP Gateway publication does not advance its exact physical scope".into(),
        )
        .into());
    }
    let next_version = current.aggregate_version.checked_add(1).ok_or_else(|| {
        PostgresPersistenceError::Invariant("Gateway scope aggregate version overflowed".into())
    })?;
    if current.aggregate_version == 0 {
        require_one_row(
            "MCP Gateway scope",
            execute(
                transaction,
                insert_into::<GatewayScopes>()
                    .value(GatewayScopes::node_id(), publication.node_id.as_uuid())
                    .value(GatewayScopes::last_issued_revision(), publication.revision)
                    .value(
                        GatewayScopes::installed_revision(),
                        current.installed_revision,
                    )
                    .value(GatewayScopes::aggregate_version(), next_version)
                    .value(GatewayScopes::updated_at(), publication.command_issued_at),
            )
            .await?,
        )?;
    } else {
        require_one_row(
            "MCP Gateway scope",
            execute(
                transaction,
                update_table::<GatewayScopes>()
                    .set(GatewayScopes::last_issued_revision(), publication.revision)
                    .set(GatewayScopes::aggregate_version(), next_version)
                    .set(GatewayScopes::updated_at(), publication.command_issued_at)
                    .filter(GatewayScopes::node_id().eq(publication.node_id.as_uuid()))
                    .filter(GatewayScopes::aggregate_version().eq(current.aggregate_version)),
            )
            .await?,
        )?;
    }
    Ok(())
}

fn validate_physical_scope(scope: &GatewayScopeState) -> Result<(), PostgresPersistenceError> {
    if scope.node_id.as_uuid().is_nil()
        || scope
            .installed_revision
            .is_some_and(|revision| revision == 0 || revision > scope.last_issued_revision)
        || if scope.last_issued_revision == 0 {
            scope.aggregate_version != 0 || scope.installed_revision.is_some()
        } else {
            scope.aggregate_version == 0
        }
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored physical Gateway scope is invalid".into(),
        ));
    }
    Ok(())
}
