use super::*;
use crate::modules::artifacts::domain::test_support::{
    succeeded_external_build_with_output, typed_build_output,
};
use crate::modules::artifacts::domain::{BuildRun, BuildRunStatus};
use crate::modules::developer_workflows::domain::{
    AcceptedBuildPlan, AcceptedBuildPlanContract, BuildPlanDetectorKind, BuildPlanProposal,
    BuildPlanProposalSpec, ScheduledTaskCatchUpPolicy, ScheduledTaskHistoryPolicy,
    ScheduledTaskRetryPolicy, ScheduledTaskSchedule, SourceLayoutIdentity, WorkloadProfileContract,
    WorkloadProfileKind, WorkloadProfileResources, WorkloadProfileSpec,
    BUILD_PLAN_DETECTOR_REVISION,
};
use crate::modules::executions::domain::Execution;
use crate::modules::executions::project_execution_task;
use crate::modules::shared_kernel::domain::{
    BuildRunId, EnvironmentId, ExecutionId, GitCommitSha, OrganizationId, PrincipalId, ProjectId,
    Sha256Digest, SourceRevisionId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::sources::domain::BuildRecipe;
use crate::modules::workloads::domain::entities::{
    HttpHealthCheck, ServicePort, ServiceProcess, WorkloadRevision,
};
use crate::modules::workloads::project_runtime_spec;
use a3s_cloud_contracts::DURABLE_CELL_BUNDLE_MEDIA_TYPE;
use a3s_runtime::contract::{NetworkMode, RestartPolicy, RuntimeUnitClass};
use chrono::{Duration, TimeZone, Utc};
use std::collections::BTreeMap;

#[test]
fn web_profile_compiles_verified_artifact_into_existing_service_rail() {
    let (build_plan, build_run) = plan_and_build();
    let profile = WorkloadProfileContract::bind(&build_plan, web_profile()).expect("web profile");

    let CompiledWorkloadProfile::Service(compiled) =
        WorkloadProfileCompilationService::compile(&build_plan, &profile, &build_run)
            .expect("compiled web profile")
    else {
        panic!("web profile must compile to a Service");
    };
    assert_eq!(compiled.kind, WorkloadProfileKind::Web);
    assert_eq!(compiled.build_plan_id, build_plan.id);
    assert_eq!(compiled.build_run_id, build_run.id);
    assert_eq!(compiled.profile_digest, *profile.digest());
    assert_eq!(compiled.public_port.as_deref(), Some("http"));
    assert_eq!(
        compiled.template.artifact.digest,
        build_run
            .published_artifact
            .as_ref()
            .expect("published artifact")
            .digest
    );

    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        WorkloadId::new(),
        1,
        compiled.template,
        build_run.finished_at.expect("finish time"),
    )
    .expect("Workload revision");
    let runtime = project_runtime_spec(&revision).expect("Runtime Service");
    assert_eq!(runtime.class, RuntimeUnitClass::Service);
    assert_eq!(runtime.network.mode, NetworkMode::Service);
    assert_eq!(runtime.network.ports[0].name, "http");
    assert!(runtime.health.is_some());
    assert_eq!(runtime.restart, RestartPolicy::Always);
}

#[test]
fn worker_profile_has_no_implicit_route_or_service_listener() {
    let (build_plan, build_run) = plan_and_build();
    let profile =
        WorkloadProfileContract::bind(&build_plan, worker_profile()).expect("worker profile");
    let CompiledWorkloadProfile::Service(compiled) =
        WorkloadProfileCompilationService::compile(&build_plan, &profile, &build_run)
            .expect("compiled worker profile")
    else {
        panic!("worker profile must compile to a Service");
    };
    assert_eq!(compiled.kind, WorkloadProfileKind::Worker);
    assert!(compiled.public_port.is_none());
    assert!(compiled.template.ports.is_empty());

    let revision = WorkloadRevision::create(
        WorkloadRevisionId::new(),
        WorkloadId::new(),
        1,
        compiled.template,
        build_run.finished_at.expect("finish time"),
    )
    .expect("Workload revision");
    let runtime = project_runtime_spec(&revision).expect("Runtime Service");
    assert_eq!(runtime.class, RuntimeUnitClass::Service);
    assert_eq!(runtime.network.mode, NetworkMode::None);
    assert!(runtime.network.ports.is_empty());
}

#[test]
fn scheduled_profile_compiles_to_networkless_existing_execution_task_rail() {
    let (build_plan, build_run) = plan_and_build();
    let profile = WorkloadProfileContract::bind(&build_plan, scheduled_profile())
        .expect("scheduled Task profile");
    let CompiledWorkloadProfile::ScheduledTask(compiled) =
        WorkloadProfileCompilationService::compile(&build_plan, &profile, &build_run)
            .expect("compiled scheduled Task")
    else {
        panic!("scheduled profile must compile to an Execution Task");
    };
    assert_eq!(compiled.schedule, schedule());
    assert_eq!(compiled.template.resources.timeout_ms, 60_000);

    let execution = Execution::create(
        compiled.organization_id,
        compiled.project_id,
        compiled.environment_id,
        ExecutionId::new(),
        compiled.template,
        build_run.finished_at.expect("finish time"),
    )
    .expect("Execution");
    let runtime = project_execution_task(&execution).expect("Runtime Task");
    assert_eq!(runtime.class, RuntimeUnitClass::Task);
    assert_eq!(runtime.network.mode, NetworkMode::None);
    assert_eq!(runtime.restart, RestartPolicy::Never);
    assert_eq!(runtime.resources.execution_timeout_ms, Some(60_000));
}

#[test]
fn compilation_rejects_scope_plan_and_build_evidence_drift() {
    let (build_plan, build_run) = plan_and_build();
    let profile = WorkloadProfileContract::bind(&build_plan, web_profile()).expect("web profile");

    let mut wrong_scope = build_run.clone();
    wrong_scope.organization_id = OrganizationId::new();
    assert!(
        WorkloadProfileCompilationService::compile(&build_plan, &profile, &wrong_scope,).is_err()
    );

    let mut unfinished = build_run.clone();
    unfinished.status = BuildRunStatus::Running;
    assert!(
        WorkloadProfileCompilationService::compile(&build_plan, &profile, &unfinished).is_err()
    );

    let mut changed_source = build_run.clone();
    changed_source.source_content_digest = Some(digest('9').to_string());
    assert!(
        WorkloadProfileCompilationService::compile(&build_plan, &profile, &changed_source,)
            .is_err()
    );

    let mut changed_identity = build_run.clone();
    changed_identity.id = BuildRunId::new();
    assert!(
        WorkloadProfileCompilationService::compile(&build_plan, &profile, &changed_identity,)
            .is_err()
    );

    let other_plan = accepted_plan(
        OrganizationId::new(),
        build_plan.project_id,
        build_plan.environment_id,
        build_plan.source_revision_id,
        build_plan.accepted_at,
    );
    assert!(WorkloadProfileCompilationService::compile(&other_plan, &profile, &build_run).is_err());
}

fn plan_and_build() -> (AcceptedBuildPlan, BuildRun) {
    let accepted_at = Utc
        .with_ymd_and_hms(2026, 8, 24, 1, 0, 0)
        .single()
        .expect("timestamp");
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let source_revision_id = SourceRevisionId::new();
    let plan = accepted_plan(
        organization_id,
        project_id,
        environment_id,
        source_revision_id,
        accepted_at,
    );
    let output_digest = digest('d').to_string();
    let build = succeeded_external_build_with_output(
        organization_id,
        project_id,
        environment_id,
        source_revision_id,
        typed_build_output(&output_digest, DURABLE_CELL_BUNDLE_MEDIA_TYPE, 64),
        accepted_at + Duration::seconds(1),
    );
    (plan, build)
}

fn accepted_plan(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    accepted_at: chrono::DateTime<Utc>,
) -> AcceptedBuildPlan {
    let recipe = BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        ".",
        "Dockerfile",
        None,
        vec!["linux/amd64".into()],
    )
    .expect("recipe");
    let proposal = BuildPlanProposal::from_spec(BuildPlanProposalSpec {
        source: SourceLayoutIdentity::new(
            digest('1'),
            GitCommitSha::parse("a".repeat(40)).expect("commit"),
            digest('2'),
        )
        .expect("source identity"),
        detector: BuildPlanDetectorKind::Dockerfile,
        detector_revision: BUILD_PLAN_DETECTOR_REVISION.into(),
        project_root: ".".into(),
        evidence_path: "Dockerfile".into(),
        evidence_digest: digest('c'),
        recipe,
    })
    .expect("proposal");
    let contract = AcceptedBuildPlanContract::from_proposal(source_revision_id, proposal)
        .expect("accepted contract");
    AcceptedBuildPlan::accept(
        organization_id,
        project_id,
        environment_id,
        contract,
        PrincipalId::new(),
        accepted_at,
    )
    .expect("accepted BuildPlan")
}

fn digest(seed: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", seed.to_string().repeat(64))).expect("digest")
}

fn process() -> ServiceProcess {
    ServiceProcess {
        command: vec!["/app/service".into()],
        args: vec!["--production".into()],
        working_directory: Some("/app".into()),
        environment: BTreeMap::from([("LOG_LEVEL".into(), "info".into())]),
    }
}

fn resources(execution_timeout_ms: Option<u64>) -> WorkloadProfileResources {
    WorkloadProfileResources {
        cpu_millis: 250,
        memory_bytes: 128 * 1024 * 1024,
        pids: 64,
        ephemeral_storage_bytes: None,
        execution_timeout_ms,
    }
}

fn web_profile() -> WorkloadProfileSpec {
    WorkloadProfileSpec {
        name: "api".into(),
        kind: WorkloadProfileKind::Web,
        process: process(),
        secrets: Vec::new(),
        resources: resources(None),
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

fn worker_profile() -> WorkloadProfileSpec {
    WorkloadProfileSpec {
        name: "events".into(),
        kind: WorkloadProfileKind::Worker,
        process: process(),
        secrets: Vec::new(),
        resources: resources(None),
        ports: Vec::new(),
        health: None,
        public_port: None,
        schedule: None,
    }
}

fn scheduled_profile() -> WorkloadProfileSpec {
    WorkloadProfileSpec {
        name: "cleanup".into(),
        kind: WorkloadProfileKind::ScheduledTask,
        process: process(),
        secrets: Vec::new(),
        resources: resources(Some(60_000)),
        ports: Vec::new(),
        health: None,
        public_port: None,
        schedule: Some(schedule()),
    }
}

fn schedule() -> ScheduledTaskSchedule {
    ScheduledTaskSchedule {
        expression: "0 */5 * * * * *".into(),
        timezone: "Asia/Shanghai".into(),
        catch_up: ScheduledTaskCatchUpPolicy::Skip,
        maximum_concurrency: 1,
        misfire_grace_ms: 60_000,
        retry: ScheduledTaskRetryPolicy {
            maximum_attempts: 3,
            initial_backoff_ms: 1_000,
            maximum_backoff_ms: 30_000,
        },
        history: ScheduledTaskHistoryPolicy {
            successful_limit: 20,
            failed_limit: 20,
            maximum_age_days: 30,
        },
    }
}
