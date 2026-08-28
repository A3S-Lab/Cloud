use super::*;

#[tokio::test]
async fn provider_restart_without_recovery_capability_fails_terminally_without_a_command() {
    let requested_at = canonical_timestamp(Utc::now() - Duration::seconds(5));
    let organization_id = OrganizationId::new();
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id) =
        enroll_command_node(nodes.as_ref(), organization_id, requested_at).await;
    let profile = AgentProviderProfile::parse_acl(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/a1.3/reference-echo-provider-profile.acl"
    )))
    .expect("reference provider profile");
    assert!(!profile.supports(AgentProviderCapabilityV1::Recovery));
    let provider =
        AgentProviderProfileBinding::from_profile(&profile).expect("reference provider binding");
    let agents = Arc::new(InMemoryAgentRepository::new());
    let (execution, binding) = prepare_bound_execution_with_provider(
        agents.as_ref(),
        organization_id,
        node_id,
        requested_at,
        provider,
    )
    .await;
    let runtime = AgentExecutionFlowRuntime::new(
        AgentExecutionFlowRuntimeDependencies {
            agents: agents.clone(),
            checkpoint_objects: checkpoint_objects(),
            providers: Arc::new(
                BuiltInAgentExecutionProviderRegistry::new().expect("provider registry"),
            ),
            workload_targets: Arc::new(InMemoryWorkloadRepository::new()),
            node_control: nodes.clone(),
        },
        AgentExecutionFlowConfig::new(AgentExecutionFlowConfigOptions {
            heartbeat_timeout_ms: 60_000,
            command_ttl_ms: 60_000,
            observation_poll_ms: 1,
            convergence_timeout_ms: 60_000,
        })
        .expect("Agent Flow configuration"),
    );
    let observed_at = canonical_timestamp(Utc::now());
    let restarted_at_ms = u64::try_from(observed_at.timestamp_millis())
        .expect("Runtime process timestamp")
        .saturating_sub(1);
    let prepared = PreparedAgentExecution {
        organization_id,
        execution_id: execution.id,
        binding: binding.clone(),
        runtime_started_at_ms: Some(restarted_at_ms.saturating_sub(1_000)),
    };
    let dispatched = match super::super::runtime::dispatch(
        &runtime,
        &execution.operation_id.to_string(),
        DispatchInput {
            prepared: Box::new(prepared),
        },
    )
    .await
    .expect("dispatch reference provider start")
    {
        DispatchOutput::Ready { dispatched } => *dispatched,
        DispatchOutput::Terminal { .. } => panic!("active execution must dispatch"),
    };
    let start = lease_and_ack_code_command(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        0,
        AgentCommandKind::Start,
    )
    .await;
    record_running_observation(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        &binding,
        restarted_at_ms,
        observed_at,
    )
    .await;

    let completed = match super::super::runtime::observe(
        &runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(dispatched.clone()),
        },
    )
    .await
    .expect("fail unsupported provider recovery closed")
    {
        ObserveOutput::Terminal { completed } => completed,
        ObserveOutput::Pending { .. } => {
            panic!("unsupported provider recovery must reach a terminal state")
        }
    };
    assert_eq!(
        completed.status,
        crate::modules::agents::domain::AgentExecutionStatus::Failed
    );
    let stored = agents
        .find_execution(organization_id, execution.id)
        .await
        .expect("find failed execution")
        .expect("failed execution");
    assert_eq!(stored.status, completed.status);
    assert!(stored
        .code
        .as_ref()
        .is_some_and(|current| current.has_same_run_binding(&binding)));
    let events = agents
        .list_events(organization_id, execution.conversation_id, None, 100)
        .await
        .expect("list terminal events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].kind, AgentExecutionEventKind::ExecutionFailed);
    assert_eq!(
        events[1].content.value(),
        &serde_json::json!({
            "reason": "Agent provider does not support process recovery"
        })
    );

    let replayed = match super::super::runtime::observe(
        &runtime,
        &execution.operation_id.to_string(),
        ObserveInput {
            dispatched: Box::new(dispatched),
        },
    )
    .await
    .expect("replay unsupported provider recovery")
    {
        ObserveOutput::Terminal { completed } => completed,
        ObserveOutput::Pending { .. } => panic!("terminal recovery failure must replay exactly"),
    };
    assert_eq!(replayed, completed);
    assert_eq!(
        agents
            .list_events(organization_id, execution.conversation_id, None, 100,)
            .await
            .expect("list replayed terminal events"),
        events
    );

    let lease = nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence: start.sequence,
                max_commands: 1,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            observed_at,
            observed_at + Duration::seconds(10),
        )
        .await
        .expect("inspect commands after unsupported recovery");
    assert!(
        lease.commands.is_empty(),
        "unsupported provider recovery must not enqueue a Recover command"
    );
}
