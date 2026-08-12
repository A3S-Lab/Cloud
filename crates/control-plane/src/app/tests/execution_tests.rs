use super::*;
use crate::modules::executions::{
    ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplateDefinition,
    ExecutionTemplateDefinitionSpec,
};
use std::collections::BTreeMap;

const EXECUTION_TOKEN: &str =
    "a3s_f111111111111111111111111111111111111111111111111111111111111111";
const READ_ONLY_TOKEN: &str =
    "a3s_f222222222222222222222222222222222222222222222222222222222222222";
const RESTRICTED_EXECUTION_TOKEN: &str =
    "a3s_f333333333333333333333333333333333333333333333333333333333333333";

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

#[tokio::test]
async fn execution_template_api_is_acl_native_immutable_and_replay_safe() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization =
        bootstrap_organization(&app, "execution-template-bootstrap", "Execution templates").await?;
    let project = create_project(
        &app,
        &organization,
        "execution-template-project",
        "Execution templates",
    )
    .await?;
    let other_project = create_project(
        &app,
        &organization,
        "execution-template-other-project",
        "Other templates",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "execution-template-writer-token",
        "execution-template-writer",
        EXECUTION_TOKEN,
        &[ApiTokenScope::EXECUTION_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "execution-template-reader-token",
        "execution-template-reader",
        READ_ONLY_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;

    let templates_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/execution-templates");
    let definition_acl = execution_template_acl()?;
    let body = json!({"definitionAcl": definition_acl});
    let denied = app
        .call(post_json_as(
            &templates_path,
            "execution-template-denied",
            body.clone(),
            READ_ONLY_TOKEN,
        ))
        .await?;
    assert_eq!(denied.status(), 403);

    let created = app
        .call(post_json_as(
            &templates_path,
            "execution-template-create",
            body.clone(),
            EXECUTION_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    let revision = &created["data"]["executionTemplate"];
    assert_eq!(revision["definitionAcl"], definition_acl);
    assert_eq!(revision["capability"], "execution.run");
    let template_id = required_execution_string(&revision["templateId"], "template ID")?;
    let revision_id = required_execution_string(&revision["revisionId"], "revision ID")?;
    let definition_digest = required_execution_string(
        &revision["definitionDigest"],
        "ExecutionTemplate definition digest",
    )?;

    let replay = app
        .call(post_json_as(
            &templates_path,
            "execution-template-create",
            body,
            EXECUTION_TOKEN,
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    let replay = response_json(&replay)?;
    assert_eq!(replay["data"]["replayed"], true);
    assert_eq!(
        replay["data"]["executionTemplate"]["revisionId"],
        revision_id
    );

    let conflicting_acl = definition_acl.replace("bounded", "changed");
    let conflict = app
        .call(post_json_as(
            &templates_path,
            "execution-template-create",
            json!({"definitionAcl": conflicting_acl}),
            EXECUTION_TOKEN,
        ))
        .await?;
    assert_eq!(conflict.status(), 409);

    let listed = app.call(get_as(&templates_path, READ_ONLY_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    assert_eq!(listed["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["data"][0]["templateId"], template_id);
    assert_eq!(
        app.call(get_as(
            format!("{templates_path}?limit=201"),
            READ_ONLY_TOKEN,
        ))
        .await?
        .status(),
        400
    );

    let revision_path = format!("{templates_path}/{template_id}/revisions/{revision_id}");
    let fetched = app.call(get_as(&revision_path, READ_ONLY_TOKEN)).await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(
        response_json(&fetched)?["data"]["definitionDigest"],
        definition_digest
    );
    let wrong_project_path = format!(
        "/api/v1/organizations/{organization}/projects/{other_project}/execution-templates/{template_id}/revisions/{revision_id}"
    );
    assert_eq!(
        app.call(get_as(&wrong_project_path, READ_ONLY_TOKEN))
            .await?
            .status(),
        404
    );
    Ok(())
}

#[tokio::test]
async fn restricted_execution_boundaries_resolve_environment_before_detail_cancel_and_replay(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "execution-grants", "Execution grants").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "execution-grants-membership",
            json!({"name": "Restricted execution operator", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = required_execution_string(
        &membership["data"]["id"],
        "restricted execution membership ID",
    )?;
    let principal_id = required_execution_string(
        &membership["data"]["principalId"],
        "restricted execution principal ID",
    )?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "execution-grants-token",
            json!({
                "name": "Restricted execution operator",
                "token": RESTRICTED_EXECUTION_TOKEN,
                "scopes": [ApiTokenScope::CLOUD_READ, ApiTokenScope::EXECUTION_WRITE],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project =
        create_project(&app, &organization, "execution-granted-project", "Granted").await?;
    let denied_project =
        create_project(&app, &organization, "execution-denied-project", "Denied").await?;
    let fallback_project = create_project(
        &app,
        &organization,
        "execution-fallback-project",
        "Fallback",
    )
    .await?;
    let granted_environment = create_execution_environment(
        &app,
        &organization,
        &granted_project,
        "execution-granted-environment",
        "Granted",
    )
    .await?;
    let denied_environment = create_execution_environment(
        &app,
        &organization,
        &denied_project,
        "execution-denied-environment",
        "Denied",
    )
    .await?;

    let granted_collection = format!(
        "/api/v1/organizations/{organization}/projects/{granted_project}/environments/{granted_environment}/executions"
    );
    let denied_collection = format!(
        "/api/v1/organizations/{organization}/projects/{denied_project}/environments/{denied_environment}/executions"
    );
    let granted = app
        .call(post_json_as(
            &granted_collection,
            "execution-grants-create-granted",
            execution_request(),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(granted.status(), 202);
    let granted_id = required_execution_string(
        &response_json(&granted)?["data"]["execution"]["id"],
        "granted execution ID",
    )?;
    let denied = app
        .call(post_json_as(
            &denied_collection,
            "execution-grants-create-denied",
            execution_request(),
            ADMIN_TOKEN,
        ))
        .await?;
    assert_eq!(denied.status(), 202);
    let denied_id = required_execution_string(
        &response_json(&denied)?["data"]["execution"]["id"],
        "denied execution ID",
    )?;

    let resource_grants =
        format!("/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants");
    let environment_grant = app
        .call(post_json(
            &resource_grants,
            "execution-grants-environment",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": granted_project,
                    "environmentId": granted_environment
                }
            }),
        ))
        .await?;
    assert_eq!(environment_grant.status(), 201);
    let environment_grant_id = response_id(&environment_grant)?;

    assert_eq!(
        app.call(get_as(&granted_collection, RESTRICTED_EXECUTION_TOKEN))
            .await?
            .status(),
        200
    );
    assert_eq!(
        app.call(get_as(&denied_collection, RESTRICTED_EXECUTION_TOKEN))
            .await?
            .status(),
        403
    );
    let granted_detail = format!("/api/v1/organizations/{organization}/executions/{granted_id}");
    let denied_detail = format!("/api/v1/organizations/{organization}/executions/{denied_id}");
    let missing_detail = format!(
        "/api/v1/organizations/{organization}/executions/{}",
        Uuid::now_v7()
    );
    assert_eq!(
        app.call(get_as(&granted_detail, RESTRICTED_EXECUTION_TOKEN))
            .await?
            .status(),
        200
    );
    assert_resource_not_found_equivalent(
        &app,
        get_as(&denied_detail, RESTRICTED_EXECUTION_TOKEN),
        get_as(&missing_detail, RESTRICTED_EXECUTION_TOKEN),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        delete_as(
            &denied_detail,
            "execution-grants-cancel-denied",
            RESTRICTED_EXECUTION_TOKEN,
        ),
        delete_as(
            &missing_detail,
            "execution-grants-cancel-denied",
            RESTRICTED_EXECUTION_TOKEN,
        ),
    )
    .await?;

    let cancel = || {
        delete_as(
            &granted_detail,
            "execution-grants-cancel-granted",
            RESTRICTED_EXECUTION_TOKEN,
        )
    };
    assert_eq!(app.call(cancel()).await?.status(), 202);
    let replay = app.call(cancel()).await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);

    let project_grant = app
        .call(post_json(
            &resource_grants,
            "execution-grants-project",
            json!({"scope": {"kind": "project", "projectId": granted_project}}),
        ))
        .await?;
    assert_eq!(project_grant.status(), 201);
    let project_grant_id = response_id(&project_grant)?;
    let fallback_grant = app
        .call(post_json(
            &resource_grants,
            "execution-grants-fallback",
            json!({"scope": {"kind": "project", "projectId": fallback_project}}),
        ))
        .await?;
    assert_eq!(fallback_grant.status(), 201);

    let revoked_environment = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/resource-grants/{environment_grant_id}/revocation"
            ),
            "execution-grants-revoke-environment",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked_environment.status(), 200);
    assert_eq!(
        app.call(get_as(&granted_detail, RESTRICTED_EXECUTION_TOKEN))
            .await?
            .status(),
        200,
        "a project grant must cover descendant executions"
    );

    let revoked_project = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/resource-grants/{project_grant_id}/revocation"
            ),
            "execution-grants-revoke-project",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked_project.status(), 200);
    assert_eq!(
        app.call(get_as(&granted_collection, RESTRICTED_EXECUTION_TOKEN))
            .await?
            .status(),
        403
    );
    assert_resource_not_found_equivalent(
        &app,
        get_as(&granted_detail, RESTRICTED_EXECUTION_TOKEN),
        get_as(&missing_detail, RESTRICTED_EXECUTION_TOKEN),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        cancel(),
        delete_as(
            &missing_detail,
            "execution-grants-cancel-granted",
            RESTRICTED_EXECUTION_TOKEN,
        ),
    )
    .await?;
    Ok(())
}

async fn create_execution_environment(
    app: &BootApplication,
    organization: &str,
    project: &str,
    idempotency_key: &str,
    name: &str,
) -> Result<String> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            idempotency_key,
            json!({"name": name}),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    response_id(&response)
}

fn required_execution_string(value: &Value, label: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal(format!("{label} is missing")))
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

pub(super) fn execution_template_acl() -> Result<String> {
    let digest = format!("sha256:{}", "a".repeat(64));
    ExecutionTemplateDefinition::from_spec(ExecutionTemplateDefinitionSpec {
        name: "workflow-echo-task".into(),
        description: "Runs one bounded finite task".into(),
        artifact: ExecutionArtifact {
            uri: format!("oci://registry.example/tasks/echo@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ExecutionProcess {
            command: vec!["/bin/echo".into()],
            args: vec!["invoke".into()],
            working_directory: Some("/workspace".into()),
            environment: BTreeMap::from([("MODE".into(), "workflow".into())]),
        },
        resources: ExecutionResources {
            cpu_millis: 100,
            memory_bytes: 64 * 1024 * 1024,
            pids: 32,
            ephemeral_storage_bytes: Some(1024 * 1024),
            timeout_ms: 30_000,
        },
    })
    .map(|definition| definition.canonical_acl().to_owned())
    .map_err(BootError::Internal)
}
