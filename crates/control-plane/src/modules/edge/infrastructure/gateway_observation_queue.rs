use crate::modules::edge::domain::services::{
    GatewayObservationCommand, GatewayObservationCommandOutcome, GatewayObservationDispatch,
    IGatewayObservationQueue,
};
use crate::modules::fleet::{
    FleetGatewayObservationOutcome, FleetGatewaySnapshotObserveRequest,
    IFleetGatewaySnapshotCommandPort,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

pub struct FleetGatewayObservationQueue {
    commands: Arc<dyn IFleetGatewaySnapshotCommandPort>,
}

impl FleetGatewayObservationQueue {
    pub fn new(commands: Arc<dyn IFleetGatewaySnapshotCommandPort>) -> Self {
        Self { commands }
    }
}

#[async_trait]
impl IGatewayObservationQueue for FleetGatewayObservationQueue {
    async fn enqueue(
        &self,
        command: &GatewayObservationCommand,
    ) -> Result<GatewayObservationDispatch, RepositoryError> {
        command.validate().map_err(RepositoryError::Conflict)?;
        let dispatch = self
            .commands
            .enqueue_observe(fleet_observe_request(command)?)
            .await?;
        Ok(GatewayObservationDispatch {
            replayed: dispatch.replayed,
        })
    }

    async fn outcome(
        &self,
        command: &GatewayObservationCommand,
    ) -> Result<Option<GatewayObservationCommandOutcome>, RepositoryError> {
        command.validate().map_err(RepositoryError::Conflict)?;
        let Some(outcome) = self
            .commands
            .observation_outcome(&fleet_observe_request(command)?)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(match outcome {
            FleetGatewayObservationOutcome::Observed {
                observation,
                completed_at,
            } => GatewayObservationCommandOutcome::Observed {
                observation,
                completed_at,
            },
            FleetGatewayObservationOutcome::Failed {
                failure,
                retryable,
                completed_at,
            } => GatewayObservationCommandOutcome::Failed {
                failure,
                retryable,
                completed_at,
            },
        }))
    }
}

fn fleet_observe_request(
    command: &GatewayObservationCommand,
) -> Result<FleetGatewaySnapshotObserveRequest, RepositoryError> {
    Ok(FleetGatewaySnapshotObserveRequest {
        node_id: command.node_id,
        command_id: command.command_id,
        correlation_id: command.correlation_id,
        issued_at: command.issued_at,
        not_after: command.not_after,
        aggregate_id: command.rollout_id.as_uuid(),
        request: command.request().map_err(RepositoryError::Conflict)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::services::IGatewayObservationQueue;
    use crate::modules::fleet::FleetGatewaySnapshotCommandService;
    use crate::modules::fleet::domain::entities::EnrollmentToken;
    use crate::modules::fleet::domain::repositories::{
        INodeControlRepository, INodeRepository, NodeEnrollmentDraft,
    };
    use crate::modules::fleet::domain::value_objects::{
        EnrollmentTokenCredential, NodeCapabilities, NodeName,
    };
    use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, EnrollmentTokenId, GatewayRolloutId, IdempotencyRequest,
        NodeCommandId, OrganizationId,
    };
    use a3s_cloud_contracts::{
        DomainEventEnvelope, GatewayManagementProtocol, GatewaySnapshotObservationState,
        NodeCommandAck, NodeCommandLeaseRequest, NodeCommandOutcome, NodeCommandPayload,
        NodeCommandResult, NodeGatewaySnapshotObservation,
    };
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    #[tokio::test]
    async fn fleet_queue_enqueues_replays_and_restores_exact_observation_outcomes() {
        let repository = Arc::new(InMemoryNodeRepository::new());
        let now = canonical_timestamp(Utc::now());
        let organization_id = OrganizationId::new();
        let credential =
            EnrollmentTokenCredential::from_secret(&format!("a3sn_{}", "9".repeat(64)))
                .expect("enrollment credential");
        let token = EnrollmentToken::new(
            EnrollmentTokenId::new(),
            organization_id,
            "Gateway observation worker",
            credential.clone(),
            now,
            now + Duration::minutes(10),
        )
        .expect("enrollment token");
        repository
            .issue_enrollment_token(
                token.clone(),
                DomainEventEnvelope {
                    event_id: Uuid::now_v7(),
                    event_key: "fleet.enrollment-token.issued".into(),
                    schema_version: 1,
                    scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                        organization_id: organization_id.as_uuid(),
                    },
                    aggregate_id: token.id.as_uuid(),
                    aggregate_version: token.aggregate_version,
                    occurred_at: now,
                    correlation_id: Uuid::now_v7(),
                    causation_id: None,
                    payload: serde_json::json!({}),
                },
                IdempotencyRequest::new(
                    "fleet/tokens",
                    "gateway-observation-worker",
                    b"Gateway observation worker",
                )
                .expect("token idempotency"),
            )
            .await
            .expect("issue enrollment token");
        let agent_instance_id = Uuid::now_v7();
        let enrollment = repository
            .reserve_enrollment(
                &credential,
                NodeEnrollmentDraft {
                    proposed_node_id: crate::modules::shared_kernel::domain::NodeId::new(),
                    name: NodeName::new("gateway-observation-worker").expect("node name"),
                    agent_instance_id,
                    agent_version: "0.1.0".into(),
                    capabilities: NodeCapabilities::new(
                        "a3s-box",
                        "gateway-observation-test",
                        serde_json::json!({
                            "schema": a3s_runtime::contract::RuntimeCapabilities::SCHEMA,
                            "provider_id": "a3s-box",
                            "provider_build": "gateway-observation-test"
                        }),
                    )
                    .expect("node capabilities"),
                    request_digest: format!("sha256:{}", "8".repeat(64)),
                    requested_at: now,
                },
            )
            .await
            .expect("reserve enrolled node");
        let node_id = enrollment.node.id;
        let control: Arc<dyn INodeControlRepository> = repository.clone();
        let fleet = Arc::new(FleetGatewaySnapshotCommandService::new(Arc::clone(&control)));
        let queue = FleetGatewayObservationQueue::new(fleet);
        let command = GatewayObservationCommand::new(
            GatewayRolloutId::new(),
            Uuid::now_v7(),
            node_id,
            7,
            format!("sha256:{}", "a".repeat(64)),
            NodeCommandId::new(),
            1,
            now,
            now + Duration::minutes(1),
        )
        .expect("observation command");

        assert!(
            !queue
                .enqueue(&command)
                .await
                .expect("enqueue command")
                .replayed
        );
        assert!(
            queue
                .enqueue(&command)
                .await
                .expect("replay command")
                .replayed
        );
        let stored = control
            .find_command(node_id, command.command_id)
            .await
            .expect("find observation command")
            .expect("stored observation command");
        assert_eq!(stored.aggregate_id, command.rollout_id.as_uuid());
        assert_eq!(
            stored.payload,
            NodeCommandPayload::GatewaySnapshotObserve {
                request: command.request().expect("observation request")
            }
        );

        let lease = control
            .lease_commands(
                &NodeCommandLeaseRequest {
                    schema: NodeCommandLeaseRequest::SCHEMA.into(),
                    node_id: node_id.as_uuid(),
                    agent_instance_id,
                    after_sequence: 0,
                    max_commands: 1,
                    wait_ms: 0,
                },
                Uuid::now_v7(),
                now + Duration::seconds(1),
                now + Duration::seconds(30),
            )
            .await
            .expect("lease observation command");
        let envelope = lease.commands.first().expect("leased observation command");
        let observation = NodeGatewaySnapshotObservation {
            schema: NodeGatewaySnapshotObservation::SCHEMA.into(),
            observation_id: Uuid::now_v7(),
            command_id: command.command_id.as_uuid(),
            node_id: node_id.as_uuid(),
            gateway_id: node_id.as_uuid(),
            revision: command.candidate_revision,
            snapshot_digest: command.candidate_snapshot_digest.clone(),
            state: GatewaySnapshotObservationState::Uninitialized,
            ready: false,
            applied: None,
            observed_at: now + Duration::seconds(2),
            management_protocol: GatewayManagementProtocol::advertised_v1(),
        };
        control
            .acknowledge_command(
                NodeCommandAck {
                    schema: NodeCommandAck::SCHEMA.into(),
                    command_id: envelope.command_id,
                    lease_id: envelope.lease_id,
                    node_id: envelope.node_id,
                    sequence: envelope.sequence,
                    payload_digest: envelope.payload_digest.clone(),
                    completed_at: now + Duration::seconds(2),
                    outcome: NodeCommandOutcome::Succeeded {
                        result: Box::new(NodeCommandResult::GatewaySnapshotObserved {
                            observation: observation.clone(),
                        }),
                    },
                },
                now + Duration::seconds(2),
            )
            .await
            .expect("acknowledge observation command");
        assert_eq!(
            queue.outcome(&command).await.expect("restore outcome"),
            Some(GatewayObservationCommandOutcome::Observed {
                observation: Box::new(observation),
                completed_at: now + Duration::seconds(2),
            })
        );
    }
}
