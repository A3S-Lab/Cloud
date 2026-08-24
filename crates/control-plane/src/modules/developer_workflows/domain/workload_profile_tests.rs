use super::*;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, SecretId,
    SourceRevisionId, WorkloadProfileRevisionId,
};
use chrono::{Duration, TimeZone, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

const BUILD_PLAN_FIXTURE: &str = include_str!("../../../../../../contracts/p0.1/build-plan.acl");
const WORKLOAD_PROFILE_FIXTURE: &str =
    include_str!("../../../../../../contracts/p0.2/workload-profile.acl");

#[test]
fn workload_profile_acl_is_canonical_closed_and_build_plan_bound() {
    let build_plan = accepted_build_plan();
    let mut profile = web_profile();
    profile.secrets = vec![
        WorkloadSecretBinding {
            name: "zeta-token".into(),
            secret_id: SecretId::new(),
            version: 1,
            target: WorkloadSecretTarget::Environment {
                variable: "ZETA_TOKEN".into(),
            },
        },
        WorkloadSecretBinding {
            name: "api-token".into(),
            secret_id: SecretId::new(),
            version: 2,
            target: WorkloadSecretTarget::Environment {
                variable: "API_TOKEN".into(),
            },
        },
    ];
    profile.ports.push(WorkloadServicePort {
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
fn public_workload_profile_fixture_is_canonical_and_closed() {
    let contract =
        WorkloadProfileContract::parse_acl(WORKLOAD_PROFILE_FIXTURE).expect("public P0.2 fixture");
    assert_eq!(contract.canonical_acl(), WORKLOAD_PROFILE_FIXTURE);
    assert_eq!(contract.schema(), WORKLOAD_PROFILE_SCHEMA);
    assert_eq!(contract.spec().profile.kind, WorkloadProfileKind::Web);
    assert_eq!(contract.spec().profile.name, "api");
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
    scheduled.ports.push(WorkloadServicePort {
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

#[test]
fn accepted_revision_identity_restore_and_event_are_exact() {
    let build_plan = accepted_build_plan();
    let contract =
        WorkloadProfileContract::bind(&build_plan, web_profile()).expect("profile contract");
    let actor = PrincipalId::new();
    let accepted_at = build_plan.accepted_at + Duration::seconds(1);
    let revision = AcceptedWorkloadProfileRevision::accept(
        &build_plan,
        contract.clone(),
        1,
        actor,
        accepted_at,
    )
    .expect("accepted revision");
    let same_identity = AcceptedWorkloadProfileRevision::accept(
        &build_plan,
        contract.clone(),
        1,
        actor,
        accepted_at + Duration::seconds(1),
    )
    .expect("same logical revision identity");
    let next = AcceptedWorkloadProfileRevision::accept(
        &build_plan,
        contract.clone(),
        2,
        actor,
        accepted_at + Duration::seconds(1),
    )
    .expect("next revision");

    assert_eq!(same_identity.profile_id, revision.profile_id);
    assert_eq!(same_identity.id, revision.id);
    assert_eq!(next.profile_id, revision.profile_id);
    assert_ne!(next.id, revision.id);
    assert_eq!(
        AcceptedWorkloadProfileRevision::restore(
            revision.organization_id,
            revision.project_id,
            revision.environment_id,
            revision.profile_id,
            revision.id,
            revision.revision_number,
            revision.build_plan_id,
            revision.source_revision_id,
            revision.contract.canonical_acl(),
            revision.contract.digest().as_str(),
            revision.accepted_by,
            revision.accepted_at,
        )
        .expect("restored revision"),
        revision
    );

    let request_id = Uuid::now_v7();
    let event =
        WorkloadProfileRevisionAccepted::envelope(&revision, request_id).expect("acceptance event");
    let payload: WorkloadProfileRevisionAccepted =
        serde_json::from_value(event.payload.clone()).expect("typed event payload");
    assert_eq!(event.aggregate_id, revision.profile_id.as_uuid());
    assert_eq!(event.aggregate_version, 1);
    assert_eq!(payload.workload_profile_revision_id, revision.id);
    assert_eq!(payload.profile_digest, revision.contract.digest().as_str());

    AcceptWorkloadProfileRevisionWrite {
        revision,
        build_plan,
        expected_previous_revision_id: None,
        event,
        actor_principal_id: actor,
        request_id,
        idempotency: IdempotencyRequest::new(
            "workload-profile-domain-test",
            "accept-1",
            b"accept revision one",
        )
        .expect("idempotency"),
    }
    .validate()
    .expect("exact write");
}

#[test]
fn revision_write_rejects_previous_and_event_drift() {
    let build_plan = accepted_build_plan();
    let contract =
        WorkloadProfileContract::bind(&build_plan, web_profile()).expect("profile contract");
    let actor = PrincipalId::new();
    let revision = AcceptedWorkloadProfileRevision::accept(
        &build_plan,
        contract,
        1,
        actor,
        build_plan.accepted_at + Duration::seconds(1),
    )
    .expect("accepted revision");
    let request_id = Uuid::now_v7();
    let event =
        WorkloadProfileRevisionAccepted::envelope(&revision, request_id).expect("acceptance event");
    let idempotency = IdempotencyRequest::new("workload-profile-domain-test", "drift", b"drift")
        .expect("idempotency");

    let wrong_previous = AcceptWorkloadProfileRevisionWrite {
        revision: revision.clone(),
        build_plan: build_plan.clone(),
        expected_previous_revision_id: Some(WorkloadProfileRevisionId::new()),
        event: event.clone(),
        actor_principal_id: actor,
        request_id,
        idempotency: idempotency.clone(),
    };
    assert!(wrong_previous.validate().is_err());

    let mut wrong_event = event;
    wrong_event.aggregate_version = 2;
    let event_drift = AcceptWorkloadProfileRevisionWrite {
        revision,
        build_plan,
        expected_previous_revision_id: None,
        event: wrong_event,
        actor_principal_id: actor,
        request_id,
        idempotency,
    };
    assert!(event_drift.validate().is_err());
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

fn process() -> WorkloadProcess {
    WorkloadProcess {
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
        ports: vec![WorkloadServicePort {
            name: "http".into(),
            container_port: 8_080,
        }],
        health: Some(WorkloadHttpHealthCheck {
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
