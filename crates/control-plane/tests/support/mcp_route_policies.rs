use a3s_cloud_contracts::{McpGrantProjection, McpLimitsProjection, MCP_PROTOCOL_VERSION};
use a3s_cloud_control_plane::modules::artifacts::domain::OCI_IMAGE_MANIFEST_MEDIA_TYPE;
use a3s_cloud_control_plane::modules::assets::{
    Asset, AssetCreated, AssetKind, AssetRelease, AssetReleaseArtifact, AssetReleaseDrafted,
    AssetReleasePublished, AssetReleaseVersion, CreateAssetReleaseWrite, CreateAssetWrite,
    IAssetRepository, IMcpServiceProfileRepository, McpServiceProfile, McpServiceProfileBinding,
    McpServiceProfileSpec, PostgresAssetRepository, TransitionAssetReleaseWrite,
};
use a3s_cloud_control_plane::modules::edge::{
    IEdgeRepository, IMcpCredentialRepository, IMcpRoutePolicyRepository, McpCredential,
    McpRoutePolicy, McpRoutePolicySpec, PostgresEdgeRepository, RouteHostname,
};
use a3s_cloud_control_plane::modules::operations::{
    OperationRequest, OperationSubject, WorkflowIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, DeploymentId, EnvironmentId, GitCommitSha, IdempotencyRequest,
    McpCredentialId, OperationId, OrganizationId, ProjectId, RepositoryError, ResourceName,
    RouteId, Sha256Digest, WorkloadId, WorkloadRevisionId,
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
use a3s_orm::{select_from, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const ROTATED_VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

pub async fn exercise(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    other_organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> TestResult {
    let edge = PostgresEdgeRepository::new(executor.clone());
    let scope = edge
        .list_gateway_scopes(organization_id, project_id, environment_id)
        .await?
        .into_iter()
        .next()
        .ok_or("MCP route policy fixture has no Gateway scope")?;
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
    assert_eq!(
        edge.create_mcp_credential(credential.clone()).await?,
        credential
    );
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
    let mut published = release.clone();
    published.publish(
        &asset,
        AssetReleaseArtifact::oci_service(digest('c')?, OCI_IMAGE_MANIFEST_MEDIA_TYPE, 4_096)?,
        release.updated_at + Duration::milliseconds(1),
    )?;
    assets
        .transition_release(TransitionAssetReleaseWrite {
            release: published.clone(),
            expected_aggregate_version: release.aggregate_version,
            event: AssetReleasePublished::envelope(&published, Uuid::now_v7())?,
            idempotency: idempotency(
                organization_id,
                format!("assets/{}/releases", asset.id),
                "postgres-mcp-policy-publication",
                b"mcp-policy-publication",
            )?,
        })
        .await?;
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
                    digest('c')?
                ),
                digest: digest('c')?.to_string(),
                media_type: OCI_IMAGE_MANIFEST_MEDIA_TYPE.into(),
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

    let workload_id = workload.id;
    let created_at = workload_created_at + Duration::milliseconds(1);
    let policy = McpRoutePolicy::create(
        McpRoutePolicySpec {
            route_id: RouteId::new(),
            organization_id,
            project_id,
            environment_id,
            gateway_scope_id: scope.id,
            workload_id,
            asset_id: asset.id,
            asset_release_id: published.id,
            profile_digest: profile.digest().clone(),
            hostname: RouteHostname::parse("mcp-policy.integration.example")?,
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

    let mut revised = policy.clone();
    let mut revised_spec = revised.spec().clone();
    revised_spec.max_request_bytes /= 2;
    revised_spec.expires_at += Duration::minutes(1);
    revised.revise(revised_spec, &profile, created_at + Duration::minutes(1))?;
    assert_eq!(
        edge.update_mcp_route_policy(revised.clone(), 1).await?,
        revised
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

    let stored_acl = Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(
            select_from::<McpPolicyEvidence>()
                .select(McpPolicyEvidence::acl())
                .filter(McpPolicyEvidence::id().eq(revised.spec().route_id.as_uuid())),
        )
        .await?;
    assert!(!stored_acl.contains("verifier_hash"));
    assert!(!stored_acl.contains("targets "));
    assert!(!stored_acl.contains("endpoint ="));

    let mut rotated = credential.clone();
    rotated.rotate(
        "a3s_mcp_def67890abc12345",
        ROTATED_VERIFIER,
        now + Duration::days(60),
        created_at + Duration::minutes(2),
    )?;
    assert_eq!(
        edge.update_mcp_credential(rotated.clone(), 1).await?,
        rotated
    );
    let mut stale = credential;
    stale.rotate(
        "a3s_mcp_ghi12345jkl67890",
        ROTATED_VERIFIER,
        now + Duration::days(60),
        created_at + Duration::minutes(3),
    )?;
    assert!(matches!(
        edge.update_mcp_credential(stale, 1).await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(rotated.revoke(created_at + Duration::minutes(4))?);
    let revoked = edge.update_mcp_credential(rotated, 2).await?;
    assert!(revoked.gateway_projection().revoked);
    assert_eq!(
        edge.list_mcp_credentials(organization_id, project_id, environment_id)
            .await?,
        vec![revoked]
    );
    Ok(())
}

a3s_orm::orm_table! {
    struct McpPolicyEvidence => "mcp_route_policies" {
        id: Uuid => "id",
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
