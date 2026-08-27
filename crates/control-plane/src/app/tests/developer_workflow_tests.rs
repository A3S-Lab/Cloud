use super::*;

const PREVIEW_POLICY_FIXTURE: &str =
    include_str!("../../../../../contracts/p0.3/pull-request-preview-policy.acl");

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

#[tokio::test]
async fn preview_management_rest_surface_is_acl_only_exact_and_bounded() -> Result<()> {
    let app = build_test_application(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
    )?;
    let organization =
        bootstrap_organization(&app, "preview-rest-organization", "Preview REST").await?;
    let project =
        create_project(&app, &organization, "preview-rest-project", "Preview REST").await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "preview-rest-environment",
            json!({"name": "Preview Source"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;
    let source_subscription_id = Uuid::now_v7();
    let preview_policy_revision_id = Uuid::now_v7();
    let base = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}"
    );
    let collection = format!("{base}/pull-request-preview-policies");
    let current = format!("{collection}/{source_subscription_id}");
    let revisions = format!("{current}/revisions");
    let preview = format!("{base}/pull-request-previews/{source_subscription_id}/pull-requests/42");

    let unauthenticated = app
        .call(BootRequest::new(HttpMethod::Get, &current))
        .await?;
    assert_eq!(unauthenticated.status(), 401);
    assert_no_store(&unauthenticated);

    let closed_request = app
        .call(post_json(
            &collection,
            "preview-rest-closed-request",
            json!({
                "sourceSubscriptionId": source_subscription_id,
                "policyAcl": PREVIEW_POLICY_FIXTURE,
                "policy": {}
            }),
        ))
        .await?;
    assert_eq!(closed_request.status(), 400);
    assert_no_store(&closed_request);

    let invalid_acl = app
        .call(post_json(
            &collection,
            "preview-rest-invalid-acl",
            json!({
                "sourceSubscriptionId": source_subscription_id,
                "policyAcl": "pull_request_preview_policy {}\n"
            }),
        ))
        .await?;
    assert_eq!(invalid_acl.status(), 422);
    assert_no_store(&invalid_acl);

    let matching_acl = PREVIEW_POLICY_FIXTURE
        .replace(
            "018f0f70-0000-7000-8000-000000000101",
            &organization.to_string(),
        )
        .replace("018f0f70-0000-7000-8000-000000000102", &project.to_string())
        .replace(
            "018f0f70-0000-7000-8000-000000000104",
            &source_subscription_id.to_string(),
        );
    let missing_source = app
        .call(post_json(
            &collection,
            "preview-rest-missing-source",
            json!({
                "sourceSubscriptionId": source_subscription_id,
                "policyAcl": matching_acl
            }),
        ))
        .await?;
    assert_eq!(missing_source.status(), 404);
    assert_no_store(&missing_source);

    let missing_current = app.call(get_as(&current, ADMIN_TOKEN)).await?;
    assert_eq!(missing_current.status(), 404);
    assert_no_store(&missing_current);

    let invalid_limit = app
        .call(get_as(format!("{revisions}?limit=0"), ADMIN_TOKEN))
        .await?;
    assert_eq!(invalid_limit.status(), 422);
    assert_no_store(&invalid_limit);

    let missing_history = app
        .call(get_as(format!("{revisions}?limit=1"), ADMIN_TOKEN))
        .await?;
    assert_eq!(missing_history.status(), 404);
    assert_no_store(&missing_history);

    let missing_revision = app
        .call(get_as(
            format!("{revisions}/{preview_policy_revision_id}"),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(missing_revision.status(), 404);
    assert_no_store(&missing_revision);

    let invalid_preview = app
        .call(get_as(
            format!(
                "{base}/pull-request-previews/{source_subscription_id}/pull-requests/{}",
                crate::modules::developer_workflows::MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER + 1
            ),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(invalid_preview.status(), 422);
    assert_no_store(&invalid_preview);

    let missing_preview = app.call(get_as(&preview, ADMIN_TOKEN)).await?;
    assert_eq!(missing_preview.status(), 404);
    assert_no_store(&missing_preview);
    Ok(())
}
