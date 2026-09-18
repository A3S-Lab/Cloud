use super::*;
use crate::modules::identity::domain::services::{
    OidcAuthorization, OidcAuthorizationRequest, OidcCodeVerificationRequest, OidcProviderError,
    VerifiedOidcIdentity,
};
use crate::modules::identity::domain::value_objects::{ExternalIdentitySubject, OidcIssuer};
use crate::modules::shared_kernel::application::{oauth_flow_digest, pkce_s256_challenge};
use crate::modules::shared_kernel::domain::Sha256Digest;
use std::sync::RwLock;

const PROVIDER_KEY: &str = "workforce";
const FIXTURE_CODE: &str = "fixture-code";

#[derive(Clone)]
struct ExpectedSecrets {
    nonce_digest: Sha256Digest,
    pkce_digest: Sha256Digest,
}

struct TransportOidcProvider {
    expected: RwLock<Option<ExpectedSecrets>>,
}

impl TransportOidcProvider {
    fn new() -> Self {
        Self {
            expected: RwLock::new(None),
        }
    }
}

#[async_trait::async_trait]
impl IOidcProviderService for TransportOidcProvider {
    async fn authorization_url(
        &self,
        request: OidcAuthorizationRequest,
    ) -> std::result::Result<OidcAuthorization, OidcProviderError> {
        if request.provider_key.as_str() != PROVIDER_KEY {
            return Err(OidcProviderError::NotConfigured);
        }
        *self.expected.write().expect("expected OIDC secrets") = Some(ExpectedSecrets {
            nonce_digest: oauth_flow_digest(&request.nonce),
            pkce_digest: oauth_flow_digest(&request.pkce_verifier),
        });
        Ok(OidcAuthorization {
            authorization_url: format!(
                "https://identity.example.test/authorize?state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
                request.state.as_str(),
                request.nonce.as_str(),
                pkce_s256_challenge(&request.pkce_verifier),
            ),
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
        let expected = self
            .expected
            .read()
            .expect("expected OIDC secrets")
            .clone()
            .ok_or(OidcProviderError::Rejected)?;
        if request.provider_key.as_str() != PROVIDER_KEY
            || request.code.as_str() != FIXTURE_CODE
            || oauth_flow_digest(&request.nonce) != expected.nonce_digest
            || oauth_flow_digest(&request.pkce_verifier) != expected.pkce_digest
        {
            return Err(OidcProviderError::Rejected);
        }
        Ok(VerifiedOidcIdentity {
            provider_key: request.provider_key,
            issuer: issuer(),
            provider_config_digest: provider_digest(),
            subject: ExternalIdentitySubject::parse("fixture-subject").expect("subject"),
            login_token_lifetime: chrono::Duration::hours(1),
        })
    }
}

#[tokio::test]
async fn oidc_link_and_login_are_cookie_bound_replay_safe_and_secretless() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let provider: Arc<dyn IOidcProviderService> = Arc::new(TransportOidcProvider::new());
    let app = build_test_application_with_oidc_provider(
        Arc::clone(&identity),
        Arc::new(InMemoryProjectsRepository::new()),
        provider,
    )?;
    let organization = bootstrap_organization(&app, "oidc-bootstrap", "OIDC Organization").await?;
    let membership = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/memberships"),
            "oidc-human-member",
            json!({
                "principalKind": "human",
                "name": "OIDC Human",
                "role": "member"
            }),
        ))
        .await?;
    assert_eq!(membership.status(), 201);
    let principal_id = response_json(&membership)?["data"]["principalId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("human membership has no principal ID".into()))?
        .to_owned();
    let human_token = format!("a3s_{}", "3".repeat(64));
    let token = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/api-tokens"),
            "oidc-human-token",
            json!({
                "principalId": principal_id,
                "name": "OIDC human token",
                "token": human_token.clone(),
                "scopes": [ApiTokenScope::CLOUD_READ],
                "expiresAt": null
            }),
        ))
        .await?;
    assert_eq!(token.status(), 201);
    let link_path =
        format!("/api/v1/organizations/{organization}/identity/oidc/{PROVIDER_KEY}/link");

    let unauthenticated = app
        .call(BootRequest::new(HttpMethod::Post, &link_path))
        .await?;
    assert_eq!(unauthenticated.status(), 401);
    assert_no_store(&unauthenticated);

    let link_begin = app
        .call(
            BootRequest::new(HttpMethod::Post, &link_path)
                .with_header("authorization", format!("Bearer {human_token}")),
        )
        .await?;
    assert_eq!(link_begin.status(), 200);
    assert_no_store(&link_begin);
    let link_location = response_json(&link_begin)?["data"]["authorizationUrl"]
        .as_str()
        .ok_or_else(|| BootError::Internal("OIDC link response has no authorization URL".into()))?
        .to_owned();
    let link_state = url_query(&link_location, "state")?;
    let link_cookies = oidc_flow_cookies(&link_begin)?;
    assert_flow_cookie_security(&link_begin, PROVIDER_KEY, &link_state, &link_cookies);

    let link_callback = callback_request(&link_state, &link_cookies);
    let linked = app.call(link_callback).await?;
    assert_eq!(linked.status(), 200);
    assert_no_store(&linked);
    assert_cookies_deleted(&linked);
    let linked_body = response_json(&linked)?;
    assert_eq!(linked_body["data"]["kind"], "linked");
    assert_eq!(linked_body["data"]["providerKey"], PROVIDER_KEY);
    assert_eq!(linked_body["data"]["subject"], Value::Null);
    assert_eq!(linked_body["data"]["issuer"], Value::Null);
    assert_response_omits(&linked, &[FIXTURE_CODE, "fixture-subject"]);

    let login_path =
        format!("/api/v1/identity/oidc/{PROVIDER_KEY}/login?organization_id={organization}");
    let login_begin = app
        .call(BootRequest::new(HttpMethod::Get, login_path))
        .await?;
    assert_eq!(login_begin.status(), 303);
    assert_no_store(&login_begin);
    let login_location = login_begin
        .location()
        .ok_or_else(|| BootError::Internal("OIDC login response has no redirect".into()))?;
    let login_state = url_query(login_location, "state")?;
    let login_cookies = oidc_flow_cookies(&login_begin)?;
    assert_flow_cookie_security(&login_begin, PROVIDER_KEY, &login_state, &login_cookies);
    assert_ne!(link_cookies[0].0, login_cookies[0].0);

    let missing_cookie = app
        .call(BootRequest::new(
            HttpMethod::Get,
            callback_path(&login_state),
        ))
        .await?;
    assert_eq!(missing_cookie.status(), 400);
    assert_no_store(&missing_cookie);
    assert_cookies_deleted(&missing_cookie);

    let logged_in = app
        .call(callback_request(&login_state, &login_cookies))
        .await?;
    assert_eq!(logged_in.status(), 200);
    assert_no_store(&logged_in);
    assert_cookies_deleted(&logged_in);
    let login_body = response_json(&logged_in)?;
    assert_eq!(login_body["data"]["kind"], "login");
    assert_eq!(login_body["data"]["token"]["organizationId"], organization);
    let scopes = login_body["data"]["token"]["scopes"]
        .as_array()
        .ok_or_else(|| BootError::Internal("OIDC token response has no scopes".into()))?;
    assert!(scopes
        .iter()
        .any(|scope| scope == ApiTokenScope::CLOUD_READ));
    assert!(!scopes
        .iter()
        .any(|scope| scope == ApiTokenScope::TOKEN_WRITE));
    let credential = login_body["data"]["credential"]
        .as_str()
        .ok_or_else(|| BootError::Internal("OIDC login response has no credential".into()))?;
    assert!(credential.starts_with("a3s_"));
    assert_response_omits(
        &logged_in,
        &[
            FIXTURE_CODE,
            "fixture-subject",
            &login_cookies[0].1,
            &login_cookies[1].1,
        ],
    );

    let authenticated = app
        .call(get_as("/api/v1/organizations", credential))
        .await?;
    assert_eq!(authenticated.status(), 200);

    let replay = app
        .call(callback_request(&login_state, &login_cookies))
        .await?;
    assert_eq!(replay.status(), 404);
    assert_no_store(&replay);
    assert_cookies_deleted(&replay);
    Ok(())
}

fn callback_path(state: &str) -> String {
    format!("/api/v1/identity/oidc/{PROVIDER_KEY}/callback?code={FIXTURE_CODE}&state={state}")
}

fn callback_request(state: &str, cookies: &[(String, String)]) -> BootRequest {
    BootRequest::new(HttpMethod::Get, callback_path(state)).with_header(
        "cookie",
        cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

fn oidc_flow_cookies(response: &BootResponse) -> Result<Vec<(String, String)>> {
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
        return Err(BootError::Internal(format!(
            "OIDC response has {} flow cookies",
            cookies.len()
        )));
    }
    Ok(cookies)
}

fn assert_flow_cookie_security(
    response: &BootResponse,
    provider_key: &str,
    state: &str,
    cookies: &[(String, String)],
) {
    assert!(cookies.iter().any(|(name, _)| name.contains("_nonce_")));
    assert!(cookies.iter().any(|(name, _)| name.contains("_pkce_")));
    for (name, value) in cookies {
        assert_eq!(value.len(), 43);
        assert!(!name.contains(state));
        let header = response
            .header_values("set-cookie")
            .into_iter()
            .find(|header| header.starts_with(&format!("{name}=")))
            .expect("flow cookie header");
        for attribute in [
            format!("Path=/api/v1/identity/oidc/{provider_key}/callback"),
            "HttpOnly".into(),
            "Secure".into(),
            "SameSite=Lax".into(),
            "Max-Age=".into(),
        ] {
            assert!(header.contains(&attribute), "{attribute}");
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

fn assert_response_omits(response: &BootResponse, values: &[&str]) {
    let body = String::from_utf8_lossy(response.body());
    for value in values {
        assert!(!body.contains(value), "response leaked fixture value");
    }
}

fn url_query(url: &str, name: &str) -> Result<String> {
    url::Url::parse(url)
        .map_err(|error| BootError::Internal(format!("invalid test URL: {error}")))?
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| BootError::Internal(format!("test URL has no {name} parameter")))
}

fn issuer() -> OidcIssuer {
    OidcIssuer::parse("https://identity.example.test").expect("issuer")
}

fn provider_digest() -> Sha256Digest {
    Sha256Digest::from_bytes(b"transport-provider-configuration")
}
