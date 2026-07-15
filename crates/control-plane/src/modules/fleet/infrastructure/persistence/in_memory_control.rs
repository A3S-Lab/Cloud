use super::in_memory::project_heartbeat;
use super::InMemoryNodeRepository;
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, NodeLogBatchReceiptDraft, NodeLogChunkReceiptDraft,
    RuntimeObservationRecord,
};
use crate::modules::fleet::domain::value_objects::{NodeCapabilities, NodeState};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, NodeCommandId, NodeId, RepositoryError,
};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeCommandOutcome,
    NodeGatewayAck, NodeGatewayAckReceipt, NodeLogChunkReceipt, NodeObservationBatch,
    NodeObservationReceipt, RuntimeObservationReport,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(super) struct StoredNodeCommand {
    pub(super) command: NodeCommand,
    lease: Option<CommandLease>,
    acknowledgement: Option<NodeCommandAck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoredObservation {
    node_id: NodeId,
    agent_instance_id: Uuid,
    report: RuntimeObservationReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoredGatewayAcknowledgement {
    acknowledgement: NodeGatewayAck,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoredLogBatch {
    draft: NodeLogBatchReceiptDraft,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StoredLogChunkReceipt {
    draft: NodeLogChunkReceiptDraft,
}

#[derive(Debug, Clone)]
struct CommandLease {
    lease_id: Uuid,
    agent_instance_id: Uuid,
    leased_until: DateTime<Utc>,
}

#[async_trait]
impl INodeControlRepository for InMemoryNodeRepository {
    async fn enqueue_command(
        &self,
        draft: NodeCommandDraft,
    ) -> Result<IdempotentWrite<NodeCommand>, RepositoryError> {
        let mut state = self.state.write().await;
        if let Some(existing) = state.commands.get(&draft.proposed_command_id) {
            let retry = NodeCommand::issue(draft, existing.command.sequence)
                .map_err(RepositoryError::Conflict)?;
            if retry != existing.command {
                return Err(RepositoryError::Conflict(
                    "node command ID was reused with different input".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.command.clone(),
                replayed: true,
            });
        }

        let node_key = state
            .nodes
            .keys()
            .find(|(_, node_id)| *node_id == draft.node_id)
            .copied()
            .ok_or(RepositoryError::NotFound)?;
        if state
            .nodes
            .get(&node_key)
            .is_some_and(|node| node.state == NodeState::Revoked)
        {
            return Err(RepositoryError::NotFound);
        }

        if let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply { request } = &draft.payload {
            let requested_generation = draft.payload.generation();
            let requested_digest = request.spec.digest().map_err(RepositoryError::Conflict)?;
            let mut prior_applies = state
                .commands
                .values()
                .filter(|stored| {
                    stored.command.node_id == draft.node_id
                        && stored.command.aggregate_id == draft.aggregate_id
                        && stored.command.kind() == "runtime_apply"
                })
                .collect::<Vec<_>>();
            prior_applies.sort_by_key(|stored| stored.command.generation());
            if let Some(existing) = prior_applies
                .iter()
                .find(|stored| stored.command.generation() == requested_generation)
            {
                let a3s_cloud_contracts::NodeCommandPayload::RuntimeApply {
                    request: existing_request,
                } = &existing.command.payload
                else {
                    return Err(RepositoryError::Storage(
                        "stored Runtime apply command has the wrong payload kind".into(),
                    ));
                };
                if existing_request
                    .spec
                    .digest()
                    .map_err(RepositoryError::Storage)?
                    != requested_digest
                {
                    return Err(RepositoryError::Conflict(
                        "Runtime apply generation was reused with a different specification".into(),
                    ));
                }
            }
            if prior_applies
                .last()
                .is_some_and(|stored| stored.command.generation() > requested_generation)
            {
                return Err(RepositoryError::Conflict(
                    "Runtime apply generation regressed".into(),
                ));
            }
        }

        let node = state
            .nodes
            .get_mut(&node_key)
            .ok_or_else(|| RepositoryError::Storage("node disappeared while enqueueing".into()))?;
        let sequence = node
            .last_sequence
            .checked_add(1)
            .ok_or_else(|| RepositoryError::Conflict("node command sequence exhausted".into()))?;
        let command = NodeCommand::issue(draft, sequence).map_err(RepositoryError::Conflict)?;
        node.last_sequence = sequence;
        state.commands.insert(
            command.id,
            StoredNodeCommand {
                command: command.clone(),
                lease: None,
                acknowledgement: None,
            },
        );
        Ok(IdempotentWrite {
            value: command,
            replayed: false,
        })
    }

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<NodeCommand>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .commands
            .get(&command_id)
            .filter(|stored| stored.command.node_id == node_id)
            .map(|stored| stored.command.clone()))
    }

    async fn lease_commands(
        &self,
        request: &NodeCommandLeaseRequest,
        lease_id: Uuid,
        now: DateTime<Utc>,
        leased_until: DateTime<Utc>,
    ) -> Result<NodeCommandLeaseResponse, RepositoryError> {
        request.validate().map_err(RepositoryError::Conflict)?;
        if lease_id.is_nil() || leased_until <= now {
            return Err(RepositoryError::Conflict(
                "command lease identity or expiry is invalid".into(),
            ));
        }
        let mut state = self.state.write().await;
        let state_node = state
            .nodes
            .values()
            .find(|node| node.id.as_uuid() == request.node_id)
            .ok_or(RepositoryError::NotFound)?;
        if state_node.state == NodeState::Revoked
            || state_node.agent_instance_id != request.agent_instance_id
        {
            return Err(RepositoryError::NotFound);
        }

        let node_id = NodeId::from_uuid(request.node_id);
        let mut ordered = state
            .commands
            .iter()
            .filter(|(_, stored)| {
                stored.command.node_id == node_id
                    && stored.command.sequence > request.after_sequence
                    && stored.acknowledgement.is_none()
            })
            .map(|(id, stored)| (*id, stored.command.sequence))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|(_, sequence)| *sequence);

        let mut selected = Vec::new();
        for (command_id, _) in ordered {
            let stored = state.commands.get(&command_id).ok_or_else(|| {
                RepositoryError::Storage("command disappeared while leasing".into())
            })?;
            if stored
                .lease
                .as_ref()
                .is_some_and(|lease| lease.leased_until > now)
            {
                break;
            }
            selected.push(command_id);
            if selected.len() == usize::from(request.max_commands) {
                break;
            }
        }

        let mut commands = Vec::with_capacity(selected.len());
        for command_id in selected {
            let stored = state.commands.get_mut(&command_id).ok_or_else(|| {
                RepositoryError::Storage("command disappeared while assigning lease".into())
            })?;
            stored.lease = Some(CommandLease {
                lease_id,
                agent_instance_id: request.agent_instance_id,
                leased_until,
            });
            commands.push(
                stored
                    .command
                    .envelope(lease_id)
                    .map_err(RepositoryError::Storage)?,
            );
        }
        let response = NodeCommandLeaseResponse {
            schema: NodeCommandLeaseResponse::SCHEMA.into(),
            lease_id,
            node_id: request.node_id,
            agent_instance_id: request.agent_instance_id,
            leased_until,
            commands,
        };
        response.validate(now).map_err(RepositoryError::Storage)?;
        Ok(response)
    }

    async fn acknowledge_command(
        &self,
        acknowledgement: NodeCommandAck,
        _received_at: DateTime<Utc>,
    ) -> Result<IdempotentWrite<NodeCommandAck>, RepositoryError> {
        let mut state = self.state.write().await;
        let stored = state
            .commands
            .get_mut(&NodeCommandId::from_uuid(acknowledgement.command_id))
            .ok_or(RepositoryError::NotFound)?;
        if let Some(existing) = &stored.acknowledgement {
            if existing != &acknowledgement {
                return Err(RepositoryError::Conflict(
                    "command acknowledgement was replayed with different content".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }
        let lease = stored
            .lease
            .as_ref()
            .ok_or_else(|| RepositoryError::Conflict("command has not been leased".into()))?;
        if lease.agent_instance_id.is_nil() {
            return Err(RepositoryError::Storage(
                "stored command lease has no agent identity".into(),
            ));
        }
        let envelope = stored
            .command
            .envelope(lease.lease_id)
            .map_err(RepositoryError::Storage)?;
        acknowledgement
            .validate_against(&envelope)
            .map_err(RepositoryError::Conflict)?;
        if acknowledgement.completed_at > lease.leased_until {
            return Err(RepositoryError::Conflict(
                "command acknowledgement completed after its lease expired".into(),
            ));
        }
        if matches!(
            acknowledgement.outcome,
            NodeCommandOutcome::Succeeded { .. }
        ) && acknowledgement.completed_at > stored.command.not_after
        {
            return Err(RepositoryError::Conflict(
                "successful command acknowledgement completed after command expiry".into(),
            ));
        }
        stored.acknowledgement = Some(acknowledgement.clone());
        Ok(IdempotentWrite {
            value: acknowledgement,
            replayed: false,
        })
    }

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<NodeCommandAck>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .commands
            .get(&command_id)
            .filter(|stored| stored.command.node_id == node_id)
            .and_then(|stored| stored.acknowledgement.clone()))
    }

    async fn record_observations(
        &self,
        batch: NodeObservationBatch,
        _received_at: DateTime<Utc>,
    ) -> Result<NodeObservationReceipt, RepositoryError> {
        batch.validate().map_err(RepositoryError::Conflict)?;
        let capabilities = NodeCapabilities::new(
            batch.heartbeat.runtime_capabilities.provider_id.clone(),
            batch.heartbeat.runtime_capabilities.provider_build.clone(),
            serde_json::to_value(&batch.heartbeat.runtime_capabilities)
                .map_err(|error| RepositoryError::Storage(error.to_string()))?,
        )
        .map_err(RepositoryError::Conflict)?;
        let update = crate::modules::fleet::domain::repositories::NodeHeartbeatUpdate {
            node_id: NodeId::from_uuid(batch.node_id),
            agent_instance_id: batch.agent_instance_id,
            agent_version: batch.heartbeat.agent_version.clone(),
            capabilities,
            observed_at: batch.heartbeat.observed_at,
        };
        let mut state = self.state.write().await;
        let node_key = state
            .nodes
            .keys()
            .find(|(_, node_id)| node_id.as_uuid() == batch.node_id)
            .copied()
            .ok_or(RepositoryError::NotFound)?;
        let projected = project_heartbeat(
            state
                .nodes
                .get(&node_key)
                .ok_or_else(|| RepositoryError::Storage("observation node disappeared".into()))?,
            &update,
        )?;

        let mut new_reports = Vec::new();
        let mut replayed_reports = 0_usize;
        for report in &batch.observations {
            if let Some(command_id) = report.command_id {
                let command = state
                    .commands
                    .get(&NodeCommandId::from_uuid(command_id))
                    .ok_or_else(|| {
                        RepositoryError::Conflict(
                            "Runtime observation references an unknown command".into(),
                        )
                    })?;
                if command.command.node_id.as_uuid() != batch.node_id {
                    return Err(RepositoryError::Conflict(
                        "Runtime observation command belongs to another node".into(),
                    ));
                }
            }
            let candidate = StoredObservation {
                node_id: NodeId::from_uuid(batch.node_id),
                agent_instance_id: batch.agent_instance_id,
                report: report.clone(),
            };
            if let Some(existing) = state.observations.get(&report.report_id) {
                if existing != &candidate {
                    return Err(RepositoryError::Conflict(
                        "Runtime observation report ID was reused with different content".into(),
                    ));
                }
                replayed_reports += 1;
            } else {
                new_reports.push(candidate);
            }
        }

        state.nodes.insert(node_key, projected);
        for report in new_reports.iter().cloned() {
            state.observations.insert(report.report.report_id, report);
        }
        let receipt = NodeObservationReceipt {
            schema: NodeObservationReceipt::SCHEMA.into(),
            node_id: batch.node_id,
            heartbeat_observed_at: batch.heartbeat.observed_at,
            accepted_reports: u16::try_from(new_reports.len()).map_err(|_| {
                RepositoryError::Storage("observation acceptance count overflowed".into())
            })?,
            replayed_reports: u16::try_from(replayed_reports).map_err(|_| {
                RepositoryError::Storage("observation replay count overflowed".into())
            })?,
        };
        receipt.validate().map_err(RepositoryError::Storage)?;
        Ok(receipt)
    }

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<RuntimeObservationRecord>, RepositoryError> {
        if unit_id.is_empty() || unit_id.len() > 512 || unit_id.contains('\0') || generation == 0 {
            return Err(RepositoryError::Conflict(
                "Runtime observation lookup identity is invalid".into(),
            ));
        }
        Ok(self
            .state
            .read()
            .await
            .observations
            .values()
            .filter(|stored| {
                stored.node_id == node_id
                    && stored.report.observation.unit_id == unit_id
                    && stored.report.observation.generation == generation
            })
            .max_by_key(|stored| (stored.report.observed_at, stored.report.report_id))
            .map(|stored| RuntimeObservationRecord {
                report_id: stored.report.report_id,
                node_id: stored.node_id,
                command_id: stored.report.command_id.map(NodeCommandId::from_uuid),
                observed_at: stored.report.observed_at,
                received_at: stored.report.observed_at,
                observation: stored.report.observation.clone(),
            }))
    }

    async fn record_gateway_acknowledgement(
        &self,
        acknowledgement: NodeGatewayAck,
        _received_at: DateTime<Utc>,
    ) -> Result<NodeGatewayAckReceipt, RepositoryError> {
        acknowledgement
            .validate()
            .map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let node = state
            .nodes
            .values()
            .find(|node| node.id.as_uuid() == acknowledgement.node_id)
            .ok_or(RepositoryError::NotFound)?;
        if node.state == NodeState::Revoked {
            return Err(RepositoryError::NotFound);
        }
        if let Some(existing) = state
            .gateway_acknowledgements
            .get(&acknowledgement.acknowledgement_id)
        {
            if existing.acknowledgement != acknowledgement {
                return Err(RepositoryError::Conflict(
                    "Gateway acknowledgement ID was reused with different content".into(),
                ));
            }
            return Ok(gateway_receipt(&acknowledgement, true));
        }
        if state.gateway_acknowledgements.values().any(|existing| {
            existing.acknowledgement.node_id == acknowledgement.node_id
                && existing.acknowledgement.revision == acknowledgement.revision
                && existing.acknowledgement.snapshot_digest == acknowledgement.snapshot_digest
        }) {
            return Err(RepositoryError::Conflict(
                "Gateway revision already has an acknowledgement".into(),
            ));
        }
        state.gateway_acknowledgements.insert(
            acknowledgement.acknowledgement_id,
            StoredGatewayAcknowledgement {
                acknowledgement: acknowledgement.clone(),
            },
        );
        Ok(gateway_receipt(&acknowledgement, false))
    }

    async fn record_log_chunks(
        &self,
        batch: NodeLogBatchReceiptDraft,
        _received_at: DateTime<Utc>,
    ) -> Result<NodeLogChunkReceipt, RepositoryError> {
        batch.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let node = state
            .nodes
            .values()
            .find(|node| node.id == batch.node_id)
            .ok_or(RepositoryError::NotFound)?;
        if node.state == NodeState::Revoked {
            return Err(RepositoryError::NotFound);
        }
        if let Some(existing) = state.log_batches.get(&batch.batch_id) {
            if existing.draft != batch {
                return Err(RepositoryError::Conflict(
                    "log batch ID was reused with different content".into(),
                ));
            }
            return log_receipt(&batch, true);
        }

        for chunk in &batch.chunks {
            let key = (
                batch.node_id,
                chunk.unit_id.clone(),
                chunk.generation,
                chunk.sequence,
            );
            if let Some(existing) = state.log_chunks.get(&key) {
                if existing.draft != *chunk {
                    return Err(RepositoryError::Conflict(
                        "log sequence was reused with different content".into(),
                    ));
                }
            }
            if state
                .log_chunks
                .iter()
                .any(|((node_id, unit_id, generation, _), existing)| {
                    *node_id == batch.node_id
                        && unit_id == &chunk.unit_id
                        && *generation == chunk.generation
                        && existing.draft.cursor == chunk.cursor
                        && existing.draft.sequence != chunk.sequence
                })
            {
                return Err(RepositoryError::Conflict(
                    "log cursor was reused for another sequence".into(),
                ));
            }
        }
        for chunk in &batch.chunks {
            state
                .log_chunks
                .entry((
                    batch.node_id,
                    chunk.unit_id.clone(),
                    chunk.generation,
                    chunk.sequence,
                ))
                .or_insert_with(|| StoredLogChunkReceipt {
                    draft: chunk.clone(),
                });
        }
        state.log_batches.insert(
            batch.batch_id,
            StoredLogBatch {
                draft: batch.clone(),
            },
        );
        log_receipt(&batch, false)
    }
}

fn gateway_receipt(acknowledgement: &NodeGatewayAck, replayed: bool) -> NodeGatewayAckReceipt {
    NodeGatewayAckReceipt {
        schema: NodeGatewayAckReceipt::SCHEMA.into(),
        acknowledgement_id: acknowledgement.acknowledgement_id,
        node_id: acknowledgement.node_id,
        replayed,
    }
}

fn log_receipt(
    batch: &NodeLogBatchReceiptDraft,
    replayed: bool,
) -> Result<NodeLogChunkReceipt, RepositoryError> {
    let receipt = NodeLogChunkReceipt {
        schema: NodeLogChunkReceipt::SCHEMA.into(),
        batch_id: batch.batch_id,
        node_id: batch.node_id.as_uuid(),
        accepted_chunks: u16::try_from(batch.chunks.len())
            .map_err(|_| RepositoryError::Storage("log chunk count overflowed".into()))?,
        replayed,
    };
    receipt.validate().map_err(RepositoryError::Storage)?;
    Ok(receipt)
}
