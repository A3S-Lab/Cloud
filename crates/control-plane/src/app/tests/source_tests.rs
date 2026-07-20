use super::*;

const COMMIT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COMMIT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn source_request(repository_url: &str, commit_sha: &str, delivery_id: &str) -> Value {
    json!({
        "repository": {
            "provider": "github",
            "url": repository_url
        },
        "commitSha": commit_sha,
        "recipe": {
            "schema": "a3s.cloud.build-recipe.v1",
            "kind": "dockerfile",
            "contextPath": "./services/api",
            "dockerfilePath": "Dockerfile",
            "target": "release",
            "platforms": ["linux/arm64", "linux/amd64"]
        },
        "webhookDeliveryId": delivery_id
    })
}

#[tokio::test]
async fn external_source_revisions_are_canonical_immutable_and_delivery_deduplicated() -> Result<()>
{
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let sources = Arc::new(InMemorySourceRevisionRepository::new());
    let app = build_test_application_with_sources(identity, projects, sources.clone())?;
    let organization = bootstrap_organization(&app, "source-organization", "Acme").await?;
    let project = create_project(&app, &organization, "source-project", "Cloud").await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "source-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;
    let path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/source-revisions"
    );

    let first = app
        .call(post_json(
            &path,
            "accept-source-a",
            source_request(
                "https://github.com/A3S-Lab/Cloud.git",
                COMMIT_A,
                "delivery-a",
            ),
        ))
        .await?;
    assert_eq!(first.status(), 201);
    let first_body = response_json(&first)?;
    assert_eq!(
        first_body["data"]["repository"]["canonicalUrl"],
        "https://github.com/a3s-lab/cloud"
    );
    assert_eq!(
        first_body["data"]["repository"]["identity"],
        "github:github.com/a3s-lab/cloud"
    );
    assert_eq!(first_body["data"]["commitSha"], COMMIT_A);
    assert_eq!(
        first_body["data"]["recipe"]["platforms"],
        json!(["linux/amd64", "linux/arm64"])
    );
    assert_eq!(
        first_body["data"]["recipeDigest"].as_str().map(str::len),
        Some(71)
    );
    assert_eq!(first_body["data"]["replayed"], false);

    let canonical_duplicate = app
        .call(post_json(
            &path,
            "accept-source-a-canonical-duplicate",
            source_request(
                "https://GITHUB.com/a3s-lab/cloud/",
                &COMMIT_A.to_ascii_uppercase(),
                "delivery-a",
            ),
        ))
        .await?;
    assert_eq!(canonical_duplicate.status(), 200);
    let duplicate_body = response_json(&canonical_duplicate)?;
    assert_eq!(duplicate_body["data"]["id"], first_body["data"]["id"]);
    assert_eq!(duplicate_body["data"]["replayed"], true);

    let moved_delivery = app
        .call(post_json(
            &path,
            "accept-source-b-reused-delivery",
            source_request("https://github.com/a3s-lab/cloud", COMMIT_B, "delivery-a"),
        ))
        .await?;
    assert_eq!(moved_delivery.status(), 409);
    assert_eq!(response_json(&moved_delivery)?["statusCode"], "CONFLICT");

    let idempotency_conflict = app
        .call(post_json(
            &path,
            "accept-source-a",
            source_request("https://github.com/a3s-lab/cloud", COMMIT_B, "delivery-b"),
        ))
        .await?;
    assert_eq!(idempotency_conflict.status(), 409);

    let listed = app.call(get_as(&path, ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    let listed_body = response_json(&listed)?;
    assert_eq!(listed_body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed_body["data"][0]["id"], first_body["data"]["id"]);
    assert_eq!(
        sources
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "source.revision.accepted")
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn source_revision_inputs_and_tenant_ownership_fail_closed() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let app = build_test_application(identity, projects)?;
    let organization = bootstrap_organization(&app, "source-validation-org", "Acme").await?;
    let project = create_project(&app, &organization, "source-validation-project", "Cloud").await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "source-validation-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    let environment = response_id(&environment)?;
    let path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/source-revisions"
    );

    create_api_token(
        &app,
        &organization,
        "source-project-only-token",
        "project-only",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;
    let denied_scope = app
        .call(post_json_as(
            &path,
            "source-denied-scope",
            source_request(
                "https://github.com/a3s-lab/cloud",
                COMMIT_A,
                "delivery-denied-scope",
            ),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(denied_scope.status(), 403);

    create_api_token(
        &app,
        &organization,
        "source-write-token",
        "source-writer",
        SOURCE_TOKEN,
        &[ApiTokenScope::SOURCE_WRITE],
        None,
    )
    .await?;
    let source_scoped = app
        .call(post_json_as(
            &path,
            "source-allowed-scope",
            source_request(
                "https://github.com/a3s-lab/cloud",
                COMMIT_A,
                "delivery-allowed-scope",
            ),
            SOURCE_TOKEN,
        ))
        .await?;
    assert_eq!(source_scoped.status(), 201);
    assert_eq!(app.call(get_as(&path, PROJECT_TOKEN)).await?.status(), 200);

    for (key, repository) in [
        ("source-http", "http://github.com/a3s-lab/cloud"),
        (
            "source-userinfo",
            "https://github.com@evil.example/a3s-lab/cloud",
        ),
        (
            "source-confused-host",
            "https://github.com.evil.example/a3s-lab/cloud",
        ),
        (
            "source-encoded-path",
            "https://github.com/a3s-lab%2fother/cloud",
        ),
        (
            "source-query",
            "https://github.com/a3s-lab/cloud?token=secret",
        ),
    ] {
        let response = app
            .call(post_json(
                &path,
                key,
                source_request(repository, COMMIT_A, key),
            ))
            .await?;
        assert_eq!(response.status(), 422, "{repository}");
    }

    let traversal = app
        .call(post_json(
            &path,
            "source-traversal",
            json!({
                "repository": {
                    "provider": "github",
                    "url": "https://github.com/a3s-lab/cloud"
                },
                "commitSha": COMMIT_A,
                "recipe": {
                    "schema": "a3s.cloud.build-recipe.v1",
                    "kind": "dockerfile",
                    "contextPath": "../outside",
                    "dockerfilePath": "Dockerfile",
                    "target": null,
                    "platforms": ["linux/amd64"]
                },
                "webhookDeliveryId": "delivery-traversal"
            }),
        ))
        .await?;
    assert_eq!(traversal.status(), 422);

    let wrong_environment = app
        .call(post_json(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{}/source-revisions",
                Uuid::new_v4()
            ),
            "source-missing-environment",
            source_request(
                "https://github.com/a3s-lab/cloud",
                COMMIT_A,
                "delivery-missing-environment",
            ),
        ))
        .await?;
    assert_eq!(wrong_environment.status(), 404);
    Ok(())
}
