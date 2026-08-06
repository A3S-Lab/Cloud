use a3s_cloud_contracts::{
    GatewayAckState, GatewayManagementProtocol, McpGrantProjection, McpLimitsProjection,
    NodeCommandPayload, NodeGatewayAck, MCP_PROTOCOL_VERSION,
};
use a3s_cloud_control_plane::modules::assets::{
    Asset, AssetCreated, AssetKind, AssetRelease, AssetReleaseDrafted, AssetReleaseVersion,
    CreateAssetReleaseWrite, CreateAssetWrite, IAssetRepository, IMcpServiceProfileRepository,
    McpServiceProfile, McpServiceProfileBinding, McpServiceProfileSpec, PostgresAssetRepository,
};
use a3s_cloud_control_plane::modules::edge::domain::events::{
    DomainClaimChanged, McpCredentialChanged,
};
use a3s_cloud_control_plane::modules::edge::domain::repositories::CreateMcpCredentialWrite;
use a3s_cloud_control_plane::modules::edge::{
    CompileMcpGatewaySnapshot, CreateDomainClaimWrite, DomainClaim, DomainNamePattern,
    FleetGatewayCommandQueue, GatewayCertificateMaterial, GatewayCertificateState,
    GatewayPublicationState, GatewaySnapshotCompiler, GatewaySnapshotCompilerConfig,
    GatewaySnapshotMetadata, GatewaySnapshotRouteInput, IEdgeRepository,
    IMcpCredentialLifecycleRepository, IMcpCredentialRepository, IMcpGatewaySnapshotRepository,
    IMcpRoutePolicyRepository, IRouteTargetReader, McpCredential, McpCredentialDeliveryReceipt,
    McpGatewayDesiredStateReconciler, McpGatewayNodeProjectionPlanner,
    McpGatewayProjectionAssembler, McpGatewayProjectionPlanner, McpGatewayProjectionSetPlanner,
    McpGatewaySnapshotReconciler, McpRoutePolicy, McpRoutePolicySpec,
    McpRouteProjectionInputReader, McpRouteProjectionPlanner, McpRouteTargetProjectionCompiler,
    PlanMcpGatewayProjectionSet, PlannedMcpGatewayNodeProjection, PostgresEdgeRepository,
    ResolvedRouteTarget, ResolvedRouteTargetSet, RouteHostname, RoutePortName, RouteTarget,
    StageMcpGatewaySnapshot, TransitionDomainClaim, UpstreamEndpoint,
};
use a3s_cloud_control_plane::modules::fleet::domain::entities::NodeCommandDraft;
use a3s_cloud_control_plane::modules::fleet::domain::repositories::INodeControlRepository;
use a3s_cloud_control_plane::modules::fleet::PostgresNodeRepository;
use a3s_cloud_control_plane::modules::operations::{
    OperationRequest, OperationSubject, WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::secrets::domain::EncryptedSecretValue;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId,
    GatewayScopeId, GitCommitSha, IdempotencyRequest, McpCredentialId, NodeCommandId, NodeId,
    OperationId, OrganizationId, ProjectId, RepositoryError, ResourceName, RouteId, Sha256Digest,
    WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_control_plane::modules::workloads::application::{
    DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION,
};
use a3s_cloud_control_plane::modules::workloads::infrastructure::project_runtime_spec;
use a3s_cloud_control_plane::modules::workloads::{
    CreateDeploymentBundle, Deployment, DeploymentRequested, HttpHealthCheck, IWorkloadRepository,
    OciArtifact, PostgresWorkloadRepository, ServicePort, ServiceProcess, ServiceResources,
    ServiceTemplate, Workload, WorkloadControlSpec, WorkloadRevision,
};
use a3s_orm::{select_from, sql_query, Database, PostgresDialect, PostgresExecutor};
use a3s_runtime::contract::RuntimeApplyRequest;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const ROTATED_VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[path = "mcp_route_policies/node_aggregation.rs"]
mod node_aggregation;

pub async fn exercise(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    other_organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> TestResult {
    let edge = PostgresEdgeRepository::new(executor.clone());
    let database = Database::new(PostgresDialect, executor.clone());
    let mut scope = None;
    for candidate in edge
        .list_gateway_scopes(organization_id, project_id, environment_id)
        .await?
    {
        let pending_publications = database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from gateway_publications where node_id = ")
                    .bind(candidate.node_id.as_uuid())
                    .append(" and state = 'pending'"),
            )
            .await?;
        if pending_publications == 0 && edge.active_routes(candidate.node_id).await?.is_empty() {
            scope = Some(candidate);
            break;
        }
    }
    let scope = scope.ok_or("MCP route policy fixture has no route-less Gateway scope")?;
    let assets = PostgresAssetRepository::new(executor.clone());
    let now = Utc::now();
    let credential = McpCredential::issue(
        McpCredentialId::new(),
        organization_id,
        project_id,
        environment_id,
        "a3s_mcp_abc12345def67890",
        VERIFIER,
        now + Duration::days(30),
        now,
    )?;
    let delivery_expires_at = now + Duration::minutes(10);
    let delivery_idempotency = idempotency(
        organization_id,
        "mcp-credentials",
        "postgres-mcp-credential-create",
        b"postgres-mcp-credential-create",
    )?;
    let delivery_receipt = McpCredentialDeliveryReceipt::new(
        organization_id,
        credential.id,
        credential.generation(),
        EncryptedSecretValue::new("test-key", "encrypted-test-credential")?,
        delivery_expires_at,
        now,
    )?;
    let created_delivery = edge
        .create_mcp_credential_delivery(CreateMcpCredentialWrite {
            credential: credential.clone(),
            receipt: delivery_receipt.clone(),
            idempotency: delivery_idempotency.clone(),
            event: McpCredentialChanged::created(&credential, Uuid::now_v7())?,
        })
        .await?;
    assert_eq!(created_delivery.credential, credential);
    assert_eq!(created_delivery.receipt, Some(delivery_receipt.clone()));
    assert!(!created_delivery.replayed);
    let replayed_delivery = edge
        .replay_mcp_credential_write(organization_id, &delivery_idempotency)
        .await?
        .ok_or("MCP credential delivery replay is missing")?;
    assert_eq!(replayed_delivery.credential, credential);
    assert_eq!(replayed_delivery.receipt, Some(delivery_receipt));
    assert!(replayed_delivery.replayed);
    assert_eq!(
        edge.sweep_expired_mcp_credential_delivery_receipts(now, 100)
            .await?,
        0
    );
    assert_eq!(
        edge.sweep_expired_mcp_credential_delivery_receipts(delivery_expires_at, 100)
            .await?,
        1
    );
    let replayed_after_sweep = edge
        .replay_mcp_credential_write(organization_id, &delivery_idempotency)
        .await?
        .ok_or("MCP credential replay disappeared after receipt sweep")?;
    assert_eq!(replayed_after_sweep.credential, credential);
    assert!(replayed_after_sweep.receipt.is_none());
    assert!(replayed_after_sweep.replayed);
    assert_eq!(
        edge.find_mcp_credential(organization_id, credential.id)
            .await?,
        Some(credential.clone())
    );
    assert_eq!(
        edge.find_mcp_credential(other_organization_id, credential.id)
            .await?,
        None
    );
    assert_eq!(
        edge.list_mcp_credentials(organization_id, project_id, environment_id)
            .await?,
        vec![credential.clone()]
    );
    assert_eq!(
        edge.resolve_mcp_credentials(
            organization_id,
            project_id,
            environment_id,
            &[McpCredentialId::new(), credential.id],
        )
        .await?,
        vec![credential.clone()]
    );
    assert!(edge
        .resolve_mcp_credentials(
            other_organization_id,
            project_id,
            environment_id,
            &[credential.id],
        )
        .await?
        .is_empty());
    assert!(matches!(
        edge.resolve_mcp_credentials(
            organization_id,
            project_id,
            environment_id,
            &[credential.id, credential.id],
        )
        .await,
        Err(RepositoryError::Conflict(_))
    ));
    let duplicate_prefix = McpCredential::issue(
        McpCredentialId::new(),
        organization_id,
        project_id,
        environment_id,
        credential.prefix(),
        VERIFIER,
        now + Duration::days(30),
        now,
    )?;
    assert!(matches!(
        edge.create_mcp_credential(duplicate_prefix).await,
        Err(RepositoryError::Conflict(_))
    ));
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("Route Policy MCP")?,
        AssetKind::Mcp,
        now,
    )?;
    assets
        .create_asset(CreateAssetWrite {
            asset: asset.clone(),
            event: AssetCreated::envelope(&asset, Uuid::now_v7())?,
            idempotency: idempotency(
                organization_id,
                "assets",
                "postgres-mcp-policy-asset",
                b"mcp-policy-asset",
            )?,
        })
        .await?;
    let release = AssetRelease::draft(
        &asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0")?,
        GitCommitSha::parse("a".repeat(40))?,
        digest('b')?,
        now + Duration::milliseconds(1),
    )?;
    assets
        .create_release(CreateAssetReleaseWrite {
            release: release.clone(),
            event: AssetReleaseDrafted::envelope(&release, Uuid::now_v7())?,
            idempotency: idempotency(
                organization_id,
                format!("assets/{}/releases", asset.id),
                "postgres-mcp-policy-release",
                b"mcp-policy-release",
            )?,
        })
        .await?;
    let published =
        crate::build_runs_support::publish_hosted_release(executor, &asset, &release).await?;
    let published_artifact = published
        .artifact
        .as_ref()
        .ok_or("published MCP release omitted its OCI artifact")?;
    let profile = McpServiceProfile::from_spec(McpServiceProfileSpec {
        protocol_versions: vec![MCP_PROTOCOL_VERSION.into()],
        endpoint_path: "/mcp".into(),
        runtime_port: "mcp".into(),
        health_path: "/health".into(),
        request_sse: true,
        subscriptions: true,
        server_discover: true,
        expected_capabilities: vec!["subscriptions".into(), "tools".into()],
        max_request_bytes: 1_048_576,
        max_response_bytes: 8_388_608,
        max_stream_seconds: 3_600,
    })?;
    let profile_binding = McpServiceProfileBinding {
        organization_id,
        asset_id: asset.id,
        asset_release_id: published.id,
        profile: profile.clone(),
        created_at: published.updated_at + Duration::milliseconds(1),
    };
    assets
        .bind_mcp_service_profile(profile_binding.clone())
        .await?;

    let workload_created_at = profile_binding.created_at + Duration::milliseconds(1);
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse("MCP policy runtime")?,
        workload_created_at,
    );
    let mut revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        workload.id,
        1,
        ServiceTemplate {
            artifact: OciArtifact {
                uri: format!(
                    "oci://registry.integration.example/mcp/policy@{}",
                    published_artifact.digest()
                ),
                digest: published_artifact.digest().to_string(),
                media_type: published_artifact.media_type().into(),
            },
            process: ServiceProcess {
                command: vec!["/app/mcp-server".into()],
                args: vec!["serve".into()],
                working_directory: Some("/app".into()),
                environment: BTreeMap::new(),
            },
            secrets: Vec::new(),
            resources: ServiceResources {
                cpu_millis: 500,
                memory_bytes: 256 * 1024 * 1024,
                pids: 128,
                ephemeral_storage_bytes: Some(1024 * 1024 * 1024),
            },
            ports: vec![ServicePort {
                name: "mcp".into(),
                container_port: 8080,
            }],
            health: Some(HttpHealthCheck {
                port_name: "mcp".into(),
                path: "/health".into(),
                interval_ms: 10_000,
                timeout_ms: 2_000,
                healthy_threshold: 1,
                unhealthy_threshold: 3,
                stabilization_window_ms: 30_000,
            }),
        },
        workload_created_at,
    )?;
    revision.bind_mcp_release(&workload, &asset, &published, &profile_binding)?;
    let deployment = Deployment::create(
        DeploymentId::new(),
        organization_id,
        workload.id,
        revision.id,
        OperationId::new(),
        workload_created_at,
    );
    let operation = OperationRequest::new(
        deployment.operation_id,
        organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid())?,
        WorkflowIdentity::new(DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION)?,
        json!({
            "deploymentId": deployment.id,
            "mcpAssetReleaseId": published.id,
            "mcpProfileDigest": profile.digest(),
            "revisionId": revision.id,
            "workloadId": workload.id,
        }),
        workload_created_at,
    );
    let deployment_request = CreateDeploymentBundle {
        workload: workload.clone(),
        control: WorkloadControlSpec::unmanaged_single_replica(),
        revision: revision.clone(),
        deployment: deployment.clone(),
        operation,
        idempotency: idempotency(
            organization_id,
            "workloads",
            "postgres-mcp-workload",
            revision.request_digest.as_bytes(),
        )?,
        event: DeploymentRequested::envelope(&deployment, &revision, Uuid::now_v7())?,
    };
    let workloads = PostgresWorkloadRepository::new(executor.clone());
    let created = workloads
        .create_deployment(deployment_request.clone())
        .await?;
    let replay = workloads.create_deployment(deployment_request).await?;
    assert!(!created.replayed);
    assert!(replay.replayed);
    assert_eq!(created.revision.mcp_binding(), revision.mcp_binding());
    let stored_revision = workloads
        .find_revision(organization_id, revision.id)
        .await?;
    assert_eq!(stored_revision, revision);
    assert!(matches!(
        workloads
            .find_revision(other_organization_id, revision.id)
            .await,
        Err(RepositoryError::NotFound)
    ));
    let runtime_spec = project_runtime_spec(&stored_revision)?;
    assert_eq!(
        runtime_spec.semantics_profile_digest.as_deref(),
        Some(profile.digest().as_str())
    );
    let stored_binding = Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(
            select_from::<McpWorkloadEvidence>()
                .select((
                    McpWorkloadEvidence::mcp_asset_release_id(),
                    McpWorkloadEvidence::mcp_profile_digest(),
                ))
                .filter(McpWorkloadEvidence::id().eq(revision.id.as_uuid())),
        )
        .await?;
    assert_eq!(
        stored_binding,
        (
            Some(published.id.as_uuid()),
            Some(profile.digest().to_string())
        )
    );
    let resolving = workloads
        .mark_resolving(
            deployment.id,
            deployment.aggregate_version,
            workload_created_at + Duration::milliseconds(1),
        )
        .await?;
    let scheduled = workloads
        .assign_node(
            deployment.id,
            resolving.aggregate_version,
            scope.node_id,
            workload_created_at + Duration::milliseconds(2),
        )
        .await?;
    let command_id = NodeCommandId::from_uuid(deployment.id.as_uuid());
    let command_deadline = scheduled.updated_at + Duration::minutes(5);
    let command = PostgresNodeRepository::new(executor.clone())
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id: scope.node_id,
            aggregate_id: deployment.workload_id.as_uuid(),
            payload: NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("deployment:{}:apply", deployment.id),
                    deadline_at_ms: Some(u64::try_from(command_deadline.timestamp_millis())?),
                    spec: runtime_spec,
                }),
                resource_claim: None,
            },
            issued_at: scheduled.updated_at,
            not_after: command_deadline,
            correlation_id: deployment.operation_id.as_uuid(),
        })
        .await?;
    assert!(!command.replayed);
    let applying = workloads
        .mark_dispatched(
            deployment.id,
            scheduled.aggregate_version,
            command.value.id,
            workload_created_at + Duration::milliseconds(3),
        )
        .await?;
    let verifying = workloads
        .mark_verifying(
            deployment.id,
            applying.aggregate_version,
            workload_created_at + Duration::milliseconds(4),
        )
        .await?;
    let (active_workload, _) = workloads
        .activate(
            deployment.id,
            verifying.aggregate_version,
            false,
            workload_created_at + Duration::milliseconds(5),
        )
        .await?;
    assert_eq!(active_workload.active_revision_id, Some(revision.id));

    let workload_id = workload.id;
    let created_at = workload_created_at + Duration::milliseconds(1);
    let hostname = RouteHostname::parse("mcp-policy.integration.example")?;
    let mut domain_claim = DomainClaim::create(
        DomainClaimId::new(),
        organization_id,
        project_id,
        environment_id,
        DomainNamePattern::parse(hostname.as_str())?,
        format!("a3s-cloud-verification={}", Uuid::now_v7()),
        created_at - Duration::milliseconds(2),
    )?;
    edge.create_domain_claim(CreateDomainClaimWrite {
        claim: domain_claim.clone(),
        idempotency: idempotency(
            organization_id,
            "domain-claims",
            "postgres-mcp-domain-claim",
            hostname.as_str().as_bytes(),
        )?,
        event: DomainClaimChanged::envelope(&domain_claim, Uuid::now_v7())?,
    })
    .await?;
    let expected_claim_version = domain_claim.aggregate_version;
    domain_claim.verify(created_at - Duration::milliseconds(1))?;
    edge.transition_domain_claim(TransitionDomainClaim {
        claim: domain_claim.clone(),
        expected_version: expected_claim_version,
        idempotency: idempotency(
            organization_id,
            format!("domain-claims/{}", domain_claim.id),
            "postgres-mcp-domain-verification",
            b"verified",
        )?,
        event: DomainClaimChanged::envelope(&domain_claim, Uuid::now_v7())?,
    })
    .await?;
    let policy = McpRoutePolicy::create(
        McpRoutePolicySpec {
            route_id: RouteId::new(),
            organization_id,
            project_id,
            environment_id,
            gateway_scope_id: scope.id,
            domain_claim_id: domain_claim.id,
            workload_id,
            asset_id: asset.id,
            asset_release_id: published.id,
            profile_digest: profile.digest().clone(),
            hostname,
            path: "/mcp".into(),
            tls_required: true,
            allowed_origins: vec!["https://console.integration.example".into()],
            max_header_bytes: 32_768,
            max_request_bytes: 524_288,
            max_response_bytes: 4_194_304,
            first_response_timeout_seconds: 30,
            stream_idle_timeout_seconds: 120,
            stream_total_timeout_seconds: 1_800,
            drain_timeout_seconds: 30,
            telemetry_names: vec!["weather".into()],
            telemetry_events_per_minute: 10_000,
            audit_required: true,
            expires_at: created_at + Duration::hours(1),
            grants: vec![McpGrantProjection {
                credential_id: credential.id.as_uuid(),
                credential_generation: credential.generation(),
                methods: vec![
                    "server/discover".into(),
                    "tools/list".into(),
                    "tools/call".into(),
                ],
                names: vec!["weather".into()],
                limits: McpLimitsProjection {
                    max_concurrent_requests: 8,
                    requests_per_minute: 120,
                    request_burst: 16,
                },
            }],
        },
        &profile,
        created_at,
    )?;
    let mut unowned_spec = policy.spec().clone();
    unowned_spec.route_id = RouteId::new();
    unowned_spec.domain_claim_id = DomainClaimId::new();
    unowned_spec.hostname = RouteHostname::parse("unowned-mcp.integration.example")?;
    let unowned = McpRoutePolicy::create(unowned_spec, &profile, created_at)?;
    assert!(matches!(
        edge.create_mcp_route_policy(unowned).await,
        Err(RepositoryError::NotFound)
    ));
    assert_eq!(edge.create_mcp_route_policy(policy.clone()).await?, policy);
    assert_eq!(
        edge.find_mcp_route_policy(organization_id, policy.spec().route_id)
            .await?,
        Some(policy.clone())
    );
    assert_eq!(
        edge.find_mcp_route_policy(other_organization_id, policy.spec().route_id)
            .await?,
        None
    );
    assert_eq!(
        edge.list_mcp_route_policies(organization_id, project_id, environment_id)
            .await?,
        vec![policy.clone()]
    );
    assert_eq!(
        edge.list_active_mcp_route_policies_for_gateway(
            organization_id,
            project_id,
            environment_id,
            scope.id,
            policy.updated_at(),
        )
        .await?,
        vec![policy.clone()]
    );
    assert!(edge
        .list_active_mcp_route_policies_for_gateway(
            organization_id,
            project_id,
            environment_id,
            scope.id,
            policy.spec().expires_at,
        )
        .await?
        .is_empty());
    assert!(edge
        .list_active_mcp_route_policies_for_gateway(
            organization_id,
            project_id,
            environment_id,
            GatewayScopeId::new(),
            policy.updated_at(),
        )
        .await?
        .is_empty());
    assert!(edge
        .list_active_mcp_route_policies_for_gateway(
            other_organization_id,
            project_id,
            environment_id,
            scope.id,
            policy.updated_at(),
        )
        .await?
        .is_empty());

    let stale_stage = plan_gateway_snapshot(
        &edge,
        &assets,
        &workloads,
        &scope,
        workload_id,
        created_at + Duration::milliseconds(10),
    )
    .await?;
    let scope_before_stale_stage = edge.gateway_scope(scope.node_id).await?;
    assert_eq!(
        stage_artifact_counts(executor, &stale_stage).await?,
        (0, 0, 0, 0)
    );

    let mut revised = policy.clone();
    let mut revised_spec = revised.spec().clone();
    revised_spec.max_request_bytes /= 2;
    revised_spec.expires_at += Duration::minutes(1);
    revised.revise(revised_spec, &profile, created_at + Duration::minutes(1))?;
    assert_eq!(
        edge.update_mcp_route_policy(revised.clone(), 1).await?,
        revised
    );
    assert!(matches!(
        edge.stage_mcp_gateway_snapshot(stale_stage.clone()).await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        edge.gateway_scope(scope.node_id).await?,
        scope_before_stale_stage
    );
    assert_eq!(
        stage_artifact_counts(executor, &stale_stage).await?,
        (0, 0, 0, 0)
    );
    let mut stale = policy;
    let mut stale_spec = stale.spec().clone();
    stale_spec.telemetry_events_per_minute /= 2;
    stale_spec.expires_at += Duration::minutes(2);
    stale.revise(stale_spec, &profile, created_at + Duration::minutes(2))?;
    assert!(matches!(
        edge.update_mcp_route_policy(stale, 1).await,
        Err(RepositoryError::Conflict(_))
    ));

    let (stored_claim_id, stored_acl) = Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(
            select_from::<McpPolicyEvidence>()
                .select((
                    McpPolicyEvidence::domain_claim_id(),
                    McpPolicyEvidence::acl(),
                ))
                .filter(McpPolicyEvidence::id().eq(revised.spec().route_id.as_uuid())),
        )
        .await?;
    assert_eq!(stored_claim_id, domain_claim.id.as_uuid());
    assert!(!stored_acl.contains("verifier_hash"));
    assert!(!stored_acl.contains("targets "));
    assert!(!stored_acl.contains("endpoint ="));

    let current_stage = plan_gateway_snapshot(
        &edge,
        &assets,
        &workloads,
        &scope,
        workload_id,
        created_at + Duration::minutes(1) + Duration::milliseconds(1),
    )
    .await?;
    let scope_before_atomic_stage = edge.gateway_scope(scope.node_id).await?;
    install_snapshot_outbox_failure(executor).await?;
    let injected_failure = edge.stage_mcp_gateway_snapshot(current_stage.clone()).await;
    remove_snapshot_outbox_failure(executor).await?;
    assert!(injected_failure.is_err());
    assert_eq!(
        edge.gateway_scope(scope.node_id).await?,
        scope_before_atomic_stage
    );
    assert_eq!(
        stage_artifact_counts(executor, &current_stage).await?,
        (0, 0, 0, 0)
    );

    let expected_publication = current_stage.publication().clone();
    let expected_certificate = current_stage
        .certificate()
        .cloned()
        .ok_or("hosted MCP snapshot did not request a Gateway certificate")?;
    let staged = edge
        .stage_mcp_gateway_snapshot(current_stage.clone())
        .await?;
    assert_eq!(staged.publication, expected_publication);
    assert_eq!(staged.certificate, Some(expected_certificate.clone()));
    assert_eq!(
        edge.find_gateway_certificate(scope.node_id, expected_certificate.id)
            .await?,
        expected_certificate
    );
    let scope_after_atomic_stage = edge.gateway_scope(scope.node_id).await?;
    assert_eq!(
        scope_after_atomic_stage.last_issued_revision,
        scope_before_atomic_stage.next_revision()?
    );
    assert_eq!(
        scope_after_atomic_stage.installed_revision,
        scope_before_atomic_stage.installed_revision
    );
    assert_eq!(
        scope_after_atomic_stage.aggregate_version,
        scope_before_atomic_stage.aggregate_version + 1
    );
    assert_eq!(
        stage_artifact_counts(executor, &current_stage).await?,
        (1, 1, 1, 1)
    );
    let stored_marker = Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(
            sql_query::<(String, u32)>(
                "select desired_state_digest, mcp_route_count from mcp_gateway_snapshot_publications where gateway_command_id = ",
            )
            .bind(current_stage.publication().command_id.as_uuid()),
        )
        .await?;
    assert_eq!(
        stored_marker,
        (
            current_stage
                .candidate()
                .desired_state_digest()
                .as_str()
                .to_owned(),
            u32::try_from(current_stage.candidate().mcp().route_versions().len())?,
        )
    );

    let snapshot_repository: Arc<dyn IMcpGatewaySnapshotRepository> = Arc::new(edge.clone());
    let node_control: Arc<dyn INodeControlRepository> =
        Arc::new(PostgresNodeRepository::new(executor.clone()));
    let commands = Arc::new(FleetGatewayCommandQueue::new(node_control.clone()));
    let reconciler = McpGatewaySnapshotReconciler::new(
        snapshot_repository,
        commands,
        std::time::Duration::from_secs(60),
        100,
    )?;
    let first_dispatch = reconciler
        .run_once(expected_publication.command_issued_at + Duration::milliseconds(1))
        .await?;
    assert_eq!(first_dispatch.pending_snapshots, 1);
    assert_eq!(first_dispatch.dispatched_commands, 1);
    assert_eq!(first_dispatch.replayed_commands, 0);
    assert!(first_dispatch.failures.is_empty());
    let queued_commands = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from node_commands where id = ")
                .bind(expected_publication.command_id.as_uuid()),
        )
        .await?;
    assert_eq!(queued_commands, 1);

    let replayed_dispatch = reconciler
        .run_once(expected_publication.command_issued_at + Duration::milliseconds(2))
        .await?;
    assert_eq!(replayed_dispatch.pending_snapshots, 1);
    assert_eq!(replayed_dispatch.dispatched_commands, 1);
    assert_eq!(replayed_dispatch.replayed_commands, 1);
    assert!(replayed_dispatch.failures.is_empty());

    let certificate_issued_at =
        expected_publication.command_issued_at + Duration::milliseconds(100);
    let certificate_expires_at = certificate_issued_at + Duration::minutes(10);
    issue_gateway_certificate(
        &edge,
        &expected_certificate,
        certificate_issued_at,
        certificate_expires_at,
    )
    .await?;
    let acknowledged_at = certificate_issued_at + Duration::milliseconds(100);
    let acknowledgement = NodeGatewayAck {
        schema: NodeGatewayAck::SCHEMA.into(),
        acknowledgement_id: Uuid::now_v7(),
        command_id: expected_publication.command_id.as_uuid(),
        node_id: expected_publication.node_id.as_uuid(),
        gateway_id: expected_publication.node_id.as_uuid(),
        revision: expected_publication.revision,
        snapshot_digest: expected_publication.snapshot_digest.clone(),
        expires_at: expected_publication.snapshot_expires_at,
        state: GatewayAckState::Applied,
        ready: true,
        message: None,
        acknowledged_at,
        management_protocol: Some(GatewayManagementProtocol::advertised_v1()),
    };
    let received_at = acknowledged_at + Duration::milliseconds(1);
    let fleet_receipt = node_control
        .record_gateway_acknowledgement(acknowledgement.clone(), received_at)
        .await?;
    assert!(!fleet_receipt.replayed);
    assert!(
        edge.project_gateway_acknowledgement(&acknowledgement, received_at,)
            .await?
    );
    let replayed_fleet_receipt = node_control
        .record_gateway_acknowledgement(acknowledgement.clone(), received_at)
        .await?;
    assert!(replayed_fleet_receipt.replayed);
    assert!(
        edge.project_gateway_acknowledgement(&acknowledgement, received_at)
            .await?
    );
    let ready_certificate = edge
        .find_gateway_certificate(scope.node_id, expected_certificate.id)
        .await?;
    assert_eq!(ready_certificate.state, GatewayCertificateState::Ready);
    let installed_scope = edge.gateway_scope(scope.node_id).await?;
    assert_eq!(
        installed_scope.installed_revision,
        Some(expected_publication.revision)
    );
    assert_eq!(
        installed_scope.aggregate_version,
        scope_after_atomic_stage.aggregate_version + 1
    );
    let terminal_publication = database
        .fetch_one_as(
            sql_query::<String>("select state from gateway_publications where node_id = ")
                .bind(expected_publication.node_id.as_uuid())
                .append(" and revision = ")
                .bind(expected_publication.revision),
        )
        .await?;
    assert_eq!(
        GatewayPublicationState::parse(&terminal_publication)?,
        GatewayPublicationState::Applied
    );
    let terminal_dispatch = reconciler
        .run_once(acknowledged_at + Duration::milliseconds(2))
        .await?;
    assert_eq!(terminal_dispatch.pending_snapshots, 0);
    assert_eq!(terminal_dispatch.dispatched_commands, 0);
    assert!(terminal_dispatch.failures.is_empty());

    let desired_edge = Arc::new(edge.clone());
    let desired_inputs = Arc::new(McpRouteProjectionInputReader::new(
        desired_edge.clone(),
        desired_edge.clone(),
        Arc::new(assets.clone()),
        Arc::new(workloads.clone()),
    ));
    let desired_route_planner = McpRouteProjectionPlanner::new(
        Arc::new(FixtureRouteTargetReader::single(workload_id)),
        McpRouteTargetProjectionCompiler,
    );
    let desired_scope_planner = Arc::new(McpGatewayProjectionSetPlanner::new(
        desired_inputs,
        McpGatewayProjectionPlanner::new(desired_route_planner, desired_edge.clone()),
        McpGatewayProjectionAssembler,
    ));
    let desired_planner = Arc::new(McpGatewayNodeProjectionPlanner::new(
        desired_scope_planner,
        McpGatewayProjectionAssembler,
    ));
    let desired_reconciler = McpGatewayDesiredStateReconciler::new(
        desired_edge.clone(),
        desired_planner,
        fixture_gateway_snapshot_compiler()?,
        std::time::Duration::from_secs(60),
        Duration::minutes(5),
        Duration::hours(1),
        Duration::minutes(5),
        Duration::minutes(5),
        100,
    )?;
    let publication_count_before = database
        .fetch_one_as(sql_query::<i64>(
            "select count(*) from gateway_publications",
        ))
        .await?;
    let desired_report = desired_reconciler
        .run_once(acknowledged_at + Duration::milliseconds(3))
        .await?;
    assert_eq!(desired_report.scopes, 1);
    assert_eq!(desired_report.unchanged_snapshots, 1);
    assert_eq!(desired_report.staged_snapshots, 0);
    assert!(desired_report.failures.is_empty());
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from gateway_publications",
            ))
            .await?,
        publication_count_before
    );

    let renewal_at = certificate_expires_at - Duration::minutes(5);
    let renewal_report = desired_reconciler.run_once(renewal_at).await?;
    assert_eq!(renewal_report.staged_snapshots, 1);
    assert_eq!(renewal_report.unchanged_snapshots, 0);
    assert!(renewal_report.failures.is_empty());
    let renewal = desired_edge
        .pending_mcp_gateway_snapshots(100)
        .await?
        .into_iter()
        .find(|target| {
            target.gateway_scope_id == scope.id
                && target.publication.node_id == scope.node_id
                && target.publication.revision == expected_publication.revision + 1
        })
        .ok_or("MCP certificate renewal publication was not staged")?;
    let renewal_certificate_id = GatewayCertificateId::from_uuid(
        renewal
            .publication
            .certificate_request
            .as_ref()
            .ok_or("MCP certificate renewal omitted certificate intent")?
            .certificate_id,
    );
    assert_ne!(renewal_certificate_id, expected_certificate.id);
    assert_eq!(
        desired_edge
            .find_gateway_certificate(scope.node_id, renewal_certificate_id)
            .await?
            .state,
        GatewayCertificateState::Provisioning
    );
    let renewal_failed_at = renewal_at + Duration::milliseconds(1);
    let unavailable_renewal = desired_edge
        .mark_mcp_gateway_snapshot_unavailable(
            scope.organization_id,
            scope.id,
            scope.node_id,
            renewal.publication.revision,
            renewal.publication.command_id,
            "injected renewal delivery failure",
            renewal_failed_at,
        )
        .await?;
    assert_eq!(
        unavailable_renewal.publication.state,
        GatewayPublicationState::Unavailable
    );
    assert_eq!(
        unavailable_renewal
            .certificate
            .ok_or("unavailable MCP renewal lost certificate evidence")?
            .state,
        GatewayCertificateState::Failed
    );

    let mut rotated = credential.clone();
    let rotated_at = renewal_failed_at + Duration::milliseconds(1);
    rotated.rotate(
        "a3s_mcp_def67890abc12345",
        ROTATED_VERIFIER,
        now + Duration::days(60),
        rotated_at,
    )?;
    assert_eq!(
        edge.update_mcp_credential(rotated.clone(), 1).await?,
        rotated
    );
    let cleanup_report = desired_reconciler
        .run_once(rotated_at + Duration::milliseconds(1))
        .await?;
    assert_eq!(cleanup_report.staged_snapshots, 1);
    assert_eq!(cleanup_report.unchanged_snapshots, 0);
    assert!(cleanup_report.failures.is_empty());
    let (cleanup_route_count, cleanup_acl) = database
        .fetch_one_as(
            sql_query::<(u32, String)>(
                "select marker.mcp_route_count, publication.acl \
                 from mcp_gateway_snapshot_publications marker \
                 join gateway_publications publication \
                   on publication.node_id = marker.node_id \
                  and publication.revision = marker.gateway_revision \
                 where marker.node_id = ",
            )
            .bind(scope.node_id.as_uuid())
            .append(" order by marker.gateway_revision desc limit 1"),
        )
        .await?;
    assert_eq!(cleanup_route_count, 0);
    assert!(!cleanup_acl.contains("\nmcp {\n"));
    let mut stale = credential;
    stale.rotate(
        "a3s_mcp_ghi12345jkl67890",
        ROTATED_VERIFIER,
        now + Duration::days(60),
        rotated_at + Duration::minutes(1),
    )?;
    assert!(matches!(
        edge.update_mcp_credential(stale, 1).await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(rotated.revoke(rotated_at + Duration::minutes(2))?);
    let revoked = edge.update_mcp_credential(rotated, 2).await?;
    assert!(revoked.gateway_projection().revoked);
    assert_eq!(
        edge.list_mcp_credentials(organization_id, project_id, environment_id)
            .await?,
        vec![revoked]
    );
    node_aggregation::exercise(node_aggregation::Fixture {
        executor,
        edge: &edge,
        assets: &assets,
        workloads: &workloads,
        organization_id,
        project_id,
        environment_id,
        scope: &scope,
        policy: &revised,
        profile: &profile,
        asset: &asset,
        release: &published,
        profile_binding: &profile_binding,
        workload_id,
        workload_revision: &revision,
        observed_at: rotated_at + Duration::minutes(3),
    })
    .await?;
    Ok(())
}

struct FixtureRouteTargetReader {
    workload_id: WorkloadId,
    workload_ids_by_revision: BTreeMap<WorkloadRevisionId, WorkloadId>,
}

impl FixtureRouteTargetReader {
    fn single(workload_id: WorkloadId) -> Self {
        Self {
            workload_id,
            workload_ids_by_revision: BTreeMap::new(),
        }
    }

    fn with_revision(mut self, revision_id: WorkloadRevisionId, workload_id: WorkloadId) -> Self {
        self.workload_ids_by_revision
            .insert(revision_id, workload_id);
        self
    }

    fn workload_id(&self, revision_id: WorkloadRevisionId) -> WorkloadId {
        self.workload_ids_by_revision
            .get(&revision_id)
            .copied()
            .unwrap_or(self.workload_id)
    }
}

#[async_trait]
impl IRouteTargetReader for FixtureRouteTargetReader {
    async fn resolve_healthy_target(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _revision_id: WorkloadRevisionId,
        _port_name: &RoutePortName,
        _now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        Err(RepositoryError::Storage(
            "MCP PostgreSQL fixture requires complete member target resolution".into(),
        ))
    }

    async fn resolve_healthy_target_set(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: &RoutePortName,
        member_node_ids: &[NodeId],
        now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTargetSet, RepositoryError> {
        let workload_id = self.workload_id(revision_id);
        let targets = member_node_ids
            .iter()
            .enumerate()
            .map(|(index, node_id)| {
                let port = 49_200_u16
                    .checked_add(u16::try_from(index).map_err(|_| {
                        RepositoryError::Conflict(
                            "Gateway member index exceeds fixture range".into(),
                        )
                    })?)
                    .ok_or_else(|| {
                        RepositoryError::Conflict("Gateway fixture port range overflowed".into())
                    })?;
                Ok(ResolvedRouteTarget {
                    workload_id,
                    node_id: *node_id,
                    target: RouteTarget::new(
                        workload_id,
                        revision_id,
                        format!("workload:{workload_id}:revision:{revision_id}"),
                        1,
                        port_name.clone(),
                        UpstreamEndpoint::parse(format!("http://127.0.0.1:{port}"))
                            .map_err(RepositoryError::Conflict)?,
                        now,
                    )
                    .map_err(RepositoryError::Conflict)?,
                })
            })
            .collect::<Result<Vec<_>, RepositoryError>>()?;
        ResolvedRouteTargetSet::new(member_node_ids, targets).map_err(RepositoryError::Conflict)
    }
}

async fn plan_gateway_snapshot(
    edge: &PostgresEdgeRepository,
    assets: &PostgresAssetRepository,
    workloads: &PostgresWorkloadRepository,
    scope: &a3s_cloud_control_plane::modules::edge::GatewayScope,
    workload_id: WorkloadId,
    observed_at: DateTime<Utc>,
) -> TestResult<StageMcpGatewaySnapshot> {
    let shared_edge = Arc::new(edge.clone());
    let input_reader = Arc::new(McpRouteProjectionInputReader::new(
        shared_edge.clone(),
        shared_edge.clone(),
        Arc::new(assets.clone()),
        Arc::new(workloads.clone()),
    ));
    let route_planner = McpRouteProjectionPlanner::new(
        Arc::new(FixtureRouteTargetReader::single(workload_id)),
        McpRouteTargetProjectionCompiler,
    );
    let planner = McpGatewayProjectionSetPlanner::new(
        input_reader,
        McpGatewayProjectionPlanner::new(route_planner, shared_edge),
        McpGatewayProjectionAssembler,
    );
    let planned = planner
        .plan(PlanMcpGatewayProjectionSet {
            scope: scope.clone(),
            gateway_node_id: scope.node_id,
            observed_at,
        })
        .await?;
    let expires_at = planned
        .projection()
        .ok_or("MCP PostgreSQL fixture planned no active route")?
        .projection()
        .expires_at;
    let physical_scope = edge.gateway_scope(scope.node_id).await?;
    let mut active_routes = Vec::new();
    for route in edge.active_routes(scope.node_id).await? {
        let claim_id = route
            .domain_claim_id
            .ok_or("active ordinary Gateway route lost its DomainClaim")?;
        let domain_claim = edge
            .find_domain_claim(route.organization_id, claim_id)
            .await?;
        active_routes.push(GatewaySnapshotRouteInput {
            route,
            domain_claim,
        });
    }
    let candidate = fixture_gateway_snapshot_compiler()?.compile_mcp_reconciliation(
        CompileMcpGatewaySnapshot {
            metadata: GatewaySnapshotMetadata::new(
                scope.node_id,
                physical_scope.next_revision()?,
                physical_scope.installed_revision,
                observed_at,
                expires_at,
            ),
            physical_scope,
            certificate_id: Some(GatewayCertificateId::new()),
            active_routes,
            mcp: PlannedMcpGatewayNodeProjection::single(planned)?,
        },
    )?;
    Ok(StageMcpGatewaySnapshot::new(
        candidate,
        NodeCommandId::new(),
        Uuid::now_v7(),
        observed_at + Duration::minutes(5),
    )?)
}

fn fixture_gateway_snapshot_compiler() -> Result<GatewaySnapshotCompiler, String> {
    GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
        entrypoint_address: "0.0.0.0:8081".into(),
        management_address: "127.0.0.1:9090".into(),
        management_path_prefix: "/api/gateway".into(),
        management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
        upstream_request_timeout_ms: 30_000,
        certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
        managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
    })
}

async fn stage_artifact_counts(
    executor: &PostgresExecutor,
    stage: &StageMcpGatewaySnapshot,
) -> TestResult<(i64, i64, i64, i64)> {
    let database = Database::new(PostgresDialect, executor.clone());
    let publications = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from gateway_publications where command_id = ")
                .bind(stage.publication().command_id.as_uuid()),
        )
        .await?;
    let certificates = match stage.certificate() {
        Some(certificate) => {
            database
                .fetch_one_as(
                    sql_query::<i64>("select count(*) from gateway_certificates where id = ")
                        .bind(certificate.id.as_uuid()),
                )
                .await?
        }
        None => 0,
    };
    let events = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from outbox_events where event_id = ")
                .bind(stage.event().event_id),
        )
        .await?;
    let markers = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from mcp_gateway_snapshot_publications where gateway_command_id = ",
            )
            .bind(stage.publication().command_id.as_uuid()),
        )
        .await?;
    Ok((publications, certificates, events, markers))
}

async fn issue_gateway_certificate(
    repository: &dyn IEdgeRepository,
    certificate: &a3s_cloud_control_plane::modules::edge::GatewayCertificate,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> TestResult {
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
            expires_at,
        },
        issued_at,
    )?;
    repository
        .transition_gateway_certificate(issued, expected_version)
        .await?;
    Ok(())
}

async fn install_snapshot_outbox_failure(executor: &PostgresExecutor) -> TestResult {
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "create function reject_mcp_gateway_snapshot_outbox() returns trigger language plpgsql as $$
               begin
                 if new.event_key = 'edge.mcp-gateway.snapshot-staged' then
                   raise exception 'injected MCP Gateway snapshot outbox failure';
                 end if;
                 return new;
               end
             $$;
             create trigger reject_mcp_gateway_snapshot_outbox before insert on outbox_events
               for each row execute function reject_mcp_gateway_snapshot_outbox();",
        )
        .await?;
    Ok(())
}

async fn remove_snapshot_outbox_failure(executor: &PostgresExecutor) -> TestResult {
    executor
        .pool()
        .get()
        .await?
        .batch_execute(
            "drop trigger reject_mcp_gateway_snapshot_outbox on outbox_events;
             drop function reject_mcp_gateway_snapshot_outbox();",
        )
        .await?;
    Ok(())
}

a3s_orm::orm_table! {
    struct McpPolicyEvidence => "mcp_route_policies" {
        id: Uuid => "id",
        domain_claim_id: Uuid => "domain_claim_id",
        acl: String => "acl",
    }
}

a3s_orm::orm_table! {
    struct McpWorkloadEvidence => "workload_revisions" {
        id: Uuid => "id",
        mcp_asset_release_id: Option<Uuid> => "mcp_asset_release_id",
        mcp_profile_digest: Option<String> => "mcp_profile_digest",
    }
}

fn digest(character: char) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(format!("sha256:{}", character.to_string().repeat(64)))
}

fn idempotency(
    organization_id: OrganizationId,
    suffix: impl std::fmt::Display,
    key: &str,
    canonical_request: &[u8],
) -> Result<IdempotencyRequest, String> {
    IdempotencyRequest::new(
        format!("organizations/{organization_id}/{suffix}"),
        key,
        canonical_request,
    )
}
