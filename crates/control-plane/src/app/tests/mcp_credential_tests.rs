use super::*;
use chrono::Duration;

#[tokio::test]
async fn hosted_mcp_credential_lifecycle_is_idempotent_and_secret_safe() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-credential-bootstrap", "Acme").await?;
    let project = create_project(&app, &organization, "mcp-credential-project", "Cloud").await?;
    let environments_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/environments");
    let environment = app
        .call(post_json(
            &environments_path,
            "mcp-credential-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;

    create_api_token(
        &app,
        &organization,
        "mcp-credential-limited-token",
        "project-only",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;

    let collection_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/mcp-credentials"
    );
    let initial_expiry = Utc::now() + Duration::days(30);
    let create_body = json!({"expiresAt": initial_expiry});
    let forbidden = app
        .call(post_json_as(
            &collection_path,
            "mcp-credential-forbidden",
            create_body.clone(),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(forbidden.status(), 403);

    let create_request = || {
        post_json(
            &collection_path,
            "mcp-credential-create",
            create_body.clone(),
        )
    };
    let created = app.call(create_request()).await?;
    let replayed_create = app.call(create_request()).await?;
    assert_eq!(created.status(), 201);
    assert_eq!(replayed_create.status(), 200);
    assert_delivery_is_not_cacheable(&created);
    assert_delivery_is_not_cacheable(&replayed_create);
    let created_json = response_json(&created)?;
    let replayed_create_json = response_json(&replayed_create)?;
    let credential_id = created_json["data"]["credential"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP credential response has no ID".into()))?;
    let initial_bearer = created_json["data"]["bearerCredential"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP credential response has no bearer".into()))?;
    assert_valid_bearer(
        initial_bearer,
        &created_json["data"]["credential"]["prefix"],
    )?;
    assert_eq!(created_json["data"]["credential"]["generation"], 1);
    assert_eq!(created_json["data"]["credential"]["aggregateVersion"], 1);
    assert_eq!(created_json["data"]["replayed"], false);
    assert_eq!(replayed_create_json["data"]["replayed"], true);
    assert_eq!(
        replayed_create_json["data"]["bearerCredential"],
        created_json["data"]["bearerCredential"]
    );
    assert_metadata_hides_secret_material(&created_json["data"]["credential"]);

    let listed = app.call(get_as(&collection_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    let listed_json = response_json(&listed)?;
    assert_eq!(listed_json["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed_json["data"][0]["id"], credential_id);
    assert_response_has_no_bearer(&listed, &[initial_bearer]);

    let item_path = format!("/api/v1/organizations/{organization}/mcp-credentials/{credential_id}");
    let found = app.call(get_as(&item_path, ADMIN_TOKEN)).await?;
    assert_eq!(found.status(), 200);
    assert_eq!(response_json(&found)?["data"]["generation"], 1);
    assert_response_has_no_bearer(&found, &[initial_bearer]);

    let rotated_expiry = Utc::now() + Duration::days(60);
    let rotate_path = format!("{item_path}/rotate");
    let invalid_version = app
        .call(post_json(
            &rotate_path,
            "mcp-credential-invalid-version",
            json!({
                "expiresAt": rotated_expiry,
                "expectedAggregateVersion": 0
            }),
        ))
        .await?;
    assert_eq!(invalid_version.status(), 422);
    assert_response_has_no_bearer(&invalid_version, &[initial_bearer]);

    let rotate_body = json!({
        "expiresAt": rotated_expiry,
        "expectedAggregateVersion": 1
    });
    let rotate_request = || post_json(&rotate_path, "mcp-credential-rotate", rotate_body.clone());
    let rotated = app.call(rotate_request()).await?;
    let replayed_rotate = app.call(rotate_request()).await?;
    assert_eq!(rotated.status(), 201);
    assert_eq!(replayed_rotate.status(), 200);
    assert_delivery_is_not_cacheable(&rotated);
    assert_delivery_is_not_cacheable(&replayed_rotate);
    let rotated_json = response_json(&rotated)?;
    let replayed_rotate_json = response_json(&replayed_rotate)?;
    let rotated_bearer = rotated_json["data"]["bearerCredential"]
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP rotation response has no bearer".into()))?;
    assert_ne!(rotated_bearer, initial_bearer);
    assert_valid_bearer(
        rotated_bearer,
        &rotated_json["data"]["credential"]["prefix"],
    )?;
    assert_eq!(rotated_json["data"]["credential"]["id"], credential_id);
    assert_eq!(rotated_json["data"]["credential"]["generation"], 2);
    assert_eq!(rotated_json["data"]["credential"]["aggregateVersion"], 2);
    assert_eq!(replayed_rotate_json["data"]["replayed"], true);
    assert_eq!(
        replayed_rotate_json["data"]["bearerCredential"],
        rotated_json["data"]["bearerCredential"]
    );

    let revoked = app
        .call(post_json(
            format!("{item_path}/revoke"),
            "mcp-credential-revoke",
            json!({"expectedAggregateVersion": 2}),
        ))
        .await?;
    assert_eq!(revoked.status(), 200);
    let revoked_json = response_json(&revoked)?;
    assert_eq!(revoked_json["data"]["credential"]["state"], "revoked");
    assert_eq!(revoked_json["data"]["credential"]["aggregateVersion"], 3);
    assert_response_has_no_bearer(&revoked, &[initial_bearer, rotated_bearer]);

    let found_after_revoke = app.call(get_as(&item_path, ADMIN_TOKEN)).await?;
    assert_eq!(found_after_revoke.status(), 200);
    assert_eq!(
        response_json(&found_after_revoke)?["data"]["state"],
        "revoked"
    );
    assert_response_has_no_bearer(&found_after_revoke, &[initial_bearer, rotated_bearer]);

    let stale_delivery_replay = app.call(create_request()).await?;
    assert_eq!(stale_delivery_replay.status(), 409);
    assert_response_has_no_bearer(&stale_delivery_replay, &[initial_bearer, rotated_bearer]);
    Ok(())
}

fn assert_valid_bearer(bearer: &str, prefix: &Value) -> Result<()> {
    let prefix = prefix
        .as_str()
        .ok_or_else(|| BootError::Internal("MCP credential response has no prefix".into()))?;
    assert_eq!(prefix.len(), 24);
    assert_eq!(bearer.len(), 88);
    assert!(bearer.starts_with(prefix));
    assert!(bearer[prefix.len()..]
        .bytes()
        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    Ok(())
}

fn assert_metadata_hides_secret_material(metadata: &Value) {
    let rendered = metadata.to_string().to_ascii_lowercase();
    for forbidden in [
        "bearercredential",
        "verifier",
        "ciphertext",
        "encryptedvalue",
    ] {
        assert!(!rendered.contains(forbidden));
    }
}

fn assert_response_has_no_bearer(response: &BootResponse, bearers: &[&str]) {
    let rendered = String::from_utf8_lossy(response.body()).to_ascii_lowercase();
    for forbidden in [
        "bearercredential",
        "verifier",
        "ciphertext",
        "encryptedvalue",
    ] {
        assert!(!rendered.contains(forbidden));
    }
    for bearer in bearers {
        assert!(!rendered.contains(&bearer.to_ascii_lowercase()));
    }
}

fn assert_delivery_is_not_cacheable(response: &BootResponse) {
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(response.header("pragma"), Some("no-cache"));
    assert_eq!(response.header("referrer-policy"), Some("no-referrer"));
}
