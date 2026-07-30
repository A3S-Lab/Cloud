use super::*;
use chrono::Duration;

#[tokio::test]
async fn mcp_credential_rest_lifecycle_is_exact_redacted_and_non_cacheable() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "mcp-credential-bootstrap", "Acme").await?;
    let project = create_project(&app, &organization, "mcp-credential-project", "Cloud").await?;
    let environment_response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "mcp-credential-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment_response.status(), 201);
    let environment = response_id(&environment_response)?;
    let collection = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/mcp-credentials"
    );
    let issue_expires_at = Utc::now() + Duration::days(30);
    let issue_request = || {
        post_json(
            &collection,
            "mcp-credential-issue",
            json!({"expiresAt": issue_expires_at}),
        )
    };

    let issued = app.call(issue_request()).await?;
    let issue_replay = app.call(issue_request()).await?;
    assert_eq!(issued.status(), 201);
    assert_eq!(issue_replay.status(), 200);
    assert_no_store(&issued);
    assert_no_store(&issue_replay);
    assert_redacted_contract(&issued);
    assert_redacted_contract(&issue_replay);
    let issued_body = response_json(&issued)?;
    let replay_body = response_json(&issue_replay)?;
    let issued_data = &issued_body["data"];
    let issued_secret = issued_data["secret"]
        .as_str()
        .ok_or_else(|| BootError::Internal("issued MCP credential has no secret".into()))?
        .to_owned();
    let credential_id = issued_data["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("issued MCP credential has no ID".into()))?
        .to_owned();
    let prefix = issued_data["prefix"]
        .as_str()
        .ok_or_else(|| BootError::Internal("issued MCP credential has no prefix".into()))?;
    assert_eq!(issued_secret.len(), 88);
    assert!(issued_secret.starts_with(prefix));
    assert_eq!(issued_data["generation"], 1);
    assert_eq!(issued_data["aggregateVersion"], 1);
    assert_eq!(issued_data["replayed"], false);
    assert_eq!(replay_body["data"]["secret"], issued_secret);
    assert_eq!(replay_body["data"]["replayed"], true);

    let list = app.call(get_as(&collection, ADMIN_TOKEN)).await?;
    let item = format!("{collection}/{credential_id}");
    let get = app.call(get_as(&item, ADMIN_TOKEN)).await?;
    assert_eq!(list.status(), 200);
    assert_eq!(get.status(), 200);
    assert_no_store(&list);
    assert_no_store(&get);
    assert_redacted_contract(&list);
    assert_redacted_contract(&get);
    assert_eq!(
        response_json(&list)?["data"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(response_json(&get)?["data"]["id"], credential_id);
    assert!(!String::from_utf8_lossy(list.body()).contains(&issued_secret));
    assert!(!String::from_utf8_lossy(get.body()).contains(&issued_secret));

    let rotate_expires_at = Utc::now() + Duration::days(60);
    let rotate_path = format!("{item}/rotate");
    let rotate_request = || {
        post_json(
            &rotate_path,
            "mcp-credential-rotate",
            json!({"expiresAt": rotate_expires_at}),
        )
    };
    let rotated = app.call(rotate_request()).await?;
    let rotate_replay = app.call(rotate_request()).await?;
    assert_eq!(rotated.status(), 200);
    assert_eq!(rotate_replay.status(), 200);
    assert_no_store(&rotated);
    assert_no_store(&rotate_replay);
    assert_redacted_contract(&rotated);
    assert_redacted_contract(&rotate_replay);
    let rotated_body = response_json(&rotated)?;
    let rotated_secret = rotated_body["data"]["secret"]
        .as_str()
        .ok_or_else(|| BootError::Internal("rotated MCP credential has no secret".into()))?
        .to_owned();
    assert_ne!(rotated_secret, issued_secret);
    assert_eq!(rotated_body["data"]["generation"], 2);
    assert_eq!(rotated_body["data"]["aggregateVersion"], 2);
    assert_eq!(
        response_json(&rotate_replay)?["data"]["secret"],
        rotated_secret
    );
    assert_eq!(response_json(&rotate_replay)?["data"]["replayed"], true);

    let stale_issue_replay = app.call(issue_request()).await?;
    assert_eq!(stale_issue_replay.status(), 409);
    assert_no_store(&stale_issue_replay);
    assert_redacted_contract(&stale_issue_replay);
    assert!(!String::from_utf8_lossy(stale_issue_replay.body()).contains(&issued_secret));
    assert!(!String::from_utf8_lossy(stale_issue_replay.body()).contains(&rotated_secret));

    let revoked = app
        .call(delete_as(&item, "mcp-credential-revoke", ADMIN_TOKEN))
        .await?;
    let revoke_replay = app
        .call(delete_as(&item, "mcp-credential-revoke", ADMIN_TOKEN))
        .await?;
    assert_eq!(revoked.status(), 200);
    assert_eq!(revoke_replay.status(), 200);
    assert_no_store(&revoked);
    assert_no_store(&revoke_replay);
    assert_redacted_contract(&revoked);
    assert_redacted_contract(&revoke_replay);
    let revoked_body = response_json(&revoked)?;
    let replayed_revoke_body = response_json(&revoke_replay)?;
    assert!(revoked_body["data"]["revokedAt"].is_string());
    assert!(revoked_body["data"].get("secret").is_none());
    assert!(replayed_revoke_body["data"].get("secret").is_none());
    assert_eq!(replayed_revoke_body["data"]["replayed"], true);

    let stale_rotation_replay = app.call(rotate_request()).await?;
    assert_eq!(stale_rotation_replay.status(), 409);
    assert_no_store(&stale_rotation_replay);
    assert_redacted_contract(&stale_rotation_replay);

    let missing_idempotency = app
        .call(
            BootRequest::new(HttpMethod::Post, collection)
                .with_header("content-type", "application/json")
                .with_header("authorization", format!("Bearer {ADMIN_TOKEN}"))
                .with_body(
                    json!({"expiresAt": issue_expires_at})
                        .to_string()
                        .into_bytes(),
                ),
        )
        .await?;
    assert_eq!(missing_idempotency.status(), 400);
    assert_no_store(&missing_idempotency);

    let unauthenticated = app.call(BootRequest::new(HttpMethod::Get, item)).await?;
    assert_eq!(unauthenticated.status(), 401);
    assert_no_store(&unauthenticated);
    Ok(())
}

fn assert_no_store(response: &BootResponse) {
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(response.header("pragma"), Some("no-cache"));
    assert_eq!(response.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
}

fn assert_redacted_contract(response: &BootResponse) {
    let body = String::from_utf8_lossy(response.body()).to_ascii_lowercase();
    for forbidden in [
        "verifier",
        "ciphertext",
        "keyid",
        "delivery_expires",
        "encrypted_delivery",
    ] {
        assert!(
            !body.contains(forbidden),
            "MCP credential response exposed `{forbidden}`: {body}"
        );
    }
}
