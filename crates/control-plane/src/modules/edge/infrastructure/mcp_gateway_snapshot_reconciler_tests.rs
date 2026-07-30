use super::{
    IMcpGatewaySnapshotRepository, McpGatewayReconciliationScope, McpGatewaySnapshotDispatchTarget,
    McpGatewaySnapshotInputs, McpGatewaySnapshotReconciler, McpGatewaySnapshotReconciliationState,
    McpGatewaySnapshotStageResult, StageMcpGatewaySnapshot,
};
use crate::modules::edge::domain::services::{GatewayCommandDispatch, IGatewayCommandQueue};
use crate::modules::edge::domain::{GatewayPublication, GatewayPublicationState};
use crate::modules::shared_kernel::domain::EnvironmentId;
use crate::modules::shared_kernel::domain::{
    GatewayScopeId, NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::GatewaySnapshot;
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

struct FakeMcpGatewaySnapshotRepository {
    targets: Mutex<Vec<McpGatewaySnapshotDispatchTarget>>,
}

impl FakeMcpGatewaySnapshotRepository {
    fn new(target: McpGatewaySnapshotDispatchTarget) -> Self {
        Self {
            targets: Mutex::new(vec![target]),
        }
    }

    fn publication(&self) -> GatewayPublication {
        self.targets.lock().expect("MCP snapshot targets")[0]
            .publication
            .clone()
    }
}

#[async_trait]
impl IMcpGatewaySnapshotRepository for FakeMcpGatewaySnapshotRepository {
    async fn mcp_gateway_reconciliation_scopes(
        &self,
        _observed_at: chrono::DateTime<Utc>,
        _after_gateway_scope_id: Option<GatewayScopeId>,
        _limit: usize,
    ) -> Result<Vec<McpGatewayReconciliationScope>, RepositoryError> {
        Ok(Vec::new())
    }

    async fn mcp_gateway_snapshot_reconciliation_state(
        &self,
        _node_id: NodeId,
    ) -> Result<McpGatewaySnapshotReconciliationState, RepositoryError> {
        Err(RepositoryError::Storage(
            "unit fake does not read MCP Gateway desired state".into(),
        ))
    }

    async fn mcp_gateway_active_scopes(
        &self,
        _node_id: NodeId,
        _observed_at: chrono::DateTime<Utc>,
    ) -> Result<Vec<crate::modules::edge::domain::GatewayScope>, RepositoryError> {
        Ok(Vec::new())
    }

    async fn mcp_gateway_snapshot_inputs(
        &self,
        _node_id: NodeId,
    ) -> Result<McpGatewaySnapshotInputs, RepositoryError> {
        Err(RepositoryError::Storage(
            "unit fake does not read MCP Gateway inputs".into(),
        ))
    }

    async fn stage_mcp_gateway_snapshot(
        &self,
        _stage: StageMcpGatewaySnapshot,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
        Err(RepositoryError::Storage(
            "unit fake does not stage MCP Gateway snapshots".into(),
        ))
    }

    async fn pending_mcp_gateway_snapshots(
        &self,
        limit: usize,
    ) -> Result<Vec<McpGatewaySnapshotDispatchTarget>, RepositoryError> {
        Ok(self
            .targets
            .lock()
            .expect("MCP snapshot targets")
            .iter()
            .filter(|target| target.publication.state == GatewayPublicationState::Pending)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn mark_mcp_gateway_snapshot_unavailable(
        &self,
        organization_id: OrganizationId,
        gateway_scope_id: GatewayScopeId,
        node_id: NodeId,
        gateway_revision: u64,
        gateway_command_id: NodeCommandId,
        failure: &str,
        observed_at: chrono::DateTime<Utc>,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
        let mut targets = self.targets.lock().expect("MCP snapshot targets");
        let target = targets
            .iter_mut()
            .find(|target| {
                target.organization_id == organization_id
                    && target.gateway_scope_id == gateway_scope_id
                    && target.publication.node_id == node_id
                    && target.publication.revision == gateway_revision
                    && target.publication.command_id == gateway_command_id
            })
            .ok_or(RepositoryError::NotFound)?;
        target
            .publication
            .mark_unavailable(failure, observed_at)
            .map_err(RepositoryError::Conflict)?;
        Ok(McpGatewaySnapshotStageResult {
            publication: target.publication.clone(),
            certificate: None,
        })
    }
}

#[derive(Default)]
struct RecordingGatewayQueue {
    state: Mutex<QueueState>,
}

#[derive(Default)]
struct QueueState {
    fail_once: bool,
    command_ids: BTreeSet<NodeCommandId>,
    attempts: usize,
}

impl RecordingGatewayQueue {
    fn failing_once() -> Self {
        Self {
            state: Mutex::new(QueueState {
                fail_once: true,
                ..QueueState::default()
            }),
        }
    }

    fn attempts(&self) -> usize {
        self.state.lock().expect("Gateway queue state").attempts
    }
}

#[async_trait]
impl IGatewayCommandQueue for RecordingGatewayQueue {
    async fn enqueue(
        &self,
        publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError> {
        let mut state = self.state.lock().expect("Gateway queue state");
        state.attempts += 1;
        if state.fail_once {
            state.fail_once = false;
            return Err(RepositoryError::Storage(
                "injected MCP Gateway command queue failure".into(),
            ));
        }
        let replayed = !state.command_ids.insert(publication.command_id);
        Ok(GatewayCommandDispatch { replayed })
    }
}

#[tokio::test]
async fn reconciler_recovers_dispatch_after_restart_and_replays_idempotently() {
    let issued_at = Utc::now();
    let repository = Arc::new(FakeMcpGatewaySnapshotRepository::new(target(issued_at)));
    let queue = Arc::new(RecordingGatewayQueue::failing_once());
    let first = McpGatewaySnapshotReconciler::new(
        repository.clone(),
        queue.clone(),
        Duration::from_secs(1),
        10,
    )
    .expect("MCP Gateway reconciler")
    .run_once(issued_at + ChronoDuration::seconds(1))
    .await
    .expect("first reconciliation");
    assert_eq!(first.pending_snapshots, 1);
    assert_eq!(first.dispatched_commands, 0);
    assert_eq!(first.failures.len(), 1);
    assert_eq!(
        repository.publication().state,
        GatewayPublicationState::Pending
    );

    let restarted = McpGatewaySnapshotReconciler::new(
        repository.clone(),
        queue.clone(),
        Duration::from_secs(1),
        10,
    )
    .expect("restarted MCP Gateway reconciler");
    let recovered = restarted
        .run_once(issued_at + ChronoDuration::seconds(2))
        .await
        .expect("recovered reconciliation");
    assert_eq!(recovered.dispatched_commands, 1);
    assert_eq!(recovered.replayed_commands, 0);
    assert!(recovered.failures.is_empty());

    let replayed = restarted
        .run_once(issued_at + ChronoDuration::seconds(3))
        .await
        .expect("replayed reconciliation");
    assert_eq!(replayed.dispatched_commands, 1);
    assert_eq!(replayed.replayed_commands, 1);
    assert!(replayed.failures.is_empty());
    assert_eq!(queue.attempts(), 3);
}

#[tokio::test]
async fn reconciler_expires_the_exact_pending_snapshot_without_dispatch() {
    let issued_at = Utc::now();
    let repository = Arc::new(FakeMcpGatewaySnapshotRepository::new(target(issued_at)));
    let queue = Arc::new(RecordingGatewayQueue::default());
    let report = McpGatewaySnapshotReconciler::new(
        repository.clone(),
        queue.clone(),
        Duration::from_secs(1),
        10,
    )
    .expect("MCP Gateway reconciler")
    .run_once(issued_at + ChronoDuration::minutes(2))
    .await
    .expect("expiry reconciliation");
    assert_eq!(report.pending_snapshots, 1);
    assert_eq!(report.unavailable_snapshots, 1);
    assert_eq!(report.dispatched_commands, 0);
    assert!(report.failures.is_empty());
    assert_eq!(queue.attempts(), 0);
    let publication = repository.publication();
    assert_eq!(publication.state, GatewayPublicationState::Unavailable);
    assert_eq!(
        publication.failure.as_deref(),
        Some("MCP Gateway snapshot command expired before exact acknowledgement")
    );
}

#[tokio::test]
async fn reconciler_fails_closed_when_its_clock_predates_the_publication() {
    let issued_at = Utc::now();
    let repository = Arc::new(FakeMcpGatewaySnapshotRepository::new(target(issued_at)));
    let queue = Arc::new(RecordingGatewayQueue::default());
    let report = McpGatewaySnapshotReconciler::new(
        repository.clone(),
        queue.clone(),
        Duration::from_secs(1),
        10,
    )
    .expect("MCP Gateway reconciler")
    .run_once(issued_at - ChronoDuration::milliseconds(1))
    .await
    .expect("future publication reconciliation");
    assert_eq!(report.pending_snapshots, 1);
    assert_eq!(report.dispatched_commands, 0);
    assert_eq!(report.unavailable_snapshots, 0);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].operation, "validate");
    assert_eq!(queue.attempts(), 0);
    assert_eq!(
        repository.publication().state,
        GatewayPublicationState::Pending
    );
}

fn target(issued_at: chrono::DateTime<Utc>) -> McpGatewaySnapshotDispatchTarget {
    let node_id = NodeId::new();
    let snapshot = GatewaySnapshot::new(
        node_id.as_uuid(),
        1,
        None,
        issued_at,
        issued_at + ChronoDuration::hours(1),
        format!("# hosted MCP reconciliation snapshot for {node_id}"),
    )
    .expect("Gateway snapshot");
    let publication = GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        Uuid::now_v7(),
        snapshot,
        issued_at,
        issued_at + ChronoDuration::minutes(1),
    )
    .expect("Gateway publication");
    let target = McpGatewaySnapshotDispatchTarget {
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        environment_id: EnvironmentId::new(),
        gateway_scope_id: GatewayScopeId::new(),
        publication,
    };
    target.validate().expect("MCP Gateway dispatch target");
    target
}
