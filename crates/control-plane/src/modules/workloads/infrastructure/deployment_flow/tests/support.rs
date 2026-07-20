use super::*;

pub(super) fn runtime(
    workloads: &Arc<InMemoryWorkloadRepository>,
    nodes: &Arc<InMemoryNodeRepository>,
    convergence_timeout: Duration,
) -> Result<DeploymentFlowRuntime, String> {
    let workload_port: Arc<dyn IWorkloadRepository> = workloads.clone();
    let node_port: Arc<dyn INodeRepository> = nodes.clone();
    let control_port: Arc<dyn INodeControlRepository> = nodes.clone();
    let milliseconds = u64::try_from(convergence_timeout.num_milliseconds())
        .map_err(|_| "test convergence timeout is invalid")?;
    let runtime_apply_timeout = (milliseconds / 2).max(1);
    DeploymentFlowRuntime::new(
        workload_port,
        Arc::new(UnusedArtifactResolver),
        node_port,
        control_port,
        Arc::new(crate::modules::workloads::domain::services::UnroutedDeploymentRouteUpdater),
        Duration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(
            milliseconds,
            runtime_apply_timeout,
            1,
            milliseconds,
            runtime_apply_timeout,
            1,
            milliseconds,
        )?,
    )
}

pub(super) fn gateway_compiler() -> Result<GatewaySnapshotCompiler, String> {
    GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8443".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 5_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
    })
}

pub(super) async fn publish_active_route(
    repository: &InMemoryEdgeRepository,
    compiler: &GatewaySnapshotCompiler,
    workload: &Workload,
    revision_id: WorkloadRevisionId,
    node_id: NodeId,
    staged_at: chrono::DateTime<Utc>,
) -> Result<Route, Box<dyn std::error::Error>> {
    let route_id = RouteId::new();
    let domain_claim_id = DomainClaimId::new();
    let certificate_id = GatewayCertificateId::new();
    let command_id = NodeCommandId::new();
    let correlation_id = Uuid::now_v7();
    let mut route = Route::create(
        route_id,
        workload.organization_id,
        workload.project_id,
        workload.environment_id,
        node_id,
        RouteHostname::parse("update.example.com")?,
        RoutePath::parse("/")?,
        domain_claim_id,
        DomainNamePattern::parse("update.example.com")?,
        certificate_id,
        workload.id,
        revision_id,
        RoutePortName::parse("http")?,
        UpstreamEndpoint::parse("http://127.0.0.1:49151")?,
        staged_at,
    )?;
    let snapshot = compiler.compile(
        node_id,
        1,
        None,
        certificate_id,
        std::slice::from_ref(&route),
    )?;
    route.stage(1, command_id, snapshot.snapshot_digest.clone(), staged_at)?;
    let publication = GatewayPublication::stage(
        node_id,
        command_id,
        correlation_id,
        snapshot,
        staged_at,
        staged_at + Duration::seconds(5),
    )?;
    let certificate_request = publication
        .certificate_request
        .clone()
        .ok_or("initial route publication omitted its certificate request")?;
    let certificate = GatewayCertificate::provision(
        certificate_id,
        workload.organization_id,
        node_id,
        vec![domain_claim_id],
        1,
        command_id,
        publication.snapshot_digest.clone(),
        certificate_request,
        staged_at,
    )?;
    let staged = repository
        .stage_route_publication(StageRoutePublication {
            route,
            certificate,
            publication,
            expected_scope_version: 0,
            idempotency: IdempotencyRequest::new(
                "test.routes",
                "initial-update-route",
                route_id.to_string().as_bytes(),
            )?,
            event: DomainEventEnvelope {
                event_id: Uuid::now_v7(),
                event_key: "edge.route.publication-staged".into(),
                schema_version: 1,
                organization_id: workload.organization_id.as_uuid(),
                aggregate_id: route_id.as_uuid(),
                aggregate_version: 2,
                occurred_at: staged_at,
                correlation_id,
                causation_id: None,
                payload: serde_json::json!({"routeId": route_id}),
            },
        })
        .await?;
    issue_gateway_certificate(
        repository,
        &staged.certificate,
        staged_at + Duration::milliseconds(1),
    )
    .await?;
    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: staged.publication.command_id.as_uuid(),
        node_id: staged.publication.node_id.as_uuid(),
        revision: staged.publication.revision,
        snapshot_digest: staged.publication.snapshot_digest,
        state: GatewayAckState::Applied,
        message: None,
        acknowledged_at: staged_at + Duration::milliseconds(2),
    };
    repository
        .project_gateway_acknowledgement(
            &acknowledgement,
            acknowledgement.acknowledged_at + Duration::milliseconds(1),
        )
        .await?;
    Ok(repository
        .find_route(workload.organization_id, route_id)
        .await?)
}

pub(super) async fn issue_cutover_certificate(
    repository: &InMemoryEdgeRepository,
    cutover: &GatewayRouteCutover,
) -> Result<(), Box<dyn std::error::Error>> {
    let certificate = repository
        .find_gateway_certificate(cutover.node_id, cutover.gateway_certificate_id)
        .await?;
    issue_gateway_certificate(
        repository,
        &certificate,
        cutover.staged_at + Duration::milliseconds(1),
    )
    .await
}

pub(super) fn cutover_acknowledgement(
    cutover: &GatewayRouteCutover,
    state: GatewayAckState,
) -> NodeGatewayAck {
    NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: cutover.gateway_command_id.as_uuid(),
        node_id: cutover.node_id.as_uuid(),
        revision: cutover.gateway_revision,
        snapshot_digest: cutover.snapshot_digest.clone(),
        state,
        message: (state == GatewayAckState::Rejected).then(|| "candidate rejected".into()),
        acknowledged_at: cutover.staged_at + Duration::milliseconds(2),
    }
}

async fn issue_gateway_certificate(
    repository: &InMemoryEdgeRepository,
    certificate: &GatewayCertificate,
    issued_at: chrono::DateTime<Utc>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut issued = certificate.clone();
    let expected_version = issued.aggregate_version;
    issued.record_issued(
        format!("sha256:{}", "b".repeat(64)),
        GatewayCertificateMaterial {
            serial_number: issued.id.to_string(),
            fingerprint: format!("sha256:{}", "a".repeat(64)),
            certificate_pem: "-----BEGIN CERTIFICATE-----\ndGVzdA==\n-----END CERTIFICATE-----\n"
                .into(),
            ca_bundle_pem: "-----BEGIN CERTIFICATE-----\ndGVzdC1jYQ==\n-----END CERTIFICATE-----\n"
                .into(),
            issued_at,
            expires_at: issued_at + Duration::days(30),
        },
        issued_at,
    )?;
    repository
        .transition_gateway_certificate(issued, expected_version)
        .await?;
    Ok(())
}

pub(super) struct UnusedArtifactResolver;

#[async_trait]
impl IOciArtifactResolver for UnusedArtifactResolver {
    async fn resolve(
        &self,
        _reference: &crate::modules::workloads::domain::entities::OciArtifactReference,
        _registry_credential: Option<
            &crate::modules::workloads::domain::services::OciRegistryCredentialReference,
        >,
    ) -> Result<OciArtifact, OciArtifactResolutionError> {
        Err(OciArtifactResolutionError::Registry(
            "resolved deployment fixture unexpectedly called the OCI resolver".into(),
        ))
    }
}

pub(super) struct MovingArtifactResolver {
    digest: RwLock<String>,
    calls: AtomicUsize,
    registry_credential:
        RwLock<Option<crate::modules::workloads::domain::services::OciRegistryCredentialReference>>,
}

impl MovingArtifactResolver {
    pub(super) fn new(digest: String) -> Self {
        Self {
            digest: RwLock::new(digest),
            calls: AtomicUsize::new(0),
            registry_credential: RwLock::new(None),
        }
    }

    pub(super) fn move_tag(&self, digest: String) {
        *self.digest.write().expect("moving resolver lock") = digest;
    }

    pub(super) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub(super) fn registry_credential(
        &self,
    ) -> Option<crate::modules::workloads::domain::services::OciRegistryCredentialReference> {
        *self
            .registry_credential
            .read()
            .expect("moving resolver credential lock")
    }
}

#[async_trait]
impl IOciArtifactResolver for MovingArtifactResolver {
    async fn resolve(
        &self,
        reference: &OciArtifactReference,
        registry_credential: Option<
            &crate::modules::workloads::domain::services::OciRegistryCredentialReference,
        >,
    ) -> Result<OciArtifact, OciArtifactResolutionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.registry_credential.write().map_err(|_| {
            OciArtifactResolutionError::Registry("resolver credential lock poisoned".into())
        })? = registry_credential.copied();
        let digest = self
            .digest
            .read()
            .map_err(|_| OciArtifactResolutionError::Registry("resolver lock poisoned".into()))?
            .clone();
        let repository = reference
            .repository()
            .map_err(OciArtifactResolutionError::InvalidReference)?;
        Ok(OciArtifact {
            uri: format!("oci://{repository}@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        })
    }
}

pub(super) async fn ready_node(
    nodes: &Arc<InMemoryNodeRepository>,
    organization_id: OrganizationId,
    enrolled_at: chrono::DateTime<Utc>,
) -> Result<
    (
        crate::modules::shared_kernel::domain::NodeId,
        Uuid,
        RuntimeCapabilities,
    ),
    Box<dyn std::error::Error>,
> {
    let secret = format!("a3sn_{}", "d".repeat(64));
    let credential = EnrollmentTokenCredential::from_secret(&secret)?;
    let token = EnrollmentToken::new(
        EnrollmentTokenId::new(),
        organization_id,
        "deployment-test",
        credential.clone(),
        enrolled_at,
        enrolled_at + Duration::minutes(5),
    )?;
    nodes
        .issue_enrollment_token(
            token,
            event(organization_id),
            IdempotencyRequest::new("test.enrollment", "deployment-test", b"token")?,
        )
        .await?;
    let runtime_capabilities = capabilities();
    let node_capabilities = NodeCapabilities::new(
        runtime_capabilities.provider_id.to_string(),
        runtime_capabilities.provider_build.clone(),
        serde_json::to_value(&runtime_capabilities)?,
    )?;
    let agent_instance_id = Uuid::now_v7();
    let reservation = nodes
        .reserve_enrollment(
            &credential,
            NodeEnrollmentDraft {
                proposed_node_id: crate::modules::shared_kernel::domain::NodeId::new(),
                name: NodeName::new("deployment-node")?,
                agent_instance_id,
                agent_version: "0.1.0".into(),
                capabilities: node_capabilities.clone(),
                request_digest: format!("sha256:{}", "e".repeat(64)),
                requested_at: enrolled_at,
            },
        )
        .await?;
    nodes
        .record_heartbeat(NodeHeartbeatUpdate {
            node_id: reservation.node.id,
            agent_instance_id,
            agent_version: "0.1.0".into(),
            capabilities: node_capabilities,
            observed_at: enrolled_at + Duration::milliseconds(1),
        })
        .await?;
    Ok((reservation.node.id, agent_instance_id, runtime_capabilities))
}

pub(super) fn deployment_bundle(
    workload: Workload,
    generation: u64,
    digest_character: char,
    requested_at: chrono::DateTime<Utc>,
    idempotency_key: &str,
) -> Result<CreateDeploymentBundle, Box<dyn std::error::Error>> {
    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        generation,
        template(digest_character),
        requested_at,
    )?;
    let deployment = Deployment::create(
        DeploymentId::new(),
        workload.organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        requested_at,
    );
    let operation = OperationRequest::new(
        deployment.operation_id,
        workload.organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid())?,
        WorkflowIdentity::new("cloud.deployment", "2")?,
        serde_json::json!({
            "deploymentId": deployment.id,
            "organizationId": workload.organization_id,
            "revisionId": revision.id,
            "workloadId": workload.id,
        }),
        requested_at,
    );
    let event = DeploymentRequested::envelope(&deployment, &revision, Uuid::now_v7())?;
    Ok(CreateDeploymentBundle {
        workload,
        revision,
        deployment,
        operation,
        idempotency: IdempotencyRequest::new(
            "test.workload.deploy",
            idempotency_key,
            idempotency_key.as_bytes(),
        )?,
        event,
    })
}

pub(super) fn requested_deployment_bundle(
    workload: Workload,
    requested_at: chrono::DateTime<Utc>,
    idempotency_key: &str,
) -> Result<CreateDeploymentBundle, Box<dyn std::error::Error>> {
    requested_deployment_bundle_with_secrets(workload, requested_at, idempotency_key, Vec::new())
}

pub(super) fn requested_deployment_bundle_with_secrets(
    workload: Workload,
    requested_at: chrono::DateTime<Utc>,
    idempotency_key: &str,
    secrets: Vec<crate::modules::workloads::domain::entities::SecretBinding>,
) -> Result<CreateDeploymentBundle, Box<dyn std::error::Error>> {
    let resolved = template('a');
    let request = RequestedServiceTemplate {
        artifact: OciArtifactReference {
            uri: "oci://registry.example/cloud/test:stable".into(),
            expected_digest: None,
        },
        process: resolved.process,
        secrets,
        resources: resolved.resources,
        ports: resolved.ports,
        health: resolved.health,
    };
    let revision = WorkloadRevision::request(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        request,
        requested_at,
    )?;
    let deployment = Deployment::create(
        DeploymentId::new(),
        workload.organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        requested_at,
    );
    let operation = OperationRequest::new(
        deployment.operation_id,
        workload.organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid())?,
        WorkflowIdentity::new("cloud.deployment", "2")?,
        serde_json::json!({
            "deploymentId": deployment.id,
            "organizationId": workload.organization_id,
            "revisionId": revision.id,
            "workloadId": workload.id,
        }),
        requested_at,
    );
    let event = DeploymentRequested::envelope(&deployment, &revision, Uuid::now_v7())?;
    Ok(CreateDeploymentBundle {
        workload,
        revision,
        deployment,
        operation,
        idempotency: IdempotencyRequest::new(
            "test.workload.deploy",
            idempotency_key,
            idempotency_key.as_bytes(),
        )?,
        event,
    })
}

pub(super) async fn lease(
    nodes: &InMemoryNodeRepository,
    node_id: crate::modules::shared_kernel::domain::NodeId,
    agent_instance_id: Uuid,
    after_sequence: u64,
) -> Result<
    a3s_cloud_contracts::NodeCommandLeaseResponse,
    crate::modules::shared_kernel::domain::RepositoryError,
> {
    let now = Utc::now();
    nodes
        .lease_commands(
            &NodeCommandLeaseRequest {
                schema: NodeCommandLeaseRequest::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                after_sequence,
                max_commands: 10,
                wait_ms: 0,
            },
            Uuid::now_v7(),
            now,
            now + Duration::seconds(1),
        )
        .await
}

pub(super) async fn record_observation(
    nodes: &InMemoryNodeRepository,
    node_id: crate::modules::shared_kernel::domain::NodeId,
    agent_instance_id: Uuid,
    capabilities: &RuntimeCapabilities,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    observation: RuntimeObservation,
) -> Result<(), Box<dyn std::error::Error>> {
    let observed_at = Utc::now();
    nodes
        .record_observations(
            NodeObservationBatch {
                schema: NodeObservationBatch::SCHEMA.into(),
                node_id: node_id.as_uuid(),
                agent_instance_id,
                sent_at: observed_at,
                heartbeat: NodeHeartbeat {
                    schema: NodeHeartbeat::SCHEMA.into(),
                    node_id: node_id.as_uuid(),
                    agent_instance_id,
                    observed_at,
                    agent_version: "0.1.0".into(),
                    runtime_capabilities: capabilities.clone(),
                },
                observations: vec![RuntimeObservationReport {
                    report_id: command.command_id,
                    command_id: Some(command.command_id),
                    observed_at,
                    observation,
                }],
            },
            observed_at,
        )
        .await?;
    Ok(())
}

pub(super) fn healthy_observation(
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
    health_state: RuntimeHealthState,
) -> Result<RuntimeObservation, String> {
    let now_ms = u64::try_from(Utc::now().timestamp_millis())
        .map_err(|_| "test clock predates Unix epoch")?;
    let spec_digest = spec.digest()?;
    let endpoint_claims = spec
        .network
        .ports
        .iter()
        .filter(|port| port.protocol == TransportProtocol::Tcp)
        .enumerate()
        .map(|(index, port)| {
            let host_port = 49_152_u16
                .checked_add(
                    u16::try_from(index)
                        .map_err(|_| "test Runtime observation has too many service ports")?,
                )
                .ok_or("test Runtime observation service port range overflowed")?;
            let endpoint = RuntimeServiceEndpoint::node_local_http(&port.name, host_port)?;
            Ok((endpoint.claim_key(), endpoint.origin))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec_digest.clone(),
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Running,
        provider_resource_id: Some(format!("container-{}", spec.generation)),
        provider_build: Some("test-runtime-1".into()),
        observed_at_ms: now_ms,
        started_at_ms: Some(now_ms),
        finished_at_ms: None,
        health: Some(RuntimeHealthObservation {
            state: health_state,
            checked_at_ms: now_ms,
            message: (health_state == RuntimeHealthState::Unhealthy)
                .then(|| "HTTP probe did not stabilize".into()),
        }),
        outputs: Vec::new(),
        usage: None,
        evidence: Some(RuntimeEvidence {
            provider_build: "test-runtime-1".into(),
            spec_digest,
            semantics_profile_digest: spec.semantics_profile_digest.clone(),
            claims: endpoint_claims,
        }),
        provider_attestation: None,
        failure: None,
    };
    observation.validate_against(spec)?;
    Ok(observation)
}

pub(super) fn stopped_observation(
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> Result<RuntimeObservation, String> {
    let now_ms = u64::try_from(Utc::now().timestamp_millis())
        .map_err(|_| "test clock predates Unix epoch")?;
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec.digest()?,
        class: RuntimeUnitClass::Service,
        state: RuntimeUnitState::Stopped,
        provider_resource_id: Some(format!("container-{}", spec.generation)),
        provider_build: Some("test-runtime-1".into()),
        observed_at_ms: now_ms,
        started_at_ms: Some(now_ms.saturating_sub(1)),
        finished_at_ms: Some(now_ms),
        health: None,
        outputs: Vec::new(),
        usage: None,
        evidence: None,
        provider_attestation: None,
        failure: None,
    };
    observation.validate_against(spec)?;
    Ok(observation)
}

pub(super) fn capabilities() -> RuntimeCapabilities {
    RuntimeCapabilities {
        schema: RuntimeCapabilities::SCHEMA.into(),
        provider_id: a3s_runtime::ProviderId::parse("test-runtime")
            .expect("valid test provider ID"),
        provider_build: "test-runtime-1".into(),
        unit_classes: vec![RuntimeUnitClass::Service],
        artifact_media_types: vec!["application/vnd.oci.image.manifest.v1+json".into()],
        isolation_levels: vec![IsolationLevel::Container],
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
        ],
    }
}

pub(super) fn template(digest_character: char) -> ServiceTemplate {
    let digest = format!("sha256:{}", digest_character.to_string().repeat(64));
    ServiceTemplate {
        artifact: OciArtifact {
            uri: format!("oci://registry.example/cloud/test@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ServiceProcess {
            command: vec!["/fixture".into()],
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        secrets: Vec::new(),
        resources: ServiceResources {
            cpu_millis: 100,
            memory_bytes: 32 * 1024 * 1024,
            pids: 32,
            ephemeral_storage_bytes: None,
        },
        ports: vec![ServicePort {
            name: "http".into(),
            container_port: 8080,
        }],
        health: HttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 10,
            timeout_ms: 5,
            healthy_threshold: 1,
            unhealthy_threshold: 1,
            stabilization_window_ms: 1,
        },
    }
}

pub(super) fn workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("cloud.deployment", "2", "a3s-cloud", "main")
}

pub(super) fn legacy_workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("cloud.deployment", "1", "a3s-cloud", "main")
}

pub(super) fn stop_workflow_spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("cloud.workload.stop", "1", "a3s-cloud", "main")
}

pub(super) fn event(organization_id: OrganizationId) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: "fleet.enrollment-token.issued".into(),
        schema_version: 1,
        organization_id: organization_id.as_uuid(),
        aggregate_id: Uuid::now_v7(),
        aggregate_version: 1,
        occurred_at: Utc::now(),
        correlation_id: Uuid::now_v7(),
        causation_id: None,
        payload: serde_json::json!({}),
    }
}

pub(super) struct FailOnceStepCompletionStore {
    inner: InMemoryEventStore,
    step_id: &'static str,
    armed: AtomicBool,
}

impl FailOnceStepCompletionStore {
    pub(super) fn new(step_id: &'static str) -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            step_id,
            armed: AtomicBool::new(true),
        }
    }
}

#[async_trait]
impl FlowEventStore for FailOnceStepCompletionStore {
    async fn append(&self, run_id: &str, event: FlowEvent) -> Result<FlowEventEnvelope, FlowError> {
        self.inner.append(run_id, event).await
    }

    async fn append_if_sequence(
        &self,
        run_id: &str,
        expected_sequence: u64,
        event: FlowEvent,
    ) -> Result<FlowEventEnvelope, FlowError> {
        let is_target = matches!(
            &event,
            FlowEvent::StepCompleted { step_id, .. } if step_id == self.step_id
        );
        if is_target && self.armed.swap(false, Ordering::SeqCst) {
            return Err(FlowError::Store(format!(
                "injected loss before persisting {run_id} step {} completion",
                self.step_id
            )));
        }
        self.inner
            .append_if_sequence(run_id, expected_sequence, event)
            .await
    }

    async fn list(&self, run_id: &str) -> Result<Vec<FlowEventEnvelope>, FlowError> {
        self.inner.list(run_id).await
    }

    async fn list_run_ids(&self) -> Result<Vec<String>, FlowError> {
        self.inner.list_run_ids().await
    }
}
