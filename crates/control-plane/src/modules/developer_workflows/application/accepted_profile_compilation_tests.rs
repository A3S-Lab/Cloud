use super::profile_compilation_tests::{plan_and_build, web_profile};
use super::{
    CompileAcceptedWorkloadProfile, CompileAcceptedWorkloadProfileHandler, CompiledWorkloadProfile,
    WorkloadProfileCompilationService,
};
use crate::modules::artifacts::application::ExternalSourceBuildOutcomeQueryService;
use crate::modules::artifacts::infrastructure::InMemoryBuildRunRepository;
use crate::modules::developer_workflows::domain::{
    AcceptBuildPlanWrite, AcceptWorkloadProfileRevisionWrite, AcceptedBuildPlan,
    AcceptedWorkloadProfileRevision, BuildPlanAccepted, IBuildPlanRepository,
    IWorkloadProfileRepository, WorkloadProfileContract, WorkloadProfileRevisionAccepted,
};
use crate::modules::developer_workflows::infrastructure::{
    ArtifactsWorkloadBuildOutcomeAdapter, ExecutionsScheduledTaskProfileAdapter,
    InMemoryBuildPlanRepository, InMemoryWorkloadProfileRepository, WorkloadsServiceProfileAdapter,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, PrincipalId, WorkloadProfileRevisionId,
};
use a3s_boot::{CqrsContext, ModuleRef, QueryHandler};
use chrono::Duration;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn exact_accepted_revision_compiles_through_the_production_acl_chain() {
    let fixture = fixture().await;

    let result = fixture
        .handler
        .execute(fixture.query, context())
        .await
        .expect("CQRS result")
        .expect("accepted profile compilation");

    assert_eq!(result.workload_profile_id, fixture.revision.profile_id);
    assert_eq!(result.workload_profile_revision_id, fixture.revision.id);
    assert_eq!(result.revision_number, fixture.revision.revision_number);
    let CompiledWorkloadProfile::Service(compiled) = result.compiled else {
        panic!("web profile must compile through Workloads Service admission");
    };
    assert_eq!(compiled.organization_id, fixture.query.organization_id);
    assert_eq!(compiled.project_id, fixture.query.project_id);
    assert_eq!(compiled.environment_id, fixture.query.environment_id);
    assert_eq!(compiled.build_plan_id, fixture.query.build_plan_id);
    assert_eq!(compiled.build_run_id, fixture.query.build_run_id);
    assert_eq!(compiled.profile_digest, *fixture.revision.contract.digest());
}

#[tokio::test]
async fn compilation_fails_closed_for_an_unverified_or_unrelated_build_run() {
    let fixture = fixture().await;
    let mut query = fixture.query;
    query.build_run_id = crate::modules::shared_kernel::domain::BuildRunId::new();

    let error = fixture
        .handler
        .execute(query, context())
        .await
        .expect("CQRS result")
        .expect_err("an unrelated BuildRun must not compile");

    assert_eq!(
        error,
        ApplicationError::NotFound("verified workload build outcome not found".into())
    );
}

#[tokio::test]
async fn compilation_rejects_invalid_or_cross_revision_identity_before_admission() {
    let fixture = fixture().await;
    let mut invalid = fixture.query;
    invalid.workload_profile_revision_id = WorkloadProfileRevisionId::from_uuid(Uuid::nil());
    let error = fixture
        .handler
        .execute(invalid, context())
        .await
        .expect("CQRS result")
        .expect_err("nil revision identity must be rejected");
    assert_eq!(
        error,
        ApplicationError::Invalid(
            "accepted workload profile compilation identity is invalid".into()
        )
    );

    let mut unrelated = fixture.query;
    unrelated.workload_profile_revision_id = WorkloadProfileRevisionId::new();
    let error = fixture
        .handler
        .execute(unrelated, context())
        .await
        .expect("CQRS result")
        .expect_err("another revision must remain concealed");
    assert_eq!(
        error,
        ApplicationError::NotFound(
            "accepted workload profile compilation authority not found".into()
        )
    );
}

struct CompilationFixture {
    handler: CompileAcceptedWorkloadProfileHandler,
    query: CompileAcceptedWorkloadProfile,
    revision: AcceptedWorkloadProfileRevision,
}

async fn fixture() -> CompilationFixture {
    let (build_plan, build_run) = plan_and_build();
    let profile = WorkloadProfileContract::bind(&build_plan, web_profile())
        .expect("accepted web profile contract");
    let revision = AcceptedWorkloadProfileRevision::accept(
        &build_plan,
        profile,
        1,
        PrincipalId::new(),
        build_plan.accepted_at + Duration::milliseconds(500),
    )
    .expect("accepted workload profile revision");

    let build_plans = Arc::new(InMemoryBuildPlanRepository::new());
    persist_build_plan(build_plans.as_ref(), &build_plan).await;
    let workload_profiles = Arc::new(InMemoryWorkloadProfileRepository::new());
    persist_workload_profile(workload_profiles.as_ref(), &build_plan, &revision).await;

    let builds = Arc::new(InMemoryBuildRunRepository::new());
    builds.seed_build(build_run.clone()).await;
    let owner_outcomes = Arc::new(ExternalSourceBuildOutcomeQueryService::new(builds));
    let build_plan_port: Arc<dyn IBuildPlanRepository> = build_plans.clone();
    let workload_profile_port: Arc<dyn IWorkloadProfileRepository> = workload_profiles;
    let compiler = Arc::new(WorkloadProfileCompilationService::new(
        Arc::new(ArtifactsWorkloadBuildOutcomeAdapter::new(
            owner_outcomes,
            Arc::clone(&build_plan_port),
        )),
        Arc::new(WorkloadsServiceProfileAdapter::new()),
        Arc::new(ExecutionsScheduledTaskProfileAdapter::new()),
    ));
    let query = CompileAcceptedWorkloadProfile {
        organization_id: build_plan.organization_id,
        project_id: build_plan.project_id,
        environment_id: build_plan.environment_id,
        build_plan_id: build_plan.id,
        workload_profile_id: revision.profile_id,
        workload_profile_revision_id: revision.id,
        build_run_id: build_run.id,
    };

    CompilationFixture {
        handler: CompileAcceptedWorkloadProfileHandler::new(
            build_plan_port,
            workload_profile_port,
            compiler,
        ),
        query,
        revision,
    }
}

async fn persist_build_plan(repository: &InMemoryBuildPlanRepository, plan: &AcceptedBuildPlan) {
    let request_id = Uuid::now_v7();
    let event = BuildPlanAccepted::envelope(plan, request_id).expect("BuildPlan event");
    repository
        .accept(AcceptBuildPlanWrite {
            plan: plan.clone(),
            event,
            actor_principal_id: plan.accepted_by,
            request_id,
            idempotency: IdempotencyRequest::new(
                "tests/developer-workflows/build-plans",
                "accepted-plan",
                plan.contract.digest().as_str().as_bytes(),
            )
            .expect("BuildPlan idempotency"),
        })
        .await
        .expect("persist BuildPlan");
}

async fn persist_workload_profile(
    repository: &InMemoryWorkloadProfileRepository,
    build_plan: &AcceptedBuildPlan,
    revision: &AcceptedWorkloadProfileRevision,
) {
    let request_id = Uuid::now_v7();
    let event = WorkloadProfileRevisionAccepted::envelope(revision, request_id)
        .expect("workload profile event");
    repository
        .accept(AcceptWorkloadProfileRevisionWrite {
            revision: revision.clone(),
            build_plan: build_plan.clone(),
            expected_previous_revision_id: None,
            event,
            actor_principal_id: revision.accepted_by,
            request_id,
            idempotency: IdempotencyRequest::new(
                "tests/developer-workflows/workload-profiles",
                "accepted-profile",
                revision.contract.digest().as_str().as_bytes(),
            )
            .expect("workload profile idempotency"),
        })
        .await
        .expect("persist workload profile");
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
