use super::{PublishRoute, PublishRouteResult};
use crate::modules::edge::domain::repositories::{IEdgeRepository, StageRoutePublication};
use crate::modules::edge::domain::services::{IGatewayCommandQueue, IRouteTargetReader};
use crate::modules::edge::domain::{RouteHostname, RoutePath, RoutePortName};
use crate::modules::edge::infrastructure::{
    GatewayRouteRolloutCompiler, GatewayRouteRolloutPlanner, GatewaySnapshotCompiler,
    PlanGatewayRouteRollout,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{GatewayRolloutId, IdempotencyRequest, RouteId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Duration;
use std::sync::Arc;

pub struct PublishRouteHandler {
    routes: Arc<dyn IEdgeRepository>,
    commands: Arc<dyn IGatewayCommandQueue>,
    rollout_planner: GatewayRouteRolloutPlanner,
}

impl PublishRouteHandler {
    pub fn new(
        routes: Arc<dyn IEdgeRepository>,
        targets: Arc<dyn IRouteTargetReader>,
        commands: Arc<dyn IGatewayCommandQueue>,
        compiler: GatewaySnapshotCompiler,
        command_ttl: Duration,
    ) -> Result<Self, String> {
        let rollout_compiler =
            GatewayRouteRolloutCompiler::new(compiler, command_ttl, Duration::hours(24))?;
        let rollout_planner =
            GatewayRouteRolloutPlanner::new(Arc::clone(&routes), targets, rollout_compiler);
        Ok(Self {
            routes,
            commands,
            rollout_planner,
        })
    }
}

impl CommandHandler<PublishRoute> for PublishRouteHandler {
    fn execute(
        &self,
        command: PublishRoute,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PublishRouteResult>>> {
        let routes = Arc::clone(&self.routes);
        let commands = Arc::clone(&self.commands);
        let rollout_planner = self.rollout_planner.clone();
        Box::pin(async move {
            let hostname = match RouteHostname::parse(command.hostname) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let path_prefix = match RoutePath::parse(command.path_prefix) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let port_name = match RoutePortName::parse(command.port_name) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organization_id": command.organization_id,
                "project_id": command.project_id,
                "environment_id": command.environment_id,
                "gateway_scope_id": command.gateway_scope_id,
                "workload_revision_id": command.workload_revision_id,
                "domain_claim_id": command.domain_claim_id,
                "hostname": hostname.as_str(),
                "path_prefix": path_prefix.as_str(),
                "port_name": port_name.as_str(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/routes",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match routes.replay_route_publication(&idempotency).await {
                Ok(Some(publication)) => {
                    let dispatched = match commands.enqueue(&publication.publication).await {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(error.into())),
                    };
                    return Ok(Ok(PublishRouteResult {
                        publication,
                        command_replayed: dispatched.replayed,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let claim = match routes
                .find_domain_claim(command.organization_id, command.domain_claim_id)
                .await
            {
                Ok(value)
                    if value.project_id == command.project_id
                        && value.environment_id == command.environment_id
                        && value.covers(&hostname) =>
                {
                    value
                }
                Ok(_) => {
                    return Ok(Err(ApplicationError::Conflict(
                        "verified domain claim does not cover this route and environment".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let gateway_scope = match routes
                .find_gateway_scope(command.organization_id, command.gateway_scope_id)
                .await
            {
                Ok(value)
                    if value.project_id == command.project_id
                        && value.environment_id == command.environment_id =>
                {
                    value
                }
                Ok(_) => {
                    return Ok(Err(ApplicationError::Conflict(
                        "Gateway scope does not belong to this project and environment".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if gateway_scope.member_node_ids.len() != 1 {
                return Ok(Err(ApplicationError::Conflict(
                    "replicated Gateway scope route publication requires durable rollout staging"
                        .into(),
                )));
            }
            let planned = match rollout_planner
                .plan(PlanGatewayRouteRollout {
                    scope: gateway_scope.clone(),
                    rollout_id: GatewayRolloutId::new(),
                    generation: 1,
                    correlation_id: command.request_id,
                    route_id: RouteId::new(),
                    workload_revision_id: command.workload_revision_id,
                    hostname,
                    path_prefix,
                    port_name,
                    domain_claim_id: claim.id,
                    domain_pattern: claim.pattern,
                    issued_at: command.requested_at,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let target_node_id = gateway_scope.node_id;
            let route = planned
                .primary_route()
                .map_err(BootError::Internal)?
                .clone();
            let publication = planned
                .publications
                .iter()
                .find(|publication| publication.node_id == target_node_id)
                .cloned()
                .ok_or_else(|| {
                    BootError::Internal(
                        "compiled Gateway rollout omitted its primary publication".into(),
                    )
                })?;
            let certificate = planned
                .certificates
                .iter()
                .find(|certificate| certificate.node_id == target_node_id)
                .cloned()
                .ok_or_else(|| {
                    BootError::Internal(
                        "compiled Gateway rollout omitted its primary certificate".into(),
                    )
                })?;
            let expected_scope_version = planned
                .expected_scope_versions
                .get(&target_node_id)
                .copied()
                .ok_or_else(|| {
                    BootError::Internal(
                        "compiled Gateway rollout omitted its primary scope version".into(),
                    )
                })?;
            let event = match crate::modules::edge::domain::events::RoutePublicationStaged::envelope(
                &route,
                &publication,
            ) {
                Ok(value) => value,
                Err(error) => return Err(BootError::Internal(error.to_string())),
            };
            let staged = match routes
                .stage_route_publication(StageRoutePublication {
                    route,
                    gateway_scope,
                    certificate,
                    publication,
                    expected_scope_version,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let dispatched = match commands.enqueue(&staged.publication).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(PublishRouteResult {
                publication: staged,
                command_replayed: dispatched.replayed,
            }))
        })
    }
}
