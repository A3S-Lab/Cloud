use crate::modules::edge::domain::events::GatewayRouteCutoverStaged;
use crate::modules::edge::domain::repositories::{
    GatewayRouteCutoverResult, IEdgeRepository, StageGatewayRouteCutover,
};
use crate::modules::edge::domain::services::IGatewayCommandQueue;
use crate::modules::edge::domain::{
    GatewayCertificate, GatewayPublication, GatewayRouteCutover, GatewayRouteCutoverState,
    RouteState, UpstreamEndpoint,
};
use crate::modules::edge::infrastructure::{GatewaySnapshotCompiler, GatewaySnapshotMetadata};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayCertificateId, IdempotencyRequest, NodeCommandId, RepositoryError,
};
use crate::modules::workloads::domain::services::{
    DeploymentGatewayPublication, DeploymentRouteObservation, DeploymentRouteStage,
    DeploymentRouteUpdateRequest, IDeploymentRouteUpdater,
};
use a3s_cloud_contracts::RuntimeServiceEndpoint;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use uuid::Uuid;

pub struct EdgeDeploymentRouteUpdater {
    routes: Arc<dyn IEdgeRepository>,
    observations: Arc<dyn INodeControlRepository>,
    commands: Arc<dyn IGatewayCommandQueue>,
    compiler: GatewaySnapshotCompiler,
    command_ttl: Duration,
}

impl EdgeDeploymentRouteUpdater {
    pub fn new(
        routes: Arc<dyn IEdgeRepository>,
        observations: Arc<dyn INodeControlRepository>,
        commands: Arc<dyn IGatewayCommandQueue>,
        compiler: GatewaySnapshotCompiler,
        command_ttl: Duration,
    ) -> Result<Self, String> {
        if command_ttl <= Duration::zero() {
            return Err("deployment Gateway publication command TTL must be positive".into());
        }
        Ok(Self {
            routes,
            observations,
            commands,
            compiler,
            command_ttl,
        })
    }

    async fn replay(
        &self,
        request: &DeploymentRouteUpdateRequest,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DeploymentRouteStage>, RepositoryError> {
        let Some(result) = self
            .routes
            .replay_gateway_route_cutover(idempotency)
            .await?
        else {
            return Ok(None);
        };
        validate_replay(request, &result)?;
        self.commands.enqueue(&result.publication).await?;
        Ok(Some(staged(&result)))
    }
}

#[async_trait]
impl IDeploymentRouteUpdater for EdgeDeploymentRouteUpdater {
    async fn stage(
        &self,
        request: &DeploymentRouteUpdateRequest,
        now: DateTime<Utc>,
    ) -> Result<DeploymentRouteStage, RepositoryError> {
        let now = canonical_timestamp(now).max(request.verified_at);
        if request.previous_revision_id == request.candidate_revision_id {
            return Ok(DeploymentRouteStage::Failed {
                reason: "Gateway route cutover must select a new immutable revision".into(),
            });
        }
        let canonical = serde_json::to_vec(request)
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!("deployments/{}/route-cutover", request.deployment_id),
            "gateway-route-cutover",
            &canonical,
        )
        .map_err(RepositoryError::Conflict)?;
        if let Some(replay) = self.replay(request, &idempotency).await? {
            return Ok(replay);
        }
        if now >= request.convergence_deadline {
            return Ok(DeploymentRouteStage::Failed {
                reason: "Gateway route cutover did not stage before the deployment deadline".into(),
            });
        }

        let environment_routes = self
            .routes
            .list_routes(
                request.organization_id,
                request.project_id,
                request.environment_id,
            )
            .await?;
        let workload_routes = environment_routes
            .into_iter()
            .filter(|route| {
                route.state == RouteState::Active && route.workload_id == request.workload_id
            })
            .collect::<Vec<_>>();
        if workload_routes.is_empty() {
            return Ok(DeploymentRouteStage::NotRequired { checked_at: now });
        }
        if workload_routes.iter().any(|route| {
            route.workload_revision_id != request.previous_revision_id
                || route.gateway_node_id != request.node_id
        }) {
            return Ok(DeploymentRouteStage::Failed {
                reason: "one-node update routes do not match the previous active revision and node"
                    .into(),
            });
        }

        let observation = self
            .observations
            .latest_runtime_observation(
                request.node_id,
                &request.spec.unit_id,
                request.spec.generation,
            )
            .await?
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "candidate route target has no durable Runtime observation".into(),
                )
            })?;
        if observation.command_id != Some(request.runtime_command_id) {
            return Ok(DeploymentRouteStage::Failed {
                reason: "candidate route target observation belongs to another command".into(),
            });
        }
        observation
            .observation
            .validate_against(&request.spec)
            .map_err(RepositoryError::Conflict)?;
        if !observation.observation.converges(&request.spec) {
            return Ok(DeploymentRouteStage::Failed {
                reason: "candidate route target is not healthy at the requested generation".into(),
            });
        }
        if observation.received_at > now {
            return Ok(DeploymentRouteStage::Failed {
                reason: "candidate route target observation is in the future".into(),
            });
        }

        let (scope, active_routes) = tokio::try_join!(
            self.routes.gateway_scope(request.node_id),
            self.routes.active_routes(request.node_id)
        )?;
        let expected_route_ids = workload_routes
            .iter()
            .map(|route| route.id)
            .collect::<BTreeSet<_>>();
        let scope_route_ids = active_routes
            .iter()
            .filter(|route| {
                route.workload_id == request.workload_id
                    && route.workload_revision_id == request.previous_revision_id
            })
            .map(|route| route.id)
            .collect::<BTreeSet<_>>();
        if expected_route_ids != scope_route_ids {
            return Ok(DeploymentRouteStage::Blocked {
                reason: "Gateway scope changed while selecting the workload routes".into(),
            });
        }

        let issued_at = active_routes
            .iter()
            .map(|route| route.updated_at)
            .fold(now, DateTime::<Utc>::max);
        let command_not_after = issued_at
            .checked_add_signed(self.command_ttl)
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "Gateway route cutover command expiry exceeds supported time".into(),
                )
            })?
            .min(request.convergence_deadline);
        if command_not_after <= issued_at {
            return Ok(DeploymentRouteStage::Failed {
                reason: "Gateway route cutover command expired before staging".into(),
            });
        }
        let snapshot_expires_at = issued_at
            .checked_add_signed(Duration::hours(24))
            .ok_or_else(|| {
                RepositoryError::Conflict(
                    "Gateway route cutover snapshot expiry exceeds supported time".into(),
                )
            })?;
        let certificate_id = GatewayCertificateId::from_uuid(Uuid::new_v5(
            &request.deployment_id.as_uuid(),
            b"gateway-route-cutover-certificate",
        ));
        let command_id = NodeCommandId::from_uuid(Uuid::new_v5(
            &request.deployment_id.as_uuid(),
            b"gateway-route-cutover-command",
        ));
        let mut candidates = Vec::with_capacity(workload_routes.len());
        for route in &workload_routes {
            let endpoint = RuntimeServiceEndpoint::from_observation(
                &observation.observation,
                route.port_name.as_str(),
            )
            .map_err(RepositoryError::Conflict)?;
            candidates.push(
                route
                    .prepare_cutover(
                        request.candidate_revision_id,
                        UpstreamEndpoint::parse(endpoint.origin)
                            .map_err(RepositoryError::Conflict)?,
                        certificate_id,
                        issued_at,
                    )
                    .map_err(RepositoryError::Conflict)?,
            );
        }
        let candidates_by_id = candidates
            .iter()
            .cloned()
            .map(|route| (route.id, route))
            .collect::<BTreeMap<_, _>>();
        let complete_routes = active_routes
            .into_iter()
            .map(|route| candidates_by_id.get(&route.id).cloned().unwrap_or(route))
            .collect::<Vec<_>>();
        let gateway_revision = scope.next_revision().map_err(RepositoryError::Conflict)?;
        let snapshot = self
            .compiler
            .compile(
                GatewaySnapshotMetadata::new(
                    request.node_id,
                    gateway_revision,
                    scope.installed_revision,
                    issued_at,
                    snapshot_expires_at,
                ),
                certificate_id,
                &complete_routes,
            )
            .map_err(RepositoryError::Conflict)?;
        for route in &mut candidates {
            route
                .stage(
                    gateway_revision,
                    command_id,
                    snapshot.snapshot_digest.clone(),
                    issued_at,
                )
                .map_err(RepositoryError::Conflict)?;
        }
        let publication = GatewayPublication::stage(
            request.node_id,
            command_id,
            request.operation_id.as_uuid(),
            snapshot,
            issued_at,
            command_not_after,
        )
        .map_err(RepositoryError::Conflict)?;
        let certificate_request = publication.certificate_request.clone().ok_or_else(|| {
            RepositoryError::Storage(
                "Gateway route cutover publication omitted its certificate request".into(),
            )
        })?;
        let mut domain_claim_ids = complete_routes
            .iter()
            .filter_map(|route| route.domain_claim_id)
            .collect::<Vec<_>>();
        domain_claim_ids.sort();
        domain_claim_ids.dedup();
        let certificate = GatewayCertificate::provision(
            certificate_id,
            request.organization_id,
            request.node_id,
            domain_claim_ids,
            gateway_revision,
            command_id,
            publication.snapshot_digest.clone(),
            certificate_request,
            issued_at,
        )
        .map_err(RepositoryError::Conflict)?;
        let cutover = GatewayRouteCutover::stage(
            request.deployment_id,
            request.organization_id,
            request.workload_id,
            request.previous_revision_id,
            request.candidate_revision_id,
            request.node_id,
            gateway_revision,
            command_id,
            certificate_id,
            publication.snapshot_digest.clone(),
            publication.snapshot_expires_at,
            candidates,
            issued_at,
        )
        .map_err(RepositoryError::Conflict)?;
        let event = GatewayRouteCutoverStaged::envelope(&cutover, &publication)
            .map_err(RepositoryError::Conflict)?;
        let result = match self
            .routes
            .stage_gateway_route_cutover(StageGatewayRouteCutover {
                cutover,
                certificate,
                publication,
                expected_scope_version: scope.aggregate_version,
                idempotency,
                event,
            })
            .await
        {
            Ok(result) => result,
            Err(RepositoryError::Conflict(reason)) => {
                return Ok(DeploymentRouteStage::Blocked { reason })
            }
            Err(error) => return Err(error),
        };
        self.commands.enqueue(&result.publication).await?;
        Ok(staged(&result))
    }

    async fn observe(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        publication: &DeploymentGatewayPublication,
        now: DateTime<Utc>,
    ) -> Result<DeploymentRouteObservation, RepositoryError> {
        let cutover = self
            .routes
            .find_gateway_route_cutover(organization_id, publication.deployment_id)
            .await?
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "staged deployment Gateway route cutover is missing".into(),
                )
            })?;
        if cutover.node_id != publication.node_id
            || cutover.gateway_revision != publication.revision
            || cutover.gateway_command_id != publication.command_id
            || cutover.snapshot_digest != publication.snapshot_digest
        {
            return Err(RepositoryError::Conflict(
                "deployment Gateway publication identity changed during observation".into(),
            ));
        }
        match cutover.state {
            GatewayRouteCutoverState::Pending
                if canonical_timestamp(now) >= publication.command_not_after =>
            {
                Ok(DeploymentRouteObservation::Expired)
            }
            GatewayRouteCutoverState::Pending => Ok(DeploymentRouteObservation::Pending),
            GatewayRouteCutoverState::Applied => Ok(DeploymentRouteObservation::Applied {
                acknowledged_at: cutover.acknowledged_at.ok_or_else(|| {
                    RepositoryError::Storage(
                        "applied Gateway route cutover omitted its acknowledgement time".into(),
                    )
                })?,
            }),
            GatewayRouteCutoverState::Rejected => Ok(DeploymentRouteObservation::Rejected {
                reason: cutover.failure.ok_or_else(|| {
                    RepositoryError::Storage(
                        "rejected Gateway route cutover omitted its failure".into(),
                    )
                })?,
                acknowledged_at: cutover.acknowledged_at.ok_or_else(|| {
                    RepositoryError::Storage(
                        "rejected Gateway route cutover omitted its acknowledgement time".into(),
                    )
                })?,
            }),
        }
    }
}

fn validate_replay(
    request: &DeploymentRouteUpdateRequest,
    result: &GatewayRouteCutoverResult,
) -> Result<(), RepositoryError> {
    let cutover = &result.cutover;
    if cutover.deployment_id != request.deployment_id
        || cutover.organization_id != request.organization_id
        || cutover.workload_id != request.workload_id
        || cutover.previous_revision_id != request.previous_revision_id
        || cutover.candidate_revision_id != request.candidate_revision_id
        || cutover.node_id != request.node_id
        || result.publication.command_correlation_id != request.operation_id.as_uuid()
    {
        return Err(RepositoryError::IdempotencyConflict);
    }
    Ok(())
}

fn staged(result: &GatewayRouteCutoverResult) -> DeploymentRouteStage {
    DeploymentRouteStage::Staged {
        publication: DeploymentGatewayPublication {
            deployment_id: result.cutover.deployment_id,
            node_id: result.publication.node_id,
            revision: result.publication.revision,
            command_id: result.publication.command_id,
            snapshot_digest: result.publication.snapshot_digest.clone(),
            command_not_after: result.publication.command_not_after,
        },
    }
}
