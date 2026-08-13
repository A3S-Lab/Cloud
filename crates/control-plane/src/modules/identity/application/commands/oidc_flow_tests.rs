use super::begin_oidc_flow::{BeginOidcFlow, BeginOidcFlowHandler};
use super::complete_oidc_flow::{
    CompleteOidcFlow, CompleteOidcFlowHandler, CompleteOidcFlowResult,
};
use crate::modules::identity::application::commands::bootstrap_identity::{
    BootstrapIdentity, BootstrapIdentityHandler,
};
use crate::modules::identity::application::commands::create_membership::{
    CreateMembership, CreateMembershipHandler,
};
use crate::modules::identity::domain::entities::OidcFlowPurpose;
use crate::modules::identity::domain::repositories::{
    IApiTokenRepository, IMembershipRepository, IOidcIdentityRepository, IOrganizationRepository,
};
use crate::modules::identity::domain::services::{
    IOidcProviderService, OidcAuthorization, OidcAuthorizationRequest, OidcCodeVerificationRequest,
    OidcProviderError, VerifiedOidcIdentity,
};
use crate::modules::identity::domain::value_objects::{
    ApiTokenScope, ApiTokenSecret, ExternalIdentitySubject, OidcIssuer, OidcProviderKey,
};
use crate::modules::identity::infrastructure::persistence::InMemoryIdentityRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{OrganizationId, Sha256Digest};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use zeroize::Zeroizing;

#[tokio::test]
async fn link_then_login_uses_one_flow_repository_and_returns_one_credential() {
    let fixture = Fixture::new().await;
    let link = fixture.begin(OidcFlowPurpose::Link).await;
    let linked = fixture.complete(link).await.expect("link callback");
    let CompleteOidcFlowResult::Linked(linked) = linked else {
        panic!("expected linked identity");
    };
    assert_eq!(linked.principal_id, fixture.human_principal_id);
    assert_eq!(linked.subject.as_str(), "subject-42");

    let login = fixture.begin(OidcFlowPurpose::Login).await;
    let state = login.state.clone();
    let nonce = login.nonce.clone();
    let verifier = login.pkce_verifier.clone();
    let logged_in = fixture.complete(login).await.expect("login callback");
    let CompleteOidcFlowResult::LoggedIn {
        api_token,
        credential,
    } = logged_in
    else {
        panic!("expected login credential");
    };
    assert_eq!(api_token.principal_id, fixture.human_principal_id);
    assert!(api_token.expires_at.is_some());
    assert!(api_token.scopes.iter().all(|scope| !matches!(
        scope.as_str(),
        ApiTokenScope::PLATFORM_WRITE | ApiTokenScope::TOKEN_WRITE
    )));
    let secret = ApiTokenSecret::parse(credential.to_string()).expect("login credential");
    let authenticated = fixture
        .repository
        .authenticate(&secret.digest(), chrono::Utc::now())
        .await
        .expect("authenticate")
        .expect("active login credential");
    assert_eq!(authenticated.principal.id, fixture.human_principal_id);

    let replay = fixture
        .complete(FlowMaterial {
            state,
            nonce,
            pkce_verifier: verifier,
        })
        .await;
    let Err(replay) = replay else {
        panic!("callback replay succeeded");
    };
    assert!(matches!(replay, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn organization_and_provider_mismatches_fail_before_external_verification() {
    let fixture = Fixture::new().await;
    let organizations: Arc<dyn IOrganizationRepository> = fixture.repository.clone();
    let memberships: Arc<dyn IMembershipRepository> = fixture.repository.clone();
    let oidc_identity: Arc<dyn IOidcIdentityRepository> = fixture.repository.clone();
    let provider: Arc<dyn IOidcProviderService> = fixture.provider.clone();
    let begin = BeginOidcFlowHandler::new(organizations, memberships, oidc_identity, provider);
    let missing = begin
        .execute(
            BeginOidcFlow {
                organization_id: OrganizationId::new(),
                provider_key: provider_key(),
                purpose: OidcFlowPurpose::Login,
                principal_id: None,
            },
            context(),
        )
        .await
        .expect("begin command");
    let Err(missing) = missing else {
        panic!("missing organization began a flow");
    };
    assert!(matches!(missing, ApplicationError::NotFound(_)));
    assert_eq!(
        fixture
            .provider
            .authorization_requests
            .load(Ordering::SeqCst),
        0
    );

    let flow = fixture.begin(OidcFlowPurpose::Link).await;
    let error = fixture
        .complete_with_provider(flow, OidcProviderKey::parse("other").expect("provider"))
        .await;
    let Err(error) = error else {
        panic!("mismatched callback provider completed a flow");
    };
    assert!(matches!(error, ApplicationError::NotFound(_)));
    assert_eq!(
        fixture
            .provider
            .verification_requests
            .load(Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn provider_configuration_drift_fails_without_consuming_the_flow() {
    let fixture = Fixture::new().await;
    let flow = fixture.begin(OidcFlowPurpose::Link).await;
    fixture
        .provider
        .set_digest(Sha256Digest::from_bytes(b"changed-provider"));
    let drifted = fixture.complete(flow.clone()).await;
    let Err(drifted) = drifted else {
        panic!("configuration drift completed a flow");
    };
    assert!(matches!(drifted, ApplicationError::Conflict(_)));

    fixture.provider.set_digest(provider_digest());
    let completed = fixture.complete(flow).await.expect("unconsumed flow");
    assert!(matches!(completed, CompleteOidcFlowResult::Linked(_)));
}

#[tokio::test]
async fn rejected_provider_response_does_not_consume_the_flow() {
    let fixture = Fixture::new().await;
    let flow = fixture.begin(OidcFlowPurpose::Link).await;
    fixture
        .provider
        .reject_verification
        .store(true, Ordering::SeqCst);
    let rejected = fixture.complete(flow.clone()).await;
    let Err(rejected) = rejected else {
        panic!("rejected provider response completed a flow");
    };
    assert!(matches!(rejected, ApplicationError::Forbidden(_)));

    fixture
        .provider
        .reject_verification
        .store(false, Ordering::SeqCst);
    let completed = fixture.complete(flow).await.expect("unconsumed flow");
    assert!(matches!(completed, CompleteOidcFlowResult::Linked(_)));
}

#[derive(Clone)]
struct FlowMaterial {
    state: Zeroizing<String>,
    nonce: Zeroizing<String>,
    pkce_verifier: Zeroizing<String>,
}

struct Fixture {
    repository: Arc<InMemoryIdentityRepository>,
    provider: Arc<TestOidcProvider>,
    organization_id: OrganizationId,
    human_principal_id: crate::modules::shared_kernel::domain::PrincipalId,
}

impl Fixture {
    async fn new() -> Self {
        let repository = Arc::new(InMemoryIdentityRepository::new());
        let api_tokens: Arc<dyn IApiTokenRepository> = repository.clone();
        let bootstrapped = BootstrapIdentityHandler::new(api_tokens)
            .execute(
                BootstrapIdentity {
                    organization_name: "OIDC Test".into(),
                    token_name: "bootstrap".into(),
                    token_secret: format!("a3s_{}", "a".repeat(64)),
                    expires_at: None,
                    idempotency_key: format!("test:{}", Uuid::new_v4()),
                    request_id: Uuid::new_v4(),
                },
                context(),
            )
            .await
            .expect("bootstrap command")
            .expect("bootstrap identity");
        let organization_id = bootstrapped.identity.organization.id;
        let owner_id = bootstrapped.identity.principal.id;
        let memberships: Arc<dyn IMembershipRepository> = repository.clone();
        let human = CreateMembershipHandler::new(memberships)
            .execute(
                CreateMembership {
                    organization_id,
                    principal_kind: "human".into(),
                    name: "OIDC User".into(),
                    role: "member".into(),
                    actor_principal_id: owner_id,
                    actor_is_platform_admin: false,
                    idempotency_key: format!("test:{}", Uuid::new_v4()),
                    request_id: Uuid::new_v4(),
                },
                context(),
            )
            .await
            .expect("membership command")
            .expect("human membership");
        Self {
            repository,
            provider: Arc::new(TestOidcProvider::new()),
            organization_id,
            human_principal_id: human.membership.principal.id,
        }
    }

    async fn begin(&self, purpose: OidcFlowPurpose) -> FlowMaterial {
        let organizations: Arc<dyn IOrganizationRepository> = self.repository.clone();
        let memberships: Arc<dyn IMembershipRepository> = self.repository.clone();
        let oidc_identity: Arc<dyn IOidcIdentityRepository> = self.repository.clone();
        let provider: Arc<dyn IOidcProviderService> = self.provider.clone();
        let result = BeginOidcFlowHandler::new(organizations, memberships, oidc_identity, provider)
            .execute(
                BeginOidcFlow {
                    organization_id: self.organization_id,
                    provider_key: provider_key(),
                    purpose,
                    principal_id: matches!(purpose, OidcFlowPurpose::Link)
                        .then_some(self.human_principal_id),
                },
                context(),
            )
            .await
            .expect("begin command")
            .expect("begin flow");
        let authorization = url::Url::parse(&result.authorization_url).expect("authorization URL");
        let state = authorization
            .query_pairs()
            .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
            .expect("authorization state");
        FlowMaterial {
            state: Zeroizing::new(state),
            nonce: result.nonce,
            pkce_verifier: result.pkce_verifier,
        }
    }

    async fn complete(
        &self,
        flow: FlowMaterial,
    ) -> Result<CompleteOidcFlowResult, ApplicationError> {
        self.complete_with_provider(flow, provider_key()).await
    }

    async fn complete_with_provider(
        &self,
        flow: FlowMaterial,
        provider_key: OidcProviderKey,
    ) -> Result<CompleteOidcFlowResult, ApplicationError> {
        let oidc_identity: Arc<dyn IOidcIdentityRepository> = self.repository.clone();
        let provider: Arc<dyn IOidcProviderService> = self.provider.clone();
        CompleteOidcFlowHandler::new(oidc_identity, provider)
            .execute(
                CompleteOidcFlow {
                    provider_key,
                    code: Zeroizing::new("fixture-code".into()),
                    state: flow.state,
                    nonce: flow.nonce,
                    pkce_verifier: flow.pkce_verifier,
                    request_id: Uuid::new_v4(),
                },
                context(),
            )
            .await
            .expect("complete command")
    }
}

struct TestOidcProvider {
    digest: RwLock<Sha256Digest>,
    authorization_requests: AtomicUsize,
    verification_requests: AtomicUsize,
    reject_verification: std::sync::atomic::AtomicBool,
}

impl TestOidcProvider {
    fn new() -> Self {
        Self {
            digest: RwLock::new(provider_digest()),
            authorization_requests: AtomicUsize::new(0),
            verification_requests: AtomicUsize::new(0),
            reject_verification: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn set_digest(&self, digest: Sha256Digest) {
        *self.digest.write().expect("provider digest") = digest;
    }

    fn digest(&self) -> Sha256Digest {
        self.digest.read().expect("provider digest").clone()
    }
}

#[async_trait]
impl IOidcProviderService for TestOidcProvider {
    async fn authorization_url(
        &self,
        request: OidcAuthorizationRequest,
    ) -> Result<OidcAuthorization, OidcProviderError> {
        self.authorization_requests.fetch_add(1, Ordering::SeqCst);
        Ok(OidcAuthorization {
            authorization_url: format!(
                "https://identity.example.test/authorize?state={}",
                request.state.as_str()
            ),
            provider_key: request.provider_key,
            issuer: issuer(),
            provider_config_digest: self.digest(),
            flow_lifetime: chrono::Duration::minutes(5),
        })
    }

    async fn verify_code(
        &self,
        request: OidcCodeVerificationRequest,
    ) -> Result<VerifiedOidcIdentity, OidcProviderError> {
        self.verification_requests.fetch_add(1, Ordering::SeqCst);
        if self.reject_verification.load(Ordering::SeqCst)
            || request.code.as_str() != "fixture-code"
        {
            return Err(OidcProviderError::Rejected);
        }
        Ok(VerifiedOidcIdentity {
            provider_key: request.provider_key,
            issuer: issuer(),
            provider_config_digest: self.digest(),
            subject: ExternalIdentitySubject::parse("subject-42").expect("subject"),
            login_token_lifetime: chrono::Duration::hours(1),
        })
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn provider_key() -> OidcProviderKey {
    OidcProviderKey::parse("workforce").expect("provider")
}

fn issuer() -> OidcIssuer {
    OidcIssuer::parse("https://identity.example.test").expect("issuer")
}

fn provider_digest() -> Sha256Digest {
    Sha256Digest::from_bytes(b"provider-configuration")
}
