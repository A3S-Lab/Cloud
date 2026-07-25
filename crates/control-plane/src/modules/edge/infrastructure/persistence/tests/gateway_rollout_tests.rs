use crate::modules::edge::domain::events::{GatewayRolloutStaged, GatewayScopeCreated};
use crate::modules::edge::domain::repositories::{
    CreateGatewayScopeWrite, IEdgeRepository, StageGatewayRollout,
};
use crate::modules::edge::domain::{
    GatewayPublication, GatewayRollout, GatewayRolloutPolicy, GatewayRolloutState, GatewayScope,
};
use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
use crate::modules::shared_kernel::domain::{EnvironmentId, RepositoryError};
use crate::modules::shared_kernel::domain::{
    GatewayRolloutId, GatewayScopeId, IdempotencyRequest, NodeCommandId, NodeId, OrganizationId,
    ProjectId,
};
use a3s_cloud_contracts::{
    GatewayAckState, GatewayManagementProtocol, GatewaySnapshot, NodeGatewayAck,
};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

#[tokio::test]
async fn replicated_rollout_persists_independent_acknowledgements_and_explicit_degradation() {
    let repository = InMemoryEdgeRepository::new();
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
                "gateway-rollout-scopes",
                "replicated-scope",
                serde_json::to_vec(&scope.member_node_ids)
                    .expect("member identities")
                    .as_slice(),
            )
            .expect("scope idempotency"),
            event: GatewayScopeCreated::envelope(&scope, Uuid::now_v7()).expect("scope event"),
        })
        .await
        .expect("create scope");
    let correlation_id = Uuid::now_v7();
    let publications = scope
        .member_node_ids
        .iter()
        .map(|node_id| publication(*node_id, correlation_id, now))
        .collect::<Vec<_>>();
    let rollout = GatewayRollout::stage(GatewayRolloutId::new(), &scope, 1, &publications, now)
        .expect("stage aggregate");
    let bundle = StageGatewayRollout {
        scope: scope.clone(),
        rollout: rollout.clone(),
        publications: publications.clone(),
        certificates: Vec::new(),
        expected_scope_versions: scope
            .member_node_ids
            .iter()
            .map(|node_id| (*node_id, 0))
            .collect::<BTreeMap<_, _>>(),
        idempotency: IdempotencyRequest::new(
            format!("gateway-scopes/{}/rollouts", scope.id),
            "replicated-rollout",
            rollout.id.to_string().as_bytes(),
        )
        .expect("rollout idempotency"),
        event: GatewayRolloutStaged::envelope(&scope, &rollout).expect("rollout event"),
    };
    let staged = repository
        .stage_gateway_rollout(bundle.clone())
        .await
        .expect("stage rollout");
    let replay = repository
        .stage_gateway_rollout(bundle)
        .await
        .expect("replay rollout");
    assert!(!staged.replayed);
    assert!(replay.replayed);
    assert_eq!(staged.rollout, rollout);
    let dispatches = repository
        .pending_gateway_rollout_dispatches(10)
        .await
        .expect("pending rollout dispatches");
    assert_eq!(dispatches.len(), 1);
    dispatches[0].validate().expect("dispatch target");
    assert_eq!(dispatches[0].rollout, rollout);
    assert_eq!(dispatches[0].publications.len(), 3);

    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                &publications[0],
                GatewayAckState::Applied,
                now + Duration::seconds(1),
            ),
            now + Duration::seconds(1),
        )
        .await
        .expect("first acknowledgement");
    let pending = repository
        .find_gateway_rollout(organization_id, rollout.id)
        .await
        .expect("pending rollout");
    assert_eq!(pending.state, GatewayRolloutState::Pending);
    assert_eq!(pending.ready_replicas, 1);
    assert_eq!(
        repository
            .pending_gateway_rollout_dispatches(10)
            .await
            .expect("remaining pending rollout dispatches")[0]
            .publications
            .len(),
        2
    );

    repository
        .project_gateway_acknowledgement(
            &acknowledgement(
                &publications[1],
                GatewayAckState::Applied,
                now + Duration::seconds(2),
            ),
            now + Duration::seconds(2),
        )
        .await
        .expect("second acknowledgement");
    let ready = repository
        .find_gateway_rollout(organization_id, rollout.id)
        .await
        .expect("ready rollout");
    assert_eq!(ready.state, GatewayRolloutState::Ready);
    assert!(ready.serves_traffic().expect("readiness"));
    assert_eq!(
        repository
            .pending_gateway_rollout_dispatches(10)
            .await
            .expect("ready rollout dispatches")[0]
            .publications,
        vec![publications[2].clone()]
    );

    let degraded = repository
        .mark_gateway_rollout_replica_unavailable(
            organization_id,
            rollout.id,
            publications[2].node_id,
            ready.aggregate_version,
            "Gateway missed the rollout readiness deadline",
            now + Duration::seconds(3),
        )
        .await
        .expect("degraded rollout");
    assert_eq!(degraded.state, GatewayRolloutState::Degraded);
    assert_eq!(degraded.ready_replicas, 2);
    assert_eq!(degraded.unavailable_replicas, 1);
    assert!(degraded.serves_traffic().expect("degraded readiness"));
    assert!(repository
        .pending_gateway_rollout_dispatches(10)
        .await
        .expect("terminal rollout dispatches")
        .is_empty());
    assert!(matches!(
        repository
            .mark_gateway_rollout_replica_unavailable(
                organization_id,
                rollout.id,
                publications[2].node_id,
                ready.aggregate_version,
                "Gateway missed the rollout readiness deadline",
                now + Duration::seconds(3),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(repository
        .pending_gateway_rollout_dispatches(0)
        .await
        .is_err());
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
        now + Duration::hours(1),
        format!("# rollout snapshot for {node_id}"),
    )
    .expect("snapshot");
    GatewayPublication::stage(
        node_id,
        NodeCommandId::new(),
        correlation_id,
        snapshot,
        now,
        now + Duration::minutes(3),
    )
    .expect("publication")
}

fn acknowledgement(
    publication: &GatewayPublication,
    state: GatewayAckState,
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
        state,
        ready: state == GatewayAckState::Applied,
        message: (state == GatewayAckState::Rejected).then(|| "snapshot rejected".into()),
        acknowledged_at,
        management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
    }
}
