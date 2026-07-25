use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, EdgeRoutePublicationResult,
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayRolloutDispatchTarget, GatewayRolloutResult, GatewayRouteCutoverResult, IEdgeRepository,
    StageGatewayCertificateConvergence, StageGatewayRollout, StageGatewayRouteCutover,
    StageRoutePublication, TransitionDomainClaim,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainNamePattern, GatewayCertificate, GatewayPublication,
    GatewayPublicationState, GatewayRollout, GatewayRouteCutover, GatewayScope, GatewayScopeState,
    Route, RouteHostname, RoutePath, RoutePortName, RouteState, RouteTarget, UpstreamEndpoint,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayRolloutId,
    GatewayScopeId, IdempotentWrite, NodeCommandId, NodeId, OrganizationId, ProjectId,
    RepositoryError, RouteId, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{GatewayCertificateRequest, NodeGatewayAck};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, update_table, Database, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresDialect, PostgresExecutor, Query, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::postgres_schema::{GatewayPublications, GatewayScopes, Nodes, Routes};
use super::postgres_tls::{self as tls, insert_certificate};
use super::{
    postgres_certificate_convergence, postgres_cutovers, postgres_gateway_scopes, postgres_rollouts,
};

#[derive(Clone)]
pub struct PostgresEdgeRepository {
    executor: PostgresExecutor,
}

impl PostgresEdgeRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

pub(super) struct RouteRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    gateway_scope_id: Uuid,
    gateway_node_id: Uuid,
    hostname: String,
    path_prefix: String,
    workload_id: Uuid,
    workload_revision_id: Uuid,
    runtime_unit_id: String,
    runtime_generation: u64,
    port_name: String,
    upstream_origin: String,
    target_observed_at: DateTime<Utc>,
    state: String,
    gateway_revision: u64,
    gateway_command_id: Uuid,
    snapshot_digest: String,
    failure: Option<String>,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    activated_at: Option<DateTime<Utc>>,
    domain_claim_id: Option<Uuid>,
    domain_pattern: Option<String>,
    gateway_certificate_id: Option<Uuid>,
}

pub(super) struct RouteSelection;

impl Selection for RouteSelection {
    type Output = RouteRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            Routes::id().expression(),
            Routes::organization_id().expression(),
            Routes::project_id().expression(),
            Routes::environment_id().expression(),
            Routes::gateway_scope_id().expression(),
            Routes::gateway_node_id().expression(),
            Routes::hostname().expression(),
            Routes::path_prefix().expression(),
            Routes::workload_id().expression(),
            Routes::workload_revision_id().expression(),
            Routes::runtime_unit_id().expression(),
            Routes::runtime_generation().expression(),
            Routes::port_name().expression(),
            Routes::upstream_origin().expression(),
            Routes::target_observed_at().expression(),
            Routes::state().expression(),
            Routes::gateway_revision().expression(),
            Routes::gateway_command_id().expression(),
            Routes::snapshot_digest().expression(),
            Routes::failure().expression(),
            Routes::aggregate_version().expression(),
            Routes::created_at().expression(),
            Routes::updated_at().expression(),
            Routes::activated_at().expression(),
            Routes::domain_claim_id().expression(),
            Routes::domain_pattern().expression(),
            Routes::gateway_certificate_id().expression(),
        ]
    }
}

impl FromRow for RouteRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            gateway_scope_id: decode(row, 4)?,
            gateway_node_id: decode(row, 5)?,
            hostname: decode(row, 6)?,
            path_prefix: decode(row, 7)?,
            workload_id: decode(row, 8)?,
            workload_revision_id: decode(row, 9)?,
            runtime_unit_id: decode(row, 10)?,
            runtime_generation: decode(row, 11)?,
            port_name: decode(row, 12)?,
            upstream_origin: decode(row, 13)?,
            target_observed_at: decode(row, 14)?,
            state: decode(row, 15)?,
            gateway_revision: decode(row, 16)?,
            gateway_command_id: decode(row, 17)?,
            snapshot_digest: decode(row, 18)?,
            failure: decode(row, 19)?,
            aggregate_version: decode(row, 20)?,
            created_at: decode(row, 21)?,
            updated_at: decode(row, 22)?,
            activated_at: decode(row, 23)?,
            domain_claim_id: decode(row, 24)?,
            domain_pattern: decode(row, 25)?,
            gateway_certificate_id: decode(row, 26)?,
        })
    }
}

impl RouteRow {
    pub(super) fn route(self) -> Result<Route, RepositoryError> {
        let workload_id = WorkloadId::from_uuid(self.workload_id);
        let target = RouteTarget::new(
            workload_id,
            WorkloadRevisionId::from_uuid(self.workload_revision_id),
            self.runtime_unit_id,
            self.runtime_generation,
            RoutePortName::parse(self.port_name).map_err(stored("port name"))?,
            UpstreamEndpoint::parse(self.upstream_origin).map_err(stored("upstream endpoint"))?,
            self.target_observed_at,
        )
        .map_err(stored("target"))?;
        let route = Route {
            id: RouteId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            gateway_scope_id: GatewayScopeId::from_uuid(self.gateway_scope_id),
            gateway_node_id: NodeId::from_uuid(self.gateway_node_id),
            hostname: RouteHostname::parse(self.hostname).map_err(stored("hostname"))?,
            path_prefix: RoutePath::parse(self.path_prefix).map_err(stored("path"))?,
            domain_claim_id: self.domain_claim_id.map(DomainClaimId::from_uuid),
            domain_pattern: self
                .domain_pattern
                .map(DomainNamePattern::parse)
                .transpose()
                .map_err(stored("domain pattern"))?,
            gateway_certificate_id: self
                .gateway_certificate_id
                .map(GatewayCertificateId::from_uuid),
            workload_id,
            target,
            state: RouteState::parse(&self.state).map_err(stored("state"))?,
            gateway_revision: Some(self.gateway_revision),
            gateway_command_id: Some(NodeCommandId::from_uuid(self.gateway_command_id)),
            snapshot_digest: Some(self.snapshot_digest),
            failure: self.failure,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            activated_at: self.activated_at,
        };
        validate_stored_route(&route)?;
        Ok(route)
    }
}

pub(super) struct PublicationRow {
    node_id: Uuid,
    revision: u64,
    expected_revision: Option<u64>,
    command_id: Uuid,
    command_correlation_id: Uuid,
    snapshot_digest: String,
    acl: String,
    state: String,
    failure: Option<String>,
    command_issued_at: DateTime<Utc>,
    command_not_after: DateTime<Utc>,
    snapshot_expires_at: DateTime<Utc>,
    acknowledged_at: Option<DateTime<Utc>>,
    certificate_request: Option<serde_json::Value>,
}

pub(super) struct PublicationSelection;

impl Selection for PublicationSelection {
    type Output = PublicationRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayPublications::node_id().expression(),
            GatewayPublications::revision().expression(),
            GatewayPublications::expected_revision().expression(),
            GatewayPublications::command_id().expression(),
            GatewayPublications::command_correlation_id().expression(),
            GatewayPublications::snapshot_digest().expression(),
            GatewayPublications::acl().expression(),
            GatewayPublications::state().expression(),
            GatewayPublications::failure().expression(),
            GatewayPublications::command_issued_at().expression(),
            GatewayPublications::command_not_after().expression(),
            GatewayPublications::snapshot_expires_at().expression(),
            GatewayPublications::acknowledged_at().expression(),
            GatewayPublications::certificate_request().expression(),
        ]
    }
}

impl FromRow for PublicationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            node_id: decode(row, 0)?,
            revision: decode(row, 1)?,
            expected_revision: decode(row, 2)?,
            command_id: decode(row, 3)?,
            command_correlation_id: decode(row, 4)?,
            snapshot_digest: decode(row, 5)?,
            acl: decode(row, 6)?,
            state: decode(row, 7)?,
            failure: decode(row, 8)?,
            command_issued_at: decode(row, 9)?,
            command_not_after: decode(row, 10)?,
            snapshot_expires_at: decode(row, 11)?,
            acknowledged_at: decode(row, 12)?,
            certificate_request: decode(row, 13)?,
        })
    }
}

impl PublicationRow {
    pub(super) fn publication(self) -> Result<GatewayPublication, RepositoryError> {
        let certificate_request = self
            .certificate_request
            .map(serde_json::from_value::<GatewayCertificateRequest>)
            .transpose()
            .map_err(|error| stored("certificate request")(error.to_string()))?;
        let publication = GatewayPublication {
            node_id: NodeId::from_uuid(self.node_id),
            revision: self.revision,
            expected_revision: self.expected_revision,
            command_id: NodeCommandId::from_uuid(self.command_id),
            command_correlation_id: self.command_correlation_id,
            snapshot_digest: self.snapshot_digest,
            acl: self.acl,
            certificate_request,
            state: GatewayPublicationState::parse(&self.state).map_err(stored("state"))?,
            failure: self.failure,
            command_issued_at: self.command_issued_at,
            command_not_after: self.command_not_after,
            snapshot_expires_at: self.snapshot_expires_at,
            acknowledged_at: self.acknowledged_at,
        };
        publication.snapshot().map_err(stored("snapshot"))?;
        Ok(publication)
    }
}

#[async_trait]
impl IEdgeRepository for PostgresEdgeRepository {
    async fn create_gateway_scope(
        &self,
        bundle: CreateGatewayScopeWrite,
    ) -> Result<IdempotentWrite<GatewayScope>, RepositoryError> {
        postgres_gateway_scopes::create(&self.executor, bundle).await
    }

    async fn find_gateway_scope(
        &self,
        organization_id: OrganizationId,
        scope_id: GatewayScopeId,
    ) -> Result<GatewayScope, RepositoryError> {
        postgres_gateway_scopes::find(&self.executor, organization_id, scope_id).await
    }

    async fn list_gateway_scopes(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<GatewayScope>, RepositoryError> {
        postgres_gateway_scopes::list(&self.executor, organization_id, project_id, environment_id)
            .await
    }

    async fn replay_domain_claim_write(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<DomainClaim>, RepositoryError> {
        tls::replay_domain_claim_write(&self.executor, idempotency).await
    }

    async fn create_domain_claim(
        &self,
        bundle: CreateDomainClaimWrite,
    ) -> Result<IdempotentWrite<DomainClaim>, RepositoryError> {
        tls::create_domain_claim(&self.executor, bundle).await
    }

    async fn transition_domain_claim(
        &self,
        bundle: TransitionDomainClaim,
    ) -> Result<IdempotentWrite<DomainClaim>, RepositoryError> {
        tls::transition_domain_claim(&self.executor, bundle).await
    }

    async fn find_domain_claim(
        &self,
        organization_id: OrganizationId,
        claim_id: DomainClaimId,
    ) -> Result<DomainClaim, RepositoryError> {
        tls::find_domain_claim(&self.executor, organization_id, claim_id).await
    }

    async fn list_domain_claims(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<DomainClaim>, RepositoryError> {
        tls::list_domain_claims(&self.executor, organization_id, project_id, environment_id).await
    }

    async fn replay_route_publication(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<EdgeRoutePublicationResult>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(mut replay) =
                        idempotency_replay::<EdgeRoutePublicationResult>(transaction, &idempotency)
                            .await?
                    else {
                        return Ok(None);
                    };
                    replay.value.replayed = true;
                    Ok(Some(replay.value))
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn gateway_scope(&self, node_id: NodeId) -> Result<GatewayScopeState, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                select_from::<GatewayScopes>()
                    .select((
                        GatewayScopes::last_issued_revision(),
                        GatewayScopes::installed_revision(),
                        GatewayScopes::aggregate_version(),
                    ))
                    .filter(GatewayScopes::node_id().eq(node_id.as_uuid())),
            )
            .await
            .map_err(storage)?;
        match row {
            Some((last_issued_revision, installed_revision, aggregate_version)) => {
                validate_scope(last_issued_revision, installed_revision, aggregate_version)?;
                Ok(GatewayScopeState {
                    node_id,
                    last_issued_revision,
                    installed_revision,
                    aggregate_version,
                })
            }
            None => Ok(GatewayScopeState::empty(node_id)),
        }
    }

    async fn active_routes(&self, node_id: NodeId) -> Result<Vec<Route>, RepositoryError> {
        query_routes(
            &self.executor,
            select_from::<Routes>()
                .select(RouteSelection)
                .filter(Routes::gateway_node_id().eq(node_id.as_uuid()))
                .filter(Routes::state().eq("active"))
                .order_by(Routes::hostname(), OrderDirection::Asc)
                .order_by(Routes::path_prefix(), OrderDirection::Asc)
                .order_by(Routes::id(), OrderDirection::Asc),
        )
        .await
    }

    async fn stage_route_publication(
        &self,
        bundle: StageRoutePublication,
    ) -> Result<EdgeRoutePublicationResult, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(mut replay) = idempotency_replay::<EdgeRoutePublicationResult>(
                        transaction,
                        &bundle.idempotency,
                    )
                    .await?
                    {
                        replay.value.replayed = true;
                        return Ok(replay.value);
                    }
                    postgres_gateway_scopes::validate_route_binding(
                        transaction,
                        &bundle.gateway_scope,
                        &bundle.route,
                    )
                    .await?;
                    let organization_id = fetch_optional::<Uuid, _>(
                        transaction,
                        select_from::<Nodes>()
                            .select(Nodes::organization_id())
                            .filter(Nodes::id().eq(bundle.publication.node_id.as_uuid()))
                            .for_update(),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if organization_id != bundle.route.organization_id.as_uuid() {
                        return Err(RepositoryError::NotFound.into());
                    }
                    let scope = fetch_optional::<(u64, Option<u64>, u64), _>(
                        transaction,
                        select_from::<GatewayScopes>()
                            .select((
                                GatewayScopes::last_issued_revision(),
                                GatewayScopes::installed_revision(),
                                GatewayScopes::aggregate_version(),
                            ))
                            .filter(
                                GatewayScopes::node_id().eq(bundle.publication.node_id.as_uuid()),
                            )
                            .for_update(),
                    )
                    .await?;
                    let current = match scope {
                        Some((last, installed, version)) => {
                            validate_scope(last, installed, version)?;
                            GatewayScopeState {
                                node_id: bundle.publication.node_id,
                                last_issued_revision: last,
                                installed_revision: installed,
                                aggregate_version: version,
                            }
                        }
                        None => GatewayScopeState::empty(bundle.publication.node_id),
                    };
                    if current.aggregate_version != bundle.expected_scope_version {
                        return Err(RepositoryError::Conflict(
                            "Gateway scope changed while compiling the complete snapshot".into(),
                        )
                        .into());
                    }
                    let pending = fetch_optional::<u64, _>(
                        transaction,
                        select_from::<GatewayPublications>()
                            .select(GatewayPublications::revision())
                            .filter(
                                GatewayPublications::node_id()
                                    .eq(bundle.publication.node_id.as_uuid()),
                            )
                            .filter(GatewayPublications::state().eq("pending"))
                            .for_update(),
                    )
                    .await?;
                    if pending.is_some() {
                        return Err(RepositoryError::Conflict(
                            "Gateway scope already has a pending complete snapshot".into(),
                        )
                        .into());
                    }
                    if bundle.publication.revision
                        != current.next_revision().map_err(RepositoryError::Conflict)?
                        || bundle.publication.expected_revision != current.installed_revision
                    {
                        return Err(RepositoryError::Conflict(
                            "Gateway publication does not advance the authoritative scope revision"
                                .into(),
                        )
                        .into());
                    }
                    insert_publication(transaction, &bundle.publication).await?;
                    insert_certificate(transaction, &bundle.certificate).await?;
                    insert_route(transaction, &bundle.route).await?;
                    if current.aggregate_version == 0 {
                        require_one_row(
                            "Gateway scope",
                            execute(
                                transaction,
                                insert_into::<GatewayScopes>()
                                    .value(
                                        GatewayScopes::node_id(),
                                        bundle.publication.node_id.as_uuid(),
                                    )
                                    .value(
                                        GatewayScopes::last_issued_revision(),
                                        bundle.publication.revision,
                                    )
                                    .value(
                                        GatewayScopes::installed_revision(),
                                        current.installed_revision,
                                    )
                                    .value(GatewayScopes::aggregate_version(), 1_u64)
                                    .value(
                                        GatewayScopes::updated_at(),
                                        bundle.publication.command_issued_at,
                                    ),
                            )
                            .await?,
                        )?;
                    } else {
                        let next_version =
                            current.aggregate_version.checked_add(1).ok_or_else(|| {
                                PostgresPersistenceError::Invariant(
                                    "Gateway scope aggregate version overflowed".into(),
                                )
                            })?;
                        require_one_row(
                            "Gateway scope",
                            execute(
                                transaction,
                                update_table::<GatewayScopes>()
                                    .set(
                                        GatewayScopes::last_issued_revision(),
                                        bundle.publication.revision,
                                    )
                                    .set(GatewayScopes::aggregate_version(), next_version)
                                    .set(
                                        GatewayScopes::updated_at(),
                                        bundle.publication.command_issued_at,
                                    )
                                    .filter(
                                        GatewayScopes::node_id()
                                            .eq(bundle.publication.node_id.as_uuid()),
                                    )
                                    .filter(
                                        GatewayScopes::aggregate_version()
                                            .eq(current.aggregate_version),
                                    ),
                            )
                            .await?,
                        )?;
                    }
                    let result = EdgeRoutePublicationResult {
                        route: bundle.route,
                        certificate: bundle.certificate,
                        publication: bundle.publication,
                        replayed: false,
                    };
                    store_outbox(transaction, &bundle.event).await?;
                    store_idempotency(transaction, &bundle.idempotency, &result).await?;
                    Ok(result)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn replay_gateway_route_cutover(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<GatewayRouteCutoverResult>, RepositoryError> {
        postgres_cutovers::replay(&self.executor, idempotency).await
    }

    async fn stage_gateway_route_cutover(
        &self,
        bundle: StageGatewayRouteCutover,
    ) -> Result<GatewayRouteCutoverResult, RepositoryError> {
        postgres_cutovers::stage(&self.executor, bundle).await
    }

    async fn stage_gateway_rollout(
        &self,
        bundle: StageGatewayRollout,
    ) -> Result<GatewayRolloutResult, RepositoryError> {
        postgres_rollouts::stage(&self.executor, bundle).await
    }

    async fn pending_gateway_rollout_dispatches(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayRolloutDispatchTarget>, RepositoryError> {
        postgres_rollouts::pending_dispatches(&self.executor, limit).await
    }

    async fn find_gateway_rollout(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
    ) -> Result<GatewayRollout, RepositoryError> {
        postgres_rollouts::find(&self.executor, organization_id, rollout_id).await
    }

    async fn mark_gateway_rollout_replica_unavailable(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        failure: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<GatewayRollout, RepositoryError> {
        postgres_rollouts::mark_unavailable(
            &self.executor,
            organization_id,
            rollout_id,
            node_id,
            expected_version,
            failure,
            observed_at,
        )
        .await
    }

    async fn gateway_certificate_convergence_targets(
        &self,
        certificate_renew_before: DateTime<Utc>,
        snapshot_renew_before: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<GatewayCertificateConvergenceTarget>, RepositoryError> {
        postgres_certificate_convergence::targets(
            &self.executor,
            certificate_renew_before,
            snapshot_renew_before,
            limit,
        )
        .await
    }

    async fn pending_gateway_certificate_convergences(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayCertificateConvergenceResult>, RepositoryError> {
        postgres_certificate_convergence::pending(&self.executor, limit).await
    }

    async fn stage_gateway_certificate_convergence(
        &self,
        bundle: StageGatewayCertificateConvergence,
    ) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
        postgres_certificate_convergence::stage(&self.executor, bundle).await
    }

    async fn find_gateway_certificate_convergence(
        &self,
        node_id: NodeId,
        gateway_revision: u64,
    ) -> Result<Option<crate::modules::edge::domain::GatewayCertificateConvergence>, RepositoryError>
    {
        postgres_certificate_convergence::find(&self.executor, node_id, gateway_revision).await
    }

    async fn obsolete_gateway_certificates(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayCertificate>, RepositoryError> {
        postgres_certificate_convergence::obsolete_certificates(&self.executor, limit).await
    }

    async fn find_gateway_route_cutover(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<Option<GatewayRouteCutover>, RepositoryError> {
        postgres_cutovers::find(&self.executor, organization_id, deployment_id).await
    }

    async fn find_route(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
    ) -> Result<Route, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                select_from::<Routes>()
                    .select(RouteSelection)
                    .filter(Routes::organization_id().eq(organization_id.as_uuid()))
                    .filter(Routes::id().eq(route_id.as_uuid())),
            )
            .await
            .map_err(storage)?
            .ok_or(RepositoryError::NotFound)?
            .route()
    }

    async fn list_routes(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Route>, RepositoryError> {
        query_routes(
            &self.executor,
            select_from::<Routes>()
                .select(RouteSelection)
                .filter(Routes::organization_id().eq(organization_id.as_uuid()))
                .filter(Routes::project_id().eq(project_id.as_uuid()))
                .filter(Routes::environment_id().eq(environment_id.as_uuid()))
                .order_by(Routes::created_at(), OrderDirection::Asc)
                .order_by(Routes::id(), OrderDirection::Asc),
        )
        .await
    }

    async fn find_gateway_certificate(
        &self,
        node_id: NodeId,
        certificate_id: GatewayCertificateId,
    ) -> Result<GatewayCertificate, RepositoryError> {
        tls::find_gateway_certificate(&self.executor, node_id, certificate_id).await
    }

    async fn list_gateway_certificates(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<GatewayCertificate>, RepositoryError> {
        tls::list_gateway_certificates(&self.executor, organization_id).await
    }

    async fn transition_gateway_certificate(
        &self,
        certificate: GatewayCertificate,
        expected_version: u64,
    ) -> Result<GatewayCertificate, RepositoryError> {
        tls::transition_gateway_certificate(&self.executor, certificate, expected_version).await
    }

    async fn project_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
        received_at: DateTime<Utc>,
    ) -> Result<bool, RepositoryError> {
        super::postgres_acknowledgement::project(&self.executor, acknowledgement, received_at).await
    }
}

pub(super) async fn insert_publication(
    transaction: &a3s_orm::PostgresTransaction,
    publication: &GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    let certificate_request = publication
        .certificate_request
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?;
    let result = execute(
        transaction,
        insert_into::<GatewayPublications>()
            .value(
                GatewayPublications::node_id(),
                publication.node_id.as_uuid(),
            )
            .value(GatewayPublications::revision(), publication.revision)
            .value(
                GatewayPublications::expected_revision(),
                publication.expected_revision,
            )
            .value(
                GatewayPublications::command_id(),
                publication.command_id.as_uuid(),
            )
            .value(
                GatewayPublications::command_correlation_id(),
                publication.command_correlation_id,
            )
            .value(
                GatewayPublications::snapshot_digest(),
                publication.snapshot_digest.as_str(),
            )
            .value(GatewayPublications::acl(), publication.acl.as_str())
            .value(GatewayPublications::state(), publication.state.as_str())
            .value(GatewayPublications::failure(), publication.failure.clone())
            .value(
                GatewayPublications::command_issued_at(),
                publication.command_issued_at,
            )
            .value(
                GatewayPublications::command_not_after(),
                publication.command_not_after,
            )
            .value(
                GatewayPublications::snapshot_expires_at(),
                publication.snapshot_expires_at,
            )
            .value(
                GatewayPublications::acknowledged_at(),
                publication.acknowledged_at,
            )
            .value(
                GatewayPublications::certificate_request(),
                certificate_request,
            ),
    )
    .await;
    map_insert("Gateway publication", result)
}

async fn insert_route(
    transaction: &a3s_orm::PostgresTransaction,
    route: &Route,
) -> Result<(), PostgresPersistenceError> {
    let gateway_revision = route.gateway_revision.ok_or_else(|| {
        PostgresPersistenceError::Invariant("staged route omitted its Gateway revision".into())
    })?;
    let gateway_command_id = route.gateway_command_id.ok_or_else(|| {
        PostgresPersistenceError::Invariant("staged route omitted its Gateway command".into())
    })?;
    let snapshot_digest = route.snapshot_digest.as_deref().ok_or_else(|| {
        PostgresPersistenceError::Invariant("staged route omitted its snapshot digest".into())
    })?;
    let result = execute(
        transaction,
        insert_into::<Routes>()
            .value(Routes::id(), route.id.as_uuid())
            .value(Routes::organization_id(), route.organization_id.as_uuid())
            .value(Routes::project_id(), route.project_id.as_uuid())
            .value(Routes::environment_id(), route.environment_id.as_uuid())
            .value(Routes::gateway_scope_id(), route.gateway_scope_id.as_uuid())
            .value(Routes::gateway_node_id(), route.gateway_node_id.as_uuid())
            .value(Routes::hostname(), route.hostname.as_str())
            .value(Routes::path_prefix(), route.path_prefix.as_str())
            .value(Routes::workload_id(), route.workload_id.as_uuid())
            .value(
                Routes::workload_revision_id(),
                route.target.workload_revision_id.as_uuid(),
            )
            .value(
                Routes::runtime_unit_id(),
                route.target.runtime_unit_id.as_str(),
            )
            .value(
                Routes::runtime_generation(),
                route.target.runtime_generation,
            )
            .value(Routes::port_name(), route.target.port_name.as_str())
            .value(Routes::upstream_origin(), route.target.upstream.as_str())
            .value(Routes::target_observed_at(), route.target.observed_at)
            .value(Routes::state(), route.state.as_str())
            .value(Routes::gateway_revision(), gateway_revision)
            .value(Routes::gateway_command_id(), gateway_command_id.as_uuid())
            .value(Routes::snapshot_digest(), snapshot_digest)
            .value(Routes::failure(), route.failure.clone())
            .value(Routes::aggregate_version(), route.aggregate_version)
            .value(Routes::created_at(), route.created_at)
            .value(Routes::updated_at(), route.updated_at)
            .value(Routes::activated_at(), route.activated_at)
            .value(
                Routes::domain_claim_id(),
                route.domain_claim_id.map(|id| id.as_uuid()),
            )
            .value(
                Routes::domain_pattern(),
                route
                    .domain_pattern
                    .as_ref()
                    .map(|pattern| pattern.as_str().to_owned()),
            )
            .value(
                Routes::gateway_certificate_id(),
                route.gateway_certificate_id.map(|id| id.as_uuid()),
            ),
    )
    .await;
    map_insert("route", result)
}

fn map_insert(
    resource: &str,
    result: Result<u64, PostgresPersistenceError>,
) -> Result<(), PostgresPersistenceError> {
    match result {
        Ok(rows) => require_one_row(resource, rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "hostname and path are already owned in this Gateway scope".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

pub(super) async fn query_routes<Q>(
    executor: &PostgresExecutor,
    query: Q,
) -> Result<Vec<Route>, RepositoryError>
where
    Q: Query<Output = RouteRow>,
{
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(query)
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(RouteRow::route)
        .collect()
}

fn validate_scope(
    last_issued_revision: u64,
    installed_revision: Option<u64>,
    aggregate_version: u64,
) -> Result<(), RepositoryError> {
    if last_issued_revision == 0
        || aggregate_version == 0
        || installed_revision
            .is_some_and(|installed| installed == 0 || installed > last_issued_revision)
    {
        return Err(RepositoryError::Storage(
            "stored Gateway scope state is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_stored_route(route: &Route) -> Result<(), RepositoryError> {
    let status_consistent = match route.state {
        RouteState::Pending => false,
        RouteState::Publishing => route.failure.is_none() && route.activated_at.is_none(),
        RouteState::Active => route.failure.is_none() && route.activated_at.is_some(),
        RouteState::Rejected => route.failure.is_some() && route.activated_at.is_none(),
    };
    let tls_consistent = match (
        route.domain_claim_id,
        route.domain_pattern.as_ref(),
        route.gateway_certificate_id,
    ) {
        (None, None, None) => true,
        (Some(_), Some(pattern), Some(_)) => pattern.covers(&route.hostname),
        _ => false,
    };
    if !status_consistent
        || !tls_consistent
        || route.gateway_revision.is_none()
        || route.gateway_command_id.is_none()
        || route.snapshot_digest.is_none()
        || route.updated_at < route.created_at
    {
        return Err(RepositoryError::Storage(
            "stored route state is inconsistent".into(),
        ));
    }
    route
        .validate_target_binding()
        .map_err(stored("target binding"))?;
    Ok(())
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored route {label} is invalid: {error}"))
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
