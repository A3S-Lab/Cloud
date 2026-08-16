use super::resource_access::{deployment_not_found, environment};
use crate::modules::durable_cells::domain::{
    DurableCellDeployment, DurableCellServiceProfile, IDurableCellDeploymentRepository,
};
use crate::modules::edge::{PublishRoute, PublishRouteHandler, PublishRouteResult};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DomainClaimId, DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId,
    GatewayScopeId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

/// Internal C4 composition command. Edge remains the only Route and Gateway
/// publication authority; the Durable Cells context supplies only the exact
/// C3 correlation and the public port from canonical A3S ACL.
#[derive(Debug, Clone)]
pub struct PublishDurableCellApplicationRoute {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub application_revision_id: DurableCellApplicationRevisionId,
    pub service_profile_acl: String,
    pub gateway_scope_id: GatewayScopeId,
    pub domain_claim_id: DomainClaimId,
    pub hostname: String,
    pub path_prefix: String,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for PublishDurableCellApplicationRoute {
    type Output = ApplicationResult<DurableCellRoutePublicationResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DurableCellRoutePublicationResult {
    pub correlation: DurableCellDeployment,
    pub route: PublishRouteResult,
}

pub struct PublishDurableCellApplicationRouteHandler {
    deployments: Arc<dyn IDurableCellDeploymentRepository>,
    routes: PublishRouteHandler,
}

impl PublishDurableCellApplicationRouteHandler {
    pub fn new(
        deployments: Arc<dyn IDurableCellDeploymentRepository>,
        routes: PublishRouteHandler,
    ) -> Self {
        Self {
            deployments,
            routes,
        }
    }
}

impl CommandHandler<PublishDurableCellApplicationRoute>
    for PublishDurableCellApplicationRouteHandler
{
    fn execute(
        &self,
        command: PublishDurableCellApplicationRoute,
        context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DurableCellRoutePublicationResult>>,
    > {
        let deployments = Arc::clone(&self.deployments);
        let routes = self.routes.clone();
        Box::pin(async move {
            if let Err(error) = environment(
                command.project_id,
                command.environment_id,
                &command.resource_access,
            ) {
                return Ok(Err(error));
            }
            let profile = match DurableCellServiceProfile::parse_acl(&command.service_profile_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let correlation = match deployments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.application_id,
                    command.application_revision_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(deployment_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            correlation.validate().map_err(BootError::Internal)?;
            if !matches_exact_deployment(&command, &correlation) {
                return Err(BootError::Internal(
                    "Durable Cell route correlation changed its exact deployment identity".into(),
                ));
            }
            if correlation.provider.service_profile_digest != *profile.digest() {
                return Ok(Err(ApplicationError::Conflict(
                    "Durable Cell route Service profile does not match the exact deployment".into(),
                )));
            }

            let route = match routes
                .execute(
                    PublishRoute {
                        organization_id: command.organization_id,
                        project_id: command.project_id,
                        environment_id: command.environment_id,
                        gateway_scope_id: command.gateway_scope_id,
                        workload_revision_id: correlation.projection.workload_revision_id,
                        domain_claim_id: command.domain_claim_id,
                        hostname: command.hostname.clone(),
                        path_prefix: command.path_prefix.clone(),
                        port_name: profile.spec().public_runtime_port.clone(),
                        idempotency_key: command.idempotency_key.clone(),
                        request_id: command.request_id,
                        requested_at: command.requested_at,
                    },
                    context,
                )
                .await?
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            validate_public_route(&command, &correlation, &profile, &route)
                .map_err(BootError::Internal)?;
            Ok(Ok(DurableCellRoutePublicationResult { correlation, route }))
        })
    }
}

fn matches_exact_deployment(
    command: &PublishDurableCellApplicationRoute,
    correlation: &DurableCellDeployment,
) -> bool {
    let projection = &correlation.projection;
    projection.organization_id == command.organization_id
        && projection.project_id == command.project_id
        && projection.environment_id == command.environment_id
        && projection.application_id == command.application_id
        && projection.application_revision_id == command.application_revision_id
}

fn validate_public_route(
    command: &PublishDurableCellApplicationRoute,
    correlation: &DurableCellDeployment,
    profile: &DurableCellServiceProfile,
    result: &PublishRouteResult,
) -> Result<(), String> {
    let route = &result.publication.route;
    if route.organization_id != command.organization_id
        || route.project_id != command.project_id
        || route.environment_id != command.environment_id
        || route.gateway_scope_id != command.gateway_scope_id
        || route.workload_id != correlation.projection.workload_id
        || route.target.workload_revision_id != correlation.projection.workload_revision_id
        || route.target.port_name.as_str() != profile.spec().public_runtime_port
        || route.target.port_name.as_str() == profile.spec().internal_runtime_port
    {
        return Err(
            "Edge returned a Route outside the exact Durable Cell public deployment binding".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::durable_cells::domain::{
        CreateDurableCellDeploymentWrite, DurableCellApplication, DurableCellApplicationDefinition,
        DurableCellApplicationDefinitionSpec, DurableCellApplicationRevision, DurableCellClassSpec,
        DurableCellProjectionIdentity, DurableCellProviderBinding, DurableCellRollbackPolicy,
        DurableCellServiceProfileSpec, DurableCellStateSchema, DurableCellStorageBinding,
    };
    use crate::modules::durable_cells::infrastructure::InMemoryDurableCellDeploymentRepository;
    use crate::modules::edge::domain::events::{DomainClaimChanged, GatewayScopeCreated};
    use crate::modules::edge::domain::repositories::{
        CreateDomainClaimWrite, CreateGatewayScopeWrite, IEdgeRepository, TransitionDomainClaim,
    };
    use crate::modules::edge::domain::services::{
        GatewayCommandDispatch, IGatewayCommandQueue, IRouteTargetReader, ResolvedRouteTarget,
    };
    use crate::modules::edge::domain::{
        DomainClaim, DomainNamePattern, GatewayPublication, GatewayScope, RoutePortName,
        RouteTarget, UpstreamEndpoint,
    };
    use crate::modules::edge::infrastructure::{
        GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
    };
    use crate::modules::edge::InMemoryEdgeRepository;
    use crate::modules::identity::domain::value_objects::ResourceGrantScope;
    use crate::modules::shared_kernel::domain::{
        BuildRunId, IdempotencyRequest, NodeId, PrincipalId, ResourceName, Sha256Digest,
    };
    use async_trait::async_trait;
    use chrono::Duration;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::Mutex;

    struct ExactPublicTargetReader {
        workload_id: crate::modules::shared_kernel::domain::WorkloadId,
        revision_id: crate::modules::shared_kernel::domain::WorkloadRevisionId,
        node_id: NodeId,
        public_port: String,
        observed_at: DateTime<Utc>,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl IRouteTargetReader for ExactPublicTargetReader {
        async fn resolve_healthy_target(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
            revision_id: crate::modules::shared_kernel::domain::WorkloadRevisionId,
            port_name: &RoutePortName,
            _now: DateTime<Utc>,
        ) -> Result<ResolvedRouteTarget, RepositoryError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if revision_id != self.revision_id || port_name.as_str() != self.public_port {
                return Err(RepositoryError::Conflict(
                    "Durable Cell route selected a non-public deployment target".into(),
                ));
            }
            Ok(ResolvedRouteTarget {
                workload_id: self.workload_id,
                node_id: self.node_id,
                target: RouteTarget::new(
                    self.workload_id,
                    self.revision_id,
                    format!(
                        "workload:{}:revision:{}",
                        self.workload_id, self.revision_id
                    ),
                    1,
                    port_name.clone(),
                    UpstreamEndpoint::parse("http://127.0.0.1:49152").expect("public upstream"),
                    self.observed_at,
                )
                .expect("healthy public target"),
            })
        }
    }

    #[derive(Default)]
    struct FailFirstGatewayQueue {
        fail_first: AtomicBool,
        calls: AtomicUsize,
        commands: Mutex<Vec<GatewayPublication>>,
    }

    impl FailFirstGatewayQueue {
        fn failing_once() -> Self {
            Self {
                fail_first: AtomicBool::new(true),
                ..Self::default()
            }
        }
    }

    #[async_trait]
    impl IGatewayCommandQueue for FailFirstGatewayQueue {
        async fn enqueue(
            &self,
            publication: &GatewayPublication,
        ) -> Result<GatewayCommandDispatch, RepositoryError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_first.swap(false, Ordering::SeqCst) {
                return Err(RepositoryError::Storage(
                    "simulated process death after Edge commit".into(),
                ));
            }
            publication.snapshot().map_err(RepositoryError::Conflict)?;
            let mut commands = self.commands.lock().await;
            let replayed = commands
                .iter()
                .any(|existing| existing.command_id == publication.command_id);
            if !replayed {
                commands.push(publication.clone());
            }
            Ok(GatewayCommandDispatch { replayed })
        }
    }

    struct Fixture {
        command: PublishDurableCellApplicationRoute,
        correlation: DurableCellDeployment,
        profile: DurableCellServiceProfile,
        deployments: Arc<InMemoryDurableCellDeploymentRepository>,
        edge: Arc<InMemoryEdgeRepository>,
        targets: Arc<ExactPublicTargetReader>,
        queue: Arc<FailFirstGatewayQueue>,
    }

    impl Fixture {
        async fn new(fail_first_dispatch: bool) -> Self {
            let now = Utc::now();
            let organization_id = OrganizationId::new();
            let project_id = ProjectId::new();
            let environment_id = EnvironmentId::new();
            let profile = service_profile();
            let correlation = deployment_correlation(
                organization_id,
                project_id,
                environment_id,
                &profile,
                now - Duration::seconds(10),
            );
            let deployments = Arc::new(InMemoryDurableCellDeploymentRepository::new());
            deployments
                .create(CreateDurableCellDeploymentWrite {
                    deployment: correlation.clone(),
                    idempotency: IdempotencyRequest::new(
                        "tests/durable-cell/public-route/deployment",
                        correlation.projection.application_revision_id.to_string(),
                        correlation
                            .projection
                            .application_revision_id
                            .as_uuid()
                            .as_bytes(),
                    )
                    .expect("deployment idempotency"),
                })
                .await
                .expect("deployment correlation");

            let edge = Arc::new(InMemoryEdgeRepository::new());
            let node_id = NodeId::new();
            let gateway_scope_id = store_gateway_scope(
                &edge,
                organization_id,
                project_id,
                environment_id,
                node_id,
                now - Duration::seconds(8),
            )
            .await;
            let domain_claim_id = store_verified_domain_claim(
                &edge,
                organization_id,
                project_id,
                environment_id,
                "cells.example.com",
                now - Duration::seconds(8),
            )
            .await;
            let targets = Arc::new(ExactPublicTargetReader {
                workload_id: correlation.projection.workload_id,
                revision_id: correlation.projection.workload_revision_id,
                node_id,
                public_port: profile.spec().public_runtime_port.clone(),
                observed_at: now - Duration::seconds(1),
                calls: AtomicUsize::new(0),
            });
            let queue = Arc::new(if fail_first_dispatch {
                FailFirstGatewayQueue::failing_once()
            } else {
                FailFirstGatewayQueue::default()
            });
            let command = PublishDurableCellApplicationRoute {
                organization_id,
                project_id,
                environment_id,
                application_id: correlation.projection.application_id,
                application_revision_id: correlation.projection.application_revision_id,
                service_profile_acl: profile.canonical_acl().into(),
                gateway_scope_id,
                domain_claim_id,
                hostname: "cells.example.com".into(),
                path_prefix: "/".into(),
                resource_access: ResourceAccessEvaluator::organization_wide(),
                idempotency_key: "publish-counter-cells".into(),
                request_id: Uuid::now_v7(),
                requested_at: now,
            };
            Self {
                command,
                correlation,
                profile,
                deployments,
                edge,
                targets,
                queue,
            }
        }

        fn handler(&self) -> PublishDurableCellApplicationRouteHandler {
            let deployments: Arc<dyn IDurableCellDeploymentRepository> = self.deployments.clone();
            let edge: Arc<dyn IEdgeRepository> = self.edge.clone();
            let targets: Arc<dyn IRouteTargetReader> = self.targets.clone();
            let queue: Arc<dyn IGatewayCommandQueue> = self.queue.clone();
            PublishDurableCellApplicationRouteHandler::new(
                deployments,
                PublishRouteHandler::new(
                    edge,
                    targets,
                    queue,
                    gateway_compiler(),
                    Duration::minutes(3),
                )
                .expect("Edge route handler"),
            )
        }
    }

    #[tokio::test]
    async fn recovers_edge_commit_before_dispatch_without_another_route_authority() {
        let fixture = Fixture::new(true).await;
        let handler = fixture.handler();

        let interrupted = handler
            .execute(fixture.command.clone(), context())
            .await
            .expect("command framework");
        assert!(interrupted.is_err());
        assert_eq!(fixture.targets.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.queue.calls.load(Ordering::SeqCst), 1);
        assert_eq!(environment_routes(&fixture).await.len(), 1);

        let recovered = handler
            .execute(fixture.command.clone(), context())
            .await
            .expect("command framework")
            .expect("recover exact Edge publication");
        assert_eq!(recovered.correlation, fixture.correlation);
        assert!(recovered.route.publication.replayed);
        assert!(!recovered.route.command_replayed);
        assert_eq!(
            recovered.route.publication.route.target.port_name.as_str(),
            fixture.profile.spec().public_runtime_port
        );
        assert_ne!(
            recovered.route.publication.route.target.port_name.as_str(),
            fixture.profile.spec().internal_runtime_port
        );
        assert_eq!(fixture.targets.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.queue.calls.load(Ordering::SeqCst), 2);

        let replay = handler
            .execute(fixture.command.clone(), context())
            .await
            .expect("command framework")
            .expect("exact replay");
        assert!(replay.route.publication.replayed);
        assert!(replay.route.command_replayed);
        assert_eq!(environment_routes(&fixture).await.len(), 1);
        assert_eq!(fixture.targets.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.queue.calls.load(Ordering::SeqCst), 3);

        let calls_before_denial = fixture.queue.calls.load(Ordering::SeqCst);
        let denied = handler
            .execute(
                PublishDurableCellApplicationRoute {
                    resource_access: ResourceAccessEvaluator::restricted([
                        ResourceGrantScope::Environment {
                            project_id: fixture.command.project_id,
                            environment_id: EnvironmentId::new(),
                        },
                    ]),
                    ..fixture.command.clone()
                },
                context(),
            )
            .await
            .expect("command framework");
        assert!(matches!(denied, Err(ApplicationError::NotFound(_))));
        assert_eq!(
            fixture.queue.calls.load(Ordering::SeqCst),
            calls_before_denial
        );
    }

    #[tokio::test]
    async fn rejects_a_changed_profile_before_resolving_or_dispatching_edge_state() {
        let fixture = Fixture::new(false).await;
        let handler = fixture.handler();
        let changed = DurableCellServiceProfile::from_spec(DurableCellServiceProfileSpec {
            max_response_bytes: fixture.profile.spec().max_response_bytes / 2,
            ..fixture.profile.spec().clone()
        })
        .expect("changed profile");
        let result = handler
            .execute(
                PublishDurableCellApplicationRoute {
                    service_profile_acl: changed.canonical_acl().into(),
                    ..fixture.command.clone()
                },
                context(),
            )
            .await
            .expect("command framework");
        assert!(matches!(result, Err(ApplicationError::Conflict(_))));
        assert_eq!(fixture.targets.calls.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.queue.calls.load(Ordering::SeqCst), 0);
        assert!(environment_routes(&fixture).await.is_empty());
    }

    async fn environment_routes(fixture: &Fixture) -> Vec<crate::modules::edge::domain::Route> {
        fixture
            .edge
            .list_routes(
                fixture.command.organization_id,
                fixture.command.project_id,
                fixture.command.environment_id,
            )
            .await
            .expect("environment routes")
    }

    async fn store_gateway_scope(
        edge: &Arc<InMemoryEdgeRepository>,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        node_id: NodeId,
        now: DateTime<Utc>,
    ) -> GatewayScopeId {
        let scope = GatewayScope::create(
            GatewayScopeId::new(),
            organization_id,
            project_id,
            environment_id,
            node_id,
            now,
        )
        .expect("Gateway scope");
        edge.create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "tests/durable-cell/public-route/gateway-scope",
                scope.id.to_string(),
                scope.id.as_uuid().as_bytes(),
            )
            .expect("scope idempotency"),
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7()).expect("scope event"),
        })
        .await
        .expect("store Gateway scope");
        scope.id
    }

    async fn store_verified_domain_claim(
        edge: &Arc<InMemoryEdgeRepository>,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        pattern: &str,
        now: DateTime<Utc>,
    ) -> DomainClaimId {
        let mut claim = DomainClaim::create(
            DomainClaimId::new(),
            organization_id,
            project_id,
            environment_id,
            DomainNamePattern::parse(pattern).expect("domain pattern"),
            format!("a3s-cloud-verification={}", Uuid::now_v7()),
            now,
        )
        .expect("domain claim");
        edge.create_domain_claim(CreateDomainClaimWrite {
            claim: claim.clone(),
            idempotency: IdempotencyRequest::new(
                "tests/durable-cell/public-route/domain-claim",
                claim.id.to_string(),
                claim.id.as_uuid().as_bytes(),
            )
            .expect("claim idempotency"),
            event: DomainClaimChanged::envelope(&claim, Uuid::now_v7()).expect("created event"),
        })
        .await
        .expect("store claim");
        let expected_version = claim.aggregate_version;
        claim
            .verify(now + Duration::milliseconds(1))
            .expect("verify claim");
        edge.transition_domain_claim(TransitionDomainClaim {
            claim: claim.clone(),
            expected_version,
            idempotency: IdempotencyRequest::new(
                "tests/durable-cell/public-route/domain-verification",
                claim.id.to_string(),
                b"verified",
            )
            .expect("verification idempotency"),
            event: DomainClaimChanged::envelope(&claim, Uuid::now_v7()).expect("verified event"),
        })
        .await
        .expect("verify stored claim");
        claim.id
    }

    fn deployment_correlation(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile: &DurableCellServiceProfile,
        now: DateTime<Utc>,
    ) -> DurableCellDeployment {
        let application_id = DurableCellApplicationId::new();
        let definition =
            DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
                build_run_id: BuildRunId::new(),
                bundle_digest: digest('a'),
                bundle_size_bytes: 4096,
                main_module: "worker.mjs".into(),
                compatibility_date: "2026-08-16".into(),
                compatibility_flags: Vec::new(),
                cell_classes: vec![DurableCellClassSpec {
                    name: "Counter".into(),
                    state_schema: DurableCellStateSchema {
                        minimum_readable_version: 1,
                        maximum_readable_version: 1,
                        write_version: 1,
                    },
                }],
                service_profile_digest: profile.digest().clone(),
                rollback_policy: DurableCellRollbackPolicy::Compatible,
            })
            .expect("application definition");
        let revision = DurableCellApplicationRevision::initial(
            organization_id,
            project_id,
            environment_id,
            application_id,
            DurableCellApplicationRevisionId::new(),
            definition,
            PrincipalId::new(),
            now,
        )
        .expect("application revision");
        let application = DurableCellApplication::create(
            application_id,
            ResourceName::parse("Counter cells").expect("application name"),
            &revision,
        )
        .expect("application");
        let projection =
            DurableCellProjectionIdentity::for_current_revision(&application, &revision)
                .expect("projection");
        let provider = DurableCellProviderBinding {
            application_id,
            application_revision_id: revision.id,
            application_revision_number: revision.revision_number,
            application_definition_digest: revision.definition.digest().clone(),
            workload_id: projection.workload_id,
            workload_revision_id: projection.workload_revision_id,
            workload_generation: 1,
            service_profile_digest: profile.digest().clone(),
            service_template_digest: digest('b'),
            provider_artifact_digest: digest('c'),
        };
        let storage = DurableCellStorageBinding {
            organization_id,
            project_id,
            environment_id,
            application_id,
            application_revision_id: revision.id,
            application_revision_number: revision.revision_number,
            application_definition_digest: revision.definition.digest().clone(),
            storage_namespace_id: projection.storage_namespace_id,
            credential_binding_generation: 1,
            credential_binding_digest: digest('d'),
            provider_profile_digest: digest('e'),
            retention_policy_digest: digest('f'),
        };
        DurableCellDeployment::bind(
            projection,
            storage,
            provider,
            digest('1'),
            PrincipalId::new(),
            Uuid::now_v7(),
            now,
        )
        .expect("deployment correlation")
    }

    fn service_profile() -> DurableCellServiceProfile {
        DurableCellServiceProfile::from_spec(DurableCellServiceProfileSpec {
            public_runtime_port: "cell-public".into(),
            internal_runtime_port: "cell-internal".into(),
            health_path: "/__celld/health".into(),
            max_cell_name_bytes: 512,
            max_request_bytes: 16 * 1024 * 1024,
            max_response_bytes: 64 * 1024 * 1024,
            max_websocket_message_bytes: 1024 * 1024,
        })
        .expect("Service profile")
    }

    fn gateway_compiler() -> GatewaySnapshotCompiler {
        GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            upstream_request_timeout_ms: 30_000,
            certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
            managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
        })
        .expect("Gateway compiler")
    }

    fn context() -> CqrsContext {
        CqrsContext::new(a3s_boot::ModuleRef::new())
    }

    fn digest(marker: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
    }
}
