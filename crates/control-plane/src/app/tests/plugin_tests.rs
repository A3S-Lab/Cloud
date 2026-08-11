use super::*;

#[tokio::test]
async fn plugin_registry_reads_are_tenant_guarded_and_catalog_posts_are_non_mutating() -> Result<()>
{
    let app = build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )?;
    let organization = bootstrap_organization(&app, "plugins-read", "Plugins").await?;
    let collection = format!("/api/v1/organizations/{organization}/plugin-registries");

    let unauthorized = app
        .call(BootRequest::new(HttpMethod::Get, collection.clone()))
        .await?;
    assert_eq!(unauthorized.status(), 401);

    let listed = app.call(get_as(collection, ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(response_json(&listed)?["data"], json!([]));

    let registry_id = Uuid::now_v7();
    let catalog_path = format!(
        "/api/v1/organizations/{organization}/plugin-registries/{registry_id}/catalog/search"
    );
    let request = BootRequest::new(HttpMethod::Post, catalog_path)
        .with_header("content-type", "application/json")
        .with_header("authorization", format!("Bearer {ADMIN_TOKEN}"))
        .with_body(
            json!({
                "host": {
                    "target": "x86_64-unknown-linux-gnu",
                    "useVersion": "0.3.0"
                },
                "search": {
                    "query": "",
                    "limit": 20
                }
            })
            .to_string()
            .into_bytes(),
        );
    let missing = app.call(request).await?;
    assert_eq!(missing.status(), 404);
    assert_eq!(response_json(&missing)?["statusCode"], "NOT_FOUND");
    Ok(())
}
