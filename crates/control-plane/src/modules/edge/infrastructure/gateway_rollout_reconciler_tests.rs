use super::GatewayRolloutReconciler;
use crate::modules::edge::domain::events::{GatewayRolloutStaged, GatewayScopeCreated};
use crate::modules::edge::domain::repositories::{
    CreateGatewayScopeWrite, IEdgeRepository, StageGatewayRollout,
};
use crate::modules::edge::domain::services::{GatewayCommandDispatch, IGatewayCommandQueue};
use crate::modules::edge::domain::{
    GatewayPublication, GatewayRollout, GatewayRolloutPolicy, GatewayRolloutState, GatewayScope,
};
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayRolloutId, GatewayScopeId, IdempotencyRequest, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::{
    GatewayAckState, GatewayManagementProtocol, GatewaySnapshot, NodeGatewayAck,
};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

#[derive(Default)]
struct RecordingGatewayQueue {
    state: Mutex<QueueState>,
}

#[derive(Default)]
struct QueueState {
    fail_once: Option<NodeId>,
    command_ids: BTreeSet<NodeCommandId>,
    attempts: Vec<NodeId>,
}

impl RecordingGatewayQueue {
    fn failing_once(node_id: NodeId) -> Self {
        Self {
            state: Mutex::new(QueueState {
                fail_once: Some(node_id),
                ..QueueState::default()
            }),
        }
    }

    fn attempts(&self) -> Vec<NodeId> {
        self.state
            .lock()
            .expect("Gateway queue state")
            .attempts
            .clone()
    }
}

#[async_trait]
impl IGatewayCommandQueue for RecordingGatewayQueue {
    async fn enqueue(
        &self,
        publication: &GatewayPublication,
    ) -> Result<GatewayCommandDispatch, RepositoryError> {
        let mut state = self.state.lock().expect("Gateway queue state");
        state.attempts.push(publication.node_id);
        if state.fail_once == Some(publication.node_id) {
            state.fail_once = None;
            return Err(RepositoryError::Storage(
                "injected Gateway command queue failure".into(),
            ));
        }
        let replayed = !state.command_ids.insert(publication.command_id);
        Ok(GatewayCommandDispatch { replayed })
    }
}

#[tokio::test]
async fn reconciler_redispatches_partial_rollout_and_expires_only_the_missing_replica() {
    let repository = Arc::new(InMemoryEdgeRepository::new());
    let now = Utc::now();
    let organization_id = OrganizationId::new();
    let primary = NodeId::new();
    let secondary = NodeId::new();
    let tertiary = NodeId::new();
    let scope = GatewayScope::create_replicated(
        GatewayScopeId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        primary,
        vec![primary, secondary, tertiary],
        GatewayRolloutPolicy::new(2, 1, 3).expect("rollout policy"),
        now,
    )
    .expect("Gateway scope");
    repository
        .create_gateway_scope(CreateGatewayScopeWrite {
            scope: scope.clone(),
            idempotency: IdempotencyRequest::new(
                "gateway-rollout-reconciler-scopes",
                scope.id.to_string(),
                serde_json::to_vec(&scope.member_node_ids)
                    .expect("scope membership")
                    .as_slice(),
            )
            .expect("scope idempotency"),
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7()).expect("scope event"),
        })
        .await
        .expect("create Gateway scope");
    let correlation_id = Uuid::now_v7();
    let publications = scope
        .member_node_ids
        .iter()
        .map(|node_id| publication(*node_id, correlation_id, now))
        .collect::<Vec<_>>();
    let rollout = GatewayRollout::stage(GatewayRolloutId::new(), &scope, 1, &publications, now)
        .expect("Gateway rollout");
    repository
        .stage_gateway_rollout(StageGatewayRollout {
            scope: scope.clone(),
            rollout: rollout.clone(),
            route_replicas: Vec::new(),
            publications: publications.clone(),
            certificates: Vec::new(),
            expected_scope_versions: scope
                .member_node_ids
                .iter()
                .map(|node_id| (*node_id, 0))
                .collect::<BTreeMap<_, _>>(),
            idempotency: IdempotencyRequest::new(
                format!("gateway-scopes/{}/rollouts", scope.id),
                "reconciler-rollout",
                rollout.id.to_string().as_bytes(),
            )
            .expect("rollout idempotency"),
            event: GatewayRolloutStaged::envelope(&scope, &rollout).expect("rollout event"),
            route_event: None,
        })
        .await
        .expect("stage Gateway rollout");

    let failed_node_id = publications[1].node_id;
    let queue = Arc::new(RecordingGatewayQueue::failing_once(failed_node_id));
    let first = GatewayRolloutReconciler::new(
        repository.clone(),
        queue.clone(),
        Duration::from_secs(1),
        10,
    )
    .expect("Gateway rollout reconciler")
    .run_once(now + ChronoDuration::seconds(1))
    .await
    .expect("first reconciliation");
    assert_eq!(first.active_rollouts, 1);
    assert_eq!(first.pending_replicas, 3);
    assert_eq!(first.dispatched_commands, 2);
    assert_eq!(first.replayed_commands, 0);
    assert_eq!(first.failures.len(), 1);
    assert_eq!(first.failures[0].node_id, failed_node_id);

    let restarted = GatewayRolloutReconciler::new(
        repository.clone(),
        queue.clone(),
        Duration::from_secs(1),
        10,
    )
    .expect("restarted Gateway rollout reconciler");
    let recovered = restarted
        .run_once(now + ChronoDuration::seconds(2))
        .await
        .expect("recovered reconciliation");
    assert_eq!(recovered.dispatched_commands, 3);
    assert_eq!(recovered.replayed_commands, 2);
    assert!(recovered.failures.is_empty());
    assert_eq!(queue.attempts().len(), 6);

    for (index, publication) in publications.iter().take(2).enumerate() {
        let acknowledged_at =
            now + ChronoDuration::seconds(i64::try_from(index + 3).expect("timestamp"));
        repository
            .project_gateway_acknowledgement(
                &acknowledgement(publication, acknowledged_at),
                acknowledged_at + ChronoDuration::milliseconds(1),
            )
            .await
            .expect("Gateway acknowledgement");
    }
    let expired = restarted
        .run_once(now + ChronoDuration::minutes(2))
        .await
        .expect("expiry reconciliation");
    assert_eq!(expired.active_rollouts, 1);
    assert_eq!(expired.pending_replicas, 1);
    assert_eq!(expired.expired_replicas, 1);
    assert_eq!(expired.dispatched_commands, 0);
    assert!(expired.failures.is_empty());

    let degraded = repository
        .find_gateway_rollout(organization_id, rollout.id)
        .await
        .expect("degraded rollout");
    assert_eq!(degraded.state, GatewayRolloutState::Degraded);
    assert_eq!(degraded.ready_replicas, 2);
    assert_eq!(degraded.unavailable_replicas, 1);
    let terminal = restarted
        .run_once(now + ChronoDuration::minutes(3))
        .await
        .expect("terminal reconciliation");
    assert_eq!(terminal.active_rollouts, 0);
    assert_eq!(terminal.pending_replicas, 0);
}

fn publication(
    node_id: NodeId,
    correlation_id: Uuid,
    now: chrono::DateTime<Utc>,
) -> GatewayPublication {
    let snapshot = GatewaySnapshot::new(
        node_id.as_uuid(),
        1,
        None,
        now,
        now + ChronoDuration::hours(1),
        format!("# rollout reconciliation snapshot for {node_id}"),
    )
    .expect("Gateway snapshot");
    GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        correlation_id,
        snapshot,
        now,
        now + ChronoDuration::minutes(1),
    )
    .expect("Gateway publication")
}

fn acknowledgement(
    publication: &GatewayPublication,
    acknowledged_at: chrono::DateTime<Utc>,
) -> NodeGatewayAck {
    NodeGatewayAck {
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
        management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
    }
}
