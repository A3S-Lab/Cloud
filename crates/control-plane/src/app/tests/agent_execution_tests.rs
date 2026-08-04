use super::*;

const AGENT_WRITER_TOKEN: &str =
    "a3s_a111111111111111111111111111111111111111111111111111111111111111";
const AGENT_READER_TOKEN: &str =
    "a3s_a222222222222222222222222222222222222222222222222222222222222222";

#[tokio::test]
async fn agent_conversation_api_is_tenant_scoped_idempotent_and_stream_ready() -> Result<()> {
    let app = build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )?;
    let organization = bootstrap_organization(&app, "agent-bootstrap", "Agent tenant").await?;
    let project = create_project(&app, &organization, "agent-project", "Agents").await?;
    let environment_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/environments");
    let environment = app
        .call(post_json(
            &environment_path,
            "agent-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;

    create_api_token(
        &app,
        &organization,
        "agent-writer-token",
        "agent-writer",
        AGENT_WRITER_TOKEN,
        &[ApiTokenScope::EXECUTION_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "agent-reader-token",
        "agent-reader",
        AGENT_READER_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;

    let conversations_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/agent-conversations"
    );
    let denied = app
        .call(post_empty_as(
            &conversations_path,
            "agent-denied",
            AGENT_READER_TOKEN,
        ))
        .await?;
    assert_eq!(denied.status(), 403);

    let created = app
        .call(post_empty_as(
            &conversations_path,
            "agent-conversation-create",
            AGENT_WRITER_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    assert_eq!(created["data"]["conversation"]["status"], "active");
    assert_eq!(created["data"]["conversation"]["lastEventSequence"], 0);
    let conversation_id = created["data"]["conversation"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("Agent conversation ID is missing".into()))?;

    let replay = app
        .call(post_empty_as(
            &conversations_path,
            "agent-conversation-create",
            AGENT_WRITER_TOKEN,
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    let replay = response_json(&replay)?;
    assert_eq!(replay["data"]["replayed"], true);
    assert_eq!(replay["data"]["conversation"]["id"], conversation_id);

    let listed = app
        .call(get_as(&conversations_path, AGENT_READER_TOKEN))
        .await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    assert_eq!(listed["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["data"][0]["id"], conversation_id);

    let conversation_path =
        format!("/api/v1/organizations/{organization}/agent-conversations/{conversation_id}");
    let detail = app
        .call(get_as(&conversation_path, AGENT_READER_TOKEN))
        .await?;
    assert_eq!(detail.status(), 200);
    assert_eq!(response_json(&detail)?["data"]["id"], conversation_id);

    let events_path = format!("{conversation_path}/events");
    let events = app.call(get_as(&events_path, AGENT_READER_TOKEN)).await?;
    assert_eq!(events.status(), 200);
    let events = response_json(&events)?;
    assert_eq!(events["data"]["headSequence"], 0);
    assert_eq!(events["data"]["records"], json!([]));
    assert_eq!(events["data"]["nextCursor"], Value::Null);

    let invalid_cursor = app
        .call(get_as(
            format!("{events_path}?cursor=invalid"),
            AGENT_READER_TOKEN,
        ))
        .await?;
    assert_eq!(invalid_cursor.status(), 400);

    let start_path = format!("{conversation_path}/executions");
    let unavailable_release = app
        .call(post_json_as(
            &start_path,
            "agent-execution-unavailable",
            json!({
                "agentAssetId": Uuid::now_v7(),
                "agentAssetReleaseId": Uuid::now_v7(),
                "input": {"prompt": "hello"}
            }),
            AGENT_WRITER_TOKEN,
        ))
        .await?;
    assert_eq!(unavailable_release.status(), 404);
    Ok(())
}

fn post_empty_as(path: &str, idempotency_key: &str, token: &str) -> BootRequest {
    BootRequest::new(HttpMethod::Post, path)
        .with_header("idempotency-key", idempotency_key)
        .with_header("authorization", format!("Bearer {token}"))
}
