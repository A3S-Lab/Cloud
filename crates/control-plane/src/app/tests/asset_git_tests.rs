use super::*;

#[tokio::test]
async fn hosted_git_routes_enforce_tenant_scope_protocol_and_uuid_admission() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let app = build_test_application(identity, Arc::new(InMemoryProjectsRepository::new()))?;
    let organization_id =
        bootstrap_organization(&app, "asset-git-bootstrap", "Asset Git tenant").await?;
    create_api_token(
        &app,
        &organization_id,
        "asset-git-read-token",
        "asset-git-read",
        PROJECT_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization_id,
        "asset-git-write-token",
        "asset-git-write",
        SOURCE_TOKEN,
        &[ApiTokenScope::ASSET_WRITE],
        None,
    )
    .await?;
    let asset_id = Uuid::now_v7();
    let advertisement = format!(
        "/api/v1/organizations/{organization_id}/assets/{asset_id}/git/info/refs?service=git-upload-pack"
    );

    let unauthenticated = app
        .call(BootRequest::new(HttpMethod::Get, &advertisement))
        .await?;
    assert_eq!(unauthenticated.status(), 401);

    let missing = app.call(get_as(&advertisement, PROJECT_TOKEN)).await?;
    assert_eq!(missing.status(), 404);

    let receive_advertisement = advertisement.replace("git-upload-pack", "git-receive-pack");
    let insufficient = app
        .call(get_as(&receive_advertisement, PROJECT_TOKEN))
        .await?;
    assert_eq!(insufficient.status(), 403);
    let writable_missing = app
        .call(get_as(&receive_advertisement, SOURCE_TOKEN))
        .await?;
    assert_eq!(writable_missing.status(), 404);

    let malformed_query = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization_id}/assets/{asset_id}/git/info/refs?service=git-upload-pack&extra=1"
            ),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(malformed_query.status(), 400);

    let wrong_media_type = app
        .call(
            BootRequest::new(
                HttpMethod::Post,
                format!(
                    "/api/v1/organizations/{organization_id}/assets/{asset_id}/git/git-upload-pack"
                ),
            )
            .with_header("authorization", format!("Bearer {PROJECT_TOKEN}"))
            .with_header("content-type", "application/json")
            .with_body(b"0000".to_vec()),
        )
        .await?;
    assert_eq!(wrong_media_type.status(), 415);

    let non_uuid = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{organization_id}/assets/not-a-uuid/git/info/refs?service=git-upload-pack"
            ),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(non_uuid.status(), 400);

    let foreign_organization = Uuid::now_v7();
    let foreign = app
        .call(get_as(
            format!(
                "/api/v1/organizations/{foreign_organization}/assets/{asset_id}/git/info/refs?service=git-upload-pack"
            ),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(foreign.status(), 403);
    Ok(())
}
