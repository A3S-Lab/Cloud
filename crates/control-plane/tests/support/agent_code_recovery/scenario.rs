async fn prepare_persisted_scenario(postgres_url: &str) -> TestResult<ScenarioState> {
    let StartedProviderScenario {
        state,
        agents,
        initial_binding,
    } = prepare_started_provider_scenario(postgres_url, NATIVE_CODE_AGENT_PROVIDER_KIND).await?;
    let initial_run_id = initial_binding.identity().run_id.clone();
    let first = event_batch(
        state.execution_id,
        &initial_binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Executing,
        1,
    )?;
    let first_accepted_at = accepted_at(&first)?;
    let first_receipt = agents
        .accept_code_event_batch(AcceptAgentCodeEventBatchWrite::new(
            state.organization_id,
            state.node_id,
            first,
            first_accepted_at,
        )?)
        .await?;
    assert!(!first_receipt.replayed);

    let checkpoint = agents
        .find_execution(state.organization_id, state.execution_id)
        .await?
        .ok_or_else(|| invalid("Agent execution disappeared after first Code page"))?
        .code
        .ok_or_else(|| invalid("Agent execution lost its first Code checkpoint"))?;
    let gap = retention_gap_batch(
        state.execution_id,
        &checkpoint,
        first_receipt.accepted_at_ms + 10,
    )?;
    let gap_accepted_at = first_accepted_at + Duration::milliseconds(1);
    let gap_write = || {
        AcceptAgentCodeEventBatchWrite::new(
            state.organization_id,
            state.node_id,
            gap.clone(),
            gap_accepted_at,
        )
    };
    let gap_receipt = agents.accept_code_event_batch(gap_write()?).await?;
    assert!(!gap_receipt.replayed);
    let gap_replay = agents.accept_code_event_batch(gap_write()?).await?;
    assert!(gap_replay.replayed);
    assert_eq!(gap_replay.accepted_at_ms, gap_receipt.accepted_at_ms);

    let recovered = agents
        .find_execution(state.organization_id, state.execution_id)
        .await?
        .ok_or_else(|| invalid("Agent execution disappeared after retention recovery"))?;
    let recovered_binding = recovered
        .code
        .clone()
        .ok_or_else(|| invalid("retention recovery omitted its successor binding"))?;
    let retention_successor_run_id = recovered_binding.identity().run_id.clone();
    assert_eq!(
        retention_successor_run_id,
        AgentCodeRunBinding::recovery_run_id(state.execution_id, &initial_run_id)
    );

    let stale = stale_checkpoint_batch(
        state.execution_id,
        &checkpoint,
        gap.page.observed_at_ms + 1,
    )?;
    let stale_write = || {
        AcceptAgentCodeEventBatchWrite::new(
            state.organization_id,
            state.node_id,
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
        state.execution_id,
        &recovered_binding,
        Uuid::now_v7(),
        AgentProtocolRunStateV1::Planning,
        1,
    )?;
    let successor_accepted_at = accepted_at(&successor)?;
    let successor_write = || {
        AcceptAgentCodeEventBatchWrite::new(
            state.organization_id,
            state.node_id,
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
            .list_events(state.organization_id, state.conversation_id, None, 100)
            .await?
            .len(),
        3
    );

    Ok(ScenarioState {
        organization_id: state.organization_id,
        conversation_id: state.conversation_id,
        execution_id: state.execution_id,
        run_id: state.run_id,
        node_id: state.node_id,
        agent_instance_id: state.agent_instance_id,
        runtime_spec: state.runtime_spec,
        runtime_capabilities: state.runtime_capabilities,
        initial_runtime_received_at: state.initial_runtime_received_at,
        initial_runtime_started_at_ms: state.initial_runtime_started_at_ms,
        initial_run_id,
        retention_successor_run_id,
        start_dispatched: state.start_dispatched,
        start_sequence: state.start_sequence,
    })
}

async fn prepare_started_provider_scenario(
    postgres_url: &str,
    provider_kind: &str,
) -> TestResult<StartedProviderScenario> {
    prepare_started_provider_scenario_with_tools(postgres_url, provider_kind, Vec::new(), None)
        .await
}

async fn prepare_started_provider_scenario_with_tools(
    postgres_url: &str,
    provider_kind: &str,
    tools: Vec<HarnessToolBindingV1>,
    execution_requested_at: Option<DateTime<Utc>>,
) -> TestResult<StartedProviderScenario> {
    let executor = migrate_and_connect_for_test(postgres_url, 8).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let now = canonical_timestamp(Utc::now());
    let created_at = canonical_timestamp(
        (now - Duration::seconds(10)).min(
            execution_requested_at
                .unwrap_or(now)
                .checked_sub_signed(Duration::seconds(10))
                .ok_or_else(|| invalid("Agent approval fixture creation time underflowed"))?,
        ),
    );
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
                &format!(
                    "test.agent-code-recovery.organizations/{organization_id}/assets"
                ),
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
                &format!(
                    "test.agent-code-recovery.organizations/{organization_id}/releases"
                ),
                "draft-agent-1.0.0",
                b"draft-agent-1.0.0",
            )?,
        })
        .await?;
    let published =
        crate::build_runs_support::publish_hosted_release(&executor, &asset, &release).await?;

    let runtime_capabilities = runtime_capabilities()?;
    let proposed_node_id = NodeId::new();
    let (node_id, agent_instance_id) = enroll_node(
        nodes.as_ref(),
        organization_id,
        proposed_node_id,
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
    let target = targets
        .iter()
        .find(|target| target.deployment.id == deployment.id)
        .ok_or_else(|| invalid("Agent recovery target disappeared after activation"))?;
    assert_eq!(target.replica_binding.node_id, Some(node_id));
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

    let conversation_requested_at = execution_requested_at.unwrap_or_else(|| {
        canonical_timestamp(Utc::now())
            .max(initial_runtime_received_at + Duration::milliseconds(1))
    });
    if conversation_requested_at < created_at {
        return Err(invalid("Agent recovery conversation time predates its tenant").into());
    }
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
    let execution = StartAgentExecutionHandler::new(
        agents.clone(),
        Arc::new(AssetsAgentReleaseAdmissionAdapter::new(assets, artifacts)),
        Arc::new(BuiltInAgentExecutionProviderRegistry::new().map_err(invalid)?),
    )
        .execute(
            StartAgentExecution {
                organization_id,
                conversation_id: conversation.conversation.id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
                agent_asset_id: asset.id,
                agent_asset_release_id: published.id,
                provider_kind: provider_kind.into(),
                input: json!({"prompt": "prove durable recovery"}),
                idempotency_key: "start-recovery-execution".into(),
                request_id: Uuid::now_v7(),
                requested_at: conversation_requested_at + Duration::milliseconds(1),
            },
            context(),
        )
        .await?
        .map_err(|error| invalid(format!("could not start Agent execution: {error}")))?;
    if !tools.is_empty() {
        let binding = approval_test_binding(
            &execution.execution,
            &workload.bundle.workload,
            &workload.bundle.revision,
            &deployment,
            &replica_binding,
            node_id,
            &runtime_spec,
            tools,
            execution.execution.updated_at + Duration::milliseconds(1),
        )?;
        agents
            .bind_code_run(BindAgentCodeRunWrite {
                organization_id,
                execution_id: execution.execution.id,
                binding,
            })
            .await?;
    }
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
    if initial_observe.get("state").and_then(Value::as_str) != Some("pending") {
        return Err(invalid(format!(
            "Agent Flow did not remain pending after provider start: {initial_observe}"
        ))
        .into());
    }
    let start_dispatched = initial_observe
        .get("dispatched")
        .cloned()
        .unwrap_or(start_dispatched);

    let bound = agents
        .find_execution(organization_id, execution.execution.id)
        .await?
        .ok_or_else(|| invalid("prepared Agent execution disappeared"))?;
    let initial_binding = bound
        .code
        .clone()
        .ok_or_else(|| invalid("prepared Agent execution omitted its Code binding"))?;
    Ok(StartedProviderScenario {
        state: StartedScenarioState {
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
            start_dispatched,
            start_sequence: start.sequence,
        },
        agents,
        initial_binding,
    })
}

#[allow(clippy::too_many_arguments)]
fn approval_test_binding(
    execution: &AgentExecution,
    workload: &Workload,
    revision: &WorkloadRevision,
    deployment: &Deployment,
    replica_binding: &DeploymentReplicaBinding,
    node_id: NodeId,
    runtime_spec: &RuntimeUnitSpec,
    tools: Vec<HarnessToolBindingV1>,
    bound_at: DateTime<Utc>,
) -> TestResult<AgentCodeRunBinding> {
    let provider_profile = execution.provider.profile()?;
    let template = revision.resolved_template()?;
    let service_port_name = template
        .health
        .as_ref()
        .map(|health| health.port_name.clone())
        .ok_or_else(|| invalid("Agent approval fixture omitted its provider health port"))?;
    let environment_policy = json!({
        "process": &runtime_spec.process,
        "secretReferences": &runtime_spec.secrets,
    });
    let security_policy = json!({
        "isolation": &runtime_spec.isolation,
        "mounts": &runtime_spec.mounts,
        "network": &runtime_spec.network,
        "resources": &runtime_spec.resources,
        "restart": &runtime_spec.restart,
    });
    let environment_policy_digest = sha256_digest(&canonical_json_bounded(
        &environment_policy,
        HARNESS_INVOCATION_PROFILE_MAX_BYTES,
        "Harness environment policy",
    )?);
    let security_policy_digest = sha256_digest(&canonical_json_bounded(
        &security_policy,
        HARNESS_INVOCATION_PROFILE_MAX_BYTES,
        "Harness security policy",
    )?);
    let mut required_capabilities = vec![
        AgentProviderCapabilityV1::Cancellation,
        AgentProviderCapabilityV1::Cleanup,
        AgentProviderCapabilityV1::EventPages,
        AgentProviderCapabilityV1::ToolCalls,
    ];
    if tools.iter().any(|tool| tool.approval_required) {
        required_capabilities.push(AgentProviderCapabilityV1::PauseResume);
    }
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
        environment_policy_digest,
        security_policy_digest,
        workspace: HarnessWorkspaceBindingV1 {
            workload_id: workload.id.as_uuid(),
            workload_revision_id: revision.id.as_uuid(),
            runtime_unit_id: runtime_spec.unit_id.clone(),
            runtime_generation: runtime_spec.generation,
            runtime_spec_digest: runtime_spec.digest()?,
            working_directory: runtime_spec.process.working_directory.clone(),
        },
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        models: Vec::new(),
        secrets: Vec::new(),
        tools,
        required_capabilities,
    };
    invocation.validate_for(&provider_profile)?;
    let binding = AgentCodeRunBinding::new_with_provider(
        execution.provider.clone(),
        node_id,
        workload.id,
        revision.id,
        deployment.id,
        replica_binding.replica_id,
        runtime_spec.unit_id.clone(),
        runtime_spec.generation,
        Sha256Digest::parse(runtime_spec.digest()?)?,
        service_port_name,
        AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: execution.provider.native_protocol().into(),
            agent_release_identity: execution.agent.artifact_digest().as_str().into(),
            session_id: format!("agent-conversation-{}", execution.conversation_id),
            run_id: format!("agent-execution-{}", execution.id),
        },
        bound_at,
    )?;

    // Integration tests are an external crate and intentionally cannot call
    // the crate-private production binder. Restore the serialized domain shape
    // and immediately run the same public invariant validation before writing.
    let mut encoded = serde_json::to_value(binding)?;
    encoded
        .as_object_mut()
        .ok_or_else(|| invalid("Agent run binding did not serialize as an object"))?
        .insert("invocation_profile".into(), serde_json::to_value(invocation)?);
    let binding: AgentCodeRunBinding = serde_json::from_value(encoded)?;
    binding.validate()?;
    Ok(binding)
}
