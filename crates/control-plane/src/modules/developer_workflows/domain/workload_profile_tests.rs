use super::*;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectId, SecretId, SourceRevisionId,
};
use crate::modules::workloads::domain::entities::{
    HttpHealthCheck, SecretBinding, SecretBindingTarget, ServicePort, ServiceProcess,
};
use chrono::{TimeZone, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

const BUILD_PLAN_FIXTURE: &str = include_str!("../../../../../../contracts/p0.1/build-plan.acl");

#[test]
fn workload_profile_acl_is_canonical_closed_and_build_plan_bound() {
    let build_plan = accepted_build_plan();
    let mut profile = web_profile();
    profile.secrets = vec![
        SecretBinding {
            name: "zeta-token".into(),
            secret_id: SecretId::new(),
            version: 1,
            target: SecretBindingTarget::Environment {
                variable: "ZETA_TOKEN".into(),
            },
        },
        SecretBinding {
            name: "api-token".into(),
            secret_id: SecretId::new(),
            version: 2,
            target: SecretBindingTarget::Environment {
                variable: "API_TOKEN".into(),
            },
        },
    ];
    profile.ports.push(ServicePort {
        name: "metrics".into(),
        container_port: 9_090,
    });

    let contract = WorkloadProfileContract::bind(&build_plan, profile).expect("profile contract");
    assert_eq!(contract.spec().profile.secrets[0].name, "api-token");
    assert_eq!(contract.spec().profile.ports[0].name, "http");
    assert_eq!(
        WorkloadProfileContract::parse_acl(contract.canonical_acl()).expect("ACL round trip"),
        contract
    );
    assert_eq!(
        WorkloadProfileContract::restore(contract.canonical_acl(), contract.digest().as_str())
            .expect("stored profile"),
        contract
    );
    contract
        .validate_for(&build_plan)
        .expect("exact BuildPlan binding");

    let unknown = contract.canonical_acl().replace(
        "  schema = \"a3s.cloud.workload-profile.v1\"\n",
        "  schema = \"a3s.cloud.workload-profile.v1\"\n  unknown = true\n",
    );
    assert!(WorkloadProfileContract::parse_acl(&unknown).is_err());
    assert!(WorkloadProfileContract::restore(
        contract.canonical_acl(),
        &format!("sha256:{}", "0".repeat(64)),
    )
    .is_err());

    let other_plan = AcceptedBuildPlan::accept(
        OrganizationId::new(),
        build_plan.project_id,
        build_plan.environment_id,
        build_plan.contract.clone(),
        PrincipalId::new(),
        build_plan.accepted_at,
    )
    .expect("other accepted plan");
    assert!(contract.validate_for(&other_plan).is_err());
}

#[test]
fn profile_kinds_enforce_service_task_and_route_ownership() {
    let build_plan = accepted_build_plan();

    let mut web = web_profile();
    web.health = None;
    assert!(WorkloadProfileContract::bind(&build_plan, web).is_err());

    let mut worker = worker_profile();
    worker.public_port = Some("http".into());
    assert!(WorkloadProfileContract::bind(&build_plan, worker).is_err());

    let mut scheduled = scheduled_profile();
    scheduled.ports.push(ServicePort {
        name: "http".into(),
        container_port: 8_080,
    });
    assert!(WorkloadProfileContract::bind(&build_plan, scheduled).is_err());

    WorkloadProfileContract::bind(&build_plan, worker_profile()).expect("worker profile");
    WorkloadProfileContract::bind(&build_plan, scheduled_profile())
        .expect("scheduled Task profile");
}

#[test]
fn scheduled_task_policy_requires_canonical_cron_timezone_and_closed_bounds() {
    let schedule = schedule();
    schedule.validate().expect("canonical schedule");

    let mut invalid = schedule.clone();
    invalid.expression = "*/5 * * * *".into();
    assert!(invalid.validate().is_err());

    let mut invalid = schedule.clone();
    invalid.expression = "0  */5 * * * * *".into();
    assert!(invalid.validate().is_err());

    let mut invalid = schedule.clone();
    invalid.timezone = "Not/A_Zone".into();
    assert!(invalid.validate().is_err());

    let mut invalid = schedule.clone();
    invalid.timezone = "A".repeat(256);
    assert!(invalid.validate().is_err());

    let mut invalid = schedule.clone();
    invalid.maximum_concurrency = 0;
    assert!(invalid.validate().is_err());

    let mut invalid = schedule;
    invalid.history.successful_limit = 0;
    invalid.history.failed_limit = 0;
    assert!(invalid.validate().is_err());
}

fn accepted_build_plan() -> AcceptedBuildPlan {
    let source_revision_id = SourceRevisionId::from_uuid(
        Uuid::parse_str("018f0f70-0000-7000-8000-000000000001").expect("Source revision ID"),
    );
    let proposal = BuildPlanProposal::parse_acl(BUILD_PLAN_FIXTURE).expect("BuildPlan fixture");
    let contract = AcceptedBuildPlanContract::from_proposal(source_revision_id, proposal)
        .expect("accepted contract");
    AcceptedBuildPlan::accept(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
        contract,
        PrincipalId::new(),
        Utc.with_ymd_and_hms(2026, 8, 24, 0, 0, 0)
            .single()
            .expect("timestamp"),
    )
    .expect("accepted plan")
}

fn process() -> ServiceProcess {
    ServiceProcess {
        command: vec!["/app/server".into()],
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
        ephemeral_storage_bytes: Some(256 * 1024 * 1024),
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
