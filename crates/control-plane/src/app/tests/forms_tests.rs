use super::*;

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
