use super::*;

const COMMIT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COMMIT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[tokio::test]
async fn signed_github_push_is_public_bounded_and_durably_deduplicated() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let sources = Arc::new(InMemorySourceRevisionRepository::new());
    let app = build_test_application_with_sources(identity, projects, sources.clone())?;
    let body = github_push_payload(COMMIT_A);

    let first = app
        .call(github_webhook_request(
            "push",
            "github-delivery-a",
            &body,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(first.status(), 202);
    assert_eq!(response_json(&first)?["data"]["received"], true);
    let replay = app
        .call(github_webhook_request(
            "push",
            "github-delivery-a",
            &body,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(replay.status(), 202);
    let inbox = sources.webhook_inbox().await;
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].commit_sha.as_str(), COMMIT_A);
    assert_eq!(inbox[0].reference.value(), "main");

    let mut reformatted = body.clone();
    reformatted.push(b'\n');
    let raw_payload_conflict = app
        .call(github_webhook_request(
            "push",
            "github-delivery-a",
            &reformatted,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(raw_payload_conflict.status(), 409);
    assert_eq!(sources.webhook_inbox().await.len(), 1);

    let changed = github_push_payload(COMMIT_B);
    let conflict = app
        .call(github_webhook_request(
            "push",
            "github-delivery-a",
            &changed,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(conflict.status(), 409);
    assert_eq!(sources.webhook_inbox().await.len(), 1);

    let invalid_signature = app
        .call(
            github_webhook_request(
                "push",
                "github-delivery-invalid",
                &body,
                "another-github-webhook-secret-0123456789",
            )
            .with_header("authorization", format!("Bearer {ADMIN_TOKEN}")),
        )
        .await?;
    assert_eq!(invalid_signature.status(), 401);
    assert_eq!(sources.webhook_inbox().await.len(), 1);

    let ping = app
        .call(github_webhook_request(
            "ping",
            "github-delivery-ping",
            &body,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(ping.status(), 202);
    assert_eq!(sources.webhook_inbox().await.len(), 1);

    let deleted = github_push_payload_for_reference(
        "refs/heads/main",
        "0000000000000000000000000000000000000000",
        true,
    );
    let deleted_response = app
        .call(github_webhook_request(
            "push",
            "github-delivery-delete",
            &deleted,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(deleted_response.status(), 202);
    let tag = github_push_payload_for_reference("refs/tags/v1", COMMIT_A, false);
    let tag_response = app
        .call(github_webhook_request(
            "push",
            "github-delivery-tag",
            &tag,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(tag_response.status(), 202);
    assert_eq!(sources.webhook_inbox().await.len(), 1);

    let oversized = vec![b'x'; 1024 * 1024 + 1];
    let too_large = app
        .call(github_webhook_request(
            "push",
            "github-delivery-large",
            &oversized,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(too_large.status(), 413);
    assert_eq!(sources.webhook_inbox().await.len(), 1);
    Ok(())
}

fn source_request(
    repository_url: &str,
    reference_kind: &str,
    reference_value: &str,
    delivery_id: &str,
) -> Value {
    json!({
        "repository": {
            "provider": "github",
            "url": repository_url
        },
        "reference": {
            "kind": reference_kind,
            "value": reference_value
        },
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

fn github_push_payload(commit: &str) -> Vec<u8> {
    github_push_payload_for_reference("refs/heads/main", commit, false)
}

fn github_push_payload_for_reference(git_reference: &str, commit: &str, deleted: bool) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "ref": git_reference,
        "after": commit,
        "deleted": deleted,
        "repository": {
            "full_name": "A3S-Lab/Cloud",
            "html_url": "https://github.com/A3S-Lab/Cloud"
        },
        "installation": {"id": 42}
    }))
    .expect("GitHub push payload")
}

fn github_webhook_request(
    event: &str,
    delivery_id: &str,
    body: &[u8],
    secret: &str,
) -> BootRequest {
    BootRequest::new(HttpMethod::Post, "/api/v1/webhooks/github")
        .with_header("content-type", "application/json")
        .with_header("x-github-event", event)
        .with_header("x-github-delivery", delivery_id)
        .with_header("x-hub-signature-256", github_signature(secret, body))
        .with_body(body.to_vec())
}

fn github_signature(secret: &str, body: &[u8]) -> String {
    use hmac::{Hmac, Mac};

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC");
    mac.update(body);
    format!("sha256={:x}", mac.finalize().into_bytes())
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
                "branch",
                "main",
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
    assert!(first_body["data"].get("reference").is_none());
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
                "branch",
                "main",
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
            source_request(
                "https://github.com/a3s-lab/cloud",
                "commit",
                COMMIT_B,
                "delivery-a",
            ),
        ))
        .await?;
    assert_eq!(moved_delivery.status(), 409);
    assert_eq!(response_json(&moved_delivery)?["statusCode"], "CONFLICT");

    let idempotency_conflict = app
        .call(post_json(
            &path,
            "accept-source-a",
            source_request(
                "https://github.com/a3s-lab/cloud",
                "commit",
                COMMIT_B,
                "delivery-b",
            ),
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
                "branch",
                "main",
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
                "branch",
                "main",
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
                source_request(repository, "branch", "main", key),
            ))
            .await?;
        assert_eq!(response.status(), 422, "{repository}");
    }

    let repository_denied = app
        .call(post_json(
            &path,
            "source-repository-denied",
            source_request(
                "https://github.com/a3s-lab/runtime",
                "branch",
                "main",
                "delivery-repository-denied",
            ),
        ))
        .await?;
    assert_eq!(repository_denied.status(), 403);

    let unsafe_reference = app
        .call(post_json(
            &path,
            "source-unsafe-reference",
            source_request(
                "https://github.com/a3s-lab/cloud",
                "branch",
                "refs/heads/main",
                "delivery-unsafe-reference",
            ),
        ))
        .await?;
    assert_eq!(unsafe_reference.status(), 422);

    let traversal = app
        .call(post_json(
            &path,
            "source-traversal",
            json!({
                "repository": {
                    "provider": "github",
                    "url": "https://github.com/a3s-lab/cloud"
                },
                "reference": {
                    "kind": "branch",
                    "value": "main"
                },
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
                "branch",
                "main",
                "delivery-missing-environment",
            ),
        ))
        .await?;
    assert_eq!(wrong_environment.status(), 404);
    Ok(())
}

#[tokio::test]
async fn idempotency_replay_never_resolves_an_accepted_moving_branch_again() -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MovingResolver {
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ISourceResolver for MovingResolver {
        async fn resolve(
            &self,
            request: &SourceResolutionRequest,
        ) -> std::result::Result<ResolvedSource, SourceResolutionError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let commit = if call == 0 { COMMIT_A } else { COMMIT_B };
            Ok(ResolvedSource {
                repository: request.repository.clone(),
                commit_sha: crate::modules::sources::domain::GitCommitSha::parse(commit)
                    .map_err(SourceResolutionError::Protocol)?,
            })
        }
    }

    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let sources = Arc::new(InMemorySourceRevisionRepository::new());
    let resolver = Arc::new(MovingResolver {
        calls: AtomicUsize::new(0),
    });
    let app = build_test_application_with_source_resolver(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        sources,
        resolver.clone(),
    )?;
    let organization = bootstrap_organization(&app, "moving-source-org", "Acme").await?;
    let project = create_project(&app, &organization, "moving-source-project", "Cloud").await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "moving-source-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    let path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{}/source-revisions",
        response_id(&environment)?
    );
    let mut body = source_request(
        "https://github.com/a3s-lab/cloud",
        "branch",
        "main",
        "delivery-moving-main",
    );
    body["webhookDeliveryId"] = Value::Null;

    let accepted = app
        .call(post_json(&path, "moving-main", body.clone()))
        .await?;
    assert_eq!(accepted.status(), 201);
    assert_eq!(response_json(&accepted)?["data"]["commitSha"], COMMIT_A);

    let replayed = app
        .call(post_json(&path, "moving-main", body.clone()))
        .await?;
    assert_eq!(replayed.status(), 200);
    assert_eq!(
        response_json(&replayed)?["data"]["id"],
        response_json(&accepted)?["data"]["id"]
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);

    let moved = app.call(post_json(&path, "moving-main-new", body)).await?;
    assert_eq!(moved.status(), 201);
    assert_eq!(response_json(&moved)?["data"]["commitSha"], COMMIT_B);
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 2);
    Ok(())
}
