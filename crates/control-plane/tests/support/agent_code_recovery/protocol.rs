async fn run_step(
    runtime: &AgentExecutionFlowRuntime,
    run_id: &str,
    step_name: &str,
    input: Value,
) -> TestResult<Value> {
    runtime
        .run_step(StepInvocation::new(
            run_id,
            format!("{step_name}-{}", Uuid::now_v7()),
            step_name,
            input,
            Vec::new(),
        ))
        .await
        .map_err(|error| invalid(format!("Agent Flow step {step_name} failed: {error}")).into())
}

fn ready_field<'a>(output: &'a Value, field: &str) -> TestResult<&'a Value> {
    if output.get("state").and_then(Value::as_str) != Some("ready") {
        return Err(invalid(format!("Agent Flow did not return ready: {output}")).into());
    }
    output
        .get(field)
        .ok_or_else(|| invalid(format!("Agent Flow ready output omitted {field}")))
        .map_err(Into::into)
}

fn pending_dispatched(output: &Value) -> TestResult<&Value> {
    if output.get("state").and_then(Value::as_str) != Some("pending") {
        return Err(invalid(format!("Agent Flow did not remain pending: {output}")).into());
    }
    output
        .get("dispatched")
        .ok_or_else(|| invalid("Agent Flow recovery did not return its durable dispatch"))
        .map_err(Into::into)
}

fn assert_pending_without_dispatch(output: &Value) -> TestResult {
    if output.get("state").and_then(Value::as_str) != Some("pending")
        || output.get("dispatched").is_some()
    {
        return Err(invalid(format!(
            "Agent Flow did not settle to a pending observation: {output}"
        ))
        .into());
    }
    Ok(())
}

async fn lease_and_ack_code_command(
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    after_sequence: u64,
    expected: ExpectedCommand,
    state: AgentProtocolRunStateV1,
) -> TestResult<NodeCommandEnvelope> {
    let now = canonical_timestamp(Utc::now());
    let lease = nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence,
                max_commands: 1,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + Duration::seconds(10),
        )
        .await?;
    if lease.commands.len() != 1 {
        return Err(invalid(format!(
            "Fleet leased {} Agent Code commands after sequence {after_sequence}",
            lease.commands.len()
        ))
        .into());
    }
    let envelope = lease
        .commands
        .into_iter()
        .next()
        .ok_or_else(|| invalid("Fleet omitted the sole leased Agent Code command"))?;
    let NodeCommandPayload::CodeAgentCommand { command, .. } = &envelope.payload else {
        return Err(invalid("Fleet leased a non-Code command for Agent recovery").into());
    };
    let kind_matches = matches!(
        (expected, command.as_ref()),
        (ExpectedCommand::Start, AgentProtocolCommandV1::Start { .. })
            | (
                ExpectedCommand::Recover,
                AgentProtocolCommandV1::Recover { .. }
            )
            | (
                ExpectedCommand::Cancel,
                AgentProtocolCommandV1::Cancel { .. }
            )
    );
    if !kind_matches {
        return Err(invalid(format!(
            "Fleet leased the wrong Agent Code command: {:?}",
            command.action()
        ))
        .into());
    }
    let completed_at = canonical_timestamp(Utc::now()).max(
        envelope
            .issued_at
            .checked_add_signed(Duration::milliseconds(1))
            .ok_or_else(|| invalid("Agent Code acknowledgement time overflowed"))?,
    );
    let receipt = AgentProtocolCommandReceiptV1 {
        schema: AgentProtocolCommandReceiptV1::SCHEMA.into(),
        action: command.action(),
        request_id: command.request_id().into(),
        identity: command.identity().clone(),
        command_digest: command.digest()?,
        state,
        latest_event_sequence_exclusive: 0,
        observed_at_ms: u64::try_from(completed_at.timestamp_millis())?,
        replayed: false,
    };
    receipt.validate_for(command)?;
    nodes
        .acknowledge_command(
            NodeCommandAck {
                schema: NodeCommandAck::SCHEMA.into(),
                command_id: envelope.command_id,
                lease_id: envelope.lease_id,
                node_id: envelope.node_id,
                sequence: envelope.sequence,
                payload_digest: envelope.payload_digest.clone(),
                completed_at,
                outcome: NodeCommandOutcome::Succeeded {
                    result: Box::new(NodeCommandResult::CodeAgentCommandAccepted {
                        receipt: Box::new(receipt),
                    }),
                },
            },
            completed_at,
        )
        .await?;
    Ok(envelope)
}

fn command_identity(command: &NodeCommandEnvelope) -> TestResult<&AgentProtocolRunIdentityV1> {
    let NodeCommandPayload::CodeAgentCommand { command, .. } = &command.payload else {
        return Err(invalid("Fleet command is not an Agent Code command").into());
    };
    Ok(command.identity())
}

fn recovery_identity(command: &NodeCommandEnvelope) -> TestResult<(String, String)> {
    let NodeCommandPayload::CodeAgentCommand { command, .. } = &command.payload else {
        return Err(invalid("Fleet recovery is not an Agent Code command").into());
    };
    let AgentProtocolCommandV1::Recover { request } = command.as_ref() else {
        return Err(invalid("Fleet command is not an Agent Code recovery").into());
    };
    Ok((
        request.identity.run_id.clone(),
        request.checkpoint_run_id.clone(),
    ))
}

fn event_record(
    identity: &AgentProtocolRunIdentityV1,
    sequence: u64,
    occurred_at_ms: u64,
) -> TestResult<AgentProtocolEventRecordV1> {
    Ok(serde_json::from_value(json!({
        "sequence": sequence,
        "occurred_at_ms": occurred_at_ms,
        "event": {
            "version": 1,
            "type": "text_delta",
            "payload": {"text": format!("event-{sequence}")},
            "metadata": {
                "session_id": identity.session_id,
                "run_id": identity.run_id,
                "sequence": sequence,
                "timestamp_ms": occurred_at_ms,
            }
        }
    }))?)
}

fn event_batch(
    execution_id: AgentExecutionId,
    binding: &AgentCodeRunBinding,
    batch_id: Uuid,
    state: AgentProtocolRunStateV1,
    event_count: u64,
) -> TestResult<NodeCodeAgentEventBatchV1> {
    let bound_at_ms = u64::try_from(binding.bound_at().timestamp_millis())?;
    let events = (0..event_count)
        .map(|sequence| event_record(binding.identity(), sequence, bound_at_ms + sequence + 1))
        .collect::<TestResult<Vec<_>>>()?;
    let observed_at_ms = bound_at_ms + event_count + 1;
    let page = AgentProtocolEventPageV1 {
        schema: AgentProtocolEventPageV1::SCHEMA.into(),
        identity: binding.identity().clone(),
        after_event_sequence: None,
        first_available_sequence: (!events.is_empty()).then_some(0),
        latest_sequence_exclusive: event_count,
        next_after_event_sequence: events.last().map(|event| event.sequence),
        state,
        observed_at_ms,
        retention_gap: false,
        has_more: false,
        events,
    };
    page.validate()?;
    let batch = NodeCodeAgentEventBatchV1 {
        schema: NodeCodeAgentEventBatchV1::SCHEMA.into(),
        batch_id,
        node_id: binding.node_id().as_uuid(),
        binding: binding.node_runtime_binding(execution_id.as_uuid()),
        page,
        change_set: None,
        sent_at_ms: observed_at_ms + 1,
    };
    batch.validate()?;
    Ok(batch)
}

fn retention_gap_batch(
    execution_id: AgentExecutionId,
    checkpoint: &AgentCodeRunBinding,
    observed_at_ms: u64,
) -> TestResult<NodeCodeAgentEventBatchV1> {
    let page = AgentProtocolEventPageV1 {
        schema: AgentProtocolEventPageV1::SCHEMA.into(),
        identity: checkpoint.identity().clone(),
        after_event_sequence: Some(0),
        first_available_sequence: Some(2),
        latest_sequence_exclusive: 3,
        next_after_event_sequence: Some(2),
        state: AgentProtocolRunStateV1::Executing,
        observed_at_ms,
        retention_gap: true,
        has_more: false,
        events: vec![event_record(checkpoint.identity(), 2, observed_at_ms)?],
    };
    page.validate()?;
    code_event_batch(execution_id, checkpoint, page)
}

fn stale_checkpoint_batch(
    execution_id: AgentExecutionId,
    checkpoint: &AgentCodeRunBinding,
    observed_at_ms: u64,
) -> TestResult<NodeCodeAgentEventBatchV1> {
    let page = AgentProtocolEventPageV1 {
        schema: AgentProtocolEventPageV1::SCHEMA.into(),
        identity: checkpoint.identity().clone(),
        after_event_sequence: Some(0),
        first_available_sequence: Some(0),
        latest_sequence_exclusive: 2,
        next_after_event_sequence: Some(1),
        state: AgentProtocolRunStateV1::Executing,
        observed_at_ms,
        retention_gap: false,
        has_more: false,
        events: vec![event_record(checkpoint.identity(), 1, observed_at_ms)?],
    };
    page.validate()?;
    code_event_batch(execution_id, checkpoint, page)
}

fn code_event_batch(
    execution_id: AgentExecutionId,
    binding: &AgentCodeRunBinding,
    page: AgentProtocolEventPageV1,
) -> TestResult<NodeCodeAgentEventBatchV1> {
    let sent_at_ms = page
        .observed_at_ms
        .checked_add(1)
        .ok_or_else(|| invalid("Code event batch timestamp overflowed"))?;
    let batch = NodeCodeAgentEventBatchV1 {
        schema: NodeCodeAgentEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: binding.node_id().as_uuid(),
        binding: binding.node_runtime_binding(execution_id.as_uuid()),
        page,
        change_set: None,
        sent_at_ms,
    };
    batch.validate()?;
    Ok(batch)
}

fn accepted_at(batch: &NodeCodeAgentEventBatchV1) -> TestResult<DateTime<Utc>> {
    let milliseconds = i64::try_from(
        batch
            .sent_at_ms
            .checked_add(1)
            .ok_or_else(|| invalid("Code event acceptance timestamp overflowed"))?,
    )?;
    DateTime::from_timestamp_millis(milliseconds)
        .ok_or_else(|| invalid("Code event acceptance timestamp is invalid").into())
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn idempotency(scope: &str, key: &str, body: &[u8]) -> Result<IdempotencyRequest, String> {
    IdempotencyRequest::new(scope, key, body)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
