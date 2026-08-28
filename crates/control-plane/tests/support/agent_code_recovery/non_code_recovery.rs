pub async fn exercise_non_code_provider_recovery_fallback(
    postgres_url: String,
) -> TestResult {
    let StartedProviderScenario {
        state,
        agents: initial_agents,
        initial_binding,
    } = prepare_started_provider_scenario(
        &postgres_url,
        REFERENCE_ECHO_AGENT_PROVIDER_KIND,
    )
    .await?;
    let initial_profile = initial_binding.provider()?.profile()?;
    assert_eq!(initial_profile.kind(), REFERENCE_ECHO_AGENT_PROVIDER_KIND);
    assert!(!initial_profile.supports(AgentProviderCapabilityV1::Recovery));

    let output_batch = reference_provider_output_batch(state.execution_id, &initial_binding)?;
    let accepted_at = provider_batch_accepted_at(&output_batch)?;
    let output_receipt = initial_agents
        .accept_provider_event_batch(AcceptAgentProviderEventBatchWrite::new(
            state.organization_id,
            state.node_id,
            output_batch.clone(),
            accepted_at,
        )?)
        .await?;
    assert!(!output_receipt.receipt.replayed);
    assert_eq!(output_receipt.receipt.accepted_events, 1);
    let replayed_output = initial_agents
        .accept_provider_event_batch(AcceptAgentProviderEventBatchWrite::new(
            state.organization_id,
            state.node_id,
            output_batch,
            accepted_at,
        )?)
        .await?;
    assert!(replayed_output.receipt.replayed);
    assert_eq!(
        replayed_output.receipt.accepted_at_ms,
        output_receipt.receipt.accepted_at_ms
    );

    // Drop the original repository pool before reconnecting. The fallback must
    // reconstruct all provider, Flow, and Fleet evidence from PostgreSQL.
    drop(initial_agents);
    let executor = connect_postgres(&postgres_url, 8).await?;
    let agents = Arc::new(PostgresAgentRepository::new(executor.clone()));
    let nodes = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let workloads = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let restored = agents
        .find_execution(state.organization_id, state.execution_id)
        .await?
        .ok_or_else(|| invalid("non-Code execution disappeared across PostgreSQL reconnect"))?;
    let restored_binding = restored
        .code
        .as_ref()
        .ok_or_else(|| invalid("non-Code provider binding disappeared across reconnect"))?;
    assert!(restored_binding.has_same_run_binding(&initial_binding));
    assert_eq!(restored_binding.accepted_after_event_sequence(), Some(0));
    let restored_profile = restored_binding.provider()?.profile()?;
    assert_eq!(restored_profile.kind(), REFERENCE_ECHO_AGENT_PROVIDER_KIND);
    assert!(!restored_profile.supports(AgentProviderCapabilityV1::Recovery));

    let restarted_started_at_ms = state
        .initial_runtime_started_at_ms
        .checked_add(2_000)
        .ok_or_else(|| invalid("non-Code Runtime process timestamp overflowed"))?;
    let restarted_observed_at_ms = restarted_started_at_ms
        .checked_add(1)
        .ok_or_else(|| invalid("non-Code Runtime observation timestamp overflowed"))?;
    let restarted_received_at = canonical_timestamp(Utc::now())
        .max(state.initial_runtime_received_at + Duration::milliseconds(1));
    record_runtime_observation(
        nodes.as_ref(),
        state.node_id,
        state.agent_instance_id,
        &state.runtime_spec,
        &state.runtime_capabilities,
        RuntimeObservationTiming {
            started_at_ms: restarted_started_at_ms,
            observed_at_ms: restarted_observed_at_ms,
            received_at: restarted_received_at,
        },
    )
    .await?;

    let runtime = flow_runtime(agents.clone(), workloads, nodes)?;
    let terminal = run_step(
        &runtime,
        &state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": state.start_dispatched.clone()}),
    )
    .await?;
    assert_failed_terminal(&terminal)?;
    let replayed_terminal = run_step(
        &runtime,
        &state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": state.start_dispatched}),
    )
    .await?;
    assert_eq!(replayed_terminal, terminal);

    let events = agents
        .list_events(state.organization_id, state.conversation_id, None, 100)
        .await?;
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].kind, AgentExecutionEventKind::ExecutionRequested);
    assert_eq!(events[1].kind, AgentExecutionEventKind::ModelOutput);
    assert_eq!(
        events[1].content.value(),
        &json!({"text": "reference harness output"})
    );
    assert_eq!(events[2].kind, AgentExecutionEventKind::ExecutionFailed);
    assert_eq!(
        events[2].content.value(),
        &json!({"reason": "Agent provider does not support process recovery"})
    );
    let failed = agents
        .find_execution(state.organization_id, state.execution_id)
        .await?
        .ok_or_else(|| invalid("non-Code execution disappeared after fallback"))?;
    assert_eq!(failed.status, AgentExecutionStatus::Failed);
    assert!(failed
        .code
        .as_ref()
        .is_some_and(|binding| binding.has_same_run_binding(&initial_binding)));

    let database = Database::new(PostgresDialect, executor);
    let command_counts = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64)>(
                "select count(*), count(*) filter (where acknowledgement is not null), count(*) filter (where payload -> 'command' ->> 'action' = 'recover') from node_commands where node_id = ",
            )
            .bind(state.node_id.as_uuid())
            .append(" and command_kind = 'agent_provider_command'"),
        )
        .await?;
    assert_eq!(command_counts, (1, 1, 0));

    println!(
        "A3S_CLOUD_A1_NON_CODE_POSTGRES_FALLBACK_CERTIFIED store=postgresql provider=reference.echo control_plane_restarts=1 provider_process_restarts=1 recovery_capability=unsupported fallback=terminal recover_commands=0 semantic_events=3 replay=exact"
    );
    Ok(())
}

fn reference_provider_output_batch(
    execution_id: AgentExecutionId,
    binding: &AgentCodeRunBinding,
) -> TestResult<NodeAgentProviderEventBatchV1> {
    let observed_at_ms = u64::try_from(binding.bound_at().timestamp_millis())?
        .checked_add(1)
        .ok_or_else(|| invalid("reference provider event timestamp overflowed"))?;
    let page = AgentProviderEventPageV1 {
        schema: AgentProviderEventPageV1::SCHEMA.into(),
        identity: binding.provider_identity()?,
        after_event_sequence: None,
        first_available_sequence: Some(0),
        source_first_sequence: Some(0),
        source_last_sequence: Some(0),
        source_event_count: 1,
        latest_sequence_exclusive: 1,
        next_after_event_sequence: Some(0),
        state: AgentProviderRunStateV1::Executing,
        observed_at_ms,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: vec![AgentProviderEventRecordV1 {
            sequence: 0,
            occurred_at_ms: observed_at_ms,
            event: AgentProviderSemanticEventV1::ModelOutput {
                text: "reference harness output".into(),
            },
        }],
    };
    let sent_at_ms = observed_at_ms
        .checked_add(1)
        .ok_or_else(|| invalid("reference provider batch timestamp overflowed"))?;
    let batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: binding.node_id().as_uuid(),
        binding: binding.node_provider_runtime_binding(execution_id.as_uuid())?,
        page,
        sent_at_ms,
    };
    batch.validate()?;
    Ok(batch)
}

fn provider_batch_accepted_at(batch: &NodeAgentProviderEventBatchV1) -> TestResult<DateTime<Utc>> {
    let accepted_at_ms = batch
        .sent_at_ms
        .checked_add(1)
        .ok_or_else(|| invalid("reference provider acceptance timestamp overflowed"))?;
    DateTime::from_timestamp_millis(i64::try_from(accepted_at_ms)?)
        .ok_or_else(|| invalid("reference provider acceptance timestamp is invalid").into())
}

fn assert_failed_terminal(output: &Value) -> TestResult {
    if output.get("state").and_then(Value::as_str) != Some("terminal")
        || output
            .pointer("/completed/status")
            .and_then(Value::as_str)
            != Some("failed")
    {
        return Err(invalid(format!(
            "non-Code recovery fallback did not fail terminally: {output}"
        ))
        .into());
    }
    Ok(())
}
