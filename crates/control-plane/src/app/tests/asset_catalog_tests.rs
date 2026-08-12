use super::*;
use crate::modules::assets::{
    Asset, AssetKind, AssetRelease, AssetReleaseVersion, McpServiceProfile, McpServiceProfileSpec,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, GitCommitSha, ResourceName, Sha256Digest,
};

const RESTRICTED_ASSET_TOKEN: &str =
    "a3s_7777777777777777777777777777777777777777777777777777777777777777";

#[tokio::test]
async fn asset_catalog_routes_enforce_read_write_and_tenant_boundaries() -> Result<()> {
    let app = build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )?;
    let organization_id =
        bootstrap_organization(&app, "asset-catalog-bootstrap", "Asset catalog tenant").await?;
    create_api_token(
        &app,
        &organization_id,
        "asset-catalog-read-token",
        "asset-catalog-read",
        PROJECT_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization_id,
        "asset-catalog-write-token",
        "asset-catalog-write",
        SOURCE_TOKEN,
        &[ApiTokenScope::ASSET_WRITE],
        None,
    )
    .await?;
    let assets_path = format!("/api/v1/organizations/{organization_id}/assets");

    assert_eq!(
        app.call(BootRequest::new(HttpMethod::Get, &assets_path))
            .await?
            .status(),
        401
    );
    let listed = app.call(get_as(&assets_path, PROJECT_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(response_json(&listed)?["data"], json!([]));

    let insufficient = app
        .call(
            BootRequest::new(HttpMethod::Post, &assets_path)
                .with_header("authorization", format!("Bearer {PROJECT_TOKEN}"))
                .with_header("content-type", "application/json")
                .with_body(br#"{"name":"agent","kind":"agent"}"#.to_vec()),
        )
        .await?;
    assert_eq!(insufficient.status(), 403);

    let missing_idempotency = app
        .call(
            BootRequest::new(HttpMethod::Post, &assets_path)
                .with_header("authorization", format!("Bearer {SOURCE_TOKEN}"))
                .with_header("content-type", "application/json")
                .with_body(br#"{"name":"agent","kind":"agent"}"#.to_vec()),
        )
        .await?;
    assert_eq!(missing_idempotency.status(), 400);

    let malformed = app
        .call(
            BootRequest::new(HttpMethod::Post, &assets_path)
                .with_header("authorization", format!("Bearer {SOURCE_TOKEN}"))
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "asset:create")
                .with_body(
                    br#"{"name":"agent","kind":"agent","manifestDigest":"caller-controlled"}"#
                        .to_vec(),
                ),
        )
        .await?;
    assert_eq!(malformed.status(), 400);

    let foreign_path = format!("/api/v1/organizations/{}/assets", Uuid::now_v7());
    assert_eq!(
        app.call(get_as(foreign_path, PROJECT_TOKEN))
            .await?
            .status(),
        403
    );
    Ok(())
}

#[tokio::test]
async fn restricted_asset_boundaries_fail_closed_across_catalog_git_and_mcp_profile() -> Result<()>
{
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let assets = Arc::new(UnavailableAssetStore::default());
    let app = build_test_application_with_asset_store(identity, projects, Arc::clone(&assets))?;
    let organization = bootstrap_organization(&app, "asset-grants", "Asset grants").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "asset-grants-membership",
            json!({"name": "Restricted Asset reader", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Asset membership has no ID".into()))?
        .to_owned();
    let principal_id = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Asset principal has no ID".into()))?
        .to_owned();
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "asset-grants-token",
            json!({
                "name": "Restricted Asset reader",
                "token": RESTRICTED_ASSET_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::ASSET_WRITE],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    // Asset has no project identity. This unrelated grant only permits the deferred guard to
    // admit the request; the authoritative Asset resolver must still fail closed.
    let project = create_project(
        &app,
        &organization,
        "asset-grants-project",
        "Unrelated project",
    )
    .await?;
    let grant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants"
            ),
            "asset-grants-create",
            json!({"scope": {"kind": "project", "projectId": project}}),
        ))
        .await?;
    assert_eq!(grant.status(), 201);

    let organization_id =
        OrganizationId::from_uuid(Uuid::parse_str(&organization).map_err(|error| {
            BootError::Internal(format!("invalid Asset organization ID: {error}"))
        })?);
    let asset = Asset::create(
        AssetId::new(),
        organization_id,
        ResourceName::parse("Restricted MCP Asset").expect("Asset name"),
        AssetKind::Mcp,
        Utc::now(),
    )
    .expect("Asset fixture");
    let release = AssetRelease::draft(
        &asset,
        AssetReleaseId::new(),
        AssetReleaseVersion::parse("1.0.0").expect("Asset release version"),
        GitCommitSha::parse("a".repeat(40)).expect("Asset release commit"),
        Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))
            .expect("Asset release manifest digest"),
        Utc::now(),
    )
    .expect("Asset release fixture");
    assets.seed_asset(asset.clone());
    assets.seed_release(release.clone());

    let asset_root = format!("/api/v1/organizations/{organization}/assets/{}", asset.id);
    let missing_root = format!(
        "/api/v1/organizations/{organization}/assets/{}",
        AssetId::new()
    );
    let release_root = format!("{asset_root}/releases/{}", release.id);
    let missing_release_root = format!("{missing_root}/releases/{}", AssetReleaseId::new());

    assert_eq!(
        app.call(get_as(&asset_root, ADMIN_TOKEN)).await?.status(),
        200
    );
    assert_eq!(
        app.call(get_as(&release_root, ADMIN_TOKEN)).await?.status(),
        200
    );
    assert_eq!(
        app.call(get_as(
            format!("/api/v1/organizations/{organization}/assets"),
            RESTRICTED_ASSET_TOKEN,
        ))
        .await?
        .status(),
        403
    );
    assert_eq!(
        app.call(post_json_as(
            format!("/api/v1/organizations/{organization}/assets"),
            "asset-create-denied",
            json!({"name": "Denied Asset", "kind": "agent"}),
            RESTRICTED_ASSET_TOKEN,
        ))
        .await?
        .status(),
        403
    );

    for (denied_path, missing_path) in [
        (asset_root.clone(), missing_root.clone()),
        (
            format!("{asset_root}/releases"),
            format!("{missing_root}/releases"),
        ),
        (release_root.clone(), missing_release_root.clone()),
        (
            format!("{asset_root}/release-selection"),
            format!("{missing_root}/release-selection"),
        ),
        (
            format!("{release_root}/mcp-service-profile"),
            format!("{missing_release_root}/mcp-service-profile"),
        ),
    ] {
        assert_resource_not_found_equivalent(
            &app,
            get_as(denied_path, RESTRICTED_ASSET_TOKEN),
            get_as(missing_path, RESTRICTED_ASSET_TOKEN),
        )
        .await?;
    }

    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!("{asset_root}/archive"),
            "asset-archive-denied",
            json!({}),
            RESTRICTED_ASSET_TOKEN,
        ),
        post_json_as(
            format!("{missing_root}/archive"),
            "asset-archive-missing",
            json!({}),
            RESTRICTED_ASSET_TOKEN,
        ),
    )
    .await?;
    let release_request = json!({"version": "2.0.0", "commitSha": "c".repeat(40)});
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!("{asset_root}/releases"),
            "asset-release-denied",
            release_request.clone(),
            RESTRICTED_ASSET_TOKEN,
        ),
        post_json_as(
            format!("{missing_root}/releases"),
            "asset-release-missing",
            release_request,
            RESTRICTED_ASSET_TOKEN,
        ),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!("{release_root}/yank"),
            "asset-yank-denied",
            json!({}),
            RESTRICTED_ASSET_TOKEN,
        ),
        post_json_as(
            format!("{missing_release_root}/yank"),
            "asset-yank-missing",
            json!({}),
            RESTRICTED_ASSET_TOKEN,
        ),
    )
    .await?;

    let profile = McpServiceProfile::from_spec(McpServiceProfileSpec {
        protocol_versions: vec![a3s_cloud_contracts::MCP_PROTOCOL_VERSION.into()],
        endpoint_path: "/mcp".into(),
        runtime_port: "mcp".into(),
        health_path: "/health".into(),
        request_sse: true,
        subscriptions: true,
        server_discover: true,
        expected_capabilities: vec!["tools".into(), "subscriptions".into()],
        max_request_bytes: 1_048_576,
        max_response_bytes: 8_388_608,
        max_stream_seconds: 3_600,
    })
    .expect("MCP Service profile fixture");
    assert_resource_not_found_equivalent(
        &app,
        post_acl_as(
            format!("{release_root}/mcp-service-profile"),
            "asset-profile-denied",
            profile.canonical_acl(),
            RESTRICTED_ASSET_TOKEN,
        ),
        post_acl_as(
            format!("{missing_release_root}/mcp-service-profile"),
            "asset-profile-missing",
            profile.canonical_acl(),
            RESTRICTED_ASSET_TOKEN,
        ),
    )
    .await?;

    let advertisement = format!("{asset_root}/git/info/refs?service=git-upload-pack");
    let missing_advertisement = format!("{missing_root}/git/info/refs?service=git-upload-pack");
    assert_resource_not_found_equivalent(
        &app,
        get_as(advertisement, RESTRICTED_ASSET_TOKEN),
        get_as(missing_advertisement, RESTRICTED_ASSET_TOKEN),
    )
    .await?;
    for (service, media_type) in [
        ("git-upload-pack", "application/x-git-upload-pack-request"),
        ("git-receive-pack", "application/x-git-receive-pack-request"),
    ] {
        assert_resource_not_found_equivalent(
            &app,
            git_rpc_as(
                format!("{asset_root}/git/{service}"),
                media_type,
                RESTRICTED_ASSET_TOKEN,
            ),
            git_rpc_as(
                format!("{missing_root}/git/{service}"),
                media_type,
                RESTRICTED_ASSET_TOKEN,
            ),
        )
        .await?;
    }
    Ok(())
}

fn git_rpc_as(path: impl Into<String>, media_type: &str, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path.into())
        .with_header("content-type", media_type)
        .with_header("authorization", format!("Bearer {token}"))
        .with_body(b"0000".to_vec())
}
