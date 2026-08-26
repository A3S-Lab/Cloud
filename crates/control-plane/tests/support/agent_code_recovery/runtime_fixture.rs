async fn insert_scope(
    database: &Database<PostgresDialect, PostgresExecutor>,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    created_at: DateTime<Utc>,
) -> TestResult {
    database
        .execute(
            sql_query::<()>(
                "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", 'Agent recovery tenant', 'agent-recovery-tenant', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", 'Agent recovery project', 'agent-recovery-project', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    database
        .execute(
            sql_query::<()>(
                "insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", ")
            .bind(environment_id.as_uuid())
            .append(", 'Agent recovery environment', 'agent-recovery-environment', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    Ok(())
}

fn runtime_capabilities() -> TestResult<RuntimeCapabilities> {
    let capabilities = RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("a3s-box")?,
        provider_build: "a3s-box-agent-recovery".into(),
        unit_classes: vec![RuntimeUnitClass::Service],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Sandbox],
        network_modes: vec![NetworkMode::Service],
        mount_kinds: Vec::new(),
        health_check_kinds: vec![HealthCheckKind::Http],
        resource_controls: vec![
            ResourceControl::Cpu,
            ResourceControl::Memory,
            ResourceControl::Pids,
        ],
        features: vec![
            RuntimeFeature::DurableIdentity,
            RuntimeFeature::Stop,
            RuntimeFeature::Remove,
            RuntimeFeature::ServiceTcp,
        ],
    };
    capabilities.validate()?;
    Ok(capabilities)
}

async fn enroll_node(
    nodes: &PostgresNodeRepository,
    organization_id: OrganizationId,
    capabilities: &RuntimeCapabilities,
    enrolled_at: DateTime<Utc>,
) -> TestResult<(NodeId, Uuid)> {
    let token_id = EnrollmentTokenId::new();
    let secret = format!("a3sn_{}", token_id.as_uuid().simple().to_string().repeat(2));
    let credential = EnrollmentTokenCredential::from_secret(&secret)?;
    let token = EnrollmentToken::new(
        token_id,
        organization_id,
        "Agent recovery worker",
        credential.clone(),
        enrolled_at,
        enrolled_at + Duration::minutes(5),
    )?;
    nodes
        .issue_enrollment_token(
            token.clone(),
            DomainEventEnvelope {
                event_id: Uuid::now_v7(),
                event_key: "fleet.enrollment-token.issued".into(),
                schema_version: 1,
                organization_id: organization_id.as_uuid(),
                aggregate_id: token.id.as_uuid(),
                aggregate_version: token.aggregate_version,
                occurred_at: token.created_at,
                correlation_id: Uuid::now_v7(),
                causation_id: None,
                payload: json!({"name": token.name}),
            },
            idempotency(
                "test.agent-code-recovery.enrollment",
                "issue-node-token",
                b"issue-node-token",
            )?,
        )
        .await?;
    let stored_capabilities = NodeCapabilities::new(
        capabilities.provider_id.to_string(),
        capabilities.provider_build.clone(),
        serde_json::to_value(capabilities)?,
    )?;
    let agent_instance_id = Uuid::now_v7();
    let reservation = nodes
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: NodeId::new(),
                name: NodeName::new("agent-recovery-worker")?,
                agent_instance_id,
                agent_version: "0.1.0-test".into(),
                capabilities: stored_capabilities.clone(),
                request_digest: format!("sha256:{}", "c".repeat(64)),
                requested_at: enrolled_at,
            },
        )
        .await?;
    nodes
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "0.1.0-test".into(),
            capabilities: stored_capabilities,
            observed_at: enrolled_at + Duration::milliseconds(1),
        })
        .await?;
    Ok((reservation.node.id, agent_instance_id))
}

fn agent_runtime_template() -> SourceWorkloadTemplate {
    SourceWorkloadTemplate {
        process: ServiceProcess {
            command: vec!["/app/a3s-code".into()],
            args: vec!["agent-protocol".into()],
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 250,
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: None,
        },
        ports: vec![ServicePort {
            name: "agent".into(),
            container_port: 49_152,
        }],
        health: Some(HttpHealthCheck {
            port_name: "agent".into(),
            path: "/health".into(),
            interval_ms: 1_000,
            timeout_ms: 500,
            healthy_threshold: 1,
            unhealthy_threshold: 3,
            stabilization_window_ms: 1_000,
        }),
    }
}

async fn record_runtime_observation(
    nodes: &PostgresNodeRepository,
    node_id: NodeId,
    agent_instance_id: Uuid,
    spec: &RuntimeUnitSpec,
    capabilities: &RuntimeCapabilities,
    timing: RuntimeObservationTiming,
) -> TestResult {
    let mut claims = BTreeMap::new();
    for (index, port) in spec
        .network
        .ports
        .iter()
        .filter(|port| port.protocol == TransportProtocol::Tcp)
        .enumerate()
    {
        let host_port = 49_152_u16
            .checked_add(u16::try_from(index)?)
            .ok_or_else(|| invalid("Agent Runtime endpoint port overflowed"))?;
        RuntimeServiceEndpoint::node_local_tcp(&port.name, host_port)?.insert_claim(&mut claims)?;
    }
    let spec_digest = spec.digest()?;
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec_digest.clone(),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some("agent-recovery-provider".into()),
        provider_build: Some(capabilities.provider_build.clone()),
        observed_at_ms: timing.observed_at_ms,
        started_at_ms: Some(timing.started_at_ms),
        finished_at_ms: None,
        health: Some(RuntimeHealthObservation {
            state: RuntimeHealthState::Healthy,
            checked_at_ms: timing.observed_at_ms,
            message: None,
        }),
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: capabilities.provider_build.clone(),
            spec_digest,
            semantics_profile_digest: spec.semantics_profile_digest.clone(),
            claims,
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate_against(spec)?;
    nodes
        .record_observations(
            NodeObservationBatch {
                schema: NodeObservationBatch::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                sent_at: timing.received_at,
                heartbeat: NodeHeartbeat {
                    schema: NodeHeartbeat::SCHEMA.into(),
                    node_id: node_id.as_uuid(),
                    agent_instance_id,
                    observed_at: timing.received_at,
                    agent_version: "0.1.0-test".into(),
                    runtime_capabilities: capabilities.clone(),
                },
                observations: vec![RuntimeObservationReport {
                    report_id: Uuid::now_v7(),
                    command_id: None,
                    observed_at: timing.received_at,
                    observation,
                }],
            }
            .into(),
            timing.received_at,
        )
        .await?;
    Ok(())
}

struct RuntimeObservationTiming {
    started_at_ms: u64,
    observed_at_ms: u64,
    received_at: DateTime<Utc>,
}

fn flow_runtime(
    agents: Arc<PostgresAgentRepository>,
    workloads: Arc<PostgresWorkloadRepository>,
    nodes: Arc<PostgresNodeRepository>,
) -> TestResult<AgentExecutionFlowRuntime> {
    Ok(AgentExecutionFlowRuntime::new(
        AgentExecutionFlowRuntimeDependencies {
            agents,
            providers: Arc::new(BuiltInAgentExecutionProviderRegistry::new().map_err(invalid)?),
            workload_targets: workloads,
            node_control: nodes,
        },
        AgentExecutionFlowConfig::new(AgentExecutionFlowConfigOptions {
            heartbeat_timeout_ms: 60_000,
            command_ttl_ms: 60_000,
            observation_poll_ms: 1,
            convergence_timeout_ms: 60_000,
        })
        .map_err(invalid)?,
    ))
}
