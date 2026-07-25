use crate::modules::edge::domain::services::{
    GatewayObservationCommand, GatewayObservationCommandOutcome, GatewayObservationDispatch,
    IGatewayObservationQueue,
};
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload, NodeCommandResult};
use async_trait::async_trait;
use std::sync::Arc;

pub struct FleetGatewayObservationQueue {
    commands: Arc<dyn INodeControlRepository>,
}

impl FleetGatewayObservationQueue {
    pub fn new(commands: Arc<dyn INodeControlRepository>) -> Self {
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
        let result = self
            .commands
            .enqueue_command(NodeCommandDraft {
                proposed_command_id: command.command_id,
                node_id: command.node_id,
                aggregate_id: command.rollout_id.as_uuid(),
                payload: NodeCommandPayload::GatewaySnapshotObserve {
                    request: command.request().map_err(RepositoryError::Conflict)?,
                },
                issued_at: command.issued_at,
                not_after: command.not_after,
                correlation_id: command.correlation_id,
            })
            .await?;
        Ok(GatewayObservationDispatch {
            replayed: result.replayed,
        })
    }

    async fn outcome(
        &self,
        command: &GatewayObservationCommand,
    ) -> Result<Option<GatewayObservationCommandOutcome>, RepositoryError> {
        command.validate().map_err(RepositoryError::Conflict)?;
        let Some(acknowledgement) = self
            .commands
            .command_acknowledgement(command.node_id, command.command_id)
            .await?
        else {
            return Ok(None);
        };
        if acknowledgement.command_id != command.command_id.as_uuid()
            || acknowledgement.node_id != command.node_id.as_uuid()
        {
            return Err(RepositoryError::Storage(
                "Gateway observation acknowledgement identity is inconsistent".into(),
            ));
        }
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => match *result {
                NodeCommandResult::GatewaySnapshotObserved { observation } => {
                    observation
                        .validate_for(
                            command.command_id.as_uuid(),
                            command.node_id.as_uuid(),
                            &command.request().map_err(RepositoryError::Conflict)?,
                        )
                        .map_err(RepositoryError::Storage)?;
                    Ok(Some(GatewayObservationCommandOutcome::Observed {
                        observation: Box::new(observation),
                        completed_at: acknowledgement.completed_at,
                    }))
                }
                _ => Err(RepositoryError::Storage(
                    "Gateway observation command stored an incompatible successful result".into(),
                )),
            },
            NodeCommandOutcome::Rejected { failure } => {
                Ok(Some(GatewayObservationCommandOutcome::Failed {
                    failure: bounded_failure("rejected", &failure.code),
                    retryable: failure.retryable,
                    completed_at: acknowledgement.completed_at,
                }))
            }
            NodeCommandOutcome::Failed { failure } => {
                Ok(Some(GatewayObservationCommandOutcome::Failed {
                    failure: bounded_failure("failed", &failure.code),
                    retryable: failure.retryable,
                    completed_at: acknowledgement.completed_at,
                }))
            }
        }
    }
}

fn bounded_failure(outcome: &str, code: &str) -> String {
    format!("Gateway observation command {outcome} with code {code}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::services::IGatewayObservationQueue;
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
        NodeCommandAck, NodeCommandLeaseRequest, NodeGatewaySnapshotObservation,
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
                    organization_id: organization_id.as_uuid(),
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
                        "docker",
                        "gateway-observation-test",
                        serde_json::json!({
                            "schema": "a3s.runtime.capabilities.v3",
                            "provider_id": "docker",
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
        let queue = FleetGatewayObservationQueue::new(Arc::clone(&control));
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
