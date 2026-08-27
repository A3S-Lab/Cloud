use super::{
    DeveloperWorkflowAction, DeveloperWorkflowEnvironmentAccess,
    GetAcceptedWorkloadProfileRevision, GetAcceptedWorkloadProfileRevisionHandler,
    GetCurrentAcceptedWorkloadProfileRevision, GetCurrentAcceptedWorkloadProfileRevisionHandler,
    IDeveloperWorkflowAuthorizationPort, ListAcceptedWorkloadProfileRevisions,
    ListAcceptedWorkloadProfileRevisionsHandler, WorkloadProfileQueryService,
    MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
};
use crate::modules::developer_workflows::domain::{
    AcceptWorkloadProfileRevisionWrite, AcceptedBuildPlan, AcceptedBuildPlanContract,
    AcceptedWorkloadProfileRevision, BuildPlanProposal, IWorkloadProfileRepository,
    WorkloadProfileContract,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId,
    RepositoryError, SourceRevisionId, WorkloadProfileId, WorkloadProfileRevisionId,
};
use a3s_boot::{CqrsContext, ModuleRef, QueryHandler};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const BUILD_PLAN_FIXTURE: &str = include_str!("../../../../../../contracts/p0.1/build-plan.acl");
const WORKLOAD_PROFILE_FIXTURE: &str =
    include_str!("../../../../../../contracts/p0.2/workload-profile.acl");
const FIXTURE_BUILD_PLAN_ID: &str = "018f0f70-0000-7000-8000-000000000002";
const FIXTURE_BUILD_PLAN_DIGEST: &str =
    "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const FIXTURE_SOURCE_REVISION_ID: &str = "018f0f70-0000-7000-8000-000000000001";

#[tokio::test]
async fn workload_profile_queries_share_one_authorized_revision_authority() {
    let fixture = Fixture::new();
    let repository = Arc::new(ScriptedWorkloadProfileRepository::new(
        Some(fixture.second.clone()),
        Some(fixture.first.clone()),
        vec![fixture.first.clone(), fixture.second.clone()],
    ));
    let authorization = Arc::new(ScriptedAuthorization::new(true));
    let queries = Arc::new(WorkloadProfileQueryService::new(
        repository.clone(),
        authorization.clone(),
    ));
    let current = GetCurrentAcceptedWorkloadProfileRevisionHandler::new(Arc::clone(&queries));
    let exact = GetAcceptedWorkloadProfileRevisionHandler::new(Arc::clone(&queries));
    let list = ListAcceptedWorkloadProfileRevisionsHandler::new(queries);

    let current_revision = current
        .execute(fixture.current_query(), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("current query Boot result")
        .expect("current WorkloadProfile revision");
    assert_eq!(current_revision, fixture.second);

    let exact_revision = exact
        .execute(
            GetAcceptedWorkloadProfileRevision {
                workload_profile_revision_id: fixture.first.id,
                ..fixture.exact_query()
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("exact query Boot result")
        .expect("exact WorkloadProfile revision");
    assert_eq!(exact_revision, fixture.first);

    let revisions = list
        .execute(fixture.list_query(50), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("list query Boot result")
        .expect("WorkloadProfile revision history");
    assert_eq!(revisions, vec![fixture.first, fixture.second]);
    assert_eq!(authorization.calls(), 3);
    assert_eq!(
        authorization.actions(),
        vec![DeveloperWorkflowAction::ReadWorkloadProfile; 3]
    );
    assert_eq!(repository.current_calls.load(Ordering::SeqCst), 1);
    assert_eq!(repository.revision_calls.load(Ordering::SeqCst), 1);
    assert_eq!(repository.list_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workload_profile_queries_authorize_before_private_input_validation() {
    let fixture = Fixture::new();
    let repository = Arc::new(ScriptedWorkloadProfileRepository::default());
    let denied_authorization = Arc::new(ScriptedAuthorization::new(false));
    let denied = ListAcceptedWorkloadProfileRevisionsHandler::new(Arc::new(
        WorkloadProfileQueryService::new(repository.clone(), denied_authorization.clone()),
    ));
    let denied_error = denied
        .execute(
            ListAcceptedWorkloadProfileRevisions {
                workload_profile_id: WorkloadProfileId::from_uuid(Uuid::nil()),
                limit: 0,
                ..fixture.list_query(1)
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("denied query Boot result")
        .expect_err("denied query must be concealed");
    assert!(matches!(denied_error, ApplicationError::NotFound(_)));
    assert_eq!(denied_authorization.calls(), 1);
    assert_eq!(repository.list_calls.load(Ordering::SeqCst), 0);

    let allowed = ListAcceptedWorkloadProfileRevisionsHandler::new(Arc::new(
        WorkloadProfileQueryService::new(
            repository.clone(),
            Arc::new(ScriptedAuthorization::new(true)),
        ),
    ));
    for limit in [0, MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT + 1] {
        let error = allowed
            .execute(
                fixture.list_query(limit),
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("invalid query Boot result")
            .expect_err("invalid page bound");
        assert!(matches!(error, ApplicationError::Invalid(_)));
    }
    let nil_error = allowed
        .execute(
            ListAcceptedWorkloadProfileRevisions {
                workload_profile_id: WorkloadProfileId::from_uuid(Uuid::nil()),
                ..fixture.list_query(1)
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await
        .expect("nil query Boot result")
        .expect_err("nil WorkloadProfile identity");
    assert!(matches!(nil_error, ApplicationError::Invalid(_)));
    assert_eq!(repository.list_calls.load(Ordering::SeqCst), 0);

    let missing_repository = Arc::new(ScriptedWorkloadProfileRepository::default());
    let missing = ListAcceptedWorkloadProfileRevisionsHandler::new(Arc::new(
        WorkloadProfileQueryService::new(
            missing_repository.clone(),
            Arc::new(ScriptedAuthorization::new(true)),
        ),
    ));
    let missing_error = missing
        .execute(fixture.list_query(1), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("missing query Boot result")
        .expect_err("unknown WorkloadProfile history must not look empty");
    assert!(matches!(missing_error, ApplicationError::NotFound(_)));
    assert_eq!(missing_repository.list_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn workload_profile_queries_reject_repository_scope_order_and_page_drift() {
    let fixture = Fixture::new();
    let mut wrong_scope = fixture.second.clone();
    wrong_scope.environment_id = EnvironmentId::new();
    let current = GetCurrentAcceptedWorkloadProfileRevisionHandler::new(Arc::new(
        WorkloadProfileQueryService::new(
            Arc::new(ScriptedWorkloadProfileRepository::new(
                Some(wrong_scope),
                None,
                Vec::new(),
            )),
            Arc::new(ScriptedAuthorization::new(true)),
        ),
    ));
    let current_error = current
        .execute(fixture.current_query(), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("drifted current query Boot result")
        .expect_err("cross-scope current revision must fail closed");
    assert!(matches!(current_error, ApplicationError::Internal(_)));

    let non_canonical = ListAcceptedWorkloadProfileRevisionsHandler::new(Arc::new(
        WorkloadProfileQueryService::new(
            Arc::new(ScriptedWorkloadProfileRepository::new(
                None,
                None,
                vec![fixture.second.clone(), fixture.first.clone()],
            )),
            Arc::new(ScriptedAuthorization::new(true)),
        ),
    ));
    let order_error = non_canonical
        .execute(fixture.list_query(2), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("non-canonical list Boot result")
        .expect_err("non-canonical revision page must fail closed");
    assert!(matches!(order_error, ApplicationError::Internal(_)));

    let over_bound = ListAcceptedWorkloadProfileRevisionsHandler::new(Arc::new(
        WorkloadProfileQueryService::new(
            Arc::new(ScriptedWorkloadProfileRepository::new(
                None,
                None,
                vec![fixture.first.clone(), fixture.second.clone()],
            )),
            Arc::new(ScriptedAuthorization::new(true)),
        ),
    ));
    let bound_error = over_bound
        .execute(fixture.list_query(1), CqrsContext::new(ModuleRef::new()))
        .await
        .expect("over-bound list Boot result")
        .expect_err("repository page overflow must fail closed");
    assert!(matches!(bound_error, ApplicationError::Internal(_)));
}

struct Fixture {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    principal_id: PrincipalId,
    first: AcceptedWorkloadProfileRevision,
    second: AcceptedWorkloadProfileRevision,
}

impl Fixture {
    fn new() -> Self {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let principal_id = PrincipalId::new();
        let source_revision_id = SourceRevisionId::new();
        let proposal = BuildPlanProposal::parse_acl(BUILD_PLAN_FIXTURE).expect("BuildPlan fixture");
        let plan = AcceptedBuildPlan::accept(
            organization_id,
            project_id,
            environment_id,
            AcceptedBuildPlanContract::from_proposal(source_revision_id, proposal)
                .expect("accepted BuildPlan contract"),
            principal_id,
            Utc::now() - Duration::seconds(5),
        )
        .expect("accepted BuildPlan");
        let first_contract = profile_contract(&plan, "info");
        let first = AcceptedWorkloadProfileRevision::accept(
            &plan,
            first_contract,
            1,
            principal_id,
            plan.accepted_at + Duration::milliseconds(1),
        )
        .expect("first accepted WorkloadProfile revision");
        let second = AcceptedWorkloadProfileRevision::accept(
            &plan,
            profile_contract(&plan, "debug"),
            2,
            principal_id,
            first.accepted_at + Duration::milliseconds(1),
        )
        .expect("second accepted WorkloadProfile revision");
        Self {
            organization_id,
            project_id,
            environment_id,
            principal_id,
            first,
            second,
        }
    }

    fn current_query(&self) -> GetCurrentAcceptedWorkloadProfileRevision {
        GetCurrentAcceptedWorkloadProfileRevision {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            workload_profile_id: self.first.profile_id,
            principal_id: self.principal_id,
        }
    }

    fn exact_query(&self) -> GetAcceptedWorkloadProfileRevision {
        GetAcceptedWorkloadProfileRevision {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            workload_profile_id: self.first.profile_id,
            workload_profile_revision_id: self.first.id,
            principal_id: self.principal_id,
        }
    }

    fn list_query(&self, limit: usize) -> ListAcceptedWorkloadProfileRevisions {
        ListAcceptedWorkloadProfileRevisions {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            workload_profile_id: self.first.profile_id,
            limit,
            principal_id: self.principal_id,
        }
    }
}

fn profile_contract(plan: &AcceptedBuildPlan, log_level: &str) -> WorkloadProfileContract {
    let acl = WORKLOAD_PROFILE_FIXTURE
        .replace(FIXTURE_BUILD_PLAN_ID, &plan.id.to_string())
        .replace(FIXTURE_BUILD_PLAN_DIGEST, plan.contract.digest().as_str())
        .replace(
            FIXTURE_SOURCE_REVISION_ID,
            &plan.source_revision_id.to_string(),
        )
        .replace("value = \"info\"", &format!("value = \"{log_level}\""));
    WorkloadProfileContract::parse_acl(&acl).expect("bound WorkloadProfile ACL")
}

#[derive(Default)]
struct ScriptedWorkloadProfileRepository {
    current: Option<AcceptedWorkloadProfileRevision>,
    revision: Option<AcceptedWorkloadProfileRevision>,
    listed: Vec<AcceptedWorkloadProfileRevision>,
    current_calls: AtomicUsize,
    revision_calls: AtomicUsize,
    list_calls: AtomicUsize,
}

impl ScriptedWorkloadProfileRepository {
    fn new(
        current: Option<AcceptedWorkloadProfileRevision>,
        revision: Option<AcceptedWorkloadProfileRevision>,
        listed: Vec<AcceptedWorkloadProfileRevision>,
    ) -> Self {
        Self {
            current,
            revision,
            listed,
            current_calls: AtomicUsize::new(0),
            revision_calls: AtomicUsize::new(0),
            list_calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl IWorkloadProfileRepository for ScriptedWorkloadProfileRepository {
    async fn replay_acceptance(
        &self,
        _idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError> {
        unreachable!("read authority never replays acceptance")
    }

    async fn accept(
        &self,
        _write: AcceptWorkloadProfileRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedWorkloadProfileRevision>, RepositoryError> {
        unreachable!("read authority never accepts revisions")
    }

    async fn find_revision(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _workload_profile_id: WorkloadProfileId,
        _workload_profile_revision_id: WorkloadProfileRevisionId,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError> {
        self.revision_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.revision.clone())
    }

    async fn find_current(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _workload_profile_id: WorkloadProfileId,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError> {
        self.current_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.current.clone())
    }

    async fn list_revisions(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
        _workload_profile_id: WorkloadProfileId,
        _limit: usize,
    ) -> Result<Vec<AcceptedWorkloadProfileRevision>, RepositoryError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.listed.clone())
    }
}

struct ScriptedAuthorization {
    allowed: bool,
    calls: AtomicUsize,
    actions: Mutex<Vec<DeveloperWorkflowAction>>,
}

impl ScriptedAuthorization {
    fn new(allowed: bool) -> Self {
        Self {
            allowed,
            calls: AtomicUsize::new(0),
            actions: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn actions(&self) -> Vec<DeveloperWorkflowAction> {
        self.actions
            .lock()
            .expect("authorization action lock")
            .clone()
    }
}

#[async_trait]
impl IDeveloperWorkflowAuthorizationPort for ScriptedAuthorization {
    async fn is_environment_action_allowed(
        &self,
        access: DeveloperWorkflowEnvironmentAccess,
    ) -> Result<bool, RepositoryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.actions
            .lock()
            .expect("authorization action lock")
            .push(access.action);
        Ok(self.allowed)
    }
}
