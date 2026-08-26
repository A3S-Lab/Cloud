async fn prepare_persisted_scenario(postgres_url: &str) -> TestResult<ScenarioState> {
    let executor = migrate_and_connect_for_test(postgres_url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let created_at = canonical_timestamp(Utc::now() - Duration::seconds(10));
    insert_scope(
        &database,
        organization_id,
        project_id,
        environment_id,
        created_at,
    )
    .await?;

    let assets = Arc::new(PostgresAssetRepository::new(executor.clone()));
    let artifacts = Arc::new(HostedArtifactQueryService::new(Arc::new(
        PostgresBuildRunRepository::new(executor.clone()),
    )));
    let agents = Arc::new(PostgresAgentRepository::new(executor.clone()));
    let nodes = Arc::new(PostgresNodeRepository::new(executor.clone()));
    let projects = Arc::new(PostgresProjectsRepository::new(executor.clone()));
    let secrets = Arc::new(PostgresSecretRepository::new(executor.clone()));
    let workloads = Arc::new(PostgresWorkloadRepository::new(executor.clone()));

    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("PostgreSQL Recovery Agent")?,
        AssetKind::Agent,
        created_at,
    )?;
    assets
        .create_asset(CreateAssetWrite {
            asset: asset.clone(),
            event: AssetCreated::envelope(&asset, Uuid::now_v7())?,
            idempotency: idempotency(
                "test.agent-code-recovery.assets",
                "create-agent",
                b"create-agent",
            )?,
        })
        .await?;
    let release = AssetRelease::draft(
        &asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0")?,
        GitCommitSha::parse("a".repeat(40))?,
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))?,
        created_at + Duration::milliseconds(1),
    )?;
    assets
        .create_release(CreateAssetReleaseWrite {
            release: release.clone(),
            event: AssetReleaseDrafted::envelope(&release, release.id.as_uuid())?,
            hosted_build_requested_event: Some(HostedAssetBuildRequested::envelope(
                &asset,
                &release,
                release.id.as_uuid(),
            )?),
            idempotency: idempotency(
                "test.agent-code-recovery.releases",
                "draft-agent-1.0.0",
                b"draft-agent-1.0.0",
            )?,
        })
        .await?;
    let published =
        crate::build_runs_support::publish_hosted_release(&executor, &asset, &release).await?;

    let runtime_capabilities = runtime_capabilities()?;
    let (node_id, agent_instance_id) = enroll_node(
        nodes.as_ref(),
        organization_id,
        &runtime_capabilities,
        canonical_timestamp(Utc::now()),
    )
    .await?;
    let workload_requested_at =
        canonical_timestamp(Utc::now()).max(published.updated_at + Duration::milliseconds(1));
    let workload = CreateAgentWorkloadDeploymentHandler::new(
        projects.clone(),
        assets.clone(),
        artifacts.clone(),
        workloads.clone(),
        secrets,
        nodes.clone(),
    )
    .execute(
        CreateAgentWorkloadDeployment {
            organization_id,
            project_id,
            environment_id,
            asset_id: asset.id,
            asset_release_id: published.id,
            name: "postgres-recovery-agent-runtime".into(),
            node_pool_id: None,
            template: agent_runtime_template(),
            idempotency_key: "deploy-agent-runtime".into(),
            request_id: Uuid::now_v7(),
            requested_at: workload_requested_at,
        },
        context(),
    )
    .await?
    .map_err(|error| invalid(format!("could not create Agent Workload: {error}")))?;
    let runtime_spec = project_runtime_spec(&workload.bundle.revision)?;
    let mut deployment = workload.bundle.deployment.clone();
    let mut transition_at = canonical_timestamp(Utc::now()).max(deployment.updated_at);
    deployment = workloads
        .mark_resolving(deployment.id, deployment.aggregate_version, transition_at)
        .await?;
    transition_at += Duration::milliseconds(1);
    deployment = workloads
        .assign_node(
            deployment.id,
            deployment.aggregate_version,
            node_id,
            transition_at,
        )
        .await?;
    let replica_binding = workloads
        .find_deployment_replica_binding(organization_id, deployment.id)
        .await?;
    let runtime_command_id = NodeCommandId::from_uuid(deployment.id.as_uuid());
    let runtime_command_issued_at = deployment.updated_at;
    let runtime_command_deadline = runtime_command_issued_at
        .checked_add_signed(Duration::seconds(60))
        .ok_or_else(|| invalid("Runtime apply command deadline overflowed"))?;
    let runtime_command = nodes
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: runtime_command_id,
            node_id,
            aggregate_id: replica_binding.replica_id.as_uuid(),
            payload: NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("deployment:{}:apply", deployment.id),
                    deadline_at_ms: Some(u64::try_from(
                        runtime_command_deadline.timestamp_millis(),
                    )?),
                    spec: runtime_spec.clone(),
                }),
                resource_claim: None,
            },
            issued_at: runtime_command_issued_at,
            not_after: runtime_command_deadline,
            correlation_id: workload.bundle.operation.id.as_uuid(),
        })
        .await?
        .value;
    transition_at += Duration::milliseconds(1);
    deployment = workloads
        .mark_dispatched(
            deployment.id,
            deployment.aggregate_version,
            runtime_command.id,
            transition_at,
        )
        .await?;
    transition_at += Duration::milliseconds(1);
    deployment = workloads
        .mark_verifying(deployment.id, deployment.aggregate_version, transition_at)
        .await?;
    transition_at += Duration::milliseconds(1);
    let (_, deployment) = workloads
        .activate(
            deployment.id,
            deployment.aggregate_version,
            false,
            transition_at,
        )
        .await?;
    let targets = workloads.list_active_runtime_targets(10).await?;
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].deployment.id, deployment.id);
    assert_eq!(targets[0].replica_binding.node_id, Some(node_id));
    let initial_runtime_received_at =
        canonical_timestamp(Utc::now()).max(deployment.updated_at + Duration::milliseconds(1));
    let provider_observed_at_ms = u64::try_from(initial_runtime_received_at.timestamp_millis())?;
    let initial_runtime_started_at_ms = provider_observed_at_ms.saturating_sub(1_000);
    record_runtime_observation(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        &runtime_spec,
        &runtime_capabilities,
        RuntimeObservationTiming {
            started_at_ms: initial_runtime_started_at_ms,
            observed_at_ms: provider_observed_at_ms,
            received_at: initial_runtime_received_at,
        },
    )
    .await?;

    let conversation_requested_at = canonical_timestamp(Utc::now())
        .max(initial_runtime_received_at + Duration::milliseconds(1));
    let conversation = CreateAgentConversationHandler::new(projects, agents.clone())
        .execute(
            CreateAgentConversation {
                organization_id,
                project_id,
                environment_id,
                idempotency_key: "create-recovery-conversation".into(),
                request_id: Uuid::now_v7(),
                requested_at: conversation_requested_at,
            },
            context(),
        )
        .await?
        .map_err(|error| invalid(format!("could not create Agent conversation: {error}")))?;
    let execution = StartAgentExecutionHandler::new(agents.clone(), assets, artifacts)
        .execute(
            StartAgentExecution {
                organization_id,
                conversation_id: conversation.conversation.id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                agent_asset_id: asset.id,
                agent_asset_release_id: published.id,
                input: json!({"prompt": "prove durable recovery"}),
                idempotency_key: "start-recovery-execution".into(),
                request_id: Uuid::now_v7(),
                requested_at: conversation_requested_at + Duration::milliseconds(1),
            },
            context(),
        )
        .await?
        .map_err(|error| invalid(format!("could not start Agent execution: {error}")))?;
    let run_id = execution.execution.operation_id.to_string();
    let runtime = flow_runtime(agents.clone(), workloads, nodes.clone())?;
    let prepared = run_step(
        &runtime,
        &run_id,
        PREPARE_STEP,
        json!({
            "organizationId": organization_id,
            "executionId": execution.execution.id,
        }),
    )
    .await?;
    let prepared = ready_field(&prepared, "prepared")?.clone();
    let dispatched = run_step(
        &runtime,
        &run_id,
        DISPATCH_STEP,
        json!({"prepared": prepared}),
    )
    .await?;
    let start_dispatched = ready_field(&dispatched, "dispatched")?.clone();
    let start = lease_and_ack_code_command(
        nodes.as_ref(),
        node_id,
        agent_instance_id,
        runtime_command.sequence,
        ExpectedCommand::Start,
        AgentProviderRunStateV1::Created,
    )
    .await?;
    let initial_observe = run_step(
        &runtime,
        &run_id,
        OBSERVE_STEP,
        json!({"dispatched": start_dispatched.clone()}),
    )
    .await?;
    assert_pending_without_dispatch(&initial_observe)?;

    let bound = agents
        .find_execution(organization_id, execution.execution.id)
        .await?
        .ok_or_else(|| invalid("prepared Agent execution disappeared"))?;
    let initial_binding = bound
        .code
        .clone()
        .ok_or_else(|| invalid("prepared Agent execution omitted its Code binding"))?;
    let initial_run_id = initial_binding.identity().run_id.clone();
    let first = event_batch(
        execution.execution.id,
        &initial_binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Executing,
        1,
    )?;
    let first_accepted_at = accepted_at(&first)?;
    let first_receipt = agents
        .accept_code_event_batch(AcceptAgentCodeEventBatchWrite::new(
            organization_id,
            node_id,
            first,
            first_accepted_at,
        )?)
        .await?;
    assert!(!first_receipt.replayed);

    let checkpoint = agents
        .find_execution(organization_id, execution.execution.id)
        .await?
        .ok_or_else(|| invalid("Agent execution disappeared after first Code page"))?
        .code
        .ok_or_else(|| invalid("Agent execution lost its first Code checkpoint"))?;
    let gap = retention_gap_batch(
        execution.execution.id,
        &checkpoint,
        first_receipt.accepted_at_ms + 10,
    )?;
    let gap_accepted_at = first_accepted_at + Duration::milliseconds(1);
    let gap_write = || {
        AcceptAgentCodeEventBatchWrite::new(organization_id, node_id, gap.clone(), gap_accepted_at)
    };
    let gap_receipt = agents.accept_code_event_batch(gap_write()?).await?;
    assert!(!gap_receipt.replayed);
    let gap_replay = agents.accept_code_event_batch(gap_write()?).await?;
    assert!(gap_replay.replayed);
    assert_eq!(gap_replay.accepted_at_ms, gap_receipt.accepted_at_ms);

    let recovered = agents
        .find_execution(organization_id, execution.execution.id)
        .await?
        .ok_or_else(|| invalid("Agent execution disappeared after retention recovery"))?;
    let recovered_binding = recovered
        .code
        .clone()
        .ok_or_else(|| invalid("retention recovery omitted its successor binding"))?;
    let retention_successor_run_id = recovered_binding.identity().run_id.clone();
    assert_eq!(
        retention_successor_run_id,
        AgentCodeRunBinding::recovery_run_id(execution.execution.id, &initial_run_id)
    );

    let stale = stale_checkpoint_batch(
        execution.execution.id,
        &checkpoint,
        gap.page.observed_at_ms + 1,
    )?;
    let stale_write = || {
        AcceptAgentCodeEventBatchWrite::new(
            organization_id,
            node_id,
            stale.clone(),
            gap_accepted_at + Duration::milliseconds(1),
        )
    };
    let stale_receipt = agents.accept_code_event_batch(stale_write()?).await?;
    assert!(!stale_receipt.replayed);
    assert!(
        agents
            .accept_code_event_batch(stale_write()?)
            .await?
            .replayed
    );

    let successor = event_batch(
        execution.execution.id,
        &recovered_binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Planning,
        1,
    )?;
    let successor_accepted_at = accepted_at(&successor)?;
    let successor_write = || {
        AcceptAgentCodeEventBatchWrite::new(
            organization_id,
            node_id,
            successor.clone(),
            successor_accepted_at,
        )
    };
    let successor_receipt = agents.accept_code_event_batch(successor_write()?).await?;
    assert!(!successor_receipt.replayed);
    assert!(
        agents
            .accept_code_event_batch(successor_write()?)
            .await?
            .replayed
    );
    assert_eq!(
        agents
            .list_events(organization_id, conversation.conversation.id, None, 100,)
            .await?
            .len(),
        3
    );

    Ok(ScenarioState {
        organization_id,
        conversation_id: conversation.conversation.id,
        execution_id: execution.execution.id,
        run_id,
        node_id,
        agent_instance_id,
        runtime_spec,
        runtime_capabilities,
        initial_runtime_received_at,
        initial_runtime_started_at_ms,
        initial_run_id,
        retention_successor_run_id,
        start_dispatched,
        start_sequence: start.sequence,
    })
}
