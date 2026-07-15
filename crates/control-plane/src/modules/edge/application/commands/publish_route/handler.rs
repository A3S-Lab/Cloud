use super::{PublishRoute, PublishRouteResult};
use crate::modules::edge::domain::repositories::{IEdgeRepository, StageRoutePublication};
use crate::modules::edge::domain::services::{IGatewayCommandQueue, IRouteTargetReader};
use crate::modules::edge::domain::{
    GatewayPublication, Route, RouteHostname, RoutePath, RoutePortName,
};
use crate::modules::edge::infrastructure::GatewaySnapshotCompiler;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, NodeCommandId, RouteId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Duration;
use std::sync::Arc;

pub struct PublishRouteHandler {
    routes: Arc<dyn IEdgeRepository>,
    targets: Arc<dyn IRouteTargetReader>,
    commands: Arc<dyn IGatewayCommandQueue>,
    compiler: GatewaySnapshotCompiler,
    command_ttl: Duration,
}

impl PublishRouteHandler {
    pub fn new(
        routes: Arc<dyn IEdgeRepository>,
        targets: Arc<dyn IRouteTargetReader>,
        commands: Arc<dyn IGatewayCommandQueue>,
        compiler: GatewaySnapshotCompiler,
        command_ttl: Duration,
    ) -> Result<Self, String> {
        if command_ttl <= Duration::zero() {
            return Err("Gateway publication command TTL must be positive".into());
        }
        Ok(Self {
            routes,
            targets,
            commands,
            compiler,
            command_ttl,
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
        let targets = Arc::clone(&self.targets);
        let commands = Arc::clone(&self.commands);
        let compiler = self.compiler.clone();
        let command_ttl = self.command_ttl;
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
                "workload_revision_id": command.workload_revision_id,
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
            let target = match targets
                .resolve_healthy_target(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.workload_revision_id,
                    &port_name,
                    command.requested_at,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let mut route = Route::create(
                RouteId::new(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                target.node_id,
                hostname,
                path_prefix,
                target.workload_id,
                target.workload_revision_id,
                port_name,
                target.upstream,
                command.requested_at,
            );
            let (scope, mut active_routes) = match tokio::try_join!(
                routes.gateway_scope(target.node_id),
                routes.active_routes(target.node_id)
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let revision = match scope.next_revision() {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            active_routes.push(route.clone());
            let snapshot = match compiler.compile(
                target.node_id,
                revision,
                scope.installed_revision,
                &active_routes,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let command_id = NodeCommandId::new();
            if let Err(error) = route.stage(
                revision,
                command_id,
                snapshot.snapshot_digest.clone(),
                command.requested_at,
            ) {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let not_after = match command.requested_at.checked_add_signed(command_ttl) {
                Some(value) => value,
                None => {
                    return Ok(Err(ApplicationError::Invalid(
                        "Gateway publication command expiry exceeds supported time".into(),
                    )))
                }
            };
            let publication = match GatewayPublication::stage(
                target.node_id,
                command_id,
                command.request_id,
                snapshot,
                command.requested_at,
                not_after,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
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
                    publication,
                    expected_scope_version: scope.aggregate_version,
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
