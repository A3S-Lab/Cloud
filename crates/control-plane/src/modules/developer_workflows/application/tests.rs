use super::{
    AcceptBuildPlan, AcceptBuildPlanHandler, BuildPlanQueryService,
    BuildPlanSourceRevisionEvidence, DeveloperWorkflowAccess, DeveloperWorkflowEnvironmentScope,
    GetAcceptedBuildPlan, GetAcceptedBuildPlanHandler, IBuildPlanSourceRevisionPort,
    IDeveloperWorkflowEnvironmentPort, ListAcceptedBuildPlans, ListAcceptedBuildPlansHandler,
    MAXIMUM_BUILD_PLAN_LIST_LIMIT,
};
use crate::modules::developer_workflows::domain::{
    AcceptBuildPlanWrite, AcceptedBuildPlan, BuildPlanProposal, IBuildPlanRepository,
};
use crate::modules::developer_workflows::infrastructure::InMemoryBuildPlanRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    BuildPlanId, EnvironmentId, GitCommitSha, IdempotencyRequest, IdempotentWrite, OrganizationId,
    PrincipalId, ProjectId, RepositoryError, Sha256Digest, SourceRevisionId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

const BUILD_PLAN_FIXTURE: &str = include_str!("../../../../../../contracts/p0.1/build-plan.acl");

#[tokio::test]
async fn acceptance_is_authorized_exact_and_replay_safe() {
    let fixture = Fixture::new();
    let repository = Arc::new(InMemoryBuildPlanRepository::new());
    let source = Arc::new(FakeSourcePort::new(Some(fixture.evidence())));
    let handler =
        AcceptBuildPlanHandler::new(repository.clone(), source.clone(), environments(true));

    let first = handler
        .execute(fixture.command("accept-1"), context())
        .await
        .expect("Boot result")
        .expect("acceptance");
    assert!(!first.replayed);
    assert_eq!(first.plan.source_revision_id, fixture.source_revision_id);
    assert_eq!(source.calls(), 1);

    let replay = handler
        .execute(fixture.command("accept-1"), context())
        .await
        .expect("Boot replay result")
        .expect("acceptance replay");
    assert!(replay.replayed);
    assert_eq!(replay.plan, first.plan);
    assert_eq!(source.calls(), 1, "replay must not re-resolve Sources");
    assert_eq!(repository.outbox_events().await.len(), 1);

    let found = repository
        .find_for_source_root(
            fixture.organization_id,
            fixture.project_id,
            fixture.environment_id,
            fixture.source_revision_id,
            ".",
        )
        .await
        .expect("repository lookup")
        .expect("accepted plan");
    assert_eq!(found, first.plan);
}

#[tokio::test]
async fn independent_idempotency_keys_converge_on_one_natural_acceptance() {
    let fixture = Fixture::new();
    let repository = Arc::new(InMemoryBuildPlanRepository::new());
    let source = Arc::new(FakeSourcePort::new(Some(fixture.evidence())));
    let handler = AcceptBuildPlanHandler::new(repository.clone(), source, environments(true));

    let first = handler
        .execute(fixture.command("natural-1"), context())
        .await
        .expect("first Boot result")
        .expect("first acceptance");
    let adopted = handler
        .execute(fixture.command("natural-2"), context())
        .await
        .expect("second Boot result")
        .expect("natural adoption");

    assert!(!first.replayed);
    assert!(adopted.replayed);
    assert_eq!(adopted.plan, first.plan);
    assert_eq!(repository.outbox_events().await.len(), 1);
    assert_eq!(
        repository
            .list_for_source(
                fixture.organization_id,
                fixture.project_id,
                fixture.environment_id,
                fixture.source_revision_id,
                10,
            )
            .await
            .expect("source plans")
            .len(),
        1
    );
}

#[tokio::test]
async fn another_authorized_actor_can_adopt_and_replay_the_existing_acceptance() {
    let fixture = Fixture::new();
    let repository = Arc::new(InMemoryBuildPlanRepository::new());
    let source = Arc::new(FakeSourcePort::new(Some(fixture.evidence())));
    let handler =
        AcceptBuildPlanHandler::new(repository.clone(), source.clone(), environments(true));
    let first = handler
        .execute(fixture.command("first-actor"), context())
        .await
        .expect("first Boot result")
        .expect("first acceptance");

    let second_actor = PrincipalId::new();
    let mut adoption = fixture.command("second-actor");
    adoption.actor_principal_id = second_actor;
    let adopted = handler
        .execute(adoption.clone(), context())
        .await
        .expect("adoption Boot result")
        .expect("natural adoption");
    let replayed = handler
        .execute(adoption, context())
        .await
        .expect("adoption replay Boot result")
        .expect("natural adoption replay");

    assert!(adopted.replayed);
    assert!(replayed.replayed);
    assert_eq!(adopted.plan, first.plan);
    assert_eq!(replayed.plan, first.plan);
    assert_eq!(first.plan.accepted_by, fixture.actor_principal_id);
    assert_ne!(first.plan.accepted_by, second_actor);
    assert_eq!(repository.outbox_events().await.len(), 1);
    assert_eq!(
        source.calls(),
        2,
        "only new idempotency keys resolve Sources"
    );
}

#[tokio::test]
async fn source_evidence_drift_fails_before_persistence() {
    let fixture = Fixture::new();
    let repository = Arc::new(InMemoryBuildPlanRepository::new());
    let mut evidence = fixture.evidence();
    evidence.recipe_digest = digest('f');
    let source = Arc::new(FakeSourcePort::new(Some(evidence)));
    let handler = AcceptBuildPlanHandler::new(repository.clone(), source, environments(true));

    let error = handler
        .execute(fixture.command("drift"), context())
        .await
        .expect("Boot result")
        .expect_err("drift must fail");
    assert!(matches!(error, ApplicationError::Conflict(_)));
    assert!(repository.outbox_events().await.is_empty());
    assert!(repository
        .list_for_source(
            fixture.organization_id,
            fixture.project_id,
            fixture.environment_id,
            fixture.source_revision_id,
            10,
        )
        .await
        .expect("source plans")
        .is_empty());
}

#[tokio::test]
async fn environment_access_precedes_source_resolution_and_replay() {
    let fixture = Fixture::new();
    let repository = Arc::new(InMemoryBuildPlanRepository::new());
    let source = Arc::new(FakeSourcePort::new(Some(fixture.evidence())));
    let handler = AcceptBuildPlanHandler::new(repository, source.clone(), environments(false));
    let mut command = fixture.command("forbidden");
    command.proposal_acl = "not an ACL document".into();

    let error = handler
        .execute(command, context())
        .await
        .expect("Boot result")
        .expect_err("unauthorized acceptance must fail");
    assert!(matches!(error, ApplicationError::NotFound(_)));
    assert_eq!(source.calls(), 0);
}

#[tokio::test]
async fn accepted_build_plan_queries_share_one_authorized_scope() {
    let fixture = Fixture::new();
    let repository = Arc::new(InMemoryBuildPlanRepository::new());
    let source = Arc::new(FakeSourcePort::new(Some(fixture.evidence())));
    let acceptance = AcceptBuildPlanHandler::new(repository.clone(), source, environments(true));
    let accepted = acceptance
        .execute(fixture.command("query-authority"), context())
        .await
        .expect("acceptance Boot result")
        .expect("accepted BuildPlan")
        .plan;

    let environment_authority = environments(true);
    let queries = Arc::new(BuildPlanQueryService::new(
        repository,
        environment_authority.clone(),
    ));
    let get = GetAcceptedBuildPlanHandler::new(Arc::clone(&queries));
    let list = ListAcceptedBuildPlansHandler::new(queries);

    let found = get
        .execute(
            GetAcceptedBuildPlan {
                organization_id: fixture.organization_id,
                project_id: fixture.project_id,
                environment_id: fixture.environment_id,
                build_plan_id: accepted.id,
                access: fixture.access(),
            },
            context(),
        )
        .await
        .expect("get Boot result")
        .expect("get accepted BuildPlan");
    let page = list
        .execute(
            ListAcceptedBuildPlans {
                organization_id: fixture.organization_id,
                project_id: fixture.project_id,
                environment_id: fixture.environment_id,
                source_revision_id: fixture.source_revision_id,
                limit: 50,
                access: fixture.access(),
            },
            context(),
        )
        .await
        .expect("list Boot result")
        .expect("list accepted BuildPlans");

    assert_eq!(found, accepted);
    assert_eq!(page, vec![accepted]);
    assert_eq!(environment_authority.calls(), 2);
}

#[tokio::test]
async fn accepted_build_plan_queries_authorize_before_private_input_validation() {
    let fixture = Fixture::new();
    let denied_environment = environments(false);
    let denied = ListAcceptedBuildPlansHandler::new(Arc::new(BuildPlanQueryService::new(
        Arc::new(InMemoryBuildPlanRepository::new()),
        denied_environment.clone(),
    )));
    let denied_error = denied
        .execute(
            ListAcceptedBuildPlans {
                organization_id: fixture.organization_id,
                project_id: fixture.project_id,
                environment_id: fixture.environment_id,
                source_revision_id: SourceRevisionId::from_uuid(Uuid::nil()),
                limit: 0,
                access: fixture.access(),
            },
            context(),
        )
        .await
        .expect("denied Boot result")
        .expect_err("denied query must be concealed");
    assert!(matches!(denied_error, ApplicationError::NotFound(_)));
    assert_eq!(denied_environment.calls(), 1);

    let allowed = ListAcceptedBuildPlansHandler::new(Arc::new(BuildPlanQueryService::new(
        Arc::new(InMemoryBuildPlanRepository::new()),
        environments(true),
    )));
    for limit in [0, MAXIMUM_BUILD_PLAN_LIST_LIMIT + 1] {
        let error = allowed
            .execute(
                ListAcceptedBuildPlans {
                    organization_id: fixture.organization_id,
                    project_id: fixture.project_id,
                    environment_id: fixture.environment_id,
                    source_revision_id: fixture.source_revision_id,
                    limit,
                    access: fixture.access(),
                },
                context(),
            )
            .await
            .expect("bounded Boot result")
            .expect_err("unbounded query must fail");
        assert!(matches!(error, ApplicationError::Invalid(_)));
    }
}

#[tokio::test]
async fn accepted_build_plan_queries_reject_repository_scope_and_page_drift() {
    let fixture = Fixture::new();
    let repository = Arc::new(InMemoryBuildPlanRepository::new());
    let acceptance = AcceptBuildPlanHandler::new(
        repository,
        Arc::new(FakeSourcePort::new(Some(fixture.evidence()))),
        environments(true),
    );
    let accepted = acceptance
        .execute(fixture.command("query-drift"), context())
        .await
        .expect("acceptance Boot result")
        .expect("accepted BuildPlan")
        .plan;

    let mut wrong_scope = accepted.clone();
    wrong_scope.project_id = ProjectId::new();
    let get = GetAcceptedBuildPlanHandler::new(Arc::new(BuildPlanQueryService::new(
        Arc::new(ScriptedBuildPlanRepository {
            found: Some(wrong_scope),
            listed: Vec::new(),
        }),
        environments(true),
    )));
    let get_error = get
        .execute(
            GetAcceptedBuildPlan {
                organization_id: fixture.organization_id,
                project_id: fixture.project_id,
                environment_id: fixture.environment_id,
                build_plan_id: accepted.id,
                access: fixture.access(),
            },
            context(),
        )
        .await
        .expect("get Boot result")
        .expect_err("cross-scope repository result must fail closed");
    assert!(matches!(get_error, ApplicationError::Internal(_)));

    let list = ListAcceptedBuildPlansHandler::new(Arc::new(BuildPlanQueryService::new(
        Arc::new(ScriptedBuildPlanRepository {
            found: None,
            listed: vec![accepted.clone(), accepted],
        }),
        environments(true),
    )));
    let list_error = list
        .execute(
            ListAcceptedBuildPlans {
                organization_id: fixture.organization_id,
                project_id: fixture.project_id,
                environment_id: fixture.environment_id,
                source_revision_id: fixture.source_revision_id,
                limit: 2,
                access: fixture.access(),
            },
            context(),
        )
        .await
        .expect("list Boot result")
        .expect_err("non-canonical repository page must fail closed");
    assert!(matches!(list_error, ApplicationError::Internal(_)));
}

struct FakeSourcePort {
    evidence: Option<BuildPlanSourceRevisionEvidence>,
    calls: AtomicUsize,
}

impl FakeSourcePort {
    fn new(evidence: Option<BuildPlanSourceRevisionEvidence>) -> Self {
        Self {
            evidence,
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IBuildPlanSourceRevisionPort for FakeSourcePort {
    async fn resolve(
        &self,
        organization_id: OrganizationId,
        source_revision_id: SourceRevisionId,
    ) -> Result<Option<BuildPlanSourceRevisionEvidence>, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.evidence.clone().filter(|evidence| {
            evidence.organization_id == organization_id
                && evidence.source_revision_id == source_revision_id
        }))
    }
}

struct FakeEnvironmentPort {
    exists: bool,
    calls: AtomicUsize,
}

impl FakeEnvironmentPort {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IDeveloperWorkflowEnvironmentPort for FakeEnvironmentPort {
    async fn environment_exists(
        &self,
        scope: DeveloperWorkflowEnvironmentScope,
    ) -> Result<bool, RepositoryError> {
        scope.validate().map_err(RepositoryError::Forbidden)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.exists)
    }
}

fn environments(exists: bool) -> Arc<FakeEnvironmentPort> {
    Arc::new(FakeEnvironmentPort {
        exists,
        calls: AtomicUsize::new(0),
    })
}

struct ScriptedBuildPlanRepository {
    found: Option<AcceptedBuildPlan>,
    listed: Vec<AcceptedBuildPlan>,
}

#[async_trait]
impl IBuildPlanRepository for ScriptedBuildPlanRepository {
    async fn replay_acceptance(
        &self,
        _idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError> {
        Ok(None)
    }

    async fn accept(
        &self,
        _write: AcceptBuildPlanWrite,
    ) -> Result<IdempotentWrite<AcceptedBuildPlan>, RepositoryError> {
        Err(RepositoryError::Storage(
            "scripted BuildPlan repository is read-only".into(),
        ))
    }

    async fn find(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _build_plan_id: BuildPlanId,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError> {
        Ok(self.found.clone())
    }

    async fn find_for_source_root(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _source_revision_id: SourceRevisionId,
        _project_root: &str,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError> {
        Ok(None)
    }

    async fn list_for_source(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _source_revision_id: SourceRevisionId,
        _limit: usize,
    ) -> Result<Vec<AcceptedBuildPlan>, RepositoryError> {
        Ok(self.listed.clone())
    }
}

struct Fixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    actor_principal_id: PrincipalId,
}

impl Fixture {
    fn new() -> Self {
        Self {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            source_revision_id: SourceRevisionId::new(),
            actor_principal_id: PrincipalId::new(),
        }
    }

    fn evidence(&self) -> BuildPlanSourceRevisionEvidence {
        let proposal = BuildPlanProposal::parse_acl(BUILD_PLAN_FIXTURE).expect("proposal fixture");
        BuildPlanSourceRevisionEvidence {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            source_revision_id: self.source_revision_id,
            source_identity_digest: proposal.spec().source.source_identity_digest.clone(),
            commit_sha: GitCommitSha::parse(proposal.spec().source.commit_sha.as_str())
                .expect("commit SHA"),
            recipe_digest: Sha256Digest::parse(
                proposal.spec().recipe.digest().expect("recipe digest"),
            )
            .expect("typed recipe digest"),
            accepted_at: chrono::DateTime::from_timestamp_micros(
                (Utc::now() - Duration::seconds(1)).timestamp_micros(),
            )
            .expect("canonical source acceptance time"),
        }
    }

    fn command(&self, idempotency_key: &str) -> AcceptBuildPlan {
        AcceptBuildPlan {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            source_revision_id: self.source_revision_id,
            proposal_acl: BUILD_PLAN_FIXTURE.into(),
            access: self.access(),
            actor_principal_id: self.actor_principal_id,
            idempotency_key: idempotency_key.into(),
            request_id: Uuid::now_v7(),
        }
    }

    fn access(&self) -> DeveloperWorkflowAccess {
        DeveloperWorkflowAccess::organization_wide()
    }
}

fn digest(seed: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", seed.to_string().repeat(64))).expect("digest")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
