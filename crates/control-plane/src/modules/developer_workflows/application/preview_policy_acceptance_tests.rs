use super::{
    AcceptPullRequestPreviewPolicy, AcceptPullRequestPreviewPolicyHandler, DeveloperWorkflowAction,
    DeveloperWorkflowEnvironmentAccess, IDeveloperWorkflowAuthorizationPort,
    IPreviewSourceSubscriptionQueryPort, PreviewSourceSubscriptionBinding,
};
use crate::modules::developer_workflows::domain::{
    GitBranch, GithubInstallationRef, IPullRequestPreviewPolicyRepository,
    PullRequestPreviewPolicyContract,
};
use crate::modules::developer_workflows::infrastructure::InMemoryPullRequestPreviewPolicyRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectId, RepositoryError, SourceSubscriptionId,
};
use crate::modules::sources::published::{GitProvider, GitRepository};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

const POLICY_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/p0.3/pull-request-preview-policy.acl"
));

#[tokio::test]
async fn policy_acceptance_is_authorized_exact_revisioned_and_replay_safe() {
    let fixture = Fixture::new();
    let repository = Arc::new(InMemoryPullRequestPreviewPolicyRepository::new());
    let source = Arc::new(FakeSourcePort::new(Some(fixture.source_binding())));
    let handler = AcceptPullRequestPreviewPolicyHandler::new(
        repository.clone(),
        source.clone(),
        authorization(true),
    );

    let first = handler
        .execute(fixture.command("accept-1", POLICY_FIXTURE), context())
        .await
        .expect("Boot result")
        .expect("acceptance");
    assert!(!first.replayed);
    assert_eq!(first.revision.revision_number, 1);
    assert_eq!(source.calls(), 1);

    let replay = handler
        .execute(fixture.command("accept-1", POLICY_FIXTURE), context())
        .await
        .expect("Boot replay result")
        .expect("acceptance replay");
    assert!(replay.replayed);
    assert_eq!(replay.revision, first.revision);
    assert_eq!(source.calls(), 1, "replay must not re-resolve Sources");

    let mut adoption = fixture.command("accept-same-other-actor", POLICY_FIXTURE);
    adoption.actor_principal_id = PrincipalId::new();
    let adopted = handler
        .execute(adoption.clone(), context())
        .await
        .expect("Boot adoption result")
        .expect("natural adoption");
    let adopted_replay = handler
        .execute(adoption, context())
        .await
        .expect("Boot adoption replay result")
        .expect("natural adoption replay");
    assert!(adopted.replayed);
    assert!(adopted_replay.replayed);
    assert_eq!(adopted.revision, first.revision);
    assert_eq!(adopted_replay.revision, first.revision);
    assert_eq!(repository.outbox_events().await.len(), 1);

    let changed = POLICY_FIXTURE.replace("lifetime_seconds = 86400", "lifetime_seconds = 172800");
    let second = handler
        .execute(fixture.command("accept-2", &changed), context())
        .await
        .expect("Boot second result")
        .expect("second revision");
    assert!(!second.replayed);
    assert_eq!(second.revision.revision_number, 2);
    assert_eq!(repository.outbox_events().await.len(), 2);
    assert_eq!(
        repository
            .list_revisions(
                fixture.organization_id,
                fixture.project_id,
                fixture.environment_id,
                fixture.subscription_id,
                10,
            )
            .await
            .expect("policy revisions"),
        vec![first.revision, second.revision]
    );
}

#[tokio::test]
async fn authorization_precedes_acl_parsing_source_resolution_and_replay() {
    let fixture = Fixture::new();
    let repository = Arc::new(InMemoryPullRequestPreviewPolicyRepository::new());
    let source = Arc::new(FakeSourcePort::new(Some(fixture.source_binding())));
    let handler = AcceptPullRequestPreviewPolicyHandler::new(
        repository.clone(),
        source.clone(),
        authorization(false),
    );
    let error = handler
        .execute(fixture.command("forbidden", "not an ACL"), context())
        .await
        .expect("Boot result")
        .expect_err("unauthorized policy must be concealed");
    assert!(matches!(error, ApplicationError::NotFound(_)));
    assert_eq!(source.calls(), 0);
    assert!(repository.outbox_events().await.is_empty());
}

#[tokio::test]
async fn inactive_or_drifted_source_binding_fails_before_persistence() {
    let fixture = Fixture::new();
    let repository = Arc::new(InMemoryPullRequestPreviewPolicyRepository::new());
    let mut binding = fixture.source_binding();
    binding.active = false;
    let source = Arc::new(FakeSourcePort::new(Some(binding)));
    let handler =
        AcceptPullRequestPreviewPolicyHandler::new(repository.clone(), source, authorization(true));
    let error = handler
        .execute(fixture.command("inactive", POLICY_FIXTURE), context())
        .await
        .expect("Boot result")
        .expect_err("inactive source must fail");
    assert!(matches!(error, ApplicationError::Conflict(_)));
    assert!(repository.outbox_events().await.is_empty());
}

struct FakeSourcePort {
    binding: Option<PreviewSourceSubscriptionBinding>,
    calls: AtomicUsize,
}

impl FakeSourcePort {
    fn new(binding: Option<PreviewSourceSubscriptionBinding>) -> Self {
        Self {
            binding,
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IPreviewSourceSubscriptionQueryPort for FakeSourcePort {
    async fn resolve(
        &self,
        organization_id: OrganizationId,
        source_subscription_id: SourceSubscriptionId,
    ) -> Result<Option<PreviewSourceSubscriptionBinding>, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.binding.clone().filter(|binding| {
            binding.organization_id == organization_id
                && binding.source_subscription_id == source_subscription_id
        }))
    }
}

struct FakeAuthorizationPort {
    allowed: bool,
}

#[async_trait]
impl IDeveloperWorkflowAuthorizationPort for FakeAuthorizationPort {
    async fn is_environment_action_allowed(
        &self,
        access: DeveloperWorkflowEnvironmentAccess,
    ) -> Result<bool, RepositoryError> {
        access.validate().map_err(RepositoryError::Forbidden)?;
        if access.action != DeveloperWorkflowAction::AcceptPullRequestPreviewPolicy {
            return Err(RepositoryError::Forbidden(
                "unexpected Developer Workflow action".into(),
            ));
        }
        Ok(self.allowed)
    }
}

fn authorization(allowed: bool) -> Arc<FakeAuthorizationPort> {
    Arc::new(FakeAuthorizationPort { allowed })
}

struct Fixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    subscription_id: SourceSubscriptionId,
}

impl Fixture {
    fn new() -> Self {
        let contract = PullRequestPreviewPolicyContract::parse_acl(POLICY_FIXTURE)
            .expect("Preview policy fixture");
        Self {
            organization_id: contract.policy().organization_id,
            project_id: contract.policy().project_id,
            environment_id: EnvironmentId::new(),
            subscription_id: contract.policy().source_subscription_id,
        }
    }

    fn source_binding(&self) -> PreviewSourceSubscriptionBinding {
        PreviewSourceSubscriptionBinding {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            source_subscription_id: self.subscription_id,
            installation_id: GithubInstallationRef::parse(42).expect("installation"),
            repository: GitRepository::parse(
                GitProvider::Github,
                "https://github.com/a3s-lab/cloud",
            )
            .expect("repository"),
            branch: GitBranch::parse("main").expect("branch"),
            active: true,
        }
    }

    fn command(&self, idempotency_key: &str, policy_acl: &str) -> AcceptPullRequestPreviewPolicy {
        AcceptPullRequestPreviewPolicy {
            organization_id: self.organization_id,
            project_id: self.project_id,
            source_environment_id: self.environment_id,
            source_subscription_id: self.subscription_id,
            policy_acl: policy_acl.into(),
            actor_principal_id: PrincipalId::from_uuid(
                Uuid::parse_str("018f0f70-0000-7000-8000-000000000105").expect("actor UUID"),
            ),
            idempotency_key: idempotency_key.into(),
            request_id: Uuid::now_v7(),
        }
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
