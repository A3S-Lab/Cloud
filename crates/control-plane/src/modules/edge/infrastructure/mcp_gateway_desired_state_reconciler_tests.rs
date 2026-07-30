use super::mcp_gateway_desired_state_reconciler::{
    reconciliation_decision, ReconciliationDecision,
};
use super::{
    CompileMcpGatewaySnapshot, GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
    GatewaySnapshotMetadata, IMcpGatewayProjectionSetPlanner, IMcpGatewaySnapshotRepository,
    McpGatewayDesiredStateReconciler, McpGatewayNodeProjectionAssembler,
    McpGatewayReconciliationScope, McpGatewaySnapshotAnchor, McpGatewaySnapshotDispatchTarget,
    McpGatewaySnapshotInputs, McpGatewaySnapshotReconciliationState, McpGatewaySnapshotStageResult,
    McpGatewaySnapshotStatus, PlanMcpGatewayProjectionSet, PlannedMcpGatewayNodeProjection,
    PlannedMcpGatewayProjectionSet, StageMcpGatewaySnapshot,
};
use crate::modules::edge::domain::{
    GatewayPublication, GatewayPublicationState, GatewayScope, GatewayScopeState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GatewayScopeId, NodeCommandId, NodeId, OrganizationId,
    ProjectId, RepositoryError, Sha256Digest,
};
use a3s_cloud_contracts::{GatewayCertificateRequest, GatewaySnapshot};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

struct FakeDesiredStateRepository {
    scopes: Vec<GatewayScope>,
    active_scopes: Vec<GatewayScope>,
    state: McpGatewaySnapshotReconciliationState,
    inputs: McpGatewaySnapshotInputs,
    state_reads: AtomicUsize,
    scan_scope_ids: Mutex<Vec<GatewayScopeId>>,
    input_reads: AtomicUsize,
    staged: Mutex<Vec<StageMcpGatewaySnapshot>>,
}

impl FakeDesiredStateRepository {
    fn new(
        scope: GatewayScope,
        state: McpGatewaySnapshotReconciliationState,
        physical_scope: GatewayScopeState,
    ) -> Self {
        Self::with_scopes(vec![scope], state, physical_scope)
    }

    fn with_scopes(
        scopes: Vec<GatewayScope>,
        state: McpGatewaySnapshotReconciliationState,
        physical_scope: GatewayScopeState,
    ) -> Self {
        let active_scopes = scopes.clone();
        Self {
            scopes,
            active_scopes,
            state,
            inputs: McpGatewaySnapshotInputs {
                physical_scope,
                active_routes: Vec::new(),
            },
            state_reads: AtomicUsize::new(0),
            scan_scope_ids: Mutex::new(Vec::new()),
            input_reads: AtomicUsize::new(0),
            staged: Mutex::new(Vec::new()),
        }
    }

    fn staged(&self) -> Vec<StageMcpGatewaySnapshot> {
        self.staged.lock().expect("staged snapshots").clone()
    }
}

#[async_trait]
impl IMcpGatewaySnapshotRepository for FakeDesiredStateRepository {
    async fn mcp_gateway_reconciliation_scopes(
        &self,
        _observed_at: DateTime<Utc>,
        after_gateway_scope_id: Option<GatewayScopeId>,
        limit: usize,
    ) -> Result<Vec<McpGatewayReconciliationScope>, RepositoryError> {
        let scopes = self
            .scopes
            .iter()
            .filter(|scope| after_gateway_scope_id.is_none_or(|cursor| scope.id > cursor))
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        self.scan_scope_ids
            .lock()
            .expect("scan scope IDs")
            .extend(scopes.iter().map(|scope| scope.id));
        Ok(scopes
            .into_iter()
            .map(|scope| McpGatewayReconciliationScope {
                node_ids: scope.member_node_ids.clone(),
                scope,
            })
            .collect())
    }

    async fn mcp_gateway_snapshot_reconciliation_state(
        &self,
        _node_id: NodeId,
    ) -> Result<McpGatewaySnapshotReconciliationState, RepositoryError> {
        self.state_reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.state.clone())
    }

    async fn mcp_gateway_active_scopes(
        &self,
        node_id: NodeId,
        _observed_at: DateTime<Utc>,
    ) -> Result<Vec<GatewayScope>, RepositoryError> {
        Ok(self
            .active_scopes
            .iter()
            .filter(|scope| scope.contains_member(node_id))
            .cloned()
            .collect())
    }

    async fn mcp_gateway_snapshot_inputs(
        &self,
        _node_id: NodeId,
    ) -> Result<McpGatewaySnapshotInputs, RepositoryError> {
        self.input_reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.inputs.clone())
    }

    async fn stage_mcp_gateway_snapshot(
        &self,
        stage: StageMcpGatewaySnapshot,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
        stage.validate().map_err(RepositoryError::Conflict)?;
        let result = McpGatewaySnapshotStageResult {
            publication: stage.publication().clone(),
            certificate: stage.certificate().cloned(),
        };
        self.staged.lock().expect("staged snapshots").push(stage);
        Ok(result)
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
struct EmptyProjectionPlanner {
    calls: AtomicUsize,
}

#[async_trait]
impl IMcpGatewayProjectionSetPlanner for EmptyProjectionPlanner {
    async fn plan(
        &self,
        request: PlanMcpGatewayProjectionSet,
    ) -> Result<PlannedMcpGatewayProjectionSet, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        PlannedMcpGatewayProjectionSet::empty(
            request.scope,
            request.gateway_node_id,
            request.observed_at,
        )
        .map_err(RepositoryError::Conflict)
    }
}

#[tokio::test]
async fn first_empty_desired_state_does_not_claim_the_ordinary_snapshot_stream() {
    let now = Utc::now();
    let scope = scope(now);
    let node_id = scope.node_id;
    let repository = Arc::new(FakeDesiredStateRepository::new(
        scope,
        McpGatewaySnapshotReconciliationState {
            pending_publication: false,
            latest_mcp_snapshot: None,
        },
        GatewayScopeState::empty(node_id),
    ));
    let planner = Arc::new(EmptyProjectionPlanner::default());
    let report = reconciler(repository.clone(), planner.clone())
        .run_once(now)
        .await
        .expect("empty desired-state reconciliation");

    assert_eq!(report.scopes, 1);
    assert_eq!(report.gateway_nodes, 1);
    assert_eq!(report.unchanged_snapshots, 1);
    assert_eq!(report.staged_snapshots, 0);
    assert!(report.failures.is_empty());
    assert_eq!(planner.calls.load(Ordering::SeqCst), 1);
    assert_eq!(repository.input_reads.load(Ordering::SeqCst), 0);
    assert!(repository.staged().is_empty());
}

#[tokio::test]
async fn removed_scope_membership_stages_one_route_less_node_cleanup_snapshot() {
    let now = Utc::now();
    let scope = scope(now);
    let node_id = scope.node_id;
    let mut repository = FakeDesiredStateRepository::new(
        scope.clone(),
        McpGatewaySnapshotReconciliationState {
            pending_publication: false,
            latest_mcp_snapshot: Some(status(
                &scope,
                digest('a'),
                GatewayPublicationState::Applied,
                1,
                now - ChronoDuration::minutes(2),
                1,
            )),
        },
        GatewayScopeState {
            node_id,
            last_issued_revision: 1,
            installed_revision: Some(1),
            aggregate_version: 1,
        },
    );
    repository.active_scopes.clear();
    let repository = Arc::new(repository);
    let planner = Arc::new(EmptyProjectionPlanner::default());
    let report = reconciler(repository.clone(), planner.clone())
        .run_once(now)
        .await
        .expect("empty removal reconciliation");

    assert_eq!(report.staged_snapshots, 1);
    assert_eq!(report.unchanged_snapshots, 0);
    assert!(report.failures.is_empty());
    let staged = repository.staged();
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].publication().revision, 2);
    assert_eq!(staged[0].publication().expected_revision, Some(1));
    assert!(staged[0].candidate().mcp().projection().is_none());
    assert!(staged[0].candidate().mcp().scope_ids().is_empty());
    assert!(staged[0].certificate().is_none());
    assert!(!staged[0].publication().acl.contains("\nmcp {\n"));
    assert_eq!(planner.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn multiple_scope_triggers_reconcile_one_physical_node_only_once() {
    let now = Utc::now();
    let node_id = NodeId::new();
    let first = scope_for_node(now, node_id);
    let second = GatewayScope::create(
        GatewayScopeId::new(),
        first.organization_id,
        first.project_id,
        first.environment_id,
        node_id,
        now,
    )
    .expect("second scope");
    let mut scopes = vec![first.clone(), second];
    scopes.sort_by_key(|scope| scope.id);
    let mut repository = FakeDesiredStateRepository::with_scopes(
        scopes,
        McpGatewaySnapshotReconciliationState {
            pending_publication: false,
            latest_mcp_snapshot: Some(status(
                &first,
                digest('a'),
                GatewayPublicationState::Applied,
                1,
                now - ChronoDuration::minutes(2),
                1,
            )),
        },
        GatewayScopeState {
            node_id,
            last_issued_revision: 1,
            installed_revision: Some(1),
            aggregate_version: 1,
        },
    );
    repository.active_scopes.clear();
    let repository = Arc::new(repository);
    let planner = Arc::new(EmptyProjectionPlanner::default());

    let report = reconciler(repository.clone(), planner.clone())
        .run_once(now)
        .await
        .expect("node-wide reconciliation");

    assert_eq!(report.scopes, 2);
    assert_eq!(report.gateway_nodes, 1);
    assert_eq!(report.staged_snapshots, 1);
    assert!(report.failures.is_empty());
    assert_eq!(repository.state_reads.load(Ordering::SeqCst), 1);
    assert_eq!(planner.calls.load(Ordering::SeqCst), 0);
    assert_eq!(repository.staged().len(), 1);
}

#[tokio::test]
async fn any_pending_physical_publication_defers_planning_and_staging() {
    let now = Utc::now();
    let scope = scope(now);
    let node_id = scope.node_id;
    let repository = Arc::new(FakeDesiredStateRepository::new(
        scope,
        McpGatewaySnapshotReconciliationState {
            pending_publication: true,
            latest_mcp_snapshot: None,
        },
        GatewayScopeState::empty(node_id),
    ));
    let planner = Arc::new(EmptyProjectionPlanner::default());
    let report = reconciler(repository.clone(), planner.clone())
        .run_once(now)
        .await
        .expect("pending desired-state reconciliation");

    assert_eq!(report.pending_publications, 1);
    assert_eq!(report.staged_snapshots, 0);
    assert!(report.failures.is_empty());
    assert_eq!(planner.calls.load(Ordering::SeqCst), 0);
    assert_eq!(repository.input_reads.load(Ordering::SeqCst), 0);
    assert!(repository.staged().is_empty());
}

#[tokio::test]
async fn bounded_scope_cursor_rotates_without_starving_later_scopes() {
    let now = Utc::now();
    let node_id = NodeId::new();
    let first = scope_for_node(now, node_id);
    let second = GatewayScope::create(
        GatewayScopeId::new(),
        first.organization_id,
        first.project_id,
        first.environment_id,
        node_id,
        now,
    )
    .expect("second Gateway scope");
    let mut scopes = vec![first, second];
    scopes.sort_by_key(|scope| scope.id);
    let expected = vec![scopes[0].id, scopes[1].id, scopes[0].id];
    let repository = Arc::new(FakeDesiredStateRepository::with_scopes(
        scopes,
        McpGatewaySnapshotReconciliationState {
            pending_publication: false,
            latest_mcp_snapshot: None,
        },
        GatewayScopeState::empty(node_id),
    ));
    let reconciler = McpGatewayDesiredStateReconciler::new(
        repository.clone(),
        Arc::new(EmptyProjectionPlanner::default()),
        compiler(),
        Duration::from_secs(1),
        ChronoDuration::minutes(1),
        ChronoDuration::minutes(10),
        ChronoDuration::hours(1),
        ChronoDuration::minutes(1),
        1,
    )
    .expect("bounded MCP Gateway desired-state reconciler");

    for offset in 0..3 {
        let report = reconciler
            .run_once(now + ChronoDuration::seconds(offset))
            .await
            .expect("cursor reconciliation");
        assert_eq!(report.scopes, 1);
        assert_eq!(report.unchanged_snapshots, 1);
        assert!(report.failures.is_empty());
    }
    assert_eq!(
        *repository.scan_scope_ids.lock().expect("scan scope IDs"),
        expected
    );
}

#[test]
fn desired_state_digest_excludes_physical_revision_and_observation_time() {
    let now = canonical_timestamp(Utc::now());
    let later = now + ChronoDuration::seconds(30);
    let scope = scope(now);
    let node_id = scope.node_id;
    let first = compiler()
        .compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
            metadata: GatewaySnapshotMetadata::new(
                node_id,
                1,
                None,
                now,
                now + ChronoDuration::hours(1),
            ),
            physical_scope: GatewayScopeState::empty(node_id),
            certificate_id: None,
            active_routes: Vec::new(),
            mcp: empty_node_projection(scope.clone(), node_id, now),
        })
        .expect("first complete snapshot");
    let second = compiler()
        .compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
            metadata: GatewaySnapshotMetadata::new(
                node_id,
                2,
                Some(1),
                later,
                later + ChronoDuration::hours(1),
            ),
            physical_scope: GatewayScopeState {
                node_id,
                last_issued_revision: 1,
                installed_revision: Some(1),
                aggregate_version: 1,
            },
            certificate_id: None,
            active_routes: Vec::new(),
            mcp: empty_node_projection(scope, node_id, later),
        })
        .expect("second complete snapshot");

    assert_eq!(first.desired_state_digest(), second.desired_state_digest());
    assert_ne!(
        first.snapshot().snapshot_digest,
        second.snapshot().snapshot_digest
    );
}

#[test]
fn empty_node_desired_digest_excludes_the_historical_scope_anchor() {
    let now = canonical_timestamp(Utc::now());
    let later = now + ChronoDuration::seconds(30);
    let node_id = NodeId::new();
    let first_scope = scope_for_node(now, node_id);
    let second_scope = GatewayScope::create(
        GatewayScopeId::new(),
        first_scope.organization_id,
        first_scope.project_id,
        first_scope.environment_id,
        node_id,
        now,
    )
    .expect("second historical anchor");
    let first = compiler()
        .compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
            metadata: GatewaySnapshotMetadata::new(
                node_id,
                1,
                None,
                now,
                now + ChronoDuration::hours(1),
            ),
            physical_scope: GatewayScopeState::empty(node_id),
            certificate_id: None,
            active_routes: Vec::new(),
            mcp: empty_node_cleanup_projection(&first_scope, node_id, now),
        })
        .expect("first cleanup snapshot");
    let second = compiler()
        .compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
            metadata: GatewaySnapshotMetadata::new(
                node_id,
                2,
                Some(1),
                later,
                later + ChronoDuration::hours(1),
            ),
            physical_scope: GatewayScopeState {
                node_id,
                last_issued_revision: 1,
                installed_revision: Some(1),
                aggregate_version: 1,
            },
            certificate_id: None,
            active_routes: Vec::new(),
            mcp: empty_node_cleanup_projection(&second_scope, node_id, later),
        })
        .expect("second cleanup snapshot");

    assert_eq!(first.desired_state_digest(), second.desired_state_digest());
}

#[test]
fn decision_retries_terminal_failures_and_repairs_displaced_mcp_state() {
    let now = Utc::now();
    let scope = scope(now);
    let desired = digest('a');
    assert_eq!(
        reconciliation_decision(
            &McpGatewaySnapshotReconciliationState {
                pending_publication: false,
                latest_mcp_snapshot: None,
            },
            &desired,
            true,
            None,
            false,
            now,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Stage
    );
    let applied = McpGatewaySnapshotReconciliationState {
        pending_publication: false,
        latest_mcp_snapshot: Some(status(
            &scope,
            desired.clone(),
            GatewayPublicationState::Applied,
            7,
            now - ChronoDuration::minutes(2),
            1,
        )),
    };
    assert_eq!(
        reconciliation_decision(
            &applied,
            &desired,
            true,
            Some(7),
            false,
            now,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Unchanged
    );
    assert_eq!(
        reconciliation_decision(
            &applied,
            &desired,
            true,
            Some(7),
            true,
            now,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Stage
    );
    assert_eq!(
        reconciliation_decision(
            &applied,
            &desired,
            true,
            Some(8),
            false,
            now,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Stage
    );
    let applied_empty = McpGatewaySnapshotReconciliationState {
        pending_publication: false,
        latest_mcp_snapshot: Some(status(
            &scope,
            desired.clone(),
            GatewayPublicationState::Applied,
            7,
            now - ChronoDuration::minutes(2),
            0,
        )),
    };
    assert_eq!(
        reconciliation_decision(
            &applied_empty,
            &digest('b'),
            false,
            Some(8),
            false,
            now,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Unchanged
    );

    let failed = McpGatewaySnapshotReconciliationState {
        pending_publication: false,
        latest_mcp_snapshot: Some(status(
            &scope,
            desired.clone(),
            GatewayPublicationState::Rejected,
            7,
            now - ChronoDuration::minutes(2),
            1,
        )),
    };
    assert_eq!(
        reconciliation_decision(
            &failed,
            &desired,
            true,
            Some(6),
            false,
            now,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Stage
    );
    assert_eq!(
        reconciliation_decision(
            &failed,
            &desired,
            true,
            Some(6),
            false,
            now - ChronoDuration::seconds(45),
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::RetryDeferred
    );
    let failed_empty = McpGatewaySnapshotReconciliationState {
        pending_publication: false,
        latest_mcp_snapshot: Some(status(
            &scope,
            desired.clone(),
            GatewayPublicationState::Unavailable,
            7,
            now - ChronoDuration::minutes(2),
            0,
        )),
    };
    assert_eq!(
        reconciliation_decision(
            &failed_empty,
            &desired,
            false,
            Some(6),
            false,
            now,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Stage
    );
    assert_eq!(
        reconciliation_decision(
            &applied,
            &digest('b'),
            true,
            Some(7),
            false,
            now,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Stage
    );
}

fn reconciler(
    repository: Arc<FakeDesiredStateRepository>,
    planner: Arc<EmptyProjectionPlanner>,
) -> McpGatewayDesiredStateReconciler {
    McpGatewayDesiredStateReconciler::new(
        repository,
        planner,
        compiler(),
        Duration::from_secs(1),
        ChronoDuration::minutes(1),
        ChronoDuration::minutes(10),
        ChronoDuration::hours(1),
        ChronoDuration::minutes(1),
        10,
    )
    .expect("MCP Gateway desired-state reconciler")
}

fn empty_node_projection(
    scope: GatewayScope,
    node_id: NodeId,
    observed_at: DateTime<Utc>,
) -> PlannedMcpGatewayNodeProjection {
    let anchor = McpGatewaySnapshotAnchor::from_scope(&scope);
    let set = PlannedMcpGatewayProjectionSet::empty(scope, node_id, observed_at)
        .expect("empty scope projection");
    McpGatewayNodeProjectionAssembler::default()
        .assemble(anchor, node_id, observed_at, vec![set])
        .expect("empty node projection")
}

fn empty_node_cleanup_projection(
    anchor_scope: &GatewayScope,
    node_id: NodeId,
    observed_at: DateTime<Utc>,
) -> PlannedMcpGatewayNodeProjection {
    McpGatewayNodeProjectionAssembler::default()
        .assemble(
            McpGatewaySnapshotAnchor::from_scope(anchor_scope),
            node_id,
            observed_at,
            Vec::new(),
        )
        .expect("empty cleanup node projection")
}

fn compiler() -> GatewaySnapshotCompiler {
    GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8443".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 5_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
    .expect("Gateway snapshot compiler")
}

fn scope(now: DateTime<Utc>) -> GatewayScope {
    scope_for_node(now, NodeId::new())
}

fn scope_for_node(now: DateTime<Utc>, node_id: NodeId) -> GatewayScope {
    GatewayScope::create(
        GatewayScopeId::new(),
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        node_id,
        now,
    )
    .expect("Gateway scope")
}

fn status(
    scope: &GatewayScope,
    desired_state_digest: Sha256Digest,
    state: GatewayPublicationState,
    revision: u64,
    issued_at: DateTime<Utc>,
    mcp_route_count: u32,
) -> McpGatewaySnapshotStatus {
    let certificate_request = (mcp_route_count > 0)
        .then(|| {
            GatewayCertificateRequest::new(
                Uuid::now_v7(),
                vec!["mcp.example.test".into()],
                "/var/lib/a3s-cloud/gateway/certificates/test.crt",
                "/var/lib/a3s-cloud/gateway/certificates/test.key",
            )
        })
        .transpose()
        .expect("Gateway certificate request");
    let acl = certificate_request.as_ref().map_or_else(
        || format!("# MCP Gateway snapshot revision {revision}"),
        |request| {
            format!(
                "# MCP Gateway snapshot revision {revision}\nentrypoints \"https\" {{ tls {{ cert_file = \"{}\"; key_file = \"{}\" }} }}\n",
                request.certificate_file, request.private_key_file
            )
        },
    );
    let snapshot = GatewaySnapshot::new_with_certificate(
        scope.node_id.as_uuid(),
        revision,
        revision.checked_sub(1).filter(|revision| *revision > 0),
        issued_at,
        issued_at + ChronoDuration::hours(1),
        acl,
        certificate_request,
    )
    .expect("Gateway snapshot");
    let mut publication = GatewayPublication::stage(
        scope.node_id,
        NodeCommandId::new(),
        Uuid::now_v7(),
        snapshot,
        issued_at,
        issued_at + ChronoDuration::minutes(1),
    )
    .expect("Gateway publication");
    if state != GatewayPublicationState::Pending {
        publication.state = state;
        publication.acknowledged_at = Some(issued_at + ChronoDuration::seconds(30));
        if matches!(
            state,
            GatewayPublicationState::Rejected | GatewayPublicationState::Unavailable
        ) {
            publication.failure = Some("injected terminal publication outcome".into());
        }
    }
    let status = McpGatewaySnapshotStatus {
        organization_id: scope.organization_id,
        project_id: scope.project_id,
        environment_id: scope.environment_id,
        gateway_scope_id: scope.id,
        desired_state_digest,
        desired_gateway_scope_ids: (mcp_route_count > 0)
            .then_some(vec![scope.id])
            .unwrap_or_default(),
        mcp_route_count,
        publication,
    };
    status.validate().expect("MCP Gateway snapshot status");
    status
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64)))
        .expect("SHA-256 digest")
}
