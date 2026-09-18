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

    let listed = app.call(get_as(collection.clone(), ADMIN_TOKEN)).await?;
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

#[tokio::test]
async fn plugin_registry_enrollment_is_idempotent_tenant_write_and_lists() -> Result<()> {
    use crate::modules::plugins::test_support::VALID_BOOTSTRAP_ROOT;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    let app = build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )?;
    let organization = bootstrap_organization(&app, "plugins-enroll", "Plugins").await?;
    let collection = format!("/api/v1/organizations/{organization}/plugin-registries");
    let bootstrap_root_base64 = STANDARD.encode(VALID_BOOTSTRAP_ROOT);
    let body = json!({
        "name": "Official",
        "endpoint": "https://registry.example.test/a3s",
        "bootstrapRootBase64": bootstrap_root_base64,
    });

    let unauthorized = app
        .call(
            BootRequest::new(HttpMethod::Post, collection.clone())
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "enroll-official")
                .with_body(body.to_string().into_bytes()),
        )
        .await?;
    assert_eq!(unauthorized.status(), 401);

    let created = app
        .call(post_json(
            collection.clone(),
            "enroll-official",
            body.clone(),
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created_json = response_json(&created)?;
    assert_eq!(created_json["data"]["replayed"], json!(false));
    assert_eq!(created_json["data"]["registry"]["name"], json!("Official"));
    assert_eq!(
        created_json["data"]["registry"]["endpoint"],
        json!("https://registry.example.test/a3s/")
    );
    let registry_id = created_json["data"]["registry"]["id"]
        .as_str()
        .expect("registry id")
        .to_owned();

    let replayed = app
        .call(post_json(collection.clone(), "enroll-official", body))
        .await?;
    assert_eq!(replayed.status(), 200);
    let replayed_json = response_json(&replayed)?;
    assert_eq!(replayed_json["data"]["replayed"], json!(true));
    assert_eq!(replayed_json["data"]["registry"]["id"], json!(registry_id));

    let listed = app.call(get_as(collection.clone(), ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    let listed_json = response_json(&listed)?;
    assert_eq!(listed_json["data"].as_array().expect("list").len(), 1);
    assert_eq!(listed_json["data"][0]["id"], json!(registry_id));

    let fetched = app
        .call(get_as(format!("{collection}/{registry_id}"), ADMIN_TOKEN))
        .await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(response_json(&fetched)?["data"]["id"], json!(registry_id));
    Ok(())
}
