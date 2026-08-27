use super::*;
use crate::modules::agents::domain::{
    AgentExecutionCheckpoint, AgentExecutionCheckpointCommitted, AgentExecutionForked,
    CommitAgentExecutionCheckpointWrite, ForkAgentExecutionWrite,
    IAgentExecutionCheckpointRepository,
};

#[tokio::test]
async fn fork_start_materializes_and_verifies_the_checkpoint_trajectory() {
    let now = canonical_timestamp(Utc::now() - Duration::seconds(5));
    let organization_id = OrganizationId::new();
    let node_id = NodeId::new();
    let agents = Arc::new(InMemoryAgentRepository::new());
    let (parent, parent_binding) =
        prepare_bound_execution(agents.as_ref(), organization_id, node_id, now).await;
    let conversation = agents
        .find_conversation(organization_id, parent.conversation_id)
        .await
        .expect("find conversation")
        .expect("parent conversation");
    let trajectory = agents
        .list_execution_trajectory_events(organization_id, parent.id, None, None, 10)
        .await
        .expect("parent trajectory");
    let captured = AgentExecutionCheckpoint::capture(&conversation, &parent, &trajectory)
        .expect("capture checkpoint");
    let mut tampered_snapshot = captured.snapshot.clone();
    tampered_snapshot.events[0].content = serde_json::json!({"prompt": "tampered"});
    let tampered_bytes = serde_json::to_vec(&tampered_snapshot).expect("tampered checkpoint JSON");
    let objects = Arc::new(TestCheckpointObjects::default());
    objects
        .put(&captured.checkpoint.object, captured.bytes.clone())
        .await
        .expect("store checkpoint object");
    let checkpoint_event =
        AgentExecutionCheckpointCommitted::envelope(&captured.checkpoint, Uuid::now_v7())
            .expect("checkpoint event");
    let checkpoint = agents
        .commit_execution_checkpoint(CommitAgentExecutionCheckpointWrite {
            checkpoint: captured.checkpoint,
            event: checkpoint_event,
            idempotency: idempotency("agent-flow-checkpoints", "capture", b"capture"),
        })
        .await
        .expect("commit checkpoint")
        .checkpoint;

    let fork = AgentExecution::fork_from(
        &parent,
        &checkpoint,
        AgentExecutionId::new(),
        OperationId::new(),
        now + Duration::seconds(2),
    )
    .expect("fork execution");
    let fork_input = serde_json::json!({"prompt": "continue from the checkpoint"});
    let initial_event = AgentExecutionEventDraft::new(
        AgentExecutionEventKind::ExecutionRequested,
        AgentEventContent::inline_json(fork_input.clone()).expect("fork input"),
        fork.requested_at,
    )
    .expect("fork initial event");
    let fork_event = AgentExecutionForked::envelope(&fork, Uuid::now_v7()).expect("fork event");
    let fork = agents
        .fork_execution(ForkAgentExecutionWrite {
            execution: fork,
            initial_event,
            event: fork_event,
            idempotency: idempotency("agent-flow-checkpoint-forks", "fork", b"fork"),
        })
        .await
        .expect("commit fork")
        .execution;
    let fork_binding = AgentCodeRunBinding::new_with_provider(
        fork.provider.clone(),
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
            protocol: fork.provider.native_protocol().into(),
            agent_release_identity: fork.agent.artifact_digest().as_str().into(),
            session_id: format!("agent-conversation-{}", fork.conversation_id),
            run_id: format!("agent-execution-{}", fork.id),
        },
        fork.requested_at + Duration::milliseconds(1),
    )
    .expect("fork provider Runtime binding")
    .with_invocation_profile(
        parent_binding
            .require_invocation_profile()
            .expect("parent invocation profile")
            .clone(),
    )
    .expect("fork invocation profile");
    let fork = agents
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id,
            execution_id: fork.id,
            binding: fork_binding,
        })
        .await
        .expect("bind fork provider Runtime")
        .execution;
    let runtime = AgentExecutionFlowRuntime::new(
        AgentExecutionFlowRuntimeDependencies {
            agents,
            checkpoint_objects: objects.clone(),
            providers: Arc::new(
                BuiltInAgentExecutionProviderRegistry::new().expect("provider registry"),
            ),
            workload_targets: Arc::new(InMemoryWorkloadRepository::new()),
            node_control: Arc::new(InMemoryNodeRepository::new()),
        },
        AgentExecutionFlowConfig::new(AgentExecutionFlowConfigOptions {
            heartbeat_timeout_ms: 60_000,
            command_ttl_ms: 60_000,
            observation_poll_ms: 1,
            convergence_timeout_ms: 60_000,
        })
        .expect("Agent Flow configuration"),
    );

    let command = super::super::runtime::start_command(&runtime, &fork)
        .await
        .expect("fork start command");
    let AgentProviderCommandV1::Start { request } = command else {
        panic!("fork must start a new provider run");
    };
    let prompt: serde_json::Value =
        serde_json::from_str(&request.prompt).expect("fork prompt JSON");
    assert_eq!(prompt["schema"], "a3s.cloud.agent-execution-fork-prompt.v1");
    assert_eq!(prompt["parentExecutionId"], parent.id.to_string());
    assert_eq!(prompt["parentCheckpointId"], checkpoint.id.to_string());
    assert_eq!(
        prompt["parentCheckpointDigest"],
        checkpoint.object.digest.as_str()
    );
    assert_eq!(
        prompt["throughEventSequence"],
        checkpoint.through_event_sequence
    );
    assert_eq!(prompt["input"], fork_input);
    assert_eq!(
        prompt["trajectory"]
            .as_array()
            .expect("checkpoint trajectory")
            .len(),
        1
    );

    objects
        .bodies
        .write()
        .await
        .insert(checkpoint.object.object_ref.clone(), tampered_bytes);
    let error = super::super::runtime::start_command(&runtime, &fork)
        .await
        .expect_err("tampered checkpoint must fail closed");
    assert!(error
        .to_string()
        .contains("could not verify Agent fork checkpoint object"));
}
