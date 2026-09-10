use super::*;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use chrono::Duration;

const INFERENCE_KEY_READ_TOKEN: &str =
    "a3s_5555555555555555555555555555555555555555555555555555555555555555";
const INFERENCE_KEY_WRITE_TOKEN: &str =
    "a3s_6666666666666666666666666666666666666666666666666666666666666666";

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
