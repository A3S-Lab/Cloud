use super::*;

const RESTRICTED_FORM_TOKEN: &str =
    "a3s_6666666666666666666666666666666666666666666666666666666666666666";

#[tokio::test]
async fn form_draft_and_release_rest_lifecycle_is_versioned_and_replay_safe() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization_id = bootstrap_organization(&app, "form-bootstrap", "Acme").await?;
    let project_id = create_project(&app, &organization_id, "form-project", "Forms").await?;
    let collection = format!("/api/v1/organizations/{organization_id}/projects/{project_id}/forms");
    let create_body = form_draft("Approval", "Manager approval", false);

    let created = app
        .call(post_json(
            &collection,
            "form-create-approval",
            create_body.clone(),
        ))
        .await?;
    let replayed = app
        .call(post_json(
            &collection,
            "form-create-approval",
            create_body.clone(),
        ))
        .await?;
    assert_eq!(created.status(), 201);
    assert_eq!(replayed.status(), 200);
    let created = response_json(&created)?;
    let replayed = response_json(&replayed)?;
    assert_eq!(created["data"]["form"]["aggregateVersion"], 1);
    assert_eq!(replayed["data"]["replayed"], true);
    assert_eq!(
        created["data"]["form"]["id"],
        replayed["data"]["form"]["id"]
    );
    let form_id = required_form_string(&created["data"]["form"]["id"], "Form ID")?;
    let root = format!("/api/v1/organizations/{organization_id}/forms/{form_id}");

    let changed_replay = app
        .call(post_json(
            &collection,
            "form-create-approval",
            form_draft("Changed", "Different intent", false),
        ))
        .await?;
    assert_eq!(changed_replay.status(), 409);

    let listed = app.call(get_as(&collection, ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    let listed = response_json(&listed)?;
    assert_eq!(listed["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["data"][0]["id"], form_id);

    let fetched = app.call(get_as(&root, ADMIN_TOKEN)).await?;
    assert_eq!(fetched.status(), 200);
    assert_eq!(response_json(&fetched)?["data"]["name"], "Approval");

    let revised_body = form_draft(
        "Approval request",
        "Manager approval with a required reason",
        true,
    );
    let revision_path = format!("{root}/draft-revisions");
    let revise_request = || {
        post_json(&revision_path, "form-revise-approval", revised_body.clone())
            .with_header("x-a3s-expected-version", "1")
    };
    let revised = app.call(revise_request()).await?;
    assert_eq!(revised.status(), 201);
    let revised = response_json(&revised)?;
    assert_eq!(revised["data"]["form"]["aggregateVersion"], 2);
    assert_eq!(
        revised["data"]["form"]["document"]["schema"]["required"][1],
        "reason"
    );

    let stale = app
        .call(
            post_json(
                &revision_path,
                "form-revise-stale",
                form_draft("Stale", "Must not commit", false),
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(stale.status(), 409);

    let releases = format!("{root}/releases");
    let publish_request = || {
        post_json(&releases, "form-publish-approval", json!({}))
            .with_header("x-a3s-expected-version", "2")
    };
    let published = app.call(publish_request()).await?;
    assert_eq!(published.status(), 201);
    let published = response_json(&published)?;
    assert_eq!(published["data"]["form"]["aggregateVersion"], 3);
    assert_eq!(published["data"]["release"]["revision"], 1);
    assert_eq!(published["data"]["release"]["sourceDraftVersion"], 2);
    assert_eq!(
        published["data"]["release"]["compilerRevision"],
        "a3s-form-core@0.1.0"
    );
    let release_id = required_form_string(&published["data"]["release"]["id"], "Form release ID")?;

    let publish_replay = app.call(publish_request()).await?;
    assert_eq!(publish_replay.status(), 200);
    let publish_replay = response_json(&publish_replay)?;
    assert_eq!(publish_replay["data"]["replayed"], true);
    assert_eq!(publish_replay["data"]["release"]["id"], release_id);

    let historical_revision_replay = app.call(revise_request()).await?;
    assert_eq!(historical_revision_replay.status(), 200);
    let historical_revision_replay = response_json(&historical_revision_replay)?;
    assert_eq!(historical_revision_replay["data"]["replayed"], true);
    assert_eq!(
        historical_revision_replay["data"]["form"]["aggregateVersion"],
        2
    );

    let listed_releases = app.call(get_as(&releases, ADMIN_TOKEN)).await?;
    assert_eq!(listed_releases.status(), 200);
    let listed_releases = response_json(&listed_releases)?;
    assert_eq!(listed_releases["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed_releases["data"][0]["id"], release_id);

    let fetched_release = app
        .call(get_as(format!("{releases}/{release_id}"), ADMIN_TOKEN))
        .await?;
    assert_eq!(fetched_release.status(), 200);
    assert_eq!(response_json(&fetched_release)?["data"]["id"], release_id);

    let current = app.call(get_as(&root, ADMIN_TOKEN)).await?;
    let current = response_json(&current)?;
    assert_eq!(current["data"]["aggregateVersion"], 3);
    assert_eq!(current["data"]["latestRelease"]["id"], release_id);
    Ok(())
}

#[tokio::test]
async fn form_publication_rejects_invalid_documents_without_mutating_the_draft() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization_id = bootstrap_organization(&app, "invalid-form-bootstrap", "Acme").await?;
    let project_id =
        create_project(&app, &organization_id, "invalid-form-project", "Forms").await?;
    let collection = format!("/api/v1/organizations/{organization_id}/projects/{project_id}/forms");
    let created = app
        .call(post_json(
            &collection,
            "invalid-form-create",
            json!({
                "name": "Invalid",
                "description": "Rejected by the owner compiler",
                "document": {}
            }),
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let form_id = required_form_string(&response_json(&created)?["data"]["form"]["id"], "Form ID")?;
    let root = format!("/api/v1/organizations/{organization_id}/forms/{form_id}");
    let releases = format!("{root}/releases");

    let rejected = app
        .call(
            post_json(&releases, "invalid-form-publish", json!({}))
                .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(rejected.status(), 422);

    let draft = app.call(get_as(&root, ADMIN_TOKEN)).await?;
    let draft = response_json(&draft)?;
    assert_eq!(draft["data"]["aggregateVersion"], 1);
    assert!(draft["data"]["latestRelease"].is_null());
    let listed_releases = app.call(get_as(&releases, ADMIN_TOKEN)).await?;
    assert_eq!(response_json(&listed_releases)?["data"], json!([]));
    Ok(())
}

#[tokio::test]
async fn form_routes_enforce_write_scope_and_organization_tenancy() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let acme = bootstrap_organization(&app, "form-authority-bootstrap", "Acme").await?;
    let beta = create_organization(&app, "form-authority-beta", "Beta").await?;
    let acme_project = create_project(&app, &acme, "form-authority-acme", "Acme Forms").await?;
    let beta_project =
        create_project(&app, &beta, "form-authority-beta-project", "Beta Forms").await?;
    create_api_token(
        &app,
        &acme,
        "form-authority-token",
        "form-author",
        FORM_TOKEN,
        &[ApiTokenScope::CLOUD_READ, ApiTokenScope::FORM_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &acme,
        "form-authority-read-token",
        "form-reader",
        PROJECT_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;
    let body = form_draft("Approval", "Tenant-scoped Form", false);

    let allowed = app
        .call(post_json_as(
            format!("/api/v1/organizations/{acme}/projects/{acme_project}/forms"),
            "form-authority-allowed",
            body.clone(),
            FORM_TOKEN,
        ))
        .await?;
    assert_eq!(allowed.status(), 201);

    let insufficient_scope = app
        .call(post_json_as(
            format!("/api/v1/organizations/{acme}/projects/{acme_project}/forms"),
            "form-authority-scope-denied",
            body.clone(),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(insufficient_scope.status(), 403);

    let cross_tenant_write = app
        .call(post_json_as(
            format!("/api/v1/organizations/{beta}/projects/{beta_project}/forms"),
            "form-authority-tenant-denied",
            body,
            FORM_TOKEN,
        ))
        .await?;
    assert_eq!(cross_tenant_write.status(), 403);

    let cross_tenant_read = app
        .call(get_as(
            format!("/api/v1/organizations/{beta}/projects/{beta_project}/forms"),
            FORM_TOKEN,
        ))
        .await?;
    assert_eq!(cross_tenant_read.status(), 403);
    Ok(())
}

#[tokio::test]
async fn restricted_form_boundaries_resolve_project_before_reads_mutations_and_replay() -> Result<()>
{
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "form-grants", "Form grants").await?;

    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "form-grants-membership",
            json!({"name": "Restricted Form author", "role": "restricted"}),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let membership = response_json(&membership)?;
    let membership_id = membership["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Form membership has no ID".into()))?;
    let principal_id = membership["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("restricted Form principal has no ID".into()))?;
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "form-grants-token",
            json!({
                "name": "Restricted Form author",
                "token": RESTRICTED_FORM_TOKEN,
                "scopes": [ApiTokenScope::FORM_WRITE],
                "principalId": principal_id,
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);

    let granted_project =
        create_project(&app, &organization, "form-granted-project", "Granted").await?;
    let environment_only_project = create_project(
        &app,
        &organization,
        "form-environment-project",
        "Environment only",
    )
    .await?;
    let environment = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{environment_only_project}/environments"
            ),
            "form-environment",
            json!({"name": "Environment only"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment_id = response_id(&environment)?;

    let granted_collection =
        format!("/api/v1/organizations/{organization}/projects/{granted_project}/forms");
    let environment_collection =
        format!("/api/v1/organizations/{organization}/projects/{environment_only_project}/forms");
    let granted_form = create_form_fixture(
        &app,
        &granted_collection,
        "form-grants-create-granted",
        "Granted Form",
    )
    .await?;
    let environment_form = create_form_fixture(
        &app,
        &environment_collection,
        "form-grants-create-environment",
        "Environment Form",
    )
    .await?;
    let environment_root = format!("/api/v1/organizations/{organization}/forms/{environment_form}");
    let environment_release = app
        .call(
            post_json(
                format!("{environment_root}/releases"),
                "form-grants-publish-environment",
                json!({}),
            )
            .with_header("x-a3s-expected-version", "1"),
        )
        .await?;
    assert_eq!(environment_release.status(), 201);
    let environment_release_id = required_form_string(
        &response_json(&environment_release)?["data"]["release"]["id"],
        "environment Form release ID",
    )?;

    let resource_grants =
        format!("/api/v1/organizations/{organization}/memberships/{membership_id}/resource-grants");
    let project_grant = app
        .call(post_json(
            &resource_grants,
            "form-grants-create-project",
            json!({"scope": {"kind": "project", "projectId": granted_project}}),
        ))
        .await?;
    assert_eq!(project_grant.status(), 201);
    let project_grant_id = response_json(&project_grant)?["data"]["id"]
        .as_str()
        .ok_or_else(|| BootError::Internal("Form Resource Grant has no ID".into()))?
        .to_owned();
    let environment_grant = app
        .call(post_json(
            &resource_grants,
            "form-grants-create-environment",
            json!({
                "scope": {
                    "kind": "environment",
                    "projectId": environment_only_project,
                    "environmentId": environment_id
                }
            }),
        ))
        .await?;
    assert_eq!(environment_grant.status(), 201);

    let granted_root = format!("/api/v1/organizations/{organization}/forms/{granted_form}");
    let missing_form = Uuid::now_v7();
    let missing_root = format!("/api/v1/organizations/{organization}/forms/{missing_form}");
    assert_eq!(
        app.call(get_as(&granted_collection, RESTRICTED_FORM_TOKEN))
            .await?
            .status(),
        200
    );
    assert_eq!(
        app.call(get_as(&environment_collection, RESTRICTED_FORM_TOKEN))
            .await?
            .status(),
        403
    );
    assert_eq!(
        app.call(get_as(&granted_root, RESTRICTED_FORM_TOKEN))
            .await?
            .status(),
        200
    );
    assert_resource_not_found_equivalent(
        &app,
        get_as(&environment_root, RESTRICTED_FORM_TOKEN),
        get_as(&missing_root, RESTRICTED_FORM_TOKEN),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        get_as(
            format!("{environment_root}/releases"),
            RESTRICTED_FORM_TOKEN,
        ),
        get_as(format!("{missing_root}/releases"), RESTRICTED_FORM_TOKEN),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        get_as(
            format!("{environment_root}/releases/{environment_release_id}"),
            RESTRICTED_FORM_TOKEN,
        ),
        get_as(
            format!("{missing_root}/releases/{}", Uuid::now_v7()),
            RESTRICTED_FORM_TOKEN,
        ),
    )
    .await?;

    let denied_revision = form_draft("Denied revision", "Must not commit", true);
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!("{environment_root}/draft-revisions"),
            "form-revise-denied",
            denied_revision.clone(),
            RESTRICTED_FORM_TOKEN,
        )
        .with_header("x-a3s-expected-version", "2"),
        post_json_as(
            format!("{missing_root}/draft-revisions"),
            "form-revise-missing",
            denied_revision,
            RESTRICTED_FORM_TOKEN,
        )
        .with_header("x-a3s-expected-version", "2"),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        post_json_as(
            format!("{environment_root}/releases"),
            "form-publish-denied",
            json!({}),
            RESTRICTED_FORM_TOKEN,
        )
        .with_header("x-a3s-expected-version", "2"),
        post_json_as(
            format!("{missing_root}/releases"),
            "form-publish-missing",
            json!({}),
            RESTRICTED_FORM_TOKEN,
        )
        .with_header("x-a3s-expected-version", "2"),
    )
    .await?;

    let mcp_allowed = app
        .call(mcp_tool_call_as(
            1,
            "a3s_cloud_forms_get",
            json!({"formId": granted_form}),
            RESTRICTED_FORM_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&mcp_allowed)?["result"]["isError"], false);
    let mcp_denied = app
        .call(mcp_tool_call_as(
            2,
            "a3s_cloud_forms_get",
            json!({"formId": environment_form}),
            RESTRICTED_FORM_TOKEN,
        ))
        .await?;
    let mcp_missing = app
        .call(mcp_tool_call_as(
            3,
            "a3s_cloud_forms_get",
            json!({"formId": missing_form}),
            RESTRICTED_FORM_TOKEN,
        ))
        .await?;
    assert_mcp_not_found_equivalent(&mcp_denied, &mcp_missing)?;

    let revised_body = form_draft("Granted revision", "Authorized change", true);
    let revise_request = || {
        post_json_as(
            format!("{granted_root}/draft-revisions"),
            "form-revise-granted",
            revised_body.clone(),
            RESTRICTED_FORM_TOKEN,
        )
        .with_header("x-a3s-expected-version", "1")
    };
    assert_eq!(app.call(revise_request()).await?.status(), 201);
    let revision_replay = app.call(revise_request()).await?;
    assert_eq!(revision_replay.status(), 200);
    assert_eq!(response_json(&revision_replay)?["data"]["replayed"], true);

    let publish_request = || {
        post_json_as(
            format!("{granted_root}/releases"),
            "form-publish-granted",
            json!({}),
            RESTRICTED_FORM_TOKEN,
        )
        .with_header("x-a3s-expected-version", "2")
    };
    let published = app.call(publish_request()).await?;
    assert_eq!(published.status(), 201);
    let granted_release = required_form_string(
        &response_json(&published)?["data"]["release"]["id"],
        "granted Form release ID",
    )?;
    let publish_replay = app.call(publish_request()).await?;
    assert_eq!(publish_replay.status(), 200);
    assert_eq!(response_json(&publish_replay)?["data"]["replayed"], true);
    assert_eq!(
        app.call(get_as(
            format!("{granted_root}/releases/{granted_release}"),
            RESTRICTED_FORM_TOKEN,
        ))
        .await?
        .status(),
        200
    );
    let mcp_release = app
        .call(mcp_tool_call_as(
            4,
            "a3s_cloud_form_releases_get",
            json!({"formId": granted_form, "releaseId": granted_release}),
            RESTRICTED_FORM_TOKEN,
        ))
        .await?;
    assert_eq!(response_json(&mcp_release)?["result"]["isError"], false);

    let fallback = app
        .call(post_json(
            &resource_grants,
            "form-grants-fallback",
            json!({
                "scope": {"kind": "project", "projectId": environment_only_project}
            }),
        ))
        .await?;
    assert_eq!(fallback.status(), 201);
    assert_eq!(
        app.call(get_as(&environment_root, RESTRICTED_FORM_TOKEN))
            .await?
            .status(),
        200
    );
    let revoked = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/resource-grants/{project_grant_id}/revocation"
            ),
            "form-grants-revoke",
            json!({"expectedVersion": 1}),
        ))
        .await?;
    assert_eq!(revoked.status(), 200);

    assert_resource_not_found_equivalent(
        &app,
        revise_request(),
        post_json_as(
            format!("{missing_root}/draft-revisions"),
            "form-revise-missing-after-revoke",
            revised_body.clone(),
            RESTRICTED_FORM_TOKEN,
        )
        .with_header("x-a3s-expected-version", "1"),
    )
    .await?;
    assert_resource_not_found_equivalent(
        &app,
        publish_request(),
        post_json_as(
            format!("{missing_root}/releases"),
            "form-publish-missing-after-revoke",
            json!({}),
            RESTRICTED_FORM_TOKEN,
        )
        .with_header("x-a3s-expected-version", "2"),
    )
    .await?;
    let revoked_mcp = app
        .call(mcp_tool_call_as(
            5,
            "a3s_cloud_forms_get",
            json!({"formId": granted_form}),
            RESTRICTED_FORM_TOKEN,
        ))
        .await?;
    let missing_mcp = app
        .call(mcp_tool_call_as(
            6,
            "a3s_cloud_forms_get",
            json!({"formId": missing_form}),
            RESTRICTED_FORM_TOKEN,
        ))
        .await?;
    assert_mcp_not_found_equivalent(&revoked_mcp, &missing_mcp)?;
    Ok(())
}

pub(super) fn form_draft(title: &str, description: &str, require_reason: bool) -> Value {
    json!({
        "name": title,
        "description": description,
        "document": {
            "kind": "a3s.form",
            "apiVersion": "a3s.dev/form/v1alpha1",
            "revision": 1,
            "metadata": { "title": title },
            "schema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "approved": { "type": "boolean" },
                    "reason": { "type": "string" }
                },
                "required": if require_reason {
                    vec!["approved", "reason"]
                } else {
                    vec!["approved"]
                },
                "additionalProperties": false
            },
            "ui": {
                "root": "root",
                "nodes": [
                    { "id": "root", "kind": "root", "children": ["approved", "reason"] },
                    {
                        "id": "approved",
                        "kind": "field",
                        "schemaPath": "/properties/approved",
                        "widget": "switch"
                    },
                    {
                        "id": "reason",
                        "kind": "field",
                        "schemaPath": "/properties/reason",
                        "widget": "textarea"
                    }
                ]
            },
            "rules": [],
            "dataSources": [],
            "actions": []
        }
    })
}

fn required_form_string(value: &Value, label: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal(format!("{label} is missing")))
}

async fn create_form_fixture(
    app: &BootApplication,
    collection: &str,
    idempotency_key: &str,
    title: &str,
) -> Result<String> {
    let response = app
        .call(post_json(
            collection,
            idempotency_key,
            form_draft(title, "Project-owned Form", false),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    required_form_string(&response_json(&response)?["data"]["form"]["id"], "Form ID")
}
