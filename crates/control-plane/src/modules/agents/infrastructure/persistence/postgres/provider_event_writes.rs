use super::approval_queries::{lock_active_checkpoint, lock_latest_checkpoint};
use super::approval_writes::insert_checkpoint;
use super::queries::{load_execution_by_id, lock_conversation, lock_execution};
use super::writes::{
    insert_event, materialize_event_drafts, persist_conversation, persist_execution,
};
use crate::infrastructure::{
    idempotency_replay, store_audit, store_idempotency, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::agents::domain::{
    project_agent_approval_checkpoint, AcceptAgentProviderEventBatchWrite, AgentApprovalCheckpoint,
    AgentApprovalCheckpointStatus, AgentConversation, AgentExecutionEvent,
    AgentExecutionEventDraft, AgentExecutionEventKind, AgentExecutionStatus,
};
use crate::modules::shared_kernel::domain::{AgentExecutionId, RepositoryError};
use a3s_cloud_contracts::{AgentProviderSemanticEventV1, NodeAgentProviderEventReceiptV1};
use a3s_orm::{PostgresExecutor, PostgresTransaction};
use uuid::Uuid;

const AGENT_TOOL_AUDIT_SCHEMA_V1: &str = "a3s.cloud.agent-tool-audit.v1";
const AGENT_APPROVAL_AUDIT_SCHEMA_V1: &str = "a3s.cloud.agent-approval-audit.v1";

pub(super) async fn accept_provider_event_batch(
    executor: &PostgresExecutor,
    write: AcceptAgentProviderEventBatchWrite,
) -> Result<NodeAgentProviderEventReceiptV1, RepositoryError> {
    write.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(receipt) = replay_provider_event_batch(transaction, &write).await? {
                    return Ok(receipt);
                }
                let execution_id = AgentExecutionId::from_uuid(write.batch.binding.execution_id);
                let probe = load_execution_by_id(transaction, write.organization_id, execution_id)
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                let mut conversation =
                    lock_conversation(transaction, write.organization_id, probe.conversation_id)
                        .await?;
                let previous_conversation_version = conversation.aggregate_version;
                let mut execution =
                    lock_execution(transaction, write.organization_id, execution_id).await?;
                let previous_execution_version = execution.aggregate_version;
                let binding = execution
                    .code
                    .as_ref()
                    .cloned()
                    .ok_or(RepositoryError::NotFound)?;
                if binding.node_id() != write.authenticated_node_id {
                    return Err(RepositoryError::NotFound.into());
                }
                let current_binding = binding
                    .node_provider_runtime_binding(execution.id.as_uuid())
                    .map_err(PostgresPersistenceError::Invariant)?;
                if current_binding != write.batch.binding {
                    if binding
                        .can_settle_recovery_predecessor_provider_runtime_binding(
                            &write.batch.binding,
                            execution.id,
                        )
                        .map_err(PostgresPersistenceError::Invariant)?
                    {
                        let receipt = write
                            .receipt(false)
                            .map_err(PostgresPersistenceError::Invariant)?;
                        store_idempotency(transaction, &write.idempotency, &receipt).await?;
                        return Ok(receipt);
                    }
                    return Err(RepositoryError::Conflict(
                        "Agent provider event batch changed its bound Runtime or run identity"
                            .into(),
                    )
                    .into());
                }
                let active_checkpoint =
                    lock_active_checkpoint(transaction, write.organization_id, execution.id)
                        .await?;
                if execution.status == AgentExecutionStatus::AwaitingApproval
                    && active_checkpoint.is_none()
                {
                    let latest_checkpoint =
                        lock_latest_checkpoint(transaction, write.organization_id, execution.id)
                            .await?;
                    let page_identity_digest = write
                        .batch
                        .page
                        .identity
                        .digest()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    if !latest_checkpoint.as_ref().is_some_and(|checkpoint| {
                        checkpoint.status == AgentApprovalCheckpointStatus::Resumed
                            && checkpoint.provider_run_identity_digest.as_str()
                                == page_identity_digest.as_str()
                            && checkpoint.invocation_profile_digest.as_str()
                                == write
                                    .batch
                                    .page
                                    .identity
                                    .invocation_profile_digest
                                    .as_deref()
                                    .unwrap_or_default()
                            && write.batch.page.after_event_sequence
                                == Some(checkpoint.source_event_sequence)
                    }) {
                        return Err(RepositoryError::Conflict(
                            "awaiting Agent provider advanced without an exact approval resume"
                                .into(),
                        )
                        .into());
                    }
                }
                if active_checkpoint.is_some()
                    && (write.batch.page.retention_gap
                        || write.batch.page.state
                            != a3s_cloud_contracts::AgentProviderRunStateV1::AwaitingApproval
                        || write.batch.page.events.iter().any(|record| {
                            matches!(
                                &record.event,
                                AgentProviderSemanticEventV1::ToolRequest { .. }
                                    | AgentProviderSemanticEventV1::ToolResult { .. }
                            )
                        }))
                {
                    return Err(RepositoryError::Conflict(
                        "paused Agent provider advanced before exact resume".into(),
                    )
                    .into());
                }

                let projected_at = write.accepted_at.max(execution.updated_at);
                let drafts = if write.batch.page.retention_gap {
                    binding
                        .validate_provider_recovery_page(&write.batch.page)
                        .map_err(RepositoryError::Conflict)?;
                    execution
                        .recover_code_run(&binding, projected_at)
                        .map_err(RepositoryError::Conflict)?;
                    Vec::new()
                } else {
                    let drafts = AgentExecutionEventDraft::semantic_from_provider_page(
                        &write.batch.page,
                        projected_at,
                    )
                    .map_err(invalid_repository_write)?;
                    execution
                        .accept_provider_event_page(&write.batch.page, projected_at, &drafts)
                        .map_err(RepositoryError::Conflict)?;
                    drafts
                };
                let checkpoint = if write.batch.page.retention_gap {
                    None
                } else {
                    project_agent_approval_checkpoint(
                        &conversation,
                        &execution,
                        &write.batch.page,
                        projected_at,
                    )
                    .map_err(invalid_repository_write)?
                };
                if execution.status == AgentExecutionStatus::AwaitingApproval
                    && active_checkpoint.is_none()
                    && checkpoint.is_none()
                {
                    return Err(RepositoryError::Conflict(
                        "Agent provider entered approval without an exact checkpoint".into(),
                    )
                    .into());
                }
                if active_checkpoint.is_some() && checkpoint.is_some() {
                    return Err(RepositoryError::Conflict(
                        "Agent execution already has an active approval checkpoint".into(),
                    )
                    .into());
                }

                let events = if drafts.is_empty() {
                    Vec::new()
                } else {
                    let last_occurred_at = drafts
                        .last()
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "non-empty Agent provider event page omitted its last draft".into(),
                            )
                        })?
                        .occurred_at;
                    let first_sequence = conversation
                        .allocate_event_sequences(drafts.len(), last_occurred_at)
                        .map_err(RepositoryError::Conflict)?;
                    materialize_event_drafts(
                        write.organization_id,
                        conversation.id,
                        execution.id,
                        drafts,
                        first_sequence,
                    )?
                };

                if !events.is_empty() {
                    persist_conversation(transaction, &conversation, previous_conversation_version)
                        .await?;
                }
                persist_execution(transaction, &execution, previous_execution_version).await?;
                for event in &events {
                    insert_event(transaction, event).await?;
                }
                if let Some(checkpoint) = &checkpoint {
                    insert_checkpoint(transaction, checkpoint).await?;
                    store_approval_requested_audit(transaction, &write, checkpoint).await?;
                }
                store_tool_event_audits(transaction, &write, &conversation, &events).await?;

                let receipt = write
                    .receipt(false)
                    .map_err(PostgresPersistenceError::Invariant)?;
                store_idempotency(transaction, &write.idempotency, &receipt).await?;
                Ok(receipt)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn store_approval_requested_audit(
    transaction: &PostgresTransaction,
    write: &AcceptAgentProviderEventBatchWrite,
    checkpoint: &AgentApprovalCheckpoint,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: None,
            action: "agent.execution.approval-requested",
            aggregate_id: checkpoint.execution_id.as_uuid(),
            occurred_at: checkpoint.requested_at,
            request_id: write.batch.batch_id,
            scope: AuditWrite::resource_scope(
                checkpoint.organization_id.as_uuid(),
                checkpoint.project_id,
                Some(checkpoint.environment_id),
            ),
            details: serde_json::json!({
                "schema": AGENT_APPROVAL_AUDIT_SCHEMA_V1,
                "projectId": checkpoint.project_id,
                "environmentId": checkpoint.environment_id,
                "conversationId": checkpoint.conversation_id,
                "executionId": checkpoint.execution_id,
                "checkpointId": checkpoint.id,
                "checkpointVersion": checkpoint.aggregate_version,
                "providerRunIdentityDigest": checkpoint.provider_run_identity_digest,
                "invocationProfileDigest": checkpoint.invocation_profile_digest,
                "providerSourceSequence": checkpoint.source_event_sequence,
                "callId": checkpoint.call_id,
                "tool": checkpoint.tool,
                "request": checkpoint.request,
                "expiresAt": checkpoint.expires_at,
                "nodeId": write.authenticated_node_id,
            }),
        },
    )
    .await
}

async fn replay_provider_event_batch(
    transaction: &PostgresTransaction,
    write: &AcceptAgentProviderEventBatchWrite,
) -> Result<Option<NodeAgentProviderEventReceiptV1>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<NodeAgentProviderEventReceiptV1>(transaction, &write.idempotency)
            .await?
    else {
        return Ok(None);
    };
    let mut receipt = replay.value;
    receipt.receipt.replayed = true;
    receipt.validate_for(&write.batch).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "Agent provider event replay changed its immutable receipt: {error}"
        ))
    })?;
    Ok(Some(receipt))
}

fn invalid_repository_write(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!("invalid Agent repository write: {error}"))
}

async fn store_tool_event_audits(
    transaction: &PostgresTransaction,
    write: &AcceptAgentProviderEventBatchWrite,
    conversation: &AgentConversation,
    events: &[AgentExecutionEvent],
) -> Result<(), PostgresPersistenceError> {
    if events.len() < write.batch.page.events.len() {
        return Err(PostgresPersistenceError::Invariant(
            "Agent provider semantic projection omitted a source event".into(),
        ));
    }
    for (source, event) in write.batch.page.events.iter().zip(events) {
        let Some(audit) = tool_event_audit_projection(&source.event) else {
            continue;
        };
        if event.kind != audit.expected_kind {
            return Err(PostgresPersistenceError::Invariant(
                "Agent provider Tool audit mapping changed its semantic event kind".into(),
            ));
        }
        store_audit(
            transaction,
            &AuditWrite {
                audit_id: Uuid::now_v7(),
                actor_id: None,
                action: audit.action,
                aggregate_id: event.execution_id.as_uuid(),
                occurred_at: event.occurred_at,
                request_id: write.batch.batch_id,
                scope: AuditWrite::resource_scope(
                    write.organization_id.as_uuid(),
                    conversation.project_id,
                    Some(conversation.environment_id),
                ),
                details: serde_json::json!({
                    "schema": AGENT_TOOL_AUDIT_SCHEMA_V1,
                    "projectId": conversation.project_id,
                    "environmentId": conversation.environment_id,
                    "conversationId": event.conversation_id,
                    "executionId": event.execution_id,
                    "eventSequence": event.sequence,
                    "providerSourceSequence": source.sequence,
                    "providerOccurredAtMs": source.occurred_at_ms,
                    "providerObservedAtMs": write.batch.page.observed_at_ms,
                    "nodeId": write.authenticated_node_id,
                    "workloadId": write.batch.binding.workload_id,
                    "workloadRevisionId": write.batch.binding.workload_revision_id,
                    "deploymentId": write.batch.binding.deployment_id,
                    "replicaId": write.batch.binding.replica_id,
                    "runtimeUnitId": write.batch.binding.runtime_unit_id.as_str(),
                    "runtimeGeneration": write.batch.binding.runtime_generation,
                    "runtimeSpecDigest": write.batch.binding.runtime_spec_digest.as_str(),
                    "providerRunId": write
                        .batch
                        .binding
                        .provider_run_identity
                        .run_id
                        .as_str(),
                    "providerProfileDigest": write.batch.binding.provider_profile_digest.as_str(),
                    "invocationProfileDigest": write
                        .batch
                        .binding
                        .provider_run_identity
                        .invocation_profile_digest
                        .as_deref(),
                    "contentDigest": event.content.digest().as_str(),
                    "contentSizeBytes": event.content.size_bytes(),
                    "event": audit.details,
                }),
            },
        )
        .await?;
    }
    Ok(())
}

struct ToolEventAuditProjection {
    action: &'static str,
    expected_kind: AgentExecutionEventKind,
    details: serde_json::Value,
}

fn tool_event_audit_projection(
    event: &AgentProviderSemanticEventV1,
) -> Option<ToolEventAuditProjection> {
    match event {
        AgentProviderSemanticEventV1::ModelOutput { .. } => None,
        AgentProviderSemanticEventV1::ToolRequest {
            call_id,
            tool,
            request,
        } => Some(ToolEventAuditProjection {
            action: "agent.execution.tool-requested",
            expected_kind: AgentExecutionEventKind::ToolRequest,
            details: serde_json::json!({
                "callId": call_id,
                "tool": tool,
                "request": request,
            }),
        }),
        AgentProviderSemanticEventV1::ToolResult {
            call_id,
            tool,
            request_digest,
            outcome,
            result,
        } => Some(ToolEventAuditProjection {
            action: "agent.execution.tool-result-recorded",
            expected_kind: AgentExecutionEventKind::ToolResult,
            details: serde_json::json!({
                "callId": call_id,
                "tool": tool,
                "requestDigest": request_digest,
                "outcome": outcome,
                "result": result,
            }),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        AgentProviderToolPayloadIdentityV1, AgentProviderToolResultOutcomeV1, HarnessToolBindingV1,
    };

    fn tool() -> HarnessToolBindingV1 {
        HarnessToolBindingV1 {
            name: "workspace.search".into(),
            revision: "1.0.0".into(),
            contract_digest: format!("sha256:{}", "a".repeat(64)),
            approval_required: false,
        }
    }

    fn payload(digest: char, size_bytes: u64) -> AgentProviderToolPayloadIdentityV1 {
        AgentProviderToolPayloadIdentityV1 {
            digest: format!("sha256:{}", digest.to_string().repeat(64)),
            size_bytes,
            media_type: "application/json".into(),
        }
    }

    #[test]
    fn tool_audit_projection_is_typed_and_body_free() {
        let request = payload('b', 128);
        let requested = tool_event_audit_projection(&AgentProviderSemanticEventV1::ToolRequest {
            call_id: "call-1".into(),
            tool: tool(),
            request: request.clone(),
        })
        .expect("Tool request audit");
        assert_eq!(requested.action, "agent.execution.tool-requested");
        assert_eq!(
            requested.expected_kind,
            AgentExecutionEventKind::ToolRequest
        );
        assert_eq!(
            requested.details,
            serde_json::json!({
                "callId": "call-1",
                "tool": tool(),
                "request": request,
            })
        );

        let recorded = tool_event_audit_projection(&AgentProviderSemanticEventV1::ToolResult {
            call_id: "call-1".into(),
            tool: tool(),
            request_digest: format!("sha256:{}", "b".repeat(64)),
            outcome: AgentProviderToolResultOutcomeV1::Succeeded,
            result: payload('c', 256),
        })
        .expect("Tool result audit");
        assert_eq!(recorded.action, "agent.execution.tool-result-recorded");
        assert_eq!(recorded.expected_kind, AgentExecutionEventKind::ToolResult);
        let encoded = serde_json::to_string(&recorded.details).expect("Tool audit JSON");
        for forbidden in ["secretMaterial", "payload", "body", "value"] {
            assert!(
                !encoded.contains(forbidden),
                "Tool audit exposed forbidden field {forbidden}"
            );
        }
    }
}
