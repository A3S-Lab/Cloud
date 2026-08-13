use super::*;

#[tokio::test]
async fn project_attribution_keeps_immutable_history_across_rest_and_mcp() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects.clone())?;
    let organization =
        bootstrap_organization(&app, "project-attribution-bootstrap", "Acme").await?;
    let project = create_project(
        &app,
        &organization,
        "project-attribution-project",
        "Platform",
    )
    .await?;
    let root = format!("/api/v1/organizations/{organization}/projects/{project}");

    let missing = app
        .call(get_as(format!("{root}/attribution-profile"), ADMIN_TOKEN))
        .await?;
    assert_eq!(missing.status(), 404);

    let first_request = || {
        post_json(
            format!("{root}/attribution-profiles"),
            "project-attribution-first",
            json!({
                "businessOwnerReference": "  finance/platform  ",
                "costAttributionCode": "  CC-1042  ",
                "labels": {
                    "region": "global",
                    "service.tier": "  critical  "
                }
            }),
        )
        .with_header("x-a3s-expected-version", "1")
    };
    let first = app.call(first_request()).await?;
    assert_eq!(first.status(), 201);
    let first_body = response_json(&first)?;
    assert_eq!(first_body["data"]["project"]["aggregateVersion"], 2);
    assert_eq!(first_body["data"]["replayed"], false);
    assert_eq!(
        first_body["data"]["attributionProfile"]["businessOwnerReference"],
        "finance/platform"
    );
    assert_eq!(
        first_body["data"]["attributionProfile"]["costAttributionCode"],
        "CC-1042"
    );
    assert_eq!(
        first_body["data"]["attributionProfile"]["labels"]["service.tier"],
        "critical"
    );
    assert!(first_body["data"]["attributionProfile"]["previousProfileId"].is_null());
    let first_id = first_body["data"]["attributionProfile"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("first attribution profile has no ID".into()))?
        .to_owned();
    assert_eq!(
        first_body["data"]["project"]["currentAttributionProfileId"],
        first_id
    );

    let replay = app.call(first_request()).await?;
    assert_eq!(replay.status(), 200);
    let replay = response_json(&replay)?;
    assert_eq!(replay["data"]["replayed"], true);
    assert_eq!(replay["data"]["attributionProfile"]["id"], first_id);

    let changed_replay = app
        .call(
            post_json(
                format!("{root}/attribution-profiles"),
                "project-attribution-first",
                json!({"businessOwnerReference": "different-owner", "labels": {}}),
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(changed_replay.status(), 409);

    let stale = app
        .call(
            post_json(
                format!("{root}/attribution-profiles"),
                "project-attribution-stale",
                json!({"businessOwnerReference": "operations/platform", "labels": {}}),
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(stale.status(), 409);

    let second = app
        .call(mcp_tool_call_as(
            1,
            "a3s_cloud_project_attribution_update",
            json!({
                "projectId": project,
                "expectedVersion": 2,
                "businessOwnerReference": "engineering/platform",
                "costAttributionCode": null,
                "labels": {"service.tier": "critical", "team": "platform"},
                "idempotencyKey": "project-attribution-second"
            }),
            ADMIN_TOKEN,
        ))
        .await?;
    let second_body = response_json(&second)?;
    assert_eq!(second_body["result"]["isError"], false);
    assert_eq!(second_body["result"]["structuredContent"]["code"], 201);
    let second_data = &second_body["result"]["structuredContent"]["data"];
    assert_eq!(second_data["project"]["aggregateVersion"], 3);
    assert_eq!(
        second_data["attributionProfile"]["previousProfileId"],
        first_id
    );
    let second_id = second_data["attributionProfile"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("second attribution profile has no ID".into()))?
        .to_owned();

    let current = app
        .call(get_as(format!("{root}/attribution-profile"), ADMIN_TOKEN))
        .await?;
    assert_eq!(current.status(), 200);
    let current = response_json(&current)?;
    assert_eq!(current["data"]["id"], second_id);
    assert_eq!(
        current["data"]["businessOwnerReference"],
        "engineering/platform"
    );

    let historical = app
        .call(get_as(
            format!("{root}/attribution-profiles/{first_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(historical.status(), 200);
    let historical = response_json(&historical)?;
    assert_eq!(historical["data"]["id"], first_id);
    assert_eq!(
        historical["data"]["businessOwnerReference"],
        "finance/platform"
    );
    assert!(historical["data"]["previousProfileId"].is_null());

    let mcp_historical = app
        .call(mcp_tool_call_as(
            2,
            "a3s_cloud_project_attribution_get",
            json!({"projectId": project, "attributionProfileId": first_id}),
            ADMIN_TOKEN,
        ))
        .await?;
    let mcp_historical = response_json(&mcp_historical)?;
    assert_eq!(mcp_historical["result"]["isError"], false);
    assert_eq!(
        mcp_historical["result"]["structuredContent"]["data"]["id"],
        first_id
    );

    let invalid = app
        .call(
            post_json(
                format!("{root}/attribution-profiles"),
                "project-attribution-invalid",
                json!({
                    "businessOwnerReference": "engineering/platform",
                    "labels": {"Invalid.Key": "value"}
                }),
            )
            .with_header("x-a3s-expected-version", "3"),
        )
        .await?;
    assert_eq!(invalid.status(), 422);

    assert_eq!(
        projects
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "project.attribution-profile.updated")
            .count(),
        2
    );
    Ok(())
}
