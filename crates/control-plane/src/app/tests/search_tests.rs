use super::*;
use crate::modules::search::{SearchResourceKind, SearchResult};
use crate::modules::shared_kernel::domain::OrganizationId;

#[tokio::test]
async fn global_search_returns_only_tenant_authorized_projections() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let search = Arc::new(InMemorySearchRepository::new());
    let app = build_test_application_with_search(identity, projects, Arc::clone(&search))?;

    let allowed_organization = bootstrap_organization(&app, "search-allowed", "Allowed").await?;
    let denied_organization = create_organization(&app, "search-denied", "Denied").await?;
    create_api_token(
        &app,
        &allowed_organization,
        "search-reader",
        "search-reader",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;

    let allowed_node_id = Uuid::new_v4();
    let denied_node_id = Uuid::new_v4();
    search
        .register(SearchResult {
            organization_id: organization_id(&allowed_organization)?,
            project_id: None,
            environment_id: None,
            workload_id: None,
            kind: SearchResourceKind::Node,
            id: allowed_node_id,
            title: "Cloud worker".into(),
            description: "Node · ready".into(),
            state: Some("ready".into()),
            updated_at: Utc::now(),
        })
        .await
        .expect("register allowed search projection");
    search
        .register(SearchResult {
            organization_id: organization_id(&denied_organization)?,
            project_id: None,
            environment_id: None,
            workload_id: None,
            kind: SearchResourceKind::Node,
            id: denied_node_id,
            title: "Cloud hidden worker".into(),
            description: "Node · ready".into(),
            state: Some("ready".into()),
            updated_at: Utc::now(),
        })
        .await
        .expect("register denied search projection");

    let allowed = app
        .call(get_as(
            format!("/api/v1/organizations/{allowed_organization}/search?q=cloud&limit=20"),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(allowed.status(), 200);
    let allowed_body = response_json(&allowed)?;
    assert_eq!(allowed_body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(allowed_body["data"][0]["kind"], "node");
    assert_eq!(allowed_body["data"][0]["id"], allowed_node_id.to_string());
    assert_eq!(
        allowed_body["data"][0]["href"],
        format!("#/organizations/{allowed_organization}/nodes/{allowed_node_id}")
    );
    assert!(!allowed_body
        .to_string()
        .contains(&denied_node_id.to_string()));
    assert!(!allowed_body.to_string().contains("Cloud hidden worker"));

    let authorized_queries = search.query_count();
    let denied_match = app
        .call(get_as(
            format!("/api/v1/organizations/{denied_organization}/search?q=hidden&limit=20"),
            PROJECT_TOKEN,
        ))
        .await?;
    let denied_empty = app
        .call(get_as(
            format!("/api/v1/organizations/{denied_organization}/search?q=missing&limit=20"),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(denied_match.status(), 403);
    assert_eq!(denied_empty.status(), 403);
    let denied_match_body = response_json(&denied_match)?;
    let denied_empty_body = response_json(&denied_empty)?;
    for field in ["code", "statusCode", "message", "details"] {
        assert_eq!(denied_match_body[field], denied_empty_body[field]);
    }
    assert_eq!(search.query_count(), authorized_queries);
    Ok(())
}

#[tokio::test]
async fn global_search_projects_organization_scoped_plugin_registry_links() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let search = Arc::new(InMemorySearchRepository::new());
    let app = build_test_application_with_search(identity, projects, Arc::clone(&search))?;
    let organization = bootstrap_organization(&app, "search-plugin-registry", "Plugins").await?;
    let registry_id = Uuid::now_v7();

    search
        .register(SearchResult {
            organization_id: organization_id(&organization)?,
            project_id: None,
            environment_id: None,
            workload_id: None,
            kind: SearchResourceKind::PluginRegistry,
            id: registry_id,
            title: "Official plugins".into(),
            description: "Plugin registry at https://registry.example/plugins/".into(),
            state: Some("active".into()),
            updated_at: Utc::now(),
        })
        .await
        .expect("register Plugin Registry search projection");

    let response = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/search?q=official&limit=20"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(response.status(), 200);
    let body = response_json(&response)?;
    assert_eq!(body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["data"][0]["kind"], "plugin_registry");
    assert_eq!(body["data"][0]["id"], registry_id.to_string());
    assert_eq!(body["data"][0]["projectId"], Value::Null);
    assert_eq!(body["data"][0]["environmentId"], Value::Null);
    assert_eq!(body["data"][0]["workloadId"], Value::Null);
    assert_eq!(
        body["data"][0]["href"],
        format!("#/organizations/{organization}/plugin-registries/{registry_id}")
    );
    Ok(())
}

#[tokio::test]
async fn global_search_rejects_unbounded_inputs_before_querying_projections() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let search = Arc::new(InMemorySearchRepository::new());
    let app = build_test_application_with_search(identity, projects, Arc::clone(&search))?;
    let organization = bootstrap_organization(&app, "search-validation", "Validation").await?;
    let initial_queries = search.query_count();

    for path in [
        format!("/api/v1/organizations/{organization}/search?q=&limit=20"),
        format!("/api/v1/organizations/{organization}/search?q=cloud&limit=0"),
        format!("/api/v1/organizations/{organization}/search?q=cloud&limit=51"),
        format!(
            "/api/v1/organizations/{organization}/search?q={}&limit=20",
            "a".repeat(129)
        ),
    ] {
        let response = app.call(get_as(path.clone(), ADMIN_TOKEN)).await?;
        assert_eq!(
            response.status(),
            422,
            "unexpected validation response for {path}: {}",
            String::from_utf8_lossy(response.body())
        );
        assert_eq!(
            response_json(&response)?["statusCode"],
            "UNPROCESSABLE_ENTITY"
        );
    }
    assert_eq!(search.query_count(), initial_queries);
    Ok(())
}

fn organization_id(value: &str) -> Result<OrganizationId> {
    Uuid::parse_str(value)
        .map(OrganizationId::from_uuid)
        .map_err(|error| BootError::Internal(format!("invalid test organization ID: {error}")))
}
