use super::gateway_certificate_reconciler_tests::Fixture;
use super::{
    GatewayNodeDesiredStatePlanner, GatewayRolloutRollbackCompiler,
    GatewayRolloutRollbackReconciler, GatewaySnapshotRouteInput, IMcpGatewayNodeProjectionPlanner,
    IMcpGatewaySnapshotRepository, McpGatewayProjectionAssembler, McpGatewayReconciliationScope,
    McpGatewaySnapshotDispatchTarget, McpGatewaySnapshotInputs,
    McpGatewaySnapshotReconciliationState, McpGatewaySnapshotStageResult,
    PlanMcpGatewayNodeProjection, PlannedMcpGatewayNodeProjection, PlannedMcpGatewayProjectionSet,
    StageManagedGatewayRolloutRollback, StageMcpGatewaySnapshot,
};
use crate::modules::edge::domain::events::GatewayRolloutStaged;
use crate::modules::edge::domain::repositories::{
    GatewayRolloutRollbackResult, IEdgeRepository, StageGatewayRollout,
};
use crate::modules::edge::domain::{
    GatewayPublication, GatewayRollout, GatewayRolloutRollbackState, GatewayRolloutState,
    GatewayScope, GatewayScopeState,
};
use crate::modules::identity::application::{
    IInferenceCredentialAclProjectionPort, InferenceCredentialEnvironmentScope,
};
use crate::modules::inference::application::{
    EmptyInferenceWorkerAclProjectionPort, IInferenceRouteAclProjectionPort,
    InferenceRouteEnvironmentScope,
};
use crate::modules::shared_kernel::domain::{
    GatewayRolloutId, GatewayScopeId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId,
    RepositoryError,
};
use a3s_cloud_contracts::{
    GatewayAckState, GatewayManagementProtocol, GatewaySnapshot, InferenceCredentialAclProjection,
    InferenceEndpointAcl, InferenceGrantAclProjection, InferenceLimitsAclProjection,
    InferenceModelAclProjection, InferenceRouteAclProjection, InferenceTargetAclProjection,
    NodeGatewayAck, INFERENCE_CREDENTIAL_AUDIENCE,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;
use uuid::Uuid;

struct RecordingManagedSnapshotRepository {
    logical_scopes: Vec<GatewayScope>,
    inputs: McpGatewaySnapshotInputs,
    staged_publication_acls: Mutex<Vec<String>>,
    stage_calls: AtomicUsize,
    reused_certificates: AtomicUsize,
    replacement_certificates: AtomicUsize,
}

impl RecordingManagedSnapshotRepository {
    fn new(logical_scope: GatewayScope, inputs: McpGatewaySnapshotInputs) -> Self {
        Self {
            logical_scopes: vec![logical_scope],
            inputs,
            staged_publication_acls: Mutex::new(Vec::new()),
            stage_calls: AtomicUsize::new(0),
            reused_certificates: AtomicUsize::new(0),
            replacement_certificates: AtomicUsize::new(0),
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
            "MCP snapshot staging is unused by managed rollout rollback tests".into(),
        ))
    }

    async fn stage_managed_gateway_rollout_rollback(
        &self,
        stage: StageManagedGatewayRolloutRollback,
    ) -> Result<GatewayRolloutRollbackResult, RepositoryError> {
        let ordinary = stage.ordinary();
        ordinary.validate().map_err(RepositoryError::Conflict)?;
        self.stage_calls.fetch_add(1, Ordering::SeqCst);
        self.reused_certificates
            .fetch_add(ordinary.reused_certificates.len(), Ordering::SeqCst);
        self.replacement_certificates
            .fetch_add(ordinary.certificates.len(), Ordering::SeqCst);
        self.staged_publication_acls
            .lock()
            .expect("staged publication ACLs")
            .extend(
                ordinary
                    .publications
                    .iter()
                    .map(|publication| publication.acl.clone()),
            );
        Ok(GatewayRolloutRollbackResult {
            rollback: ordinary.rollback.clone(),
            rollout: ordinary.rollout.clone(),
            publications: ordinary.publications.clone(),
            certificates: ordinary.certificates.clone(),
            reused_certificates: ordinary.reused_certificates.clone(),
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

/// Stages a bare complete-snapshot rollout on the fixture node and rejects it so the
/// repository records exactly one `Required` exact rollback for the logical scope.
async fn reject_bare_rollout(
    fixture: &Fixture,
    physical_scope: &GatewayScopeState,
    now: DateTime<Utc>,
) -> GatewayRollout {
    let logical_scope = fixture
        .repository
        .find_gateway_scope(fixture.organization_id, fixture.gateway_scope_id)
        .await
        .expect("logical Gateway scope");
    let revision = physical_scope.next_revision().expect("next revision");
    let snapshot = GatewaySnapshot::new(
        fixture.node_id.as_uuid(),
        revision,
        physical_scope.installed_revision,
        now,
        now + Duration::hours(1),
        format!("# failed rollout snapshot for {}", fixture.node_id),
    )
    .expect("failed rollout snapshot");
    let publication = GatewayPublication::stage(
        fixture.node_id,
        NodeCommandId::new(),
        Uuid::now_v7(),
        snapshot,
        now,
        now + Duration::minutes(3),
    )
    .expect("failed rollout publication");
    let rollout = GatewayRollout::stage(
        GatewayRolloutId::new(),
        &logical_scope,
        1,
        std::slice::from_ref(&publication),
        now,
    )
    .expect("failed rollout aggregate");
    fixture
        .repository
        .stage_gateway_rollout(StageGatewayRollout {
            scope: logical_scope.clone(),
            rollout: rollout.clone(),
            route_replicas: Vec::new(),
            publications: vec![publication.clone()],
            certificates: Vec::new(),
            expected_scope_versions: BTreeMap::from([(
                fixture.node_id,
                physical_scope.aggregate_version,
            )]),
            idempotency: IdempotencyRequest::new(
                format!("gateway-scopes/{}/rollouts", logical_scope.id),
                "managed-rollback-failed",
                rollout.id.to_string().as_bytes(),
            )
            .expect("rollout idempotency"),
            event: GatewayRolloutStaged::envelope(&logical_scope, &rollout).expect("rollout event"),
            route_event: None,
        })
        .await
        .expect("stage failed rollout");
    let rejected_at = now + Duration::seconds(1);
    fixture
        .repository
        .project_gateway_acknowledgement(
            &NodeGatewayAck {
                schema: NodeGatewayAck::SCHEMA.into(),
                acknowledgement_id: Uuid::now_v7(),
                command_id: publication.command_id.as_uuid(),
                node_id: fixture.node_id.as_uuid(),
                gateway_id: fixture.node_id.as_uuid(),
                revision: publication.revision,
                snapshot_digest: publication.snapshot_digest.clone(),
                expires_at: publication.snapshot_expires_at,
                state: GatewayAckState::Rejected,
                ready: false,
                message: Some("snapshot rejected".into()),
                acknowledged_at: rejected_at,
                management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
            },
            rejected_at,
        )
        .await
        .expect("reject failed rollout");
    fixture
        .repository
        .find_gateway_rollout(fixture.organization_id, rollout.id)
        .await
        .expect("degraded rollout")
}

fn rollback_compiler(fixture: &Fixture) -> GatewayRolloutRollbackCompiler {
    GatewayRolloutRollbackCompiler::new(
        fixture.compiler.clone(),
        Duration::minutes(3),
        Duration::hours(24),
    )
    .expect("rollback compiler")
}

#[tokio::test]
async fn managed_rollout_rollback_stages_inference_credential_and_route_acl_without_workers() {
    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const PREFIX: &str = "a3s_inf_rbckstage01";
    let credential_id =
        Uuid::parse_str("33333333-3333-4333-8333-333333333333").expect("credential id");
    let inference_route_id =
        Uuid::parse_str("44444444-4444-4444-8444-444444444444").expect("inference route id");

    let fixture = Fixture::new();
    let base = Utc::now();
    let claim = fixture
        .verified_claim("rollback-inference.example.com", base)
        .await;
    let (route, _certificate) = fixture
        .activate_route(
            &claim,
            "rollback-inference.example.com",
            base + Duration::seconds(1),
            base + Duration::days(30),
        )
        .await;
    let retained_scope = fixture
        .repository
        .gateway_scope(fixture.node_id)
        .await
        .expect("retained physical Gateway scope");
    assert_eq!(retained_scope.installed_revision, route.gateway_revision);

    let failed = reject_bare_rollout(&fixture, &retained_scope, base + Duration::seconds(10)).await;
    assert_eq!(failed.state, GatewayRolloutState::Degraded);
    assert!(!failed.serves_traffic().expect("failed readiness"));
    let pending = fixture
        .repository
        .pending_gateway_rollout_rollbacks(10)
        .await
        .expect("pending rollbacks");
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].rollback.state,
        GatewayRolloutRollbackState::Required
    );

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
            last_issued_revision: failed.replicas[0].revision,
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
        base + Duration::hours(2),
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
            model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").expect("model id"),
            targets: vec![InferenceTargetAclProjection {
                target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666")
                    .expect("target id"),
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

    let repository: Arc<dyn IEdgeRepository> = fixture.repository.clone();
    let managed_repository: Arc<dyn IMcpGatewaySnapshotRepository> = managed.clone();
    let desired_state = GatewayNodeDesiredStatePlanner::new(
        managed_repository.clone(),
        Arc::new(EmptyProjectionPlanner),
    );
    let reconciler = GatewayRolloutRollbackReconciler::new_managed(
        repository,
        managed_repository,
        desired_state,
        Arc::new(StubCredentialPort {
            projection: credential,
        }),
        Arc::new(StubRoutePort {
            projection: inference_route,
        }),
        Arc::new(EmptyInferenceWorkerAclProjectionPort),
        rollback_compiler(&fixture),
        StdDuration::from_secs(60),
        10,
    )
    .expect("managed rollback reconciler");
    let run_at = base + Duration::minutes(1);

    let report = reconciler
        .run_once(run_at)
        .await
        .expect("managed rollout rollback with inference ACL");
    assert_eq!(report.required_rollbacks, 1);
    assert_eq!(report.staged_rollbacks, 1);
    assert_eq!(report.replayed_rollbacks, 0);
    assert_eq!(report.superseded_rollbacks, 0);
    assert!(
        report.failures.is_empty(),
        "failures: {:?}",
        report.failures
    );
    assert_eq!(managed.stage_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        managed.reused_certificates.load(Ordering::SeqCst),
        1,
        "rollback must reuse the retained Ready certificate"
    );
    assert_eq!(
        managed.replacement_certificates.load(Ordering::SeqCst),
        0,
        "rollback must not provision a replacement certificate"
    );

    let staged = managed.staged_publication_acls();
    assert_eq!(
        staged.len(),
        1,
        "single physical member stages one publication"
    );
    for acl in &staged {
        assert!(
            acl.contains("inference {"),
            "staged rollback ACL must embed inference policy shell"
        );
        assert!(
            acl.contains(&format!("prefix = \"{PREFIX}\"")),
            "staged rollback ACL must embed Identity credential prefix"
        );
        assert!(
            acl.contains(&format!("routes \"{inference_route_id}\"")),
            "staged rollback ACL must embed Inference route grants"
        );
        assert!(
            acl.contains("models \"chat-model\""),
            "staged rollback ACL must embed model grant aliases"
        );
        assert!(
            !acl.contains("\n  workers ") && !acl.contains("\nworkers "),
            "empty Power worker port must not invent workers blocks"
        );
        assert!(
            !acl.contains("bearer") && !acl.contains("a3s_inf_rbckstage01."),
            "staged rollback ACL must not embed bearer secret material"
        );
    }
}
