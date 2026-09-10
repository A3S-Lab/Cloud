use super::*;
use crate::modules::edge::domain::repositories::GatewayRouteCutoverResult;
use crate::modules::edge::domain::{DomainClaim, DomainNamePattern, GatewayScopeState};
use crate::modules::edge::infrastructure::{
    GatewayNodeDesiredStatePlanner, GatewaySnapshotRouteInput, IMcpGatewayNodeProjectionPlanner,
    IMcpGatewaySnapshotRepository, McpGatewayProjectionAssembler, McpGatewayReconciliationScope,
    McpGatewaySnapshotDispatchTarget, McpGatewaySnapshotInputs,
    McpGatewaySnapshotReconciliationState, McpGatewaySnapshotStageResult,
    PlanMcpGatewayNodeProjection, PlannedMcpGatewayNodeProjection, PlannedMcpGatewayProjectionSet,
    StageManagedGatewayRouteCutover, StageMcpGatewaySnapshot,
};
use crate::modules::identity::application::{
    IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope,
};
use crate::modules::inference::application::{
    EmptyInferenceWorkerAclProjectionPort, IInferenceRouteAclProjectionPort,
    InferenceRouteEnvironmentScope,
};
use a3s_cloud_contracts::{
    InferenceCredentialAclProjection, InferenceEndpointAcl, InferenceGrantAclProjection,
    InferenceLimitsAclProjection, InferenceModelAclProjection, InferenceRouteAclProjection,
    InferenceTargetAclProjection, INFERENCE_CREDENTIAL_AUDIENCE,
};
use chrono::DateTime;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex as StdMutex;

struct RecordingManagedSnapshotRepository {
    logical_scope: GatewayScope,
    inputs: McpGatewaySnapshotInputs,
    staged_publication_acls: StdMutex<Vec<String>>,
    stage_calls: AtomicUsize,
}

impl RecordingManagedSnapshotRepository {
    fn new(logical_scope: GatewayScope, inputs: McpGatewaySnapshotInputs) -> Self {
        Self {
            logical_scope,
            inputs,
            staged_publication_acls: StdMutex::new(Vec::new()),
            stage_calls: AtomicUsize::new(0),
        }
    }

    fn staged_publication_acls(&self) -> Vec<String> {
        self.staged_publication_acls
            .lock()
            .expect("staged publication ACLs")
            .clone()
    }
}

#[async_trait]
impl IMcpGatewaySnapshotRepository for RecordingManagedSnapshotRepository {
    async fn mcp_gateway_reconciliation_scopes(
        &self,
        _observed_at: DateTime<Utc>,
        _after_gateway_scope_id: Option<GatewayScopeId>,
        _limit: usize,
    ) -> Result<Vec<McpGatewayReconciliationScope>, RepositoryError> {
        Ok(vec![McpGatewayReconciliationScope {
            node_ids: self.logical_scope.member_node_ids.clone(),
            scope: self.logical_scope.clone(),
        }])
    }

    async fn mcp_gateway_reconciliation_scope_set(
        &self,
        node_id: NodeId,
        _observed_at: DateTime<Utc>,
    ) -> Result<Vec<GatewayScope>, RepositoryError> {
        if self.logical_scope.contains_member(node_id) {
            Ok(vec![self.logical_scope.clone()])
        } else {
            Ok(Vec::new())
        }
    }

    async fn mcp_gateway_snapshot_reconciliation_state(
        &self,
        _node_id: NodeId,
    ) -> Result<McpGatewaySnapshotReconciliationState, RepositoryError> {
        Ok(McpGatewaySnapshotReconciliationState {
            pending_publication: false,
            latest_mcp_snapshot: None,
        })
    }

    async fn mcp_gateway_snapshot_inputs(
        &self,
        _node_id: NodeId,
    ) -> Result<McpGatewaySnapshotInputs, RepositoryError> {
        Ok(self.inputs.clone())
    }

    async fn stage_mcp_gateway_snapshot(
        &self,
        _stage: StageMcpGatewaySnapshot,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
        Err(RepositoryError::Storage(
            "MCP snapshot staging is unused by managed route-cutover inference ACL tests".into(),
        ))
    }

    async fn stage_managed_gateway_route_cutover(
        &self,
        stage: StageManagedGatewayRouteCutover,
    ) -> Result<GatewayRouteCutoverResult, RepositoryError> {
        stage
            .ordinary()
            .validate()
            .map_err(RepositoryError::Conflict)?;
        self.stage_calls.fetch_add(1, Ordering::SeqCst);
        let publication = stage.ordinary().publication.clone();
        self.staged_publication_acls
            .lock()
            .expect("staged publication ACLs")
            .push(publication.acl.clone());
        Ok(GatewayRouteCutoverResult {
            cutover: stage.ordinary().cutover.clone(),
            certificate: stage.ordinary().certificate.clone(),
            publication,
            replayed: false,
        })
    }

    async fn pending_mcp_gateway_snapshots(
        &self,
        _limit: usize,
    ) -> Result<Vec<McpGatewaySnapshotDispatchTarget>, RepositoryError> {
        Ok(Vec::new())
    }

    async fn mark_mcp_gateway_snapshot_unavailable(
        &self,
        _organization_id: OrganizationId,
        _gateway_scope_id: GatewayScopeId,
        _node_id: NodeId,
        _gateway_revision: u64,
        _gateway_command_id: NodeCommandId,
        _failure: &str,
        _observed_at: DateTime<Utc>,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
        Err(RepositoryError::NotFound)
    }
}

#[derive(Default)]
struct EmptyProjectionPlanner;

#[async_trait]
impl IMcpGatewayNodeProjectionPlanner for EmptyProjectionPlanner {
    async fn plan(
        &self,
        request: PlanMcpGatewayNodeProjection,
    ) -> Result<PlannedMcpGatewayNodeProjection, RepositoryError> {
        let planned = request
            .scopes
            .into_iter()
            .map(|scope| {
                if scope.contains_member(request.gateway_node_id) {
                    PlannedMcpGatewayProjectionSet::empty(
                        scope,
                        request.gateway_node_id,
                        request.observed_at,
                    )
                } else {
                    PlannedMcpGatewayProjectionSet::empty_for_departed_member(
                        scope,
                        request.gateway_node_id,
                        request.observed_at,
                    )
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(RepositoryError::Conflict)?;
        PlannedMcpGatewayNodeProjection::aggregate(planned, McpGatewayProjectionAssembler)
            .map_err(RepositoryError::Conflict)
    }
}

struct StubCredentialPort {
    projection: InferenceCredentialAclProjection,
}

#[async_trait]
impl IInferenceCredentialAclProjectionPort for StubCredentialPort {
    async fn list_inference_credential_acl_projections(
        &self,
        _scopes: &[InferenceCredentialEnvironmentScope],
    ) -> Result<Vec<InferenceCredentialAclProjection>, RepositoryError> {
        Ok(vec![self.projection.clone()])
    }
}

struct StubRoutePort {
    projection: InferenceRouteAclProjection,
}

#[async_trait]
impl IInferenceRouteAclProjectionPort for StubRoutePort {
    async fn list_inference_route_acl_projections(
        &self,
        _scopes: &[InferenceRouteEnvironmentScope],
    ) -> Result<Vec<InferenceRouteAclProjection>, RepositoryError> {
        Ok(vec![self.projection.clone()])
    }
}

#[tokio::test]
async fn managed_route_cutover_stages_inference_credential_and_route_acl_without_workers(
) -> Result<(), Box<dyn std::error::Error>> {
    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const PREFIX: &str = "a3s_inf_cutstage01";
    let credential_id = Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap();
    let inference_route_id = Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap();

    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let routes = Arc::new(InMemoryEdgeRepository::new());
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&nodes, organization_id, base).await?;
    let compiler = gateway_compiler()?;
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("managed cutover inference ACL")?,
        base,
    );
    let first = deployment_bundle(workload.clone(), 1, 'a', base, "cutover-inf-acl-first")?;
    let initial_route = publish_active_route(
        &routes,
        &compiler,
        &workload,
        first.revision.id,
        node_id,
        Utc::now(),
    )
    .await?;
    let candidate = deployment_bundle(
        workload.clone(),
        2,
        'b',
        Utc::now(),
        "cutover-inf-acl-candidate",
    )?;
    let candidate_spec = project_runtime_spec(&candidate.revision)?;
    let runtime_command_id = NodeCommandId::new();
    nodes
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: runtime_command_id,
            node_id,
            aggregate_id: candidate.deployment.id.as_uuid(),
            payload: NodeCommandPayload::RuntimeInspect {
                unit_id: candidate_spec.unit_id.clone(),
                generation: candidate_spec.generation,
            },
            issued_at: Utc::now(),
            not_after: Utc::now() + Duration::minutes(1),
            correlation_id: candidate.operation.id.as_uuid(),
        })
        .await?;
    let leased = lease(&nodes, node_id, agent_instance_id, 0).await?;
    let command = leased
        .commands
        .iter()
        .find(|command| command.command_id == runtime_command_id.as_uuid())
        .ok_or("RuntimeInspect command was not leased")?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        command,
        healthy_observation(&candidate_spec, RuntimeHealthState::Healthy)?,
    )
    .await?;

    let logical_scope = routes
        .find_gateway_scope(organization_id, initial_route.gateway_scope_id)
        .await?;
    let physical_scope = routes.gateway_scope(node_id).await?;
    assert_eq!(
        physical_scope,
        GatewayScopeState {
            node_id,
            last_issued_revision: initial_route.gateway_revision.expect("installed revision"),
            installed_revision: initial_route.gateway_revision,
            aggregate_version: physical_scope.aggregate_version,
        }
    );

    let claim_id = initial_route.domain_claim_id.expect("route domain claim");
    let mut domain_claim = DomainClaim::create(
        claim_id,
        organization_id,
        workload.project_id,
        workload.environment_id,
        DomainNamePattern::parse("update.example.com")?,
        format!("a3s-cloud-verification={claim_id}"),
        base,
    )?;
    domain_claim.verify(base + Duration::milliseconds(1))?;

    let managed = Arc::new(RecordingManagedSnapshotRepository::new(
        logical_scope,
        McpGatewaySnapshotInputs {
            physical_scope,
            active_routes: vec![GatewaySnapshotRouteInput {
                route: initial_route.clone(),
                domain_claim,
            }],
        },
    ));
    let managed_repo: Arc<dyn IMcpGatewaySnapshotRepository> = managed.clone();
    let desired_state =
        GatewayNodeDesiredStatePlanner::new(managed_repo.clone(), Arc::new(EmptyProjectionPlanner));

    let credential = InferenceCredentialAclProjection::new(
        credential_id,
        workload.environment_id.as_uuid(),
        INFERENCE_CREDENTIAL_AUDIENCE,
        PREFIX,
        VERIFIER,
        3,
        Utc::now() + Duration::hours(2),
        false,
    )?;
    let inference_route = InferenceRouteAclProjection {
        route_id: inference_route_id,
        router: "inference".into(),
        environment_id: workload.environment_id.as_uuid(),
        policy_revision: 11,
        models: vec![InferenceModelAclProjection {
            alias: "chat-model".into(),
            model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
            targets: vec![InferenceTargetAclProjection {
                target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
                service: "model-service".into(),
                upstream_model: "internal/model-v1".into(),
                priority: 0,
                weight: 100,
            }],
        }],
        grants: vec![InferenceGrantAclProjection {
            credential_id,
            credential_generation: 3,
            models: vec!["chat-model".into()],
            endpoints: vec![InferenceEndpointAcl::Models],
            limits: InferenceLimitsAclProjection {
                max_concurrent_requests: 2,
                requests_per_minute: 60,
                request_burst: 2,
                tokens_per_minute: 10_000,
            },
        }],
    };

    let route_port: Arc<dyn IEdgeRepository> = routes.clone();
    let control_port: Arc<dyn INodeControlRepository> = nodes.clone();
    let gateway_commands: Arc<dyn crate::modules::edge::domain::services::IGatewayCommandQueue> =
        Arc::new(FleetGatewayCommandQueue::new(Arc::clone(&control_port)));
    let updater = EdgeDeploymentRouteUpdater::new_managed(
        route_port,
        managed_repo,
        control_port,
        gateway_commands,
        compiler,
        desired_state,
        Arc::new(StubCredentialPort {
            projection: credential,
        }),
        Arc::new(StubRoutePort {
            projection: inference_route,
        }),
        Arc::new(EmptyInferenceWorkerAclProjectionPort),
        Duration::seconds(5),
    )?;

    let now = Utc::now() + Duration::milliseconds(1);
    let request = DeploymentRouteUpdateRequest {
        deployment_id: candidate.deployment.id,
        operation_id: candidate.operation.id,
        organization_id,
        project_id: workload.project_id,
        environment_id: workload.environment_id,
        workload_id: workload.id,
        previous_revision_id: first.revision.id,
        candidate_revision_id: candidate.revision.id,
        node_id,
        runtime_command_id,
        spec: candidate_spec,
        verified_at: now,
        convergence_deadline: now + Duration::seconds(5),
    };
    let staged = updater.stage(&request, now).await?;
    assert!(
        matches!(staged, DeploymentRouteStage::Staged { .. }),
        "expected staged cutover, got {staged:?}"
    );
    assert_eq!(managed.stage_calls.load(Ordering::SeqCst), 1);

    let acls = managed.staged_publication_acls();
    assert_eq!(acls.len(), 1);
    let acl = &acls[0];
    assert!(
        acl.contains("inference {"),
        "staged ACL must embed inference policy shell"
    );
    assert!(
        acl.contains(&format!("prefix = \"{PREFIX}\"")),
        "staged ACL must embed Identity credential prefix"
    );
    assert!(
        acl.contains(&format!("routes \"{inference_route_id}\"")),
        "staged ACL must embed Inference route grants"
    );
    assert!(
        acl.contains("models \"chat-model\""),
        "staged ACL must embed model grant aliases"
    );
    assert!(
        !acl.contains("\n  workers ") && !acl.contains("\nworkers "),
        "empty Power worker port must not invent workers blocks"
    );
    assert!(
        !acl.contains("bearer") && !acl.contains("a3s_inf_cutstage01."),
        "staged ACL must not embed bearer secret material"
    );
    Ok(())
}
