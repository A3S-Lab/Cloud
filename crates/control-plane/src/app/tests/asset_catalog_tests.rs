use super::*;

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
