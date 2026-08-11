use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, EdgeRoutePublicationResult,
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayReplicaRecoveryTarget, GatewayRolloutDispatchTarget, GatewayRolloutResult,
    GatewayRolloutRollbackResult, GatewayRouteCutoverResult, IEdgeRepository,
    StageGatewayCertificateConvergence, StageGatewayRollout, StageGatewayRolloutRollback,
    StageGatewayRouteCutover, StageRoutePublication, TransitionDomainClaim,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainNamePattern, GatewayCertificate, GatewayPublication,
    GatewayPublicationState, GatewayRollout, GatewayRolloutRollback, GatewayRouteCutover,
    GatewayScope, GatewayScopeState, Route, RouteHostname, RoutePath, RoutePortName, RouteState,
    RouteTarget, UpstreamEndpoint,
};
use crate::modules::edge::infrastructure::{
    GatewayManagedSnapshotComposition, StageManagedRoutePublication,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayRolloutId,
    GatewayScopeId, IdempotencyRequest, IdempotentWrite, NodeCommandId, NodeId, OrganizationId,
    ProjectId, RepositoryError, RouteId, WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{
    GatewayCertificateRequest, NodeGatewayAck, NodeGatewaySnapshotObservation,
};
use a3s_orm::expression::{exists, not, Selection};
use a3s_orm::{
    insert_into, select_from, update_table, Database, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresDialect, PostgresExecutor, Query, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::postgres_schema::{
    GatewayPublications, GatewayRouteProjections, GatewayScopes, Nodes, Routes,
};
use super::postgres_tls::{self as tls, insert_certificate};
use super::{
    postgres_certificate_convergence, postgres_cutovers, postgres_gateway_scopes,
    postgres_rollout_routes, postgres_rollouts,
};

#[derive(Clone)]
pub struct PostgresEdgeRepository {
    pub(super) executor: PostgresExecutor,
}

impl PostgresEdgeRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

mod models;

pub(super) use models::{PublicationRow, PublicationSelection, RouteRow, RouteSelection};

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
        let projected_route = exists(
            select_from::<GatewayRouteProjections>()
                .select(GatewayRouteProjections::route_id())
                .filter(GatewayRouteProjections::route_id().eq_column(Routes::id())),
        );
        let mut routes = query_routes(
            &self.executor,
            select_from::<Routes>()
                .select(RouteSelection)
                .filter(Routes::gateway_node_id().eq(node_id.as_uuid()))
                .filter(Routes::state().eq("active"))
                .filter(not(projected_route))
                .order_by(Routes::hostname(), OrderDirection::Asc)
                .order_by(Routes::path_prefix(), OrderDirection::Asc)
                .order_by(Routes::id(), OrderDirection::Asc),
        )
        .await?;
        routes.extend(postgres_rollout_routes::active(&self.executor, node_id).await?);
        routes.sort_by(|left, right| {
            (left.hostname.as_str(), left.path_prefix.as_str(), left.id).cmp(&(
                right.hostname.as_str(),
                right.path_prefix.as_str(),
                right.id,
            ))
        });
        Ok(routes)
    }

    async fn stage_route_publication(
        &self,
        bundle: StageRoutePublication,
    ) -> Result<EdgeRoutePublicationResult, RepositoryError> {
        stage_route_publication_impl(&self.executor, bundle, None).await
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

    async fn stage_gateway_rollout_rollback(
        &self,
        bundle: StageGatewayRolloutRollback,
    ) -> Result<GatewayRolloutRollbackResult, RepositoryError> {
        postgres_rollouts::stage_rollback(&self.executor, bundle).await
    }

    async fn replay_gateway_rollout(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<GatewayRolloutResult>, RepositoryError> {
        postgres_rollouts::replay(&self.executor, idempotency).await
    }

    async fn next_gateway_rollout_generation(
        &self,
        organization_id: OrganizationId,
        gateway_scope_id: GatewayScopeId,
    ) -> Result<u64, RepositoryError> {
        postgres_rollouts::next_generation(&self.executor, organization_id, gateway_scope_id).await
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

    async fn find_gateway_rollout_rollback(
        &self,
        organization_id: OrganizationId,
        failed_rollout_id: GatewayRolloutId,
    ) -> Result<GatewayRolloutRollback, RepositoryError> {
        postgres_rollouts::find_rollback(&self.executor, organization_id, failed_rollout_id).await
    }

    async fn pending_gateway_rollout_rollbacks(
        &self,
        limit: usize,
    ) -> Result<
        Vec<crate::modules::edge::domain::repositories::GatewayRolloutRollbackTarget>,
        RepositoryError,
    > {
        postgres_rollouts::pending_rollbacks(&self.executor, limit).await
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

    async fn pending_gateway_replica_recoveries(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayReplicaRecoveryTarget>, RepositoryError> {
        postgres_rollouts::pending_recoveries(&self.executor, limit).await
    }

    async fn stage_gateway_replica_recovery_observation(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        command_id: NodeCommandId,
        issued_at: DateTime<Utc>,
        not_after: DateTime<Utc>,
    ) -> Result<GatewayRollout, RepositoryError> {
        postgres_rollouts::stage_recovery_observation(
            &self.executor,
            organization_id,
            rollout_id,
            node_id,
            expected_version,
            command_id,
            issued_at,
            not_after,
        )
        .await
    }

    async fn record_gateway_replica_recovery_observation(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        observation: NodeGatewaySnapshotObservation,
    ) -> Result<GatewayRollout, RepositoryError> {
        postgres_rollouts::record_recovery_observation(
            &self.executor,
            organization_id,
            rollout_id,
            node_id,
            expected_version,
            observation,
        )
        .await
    }

    async fn record_gateway_replica_recovery_failure(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        command_id: NodeCommandId,
        failure: &str,
        retryable: bool,
        failed_at: DateTime<Utc>,
    ) -> Result<GatewayRollout, RepositoryError> {
        postgres_rollouts::record_recovery_failure(
            &self.executor,
            organization_id,
            rollout_id,
            node_id,
            expected_version,
            command_id,
            failure,
            retryable,
            failed_at,
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

    async fn mark_gateway_certificate_convergence_unavailable(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
        gateway_revision: u64,
        gateway_command_id: NodeCommandId,
        failure: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
        postgres_certificate_convergence::mark_unavailable(
            &self.executor,
            organization_id,
            node_id,
            gateway_revision,
            gateway_command_id,
            failure,
            observed_at,
        )
        .await
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

pub(super) async fn stage_managed_route_publication(
    executor: &PostgresExecutor,
    stage: StageManagedRoutePublication,
) -> Result<EdgeRoutePublicationResult, RepositoryError> {
    let (ordinary, composition) = stage.into_parts();
    stage_route_publication_impl(executor, ordinary, Some(composition)).await
}

async fn stage_route_publication_impl(
    executor: &PostgresExecutor,
    bundle: StageRoutePublication,
    composition: Option<GatewayManagedSnapshotComposition>,
) -> Result<EdgeRoutePublicationResult, RepositoryError> {
    bundle.validate().map_err(RepositoryError::Conflict)?;
    if let Some(composition) = &composition {
        composition
            .validate_for(&bundle.publication)
            .map_err(RepositoryError::Conflict)?;
    }
    executor
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
                let managed_scope = match &composition {
                    Some(composition) => Some(
                        super::postgres_mcp_gateway_snapshots::lock_managed_composition(
                            transaction,
                            composition,
                        )
                        .await?,
                    ),
                    None => None,
                };
                postgres_gateway_scopes::validate_route_binding(
                    transaction,
                    &bundle.gateway_scope,
                    &bundle.route,
                )
                .await?;
                let current = match managed_scope {
                    Some(current) => current,
                    None => {
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
                                    GatewayScopes::node_id()
                                        .eq(bundle.publication.node_id.as_uuid()),
                                )
                                .for_update(),
                        )
                        .await?;
                        match scope {
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
                        }
                    }
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
                            GatewayPublications::node_id().eq(bundle.publication.node_id.as_uuid()),
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
                if let Some(composition) = &composition {
                    super::postgres_mcp_gateway_snapshots::persist_managed_composition(
                        transaction,
                        composition,
                        &bundle.publication,
                    )
                    .await?;
                }
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

pub(super) async fn insert_route(
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
        RouteState::Rejected | RouteState::Unavailable => {
            route.failure.is_some() && route.activated_at.is_none()
        }
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
