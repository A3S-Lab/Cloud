use super::*;

const EXECUTION_TOKEN: &str =
    "a3s_f111111111111111111111111111111111111111111111111111111111111111";
const READ_ONLY_TOKEN: &str =
    "a3s_f222222222222222222222222222222222222222222222222222222222222222";

#[tokio::test]
async fn execution_api_is_scoped_idempotent_and_queryable() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "execution-bootstrap", "Execution tenant").await?;
    let project = create_project(
        &app,
        &organization,
        "execution-project",
        "Execution project",
    )
    .await?;
    let environment_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/environments");
    let environment_response = app
        .call(post_json(
            &environment_path,
            "execution-environment",
            json!({"name": "Ephemeral"}),
        ))
        .await?;
    assert_eq!(environment_response.status(), 201);
    let environment = response_id(&environment_response)?;

    create_api_token(
        &app,
        &organization,
        "execution-writer-token",
        "execution-writer",
        EXECUTION_TOKEN,
        &[ApiTokenScope::EXECUTION_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "execution-reader-token",
        "execution-reader",
        READ_ONLY_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;

    let executions_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/executions"
    );
    let body = execution_request();
    let denied = app
        .call(post_json_as(
            &executions_path,
            "execution-denied",
            body.clone(),
            READ_ONLY_TOKEN,
        ))
        .await?;
    assert_eq!(denied.status(), 403);

    let created = app
        .call(post_json_as(
            &executions_path,
            "execution-create",
            body.clone(),
            EXECUTION_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 202);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    assert_eq!(created["data"]["execution"]["status"], "queued");
    assert_eq!(
        created["data"]["execution"]["template"]["input"],
        json!({"message": "hello"})
    );
    assert_eq!(
        created["data"]["execution"]["template"]["resources"]["cpuMillis"],
        250
    );
    assert!(
        created["data"]["execution"].get("nodeId").is_none(),
        "Runtime routing identity must remain internal"
    );
    let execution_id = created["data"]["execution"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("execution response has no execution ID".into()))?;

    let replay = app
        .call(post_json_as(
            &executions_path,
            "execution-create",
            body,
            EXECUTION_TOKEN,
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    let replay = response_json(&replay)?;
    assert_eq!(replay["data"]["replayed"], true);
    assert_eq!(replay["data"]["execution"]["id"], execution_id);

    let listed = app.call(get_as(&executions_path, READ_ONLY_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    assert_eq!(listed["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["data"][0]["id"], execution_id);

    let detail_path = format!("/api/v1/organizations/{organization}/executions/{execution_id}");
    let detail = app.call(get_as(&detail_path, READ_ONLY_TOKEN)).await?;
    assert_eq!(detail.status(), 200);
    assert_eq!(response_json(&detail)?["data"]["id"], execution_id);

    let cancelled = app
        .call(delete_as(&detail_path, "execution-cancel", EXECUTION_TOKEN))
        .await?;
    assert_eq!(cancelled.status(), 202);
    let cancelled = response_json(&cancelled)?;
    assert_eq!(cancelled["data"]["execution"]["status"], "cancelling");
    assert_eq!(cancelled["data"]["replayed"], false);

    let cancel_replay = app
        .call(delete_as(&detail_path, "execution-cancel", EXECUTION_TOKEN))
        .await?;
    assert_eq!(cancel_replay.status(), 200);
    assert_eq!(response_json(&cancel_replay)?["data"]["replayed"], true);
    Ok(())
}

fn execution_request() -> Value {
    let digest = format!("sha256:{}", "a".repeat(64));
    json!({
        "artifact": {
            "uri": format!("oci://registry.example/functions/echo@{digest}"),
            "digest": digest,
            "mediaType": "application/vnd.oci.image.manifest.v1+json"
        },
        "process": {
            "command": ["/app/echo"],
            "args": [],
            "workingDirectory": null,
            "environment": {}
        },
        "input": {"message": "hello"},
        "resources": {
            "cpuMillis": 250,
            "memoryBytes": 134217728,
            "pids": 64,
            "ephemeralStorageBytes": null,
            "timeoutMs": 5000
        }
    })
}
