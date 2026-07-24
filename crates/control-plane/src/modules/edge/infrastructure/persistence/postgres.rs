use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, EdgeRoutePublicationResult, GatewayCertificateConvergenceResult,
    GatewayCertificateConvergenceTarget, GatewayRouteCutoverResult, IEdgeRepository,
    StageGatewayCertificateConvergence, StageGatewayRouteCutover, StageRoutePublication,
    TransitionDomainClaim,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainNamePattern, GatewayCertificate, GatewayPublication,
    GatewayPublicationState, GatewayRouteCutover, GatewayScopeState, Route, RouteHostname,
    RoutePath, RoutePortName, RouteState, UpstreamEndpoint,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId, IdempotentWrite,
    NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError, RouteId, WorkloadId,
    WorkloadRevisionId,
};
use a3s_cloud_contracts::{GatewayCertificateRequest, NodeGatewayAck};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::postgres_tls::{self as tls, insert_certificate};
use super::{postgres_certificate_convergence, postgres_cutovers};

pub(super) const SELECT_ROUTES: &str = "select id, organization_id, project_id, environment_id, gateway_node_id, hostname, path_prefix, workload_id, workload_revision_id, port_name, upstream_origin, state, gateway_revision, gateway_command_id, snapshot_digest, failure, aggregate_version, created_at, updated_at, activated_at, domain_claim_id, domain_pattern, gateway_certificate_id from routes";
pub(super) const SELECT_PUBLICATIONS: &str = "select node_id, revision, expected_revision, command_id, command_correlation_id, snapshot_digest, acl, state, failure, command_issued_at, command_not_after, snapshot_expires_at, acknowledged_at, certificate_request from gateway_publications";

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
    gateway_node_id: Uuid,
    hostname: String,
    path_prefix: String,
    workload_id: Uuid,
    workload_revision_id: Uuid,
    port_name: String,
    upstream_origin: String,
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

impl FromRow for RouteRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            gateway_node_id: decode(row, 4)?,
            hostname: decode(row, 5)?,
            path_prefix: decode(row, 6)?,
            workload_id: decode(row, 7)?,
            workload_revision_id: decode(row, 8)?,
            port_name: decode(row, 9)?,
            upstream_origin: decode(row, 10)?,
            state: decode(row, 11)?,
            gateway_revision: decode(row, 12)?,
            gateway_command_id: decode(row, 13)?,
            snapshot_digest: decode(row, 14)?,
            failure: decode(row, 15)?,
            aggregate_version: decode(row, 16)?,
            created_at: decode(row, 17)?,
            updated_at: decode(row, 18)?,
            activated_at: decode(row, 19)?,
            domain_claim_id: decode(row, 20)?,
            domain_pattern: decode(row, 21)?,
            gateway_certificate_id: decode(row, 22)?,
        })
    }
}

impl RouteRow {
    pub(super) fn route(self) -> Result<Route, RepositoryError> {
        let route = Route {
            id: RouteId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
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
            workload_id: WorkloadId::from_uuid(self.workload_id),
            workload_revision_id: WorkloadRevisionId::from_uuid(self.workload_revision_id),
            port_name: RoutePortName::parse(self.port_name).map_err(stored("port name"))?,
            upstream: UpstreamEndpoint::parse(self.upstream_origin)
                .map_err(stored("upstream endpoint"))?,
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
                sql_query::<(u64, Option<u64>, u64)>(
                    "select last_issued_revision, installed_revision, aggregate_version from gateway_scopes where node_id = ",
                )
                .bind(node_id.as_uuid()),
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
            sql_query::<RouteRow>(SELECT_ROUTES)
                .append(" where gateway_node_id = ")
                .bind(node_id.as_uuid())
                .append(" and state = 'active' order by hostname, path_prefix, id"),
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
                    let organization_id = fetch_optional::<Uuid, _>(
                        transaction,
                        sql_query::<Uuid>("select organization_id from nodes where id = ")
                            .bind(bundle.publication.node_id.as_uuid())
                            .append(" for update"),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if organization_id != bundle.route.organization_id.as_uuid() {
                        return Err(RepositoryError::NotFound.into());
                    }
                    let scope = fetch_optional::<(u64, Option<u64>, u64), _>(
                        transaction,
                        sql_query::<(u64, Option<u64>, u64)>(
                            "select last_issued_revision, installed_revision, aggregate_version from gateway_scopes where node_id = ",
                        )
                        .bind(bundle.publication.node_id.as_uuid())
                        .append(" for update"),
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
                    let pending = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from gateway_publications where node_id = ",
                        )
                        .bind(bundle.publication.node_id.as_uuid())
                        .append(" and state = 'pending' for update"),
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
                                sql_query::<()>(
                                    "insert into gateway_scopes (node_id, last_issued_revision, installed_revision, aggregate_version, updated_at) values (",
                                )
                                .bind(bundle.publication.node_id.as_uuid())
                                .append(", ")
                                .bind(bundle.publication.revision)
                                .append(", ")
                                .bind(current.installed_revision)
                                .append(", 1, ")
                                .bind(bundle.publication.command_issued_at)
                                .append(")"),
                            )
                            .await?,
                        )?;
                    } else {
                        require_one_row(
                            "Gateway scope",
                            execute(
                                transaction,
                                sql_query::<()>(
                                    "update gateway_scopes set last_issued_revision = ",
                                )
                                .bind(bundle.publication.revision)
                                .append(", aggregate_version = aggregate_version + 1, updated_at = ")
                                .bind(bundle.publication.command_issued_at)
                                .append(" where node_id = ")
                                .bind(bundle.publication.node_id.as_uuid())
                                .append(" and aggregate_version = ")
                                .bind(current.aggregate_version),
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
                sql_query::<RouteRow>(SELECT_ROUTES)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(route_id.as_uuid()),
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
            sql_query::<RouteRow>(SELECT_ROUTES)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and environment_id = ")
                .bind(environment_id.as_uuid())
                .append(" order by created_at, id"),
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
        sql_query::<()>(
            "insert into gateway_publications (node_id, revision, expected_revision, command_id, command_correlation_id, snapshot_digest, acl, state, failure, command_issued_at, command_not_after, snapshot_expires_at, acknowledged_at, certificate_request) values (",
        )
        .bind(publication.node_id.as_uuid())
        .append(", ")
        .bind(publication.revision)
        .append(", ")
        .bind(publication.expected_revision)
        .append(", ")
        .bind(publication.command_id.as_uuid())
        .append(", ")
        .bind(publication.command_correlation_id)
        .append(", ")
        .bind(publication.snapshot_digest.as_str())
        .append(", ")
        .bind(publication.acl.as_str())
        .append(", ")
        .bind(publication.state.as_str())
        .append(", ")
        .bind(publication.failure.as_deref())
        .append(", ")
        .bind(publication.command_issued_at)
        .append(", ")
        .bind(publication.command_not_after)
        .append(", ")
        .bind(publication.snapshot_expires_at)
        .append(", ")
        .bind(publication.acknowledged_at)
        .append(", ")
        .bind(certificate_request)
        .append(")"),
    )
    .await;
    map_insert("Gateway publication", result)
}

async fn insert_route(
    transaction: &a3s_orm::PostgresTransaction,
    route: &Route,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        sql_query::<()>(
            "insert into routes (id, organization_id, project_id, environment_id, gateway_node_id, hostname, path_prefix, workload_id, workload_revision_id, port_name, upstream_origin, state, gateway_revision, gateway_command_id, snapshot_digest, failure, aggregate_version, created_at, updated_at, activated_at, domain_claim_id, domain_pattern, gateway_certificate_id) values (",
        )
        .bind(route.id.as_uuid())
        .append(", ")
        .bind(route.organization_id.as_uuid())
        .append(", ")
        .bind(route.project_id.as_uuid())
        .append(", ")
        .bind(route.environment_id.as_uuid())
        .append(", ")
        .bind(route.gateway_node_id.as_uuid())
        .append(", ")
        .bind(route.hostname.as_str())
        .append(", ")
        .bind(route.path_prefix.as_str())
        .append(", ")
        .bind(route.workload_id.as_uuid())
        .append(", ")
        .bind(route.workload_revision_id.as_uuid())
        .append(", ")
        .bind(route.port_name.as_str())
        .append(", ")
        .bind(route.upstream.as_str())
        .append(", ")
        .bind(route.state.as_str())
        .append(", ")
        .bind(route.gateway_revision)
        .append(", ")
        .bind(route.gateway_command_id.map(|id| id.as_uuid()))
        .append(", ")
        .bind(route.snapshot_digest.as_deref())
        .append(", ")
        .bind(route.failure.as_deref())
        .append(", ")
        .bind(route.aggregate_version)
        .append(", ")
        .bind(route.created_at)
        .append(", ")
        .bind(route.updated_at)
        .append(", ")
        .bind(route.activated_at)
        .append(", ")
        .bind(route.domain_claim_id.map(|id| id.as_uuid()))
        .append(", ")
        .bind(route.domain_pattern.as_ref().map(|pattern| pattern.as_str()))
        .append(", ")
        .bind(route.gateway_certificate_id.map(|id| id.as_uuid()))
        .append(")"),
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

pub(super) async fn query_routes(
    executor: &PostgresExecutor,
    query: a3s_orm::SqlQuery<RouteRow>,
) -> Result<Vec<Route>, RepositoryError> {
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
