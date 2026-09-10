use super::*;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use chrono::Duration;

const INFERENCE_KEY_READ_TOKEN: &str =
    "a3s_5555555555555555555555555555555555555555555555555555555555555555";
const INFERENCE_KEY_WRITE_TOKEN: &str =
    "a3s_6666666666666666666666666666666666666666666666666666666666666666";
const INFERENCE_KEY_RESTRICTED_TOKEN: &str =
    "a3s_7777777777777777777777777777777777777777777777777777777777777770";

#[tokio::test]
async fn inference_key_create_revoke_is_idempotent_cas_and_secret_safe() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "inference-key-http", "Inference key tenant").await?;
    let project = create_project(
        &app,
        &organization,
        "inference-key-project",
        "Inference Keys",
    )
    .await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "inference-key-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;

    create_api_token(
        &app,
        &organization,
        "inference-key-read-token",
        "inference-key-read",
        INFERENCE_KEY_READ_TOKEN,
        &[ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-key-write-token",
        "inference-key-write",
        INFERENCE_KEY_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE],
        None,
    )
    .await?;

    let collection_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/keys"
    );
    let create_body = json!({ "expiresAt": (Utc::now() + Duration::hours(2)).to_rfc3339() });

    assert_eq!(
        app.call(
            BootRequest::new(HttpMethod::Post, &collection_path)
                .with_header("content-type", "application/json")
                .with_header("idempotency-key", "inference-key:unauth")
                .with_body(create_body.to_string().into_bytes()),
        )
        .await?
        .status(),
        401
    );

    let missing_idempotency = app
        .call(
            BootRequest::new(HttpMethod::Post, &collection_path)
                .with_header(
                    "authorization",
                    format!("Bearer {INFERENCE_KEY_WRITE_TOKEN}"),
                )
                .with_header("content-type", "application/json")
                .with_body(create_body.to_string().into_bytes()),
        )
        .await?;
    assert_eq!(missing_idempotency.status(), 400);

    let insufficient = app
        .call(post_json_as(
            &collection_path,
            "inference-key:read-create",
            create_body.clone(),
            INFERENCE_KEY_READ_TOKEN,
        ))
        .await?;
    assert_eq!(insufficient.status(), 403);

    let create_request = || {
        post_json_as(
            &collection_path,
            "inference-key:create",
            create_body.clone(),
            INFERENCE_KEY_WRITE_TOKEN,
        )
    };
    let created = app.call(create_request()).await?;
    let replayed = app.call(create_request()).await?;
    assert_eq!(created.status(), 201);
    assert_eq!(replayed.status(), 200);
    assert_delivery_is_not_cacheable(&created);
    assert_delivery_is_not_cacheable(&replayed);

    let created_json = response_json(&created)?;
    let replayed_json = response_json(&replayed)?;
    let credential_id = created_json["data"]["credential"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("inference key response has no id".into()))?;
    let aggregate_version = created_json["data"]["credential"]["aggregateVersion"]
        .as_u64()
        .ok_or_else(|| BootError::Internal("inference key response has no aggregateVersion".into()))?;
    let bearer = created_json["data"]["bearerCredential"]
        .as_str()
        .ok_or_else(|| BootError::Internal("inference key response has no bearer".into()))?;
    assert_valid_inference_bearer(bearer, &created_json["data"]["credential"]["prefix"])?;
    assert_eq!(created_json["data"]["credential"]["generation"], json!(1));
    assert_eq!(created_json["data"]["replayed"], json!(false));
    assert_eq!(replayed_json["data"]["replayed"], json!(true));
    assert_eq!(
        replayed_json["data"]["bearerCredential"],
        created_json["data"]["bearerCredential"]
    );
    assert_metadata_hides_secret_material(&created_json["data"]["credential"]);

    let listed = app
        .call(BootRequest::new(HttpMethod::Get, &collection_path).with_header(
            "authorization",
            format!("Bearer {INFERENCE_KEY_READ_TOKEN}"),
        ))
        .await?;
    assert_eq!(listed.status(), 200);
    let listed_json = response_json(&listed)?;
    assert_eq!(listed_json["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed_json["data"][0]["id"], credential_id);
    assert_response_has_no_bearer(&listed, &[bearer]);

    let item_path = format!("/api/v1/organizations/{organization}/inference/keys/{credential_id}");
    let fetched = app
        .call(BootRequest::new(HttpMethod::Get, &item_path).with_header(
            "authorization",
            format!("Bearer {INFERENCE_KEY_READ_TOKEN}"),
        ))
        .await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(response_json(&fetched)?["data"]["generation"], json!(1));
    assert_response_has_no_bearer(&fetched, &[bearer]);

    let revoke_path = format!("{collection_path}/{credential_id}/revoke");
    let zero_cas = app
        .call(post_json_as(
            &revoke_path,
            "inference-key:revoke-zero",
            json!({ "expectedAggregateVersion": 0 }),
            INFERENCE_KEY_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(zero_cas.status(), 422);

    let revoke_body = json!({ "expectedAggregateVersion": aggregate_version });
    let revoke_request = || {
        post_json_as(
            &revoke_path,
            "inference-key:revoke",
            revoke_body.clone(),
            INFERENCE_KEY_WRITE_TOKEN,
        )
    };
    let revoked = app.call(revoke_request()).await?;
    let replayed_revoke = app.call(revoke_request()).await?;
    assert_eq!(revoked.status(), 202);
    assert_eq!(replayed_revoke.status(), 202);
    assert_no_store(&revoked);
    assert_no_store(&replayed_revoke);
    let revoked_json = response_json(&revoked)?;
    assert_eq!(revoked_json["data"]["credential"]["state"], json!("revoked"));
    assert!(revoked_json["data"]["credential"]["revokedAt"].is_string());
    assert_eq!(
        revoked_json["data"]["credential"]["aggregateVersion"],
        json!(aggregate_version + 1)
    );
    assert_eq!(replayed_revoke_json_replayed(&replayed_revoke)?, true);
    assert_response_has_no_bearer(&revoked, &[bearer]);

    let stale = app
        .call(post_json_as(
            &revoke_path,
            "inference-key:revoke-stale",
            json!({ "expectedAggregateVersion": aggregate_version }),
            INFERENCE_KEY_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(stale.status(), 409);
    assert_response_has_no_bearer(&stale, &[bearer]);

    let fetched_revoked = app
        .call(BootRequest::new(HttpMethod::Get, &item_path).with_header(
            "authorization",
            format!("Bearer {INFERENCE_KEY_READ_TOKEN}"),
        ))
        .await?;
    assert_eq!(fetched_revoked.status(), 200);
    assert_eq!(
        response_json(&fetched_revoked)?["data"]["state"],
        json!("revoked")
    );
    assert_response_has_no_bearer(&fetched_revoked, &[bearer]);
    Ok(())
}

#[tokio::test]
async fn inference_key_create_replay_after_delivery_receipt_sweep_returns_conflict_without_bearer(
) -> Result<()> {
    use crate::modules::identity::application::InferenceCredentialDeliveryReceiptSweeper;
    use crate::modules::identity::InMemoryInferenceCredentialRepository;
    use std::time::Duration as StdDuration;

    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let credentials = Arc::new(InMemoryInferenceCredentialRepository::default());
    let app = build_test_application_with_inference_credentials(
        identity,
        projects,
        Arc::clone(&credentials),
    )?;
    let organization =
        bootstrap_organization(&app, "inference-key-sweep-http", "Inference key sweep").await?;
    let project = create_project(
        &app,
        &organization,
        "inference-key-sweep-project",
        "Inference Keys Sweep",
    )
    .await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "inference-key-sweep-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;

    create_api_token(
        &app,
        &organization,
        "inference-key-sweep-write-token",
        "inference-key-sweep-write",
        INFERENCE_KEY_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE],
        None,
    )
    .await?;

    let collection_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/keys"
    );
    let create_body = json!({ "expiresAt": (Utc::now() + Duration::hours(2)).to_rfc3339() });
    let create_request = || {
        post_json_as(
            &collection_path,
            "inference-key:create-then-sweep",
            create_body.clone(),
            INFERENCE_KEY_WRITE_TOKEN,
        )
    };

    let created = app.call(create_request()).await?;
    assert_eq!(created.status(), 201);
    let created_json = response_json(&created)?;
    let bearer = created_json["data"]["bearerCredential"]
        .as_str()
        .ok_or_else(|| BootError::Internal("inference key response has no bearer".into()))?
        .to_owned();
    let delivery_expires_at = created_json["data"]["deliveryExpiresAt"]
        .as_str()
        .ok_or_else(|| BootError::Internal("inference key response has no deliveryExpiresAt".into()))?;
    let delivery_expires_at = chrono::DateTime::parse_from_rfc3339(delivery_expires_at)
        .map_err(|error| BootError::Internal(error.to_string()))?
        .with_timezone(&Utc);

    let live_replay = app.call(create_request()).await?;
    assert_eq!(live_replay.status(), 200);
    assert_eq!(
        response_json(&live_replay)?["data"]["bearerCredential"],
        bearer
    );

    let sweeper = InferenceCredentialDeliveryReceiptSweeper::new(
        credentials as Arc<dyn crate::modules::identity::IInferenceCredentialLifecycleRepository>,
        StdDuration::from_secs(60),
        100,
    )
    .map_err(|error| BootError::Internal(error))?;
    assert_eq!(
        sweeper
            .run_once(delivery_expires_at)
            .await
            .map_err(|error| BootError::Internal(error.to_string()))?,
        1
    );

    let after_sweep = app.call(create_request()).await?;
    assert_eq!(after_sweep.status(), 409);
    assert_delivery_is_not_cacheable(&after_sweep);
    let after_sweep_json = response_json(&after_sweep)?;
    let message = after_sweep_json["message"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        message.contains("no longer recoverable") || message.contains("expired"),
        "expected delivery fail-closed conflict, got {message}"
    );
    assert_response_has_no_bearer(&after_sweep, &[&bearer]);
    assert!(
        after_sweep_json["data"].get("bearerCredential").is_none(),
        "swept receipt must not reissue bearer in Conflict body"
    );
    Ok(())
}

#[tokio::test]
async fn inference_key_reads_fail_closed_for_ungranted_environment() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "inference-key-visibility", "Inference key visibility")
            .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-key-visibility-project",
        "Inference Key Visibility",
    )
    .await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "inference-key-visibility-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;
    let other_environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "inference-key-visibility-other",
            json!({"name": "Staging"}),
        ))
        .await?;
    assert_eq!(other_environment.status(), 201);
    let other_environment = response_id(&other_environment)?;

    create_api_token(
        &app,
        &organization,
        "inference-key-visibility-write",
        "inference-key-visibility-write",
        INFERENCE_KEY_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE],
        None,
    )
    .await?;

    let collection_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/keys"
    );
    let created = app
        .call(post_json_as(
            &collection_path,
            "inference-key:visibility-create",
            json!({ "expiresAt": (Utc::now() + Duration::hours(2)).to_rfc3339() }),
            INFERENCE_KEY_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created_json = response_json(&created)?;
    let credential_id = created_json["data"]["credential"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("missing credential id".into()))?
        .to_owned();
    let bearer = created_json["data"]["bearerCredential"]
        .as_str()
        .ok_or_else(|| BootError::Internal("missing bearer".into()))?
        .to_owned();

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "inference-key-visibility-membership",
            json!({"name": "Restricted key reader", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted membership has no ID".into()))?;
    let principal_id = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted principal has no ID".into()))?;
    let restricted = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "inference-key-visibility-restricted-token",
            json!({
                "name": "Restricted key reader",
                "token": INFERENCE_KEY_RESTRICTED_TOKEN,
                "scopes": [ApiTokenScope::INFERENCE_READ],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(restricted.status(), 201);
    let grant = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants"
            ),
            "inference-key-visibility-grant",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": project,
                    "environmentId": other_environment
                }
            }),
        ))
        .await?;
    assert_eq!(grant.status(), 201);

    let denied_list = app
        .call(get_as(&collection_path, INFERENCE_KEY_RESTRICTED_TOKEN))
        .await?;
    assert_eq!(denied_list.status(), 403);
    assert_response_has_no_bearer(&denied_list, &[&bearer]);

    let denied_get = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/inference/keys/{credential_id}"),
            INFERENCE_KEY_RESTRICTED_TOKEN,
        ))
        .await?;
    assert!(
        denied_get.status() == 403 || denied_get.status() == 404,
        "ungranted key get must fail closed, got {}",
        denied_get.status()
    );
    assert_response_has_no_bearer(&denied_get, &[&bearer]);
    Ok(())
}

#[tokio::test]
async fn inference_key_create_missing_environment_fails_closed_as_not_found() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "inference-key-missing-env", "Inference key missing env")
            .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-key-missing-env-project",
        "Inference Key Missing Env",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "inference-key-missing-env-write",
        "inference-key-missing-env-write",
        INFERENCE_KEY_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE],
        None,
    )
    .await?;

    let missing_environment = Uuid::now_v7();
    let path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{missing_environment}/inference/keys"
    );
    let rejected = app
        .call(post_json_as(
            &path,
            "inference-key:missing-environment",
            json!({ "expiresAt": (Utc::now() + Duration::hours(2)).to_rfc3339() }),
            INFERENCE_KEY_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(rejected.status(), 404);
    assert_response_has_no_bearer(&rejected, &[]);
    let body = response_json(&rejected)?;
    assert!(body["data"].get("bearerCredential").is_none());
    Ok(())
}

#[tokio::test]
async fn inference_key_revoke_rejects_wrong_environment_path_as_not_found() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(
        &app,
        "inference-key-revoke-scope",
        "Inference key revoke path scope",
    )
    .await?;
    let project = create_project(
        &app,
        &organization,
        "inference-key-revoke-scope-project",
        "Inference Key Revoke Scope",
    )
    .await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "inference-key-revoke-scope-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;
    let other_environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "inference-key-revoke-scope-other",
            json!({"name": "Staging"}),
        ))
        .await?;
    assert_eq!(other_environment.status(), 201);
    let other_environment = response_id(&other_environment)?;

    create_api_token(
        &app,
        &organization,
        "inference-key-revoke-scope-write",
        "inference-key-revoke-scope-write",
        INFERENCE_KEY_WRITE_TOKEN,
        &[ApiTokenScope::INFERENCE_WRITE, ApiTokenScope::INFERENCE_READ],
        None,
    )
    .await?;

    let collection_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/inference/keys"
    );
    let created = app
        .call(post_json_as(
            &collection_path,
            "inference-key:revoke-scope-create",
            json!({ "expiresAt": (Utc::now() + Duration::hours(2)).to_rfc3339() }),
            INFERENCE_KEY_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created_json = response_json(&created)?;
    let credential_id = created_json["data"]["credential"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("inference key response has no id".into()))?;
    let aggregate_version = created_json["data"]["credential"]["aggregateVersion"]
        .as_u64()
        .ok_or_else(|| BootError::Internal("inference key response has no aggregateVersion".into()))?;

    let wrong_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{other_environment}/inference/keys/{credential_id}/revoke"
    );
    let rejected = app
        .call(post_json_as(
            &wrong_path,
            "inference-key:revoke-wrong-environment",
            json!({ "expectedAggregateVersion": aggregate_version }),
            INFERENCE_KEY_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(rejected.status(), 404);

    let missing_environment = Uuid::now_v7();
    let missing_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{missing_environment}/inference/keys/{credential_id}/revoke"
    );
    let missing = app
        .call(post_json_as(
            &missing_path,
            "inference-key:revoke-missing-environment",
            json!({ "expectedAggregateVersion": aggregate_version }),
            INFERENCE_KEY_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(missing.status(), 404);

    let fetched = app
        .call(
            BootRequest::new(
                HttpMethod::Get,
                format!("/api/v1/organizations/{organization}/inference/keys/{credential_id}"),
            )
            .with_header(
                "authorization",
                format!("Bearer {INFERENCE_KEY_WRITE_TOKEN}"),
            ),
        )
        .await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(
        response_json(&fetched)?["data"]["state"],
        json!("active"),
        "path-scoped revoke failures must leave the credential active"
    );
    Ok(())
}

fn replayed_revoke_json_replayed(response: &BootResponse) -> Result<bool> {
    response_json(response)?["data"]["replayed"]
        .as_bool()
        .ok_or_else(|| BootError::Internal("revoke response missing replayed".into()))
}

fn assert_valid_inference_bearer(bearer: &str, prefix: &Value) -> Result<()> {
    let prefix = prefix
        .as_str()
        .ok_or_else(|| BootError::Internal("inference key response has no prefix".into()))?;
    assert!(prefix.starts_with("a3s_inf_"));
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
