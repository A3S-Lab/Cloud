use a3s_cloud_contracts::{McpGrantProjection, McpLimitsProjection, MCP_PROTOCOL_VERSION};
use a3s_cloud_control_plane::modules::artifacts::domain::OCI_IMAGE_MANIFEST_MEDIA_TYPE;
use a3s_cloud_control_plane::modules::assets::{
    Asset, AssetCreated, AssetKind, AssetRelease, AssetReleaseArtifact, AssetReleaseDrafted,
    AssetReleasePublished, AssetReleaseVersion, CreateAssetReleaseWrite, CreateAssetWrite,
    IAssetRepository, IMcpServiceProfileRepository, McpServiceProfile, McpServiceProfileBinding,
    McpServiceProfileSpec, PostgresAssetRepository, TransitionAssetReleaseWrite,
};
use a3s_cloud_control_plane::modules::edge::{
    IEdgeRepository, IMcpRoutePolicyRepository, McpRoutePolicy, McpRoutePolicySpec,
    PostgresEdgeRepository, RouteHostname,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, EnvironmentId, GitCommitSha, IdempotencyRequest, OrganizationId,
    ProjectId, RepositoryError, ResourceName, RouteId, Sha256Digest, WorkloadId,
};
use a3s_orm::{select_from, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Utc};
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub async fn exercise(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    other_organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
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
    assets
        .bind_mcp_service_profile(McpServiceProfileBinding {
            organization_id,
            asset_id: asset.id,
            asset_release_id: published.id,
            profile: profile.clone(),
            created_at: published.updated_at + Duration::milliseconds(1),
        })
        .await?;

    let created_at = published.updated_at + Duration::milliseconds(2);
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
                credential_id: Uuid::now_v7(),
                credential_generation: 1,
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
    Ok(())
}

a3s_orm::orm_table! {
    struct McpPolicyEvidence => "mcp_route_policies" {
        id: Uuid => "id",
        acl: String => "acl",
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
