use super::source_private_tests::{connect_github_installation, source_request};
use super::source_subscription_tests::{
    github_push_payload_for_binding, github_subscription_request,
};
use super::source_tests::github_webhook_request;
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const COMMIT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COMMIT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const COMMIT_C: &str = "cccccccccccccccccccccccccccccccccccccccc";
const COMMIT_D: &str = "dddddddddddddddddddddddddddddddddddddddd";
const COMMIT_E: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

struct UnavailableSourceResolver;

#[async_trait::async_trait]
impl ISourceResolver for UnavailableSourceResolver {
    async fn resolve(
        &self,
        _request: &SourceResolutionRequest,
        _credential: Option<&SourceProviderCredential>,
    ) -> std::result::Result<ResolvedSource, SourceResolutionError> {
        Err(SourceResolutionError::Unavailable)
    }
}

#[derive(Default)]
struct CountingInstallationTokens {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl IGithubInstallationTokenService for CountingInstallationTokens {
    async fn issue(
        &self,
        _request: GithubInstallationTokenRequest,
    ) -> std::result::Result<SourceProviderCredential, GithubInstallationTokenError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(GithubInstallationTokenError::Unavailable)
    }
}

#[tokio::test]
async fn signed_lifecycle_gates_authority_replays_and_requires_explicit_reconnection() -> Result<()>
{
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let sources = Arc::new(InMemorySourceRevisionRepository::new());
    let connections = Arc::new(InMemoryGithubConnectionRepository::new());
    let tokens = Arc::new(CountingInstallationTokens::default());
    let app = build_test_application_with_source_dependencies_and_tokens(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::clone(&sources),
        Arc::new(UnavailableSourceResolver),
        Arc::clone(&connections),
        Arc::new(TestGithubAppAuthorization),
        tokens.clone(),
    )?;
    let organization = bootstrap_organization(&app, "github-lifecycle-org", "Acme").await?;
    let project = create_project(&app, &organization, "github-lifecycle-project", "Cloud").await?;
    let environment_response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            "github-lifecycle-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    let environment = response_id(&environment_response)?;
    connect_github_installation(&app, &organization).await?;

    let connection_path = format!("/api/v1/organizations/{organization}/source-connections/github");
    let initial = response_json(&app.call(get_as(&connection_path, ADMIN_TOKEN)).await?)?;
    assert_eq!(initial["data"]["status"], "active");
    assert_eq!(initial["data"]["connectedAt"], initial["data"]["updatedAt"]);
    assert_eq!(
        initial["data"]["providerAuthority"]["checkedAt"],
        initial["data"]["connectedAt"]
    );
    assert_eq!(
        initial["data"]["providerAuthority"]["consecutiveFailures"],
        0
    );
    assert!(initial["data"]["providerAuthority"]["lastError"].is_null());
    let initial_connection_id = initial["data"]["id"].clone();
    let subscriptions_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/source-subscriptions/github"
    );
    assert_eq!(
        app.call(post_json(
            &subscriptions_path,
            "github-lifecycle-subscription-old",
            github_subscription_request("main", Some("release")),
        ))
        .await?
        .status(),
        201
    );
    let revisions_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/source-revisions"
    );

    let suspended = installation_payload("suspend", "A3S-Lab");
    for _ in 0..2 {
        assert_eq!(
            app.call(github_webhook_request(
                "installation",
                "github-lifecycle-suspend",
                &suspended,
                GITHUB_WEBHOOK_SECRET,
            ))
            .await?
            .status(),
            202
        );
    }
    assert_eq!(
        response_json(&app.call(get_as(&connection_path, ADMIN_TOKEN)).await?)?["data"]["status"],
        "suspended"
    );
    assert_eq!(
        connections
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "source.github-connection.reconciled")
            .count(),
        1
    );
    assert_eq!(
        app.call(post_json(
            &subscriptions_path,
            "github-lifecycle-subscription-blocked",
            github_subscription_request("develop", None),
        ))
        .await?
        .status(),
        409
    );
    assert_eq!(
        app.call(post_json(
            &revisions_path,
            "github-lifecycle-private-blocked",
            source_request("github-lifecycle-private-delivery"),
        ))
        .await?
        .status(),
        404
    );
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 0);
    push(&app, "github-lifecycle-push-suspended", COMMIT_A).await?;
    assert_eq!(revision_count(&app, &revisions_path).await?, 0);

    let changed_replay = installation_payload("deleted", "A3S-Lab");
    assert_eq!(
        app.call(github_webhook_request(
            "installation",
            "github-lifecycle-suspend",
            &changed_replay,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?
        .status(),
        409
    );
    let unsuspended = installation_payload("unsuspend", "A3S-Lab");
    assert_eq!(
        app.call(github_webhook_request(
            "installation",
            "github-lifecycle-unsuspend",
            &unsuspended,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?
        .status(),
        202
    );
    assert_eq!(
        response_json(&app.call(get_as(&connection_path, ADMIN_TOKEN)).await?)?["data"]["status"],
        "active"
    );
    push(&app, "github-lifecycle-push-active", COMMIT_B).await?;
    assert_eq!(revision_count(&app, &revisions_path).await?, 1);

    let revoked = serde_json::to_vec(&json!({
        "action": "revoked",
        "sender": {"id": 200, "login": "octocat"}
    }))
    .map_err(|error| BootError::Internal(error.to_string()))?;
    assert_eq!(
        app.call(github_webhook_request(
            "github_app_authorization",
            "github-lifecycle-verification-revoked",
            &revoked,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?
        .status(),
        202
    );
    assert_eq!(
        response_json(&app.call(get_as(&connection_path, ADMIN_TOKEN)).await?)?["data"]["status"],
        "verification_revoked"
    );
    push(&app, "github-lifecycle-push-revoked", COMMIT_C).await?;
    assert_eq!(revision_count(&app, &revisions_path).await?, 1);

    connect_github_installation(&app, &organization).await?;
    let reconnected = response_json(&app.call(get_as(&connection_path, ADMIN_TOKEN)).await?)?;
    assert_eq!(reconnected["data"]["status"], "active");
    assert_ne!(reconnected["data"]["id"], initial_connection_id);
    assert_eq!(connections.connections().await.len(), 2);
    push(&app, "github-lifecycle-push-old-binding", COMMIT_C).await?;
    assert_eq!(revision_count(&app, &revisions_path).await?, 1);

    assert_eq!(
        app.call(post_json(
            &subscriptions_path,
            "github-lifecycle-subscription-new",
            github_subscription_request("main", Some("release")),
        ))
        .await?
        .status(),
        201
    );
    push(&app, "github-lifecycle-push-new-binding", COMMIT_D).await?;
    assert_eq!(revision_count(&app, &revisions_path).await?, 2);

    let deleted = installation_payload("deleted", "A3S-Lab");
    assert_eq!(
        app.call(github_webhook_request(
            "installation",
            "github-lifecycle-installation-deleted",
            &deleted,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?
        .status(),
        202
    );
    assert_eq!(
        response_json(&app.call(get_as(&connection_path, ADMIN_TOKEN)).await?)?["data"]["status"],
        "installation_deleted"
    );
    push(&app, "github-lifecycle-push-deleted", COMMIT_E).await?;
    assert_eq!(revision_count(&app, &revisions_path).await?, 2);
    assert_eq!(
        connections
            .outbox_events()
            .await
            .iter()
            .filter(|event| event.event_key == "source.github-connection.reconciled")
            .count(),
        4
    );
    let durable = serde_json::to_string(&connections.outbox_events().await)
        .map_err(|error| BootError::Internal(error.to_string()))?;
    for forbidden in ["access_token", "client_secret", "private_key", "password"] {
        assert!(!durable.to_ascii_lowercase().contains(forbidden));
    }
    Ok(())
}

fn installation_payload(action: &str, login: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "action": action,
        "installation": {
            "id": 42,
            "account": {"id": 100, "login": login, "type": "Organization"}
        },
        "sender": {"id": 200, "login": "octocat"}
    }))
    .expect("installation payload")
}

async fn push(app: &BootApplication, delivery_id: &str, commit: &str) -> Result<()> {
    let body = github_push_payload_for_binding(
        "A3S-Lab/Cloud",
        "https://github.com/A3S-Lab/Cloud",
        42,
        "main",
        commit,
    );
    let response = app
        .call(github_webhook_request(
            "push",
            delivery_id,
            &body,
            GITHUB_WEBHOOK_SECRET,
        ))
        .await?;
    if response.status() != 202 {
        return Err(BootError::Internal(format!(
            "test GitHub push returned {}",
            response.status()
        )));
    }
    Ok(())
}

async fn revision_count(app: &BootApplication, revisions_path: &str) -> Result<usize> {
    response_json(&app.call(get_as(revisions_path, ADMIN_TOKEN)).await?)?["data"]
        .as_array()
        .map(Vec::len)
        .ok_or_else(|| BootError::Internal("source revisions response is not an array".into()))
}
