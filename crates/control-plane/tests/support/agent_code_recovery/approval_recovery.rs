struct PendingApprovalScenario {
    state: StartedScenarioState,
    binding: AgentCodeRunBinding,
    checkpoint: AgentApprovalCheckpoint,
}

struct ApprovalRuntime {
    executor: PostgresExecutor,
    agents: Arc<PostgresAgentRepository>,
    nodes: Arc<PostgresNodeRepository>,
    runtime: AgentExecutionFlowRuntime,
}

#[derive(Clone, Default)]
struct RecordingApprovalAuthorizer {
    calls: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl IResourceAuthorizationDecisionRepository for RecordingApprovalAuthorizer {
    async fn authorize_resource(
        &self,
        request: ResourceAuthorizationDecisionRequest,
    ) -> Result<AuthorizationDecisionRef, RepositoryError> {
        if request.action != "agent.execution.approval.decide" {
            return Err(RepositoryError::Storage(
                "Agent approval fixture received another authorization action".into(),
            ));
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        AuthorizationDecisionRef::new(
            format!("agent-approval-postgres:{}", request.request_id),
            Sha256Digest::parse(format!("sha256:{}", "9".repeat(64)))
                .map_err(RepositoryError::Storage)?,
        )
        .map_err(RepositoryError::Storage)
    }
}

pub async fn exercise_agent_approval_postgres_recovery(postgres_url: String) -> TestResult {
    let approved = prepare_pending_approval(&postgres_url, "approve-1", false).await?;
    assert_pending_approval_survives_reconnect(&postgres_url, &approved).await?;
    let approved_checkpoint = decide_approval_across_reconnect(
        &postgres_url,
        &approved,
        AgentProviderApprovalOutcomeV1::Approved,
        "approve-release",
    )
    .await?;
    settle_approval_resolution(
        &postgres_url,
        approved,
        approved_checkpoint,
        AgentProviderApprovalOutcomeV1::Approved,
    )
    .await?;

    let denied = prepare_pending_approval(&postgres_url, "deny-1", false).await?;
    assert_pending_approval_survives_reconnect(&postgres_url, &denied).await?;
    let denied_checkpoint = decide_approval_across_reconnect(
        &postgres_url,
        &denied,
        AgentProviderApprovalOutcomeV1::Denied,
        "deny-release",
    )
    .await?;
    settle_approval_resolution(
        &postgres_url,
        denied,
        denied_checkpoint,
        AgentProviderApprovalOutcomeV1::Denied,
    )
    .await?;

    let expired = prepare_pending_approval(&postgres_url, "expire-1", true).await?;
    if expired.checkpoint.expires_at >= Utc::now() {
        return Err(invalid("aged Agent approval checkpoint did not expire").into());
    }
    let expired_checkpoint = expired.checkpoint.clone();
    settle_approval_resolution(
        &postgres_url,
        expired,
        expired_checkpoint,
        AgentProviderApprovalOutcomeV1::Expired,
    )
    .await?;

    exercise_pending_approval_cancellation(&postgres_url).await?;
    exercise_pending_approval_provider_restart(&postgres_url).await?;

    println!(
        "A3S_CLOUD_A1_APPROVAL_POSTGRES_RECOVERY_CERTIFIED store=postgresql provider=reference.echo approval_requests=5 approved=1 denied=1 expired=1 cancelled=1 provider_restart_fail_closed=1 resume_commands=3 cancel_commands=1 recover_commands=0 control_plane_reconnect=verified decision_replay=exact audit=digest_only"
    );
    Ok(())
}

async fn prepare_pending_approval(
    postgres_url: &str,
    call_id: &str,
    expired: bool,
) -> TestResult<PendingApprovalScenario> {
    let tool = HarnessToolBindingV1 {
        name: "workspace.publish".into(),
        revision: "1.0.0".into(),
        contract_digest: format!("sha256:{}", "e".repeat(64)),
        approval_required: true,
    };
    let requested_at = expired.then(|| canonical_timestamp(Utc::now() - Duration::hours(25)));
    let StartedProviderScenario {
        state,
        agents,
        initial_binding,
    } = prepare_started_provider_scenario_with_tools(
        postgres_url,
        REFERENCE_ECHO_AGENT_PROVIDER_KIND,
        vec![tool.clone()],
        requested_at,
    )
    .await?;
    let invocation = initial_binding.require_invocation_profile()?;
    assert_eq!(invocation.tools, vec![tool.clone()]);
    assert!(invocation
        .required_capabilities
        .contains(&AgentProviderCapabilityV1::ToolCalls));
    assert!(invocation
        .required_capabilities
        .contains(&AgentProviderCapabilityV1::PauseResume));

    let batch = approval_request_batch(state.execution_id, &initial_binding, call_id, tool)?;
    let accepted_at = provider_batch_accepted_at(&batch)?;
    let write = || {
        AcceptAgentProviderEventBatchWrite::new(
            state.organization_id,
            state.node_id,
            batch.clone(),
            accepted_at,
        )
    };
    let receipt = agents.accept_provider_event_batch(write()?).await?;
    assert!(!receipt.receipt.replayed);
    assert_eq!(receipt.receipt.accepted_events, 1);
    let replay = agents.accept_provider_event_batch(write()?).await?;
    assert!(replay.receipt.replayed);
    assert_eq!(replay.receipt.accepted_at_ms, receipt.receipt.accepted_at_ms);
    let checkpoint = agents
        .find_active_checkpoint(state.organization_id, state.execution_id)
        .await?
        .ok_or_else(|| invalid("provider Tool request omitted its approval checkpoint"))?;
    assert_eq!(checkpoint.status, AgentApprovalCheckpointStatus::Pending);
    drop(agents);
    Ok(PendingApprovalScenario {
        state,
        binding: initial_binding,
        checkpoint,
    })
}

fn approval_request_batch(
    execution_id: AgentExecutionId,
    binding: &AgentCodeRunBinding,
    call_id: &str,
    tool: HarnessToolBindingV1,
) -> TestResult<NodeAgentProviderEventBatchV1> {
    let observed_at_ms = u64::try_from(binding.bound_at().timestamp_millis())?
        .checked_add(1)
        .ok_or_else(|| invalid("Agent approval event timestamp overflowed"))?;
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
        state: AgentProviderRunStateV1::AwaitingApproval,
        observed_at_ms,
        retention_gap: false,
        has_more: false,
        terminal_failure: None,
        events: vec![AgentProviderEventRecordV1 {
            sequence: 0,
            occurred_at_ms: observed_at_ms,
            event: AgentProviderSemanticEventV1::ToolRequest {
                call_id: call_id.into(),
                tool,
                request: AgentProviderToolPayloadIdentityV1 {
                    digest: format!("sha256:{}", "f".repeat(64)),
                    size_bytes: 128,
                    media_type: "application/json".into(),
                },
            },
        }],
    };
    let batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: binding.node_id().as_uuid(),
        binding: binding.node_provider_runtime_binding(execution_id.as_uuid())?,
        page,
        sent_at_ms: observed_at_ms + 1,
    };
    batch.validate()?;
    Ok(batch)
}

async fn reconnect_approval_runtime(postgres_url: &str) -> TestResult<ApprovalRuntime> {
    let executor = connect_postgres(postgres_url, 8).await?;
    let agents = Arc::new(PostgresAgentRepository::new(executor.clone()));
    let nodes = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let workloads = Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let runtime = flow_runtime(agents.clone(), workloads, nodes.clone())?;
    Ok(ApprovalRuntime {
        executor,
        agents,
        nodes,
        runtime,
    })
}

async fn assert_pending_approval_survives_reconnect(
    postgres_url: &str,
    scenario: &PendingApprovalScenario,
) -> TestResult {
    let restored = reconnect_approval_runtime(postgres_url).await?;
    let output = run_step(
        &restored.runtime,
        &scenario.state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": scenario.state.start_dispatched}),
    )
    .await?;
    assert_pending_without_dispatch(&output)?;
    assert_eq!(
        restored
            .agents
            .find_active_checkpoint(
                scenario.state.organization_id,
                scenario.state.execution_id,
            )
            .await?,
        Some(scenario.checkpoint.clone())
    );
    Ok(())
}

async fn decide_approval_across_reconnect(
    postgres_url: &str,
    scenario: &PendingApprovalScenario,
    outcome: AgentProviderApprovalOutcomeV1,
    idempotency_key: &str,
) -> TestResult<AgentApprovalCheckpoint> {
    let first = reconnect_approval_runtime(postgres_url).await?;
    let authorizer = RecordingApprovalAuthorizer::default();
    let actor_principal_id = PrincipalId::new();
    seed_approval_actor(
        &first.executor,
        actor_principal_id,
        scenario.checkpoint.requested_at,
    )
    .await?;
    let command = DecideAgentApprovalCheckpoint {
        organization_id: scenario.state.organization_id,
        execution_id: scenario.state.execution_id,
        checkpoint_id: scenario.checkpoint.id,
        expected_version: scenario.checkpoint.aggregate_version,
        outcome,
        reason: Some(format!("{idempotency_key} by integration policy")),
        resource_access: ResourceAccessEvaluator::organization_wide(),
        actor_principal_id,
        credential_id: ApiTokenId::new(),
        actor_is_platform_admin: false,
        idempotency_key: idempotency_key.into(),
        request_id: Uuid::now_v7(),
        requested_at: canonical_timestamp(Utc::now().max(scenario.checkpoint.updated_at)),
    };
    let decided = DecideAgentApprovalCheckpointHandler::new(
        first.agents.clone(),
        Arc::new(authorizer.clone()),
    )
    .execute(command.clone(), context())
    .await?
    .map_err(|error| invalid(format!("could not decide Agent approval: {error}")))?;
    assert!(!decided.replayed);
    assert_eq!(decided.checkpoint.outcome, Some(outcome));
    assert_eq!(authorizer.calls.load(Ordering::SeqCst), 1);
    drop(first);

    let replay_runtime = reconnect_approval_runtime(postgres_url).await?;
    let replay_authorizer = RecordingApprovalAuthorizer::default();
    let replayed = DecideAgentApprovalCheckpointHandler::new(
        replay_runtime.agents,
        Arc::new(replay_authorizer.clone()),
    )
    .execute(command, context())
    .await?
    .map_err(|error| invalid(format!("could not replay Agent approval: {error}")))?;
    assert!(replayed.replayed);
    assert_eq!(replayed.checkpoint, decided.checkpoint);
    assert_eq!(replay_authorizer.calls.load(Ordering::SeqCst), 0);
    Ok(decided.checkpoint)
}

async fn seed_approval_actor(
    executor: &PostgresExecutor,
    actor_principal_id: PrincipalId,
    created_at: DateTime<Utc>,
) -> TestResult {
    Database::new(PostgresDialect, executor.clone())
        .execute(
            sql_query::<()>(
                "insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (",
            )
            .bind(actor_principal_id.as_uuid())
            .append(", 'human', 'Agent approval recovery actor', 1, ")
            .bind(created_at)
            .append(", null)"),
        )
        .await?;
    Ok(())
}

async fn settle_approval_resolution(
    postgres_url: &str,
    scenario: PendingApprovalScenario,
    expected_checkpoint: AgentApprovalCheckpoint,
    expected_outcome: AgentProviderApprovalOutcomeV1,
) -> TestResult {
    let dispatch = reconnect_approval_runtime(postgres_url).await?;
    let output = run_step(
        &dispatch.runtime,
        &scenario.state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": scenario.state.start_dispatched}),
    )
    .await?;
    let unresolved_dispatch = pending_dispatched(&output)?.clone();
    let resume = lease_and_ack_code_command(
        dispatch.nodes.as_ref(),
        scenario.state.node_id,
        scenario.state.agent_instance_id,
        scenario.state.start_sequence,
        ExpectedCommand::Resume,
        AgentProviderRunStateV1::Executing,
    )
    .await?;
    let NodeCommandPayload::AgentProviderCommand { command, .. } = &resume.payload else {
        return Err(invalid("Agent approval resume used another Fleet command kind").into());
    };
    let AgentProviderCommandV1::Resume { request } = command.as_ref() else {
        return Err(invalid("Agent approval resolution did not dispatch Resume").into());
    };
    assert_eq!(request.decision.outcome, expected_outcome);
    assert_eq!(
        request.decision.checkpoint_id,
        expected_checkpoint.id.to_string()
    );
    drop(dispatch);

    let settlement = reconnect_approval_runtime(postgres_url).await?;
    let settled = run_step(
        &settlement.runtime,
        &scenario.state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": unresolved_dispatch.clone()}),
    )
    .await?;
    let settled_dispatch = pending_dispatched(&settled)?;
    assert!(settled_dispatch.get("approval").is_none());
    let resumed = settlement
        .agents
        .find_checkpoint(scenario.state.organization_id, expected_checkpoint.id)
        .await?
        .ok_or_else(|| invalid("resumed Agent approval checkpoint disappeared"))?;
    assert_eq!(resumed.status, AgentApprovalCheckpointStatus::Resumed);
    assert_eq!(resumed.outcome, Some(expected_outcome));
    assert_eq!(resumed.resume_command_id.map(|id| id.as_uuid()), Some(resume.command_id));
    let replayed = run_step(
        &settlement.runtime,
        &scenario.state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": unresolved_dispatch}),
    )
    .await?;
    assert!(pending_dispatched(&replayed)?.get("approval").is_none());
    assert_eq!(
        settlement
            .agents
            .find_checkpoint(scenario.state.organization_id, expected_checkpoint.id)
            .await?,
        Some(resumed)
    );

    let continuation = provider_state_batch(
        scenario.state.execution_id,
        &scenario.binding,
        AgentProviderRunStateV1::Executing,
    )?;
    settlement
        .agents
        .accept_provider_event_batch(AcceptAgentProviderEventBatchWrite::new(
            scenario.state.organization_id,
            scenario.state.node_id,
            continuation.clone(),
            provider_batch_accepted_at(&continuation)?,
        )?)
        .await?;
    let execution = settlement
        .agents
        .find_execution(scenario.state.organization_id, scenario.state.execution_id)
        .await?
        .ok_or_else(|| invalid("resumed Agent execution disappeared"))?;
    assert_eq!(execution.status, AgentExecutionStatus::Running);

    let database = Database::new(PostgresDialect, settlement.executor);
    let command_counts = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64)>(
                "select count(*) filter (where payload -> 'command' ->> 'action' = 'resume'), count(*) filter (where payload -> 'command' ->> 'action' = 'resume' and acknowledgement is not null), count(*) filter (where payload -> 'command' ->> 'action' = 'recover') from node_commands where aggregate_id = ",
            )
            .bind(scenario.state.execution_id.as_uuid()),
        )
        .await?;
    assert_eq!(command_counts, (1, 1, 0));
    assert_approval_audit(
        &database,
        scenario.state.organization_id,
        scenario.state.execution_id,
        expected_outcome,
        true,
    )
    .await
}

fn provider_state_batch(
    execution_id: AgentExecutionId,
    binding: &AgentCodeRunBinding,
    state: AgentProviderRunStateV1,
) -> TestResult<NodeAgentProviderEventBatchV1> {
    let observed_at_ms = u64::try_from(binding.bound_at().timestamp_millis())?
        .checked_add(2)
        .ok_or_else(|| invalid("Agent provider continuation timestamp overflowed"))?;
    let batch = NodeAgentProviderEventBatchV1 {
        schema: NodeAgentProviderEventBatchV1::SCHEMA.into(),
        batch_id: Uuid::now_v7(),
        node_id: binding.node_id().as_uuid(),
        binding: binding.node_provider_runtime_binding(execution_id.as_uuid())?,
        page: AgentProviderEventPageV1 {
            schema: AgentProviderEventPageV1::SCHEMA.into(),
            identity: binding.provider_identity()?,
            after_event_sequence: Some(0),
            first_available_sequence: Some(0),
            source_first_sequence: None,
            source_last_sequence: None,
            source_event_count: 0,
            latest_sequence_exclusive: 1,
            next_after_event_sequence: Some(0),
            state,
            observed_at_ms,
            retention_gap: false,
            has_more: false,
            terminal_failure: None,
            events: Vec::new(),
        },
        sent_at_ms: observed_at_ms + 1,
    };
    batch.validate()?;
    Ok(batch)
}

async fn exercise_pending_approval_cancellation(postgres_url: &str) -> TestResult {
    let scenario = prepare_pending_approval(postgres_url, "cancel-1", false).await?;
    let cancellation = reconnect_approval_runtime(postgres_url).await?;
    let current = cancellation
        .agents
        .find_execution(scenario.state.organization_id, scenario.state.execution_id)
        .await?
        .ok_or_else(|| invalid("Agent execution disappeared before approval cancellation"))?;
    let expected_version = current.aggregate_version;
    let mut cancelling = current;
    cancelling.request_cancellation(canonical_timestamp(Utc::now()).max(cancelling.updated_at))?;
    cancellation
        .agents
        .request_cancellation(RequestAgentExecutionCancellationWrite {
            event: AgentExecutionCancellationRequested::envelope(&cancelling, Uuid::now_v7())?,
            execution: cancelling,
            expected_version,
            idempotency: idempotency(
                "test.agent-approval.cancellation",
                "cancel-pending-approval",
                b"cancel-pending-approval",
            )?,
        })
        .await?;
    let waiting = run_step(
        &cancellation.runtime,
        &scenario.state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": scenario.state.start_dispatched}),
    )
    .await?;
    assert_pending_without_dispatch(&waiting)?;
    let cancel = lease_and_ack_code_command(
        cancellation.nodes.as_ref(),
        scenario.state.node_id,
        scenario.state.agent_instance_id,
        scenario.state.start_sequence,
        ExpectedCommand::Cancel,
        AgentProviderRunStateV1::Cancelled,
    )
    .await?;
    drop(cancellation);

    let terminal_runtime = reconnect_approval_runtime(postgres_url).await?;
    let terminal_batch = provider_state_batch(
        scenario.state.execution_id,
        &scenario.binding,
        AgentProviderRunStateV1::Cancelled,
    )?;
    terminal_runtime
        .agents
        .accept_provider_event_batch(AcceptAgentProviderEventBatchWrite::new(
            scenario.state.organization_id,
            scenario.state.node_id,
            terminal_batch.clone(),
            provider_batch_accepted_at(&terminal_batch)?,
        )?)
        .await?;
    let terminal = run_step(
        &terminal_runtime.runtime,
        &scenario.state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": scenario.state.start_dispatched}),
    )
    .await?;
    if terminal.get("state").and_then(Value::as_str) != Some("terminal")
        || terminal
            .pointer("/completed/status")
            .and_then(Value::as_str)
            != Some("cancelled")
    {
        return Err(invalid(format!(
            "Agent approval cancellation did not become terminal: {terminal}"
        ))
        .into());
    }
    let checkpoint = terminal_runtime
        .agents
        .find_checkpoint(scenario.state.organization_id, scenario.checkpoint.id)
        .await?
        .ok_or_else(|| invalid("cancelled Agent approval checkpoint disappeared"))?;
    assert_eq!(checkpoint.status, AgentApprovalCheckpointStatus::Cancelled);
    let database = Database::new(PostgresDialect, terminal_runtime.executor);
    let command_counts = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64)>(
                "select count(*) filter (where payload -> 'command' ->> 'action' = 'cancel'), count(*) filter (where payload -> 'command' ->> 'action' = 'resume'), count(*) filter (where payload -> 'command' ->> 'action' = 'recover') from node_commands where aggregate_id = ",
            )
            .bind(scenario.state.execution_id.as_uuid()),
        )
        .await?;
    assert_eq!(command_counts, (1, 0, 0));
    assert_eq!(cancel.sequence, scenario.state.start_sequence + 1);
    assert_approval_audit(
        &database,
        scenario.state.organization_id,
        scenario.state.execution_id,
        AgentProviderApprovalOutcomeV1::Approved,
        false,
    )
    .await?;
    assert_eq!(
        approval_audit_count(
            &database,
            scenario.state.organization_id,
            scenario.state.execution_id,
            "agent.execution.approval-cancelled",
        )
        .await?,
        1
    );
    Ok(())
}

async fn exercise_pending_approval_provider_restart(postgres_url: &str) -> TestResult {
    let scenario = prepare_pending_approval(postgres_url, "restart-1", false).await?;
    let restarted = reconnect_approval_runtime(postgres_url).await?;
    let restarted_started_at_ms = scenario
        .state
        .initial_runtime_started_at_ms
        .checked_add(2_000)
        .ok_or_else(|| invalid("Agent approval restart timestamp overflowed"))?;
    let restarted_received_at = canonical_timestamp(Utc::now())
        .max(scenario.state.initial_runtime_received_at + Duration::milliseconds(1));
    record_runtime_observation(
        restarted.nodes.as_ref(),
        scenario.state.node_id,
        scenario.state.agent_instance_id,
        &scenario.state.runtime_spec,
        &scenario.state.runtime_capabilities,
        RuntimeObservationTiming {
            started_at_ms: restarted_started_at_ms,
            observed_at_ms: restarted_started_at_ms + 1,
            received_at: restarted_received_at,
        },
    )
    .await?;
    let terminal = run_step(
        &restarted.runtime,
        &scenario.state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": scenario.state.start_dispatched.clone()}),
    )
    .await?;
    assert_failed_terminal(&terminal)?;
    let replayed = run_step(
        &restarted.runtime,
        &scenario.state.run_id,
        OBSERVE_STEP,
        json!({"dispatched": scenario.state.start_dispatched}),
    )
    .await?;
    assert_eq!(replayed, terminal);
    let checkpoint = restarted
        .agents
        .find_checkpoint(scenario.state.organization_id, scenario.checkpoint.id)
        .await?
        .ok_or_else(|| invalid("provider-restart approval checkpoint disappeared"))?;
    assert_eq!(checkpoint.status, AgentApprovalCheckpointStatus::Cancelled);
    let execution = restarted
        .agents
        .find_execution(scenario.state.organization_id, scenario.state.execution_id)
        .await?
        .ok_or_else(|| invalid("provider-restart Agent execution disappeared"))?;
    assert_eq!(execution.status, AgentExecutionStatus::Failed);
    let database = Database::new(PostgresDialect, restarted.executor);
    let command_counts = database
        .fetch_one_as(
            sql_query::<(i64, i64)>(
                "select count(*) filter (where payload -> 'command' ->> 'action' = 'resume'), count(*) filter (where payload -> 'command' ->> 'action' = 'recover') from node_commands where aggregate_id = ",
            )
            .bind(scenario.state.execution_id.as_uuid()),
        )
        .await?;
    assert_eq!(command_counts, (0, 0));
    assert_eq!(
        approval_audit_count(
            &database,
            scenario.state.organization_id,
            scenario.state.execution_id,
            "agent.execution.approval-cancelled",
        )
        .await?,
        1
    );
    Ok(())
}

async fn assert_approval_audit(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
    outcome: AgentProviderApprovalOutcomeV1,
    resumed: bool,
) -> TestResult {
    assert_eq!(
        approval_audit_count(
            database,
            organization_id,
            execution_id,
            "agent.execution.tool-requested",
        )
        .await?,
        1
    );
    assert_eq!(
        approval_audit_count(
            database,
            organization_id,
            execution_id,
            "agent.execution.approval-requested",
        )
        .await?,
        1
    );
    let decision_action = match outcome {
        AgentProviderApprovalOutcomeV1::Approved => "agent.execution.approval-approved",
        AgentProviderApprovalOutcomeV1::Denied => "agent.execution.approval-denied",
        AgentProviderApprovalOutcomeV1::Expired => "agent.execution.approval-expired",
    };
    if resumed {
        assert_eq!(
            approval_audit_count(database, organization_id, execution_id, decision_action).await?,
            1
        );
        assert_eq!(
            approval_audit_count(
                database,
                organization_id,
                execution_id,
                "agent.execution.approval-resumed",
            )
            .await?,
            1
        );
    }
    let unsafe_payloads = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from audit_records where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(execution_id.as_uuid())
            .append(" and action like 'agent.execution.%' and details::text like '%\"payload\"%'"),
        )
        .await?;
    assert_eq!(unsafe_payloads, 0);
    Ok(())
}

async fn approval_audit_count(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
    action: &str,
) -> TestResult<i64> {
    Ok(database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from audit_records where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and aggregate_id = ")
                .bind(execution_id.as_uuid())
                .append(" and action = ")
                .bind(action),
        )
        .await?)
}
