use super::*;
use crate::modules::secrets::ISecretRepository;

#[tokio::test]
async fn secret_api_encrypts_versions_and_never_returns_or_events_values() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let secrets = Arc::new(InMemorySecretRepository::new());
    let app = build_test_application_with_secrets(identity, projects, Arc::clone(&secrets))?;
    let organization = bootstrap_organization(&app, "secret-bootstrap", "Acme").await?;
    let project = create_project(&app, &organization, "secret-project", "Cloud").await?;
    let environments_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/environments");
    let environment = app
        .call(post_json(
            &environments_path,
            "secret-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;
    create_api_token(
        &app,
        &organization,
        "secret-limited-token",
        "project-only",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;

    let list_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/secrets"
    );
    let first_plaintext = "postgres://user:first-secret@database";
    let forbidden = app
        .call(post_json_as(
            &list_path,
            "secret-forbidden",
            json!({"name": "Database URL", "value": first_plaintext}),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(forbidden.status(), 403);

    let create_request = || {
        post_json(
            &list_path,
            "secret-create",
            json!({"name": "Database URL", "value": first_plaintext}),
        )
    };
    let created = app.call(create_request()).await?;
    let replayed = app.call(create_request()).await?;
    assert_eq!(created.status(), 201);
    assert_eq!(replayed.status(), 200);
    let created_json = response_json(&created)?;
    let replayed_json = response_json(&replayed)?;
    assert_eq!(created_json["data"]["id"], replayed_json["data"]["id"]);
    assert_eq!(replayed_json["data"]["replayed"], true);
    assert_eq!(created_json["data"]["currentVersion"], 1);
    assert_eq!(created_json["data"]["version"]["version"], 1);
    assert_response_hides_secret_material(&created, &[first_plaintext]);

    let changed = app
        .call(post_json(
            &list_path,
            "secret-create",
            json!({"name": "Database URL", "value": "different"}),
        ))
        .await?;
    assert_eq!(changed.status(), 409);

    let secret_id = created_json["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("Secret response has no ID".into()))?;
    let versions_path =
        format!("/api/v1/organizations/{organization}/secrets/{secret_id}/versions");
    let second_plaintext = "postgres://user:rotated-secret@database";
    let rotate_request = || {
        post_json(
            &versions_path,
            "secret-rotate",
            json!({"value": second_plaintext}),
        )
    };
    let rotated = app.call(rotate_request()).await?;
    let rotate_replay = app.call(rotate_request()).await?;
    assert_eq!(rotated.status(), 201);
    assert_eq!(rotate_replay.status(), 200);
    assert_eq!(response_json(&rotated)?["data"]["currentVersion"], 2);
    assert_eq!(response_json(&rotated)?["data"]["version"]["version"], 2);
    assert_response_hides_secret_material(&rotated, &[first_plaintext, second_plaintext]);

    let workloads_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/workloads"
    );
    let workload_request = |key: &str, version: u64| {
        post_json(
            &workloads_path,
            key,
            json!({
                "name": format!("api-v{version}"),
                "template": {
                    "artifact": {
                        "uri": "oci://registry.example/cloud/api:stable",
                        "expectedDigest": null
                    },
                    "process": {},
                    "secrets": [{
                        "name": "database-url",
                        "secretId": secret_id,
                        "version": version,
                        "target": {
                            "kind": "environment",
                            "variable": "DATABASE_URL"
                        }
                    }],
                    "resources": {
                        "cpuMillis": 100,
                        "memoryBytes": 33554432,
                        "pids": 32,
                        "ephemeralStorageBytes": null
                    },
                    "ports": [{"name": "http", "containerPort": 8080}],
                    "health": {
                        "portName": "http",
                        "path": "/health",
                        "intervalMs": 1000,
                        "timeoutMs": 500,
                        "healthyThreshold": 1,
                        "unhealthyThreshold": 3,
                        "stabilizationWindowMs": 1000
                    }
                }
            }),
        )
    };
    let bound_workload = app.call(workload_request("secret-workload-v2", 2)).await?;
    assert_eq!(bound_workload.status(), 202);
    assert_response_hides_secret_material(&bound_workload, &[first_plaintext, second_plaintext]);
    let bound_workload_json = response_json(&bound_workload)?;
    let workload_id = bound_workload_json["data"]["workloadId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("workload response has no workload ID".into()))?;
    let revision_id = bound_workload_json["data"]["revisionId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("workload response has no revision ID".into()))?;
    let workload_detail = app
        .call(get_as(
            format!("/api/v1/organizations/{organization}/workloads/{workload_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(workload_detail.status(), 200);
    assert_eq!(
        response_json(&workload_detail)?["data"]["desiredRevision"]["requestedTemplate"]["secrets"],
        json!([{
            "name": "database-url",
            "secretId": secret_id,
            "version": 2,
            "target": {
                "kind": "environment",
                "variable": "DATABASE_URL"
            }
        }])
    );
    assert_response_hides_secret_material(&workload_detail, &[first_plaintext, second_plaintext]);
    let logs_path = format!(
        "/api/v1/organizations/{organization}/workloads/{workload_id}/revisions/{revision_id}/logs?limit=2&stream=stdout"
    );
    let logs = app.call(get_as(&logs_path, ADMIN_TOKEN)).await?;
    assert_eq!(logs.status(), 200);
    assert_eq!(response_json(&logs)?["data"]["records"], json!([]));
    assert_eq!(response_json(&logs)?["data"]["nodeId"], json!(null));
    let invalid_cursor = app
        .call(get_as(format!("{logs_path}&cursor=untrusted"), ADMIN_TOKEN))
        .await?;
    assert_eq!(invalid_cursor.status(), 400);
    let live_logs_path = format!(
        "/api/v1/organizations/{organization}/workloads/{workload_id}/revisions/{revision_id}/logs/stream?limit=16&stream=stdout"
    );
    let live_logs = app
        .call(
            BootRequest::new(
                HttpMethod::Get,
                format!("{live_logs_path}&cursor=untrusted"),
            )
            .with_header("accept", "text/event-stream")
            .with_header("authorization", format!("Bearer {ADMIN_TOKEN}"))
            .with_header("last-event-id", "v1:4"),
        )
        .await?;
    assert_eq!(live_logs.status(), 200);
    assert!(live_logs.is_streaming());
    assert!(live_logs.is_event_stream());
    let invalid_live_cursor = app
        .call(
            BootRequest::new(HttpMethod::Get, live_logs_path.clone())
                .with_header("accept", "text/event-stream")
                .with_header("authorization", format!("Bearer {ADMIN_TOKEN}"))
                .with_header("last-event-id", "untrusted"),
        )
        .await?;
    assert_eq!(invalid_live_cursor.status(), 400);
    let oversized_live_page = app
        .call(
            BootRequest::new(
                HttpMethod::Get,
                format!(
                    "/api/v1/organizations/{organization}/workloads/{workload_id}/revisions/{revision_id}/logs/stream?limit=17"
                ),
            )
            .with_header("accept", "text/event-stream")
            .with_header("authorization", format!("Bearer {ADMIN_TOKEN}")),
        )
        .await?;
    assert_eq!(oversized_live_page.status(), 400);
    let missing_version = app
        .call(workload_request("secret-workload-missing", 3))
        .await?;
    assert_eq!(missing_version.status(), 422);

    let revoke_path = format!("{versions_path}/1/revoke");
    let revoked = app
        .call(post_json(&revoke_path, "secret-revoke-v1", json!({})))
        .await?;
    let revoke_replay = app
        .call(post_json(&revoke_path, "secret-revoke-v1", json!({})))
        .await?;
    assert_eq!(revoked.status(), 200);
    assert_eq!(
        response_json(&revoked)?["data"]["version"]["state"],
        "revoked"
    );
    assert_eq!(response_json(&revoke_replay)?["data"]["replayed"], true);
    let revoked_binding = app
        .call(workload_request("secret-workload-revoked", 1))
        .await?;
    assert_eq!(revoked_binding.status(), 422);

    let listed = app.call(get_as(&list_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(response_json(&listed)?["data"][0]["currentVersion"], 2);
    let details_path = format!("/api/v1/organizations/{organization}/secrets/{secret_id}");
    let details = app.call(get_as(&details_path, ADMIN_TOKEN)).await?;
    let details_json = response_json(&details)?;
    assert_eq!(
        details_json["data"]["versions"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(details_json["data"]["versions"][0]["state"], "revoked");
    assert_eq!(details_json["data"]["versions"][1]["state"], "active");
    assert_response_hides_secret_material(&details, &[first_plaintext, second_plaintext]);

    let events = secrets.outbox_events().await;
    assert_eq!(events.len(), 3);
    let events_json =
        serde_json::to_string(&events).map_err(|error| BootError::Internal(error.to_string()))?;
    assert!(!events_json.contains(first_plaintext));
    assert!(!events_json.contains(second_plaintext));
    assert!(!events_json.contains("ciphertext"));
    assert!(!events_json.contains("key_id"));
    assert_eq!(secrets.idempotency_references().await.len(), 3);
    let first_version = secrets
        .find_version(
            crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                Uuid::parse_str(&organization)
                    .map_err(|error| BootError::Internal(error.to_string()))?,
            ),
            crate::modules::shared_kernel::domain::SecretId::from_uuid(
                Uuid::parse_str(secret_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?,
            ),
            1,
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    assert!(!first_version
        .encrypted_value
        .ciphertext()
        .contains(first_plaintext));
    Ok(())
}

fn assert_response_hides_secret_material(response: &BootResponse, plaintexts: &[&str]) {
    let body = String::from_utf8_lossy(response.body());
    for plaintext in plaintexts {
        assert!(!body.contains(plaintext));
    }
    assert!(!body.contains("ciphertext"));
    assert!(!body.contains("keyId"));
    assert!(!body.contains("encryptedValue"));
}
