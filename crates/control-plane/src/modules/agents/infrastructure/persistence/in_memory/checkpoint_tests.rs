use super::tests::{code_binding, conversation, create_conversation, execution, start_execution};
use super::*;
use crate::modules::agents::domain::{
    AgentCodeRunBinding, AgentEventContent, AgentExecution, AgentExecutionCheckpoint,
    AgentExecutionCheckpointCommitted, AgentExecutionCheckpointObjectCaptureReservation,
    AgentExecutionCheckpointObjectReconcileDisposition, AgentExecutionEvent,
    AgentExecutionEventDraft, AgentExecutionEventKind, AgentExecutionForked, BindAgentCodeRunWrite,
    ClaimExpiredAgentExecutionCheckpointObjectsWrite, CommitAgentExecutionCheckpointWrite,
    CompleteAgentExecutionCheckpointObjectCleanupWrite, ForkAgentExecutionWrite,
    IAgentExecutionCheckpointRepository, ReconcileAgentExecutionCheckpointObjectWrite,
    RecoverAgentCodeRunWrite, ReserveAgentExecutionCheckpointObjectWrite,
    MAX_INLINE_AGENT_EVENT_BYTES,
};
use crate::modules::shared_kernel::domain::{
    AgentExecutionId, IdempotencyRequest, NodeId, OperationId,
};
use a3s_cloud_contracts::{
    AgentProtocolRunIdentityV1, AgentProviderCapabilityV1, HarnessAgentReleaseBindingV1,
    HarnessInvocationProfileV1, HarnessProviderBindingV1, HarnessWorkspaceBindingV1,
};
use chrono::Duration;
use uuid::Uuid;

fn idempotency(scope: &str, key: &str, body: &[u8]) -> IdempotencyRequest {
    IdempotencyRequest::new(scope, key, body).expect("idempotency")
}

fn checkpoint_binding(
    execution: &AgentExecution,
    node_id: NodeId,
    bound_at: chrono::DateTime<chrono::Utc>,
) -> AgentCodeRunBinding {
    let binding = code_binding(execution, node_id, bound_at);
    let mut required_capabilities = vec![
        AgentProviderCapabilityV1::Cancellation,
        AgentProviderCapabilityV1::Cleanup,
        AgentProviderCapabilityV1::EventPages,
    ];
    required_capabilities.sort_by_key(|capability| capability.as_str());
    let invocation = HarnessInvocationProfileV1 {
        schema: HarnessInvocationProfileV1::SCHEMA.into(),
        agent: HarnessAgentReleaseBindingV1 {
            organization_id: execution.organization_id.as_uuid(),
            asset_id: execution.agent.asset_id().as_uuid(),
            asset_release_id: execution.agent.asset_release_id().as_uuid(),
            build_run_id: execution.agent.build_run_id().as_uuid(),
            artifact_digest: execution.agent.artifact_digest().as_str().into(),
        },
        provider: HarnessProviderBindingV1 {
            kind: execution.provider.kind().into(),
            revision: execution.provider.revision().into(),
            profile_digest: execution.provider.profile_digest().into(),
            capability_digest: execution.provider.capability_digest().into(),
        },
        instructions_digest: execution.agent.artifact_digest().as_str().into(),
        environment_policy_digest: format!("sha256:{}", "c".repeat(64)),
        security_policy_digest: format!("sha256:{}", "d".repeat(64)),
        workspace: HarnessWorkspaceBindingV1 {
            workload_id: binding.workload_id().as_uuid(),
            workload_revision_id: binding.workload_revision_id().as_uuid(),
            runtime_unit_id: binding.runtime_unit_id().into(),
            runtime_generation: binding.runtime_generation(),
            runtime_spec_digest: binding.runtime_spec_digest().as_str().into(),
            working_directory: Some("/workspace".into()),
        },
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        models: Vec::new(),
        secrets: Vec::new(),
        tools: Vec::new(),
        required_capabilities,
    };
    binding
        .with_invocation_profile(invocation)
        .expect("checkpoint Harness invocation profile")
}

#[tokio::test]
async fn checkpoint_commit_and_fork_preserve_immutable_lineage() {
    let repository = InMemoryAgentRepository::new();
    let conversation = create_conversation(&repository, conversation()).await;
    let parent = execution(&conversation);
    start_execution(&repository, parent.clone()).await;
    let binding = checkpoint_binding(
        &parent,
        NodeId::new(),
        parent.requested_at + Duration::milliseconds(1),
    );
    let parent = repository
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id: parent.organization_id,
            execution_id: parent.id,
            binding,
        })
        .await
        .expect("bind provider Runtime")
        .execution;
    let trajectory = repository
        .list_execution_trajectory_events(parent.organization_id, parent.id, None, None, 10)
        .await
        .expect("parent trajectory");
    let captured = AgentExecutionCheckpoint::capture(&conversation, &parent, &trajectory)
        .expect("capture checkpoint");
    let repeated = AgentExecutionCheckpoint::capture(&conversation, &parent, &trajectory)
        .expect("repeat deterministic capture");
    assert_eq!(repeated.checkpoint.id, captured.checkpoint.id);
    assert_eq!(repeated.checkpoint.object, captured.checkpoint.object);
    assert_eq!(repeated.bytes, captured.bytes);

    let mut non_canonical_telemetry = captured.snapshot.telemetry_correlation.clone();
    non_canonical_telemetry.runtime_unit_id =
        format!(" {} ", non_canonical_telemetry.runtime_unit_id);
    assert!(non_canonical_telemetry.validate().is_err());

    let large_content = AgentEventContent::inline_json(serde_json::Value::String(
        "x".repeat(MAX_INLINE_AGENT_EVENT_BYTES - 2),
    ))
    .expect("maximum inline event content");
    let mut oversized_trajectory = trajectory.clone();
    for sequence in 2_u64..=15 {
        oversized_trajectory.push(AgentExecutionEvent {
            organization_id: parent.organization_id,
            conversation_id: parent.conversation_id,
            execution_id: parent.id,
            sequence,
            kind: AgentExecutionEventKind::ModelOutput,
            content: large_content.clone(),
            occurred_at: parent.requested_at
                + Duration::milliseconds(i64::try_from(sequence).expect("event sequence")),
        });
    }
    assert!(
        AgentExecutionCheckpoint::capture(&conversation, &parent, &oversized_trajectory)
            .expect_err("oversized checkpoint")
            .contains("snapshot bound")
    );

    let mut tampered = captured.snapshot.clone();
    tampered.events[0].content = serde_json::json!({"prompt": "tampered"});
    assert!(captured.checkpoint.validate_snapshot(&tampered).is_err());

    let checkpoint_event =
        AgentExecutionCheckpointCommitted::envelope(&captured.checkpoint, Uuid::now_v7())
            .expect("checkpoint event");
    let committed_at = captured.checkpoint.captured_at + Duration::seconds(1);
    let first_object_lease = match repository
        .reserve_execution_checkpoint_object(ReserveAgentExecutionCheckpointObjectWrite {
            checkpoint: captured.checkpoint.clone(),
            reserved_at: committed_at,
            lease_duration: Duration::minutes(5),
        })
        .await
        .expect("reserve checkpoint object")
    {
        AgentExecutionCheckpointObjectCaptureReservation::Reserved(lease) => lease,
        AgentExecutionCheckpointObjectCaptureReservation::Committed(_) => {
            panic!("new checkpoint cannot already be committed")
        }
    };
    assert!(matches!(
        repository
            .reconcile_execution_checkpoint_object(ReconcileAgentExecutionCheckpointObjectWrite {
                reference: captured.checkpoint.object.clone(),
                observed_at: committed_at + Duration::seconds(1),
                orphan_grace: Duration::hours(1),
                cleanup_lease_duration: Duration::minutes(5),
            },)
            .await
            .expect("active capture inventory"),
        AgentExecutionCheckpointObjectReconcileDisposition::Deferred { .. }
    ));
    let cleanup_at = committed_at + Duration::minutes(6);
    let cleanup_claims = repository
        .claim_expired_execution_checkpoint_objects(
            ClaimExpiredAgentExecutionCheckpointObjectsWrite {
                claimed_at: cleanup_at,
                cleanup_lease_duration: Duration::minutes(5),
                limit: 10,
            },
        )
        .await
        .expect("claim expired capture");
    assert_eq!(cleanup_claims.len(), 1);
    assert!(repository
        .commit_execution_checkpoint(CommitAgentExecutionCheckpointWrite {
            checkpoint: captured.checkpoint.clone(),
            event: checkpoint_event.clone(),
            idempotency: idempotency("agent-checkpoints", "stale-capture", b"stale-capture"),
            object_lease_id: Some(first_object_lease.lease_id),
            committed_at: cleanup_at,
        })
        .await
        .is_err());
    assert!(repository
        .reserve_execution_checkpoint_object(ReserveAgentExecutionCheckpointObjectWrite {
            checkpoint: captured.checkpoint.clone(),
            reserved_at: cleanup_at + Duration::minutes(6),
            lease_duration: Duration::minutes(5),
        })
        .await
        .is_err());
    repository
        .complete_execution_checkpoint_object_cleanup(
            CompleteAgentExecutionCheckpointObjectCleanupWrite {
                lease: cleanup_claims[0].clone(),
                completed_at: cleanup_at + Duration::minutes(6),
            },
        )
        .await
        .expect("complete orphan cleanup");
    let committed_at = cleanup_at + Duration::minutes(6) + Duration::seconds(1);
    let object_lease = match repository
        .reserve_execution_checkpoint_object(ReserveAgentExecutionCheckpointObjectWrite {
            checkpoint: captured.checkpoint.clone(),
            reserved_at: committed_at,
            lease_duration: Duration::minutes(5),
        })
        .await
        .expect("reserve checkpoint object after cleanup")
    {
        AgentExecutionCheckpointObjectCaptureReservation::Reserved(lease) => lease,
        AgentExecutionCheckpointObjectCaptureReservation::Committed(_) => {
            panic!("cleaned checkpoint cannot already be committed")
        }
    };
    let checkpoint_write = || CommitAgentExecutionCheckpointWrite {
        checkpoint: captured.checkpoint.clone(),
        event: checkpoint_event.clone(),
        idempotency: idempotency("agent-checkpoints", "capture", b"capture"),
        object_lease_id: Some(object_lease.lease_id),
        committed_at,
    };
    let committed = repository
        .commit_execution_checkpoint(checkpoint_write())
        .await
        .expect("commit checkpoint");
    assert!(!committed.replayed);
    let mut rebound_object = committed.checkpoint.clone();
    rebound_object.object.object_ref = rebound_object.object.object_ref.replace(
        &parent.organization_id.to_string(),
        &Uuid::now_v7().to_string(),
    );
    assert!(rebound_object.validate().is_err());
    assert!(
        repository
            .commit_execution_checkpoint(checkpoint_write())
            .await
            .expect("replay checkpoint")
            .replayed
    );
    let mut adopted_write = checkpoint_write();
    adopted_write.idempotency =
        idempotency("agent-checkpoints", "adopt-existing", b"adopt-existing");
    assert!(
        repository
            .commit_execution_checkpoint(adopted_write)
            .await
            .expect("adopt existing deterministic checkpoint")
            .replayed
    );
    assert_eq!(
        repository
            .list_execution_checkpoints(parent.organization_id, parent.id, 10)
            .await
            .expect("list checkpoints"),
        vec![committed.checkpoint.clone()]
    );

    let fork = AgentExecution::fork_from(
        &parent,
        &committed.checkpoint,
        AgentExecutionId::new(),
        OperationId::new(),
        parent.requested_at + Duration::milliseconds(2),
    )
    .expect("fork execution");
    let initial_event = AgentExecutionEventDraft::new(
        AgentExecutionEventKind::ExecutionRequested,
        AgentEventContent::inline_json(serde_json::json!({"prompt": "branch"}))
            .expect("fork input"),
        fork.requested_at,
    )
    .expect("fork initial event");
    let fork_event = AgentExecutionForked::envelope(&fork, Uuid::now_v7()).expect("fork event");
    let fork_write = || ForkAgentExecutionWrite {
        execution: fork.clone(),
        initial_event: initial_event.clone(),
        event: fork_event.clone(),
        idempotency: idempotency("agent-checkpoint-forks", "fork", b"fork"),
    };
    let forked = repository
        .fork_execution(fork_write())
        .await
        .expect("commit fork");
    assert!(!forked.replayed);
    assert_eq!(
        forked
            .execution
            .lineage
            .as_ref()
            .expect("fork lineage")
            .parent_checkpoint_id,
        committed.checkpoint.id
    );
    assert_eq!(
        repository
            .find_execution(parent.organization_id, parent.id)
            .await
            .expect("find parent")
            .expect("parent execution"),
        parent
    );
    assert!(
        repository
            .fork_execution(fork_write())
            .await
            .expect("replay fork")
            .replayed
    );
    let fork_trajectory = repository
        .list_execution_trajectory_events(
            forked.execution.organization_id,
            forked.execution.id,
            None,
            None,
            10,
        )
        .await
        .expect("fork trajectory");
    assert_eq!(fork_trajectory.len(), 1);
    assert_eq!(fork_trajectory[0].execution_id, forked.execution.id);
    assert_eq!(
        fork_trajectory[0].kind,
        AgentExecutionEventKind::ExecutionRequested
    );
    let parent_binding = parent.code.as_ref().expect("parent provider Runtime");
    let fork_binding = AgentCodeRunBinding::new_with_provider(
        forked.execution.provider.clone(),
        parent_binding.node_id(),
        parent_binding.workload_id(),
        parent_binding.workload_revision_id(),
        parent_binding.deployment_id(),
        parent_binding.replica_id(),
        parent_binding.runtime_unit_id(),
        parent_binding.runtime_generation(),
        parent_binding.runtime_spec_digest().clone(),
        parent_binding.service_port_name(),
        AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: forked.execution.provider.native_protocol().into(),
            agent_release_identity: forked.execution.agent.artifact_digest().as_str().into(),
            session_id: format!("conversation-{}", forked.execution.conversation_id),
            run_id: format!("execution-{}-attempt-1", forked.execution.id),
        },
        forked.execution.requested_at + Duration::milliseconds(1),
    )
    .expect("fork provider Runtime binding")
    .with_invocation_profile(
        parent_binding
            .require_invocation_profile()
            .expect("parent invocation profile")
            .clone(),
    )
    .expect("fork invocation profile");
    let fork_execution = repository
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id: forked.execution.organization_id,
            execution_id: forked.execution.id,
            binding: fork_binding,
        })
        .await
        .expect("bind fork provider Runtime")
        .execution;
    assert!(
        AgentExecutionCheckpoint::capture(&conversation, &fork_execution, &fork_trajectory,)
            .expect_err("fork checkpoint without inherited trajectory")
            .contains("inherited trajectory")
    );
    let fork_checkpoint = AgentExecutionCheckpoint::capture_with_parent(
        &conversation,
        &fork_execution,
        &fork_trajectory,
        &committed.checkpoint,
        &captured.snapshot,
    )
    .expect("capture materialized fork checkpoint");
    assert_eq!(fork_checkpoint.snapshot.events.len(), 2);
    assert_eq!(
        fork_checkpoint.snapshot.events[0].sequence,
        trajectory[0].sequence
    );
    assert_eq!(
        fork_checkpoint.snapshot.events[1].sequence,
        fork_trajectory[0].sequence
    );
    assert_eq!(repository.outbox_events().await.len(), 4);

    repository
        .recover_code_run(RecoverAgentCodeRunWrite {
            organization_id: parent.organization_id,
            execution_id: parent.id,
            expected_binding: parent.code.clone().expect("parent provider Runtime"),
            recovered_at: parent.updated_at + Duration::seconds(1),
        })
        .await
        .expect("recover parent provider Runtime");
    let mut historical_replay = checkpoint_write();
    historical_replay.idempotency = idempotency(
        "agent-checkpoints",
        "adopt-existing-after-recovery",
        b"adopt-existing-after-recovery",
    );
    assert!(
        repository
            .commit_execution_checkpoint(historical_replay)
            .await
            .expect("adopt immutable checkpoint after Runtime recovery")
            .replayed
    );
}
