use super::*;

#[tokio::test]
async fn manual_rollback_clones_a_previous_success_into_one_idempotent_new_generation() -> Result<()>
{
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let secrets = Arc::new(InMemorySecretRepository::new());
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let app = build_test_application_with_repositories(
        identity,
        projects,
        secrets,
        Arc::clone(&workloads),
    )?;
    let organization = bootstrap_organization(&app, "rollback-bootstrap", "Acme").await?;
    let project = create_project(&app, &organization, "rollback-project", "Cloud").await?;
    let environment_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/environments");
    let environment = app
        .call(post_json(
            &environment_path,
            "rollback-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;
    let workloads_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/workloads"
    );
    let created = app
        .call(post_json(
            &workloads_path,
            "rollback-workload",
            json!({
                "name": "api",
                "template": workload_template("v1", json!([]))
            }),
        ))
        .await?;
    assert_eq!(created.status(), 202);
    let created_json = response_json(&created)?;
    let organization_id = parse_organization_id(&organization)?;
    let workload_id = created_json["data"]["workloadId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("workload response has no workload ID".into()))?;
    let source_revision_id = parse_revision_id(
        created_json["data"]["revisionId"]
            .as_str()
            .ok_or_else(|| BootError::Internal("workload response has no revision ID".into()))?,
    )?;
    let first_deployment_id = parse_deployment_id(
        created_json["data"]["deploymentId"]
            .as_str()
            .ok_or_else(|| BootError::Internal("workload response has no deployment ID".into()))?,
    )?;
    let node_id = NodeId::new();
    resolve_and_activate(
        workloads.as_ref(),
        organization_id,
        first_deployment_id,
        'a',
        node_id,
    )
    .await?;
    let source = workloads
        .find_revision(organization_id, source_revision_id)
        .await
        .map_err(repository_error)?;

    let update_path =
        format!("/api/v1/organizations/{organization}/workloads/{workload_id}/deployments");
    let failed_update = app
        .call(post_json(
            &update_path,
            "rollback-failed-update",
            json!({"template": workload_template("failed", json!([]))}),
        ))
        .await?;
    assert_eq!(failed_update.status(), 202);
    let failed_json = response_json(&failed_update)?;
    let failed_revision_id = parse_revision_id(
        failed_json["data"]["revisionId"]
            .as_str()
            .ok_or_else(|| BootError::Internal("failed update has no revision ID".into()))?,
    )?;
    let failed_deployment_id = parse_deployment_id(
        failed_json["data"]["deploymentId"]
            .as_str()
            .ok_or_else(|| BootError::Internal("failed update has no deployment ID".into()))?,
    )?;
    let failed_deployment = workloads
        .find_deployment(organization_id, failed_deployment_id)
        .await
        .map_err(repository_error)?;
    let failed_deployment = workloads
        .mark_resolving(
            failed_deployment.id,
            failed_deployment.aggregate_version,
            Utc::now().max(failed_deployment.updated_at),
        )
        .await
        .map_err(repository_error)?;
    workloads
        .fail(
            failed_deployment.id,
            failed_deployment.aggregate_version,
            "candidate health failed".into(),
            Utc::now().max(failed_deployment.updated_at),
        )
        .await
        .map_err(repository_error)?;

    let current_update = app
        .call(post_json(
            &update_path,
            "rollback-current-update",
            json!({"template": workload_template("v3", json!([]))}),
        ))
        .await?;
    assert_eq!(current_update.status(), 202);
    let current_json = response_json(&current_update)?;
    let current_revision_id = parse_revision_id(
        current_json["data"]["revisionId"]
            .as_str()
            .ok_or_else(|| BootError::Internal("current update has no revision ID".into()))?,
    )?;
    let current_deployment_id = parse_deployment_id(
        current_json["data"]["deploymentId"]
            .as_str()
            .ok_or_else(|| BootError::Internal("current update has no deployment ID".into()))?,
    )?;
    resolve_and_activate(
        workloads.as_ref(),
        organization_id,
        current_deployment_id,
        'c',
        node_id,
    )
    .await?;

    let other_workload = app
        .call(post_json(
            &workloads_path,
            "rollback-other-workload",
            json!({
                "name": "worker",
                "template": workload_template("other", json!([]))
            }),
        ))
        .await?;
    assert_eq!(other_workload.status(), 202);
    let other_revision_id = response_json(&other_workload)?["data"]["revisionId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("other workload has no revision ID".into()))?
        .to_owned();

    let rollback_path =
        format!("/api/v1/organizations/{organization}/workloads/{workload_id}/rollback");
    let current_target = app
        .call(post_json(
            &rollback_path,
            "rollback-current",
            json!({"revisionId": current_revision_id}),
        ))
        .await?;
    assert_eq!(current_target.status(), 409);
    let failed_target = app
        .call(post_json(
            &rollback_path,
            "rollback-failed",
            json!({"revisionId": failed_revision_id}),
        ))
        .await?;
    assert_eq!(failed_target.status(), 409);
    let missing_target = app
        .call(post_json(
            &rollback_path,
            "rollback-missing",
            json!({"revisionId": Uuid::now_v7()}),
        ))
        .await?;
    assert_eq!(missing_target.status(), 404);
    let cross_workload_target = app
        .call(post_json(
            &rollback_path,
            "rollback-cross-workload",
            json!({"revisionId": other_revision_id}),
        ))
        .await?;
    assert_eq!(cross_workload_target.status(), 404);

    let request = || {
        post_json(
            &rollback_path,
            "rollback-v1",
            json!({"revisionId": source_revision_id}),
        )
    };
    let accepted = app.call(request()).await?;
    assert_eq!(accepted.status(), 202);
    let accepted_json = response_json(&accepted)?;
    let rollback_deployment_id = parse_deployment_id(
        accepted_json["data"]["deploymentId"]
            .as_str()
            .ok_or_else(|| BootError::Internal("rollback has no deployment ID".into()))?,
    )?;
    workloads
        .cancel(rollback_deployment_id, 1, Utc::now())
        .await
        .map_err(repository_error)?;
    let stopped = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/workloads/{workload_id}/stop"),
            "rollback-stop",
            json!({}),
        ))
        .await?;
    assert_eq!(stopped.status(), 202);

    let replayed = app.call(request()).await?;
    assert_eq!(replayed.status(), 200);
    let replayed_json = response_json(&replayed)?;
    assert_eq!(accepted_json["data"]["generation"], 4);
    assert_eq!(
        accepted_json["data"]["rollbackSourceRevisionId"],
        source_revision_id.to_string()
    );
    assert_eq!(
        accepted_json["data"]["artifactDigest"],
        format!("sha256:{}", "a".repeat(64))
    );
    assert_eq!(
        accepted_json["data"]["templateDigest"],
        source
            .template_digest
            .clone()
            .expect("source template digest")
    );
    assert_eq!(
        accepted_json["data"]["revisionId"],
        replayed_json["data"]["revisionId"]
    );
    assert_eq!(
        accepted_json["data"]["deploymentId"],
        replayed_json["data"]["deploymentId"]
    );
    assert_eq!(replayed_json["data"]["replayed"], true);
    let changed_replay = app
        .call(post_json(
            &rollback_path,
            "rollback-v1",
            json!({"revisionId": current_revision_id}),
        ))
        .await?;
    assert_eq!(changed_replay.status(), 409);

    let rollback_revision_id = parse_revision_id(
        accepted_json["data"]["revisionId"]
            .as_str()
            .ok_or_else(|| BootError::Internal("rollback has no revision ID".into()))?,
    )?;
    let rollback = workloads
        .find_revision(organization_id, rollback_revision_id)
        .await
        .map_err(repository_error)?;
    assert_eq!(rollback.template, source.template);
    assert_eq!(rollback.template_digest, source.template_digest);
    assert_eq!(
        rollback.request.artifact.expected_digest,
        source
            .resolved_template()
            .map_err(BootError::Internal)?
            .artifact
            .digest
            .clone()
            .into()
    );
    Ok(())
}
