use super::{AcceptWorkloadProfile, AcceptWorkloadProfileHandler};
use crate::modules::developer_workflows::domain::{
    AcceptBuildPlanWrite, AcceptWorkloadProfileRevisionWrite, AcceptedBuildPlan,
    AcceptedBuildPlanContract, AcceptedWorkloadProfileRevision, BuildPlanAccepted,
    BuildPlanProposal, IBuildPlanRepository, IWorkloadProfileRepository, WorkloadProfileContract,
    WorkloadProfileKind, WorkloadProfileResources, WorkloadProfileRevisionAccepted,
    WorkloadProfileSpec,
};
use crate::modules::developer_workflows::infrastructure::{
    InMemoryBuildPlanRepository, InMemoryWorkloadProfileRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    SourceRevisionId,
};
use crate::modules::workloads::domain::entities::{HttpHealthCheck, ServicePort, ServiceProcess};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

const BUILD_PLAN_FIXTURE: &str = include_str!("../../../../../../contracts/p0.1/build-plan.acl");

#[tokio::test]
async fn acceptance_is_revisioned_replay_safe_and_queryable() {
    let (fixture, profiles, handler) = setup().await;
    let initial = fixture.contract(250);
    let first_command = fixture.command(&initial, fixture.actor, "accept-1");
    let first = execute(&handler, first_command.clone()).await;
    assert!(!first.replayed);
    assert_eq!(first.revision.revision_number, 1);

    let replay = execute(&handler, first_command).await;
    assert!(replay.replayed);
    assert_eq!(replay.revision, first.revision);

    let adopted = execute(
        &handler,
        fixture.command(&initial, fixture.actor, "accept-same-content"),
    )
    .await;
    assert!(adopted.replayed);
    assert_eq!(adopted.revision, first.revision);

    let changed = fixture.contract(500);
    let second = execute(
        &handler,
        fixture.command(&changed, fixture.actor, "accept-2"),
    )
    .await;
    assert!(!second.replayed);
    assert_eq!(second.revision.revision_number, 2);
    assert_eq!(second.revision.profile_id, first.revision.profile_id);
    assert_ne!(second.revision.id, first.revision.id);

    assert_eq!(
        profiles
            .find_current(
                fixture.plan.organization_id,
                fixture.plan.project_id,
                fixture.plan.environment_id,
                first.revision.profile_id,
            )
            .await
            .expect("current revision")
            .expect("current workload profile"),
        second.revision
    );
    assert_eq!(
        profiles
            .list_revisions(
                fixture.plan.organization_id,
                fixture.plan.project_id,
                fixture.plan.environment_id,
                first.revision.profile_id,
                10,
            )
            .await
            .expect("revision history"),
        vec![first.revision, second.revision]
    );
    assert_eq!(profiles.outbox_events().await.len(), 2);
}

#[tokio::test]
async fn another_actor_creates_an_auditable_revision_and_can_replay_it() {
    let (fixture, profiles, handler) = setup().await;
    let contract = fixture.contract(250);
    let first = execute(
        &handler,
        fixture.command(&contract, fixture.actor, "first-actor"),
    )
    .await;

    let other_actor = PrincipalId::new();
    let other_command = fixture.command(&contract, other_actor, "other-actor");
    let accepted = execute(&handler, other_command.clone()).await;
    let replayed = execute(&handler, other_command).await;
    let adopted = execute(
        &handler,
        fixture.command(&contract, other_actor, "other-actor-same-content"),
    )
    .await;

    assert_eq!(first.revision.revision_number, 1);
    assert_eq!(accepted.revision.revision_number, 2);
    assert_eq!(accepted.revision.accepted_by, other_actor);
    assert!(!accepted.replayed);
    assert!(replayed.replayed);
    assert!(adopted.replayed);
    assert_eq!(replayed.revision, accepted.revision);
    assert_eq!(adopted.revision, accepted.revision);
    assert_eq!(profiles.outbox_events().await.len(), 2);
}

#[tokio::test]
async fn authorization_precedes_acl_parsing_plan_lookup_and_replay() {
    let (fixture, _profiles, handler) = setup().await;
    let contract = fixture.contract(250);
    let original = fixture.command(&contract, fixture.actor, "authorization-order");
    execute(&handler, original.clone()).await;

    let mut forbidden = original;
    forbidden.profile_acl = "not an ACL document".into();
    forbidden.resource_access = ResourceAccessEvaluator::restricted([]);
    let error = handler
        .execute(forbidden, context())
        .await
        .expect("Boot result")
        .expect_err("forbidden acceptance");
    assert!(matches!(error, ApplicationError::NotFound(_)));
}

#[tokio::test]
async fn embedded_plan_drift_and_idempotency_reuse_are_rejected() {
    let (fixture, _profiles, handler) = setup().await;
    let initial = fixture.contract(250);
    execute(
        &handler,
        fixture.command(&initial, fixture.actor, "same-key"),
    )
    .await;

    let changed = fixture.contract(500);
    let conflict = handler
        .execute(
            fixture.command(&changed, fixture.actor, "same-key"),
            context(),
        )
        .await
        .expect("Boot result")
        .expect_err("idempotency conflict");
    assert!(matches!(conflict, ApplicationError::Conflict(_)));

    let mut drifted = fixture.command(&initial, fixture.actor, "drifted-plan");
    drifted.profile_acl = drifted.profile_acl.replace(
        &fixture.plan.id.to_string(),
        &crate::modules::shared_kernel::domain::BuildPlanId::new().to_string(),
    );
    let invalid = handler
        .execute(drifted, context())
        .await
        .expect("Boot result")
        .expect_err("embedded BuildPlan drift");
    assert!(matches!(invalid, ApplicationError::Invalid(_)));
}

#[tokio::test]
async fn stale_competing_revision_write_conflicts() {
    let (fixture, profiles, handler) = setup().await;
    let initial = fixture.contract(250);
    let first = execute(
        &handler,
        fixture.command(&initial, fixture.actor, "base-revision"),
    )
    .await;
    let left = AcceptedWorkloadProfileRevision::accept(
        &fixture.plan,
        fixture.contract(500),
        2,
        fixture.actor,
        Utc::now(),
    )
    .expect("left revision");
    let right = AcceptedWorkloadProfileRevision::accept(
        &fixture.plan,
        fixture.contract(750),
        2,
        fixture.actor,
        Utc::now(),
    )
    .expect("right revision");

    profiles
        .accept(revision_write(
            &fixture.plan,
            left,
            Some(first.revision.id),
            "left-write",
        ))
        .await
        .expect("left acceptance");
    let error = profiles
        .accept(revision_write(
            &fixture.plan,
            right,
            Some(first.revision.id),
            "right-write",
        ))
        .await
        .expect_err("stale write conflict");
    assert!(matches!(error, RepositoryError::Conflict(_)));
    assert_eq!(profiles.outbox_events().await.len(), 2);
}

struct Fixture {
    plan: AcceptedBuildPlan,
    actor: PrincipalId,
}

impl Fixture {
    fn new() -> Self {
        let source_revision_id = SourceRevisionId::new();
        let proposal = BuildPlanProposal::parse_acl(BUILD_PLAN_FIXTURE).expect("BuildPlan fixture");
        let contract = AcceptedBuildPlanContract::from_proposal(source_revision_id, proposal)
            .expect("accepted BuildPlan contract");
        let actor = PrincipalId::new();
        let plan = AcceptedBuildPlan::accept(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            contract,
            actor,
            Utc.with_ymd_and_hms(2026, 8, 24, 0, 0, 0)
                .single()
                .expect("timestamp"),
        )
        .expect("accepted BuildPlan");
        Self { plan, actor }
    }

    fn contract(&self, cpu_millis: u64) -> WorkloadProfileContract {
        WorkloadProfileContract::bind(&self.plan, web_profile(cpu_millis))
            .expect("workload profile contract")
    }

    fn command(
        &self,
        contract: &WorkloadProfileContract,
        actor_principal_id: PrincipalId,
        idempotency_key: &str,
    ) -> AcceptWorkloadProfile {
        AcceptWorkloadProfile {
            organization_id: self.plan.organization_id,
            project_id: self.plan.project_id,
            environment_id: self.plan.environment_id,
            build_plan_id: self.plan.id,
            profile_acl: contract.canonical_acl().into(),
            actor_principal_id,
            resource_access: ResourceAccessEvaluator::organization_wide(),
            idempotency_key: idempotency_key.into(),
            request_id: Uuid::now_v7(),
        }
    }
}

async fn setup() -> (
    Fixture,
    Arc<InMemoryWorkloadProfileRepository>,
    AcceptWorkloadProfileHandler,
) {
    let fixture = Fixture::new();
    let plans = Arc::new(InMemoryBuildPlanRepository::new());
    seed_plan(&plans, &fixture.plan).await;
    let profiles = Arc::new(InMemoryWorkloadProfileRepository::new());
    let handler = AcceptWorkloadProfileHandler::new(profiles.clone(), plans);
    (fixture, profiles, handler)
}

async fn seed_plan(repository: &InMemoryBuildPlanRepository, plan: &AcceptedBuildPlan) {
    let request_id = Uuid::now_v7();
    repository
        .accept(AcceptBuildPlanWrite {
            plan: plan.clone(),
            event: BuildPlanAccepted::envelope(plan, request_id).expect("BuildPlan event"),
            actor_principal_id: plan.accepted_by,
            request_id,
            idempotency: IdempotencyRequest::new(
                "workload-profile-test-build-plan",
                plan.id.to_string(),
                plan.contract.digest().as_str().as_bytes(),
            )
            .expect("BuildPlan idempotency"),
        })
        .await
        .expect("seed accepted BuildPlan");
}

fn revision_write(
    plan: &AcceptedBuildPlan,
    revision: AcceptedWorkloadProfileRevision,
    expected_previous_revision_id: Option<
        crate::modules::shared_kernel::domain::WorkloadProfileRevisionId,
    >,
    idempotency_key: &str,
) -> AcceptWorkloadProfileRevisionWrite {
    let request_id = Uuid::now_v7();
    let event = WorkloadProfileRevisionAccepted::envelope(&revision, request_id)
        .expect("workload profile event");
    AcceptWorkloadProfileRevisionWrite {
        actor_principal_id: revision.accepted_by,
        idempotency: IdempotencyRequest::new(
            "workload-profile-stale-write-test",
            idempotency_key,
            revision.contract.digest().as_str().as_bytes(),
        )
        .expect("workload profile idempotency"),
        revision,
        build_plan: plan.clone(),
        expected_previous_revision_id,
        event,
        request_id,
    }
}

async fn execute(
    handler: &AcceptWorkloadProfileHandler,
    command: AcceptWorkloadProfile,
) -> super::AcceptWorkloadProfileResult {
    handler
        .execute(command, context())
        .await
        .expect("Boot result")
        .expect("workload profile acceptance")
}

fn web_profile(cpu_millis: u64) -> WorkloadProfileSpec {
    WorkloadProfileSpec {
        name: "api".into(),
        kind: WorkloadProfileKind::Web,
        process: ServiceProcess {
            command: vec!["/app/server".into()],
            args: vec!["--production".into()],
            working_directory: Some("/app".into()),
            environment: BTreeMap::from([("LOG_LEVEL".into(), "info".into())]),
        },
        secrets: Vec::new(),
        resources: WorkloadProfileResources {
            cpu_millis,
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: Some(256 * 1024 * 1024),
            execution_timeout_ms: None,
        },
        ports: vec![ServicePort {
            name: "http".into(),
            container_port: 8_080,
        }],
        health: Some(HttpHealthCheck {
            port_name: "http".into(),
            path: "/health".into(),
            interval_ms: 5_000,
            timeout_ms: 1_000,
            healthy_threshold: 2,
            unhealthy_threshold: 3,
            stabilization_window_ms: 10_000,
        }),
        public_port: Some("http".into()),
        schedule: None,
    }
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}
