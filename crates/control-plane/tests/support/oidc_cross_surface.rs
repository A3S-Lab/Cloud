use super::*;
use a3s_cloud_control_plane::modules::identity::domain::value_objects::{
    ExternalIdentitySubject, OidcIssuer,
};
use a3s_cloud_control_plane::modules::identity::{
    IOidcProviderService, OidcAuthorization, OidcAuthorizationRequest, OidcCodeVerificationRequest,
    OidcProviderError, VerifiedOidcIdentity,
};
use a3s_cloud_control_plane::modules::shared_kernel::application::{
    oauth_flow_digest, pkce_s256_challenge,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::Sha256Digest;
use a3s_cloud_control_plane::ControlPlane;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const POSTGRES_URL_ENV: &str = "A3S_CLOUD_OIDC3_POSTGRES_URL";
const BOOTSTRAP_TOKEN_ENV: &str = "A3S_CLOUD_OIDC3_BOOTSTRAP_TOKEN";
const BOOTSTRAP_TOKEN_VALUE: &str = "oidc3-bootstrap-credential-0123456789abcdef";
const HUMAN_TOKEN: &str = "a3s_9999999999999999999999999999999999999999999999999999999999999999";
const PROVIDER_KEY: &str = "workforce";
const PROVIDER_SUBJECT: &str = "cross-surface-human";

#[derive(Default)]
struct CrossSurfaceOidcProvider {
    expected_secrets: Mutex<Vec<(Sha256Digest, Sha256Digest)>>,
    authorization_count: AtomicUsize,
    verification_count: AtomicUsize,
}

#[async_trait]
impl IOidcProviderService for CrossSurfaceOidcProvider {
    async fn authorization_url(
        &self,
        request: OidcAuthorizationRequest,
    ) -> std::result::Result<OidcAuthorization, OidcProviderError> {
        if request.provider_key.as_str() != PROVIDER_KEY {
            return Err(OidcProviderError::NotConfigured);
        }
        self.expected_secrets
            .lock()
            .map_err(|_| OidcProviderError::Unavailable)?
            .push((
                oauth_flow_digest(&request.nonce),
                oauth_flow_digest(&request.pkce_verifier),
            ));
        self.authorization_count.fetch_add(1, Ordering::SeqCst);
        let mut authorization_url = url::Url::parse("https://identity.example.test/authorize")
            .map_err(|_| OidcProviderError::Unavailable)?;
        authorization_url
            .query_pairs_mut()
            .append_pair("state", request.state.as_str())
            .append_pair("nonce", request.nonce.as_str())
            .append_pair(
                "code_challenge",
                pkce_s256_challenge(&request.pkce_verifier).as_str(),
            )
            .append_pair("code_challenge_method", "S256");
        Ok(OidcAuthorization {
            authorization_url: authorization_url.into(),
            provider_key: request.provider_key,
            issuer: issuer(),
            provider_config_digest: provider_digest(),
            flow_lifetime: chrono::Duration::minutes(5),
        })
    }

    async fn verify_code(
        &self,
        request: OidcCodeVerificationRequest,
    ) -> std::result::Result<VerifiedOidcIdentity, OidcProviderError> {
        if request.provider_key.as_str() != PROVIDER_KEY
            || !matches!(request.code.as_str(), "link-code" | "login-code")
        {
            return Err(OidcProviderError::Rejected);
        }
        let expected = (
            oauth_flow_digest(&request.nonce),
            oauth_flow_digest(&request.pkce_verifier),
        );
        let mut secrets = self
            .expected_secrets
            .lock()
            .map_err(|_| OidcProviderError::Unavailable)?;
        let position = secrets
            .iter()
            .position(|candidate| candidate == &expected)
            .ok_or(OidcProviderError::Rejected)?;
        secrets.swap_remove(position);
        self.verification_count.fetch_add(1, Ordering::SeqCst);
        Ok(VerifiedOidcIdentity {
            provider_key: request.provider_key,
            issuer: issuer(),
            provider_config_digest: provider_digest(),
            subject: ExternalIdentitySubject::parse(PROVIDER_SUBJECT).expect("fixture subject"),
            login_token_lifetime: chrono::Duration::hours(1),
        })
    }
}

pub async fn exercise_oidc_cross_surface(
    postgres_url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&postgres_url, 8).await?;
    let database = Database::new(PostgresDialect, executor);
    let _postgres_url = EnvironmentOverride::set(POSTGRES_URL_ENV, &postgres_url);
    let _bootstrap_token = EnvironmentOverride::set(BOOTSTRAP_TOKEN_ENV, BOOTSTRAP_TOKEN_VALUE);
    let state_directory = tempfile::tempdir()?;
    let mut application_config = config();
    application_config.postgres.serving_url_env = POSTGRES_URL_ENV.into();
    application_config.auth.bootstrap_token_env = BOOTSTRAP_TOKEN_ENV.into();
    configure_ephemeral_application_state(&mut application_config, state_directory.path());
    let provider = Arc::new(CrossSurfaceOidcProvider::default());

    let app = build_test_application(application_config.clone(), Arc::clone(&provider)).await?;
    let organization_id = bootstrap(&app).await?;
    let principal_id = create_human_principal(&app, &organization_id).await?;
    create_human_token(&app, &organization_id, &principal_id).await?;

    let link_path =
        format!("/api/v1/organizations/{organization_id}/identity/oidc/{PROVIDER_KEY}/link");
    let link_begin = app
        .call(
            BootRequest::new(HttpMethod::Post, link_path)
                .with_header("authorization", format!("Bearer {HUMAN_TOKEN}")),
        )
        .await?;
    assert_eq!(link_begin.status(), 200);
    assert_no_store(&link_begin);
    let link_authorization_url = required_string(
        &response_json(&link_begin)?["data"]["authorizationUrl"],
        "link authorization URL",
    )?;
    let link_state = url_query(&link_authorization_url, "state")?;
    let link_cookies = oidc_flow_cookies(&link_begin)?;
    assert_flow_cookie_security(&link_begin, &link_state, &link_cookies);

    drop(app);
    let app = build_test_application(application_config.clone(), Arc::clone(&provider)).await?;
    let linked = app
        .call(callback_request("link-code", &link_state, &link_cookies))
        .await?;
    assert_eq!(linked.status(), 200);
    assert_no_store(&linked);
    assert_cookies_deleted(&linked);
    let linked_body = response_json(&linked)?;
    assert_eq!(linked_body["data"]["kind"], "linked");
    assert_eq!(linked_body["data"]["principalId"], principal_id);
    let link_id = required_string(&linked_body["data"]["linkId"], "external identity link ID")?;
    assert_response_omits(
        &linked,
        &[
            "link-code",
            PROVIDER_SUBJECT,
            issuer().as_str(),
            &link_cookies[0].1,
            &link_cookies[1].1,
        ],
    );

    let link_replay = app
        .call(callback_request("link-code", &link_state, &link_cookies))
        .await?;
    assert_eq!(link_replay.status(), 404);
    assert_eq!(provider.verification_count.load(Ordering::SeqCst), 1);

    let login_path =
        format!("/api/v1/identity/oidc/{PROVIDER_KEY}/login?organization_id={organization_id}");
    let login_begin = app
        .call(BootRequest::new(HttpMethod::Get, login_path))
        .await?;
    assert_eq!(login_begin.status(), 303);
    assert_no_store(&login_begin);
    let login_authorization_url = login_begin
        .location()
        .ok_or("login response has no authorization redirect")?
        .to_owned();
    let login_state = url_query(&login_authorization_url, "state")?;
    let login_cookies = oidc_flow_cookies(&login_begin)?;
    assert_flow_cookie_security(&login_begin, &login_state, &login_cookies);

    drop(app);
    let app = build_test_application(application_config.clone(), Arc::clone(&provider)).await?;
    let logged_in = app
        .call(callback_request("login-code", &login_state, &login_cookies))
        .await?;
    assert_eq!(logged_in.status(), 200);
    assert_no_store(&logged_in);
    assert_cookies_deleted(&logged_in);
    let login_body = response_json(&logged_in)?;
    assert_eq!(login_body["data"]["kind"], "login");
    let token_id = required_string(&login_body["data"]["token"]["id"], "login token ID")?;
    let credential = required_string(&login_body["data"]["credential"], "login credential")?;
    assert_response_omits(
        &logged_in,
        &[
            "login-code",
            PROVIDER_SUBJECT,
            issuer().as_str(),
            &login_cookies[0].1,
            &login_cookies[1].1,
        ],
    );

    drop(app);
    let app = build_test_application(application_config, Arc::clone(&provider)).await?;
    let authenticated = app
        .call(get_as("/api/v1/organizations", &credential))
        .await?;
    assert_eq!(authenticated.status(), 200);
    let login_replay = app
        .call(callback_request("login-code", &login_state, &login_cookies))
        .await?;
    assert_eq!(login_replay.status(), 404);
    assert_eq!(provider.authorization_count.load(Ordering::SeqCst), 2);
    assert_eq!(provider.verification_count.load(Ordering::SeqCst), 2);
    assert!(provider
        .expected_secrets
        .lock()
        .expect("provider secret fixture")
        .is_empty());

    let organization_id = Uuid::parse_str(&organization_id)?;
    let principal_id = Uuid::parse_str(&principal_id)?;
    let link_id = Uuid::parse_str(&link_id)?;
    let token_id = Uuid::parse_str(&token_id)?;
    let evidence = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64)>(
                "select (select count(*) from external_identity_links where id = ",
            )
            .bind(link_id)
            .append(" and principal_id = ")
            .bind(principal_id)
            .append(" and provider_key = 'workforce' and issuer = 'https://identity.example.test' and subject = 'cross-surface-human' and revoked_at is null), (select count(*) from oidc_flows where organization_id = ")
            .bind(organization_id)
            .append(" and consumed_at is not null), (select count(*) from api_tokens where id = ")
            .bind(token_id)
            .append(" and organization_id = ")
            .bind(organization_id)
            .append(" and principal_id = ")
            .bind(principal_id)
            .append(" and revoked_at is null), (select count(*) from audit_records where (aggregate_id = ")
            .bind(link_id)
            .append(" and action = 'identity.external-identity.linked') or (aggregate_id = ")
            .bind(token_id)
            .append(" and action = 'identity.oidc.login')), (select count(*) from outbox_events where (aggregate_id = ")
            .bind(link_id)
            .append(" and event_key = 'identity.external-identity.linked') or (aggregate_id = ")
            .bind(token_id)
            .append(" and event_key = 'identity.token.created')), (select count(*) from oidc_flows where organization_id = ")
            .bind(organization_id)
            .append(" and consumed_at is null)"),
        )
        .await?;
    assert_eq!(
        evidence,
        (1, 2, 1, 2, 2, 0),
        "link, both flows, login token, audit, and Outbox must commit exactly once"
    );
    let plaintext_credential_rows = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from api_tokens where token_hash = ")
                .bind(credential.as_str()),
        )
        .await?;
    assert_eq!(plaintext_credential_rows, 0);
    for secret in link_cookies
        .iter()
        .chain(&login_cookies)
        .map(|(_, value)| value)
        .chain([&link_state, &login_state])
    {
        let plaintext_flow_rows = database
            .fetch_one_as(
                sql_query::<i64>("select count(*) from oidc_flows where state_digest = ")
                    .bind(secret.as_str())
                    .append(" or nonce_digest = ")
                    .bind(secret.as_str())
                    .append(" or pkce_verifier_digest = ")
                    .bind(secret.as_str()),
            )
            .await?;
        assert_eq!(
            plaintext_flow_rows, 0,
            "OIDC flow secret was stored in plaintext"
        );
    }
    Ok(())
}

async fn build_test_application(
    config: CloudConfig,
    provider: Arc<CrossSurfaceOidcProvider>,
) -> Result<ControlPlane, Box<dyn std::error::Error>> {
    Ok(build_application_with_source_resolver_and_oidc_provider(
        config,
        Arc::new(OfflineCommitSourceResolver),
        provider,
    )
    .await?)
}

async fn bootstrap(app: &ControlPlane) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(
            post_json(
                "/api/v1/bootstrap",
                "oidc3:bootstrap",
                json!({
                    "organizationName": "OIDC Cross Surface",
                    "tokenName": "OIDC bootstrap owner",
                    "token": ADMIN_TOKEN,
                    "expiresAt": null
                }),
            )
            .with_header("x-a3s-bootstrap-token", BOOTSTRAP_TOKEN_VALUE),
        )
        .await?;
    assert_eq!(response.status(), 201);
    required_string(
        &response_json(&response)?["data"]["organization"]["id"],
        "organization ID",
    )
}

async fn create_human_principal(
    app: &ControlPlane,
    organization_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/memberships"),
            "oidc3:human-membership",
            json!({
                "principalKind": "human",
                "name": "OIDC human",
                "role": "member"
            }),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    required_string(
        &response_json(&response)?["data"]["principalId"],
        "human principal ID",
    )
}

async fn create_human_token(
    app: &ControlPlane,
    organization_id: &str,
    principal_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization_id}/api-tokens"),
            "oidc3:human-token",
            json!({
                "principalId": principal_id,
                "name": "OIDC human link token",
                "token": HUMAN_TOKEN,
                "scopes": ["cloud:read"],
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(response.status(), 201);
    Ok(())
}

fn callback_request(code: &str, state: &str, cookies: &[(String, String)]) -> BootRequest {
    BootRequest::new(
        HttpMethod::Get,
        format!("/api/v1/identity/oidc/{PROVIDER_KEY}/callback?code={code}&state={state}"),
    )
    .with_header(
        "cookie",
        cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

fn oidc_flow_cookies(
    response: &BootResponse,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let cookies = response
        .header_values("set-cookie")
        .into_iter()
        .filter_map(|header| {
            header
                .split(';')
                .next()
                .and_then(|pair| pair.split_once('='))
                .filter(|(name, _)| name.starts_with("a3s_oidc_"))
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
        })
        .collect::<Vec<_>>();
    if cookies.len() != 2 {
        return Err(format!("OIDC response has {} flow cookies", cookies.len()).into());
    }
    Ok(cookies)
}

fn assert_flow_cookie_security(response: &BootResponse, state: &str, cookies: &[(String, String)]) {
    for (name, _) in cookies {
        assert!(!name.contains(state));
        let header = response
            .header_values("set-cookie")
            .into_iter()
            .find(|header| header.starts_with(&format!("{name}=")))
            .expect("flow cookie header");
        for attribute in [
            format!("Path=/api/v1/identity/oidc/{PROVIDER_KEY}/callback"),
            "HttpOnly".into(),
            "Secure".into(),
            "SameSite=Lax".into(),
        ] {
            assert!(header.contains(&attribute), "missing {attribute}");
        }
    }
}

fn assert_cookies_deleted(response: &BootResponse) {
    assert_eq!(
        response
            .header_values("set-cookie")
            .iter()
            .filter(|header| header.contains("Max-Age=0"))
            .count(),
        2
    );
}

fn assert_no_store(response: &BootResponse) {
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(response.header("pragma"), Some("no-cache"));
    assert_eq!(response.header("referrer-policy"), Some("no-referrer"));
}

fn assert_response_omits(response: &BootResponse, values: &[&str]) {
    let body = String::from_utf8_lossy(response.body());
    for value in values {
        assert!(!body.contains(value), "response leaked fixture value");
    }
}

fn url_query(url: &str, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    url::Url::parse(url)?
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| format!("URL has no {name} parameter").into())
}

fn required_string(value: &Value, label: &str) -> Result<String, Box<dyn std::error::Error>> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{label} is missing").into())
}

fn issuer() -> OidcIssuer {
    OidcIssuer::parse("https://identity.example.test").expect("fixture issuer")
}

fn provider_digest() -> Sha256Digest {
    Sha256Digest::from_bytes(b"OIDC cross-surface provider configuration")
}
