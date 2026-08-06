use super::mcp_gateway_desired_state_reconciler::{
    reconciliation_decision, ReconciliationDecision,
};
use super::{
    CompileMcpGatewaySnapshot, GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
    GatewaySnapshotMetadata, IMcpGatewayProjectionSetPlanner, IMcpGatewaySnapshotRepository,
    McpGatewayDesiredStateReconciler, McpGatewaySnapshotDispatchTarget, McpGatewaySnapshotInputs,
    McpGatewaySnapshotReconciliationState, McpGatewaySnapshotStageResult, McpGatewaySnapshotStatus,
    PlanMcpGatewayProjectionSet, PlannedMcpGatewayProjectionSet, StageMcpGatewaySnapshot,
};
use crate::modules::edge::domain::{
    GatewayCertificate, GatewayCertificateMaterial, GatewayPublication, GatewayPublicationState,
    GatewayScope, GatewayScopeState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId,
    NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError, Sha256Digest,
};
use a3s_cloud_contracts::{
    GatewayAckState, GatewayCertificateRequest, GatewayManagementProtocol, GatewaySnapshot,
    NodeGatewayAck,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

struct FakeDesiredStateRepository {
    scopes: Vec<GatewayScope>,
    state: McpGatewaySnapshotReconciliationState,
    inputs: McpGatewaySnapshotInputs,
    state_reads: AtomicUsize,
    state_scope_ids: Mutex<Vec<GatewayScopeId>>,
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
        Self {
            scopes,
            state,
            inputs: McpGatewaySnapshotInputs {
                physical_scope,
                active_routes: Vec::new(),
            },
            state_reads: AtomicUsize::new(0),
            state_scope_ids: Mutex::new(Vec::new()),
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
    ) -> Result<Vec<GatewayScope>, RepositoryError> {
        Ok(self
            .scopes
            .iter()
            .filter(|scope| after_gateway_scope_id.is_none_or(|cursor| scope.id > cursor))
            .take(limit)
            .cloned()
            .collect())
    }

    async fn mcp_gateway_snapshot_reconciliation_state(
        &self,
        gateway_scope_id: GatewayScopeId,
        _node_id: NodeId,
    ) -> Result<McpGatewaySnapshotReconciliationState, RepositoryError> {
        self.state_reads.fetch_add(1, Ordering::SeqCst);
        self.state_scope_ids
            .lock()
            .expect("state scope IDs")
            .push(gateway_scope_id);
        Ok(self.state.clone())
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
    assert_eq!(report.gateway_members, 1);
    assert_eq!(report.unchanged_snapshots, 1);
    assert_eq!(report.staged_snapshots, 0);
    assert!(report.failures.is_empty());
    assert_eq!(planner.calls.load(Ordering::SeqCst), 1);
    assert_eq!(repository.input_reads.load(Ordering::SeqCst), 0);
    assert!(repository.staged().is_empty());
}

#[tokio::test]
async fn changed_empty_desired_state_stages_one_route_less_removal_snapshot() {
    let now = Utc::now();
    let scope = scope(now);
    let node_id = scope.node_id;
    let repository = Arc::new(FakeDesiredStateRepository::new(
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
    ));
    let planner = Arc::new(EmptyProjectionPlanner::default());
    let report = reconciler(repository.clone(), planner)
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
    assert!(staged[0].certificate().is_none());
    assert!(!staged[0].publication().acl.contains("\nmcp {\n"));
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
    let mut scopes = vec![scope_for_node(now, node_id), scope_for_node(now, node_id)];
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
        ChronoDuration::hours(1),
        ChronoDuration::days(7),
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
        *repository.state_scope_ids.lock().expect("state scope IDs"),
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
            mcp: PlannedMcpGatewayProjectionSet::empty(scope.clone(), node_id, now)
                .expect("first empty projection"),
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
            mcp: PlannedMcpGatewayProjectionSet::empty(scope, node_id, later)
                .expect("second empty projection"),
        })
        .expect("second complete snapshot");

    assert_eq!(first.desired_state_digest(), second.desired_state_digest());
    assert_ne!(
        first.snapshot().snapshot_digest,
        second.snapshot().snapshot_digest
    );
}

#[test]
fn decision_retries_terminal_failures_and_repairs_displaced_mcp_state() {
    let now = Utc::now();
    let certificate_renew_before = now + ChronoDuration::days(7);
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
            false,
            None,
            now,
            certificate_renew_before,
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
            false,
            Some(7),
            now,
            certificate_renew_before,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Unchanged
    );
    assert_eq!(
        reconciliation_decision(
            &applied,
            &desired,
            true,
            false,
            Some(8),
            now,
            certificate_renew_before,
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
            false,
            Some(8),
            now,
            certificate_renew_before,
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
            false,
            Some(6),
            now,
            certificate_renew_before,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Stage
    );
    assert_eq!(
        reconciliation_decision(
            &failed,
            &desired,
            true,
            false,
            Some(6),
            now - ChronoDuration::seconds(45),
            certificate_renew_before,
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
            false,
            Some(6),
            now,
            certificate_renew_before,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Stage
    );
    assert_eq!(
        reconciliation_decision(
            &applied,
            &digest('b'),
            true,
            false,
            Some(7),
            now,
            certificate_renew_before,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Stage
    );
}

#[test]
fn decision_renews_only_mcp_owned_certificate_before_expiry() {
    let now = canonical_timestamp(Utc::now());
    let scope = scope(now);
    let desired = digest('a');
    let installed_revision = Some(7);
    let certificate_renew_before = now + ChronoDuration::minutes(1);
    let expiring = McpGatewaySnapshotReconciliationState {
        pending_publication: false,
        latest_mcp_snapshot: Some(status_with_certificate_expiry(
            &scope,
            desired.clone(),
            GatewayPublicationState::Applied,
            7,
            now - ChronoDuration::minutes(2),
            1,
            now + ChronoDuration::seconds(30),
        )),
    };

    assert_eq!(
        reconciliation_decision(
            &expiring,
            &desired,
            true,
            false,
            installed_revision,
            now,
            certificate_renew_before,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Stage
    );
    assert_eq!(
        reconciliation_decision(
            &expiring,
            &desired,
            true,
            true,
            installed_revision,
            now,
            certificate_renew_before,
            ChronoDuration::minutes(1),
        ),
        ReconciliationDecision::Unchanged
    );

    let mut missing = expiring;
    missing
        .latest_mcp_snapshot
        .as_mut()
        .expect("latest MCP snapshot")
        .certificate = None;
    assert_eq!(
        reconciliation_decision(
            &missing,
            &desired,
            true,
            false,
            installed_revision,
            now,
            certificate_renew_before,
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
        ChronoDuration::hours(1),
        ChronoDuration::days(7),
        ChronoDuration::minutes(1),
        10,
    )
    .expect("MCP Gateway desired-state reconciler")
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
    status_with_certificate_expiry(
        scope,
        desired_state_digest,
        state,
        revision,
        issued_at,
        mcp_route_count,
        issued_at + ChronoDuration::days(30),
    )
}

fn status_with_certificate_expiry(
    scope: &GatewayScope,
    desired_state_digest: Sha256Digest,
    state: GatewayPublicationState,
    revision: u64,
    issued_at: DateTime<Utc>,
    mcp_route_count: u32,
    certificate_expires_at: DateTime<Utc>,
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
        certificate_request.clone(),
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
    let certificate = certificate_request
        .map(|request| {
            GatewayCertificate::provision(
                GatewayCertificateId::from_uuid(request.certificate_id),
                scope.organization_id,
                scope.node_id,
                vec![DomainClaimId::new()],
                publication.revision,
                publication.command_id,
                publication.snapshot_digest.clone(),
                request,
                publication.command_issued_at,
            )
        })
        .transpose()
        .expect("MCP Gateway certificate");
    let certificate = certificate
        .map(|mut certificate| {
            let acknowledged_at = issued_at + ChronoDuration::seconds(30);
            match state {
                GatewayPublicationState::Pending => {}
                GatewayPublicationState::Applied => {
                    let certificate_issued_at = issued_at + ChronoDuration::seconds(10);
                    certificate
                        .record_issued(
                            format!("sha256:{}", "b".repeat(64)),
                            GatewayCertificateMaterial {
                                serial_number: certificate.id.to_string(),
                                fingerprint: format!("sha256:{}", "c".repeat(64)),
                                certificate_pem: "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n".into(),
                                ca_bundle_pem: "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n".into(),
                                issued_at: certificate_issued_at,
                                expires_at: certificate_expires_at,
                            },
                            certificate_issued_at,
                        )
                        .expect("issued MCP Gateway certificate");
                    certificate
                        .apply_gateway_acknowledgement(&NodeGatewayAck {
                            schema: NodeGatewayAck::SCHEMA.into(),
                            acknowledgement_id: Uuid::now_v7(),
                            command_id: publication.command_id.as_uuid(),
                            node_id: publication.node_id.as_uuid(),
                            gateway_id: publication.node_id.as_uuid(),
                            revision: publication.revision,
                            snapshot_digest: publication.snapshot_digest.clone(),
                            expires_at: publication.snapshot_expires_at,
                            state: GatewayAckState::Applied,
                            ready: true,
                            message: None,
                            acknowledged_at,
                            management_protocol: Some(
                                GatewayManagementProtocol::advertised_v1(),
                            ),
                        })
                        .expect("ready MCP Gateway certificate");
                }
                GatewayPublicationState::Rejected | GatewayPublicationState::Unavailable => {
                    certificate
                        .mark_delivery_unavailable(
                            "injected terminal publication outcome",
                            acknowledged_at,
                        )
                        .expect("failed MCP Gateway certificate");
                }
            }
            certificate
        });
    let status = McpGatewaySnapshotStatus {
        organization_id: scope.organization_id,
        project_id: scope.project_id,
        environment_id: scope.environment_id,
        gateway_scope_id: scope.id,
        desired_state_digest,
        mcp_route_count,
        publication,
        certificate,
    };
    status.validate().expect("MCP Gateway snapshot status");
    status
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64)))
        .expect("SHA-256 digest")
}
