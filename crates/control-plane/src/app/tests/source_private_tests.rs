use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PRIVATE_SOURCE_TOKEN: &str = "fixture-private-source-installation-token";

#[derive(Clone, Copy)]
enum AuthenticatedResolution {
    Success,
    LeakingFailure,
}

struct PrivateSourceResolver {
    anonymous_calls: AtomicUsize,
    authenticated_calls: AtomicUsize,
    authenticated_resolution: AuthenticatedResolution,
}

impl PrivateSourceResolver {
    fn new(authenticated_resolution: AuthenticatedResolution) -> Self {
        Self {
            anonymous_calls: AtomicUsize::new(0),
            authenticated_calls: AtomicUsize::new(0),
            authenticated_resolution,
        }
    }
}

#[async_trait::async_trait]
impl ISourceResolver for PrivateSourceResolver {
    async fn resolve(
        &self,
        request: &SourceResolutionRequest,
        credential: Option<&SourceProviderCredential>,
    ) -> std::result::Result<ResolvedSource, SourceResolutionError> {
        let Some(credential) = credential else {
            self.anonymous_calls.fetch_add(1, Ordering::SeqCst);
            return Err(SourceResolutionError::Unavailable);
        };
        self.authenticated_calls.fetch_add(1, Ordering::SeqCst);
        assert!(credential.authorizes(&request.repository, Utc::now(), chrono::Duration::zero()));
        assert_eq!(credential.expose_token(), PRIVATE_SOURCE_TOKEN);
        match self.authenticated_resolution {
            AuthenticatedResolution::Success => Ok(ResolvedSource {
                repository: request.repository.clone(),
                commit_sha: crate::modules::sources::domain::GitCommitSha::parse(COMMIT)
                    .map_err(SourceResolutionError::Protocol)?,
            }),
            AuthenticatedResolution::LeakingFailure => Err(SourceResolutionError::Protocol(
                format!("provider accidentally rendered {PRIVATE_SOURCE_TOKEN}"),
            )),
        }
    }
}

struct TestGithubInstallationTokens {
    calls: AtomicUsize,
    fail_first: bool,
    requests: Mutex<Vec<(u64, String)>>,
}

impl TestGithubInstallationTokens {
    fn new(fail_first: bool) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            fail_first,
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl IGithubInstallationTokenService for TestGithubInstallationTokens {
    async fn issue(
        &self,
        request: GithubInstallationTokenRequest,
    ) -> std::result::Result<SourceProviderCredential, GithubInstallationTokenError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests
            .lock()
            .expect("installation-token requests")
            .push((
                request.installation_id.as_u64(),
                request.repository.identity().into(),
            ));
        if self.fail_first && call == 0 {
            return Err(GithubInstallationTokenError::Protocol(format!(
                "provider accidentally rendered {PRIVATE_SOURCE_TOKEN}"
            )));
        }
        SourceProviderCredential::new(
            &request.repository,
            zeroize::Zeroizing::new(PRIVATE_SOURCE_TOKEN.into()),
            request.requested_at,
            request.requested_at + chrono::Duration::hours(1),
        )
        .map_err(GithubInstallationTokenError::Protocol)
    }
}

#[tokio::test]
async fn private_source_uses_verified_installation_authority_without_persisting_the_token(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let sources = Arc::new(InMemorySourceRevisionRepository::new());
    let resolver = Arc::new(PrivateSourceResolver::new(AuthenticatedResolution::Success));
    let tokens = Arc::new(TestGithubInstallationTokens::new(false));
    let app = private_source_application(
        identity,
        projects,
        Arc::clone(&sources),
        resolver.clone(),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        tokens.clone(),
    )?;
    let organization = bootstrap_organization(&app, "private-source-org", "Acme").await?;
    connect_github_installation(&app, &organization).await?;
    let path = create_source_path(&app, &organization, "private-source").await?;
    let body = source_request("private-source-delivery");

    let accepted = app
        .call(post_json(&path, "private-source-resolution", body.clone()))
        .await?;
    assert_eq!(accepted.status(), 201);
    assert_eq!(response_json(&accepted)?["data"]["commitSha"], COMMIT);
    let replayed = app
        .call(post_json(&path, "private-source-resolution", body))
        .await?;
    assert_eq!(replayed.status(), 200);
    assert_eq!(resolver.anonymous_calls.load(Ordering::SeqCst), 1);
    assert_eq!(resolver.authenticated_calls.load(Ordering::SeqCst), 1);
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *tokens.requests.lock().expect("installation-token requests"),
        vec![(42, "github:github.com/a3s-lab/cloud".into())]
    );

    let listed = app.call(get_as(&path, ADMIN_TOKEN)).await?;
    let durable_and_api = format!(
        "{}{}{}{}",
        String::from_utf8_lossy(accepted.body()),
        String::from_utf8_lossy(replayed.body()),
        String::from_utf8_lossy(listed.body()),
        serde_json::to_string(&sources.outbox_events().await)
            .map_err(|error| BootError::Internal(error.to_string()))?
    );
    assert!(!durable_and_api.contains(PRIVATE_SOURCE_TOKEN));
    Ok(())
}

#[tokio::test]
async fn public_source_success_and_replay_never_issue_an_installation_token() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let tokens = Arc::new(TestGithubInstallationTokens::new(false));
    let app = build_test_application_with_source_dependencies_and_tokens(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::new(TestSourceResolver),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(TestGithubAppAuthorization),
        tokens.clone(),
    )?;
    let organization = bootstrap_organization(&app, "public-token-bypass-org", "Acme").await?;
    let path = create_source_path(&app, &organization, "public-token-bypass").await?;
    let body = source_request("public-token-bypass-delivery");

    assert_eq!(
        app.call(post_json(&path, "public-token-bypass", body.clone()))
            .await?
            .status(),
        201
    );
    assert_eq!(
        app.call(post_json(&path, "public-token-bypass", body))
            .await?
            .status(),
        200
    );
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn cross_tenant_connection_cannot_authorize_private_source_resolution() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let resolver = Arc::new(PrivateSourceResolver::new(AuthenticatedResolution::Success));
    let tokens = Arc::new(TestGithubInstallationTokens::new(false));
    let app = private_source_application(
        identity,
        projects,
        Arc::new(InMemorySourceRevisionRepository::new()),
        resolver.clone(),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        tokens.clone(),
    )?;
    let connected_organization =
        bootstrap_organization(&app, "private-connected-org", "Connected").await?;
    connect_github_installation(&app, &connected_organization).await?;
    let other_organization = create_organization(&app, "private-other-org", "Other").await?;
    let path = create_source_path(&app, &other_organization, "private-other").await?;

    let response = app
        .call(post_json(
            &path,
            "private-cross-tenant",
            source_request("private-cross-tenant-delivery"),
        ))
        .await?;
    assert_eq!(response.status(), 404);
    let rendered = String::from_utf8_lossy(response.body());
    assert!(!rendered.contains("connection"));
    assert!(!rendered.contains("installation"));
    assert!(!rendered.contains(PRIVATE_SOURCE_TOKEN));
    assert_eq!(resolver.anonymous_calls.load(Ordering::SeqCst), 1);
    assert_eq!(resolver.authenticated_calls.load(Ordering::SeqCst), 0);
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn private_source_token_and_authenticated_provider_failures_are_sanitized() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let resolver = Arc::new(PrivateSourceResolver::new(
        AuthenticatedResolution::LeakingFailure,
    ));
    let tokens = Arc::new(TestGithubInstallationTokens::new(true));
    let app = private_source_application(
        identity,
        projects,
        Arc::new(InMemorySourceRevisionRepository::new()),
        resolver.clone(),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        tokens.clone(),
    )?;
    let organization = bootstrap_organization(&app, "private-failure-org", "Acme").await?;
    connect_github_installation(&app, &organization).await?;
    let path = create_source_path(&app, &organization, "private-failure").await?;

    for (key, delivery) in [
        ("private-token-failure", "private-token-failure-delivery"),
        (
            "private-provider-failure",
            "private-provider-failure-delivery",
        ),
    ] {
        let response = app
            .call(post_json(&path, key, source_request(delivery)))
            .await?;
        assert_eq!(response.status(), 404);
        assert!(!String::from_utf8_lossy(response.body()).contains(PRIVATE_SOURCE_TOKEN));
    }
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 2);
    assert_eq!(resolver.anonymous_calls.load(Ordering::SeqCst), 2);
    assert_eq!(resolver.authenticated_calls.load(Ordering::SeqCst), 1);
    Ok(())
}

fn private_source_application(
    identity: Arc<InMemoryIdentityRepository>,
    projects: Arc<InMemoryProjectsRepository>,
    sources: Arc<InMemorySourceRevisionRepository>,
    resolver: Arc<dyn ISourceResolver>,
    connections: Arc<InMemoryGithubConnectionRepository>,
    tokens: Arc<dyn IGithubInstallationTokenService>,
) -> Result<a3s_boot::BootApplication> {
    build_test_application_with_source_dependencies_and_tokens(
        identity,
        projects,
        Arc::new(InMemorySecretRepository::new()),
        Arc::new(InMemoryWorkloadRepository::new()),
        sources,
        resolver,
        connections,
        Arc::new(TestGithubAppAuthorization),
        tokens,
    )
}

pub(super) async fn connect_github_installation(
    app: &a3s_boot::BootApplication,
    organization: &str,
) -> Result<()> {
    let started = app
        .call(
            BootRequest::new(
                HttpMethod::Post,
                format!("/api/v1/organizations/{organization}/source-connections/github"),
            )
            .with_header("authorization", format!("Bearer {ADMIN_TOKEN}")),
        )
        .await?;
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
    if connected.status() != 201 {
        return Err(BootError::Internal(
            "test GitHub installation did not connect".into(),
        ));
    }
    Ok(())
}

async fn create_source_path(
    app: &a3s_boot::BootApplication,
    organization: &str,
    key_prefix: &str,
) -> Result<String> {
    let project =
        create_project(app, organization, &format!("{key_prefix}-project"), "Cloud").await?;
    let environment = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            &format!("{key_prefix}-environment"),
            json!({"name": "Production"}),
        ))
        .await?;
    Ok(format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{}/source-revisions",
        response_id(&environment)?
    ))
}

pub(super) fn source_request(delivery_id: &str) -> Value {
    json!({
        "repository": {
            "provider": "github",
            "url": "https://github.com/a3s-lab/cloud"
        },
        "reference": {"kind": "branch", "value": "main"},
        "recipe": {
            "schema": "a3s.cloud.build-recipe.v1",
            "kind": "dockerfile",
            "contextPath": "./services/api",
            "dockerfilePath": "Dockerfile",
            "target": "release",
            "platforms": ["linux/amd64"]
        },
        "webhookDeliveryId": delivery_id
    })
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
