use super::*;
use crate::modules::edge::domain::repositories::GatewayCertificateConvergenceResult;
use crate::modules::edge::domain::GatewayScopeState;
use crate::modules::edge::infrastructure::{
    GatewayNodeDesiredStatePlanner, GatewaySnapshotRouteInput, IMcpGatewayNodeProjectionPlanner,
    IMcpGatewaySnapshotRepository, McpGatewayProjectionAssembler, McpGatewayReconciliationScope,
    McpGatewaySnapshotDispatchTarget, McpGatewaySnapshotInputs,
    McpGatewaySnapshotReconciliationState, McpGatewaySnapshotStageResult,
    PlanMcpGatewayNodeProjection, PlannedMcpGatewayNodeProjection, PlannedMcpGatewayProjectionSet,
    StageManagedGatewayCertificateConvergence, StageMcpGatewaySnapshot,
};
use crate::modules::identity::application::IInferenceCredentialAclProjectionPort;
use crate::modules::inference::application::{
    EmptyInferenceWorkerAclProjectionPort, IInferenceRouteAclProjectionPort,
};
use crate::modules::shared_kernel::domain::{GatewayScopeId, NodeCommandId, OrganizationId};
use a3s_cloud_contracts::{
    InferenceCredentialAclProjection, InferenceEndpointAcl, InferenceGrantAclProjection,
    InferenceLimitsAclProjection, InferenceModelAclProjection, InferenceRouteAclProjection,
    InferenceTargetAclProjection, INFERENCE_CREDENTIAL_AUDIENCE,
};
use chrono::{DateTime, Duration as ChronoDuration};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex as StdMutex;
use std::time::Duration as StdDuration;

struct RecordingManagedSnapshotRepository {
    logical_scopes: Vec<GatewayScope>,
    inputs: McpGatewaySnapshotInputs,
    staged_publication_acls: StdMutex<Vec<String>>,
    stage_calls: AtomicUsize,
}

impl RecordingManagedSnapshotRepository {
    fn new(logical_scope: GatewayScope, inputs: McpGatewaySnapshotInputs) -> Self {
        Self {
            logical_scopes: vec![logical_scope],
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
        after_gateway_scope_id: Option<GatewayScopeId>,
        limit: usize,
    ) -> Result<Vec<McpGatewayReconciliationScope>, RepositoryError> {
        Ok(self
            .logical_scopes
            .iter()
            .filter(|scope| after_gateway_scope_id.is_none_or(|cursor| scope.id > cursor))
            .take(limit)
            .cloned()
            .map(|scope| McpGatewayReconciliationScope {
                node_ids: scope.member_node_ids.clone(),
                scope,
            })
            .collect())
    }

    async fn mcp_gateway_reconciliation_scope_set(
        &self,
        node_id: NodeId,
        _observed_at: DateTime<Utc>,
    ) -> Result<Vec<GatewayScope>, RepositoryError> {
        let mut scopes = self
            .logical_scopes
            .iter()
            .filter(|scope| scope.contains_member(node_id))
            .cloned()
            .collect::<Vec<_>>();
        scopes.sort_by_key(|scope| scope.id);
        Ok(scopes)
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
            "MCP snapshot staging is unused by managed certificate convergence tests".into(),
        ))
    }

    async fn stage_managed_gateway_certificate_convergence(
        &self,
        stage: StageManagedGatewayCertificateConvergence,
    ) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
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
        Ok(GatewayCertificateConvergenceResult {
            convergence: stage.ordinary().convergence.clone(),
            certificate: stage.ordinary().certificate.clone(),
            publication,
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
        _scopes: &[crate::modules::identity::application::InferenceCredentialEnvironmentScope],
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
        _scopes: &[crate::modules::inference::application::InferenceRouteEnvironmentScope],
    ) -> Result<Vec<InferenceRouteAclProjection>, RepositoryError> {
        Ok(vec![self.projection.clone()])
    }
}

fn managed_reconciler(
    fixture: &Fixture,
    managed: Arc<RecordingManagedSnapshotRepository>,
    queue: Arc<RecordingGatewayQueue>,
    authority: Arc<RecordingGatewayCertificateAuthority>,
    inference_credentials: Arc<dyn IInferenceCredentialAclProjectionPort>,
    inference_routes: Arc<dyn IInferenceRouteAclProjectionPort>,
) -> GatewayCertificateReconciler {
    let repository: Arc<dyn IEdgeRepository> = fixture.repository.clone();
    let managed_repository: Arc<dyn IMcpGatewaySnapshotRepository> = managed;
    let desired_state = GatewayNodeDesiredStatePlanner::new(
        managed_repository.clone(),
        Arc::new(EmptyProjectionPlanner),
    );
    GatewayCertificateReconciler::new_managed(
        repository,
        managed_repository,
        desired_state,
        inference_credentials,
        inference_routes,
        Arc::new(EmptyInferenceWorkerAclProjectionPort),
        queue,
        authority,
        fixture.compiler.clone(),
        StdDuration::from_secs(60),
        Duration::days(7),
        Duration::hours(6),
        Duration::minutes(3),
        100,
    )
    .expect("managed certificate reconciler")
}

#[tokio::test]
async fn managed_certificate_convergence_stages_inference_credential_and_route_acl_without_workers()
{
    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const PREFIX: &str = "a3s_inf_certstage01";
    let credential_id = Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap();
    let inference_route_id = Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap();

    let fixture = Fixture::new();
    let base = Utc::now();
    let claim = fixture.verified_claim("cert-inference.example.com", base).await;
    let (route, _certificate) = fixture
        .activate_route(
            &claim,
            "cert-inference.example.com",
            base + Duration::seconds(1),
            base + Duration::days(30),
        )
        .await;

    let logical_scope = fixture
        .repository
        .find_gateway_scope(fixture.organization_id, fixture.gateway_scope_id)
        .await
        .expect("logical Gateway scope");
    let physical_scope = fixture
        .repository
        .gateway_scope(fixture.node_id)
        .await
        .expect("physical Gateway scope");
    assert_eq!(
        physical_scope,
        GatewayScopeState {
            node_id: fixture.node_id,
            last_issued_revision: route.gateway_revision.expect("installed revision"),
            installed_revision: route.gateway_revision,
            aggregate_version: physical_scope.aggregate_version,
        }
    );

    let managed = Arc::new(RecordingManagedSnapshotRepository::new(
        logical_scope,
        McpGatewaySnapshotInputs {
            physical_scope,
            active_routes: vec![GatewaySnapshotRouteInput {
                route: route.clone(),
                domain_claim: claim,
            }],
        },
    ));

    let credential = InferenceCredentialAclProjection::new(
        credential_id,
        fixture.environment_id.as_uuid(),
        INFERENCE_CREDENTIAL_AUDIENCE,
        PREFIX,
        VERIFIER,
        3,
        base + ChronoDuration::hours(2),
        false,
    )
    .expect("credential projection");
    let inference_route = InferenceRouteAclProjection {
        route_id: inference_route_id,
        router: "inference".into(),
        environment_id: fixture.environment_id.as_uuid(),
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

    let queue = Arc::new(RecordingGatewayQueue::default());
    let authority = Arc::new(RecordingGatewayCertificateAuthority::default());
    let reconciler = managed_reconciler(
        &fixture,
        managed.clone(),
        queue,
        authority,
        Arc::new(StubCredentialPort {
            projection: credential,
        }),
        Arc::new(StubRoutePort {
            projection: inference_route,
        }),
    );
    let run_at = base + Duration::hours(18) + Duration::seconds(2);

    let report = reconciler
        .run_once(run_at)
        .await
        .expect("managed certificate convergence with inference ACL");
    assert_eq!(report.staged_convergences, 1);
    assert!(
        report.failures.is_empty(),
        "failures: {:?}",
        report.failures
    );
    assert_eq!(managed.stage_calls.load(Ordering::SeqCst), 1);

    let staged = managed.staged_publication_acls();
    assert_eq!(staged.len(), 1);
    let acl = &staged[0];
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
        !acl.contains("bearer") && !acl.contains("a3s_inf_certstage01."),
        "staged ACL must not embed bearer secret material"
    );
}
