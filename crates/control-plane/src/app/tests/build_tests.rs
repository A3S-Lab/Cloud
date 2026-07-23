use super::*;
use crate::modules::artifacts::{BuildRun, BuildRunStatus};
use crate::modules::shared_kernel::domain::{
    BuildRunId, EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
};

const SOURCE_ONLY_TOKEN: &str =
    "a3s_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

#[tokio::test]
async fn build_run_queries_and_cancellation_expose_authoritative_state() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    let app = build_test_application_with_external_builds(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::clone(&builds),
    )?;
    let organization = bootstrap_organization(&app, "build-bootstrap", "Build tenant").await?;
    let project = create_project(&app, &organization, "build-project", "Build project").await?;
    let environment_response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "build-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment_response.status(), 201);
    let environment = response_id(&environment_response)?;
    create_api_token(
        &app,
        &organization,
        "source-only-token",
        "source-only",
        SOURCE_ONLY_TOKEN,
        &[ApiTokenScope::SOURCE_WRITE],
        None,
    )
    .await?;

    let organization_id = OrganizationId::from_uuid(
        Uuid::parse_str(&organization).map_err(|error| BootError::Internal(error.to_string()))?,
    );
    let project_id = ProjectId::from_uuid(
        Uuid::parse_str(&project).map_err(|error| BootError::Internal(error.to_string()))?,
    );
    let environment_id = EnvironmentId::from_uuid(
        Uuid::parse_str(&environment).map_err(|error| BootError::Internal(error.to_string()))?,
    );
    let source_revision_id = SourceRevisionId::new();
    let accepted_at = Utc::now();
    builds
        .add_source_revision(
            organization_id,
            project_id,
            environment_id,
            source_revision_id,
            accepted_at,
        )
        .await;
    let queued = builds
        .reserve_pending(1, accepted_at)
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?
        .pop()
        .ok_or_else(|| BootError::Internal("build was not reserved".into()))?;

    let list_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/build-runs"
    );
    let listed = app.call(get_as(&list_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    assert_eq!(listed["data"][0]["id"], queued.id.to_string());
    assert_eq!(
        listed["data"][0]["sourceRevisionId"],
        source_revision_id.to_string()
    );
    assert_eq!(
        listed["data"][0]["operationId"],
        queued.operation_id.to_string()
    );
    assert_eq!(listed["data"][0]["status"], "queued");
    assert!(listed["data"][0].get("inputArtifact").is_none());

    let newer_source_revision_id = SourceRevisionId::new();
    let newer_requested_at = accepted_at + chrono::Duration::seconds(1);
    builds
        .add_source_revision(
            organization_id,
            project_id,
            environment_id,
            newer_source_revision_id,
            newer_requested_at,
        )
        .await;
    let newer = builds
        .reserve_pending(1, newer_requested_at)
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?
        .pop()
        .ok_or_else(|| BootError::Internal("newer build was not reserved".into()))?;
    let limited = app
        .call(get_as(format!("{list_path}?limit=1"), ADMIN_TOKEN))
        .await?;
    assert_eq!(limited.status(), 200);
    let limited = response_json(&limited)?;
    assert_eq!(limited["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(limited["data"][0]["id"], newer.id.to_string());

    for invalid_limit in ["0", "201"] {
        let invalid = app
            .call(get_as(
                format!("{list_path}?limit={invalid_limit}"),
                ADMIN_TOKEN,
            ))
            .await?;
        assert_eq!(invalid.status(), 400);
    }

    let detail_path = format!(
        "/api/v1/organizations/{organization}/build-runs/{}",
        queued.id
    );
    let detail = app.call(get_as(&detail_path, ADMIN_TOKEN)).await?;
    assert_eq!(detail.status(), 200);
    assert_eq!(response_json(&detail)?["data"]["aggregateVersion"], 1);
    let evidence = app
        .call(get_as(format!("{detail_path}/evidence"), ADMIN_TOKEN))
        .await?;
    assert_eq!(evidence.status(), 404);
    assert_eq!(response_json(&evidence)?["statusCode"], "NOT_FOUND");

    let logs_path = format!("{detail_path}/logs");
    let logs = app.call(get_as(&logs_path, ADMIN_TOKEN)).await?;
    assert_eq!(logs.status(), 200);
    let logs = response_json(&logs)?;
    assert_eq!(logs["data"]["buildRunId"], queued.id.to_string());
    assert_eq!(logs["data"]["operationId"], queued.operation_id.to_string());
    assert_eq!(logs["data"]["generation"], 1);
    assert_eq!(logs["data"]["records"].as_array().map(Vec::len), Some(0));
    assert!(logs["data"]["nextCursor"].is_null());
    assert!(logs["data"].get("nodeId").is_none());
    assert!(logs["data"].get("unitId").is_none());

    let invalid_cursor = app
        .call(get_as(format!("{logs_path}?cursor=invalid"), ADMIN_TOKEN))
        .await?;
    assert_eq!(invalid_cursor.status(), 400);
    for invalid_query in ["limit=0", "limit=257"] {
        let invalid = app
            .call(get_as(format!("{logs_path}?{invalid_query}"), ADMIN_TOKEN))
            .await?;
        assert_eq!(invalid.status(), 422);
    }

    let forbidden = app
        .call(delete_as(
            &detail_path,
            "cancel-with-source-scope",
            SOURCE_ONLY_TOKEN,
        ))
        .await?;
    assert_eq!(forbidden.status(), 403);

    let missing_idempotency = app
        .call(
            BootRequest::new(HttpMethod::Delete, detail_path.clone())
                .with_header("authorization", format!("Bearer {ADMIN_TOKEN}")),
        )
        .await?;
    assert_eq!(missing_idempotency.status(), 400);

    let accepted = app
        .call(delete_as(&detail_path, "cancel-build", ADMIN_TOKEN))
        .await?;
    assert_eq!(accepted.status(), 202);
    let accepted = response_json(&accepted)?;
    assert_eq!(accepted["data"]["buildRunId"], queued.id.to_string());
    assert_eq!(
        accepted["data"]["operationId"],
        queued.operation_id.to_string()
    );
    assert_eq!(accepted["data"]["status"], "cancelling");
    assert_eq!(accepted["data"]["replayed"], false);
    assert!(accepted["data"]["cancellationRequestedAt"].is_string());

    let replayed = app
        .call(delete_as(&detail_path, "cancel-build", ADMIN_TOKEN))
        .await?;
    assert_eq!(replayed.status(), 200);
    let replayed = response_json(&replayed)?;
    assert_eq!(replayed["data"]["replayed"], true);
    assert_eq!(
        replayed["data"]["cancellationRequestedAt"],
        accepted["data"]["cancellationRequestedAt"]
    );

    let duplicate_intent = app
        .call(delete_as(&detail_path, "another-cancel", ADMIN_TOKEN))
        .await?;
    assert_eq!(duplicate_intent.status(), 409);
    let after = app.call(get_as(&detail_path, ADMIN_TOKEN)).await?;
    assert_eq!(response_json(&after)?["data"]["status"], "cancelling");
    assert_eq!(
        builds
            .find(organization_id, queued.id)
            .await
            .map_err(|error| BootError::Internal(error.to_string()))?
            .aggregate_version,
        2
    );

    let cancelling = builds
        .find(organization_id, queued.id)
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    let mut cancelled = cancelling.clone();
    cancelled
        .complete(Utc::now())
        .map_err(BootError::Internal)?;
    let cancelled = builds
        .save(cancelled, cancelling.aggregate_version)
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    assert_eq!(cancelled.status, BuildRunStatus::Cancelled);

    let retry_path = format!("{detail_path}/retry");
    let retry_forbidden = app
        .call(post_json_as(
            &retry_path,
            "retry-with-source-scope",
            json!({}),
            SOURCE_ONLY_TOKEN,
        ))
        .await?;
    assert_eq!(retry_forbidden.status(), 403);

    let retry_accepted = app
        .call(post_json(&retry_path, "retry-build", json!({})))
        .await?;
    assert_eq!(retry_accepted.status(), 202);
    let retry_accepted = response_json(&retry_accepted)?;
    let retry_id = BuildRun::id_for_attempt(source_revision_id, 2).map_err(BootError::Internal)?;
    assert_eq!(retry_accepted["data"]["buildRunId"], retry_id.to_string());
    assert_eq!(retry_accepted["data"]["operationId"], retry_id.to_string());
    assert_eq!(retry_accepted["data"]["attempt"], 2);
    assert_eq!(
        retry_accepted["data"]["retryOfBuildRunId"],
        queued.id.to_string()
    );
    assert_eq!(retry_accepted["data"]["status"], "queued");
    assert_eq!(retry_accepted["data"]["replayed"], false);

    let retry_replayed = app
        .call(post_json(&retry_path, "retry-build", json!({})))
        .await?;
    assert_eq!(retry_replayed.status(), 200);
    let retry_replayed = response_json(&retry_replayed)?;
    assert_eq!(retry_replayed["data"]["buildRunId"], retry_id.to_string());
    assert_eq!(retry_replayed["data"]["replayed"], true);

    let duplicate_retry = app
        .call(post_json(&retry_path, "retry-build-again", json!({})))
        .await?;
    assert_eq!(duplicate_retry.status(), 409);
    let retry_nonterminal = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/build-runs/{retry_id}/retry"),
            "retry-queued-build",
            json!({}),
        ))
        .await?;
    assert_eq!(retry_nonterminal.status(), 409);
    let attempts = app.call(get_as(&list_path, ADMIN_TOKEN)).await?;
    let attempts = response_json(&attempts)?;
    assert_eq!(attempts["data"].as_array().map(Vec::len), Some(3));
    assert_eq!(attempts["data"][0]["id"], newer.id.to_string());
    assert_eq!(attempts["data"][1]["id"], retry_id.to_string());
    assert_eq!(attempts["data"][1]["attempt"], 2);
    assert_eq!(attempts["data"][2]["attempt"], 1);
    Ok(())
}

#[tokio::test]
async fn build_run_detail_hides_cross_tenant_and_unknown_identities() -> Result<()> {
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    let app = build_test_application_with_external_builds(
        Arc::new(InMemoryIdentityRepository::new()),
        Arc::new(InMemoryProjectsRepository::new()),
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::clone(&builds),
    )?;
    let organization = bootstrap_organization(&app, "build-not-found", "Build tenant").await?;
    let accepted_at = Utc::now();
    builds
        .add_source_revision(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            SourceRevisionId::new(),
            accepted_at,
        )
        .await;
    let cross_tenant = builds
        .reserve_pending(1, accepted_at)
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?
        .pop()
        .ok_or_else(|| BootError::Internal("cross-tenant build was not reserved".into()))?;

    for build_run_id in [cross_tenant.id, BuildRunId::new()] {
        for suffix in ["", "/logs", "/evidence"] {
            let response = app
                .call(get_as(
                    format!(
                        "/api/v1/organizations/{organization}/build-runs/{build_run_id}{suffix}"
                    ),
                    ADMIN_TOKEN,
                ))
                .await?;
            assert_eq!(response.status(), 404);
            assert_eq!(response_json(&response)?["statusCode"], "NOT_FOUND");
        }
    }
    Ok(())
}
