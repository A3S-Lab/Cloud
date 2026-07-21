use super::*;

const COMMIT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COMMIT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[tokio::test]
async fn github_repository_subscriptions_are_tenant_owned_and_fan_out_exact_pushes() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let sources = Arc::new(InMemorySourceRevisionRepository::new());
    let connections = Arc::new(InMemoryGithubConnectionRepository::new());
    let app = build_test_application_with_source_dependencies(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::clone(&sources),
        Arc::new(TestSourceResolver),
        Arc::clone(&connections),
        Arc::new(TestGithubAppAuthorization),
    )?;
    let organization = bootstrap_organization(&app, "github-subscription-org", "Acme").await?;
    let project =
        create_project(&app, &organization, "github-subscription-project", "Cloud").await?;
    let environment_response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "github-subscription-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment_response.status(), 201);
    let environment = response_id(&environment_response)?;
    connect_test_github_installation(&app, &organization).await?;

    let subscriptions_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/source-subscriptions/github"
    );
    let first = app
        .call(post_json(
            &subscriptions_path,
            "github-subscription-api",
            github_subscription_request("main", Some("release")),
        ))
        .await?;
    assert_eq!(first.status(), 201);
    let first_body = response_json(&first)?;
    assert_eq!(first_body["data"]["installationId"], 42);
    assert_eq!(first_body["data"]["branch"], "main");
    assert_eq!(first_body["data"]["status"], "active");
    assert_eq!(first_body["data"]["replayed"], false);
    assert_eq!(
        first_body["data"]["repository"]["identity"],
        "github:github.com/a3s-lab/cloud"
    );
    let first_id = response_id(&first)?;

    let replay = app
        .call(post_json(
            &subscriptions_path,
            "github-subscription-api",
            github_subscription_request("main", Some("release")),
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_id(&replay)?, first_id);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);
    let canonical_duplicate = app
        .call(post_json(
            &subscriptions_path,
            "github-subscription-api-canonical",
            github_subscription_request("main", Some("release")),
        ))
        .await?;
    assert_eq!(canonical_duplicate.status(), 200);
    assert_eq!(response_id(&canonical_duplicate)?, first_id);

    let second = app
        .call(post_json(
            &subscriptions_path,
            "github-subscription-worker",
            github_subscription_request("main", None),
        ))
        .await?;
    assert_eq!(second.status(), 201);
    let second_id = response_id(&second)?;
    assert_ne!(first_id, second_id);
    let listed = app.call(get_as(&subscriptions_path, ADMIN_TOKEN)).await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(
        response_json(&listed)?["data"].as_array().map(Vec::len),
        Some(2)
    );

    let revisions_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/source-revisions"
    );
    for (delivery_id, body) in [
        (
            "github-subscription-wrong-installation",
            github_push_payload_for_binding(
                "A3S-Lab/Cloud",
                "https://github.com/A3S-Lab/Cloud",
                99,
                "main",
                COMMIT_A,
            ),
        ),
        (
            "github-subscription-wrong-repository",
            github_push_payload_for_binding(
                "A3S-Lab/Runtime",
                "https://github.com/A3S-Lab/Runtime",
                42,
                "main",
                COMMIT_A,
            ),
        ),
        (
            "github-subscription-wrong-branch",
            github_push_payload_for_binding(
                "A3S-Lab/Cloud",
                "https://github.com/A3S-Lab/Cloud",
                42,
                "develop",
                COMMIT_A,
            ),
        ),
    ] {
        let response = app
            .call(github_webhook_request(
                "push",
                delivery_id,
                &body,
                GITHUB_WEBHOOK_SECRET,
            ))
            .await?;
        assert_eq!(response.status(), 202, "{delivery_id}");
    }
    assert_eq!(
        response_json(&app.call(get_as(&revisions_path, ADMIN_TOKEN)).await?)?["data"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    let accepted_body = github_push_payload(COMMIT_A);
    let accepted = app
        .call(github_webhook_request(
            "push",
            "github-subscription-accepted-a",
            &accepted_body,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(accepted.status(), 202);
    let accepted_replay = app
        .call(github_webhook_request(
            "push",
            "github-subscription-accepted-a",
            &accepted_body,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(accepted_replay.status(), 202);
    let after_first_push = response_json(&app.call(get_as(&revisions_path, ADMIN_TOKEN)).await?)?;
    assert_eq!(after_first_push["data"].as_array().map(Vec::len), Some(2));
    assert!(after_first_push["data"]
        .as_array()
        .is_some_and(|revisions| revisions.iter().all(|revision| {
            revision["commitSha"] == COMMIT_A && revision.get("reference").is_none()
        })));
    let changed_replay = app
        .call(github_webhook_request(
            "push",
            "github-subscription-accepted-a",
            &github_push_payload(COMMIT_B),
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(changed_replay.status(), 409);

    let deactivate_path = format!("{subscriptions_path}/{first_id}/deactivate");
    let deactivated = app
        .call(post_json(
            &deactivate_path,
            "github-subscription-api-deactivate",
            json!({}),
        ))
        .await?;
    assert_eq!(deactivated.status(), 200);
    assert_eq!(response_json(&deactivated)?["data"]["status"], "inactive");
    assert_eq!(response_json(&deactivated)?["data"]["aggregateVersion"], 2);
    let deactivation_replay = app
        .call(post_json(
            &deactivate_path,
            "github-subscription-api-deactivate",
            json!({}),
        ))
        .await?;
    assert_eq!(deactivation_replay.status(), 200);
    assert_eq!(
        response_json(&deactivation_replay)?["data"]["replayed"],
        true
    );

    let second_push = app
        .call(github_webhook_request(
            "push",
            "github-subscription-accepted-b",
            &github_push_payload(COMMIT_B),
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    assert_eq!(second_push.status(), 202);
    let final_revisions = response_json(&app.call(get_as(&revisions_path, ADMIN_TOKEN)).await?)?;
    assert_eq!(final_revisions["data"].as_array().map(Vec::len), Some(3));
    assert_eq!(
        sources
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "source.revision.accepted")
            .count(),
        3
    );
    assert_eq!(
        sources
            .outbox_events()
            .await
            .iter()
            .filter(|event| { event.event_key == "source.github-repository-subscription.created" })
            .count(),
        2
    );
    assert_eq!(
        sources
            .outbox_events()
            .await
            .iter()
            .filter(|event| {
                event.event_key == "source.github-repository-subscription.deactivated"
            })
            .count(),
        1
    );
    let durable_text = serde_json::to_string(&sources.outbox_events().await)
        .map_err(|error| BootError::Internal(error.to_string()))?;
    for forbidden in ["access_token", "client_secret", "private_key", "password"] {
        assert!(!durable_text.to_ascii_lowercase().contains(forbidden));
    }
    Ok(())
}

async fn connect_test_github_installation(app: &BootApplication, organization: &str) -> Result<()> {
    let path = format!("/api/v1/organizations/{organization}/source-connections/github");
    let started = app
        .call(
            BootRequest::new(HttpMethod::Post, path)
                .with_header("authorization", format!("Bearer {ADMIN_TOKEN}")),
        )
        .await?;
    assert_eq!(started.status(), 201);
    let installation_url = response_json(&started)?["data"]["installationUrl"]
        .as_str()
        .ok_or_else(|| BootError::Internal("install response has no URL".into()))?
        .to_owned();
    let installation_state = url_query(&installation_url, "state")?;
    let setup = app
        .call(BootRequest::new(
            HttpMethod::Get,
            format!(
                "/api/v1/source-connections/github/setup?installation_id=42&state={installation_state}"
            ),
        ))
        .await?;
    assert_eq!(setup.status(), 303);
    let oauth_state = url_query(
        setup
            .location()
            .ok_or_else(|| BootError::Internal("setup response has no redirect".into()))?,
        "state",
    )?;
    let verifier = response_cookie(&setup, "a3s_github_oauth_pkce")?;
    let connected = app
        .call(
            BootRequest::new(
                HttpMethod::Get,
                format!(
                    "/api/v1/source-connections/github/callback?code=valid-code&state={oauth_state}"
                ),
            )
            .with_header("cookie", format!("a3s_github_oauth_pkce={verifier}")),
        )
        .await?;
    assert_eq!(connected.status(), 201);
    Ok(())
}

#[tokio::test]
async fn github_repository_subscription_scope_hierarchy_and_inputs_fail_closed() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let sources = Arc::new(InMemorySourceRevisionRepository::new());
    let connections = Arc::new(InMemoryGithubConnectionRepository::new());
    let app = build_test_application_with_source_dependencies(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        sources,
        Arc::new(TestSourceResolver),
        connections,
        Arc::new(TestGithubAppAuthorization),
    )?;
    let organization =
        bootstrap_organization(&app, "github-subscription-validation-org", "Acme").await?;
    let project = create_project(
        &app,
        &organization,
        "github-subscription-validation-project",
        "Cloud",
    )
    .await?;
    let environment_response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "github-subscription-validation-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    let environment = response_id(&environment_response)?;
    let path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/source-subscriptions/github"
    );

    let without_connection = app
        .call(post_json(
            &path,
            "github-subscription-no-connection",
            github_subscription_request("main", Some("release")),
        ))
        .await?;
    assert_eq!(without_connection.status(), 404);
    connect_test_github_installation(&app, &organization).await?;
    create_api_token(
        &app,
        &organization,
        "github-subscription-project-token",
        "project-only",
        PROJECT_TOKEN,
        &[ApiTokenScope::PROJECT_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "github-subscription-source-token",
        "source-only",
        SOURCE_TOKEN,
        &[ApiTokenScope::SOURCE_WRITE],
        None,
    )
    .await?;
    let wrong_scope = app
        .call(post_json_as(
            &path,
            "github-subscription-wrong-scope",
            github_subscription_request("main", Some("release")),
            PROJECT_TOKEN,
        ))
        .await?;
    assert_eq!(wrong_scope.status(), 403);

    let invalid_branch = app
        .call(post_json_as(
            &path,
            "github-subscription-invalid-branch",
            github_subscription_request("refs/heads/main", Some("release")),
            SOURCE_TOKEN,
        ))
        .await?;
    assert_eq!(invalid_branch.status(), 422);
    let mut denied_request = github_subscription_request("main", Some("release"));
    denied_request["repository"]["url"] = json!("https://github.com/A3S-Lab/Runtime");
    let denied_repository = app
        .call(post_json_as(
            &path,
            "github-subscription-denied-repository",
            denied_request,
            SOURCE_TOKEN,
        ))
        .await?;
    assert_eq!(denied_repository.status(), 403);

    let created = app
        .call(post_json_as(
            &path,
            "github-subscription-valid",
            github_subscription_request("main", Some("release")),
            SOURCE_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let subscription_id = response_id(&created)?;
    let idempotency_conflict = app
        .call(post_json_as(
            &path,
            "github-subscription-valid",
            github_subscription_request("develop", Some("release")),
            SOURCE_TOKEN,
        ))
        .await?;
    assert_eq!(idempotency_conflict.status(), 409);

    let other_organization =
        create_organization(&app, "github-subscription-other-org", "Other").await?;
    let cross_tenant_path = format!(
        "/api/v1/organizations/{other_organization}/projects/{project}/environments/{environment}/source-subscriptions/github"
    );
    let cross_tenant = app
        .call(post_json_as(
            &cross_tenant_path,
            "github-subscription-cross-tenant",
            github_subscription_request("main", Some("release")),
            SOURCE_TOKEN,
        ))
        .await?;
    assert_eq!(cross_tenant.status(), 403);

    let other_environment_response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "github-subscription-other-environment",
            json!({"name": "Staging"}),
        ))
        .await?;
    let other_environment = response_id(&other_environment_response)?;
    let wrong_environment_deactivation = app
        .call(post_json_as(
            format!(
                "/api/v1/organizations/{organization}/projects/{project}/environments/{other_environment}/source-subscriptions/github/{subscription_id}/deactivate"
            ),
            "github-subscription-wrong-environment",
            json!({}),
            SOURCE_TOKEN,
        ))
        .await?;
    assert_eq!(wrong_environment_deactivation.status(), 404);
    Ok(())
}

pub(super) fn github_subscription_request(branch: &str, target: Option<&str>) -> Value {
    json!({
        "repository": {
            "provider": "github",
            "url": "https://github.com/A3S-Lab/Cloud.git"
        },
        "branch": branch,
        "recipe": {
            "schema": "a3s.cloud.build-recipe.v1",
            "kind": "dockerfile",
            "contextPath": "./services/api",
            "dockerfilePath": "Dockerfile",
            "target": target,
            "platforms": ["linux/arm64", "linux/amd64"]
        }
    })
}

pub(super) fn github_push_payload_for_binding(
    full_name: &str,
    html_url: &str,
    installation_id: u64,
    branch: &str,
    commit: &str,
) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "ref": format!("refs/heads/{branch}"),
        "after": commit,
        "deleted": false,
        "repository": {
            "full_name": full_name,
            "html_url": html_url
        },
        "installation": {"id": installation_id}
    }))
    .expect("GitHub push payload")
}

fn url_query(url: &str, name: &str) -> Result<String> {
    url::Url::parse(url)
        .map_err(|error| BootError::Internal(format!("invalid test URL: {error}")))?
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| BootError::Internal(format!("test URL has no {name} parameter")))
}

fn response_cookie(response: &BootResponse, name: &str) -> Result<String> {
    response
        .header_values("set-cookie")
        .into_iter()
        .find_map(|header| {
            header
                .split(';')
                .next()
                .and_then(|pair| pair.split_once('='))
                .filter(|(cookie_name, _)| *cookie_name == name)
                .map(|(_, value)| value.to_owned())
        })
        .ok_or_else(|| BootError::Internal(format!("response has no {name} cookie")))
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
