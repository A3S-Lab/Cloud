use super::{
    DeveloperWorkflowAction, DeveloperWorkflowEnvironmentAccess,
    GetAcceptedPullRequestPreviewPolicyRevision,
    GetAcceptedPullRequestPreviewPolicyRevisionHandler,
    GetCurrentAcceptedPullRequestPreviewPolicyRevision,
    GetCurrentAcceptedPullRequestPreviewPolicyRevisionHandler, GetPullRequestPreview,
    GetPullRequestPreviewHandler, IDeveloperWorkflowAuthorizationPort,
    ListAcceptedPullRequestPreviewPolicyRevisions,
    ListAcceptedPullRequestPreviewPolicyRevisionsHandler, PreviewPolicyQueryService,
    PullRequestPreviewQueryService, MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT,
};
use crate::modules::developer_workflows::domain::{
    reconcile_pull_request_preview, AcceptPullRequestPreviewPolicyRevisionWrite,
    AcceptedPullRequestPreviewPolicyRevision, CommitPullRequestPreviewProjection, GitBranch,
    GithubInstallationRef, IPullRequestPreviewPolicyRepository,
    IPullRequestPreviewProjectionRepository, PullRequestChange, PullRequestChangeKind,
    PullRequestPreview, PullRequestPreviewPolicyContract, PullRequestPreviewProjectionReceipt,
    MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GitCommitSha, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId,
    ProjectId, PullRequestPreviewPolicyRevisionId, RepositoryError, SourcePullRequestChangeId,
    SourceSubscriptionId,
};
use crate::modules::sources::published::{GitProvider, GitRepository};
use a3s_boot::{CqrsContext, ModuleRef, QueryHandler};
use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const POLICY_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/p0.3/pull-request-preview-policy.acl"
));

#[tokio::test]
async fn preview_queries_share_exact_authorized_application_authorities() {
    let fixture = Fixture::new();
    let policies = Arc::new(ScriptedPolicyRepository::new(
        Some(fixture.second.clone()),
        Some(fixture.first.clone()),
        vec![fixture.first.clone(), fixture.second.clone()],
    ));
    let previews = Arc::new(ScriptedPreviewRepository::new(Some(
        fixture.preview.clone(),
    )));
    let authorization = Arc::new(ScriptedAuthorization::new(true));
    let policy_queries = Arc::new(PreviewPolicyQueryService::new(
        policies.clone(),
        authorization.clone(),
    ));
    let current =
        GetCurrentAcceptedPullRequestPreviewPolicyRevisionHandler::new(Arc::clone(&policy_queries));
    let exact =
        GetAcceptedPullRequestPreviewPolicyRevisionHandler::new(Arc::clone(&policy_queries));
    let list = ListAcceptedPullRequestPreviewPolicyRevisionsHandler::new(policy_queries);
    let preview = GetPullRequestPreviewHandler::new(Arc::new(PullRequestPreviewQueryService::new(
        previews.clone(),
        authorization.clone(),
    )));

    assert_eq!(
        current
            .execute(fixture.current_query(), context())
            .await
            .expect("current query Boot result")
            .expect("current Preview Policy"),
        fixture.second
    );
    assert_eq!(
        exact
            .execute(fixture.exact_query(), context())
            .await
            .expect("exact query Boot result")
            .expect("exact Preview Policy"),
        fixture.first
    );
    assert_eq!(
        list.execute(fixture.list_query(50), context())
            .await
            .expect("list query Boot result")
            .expect("Preview Policy history"),
        vec![fixture.first.clone(), fixture.second.clone()]
    );
    assert_eq!(
        preview
            .execute(fixture.preview_query(), context())
            .await
            .expect("Preview query Boot result")
            .expect("current Preview"),
        fixture.preview
    );
    assert_eq!(
        authorization.actions(),
        vec![
            DeveloperWorkflowAction::ReadPullRequestPreviewPolicy,
            DeveloperWorkflowAction::ReadPullRequestPreviewPolicy,
            DeveloperWorkflowAction::ReadPullRequestPreviewPolicy,
            DeveloperWorkflowAction::ReadPullRequestPreview,
        ]
    );
    assert_eq!(policies.current_calls.load(Ordering::SeqCst), 1);
    assert_eq!(policies.revision_calls.load(Ordering::SeqCst), 1);
    assert_eq!(policies.list_calls.load(Ordering::SeqCst), 1);
    assert_eq!(previews.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn preview_queries_authorize_before_private_identifiers_and_bounds() {
    let fixture = Fixture::new();
    let policies = Arc::new(ScriptedPolicyRepository::default());
    let previews = Arc::new(ScriptedPreviewRepository::default());
    let denied_authorization = Arc::new(ScriptedAuthorization::new(false));
    let denied_policy = ListAcceptedPullRequestPreviewPolicyRevisionsHandler::new(Arc::new(
        PreviewPolicyQueryService::new(policies.clone(), denied_authorization.clone()),
    ));
    let denied_preview = GetPullRequestPreviewHandler::new(Arc::new(
        PullRequestPreviewQueryService::new(previews.clone(), denied_authorization),
    ));

    let denied_policy_error = denied_policy
        .execute(
            ListAcceptedPullRequestPreviewPolicyRevisions {
                source_subscription_id: SourceSubscriptionId::from_uuid(uuid::Uuid::nil()),
                limit: 0,
                ..fixture.list_query(1)
            },
            context(),
        )
        .await
        .expect("denied policy Boot result")
        .expect_err("denied policy must be concealed");
    let denied_preview_error = denied_preview
        .execute(
            GetPullRequestPreview {
                source_subscription_id: SourceSubscriptionId::from_uuid(uuid::Uuid::nil()),
                pull_request_id: 0,
                ..fixture.preview_query()
            },
            context(),
        )
        .await
        .expect("denied Preview Boot result")
        .expect_err("denied Preview must be concealed");
    assert!(matches!(denied_policy_error, ApplicationError::NotFound(_)));
    assert!(matches!(
        denied_preview_error,
        ApplicationError::NotFound(_)
    ));
    assert_eq!(policies.list_calls.load(Ordering::SeqCst), 0);
    assert_eq!(previews.calls.load(Ordering::SeqCst), 0);

    let allowed_policy = ListAcceptedPullRequestPreviewPolicyRevisionsHandler::new(Arc::new(
        PreviewPolicyQueryService::new(
            policies.clone(),
            Arc::new(ScriptedAuthorization::new(true)),
        ),
    ));
    for limit in [0, MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT + 1] {
        let error = allowed_policy
            .execute(fixture.list_query(limit), context())
            .await
            .expect("invalid policy query Boot result")
            .expect_err("invalid page bound");
        assert!(matches!(error, ApplicationError::Invalid(_)));
    }
    let allowed_preview =
        GetPullRequestPreviewHandler::new(Arc::new(PullRequestPreviewQueryService::new(
            previews.clone(),
            Arc::new(ScriptedAuthorization::new(true)),
        )));
    let error = allowed_preview
        .execute(
            GetPullRequestPreview {
                pull_request_id: MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER + 1,
                ..fixture.preview_query()
            },
            context(),
        )
        .await
        .expect("invalid Preview query Boot result")
        .expect_err("non-portable pull-request ID");
    assert!(matches!(error, ApplicationError::Invalid(_)));
    assert_eq!(policies.list_calls.load(Ordering::SeqCst), 0);
    assert_eq!(previews.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn preview_queries_fail_closed_on_restored_scope_order_and_bound_drift() {
    let fixture = Fixture::new();
    let mut wrong_policy = fixture.second.clone();
    wrong_policy.source_environment_id = EnvironmentId::new();
    let current = GetCurrentAcceptedPullRequestPreviewPolicyRevisionHandler::new(Arc::new(
        PreviewPolicyQueryService::new(
            Arc::new(ScriptedPolicyRepository::new(
                Some(wrong_policy),
                None,
                Vec::new(),
            )),
            Arc::new(ScriptedAuthorization::new(true)),
        ),
    ));
    assert!(matches!(
        current
            .execute(fixture.current_query(), context())
            .await
            .expect("drifted current Boot result")
            .expect_err("cross-scope policy must fail"),
        ApplicationError::Internal(_)
    ));

    let non_canonical = ListAcceptedPullRequestPreviewPolicyRevisionsHandler::new(Arc::new(
        PreviewPolicyQueryService::new(
            Arc::new(ScriptedPolicyRepository::new(
                None,
                None,
                vec![fixture.second.clone(), fixture.first.clone()],
            )),
            Arc::new(ScriptedAuthorization::new(true)),
        ),
    ));
    assert!(matches!(
        non_canonical
            .execute(fixture.list_query(2), context())
            .await
            .expect("non-canonical list Boot result")
            .expect_err("non-canonical history must fail"),
        ApplicationError::Internal(_)
    ));

    let over_bound = ListAcceptedPullRequestPreviewPolicyRevisionsHandler::new(Arc::new(
        PreviewPolicyQueryService::new(
            Arc::new(ScriptedPolicyRepository::new(
                None,
                None,
                vec![fixture.first.clone(), fixture.second.clone()],
            )),
            Arc::new(ScriptedAuthorization::new(true)),
        ),
    ));
    assert!(matches!(
        over_bound
            .execute(fixture.list_query(1), context())
            .await
            .expect("over-bound list Boot result")
            .expect_err("over-bound history must fail"),
        ApplicationError::Internal(_)
    ));

    let mut wrong_preview = fixture.preview.clone();
    wrong_preview.policy_authority.source_environment_id = EnvironmentId::new();
    let preview = GetPullRequestPreviewHandler::new(Arc::new(PullRequestPreviewQueryService::new(
        Arc::new(ScriptedPreviewRepository::new(Some(wrong_preview))),
        Arc::new(ScriptedAuthorization::new(true)),
    )));
    assert!(matches!(
        preview
            .execute(fixture.preview_query(), context())
            .await
            .expect("drifted Preview Boot result")
            .expect_err("cross-scope Preview must fail"),
        ApplicationError::Internal(_)
    ));
}

struct Fixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    subscription_id: SourceSubscriptionId,
    principal_id: PrincipalId,
    first: AcceptedPullRequestPreviewPolicyRevision,
    second: AcceptedPullRequestPreviewPolicyRevision,
    preview: PullRequestPreview,
}

impl Fixture {
    fn new() -> Self {
        let first_contract =
            PullRequestPreviewPolicyContract::parse_acl(POLICY_FIXTURE).expect("policy fixture");
        let organization_id = first_contract.policy().organization_id;
        let project_id = first_contract.policy().project_id;
        let subscription_id = first_contract.policy().source_subscription_id;
        let environment_id = EnvironmentId::new();
        let principal_id = PrincipalId::new();
        let accepted_at = Utc
            .with_ymd_and_hms(2026, 8, 28, 2, 0, 0)
            .single()
            .expect("timestamp");
        let first = AcceptedPullRequestPreviewPolicyRevision::accept(
            environment_id,
            first_contract,
            1,
            principal_id,
            accepted_at,
        )
        .expect("first policy");
        let second_contract = PullRequestPreviewPolicyContract::parse_acl(
            &POLICY_FIXTURE.replace("lifetime_seconds = 86400", "lifetime_seconds = 172800"),
        )
        .expect("second policy fixture");
        let second = AcceptedPullRequestPreviewPolicyRevision::accept(
            environment_id,
            second_contract,
            2,
            principal_id,
            accepted_at + Duration::seconds(1),
        )
        .expect("second policy");
        let authority = second.preview_authority().expect("policy authority");
        let base_repository =
            GitRepository::parse(GitProvider::Github, "https://github.com/a3s-lab/cloud")
                .expect("repository");
        let change = PullRequestChange {
            installation_id: GithubInstallationRef::parse(42).expect("installation"),
            base_repository: base_repository.clone(),
            base_branch: GitBranch::parse("main").expect("base branch"),
            head_repository: Some(base_repository),
            head_branch: GitBranch::parse("feature/preview").expect("head branch"),
            head_commit_sha: GitCommitSha::parse("0123456789abcdef0123456789abcdef01234567")
                .expect("commit"),
            pull_request_id: 1_000_042,
            pull_request_number: 42,
            kind: PullRequestChangeKind::Opened,
            merged: false,
            provider_created_at: accepted_at,
            provider_updated_at: accepted_at + Duration::seconds(2),
        };
        let preview = reconcile_pull_request_preview(&authority, None, &change)
            .expect("Preview reconciliation")
            .preview
            .expect("created Preview");
        Self {
            organization_id,
            project_id,
            environment_id,
            subscription_id,
            principal_id,
            first,
            second,
            preview,
        }
    }

    fn current_query(&self) -> GetCurrentAcceptedPullRequestPreviewPolicyRevision {
        GetCurrentAcceptedPullRequestPreviewPolicyRevision {
            organization_id: self.organization_id,
            project_id: self.project_id,
            source_environment_id: self.environment_id,
            source_subscription_id: self.subscription_id,
            principal_id: self.principal_id,
        }
    }

    fn exact_query(&self) -> GetAcceptedPullRequestPreviewPolicyRevision {
        GetAcceptedPullRequestPreviewPolicyRevision {
            organization_id: self.organization_id,
            project_id: self.project_id,
            source_environment_id: self.environment_id,
            source_subscription_id: self.subscription_id,
            preview_policy_revision_id: self.first.id,
            principal_id: self.principal_id,
        }
    }

    fn list_query(&self, limit: usize) -> ListAcceptedPullRequestPreviewPolicyRevisions {
        ListAcceptedPullRequestPreviewPolicyRevisions {
            organization_id: self.organization_id,
            project_id: self.project_id,
            source_environment_id: self.environment_id,
            source_subscription_id: self.subscription_id,
            limit,
            principal_id: self.principal_id,
        }
    }

    fn preview_query(&self) -> GetPullRequestPreview {
        GetPullRequestPreview {
            organization_id: self.organization_id,
            project_id: self.project_id,
            source_environment_id: self.environment_id,
            source_subscription_id: self.subscription_id,
            pull_request_id: self.preview.pull_request_id,
            principal_id: self.principal_id,
        }
    }
}

#[derive(Default)]
struct ScriptedPolicyRepository {
    current: Option<AcceptedPullRequestPreviewPolicyRevision>,
    exact: Option<AcceptedPullRequestPreviewPolicyRevision>,
    list: Vec<AcceptedPullRequestPreviewPolicyRevision>,
    current_calls: AtomicUsize,
    revision_calls: AtomicUsize,
    list_calls: AtomicUsize,
}

impl ScriptedPolicyRepository {
    fn new(
        current: Option<AcceptedPullRequestPreviewPolicyRevision>,
        exact: Option<AcceptedPullRequestPreviewPolicyRevision>,
        list: Vec<AcceptedPullRequestPreviewPolicyRevision>,
    ) -> Self {
        Self {
            current,
            exact,
            list,
            ..Self::default()
        }
    }
}

#[async_trait]
impl IPullRequestPreviewPolicyRepository for ScriptedPolicyRepository {
    async fn replay_acceptance(
        &self,
        _idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        Ok(None)
    }

    async fn accept(
        &self,
        _write: AcceptPullRequestPreviewPolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        Err(RepositoryError::Storage("unexpected policy write".into()))
    }

    async fn find_revision(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _source_environment_id: EnvironmentId,
        _source_subscription_id: SourceSubscriptionId,
        _revision_id: PullRequestPreviewPolicyRevisionId,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        self.revision_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.exact.clone())
    }

    async fn find_current(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _source_environment_id: EnvironmentId,
        _source_subscription_id: SourceSubscriptionId,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        self.current_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.current.clone())
    }

    async fn find_effective_at(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _source_environment_id: EnvironmentId,
        _source_subscription_id: SourceSubscriptionId,
        _fact_occurred_at: chrono::DateTime<Utc>,
    ) -> Result<Option<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        Ok(None)
    }

    async fn list_revisions(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _source_environment_id: EnvironmentId,
        _source_subscription_id: SourceSubscriptionId,
        _limit: usize,
    ) -> Result<Vec<AcceptedPullRequestPreviewPolicyRevision>, RepositoryError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.list.clone())
    }
}

#[derive(Default)]
struct ScriptedPreviewRepository {
    preview: Option<PullRequestPreview>,
    calls: AtomicUsize,
}

impl ScriptedPreviewRepository {
    fn new(preview: Option<PullRequestPreview>) -> Self {
        Self {
            preview,
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl IPullRequestPreviewProjectionRepository for ScriptedPreviewRepository {
    async fn find_receipt(
        &self,
        _organization_id: OrganizationId,
        _source_pull_request_change_id: SourcePullRequestChangeId,
    ) -> Result<Option<PullRequestPreviewProjectionReceipt>, RepositoryError> {
        Ok(None)
    }

    async fn find_preview(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _source_environment_id: EnvironmentId,
        _source_subscription_id: SourceSubscriptionId,
        _pull_request_id: u64,
    ) -> Result<Option<PullRequestPreview>, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.preview.clone())
    }

    async fn commit_projection(
        &self,
        _write: CommitPullRequestPreviewProjection,
    ) -> Result<IdempotentWrite<PullRequestPreviewProjectionReceipt>, RepositoryError> {
        Err(RepositoryError::Storage("unexpected Preview write".into()))
    }
}

struct ScriptedAuthorization {
    allowed: bool,
    actions: Mutex<Vec<DeveloperWorkflowAction>>,
}

impl ScriptedAuthorization {
    fn new(allowed: bool) -> Self {
        Self {
            allowed,
            actions: Mutex::new(Vec::new()),
        }
    }

    fn actions(&self) -> Vec<DeveloperWorkflowAction> {
        self.actions.lock().expect("actions lock").clone()
    }
}

#[async_trait]
impl IDeveloperWorkflowAuthorizationPort for ScriptedAuthorization {
    async fn is_environment_action_allowed(
        &self,
        access: DeveloperWorkflowEnvironmentAccess,
    ) -> Result<bool, RepositoryError> {
        access.validate().map_err(RepositoryError::Forbidden)?;
        self.actions
            .lock()
            .expect("actions lock")
            .push(access.action);
        Ok(self.allowed)
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
