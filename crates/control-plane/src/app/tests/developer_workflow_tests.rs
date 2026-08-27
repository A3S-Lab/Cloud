use super::*;

#[tokio::test]
async fn build_plan_rest_surface_uses_one_authenticated_environment_boundary() -> Result<()> {
    let app = build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )?;
    let organization =
        bootstrap_organization(&app, "build-plan-rest-organization", "Build Plans").await?;
    let project = create_project(
        &app,
        &organization,
        "build-plan-rest-project",
        "Build Plans",
    )
    .await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "build-plan-rest-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;
    let source_revision_id = Uuid::now_v7();
    let build_plan_id = Uuid::now_v7();
    let base = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}"
    );
    let detection_path = format!("{base}/build-plan-detections");
    let collection_path = format!("{base}/build-plans");

    let unauthenticated = app
        .call(BootRequest::new(
            HttpMethod::Get,
            format!("{collection_path}?sourceRevisionId={source_revision_id}"),
        ))
        .await?;
    assert_eq!(unauthenticated.status(), 401);
    assert_no_store(&unauthenticated);

    let missing_source_filter = app.call(get_as(&collection_path, ADMIN_TOKEN)).await?;
    assert_eq!(missing_source_filter.status(), 400);
    assert_no_store(&missing_source_filter);

    let invalid_limit = app
        .call(get_as(
            format!("{collection_path}?sourceRevisionId={source_revision_id}&limit=0"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(invalid_limit.status(), 400);
    assert_no_store(&invalid_limit);

    let listed = app
        .call(get_as(
            format!("{collection_path}?sourceRevisionId={source_revision_id}&limit=1"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(response_json(&listed)?["data"], json!([]));
    assert_no_store(&listed);

    let missing = app
        .call(get_as(
            format!("{collection_path}/{build_plan_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(missing.status(), 404);
    assert_no_store(&missing);

    let detection = app
        .call(
            BootRequest::new(HttpMethod::Post, detection_path)
                .with_header("content-type", "application/json")
                .with_header("authorization", format!("Bearer {ADMIN_TOKEN}"))
                .with_body(
                    json!({"sourceRevisionId": source_revision_id})
                        .to_string()
                        .into_bytes(),
                ),
        )
        .await?;
    assert_eq!(detection.status(), 404);
    assert_no_store(&detection);

    let invalid_acceptance = app
        .call(post_json(
            collection_path,
            "build-plan-rest-invalid-acceptance",
            json!({
                "sourceRevisionId": source_revision_id,
                "proposalAcl": "build_plan {}\n"
            }),
        ))
        .await?;
    assert_eq!(invalid_acceptance.status(), 422);
    assert_no_store(&invalid_acceptance);
    Ok(())
}
