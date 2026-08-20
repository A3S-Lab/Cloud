use super::SeedTransaction;
use crate::migrate_and_connect_for_test;
use a3s_cloud_control_plane::infrastructure::{FlowInfrastructure, FlowOperationCoordinator};
use a3s_cloud_control_plane::modules::forms::{
    AcceptedFormSubmission, CreateFormDraftWrite, FormDocument, FormDraft, FormDraftChanged,
    FormPublicationRecord, FormRelease, FormReleaseContent, FormReleasePublished, FormSubmission,
    IFormRepository, InMemoryFormRepository, PostgresFormRepository, PublishFormReleaseWrite,
};
use a3s_cloud_control_plane::modules::operations::{
    FlowOperationEngine, IOperationRepository, OperationReconciler, PostgresOperationRepository,
    ReconcileOperationsHandler,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    canonical_json_bounded, sha256_digest, AuthorizationDecisionRef, FormId, FormReleaseId,
    FormSubmissionId, IdempotencyRequest, OntologyId, OntologyRevisionId, OrganizationId,
    PlanRevisionId, PrincipalId, ProjectId, Sha256Digest, WorkflowDecisionId, WorkflowDefinitionId,
    WorkflowGoalId, WorkflowRevisionId, WorkflowRunId,
};
use a3s_cloud_control_plane::modules::workflow::domain::ResolvedWorkflowPayload;
use a3s_cloud_control_plane::modules::workflow::{
    CancelWorkflowRunWrite, CapabilityOwner, CapabilityReference, CapabilityType,
    ChangeHumanTaskWrite, CreateWorkflowRunWrite, DecideHumanTaskWrite, FlowResumeDisposition,
    FlowResumePayload, FlowWorkflowRunCoordinator, HumanTaskCoordinator, HumanTaskDecisionRecord,
    HumanTaskResumeWorker, HumanTaskResumeWorkerConfig, HumanTaskStateChanged, HumanTaskStatus,
    IHumanTaskRepository, IWorkflowRunCoordinator, IWorkflowRunRepository,
    PostgresHumanTaskRepository, PostgresWorkflowRunRepository, WorkflowDataSchema,
    WorkflowDataType, WorkflowDecision, WorkflowDecisionOutcome, WorkflowEdgeSpec, WorkflowPayload,
    WorkflowPayloadContent, WorkflowPlan, WorkflowPlanStep, WorkflowRun,
    WorkflowRunCancellationRequested, WorkflowRunFlowRuntime, WorkflowRunInput,
    WorkflowRunReconciler, WorkflowRunRecord, WorkflowRunRequested, WorkflowStepConfiguration,
    WorkflowStepKind, WORKFLOW_PLAN_COMPILER_REVISION, WORKFLOW_PLAN_MAX_BYTES,
    WORKFLOW_PLAN_SCHEMA, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION,
    WORKFLOW_RUN_INPUT_SCHEMA, WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION,
};
use a3s_flow::WorkflowRunStatus as FlowRunStatus;
use a3s_form_core::{
    canonicalize_json, compile_bytes, digest_interaction_value, parse_json, FormInteractionOutcome,
    FormInteractionSubmission, FormInteractionSubmissionAssignment, COMPILE_REQUEST_API_VERSION,
    FORM_INTERACTION_SUBMISSION_API_VERSION,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresError};
use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Timelike, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const HUMAN_STEP_ID: &str = "human_review";

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[path = "end_to_end/seed.rs"]
mod seed;

use seed::seed_identity_authority;

#[derive(Clone, Copy)]
struct Authorities {
    organization_id: OrganizationId,
    project_id: ProjectId,
    other_organization_id: OrganizationId,
    other_project_id: ProjectId,
    actor: PrincipalId,
    reviewer: PrincipalId,
    form_id: FormId,
    form_release_id: FormReleaseId,
}

pub(crate) async fn exercise_human_task_flow_end_to_end(url: String) -> TestResult<()> {
    let executor = migrate_and_connect_for_test(&url, 8).await?;
    let authorities = Authorities {
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        other_organization_id: OrganizationId::new(),
        other_project_id: ProjectId::new(),
        actor: PrincipalId::new(),
        reviewer: PrincipalId::new(),
        form_id: FormId::new(),
        form_release_id: FormReleaseId::new(),
    };
    let requested_at = canonical_timestamp(Utc::now());
    seed_identity_authority(&executor, authorities, requested_at).await?;

    let forms: Arc<dyn IFormRepository> = Arc::new(PostgresFormRepository::new(executor.clone()));
    let release = publish_interaction_form(Arc::clone(&forms), authorities, requested_at).await?;
    let input = human_decision_input(authorities, &release, requested_at)?;
    let flow_run_id = input.workflow_run_id.to_string();
    seed_workflow_authority(&executor, &input, authorities.actor).await?;

    let workflow_runs: Arc<dyn IWorkflowRunRepository> =
        Arc::new(PostgresWorkflowRunRepository::new(executor.clone()));
    let human_tasks: Arc<dyn IHumanTaskRepository> =
        Arc::new(PostgresHumanTaskRepository::new(executor.clone()));
    let (run, steps) = WorkflowRun::create(input.clone(), authorities.actor)?;
    let requested = WorkflowRunRecord { run, steps };
    workflow_runs
        .create(CreateWorkflowRunWrite {
            event: WorkflowRunRequested::envelope(&requested.run, Uuid::now_v7())?,
            record: requested.clone(),
            actor_principal_id: authorities.actor,
            request_id: Uuid::now_v7(),
            idempotency: idempotency(
                "postgres-human-task-flow-runs",
                "start",
                input.workflow_run_id.to_string().as_bytes(),
            ),
        })
        .await?;

    let flow =
        FlowInfrastructure::connect(&url, Arc::new(WorkflowRunFlowRuntime::default())).await?;
    let engine = flow.engine();
    let operation_repository: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(executor.clone()));
    let operation_reconciler = OperationReconciler::new(
        Arc::new(ReconcileOperationsHandler::new(
            Arc::clone(&operation_repository),
            Arc::new(FlowOperationEngine::new(engine.clone())),
        )),
        100,
    );
    let flow_coordinator = FlowOperationCoordinator::new(
        operation_reconciler,
        &flow,
        Duration::from_millis(5),
        Duration::from_secs(2),
    )?;
    let workflow_run_coordinator: Arc<dyn IWorkflowRunCoordinator> =
        Arc::new(FlowWorkflowRunCoordinator::new(engine.clone()));
    let workflow_reconciler = WorkflowRunReconciler::new(
        Arc::clone(&workflow_runs),
        workflow_run_coordinator,
        Duration::from_millis(5),
        100,
    )?;

    drive_flow_until(
        &flow_coordinator,
        &workflow_reconciler,
        &engine,
        &flow_run_id,
        FlowRunStatus::Suspended,
    )
    .await?;

    let missing_release_forms: Arc<dyn IFormRepository> = Arc::new(InMemoryFormRepository::new());
    let missing_release_report = HumanTaskCoordinator::new(
        Arc::clone(&workflow_runs),
        missing_release_forms,
        Arc::clone(&human_tasks),
        engine.clone(),
        Duration::from_millis(5),
        100,
    )?
    .run_once(100)
    .await?;
    assert_eq!(missing_release_report.failures.len(), 1);
    assert!(missing_release_report.failures[0]
        .error
        .contains("FormRelease does not exist"));

    let drifted_release_forms: Arc<dyn IFormRepository> = Arc::new(InMemoryFormRepository::new());
    publish_interaction_form_document(
        Arc::clone(&drifted_release_forms),
        authorities,
        requested_at,
        drifted_interaction_form_document()?,
    )
    .await?;
    let drifted_release_report = HumanTaskCoordinator::new(
        Arc::clone(&workflow_runs),
        drifted_release_forms,
        Arc::clone(&human_tasks),
        engine.clone(),
        Duration::from_millis(5),
        100,
    )?
    .run_once(100)
    .await?;
    assert_eq!(drifted_release_report.failures.len(), 1);
    assert!(drifted_release_report.failures[0]
        .error
        .contains("FormRelease authority drifted"));
    assert!(human_tasks
        .list_tasks(
            authorities.organization_id,
            authorities.project_id,
            None,
            10,
        )
        .await?
        .is_empty());

    let coordinator_a = HumanTaskCoordinator::new(
        Arc::clone(&workflow_runs),
        Arc::clone(&forms),
        Arc::clone(&human_tasks),
        engine.clone(),
        Duration::from_millis(5),
        100,
    )?;
    let coordinator_b = HumanTaskCoordinator::new(
        Arc::clone(&workflow_runs),
        Arc::clone(&forms),
        Arc::clone(&human_tasks),
        engine.clone(),
        Duration::from_millis(5),
        100,
    )?;
    let (first, second) = tokio::join!(coordinator_a.run_once(100), coordinator_b.run_once(100));
    let first = first?;
    let second = second?;
    assert!(first.failures.is_empty(), "{first:#?}");
    assert!(second.failures.is_empty(), "{second:#?}");
    assert_eq!(first.created_tasks + second.created_tasks, 1);

    let ready = human_tasks
        .list_tasks(
            authorities.organization_id,
            authorities.project_id,
            Some(HumanTaskStatus::Ready),
            10,
        )
        .await?;
    assert_eq!(ready.len(), 1);
    assert_eq!(
        human_tasks
            .find_task(authorities.other_organization_id, ready[0].task.id)
            .await?,
        None
    );

    let restarted_coordinator = HumanTaskCoordinator::new(
        Arc::clone(&workflow_runs),
        Arc::clone(&forms),
        Arc::clone(&human_tasks),
        engine.clone(),
        Duration::from_millis(5),
        100,
    )?;
    let replay = restarted_coordinator.run_once(100).await?;
    assert!(replay.failures.is_empty(), "{replay:#?}");
    assert_eq!(replay.replayed_tasks, 1);
    assert_eq!(replay.created_tasks, 0);

    let mut claimed = ready[0].clone();
    let claimed_at = transition_time(claimed.task.updated_at);
    claimed.claim(2, authorities.reviewer, claimed_at)?;
    human_tasks
        .change_task(ChangeHumanTaskWrite {
            event: HumanTaskStateChanged::envelope(&claimed, None)?,
            record: claimed.clone(),
            expected_version: 2,
            actor_principal_id: authorities.reviewer,
            request_id: Uuid::now_v7(),
            idempotency: idempotency(
                "postgres-human-task-flow-claims",
                "claim",
                claimed.task.id.to_string().as_bytes(),
            ),
        })
        .await?;

    let submission =
        accepted_submission(&claimed, authorities.reviewer, transition_time(claimed_at))?;
    let decision = WorkflowDecision::from_submission(
        WorkflowDecisionId::new(),
        &claimed.task,
        &submission,
        submission.accepted_output()?,
        transition_time(submission.accepted_at),
    )?;
    let mut completed = claimed;
    completed.complete(3, &decision)?;
    let decision_record = HumanTaskDecisionRecord {
        task: completed.clone(),
        submission: Some(submission.clone()),
        resume_payload: FlowResumePayload::from_decision(&decision)?,
        resume_receipt: None,
        decision,
    };
    human_tasks
        .decide_task(DecideHumanTaskWrite {
            event: HumanTaskStateChanged::envelope(&completed, Some(submission.id.as_uuid()))?,
            record: decision_record.clone(),
            expected_version: 3,
            actor_principal_id: authorities.reviewer,
            request_id: Uuid::now_v7(),
            idempotency: idempotency(
                "postgres-human-task-flow-decisions",
                "approve",
                decision_record.decision.digest.as_str().as_bytes(),
            ),
        })
        .await?;

    // Model a worker that committed HookReceived and crashed before acknowledging
    // its PostgreSQL resume lease. The restarted worker must take over the expired
    // lease, observe the exact durable event, and finish delivery idempotently.
    tokio::time::sleep(Duration::from_millis(2)).await;
    let abandoned_owner = Uuid::now_v7();
    let abandoned = human_tasks
        .claim_resume_deliveries(abandoned_owner, 10, Utc::now(), Duration::from_millis(1))
        .await?;
    assert_eq!(abandoned.len(), 1);
    engine
        .resume_hook(
            &decision_record.resume_payload.flow_run_id,
            &decision_record.resume_payload.flow_hook_id,
            decision_record.resume_payload.to_flow_value()?,
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(5)).await;

    let resume_worker = HumanTaskResumeWorker::new(
        Arc::clone(&human_tasks),
        engine.clone(),
        HumanTaskResumeWorkerConfig {
            batch_size: 10,
            poll_interval: Duration::from_millis(5),
            lease_duration: Duration::from_secs(5),
            flow_operation_timeout: Duration::from_secs(1),
            initial_backoff: Duration::from_millis(5),
            maximum_backoff: Duration::from_secs(1),
        },
    )?;
    let delivered = resume_worker.run_once().await?;
    assert_eq!(delivered.claimed, 1, "{delivered:#?}");
    assert_eq!(delivered.delivered, 1, "{delivered:#?}");
    assert!(delivered.failures.is_empty(), "{delivered:#?}");
    assert_eq!(resume_worker.run_once().await?.claimed, 0);

    drive_flow_until(
        &flow_coordinator,
        &workflow_reconciler,
        &engine,
        &flow_run_id,
        FlowRunStatus::Completed,
    )
    .await?;
    let persisted_run = workflow_runs
        .find(authorities.organization_id, input.workflow_run_id)
        .await?
        .expect("persisted WorkflowRun");
    assert_eq!(
        persisted_run.run.status,
        a3s_cloud_control_plane::modules::workflow::WorkflowRunStatus::Completed
    );
    assert_eq!(
        persisted_run.run.output,
        Some(serde_json::to_value(
            &decision_record.resume_payload.output
        )?)
    );
    let persisted_decision = human_tasks
        .find_decision(authorities.organization_id, decision_record.decision.id)
        .await?
        .expect("persisted WorkflowDecision");
    assert!(persisted_decision.resume_receipt.is_some());
    assert_eq!(persisted_decision.submission, Some(submission));

    let database = Database::new(PostgresDialect, executor.clone());
    let rows = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, String, i32)>(
                "select (select count(*) from human_tasks where organization_id = ",
            )
            .bind(authorities.organization_id.as_uuid())
            .append("), (select count(*) from form_submissions where organization_id = ")
            .bind(authorities.organization_id.as_uuid())
            .append("), (select count(*) from workflow_resume_receipts where organization_id = ")
            .bind(authorities.organization_id.as_uuid())
            .append("), delivery.state, delivery.attempt_count from workflow_resume_outbox delivery where delivery.organization_id = ")
            .bind(authorities.organization_id.as_uuid())
            .append(" and delivery.workflow_decision_id = ")
            .bind(decision_record.decision.id.as_uuid()),
        )
        .await?;
    assert_eq!(rows, (1, 1, 1, "delivered".into(), 2));

    exercise_parent_cancellation(
        &executor,
        authorities,
        &input,
        Arc::clone(&forms),
        Arc::clone(&workflow_runs),
        Arc::clone(&human_tasks),
        &flow_coordinator,
        &workflow_reconciler,
        &engine,
    )
    .await?;
    exercise_expiry_after_flow_timeout(
        &executor,
        authorities,
        &input,
        Arc::clone(&forms),
        Arc::clone(&workflow_runs),
        Arc::clone(&human_tasks),
        &flow_coordinator,
        &workflow_reconciler,
        &engine,
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn exercise_parent_cancellation(
    executor: &a3s_orm::PostgresExecutor,
    authorities: Authorities,
    base_input: &WorkflowRunInput,
    forms: Arc<dyn IFormRepository>,
    workflow_runs: Arc<dyn IWorkflowRunRepository>,
    human_tasks: Arc<dyn IHumanTaskRepository>,
    flow_coordinator: &FlowOperationCoordinator,
    workflow_reconciler: &WorkflowRunReconciler,
    engine: &a3s_flow::FlowEngine,
) -> TestResult<()> {
    let requested_at = canonical_timestamp(Utc::now());
    let deadline_at = requested_at + ChronoDuration::seconds(5);
    let mut input = base_input.clone();
    input.workflow_run_id = WorkflowRunId::new();
    input.requested_at = requested_at;
    input.deadline_at = deadline_at;
    input.validate()?;
    let flow_run_id = input.workflow_run_id.to_string();
    let (run, steps) = WorkflowRun::create(input.clone(), authorities.actor)?;
    let requested = WorkflowRunRecord { run, steps };
    workflow_runs
        .create(CreateWorkflowRunWrite {
            event: WorkflowRunRequested::envelope(&requested.run, Uuid::now_v7())?,
            record: requested,
            actor_principal_id: authorities.actor,
            request_id: Uuid::now_v7(),
            idempotency: idempotency(
                "postgres-human-task-parent-cancellation-runs",
                "start",
                input.workflow_run_id.to_string().as_bytes(),
            ),
        })
        .await?;

    drive_flow_until(
        flow_coordinator,
        workflow_reconciler,
        engine,
        &flow_run_id,
        FlowRunStatus::Suspended,
    )
    .await?;
    let coordinator = HumanTaskCoordinator::new(
        Arc::clone(&workflow_runs),
        Arc::clone(&forms),
        Arc::clone(&human_tasks),
        engine.clone(),
        Duration::from_millis(5),
        100,
    )?;
    let created = coordinator.run_once_at(100, requested_at).await?;
    assert!(created.failures.is_empty(), "{created:#?}");
    assert!(created.cancellation_failures.is_empty(), "{created:#?}");
    assert!(created.expiry_failures.is_empty(), "{created:#?}");
    assert_eq!(created.created_tasks, 1, "{created:#?}");
    assert_eq!(created.activated_tasks, 1, "{created:#?}");

    let ready = human_tasks
        .list_tasks(
            authorities.organization_id,
            authorities.project_id,
            Some(HumanTaskStatus::Ready),
            10,
        )
        .await?
        .into_iter()
        .find(|task| task.task.workflow_run_id == input.workflow_run_id)
        .expect("ready cancellation HumanTask");

    let mut cancelling = workflow_runs
        .find(authorities.organization_id, input.workflow_run_id)
        .await?
        .expect("cancellation WorkflowRun");
    let expected_version = cancelling.run.aggregate_version;
    let cancellation_requested_at =
        canonical_timestamp(Utc::now()).max(transition_time(cancelling.run.updated_at));
    cancelling.run.request_cancellation(
        Some("operator cancelled parent run".into()),
        authorities.reviewer,
        cancellation_requested_at,
    )?;
    let cancellation_request_id = Uuid::now_v7();
    workflow_runs
        .request_cancellation(CancelWorkflowRunWrite {
            event: WorkflowRunCancellationRequested::envelope(
                &cancelling.run,
                cancellation_request_id,
            )?,
            record: cancelling,
            expected_version,
            actor_principal_id: authorities.reviewer,
            request_id: cancellation_request_id,
            idempotency: idempotency(
                "postgres-human-task-parent-cancellation-runs",
                "cancel",
                input.workflow_run_id.to_string().as_bytes(),
            ),
        })
        .await?;

    let preempted = coordinator
        .run_once_at(100, deadline_at + ChronoDuration::milliseconds(1))
        .await?;
    assert!(preempted.failures.is_empty(), "{preempted:#?}");
    assert!(preempted.cancellation_failures.is_empty(), "{preempted:#?}");
    assert!(preempted.expiry_failures.is_empty(), "{preempted:#?}");
    assert_eq!(preempted.deferred_cancellations, 1, "{preempted:#?}");
    assert_eq!(preempted.inspected_expirations, 0, "{preempted:#?}");
    assert_eq!(preempted.expired_tasks, 0, "{preempted:#?}");
    assert_eq!(
        human_tasks
            .find_task(authorities.organization_id, ready.task.id)
            .await?
            .expect("preempted HumanTask")
            .task
            .status,
        HumanTaskStatus::Ready
    );

    drive_flow_until(
        flow_coordinator,
        workflow_reconciler,
        engine,
        &flow_run_id,
        FlowRunStatus::Cancelled,
    )
    .await?;
    let coordinator_b = HumanTaskCoordinator::new(
        Arc::clone(&workflow_runs),
        forms,
        Arc::clone(&human_tasks),
        engine.clone(),
        Duration::from_millis(5),
        100,
    )?;
    let (first, second) = tokio::join!(coordinator.run_once(100), coordinator_b.run_once(100));
    let first = first?;
    let second = second?;
    assert!(first.cancellation_failures.is_empty(), "{first:#?}");
    assert!(second.cancellation_failures.is_empty(), "{second:#?}");
    assert_eq!(first.cancelled_tasks + second.cancelled_tasks, 1);
    assert_eq!(first.expired_tasks + second.expired_tasks, 0);

    let cancelled = human_tasks
        .find_task(authorities.organization_id, ready.task.id)
        .await?
        .expect("cancelled HumanTask");
    assert_eq!(cancelled.task.status, HumanTaskStatus::Cancelled);
    let decision = human_tasks
        .find_decision(
            authorities.organization_id,
            cancelled
                .task
                .decision_id
                .expect("cancellation decision id"),
        )
        .await?
        .expect("cancellation decision");
    assert_eq!(decision.decision.outcome, WorkflowDecisionOutcome::Cancel);
    assert_eq!(decision.decision.decided_by, authorities.reviewer);
    assert!(decision.resume_receipt.is_none());

    let worker = HumanTaskResumeWorker::new(
        Arc::clone(&human_tasks),
        engine.clone(),
        HumanTaskResumeWorkerConfig {
            batch_size: 10,
            poll_interval: Duration::from_millis(5),
            lease_duration: Duration::from_secs(5),
            flow_operation_timeout: Duration::from_secs(1),
            initial_backoff: Duration::from_millis(5),
            maximum_backoff: Duration::from_secs(1),
        },
    )?;
    let settled = worker.run_once().await?;
    assert_eq!(settled.claimed, 1, "{settled:#?}");
    assert_eq!(settled.superseded, 1, "{settled:#?}");
    assert_eq!(settled.conflicted, 0, "{settled:#?}");
    assert!(settled.failures.is_empty(), "{settled:#?}");

    let decision = human_tasks
        .find_decision(authorities.organization_id, decision.decision.id)
        .await?
        .expect("settled cancellation decision");
    let receipt = decision
        .resume_receipt
        .as_ref()
        .expect("RunCancelled receipt");
    assert_eq!(receipt.disposition(), FlowResumeDisposition::RunCancelled);
    assert_eq!(
        receipt.cancellation_reason(),
        Some("operator cancelled parent run")
    );
    assert_eq!(receipt.flow_event_at(), decision.decision.decided_at);

    let database = Database::new(PostgresDialect, executor.clone());
    let state = database
        .fetch_one_as(
            sql_query::<(String, String, Uuid, Uuid)>(
                "select delivery.state, receipt.disposition, decision.decided_by, run.cancellation_requested_by from workflow_resume_outbox delivery join workflow_resume_receipts receipt on receipt.organization_id = delivery.organization_id and receipt.workflow_decision_id = delivery.workflow_decision_id join workflow_decisions decision on decision.organization_id = delivery.organization_id and decision.id = delivery.workflow_decision_id join workflow_runs run on run.organization_id = delivery.organization_id and run.id = delivery.workflow_run_id where delivery.organization_id = ",
            )
            .bind(authorities.organization_id.as_uuid())
            .append(" and delivery.human_task_id = ")
            .bind(cancelled.task.id.as_uuid()),
        )
        .await?;
    assert_eq!(
        state,
        (
            "delivered".into(),
            "run_cancelled".into(),
            authorities.reviewer.as_uuid(),
            authorities.reviewer.as_uuid(),
        )
    );
    let replay = coordinator_b.run_once(100).await?;
    assert_eq!(replay.inspected_cancellations, 0, "{replay:#?}");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn exercise_expiry_after_flow_timeout(
    executor: &a3s_orm::PostgresExecutor,
    authorities: Authorities,
    base_input: &WorkflowRunInput,
    forms: Arc<dyn IFormRepository>,
    workflow_runs: Arc<dyn IWorkflowRunRepository>,
    human_tasks: Arc<dyn IHumanTaskRepository>,
    flow_coordinator: &FlowOperationCoordinator,
    workflow_reconciler: &WorkflowRunReconciler,
    engine: &a3s_flow::FlowEngine,
) -> TestResult<()> {
    let requested_at = canonical_timestamp(Utc::now());
    let deadline_at = requested_at + ChronoDuration::seconds(5);
    let mut input = base_input.clone();
    input.workflow_run_id = WorkflowRunId::new();
    input.requested_at = requested_at;
    input.deadline_at = deadline_at;
    input.validate()?;
    let flow_run_id = input.workflow_run_id.to_string();
    let (run, steps) = WorkflowRun::create(input.clone(), authorities.actor)?;
    let requested = WorkflowRunRecord { run, steps };
    workflow_runs
        .create(CreateWorkflowRunWrite {
            event: WorkflowRunRequested::envelope(&requested.run, Uuid::now_v7())?,
            record: requested,
            actor_principal_id: authorities.actor,
            request_id: Uuid::now_v7(),
            idempotency: idempotency(
                "postgres-human-task-flow-expiry-runs",
                "start",
                input.workflow_run_id.to_string().as_bytes(),
            ),
        })
        .await?;

    drive_flow_until(
        flow_coordinator,
        workflow_reconciler,
        engine,
        &flow_run_id,
        FlowRunStatus::Suspended,
    )
    .await?;
    let coordinator = HumanTaskCoordinator::new(
        Arc::clone(&workflow_runs),
        forms,
        Arc::clone(&human_tasks),
        engine.clone(),
        Duration::from_millis(5),
        100,
    )?;
    let created = coordinator.run_once_at(100, requested_at).await?;
    assert!(created.failures.is_empty(), "{created:#?}");
    assert!(created.expiry_failures.is_empty(), "{created:#?}");
    assert_eq!(created.created_tasks, 1, "{created:#?}");
    assert_eq!(created.activated_tasks, 1, "{created:#?}");
    assert_eq!(created.expired_tasks, 0, "{created:#?}");

    let ready = human_tasks
        .list_tasks(
            authorities.organization_id,
            authorities.project_id,
            Some(HumanTaskStatus::Ready),
            10,
        )
        .await?;
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].task.workflow_run_id, input.workflow_run_id);
    assert_eq!(ready[0].task.expires_at, Some(deadline_at));

    let remaining = (deadline_at - Utc::now())
        .to_std()
        .unwrap_or_default()
        .saturating_add(Duration::from_millis(25));
    tokio::time::sleep(remaining).await;
    let timeout_report = workflow_reconciler.run_once(100).await?;
    assert!(timeout_report.failures.is_empty(), "{timeout_report:#?}");
    let timeout_snapshot = engine.snapshot(&flow_run_id).await?;
    assert!(matches!(
        timeout_snapshot.terminal_outcome,
        Some(a3s_flow::WorkflowTerminalOutcome::TimedOut { deadline, .. }) if deadline == deadline_at
    ));

    let expired = coordinator
        .run_once_at(100, deadline_at + ChronoDuration::milliseconds(1))
        .await?;
    assert!(expired.failures.is_empty(), "{expired:#?}");
    assert!(expired.expiry_failures.is_empty(), "{expired:#?}");
    assert_eq!(expired.expired_tasks, 1, "{expired:#?}");
    assert_eq!(expired.contended_expirations, 0, "{expired:#?}");

    let worker = HumanTaskResumeWorker::new(
        Arc::clone(&human_tasks),
        engine.clone(),
        HumanTaskResumeWorkerConfig {
            batch_size: 10,
            poll_interval: Duration::from_millis(5),
            lease_duration: Duration::from_secs(5),
            flow_operation_timeout: Duration::from_secs(1),
            initial_backoff: Duration::from_millis(5),
            maximum_backoff: Duration::from_secs(1),
        },
    )?;
    let settled = worker.run_once().await?;
    assert_eq!(settled.claimed, 1, "{settled:#?}");
    assert_eq!(settled.delivered, 0, "{settled:#?}");
    assert_eq!(settled.superseded, 1, "{settled:#?}");
    assert_eq!(settled.conflicted, 0, "{settled:#?}");
    assert!(settled.failures.is_empty(), "{settled:#?}");

    let task = human_tasks
        .find_task(authorities.organization_id, ready[0].task.id)
        .await?
        .expect("expired HumanTask");
    assert_eq!(task.task.status, HumanTaskStatus::Expired);
    let decision = human_tasks
        .find_decision(
            authorities.organization_id,
            task.task.decision_id.expect("expiry decision id"),
        )
        .await?
        .expect("expiry decision");
    assert!(decision.submission.is_none());
    assert_eq!(decision.decision.decided_at, deadline_at);
    assert_eq!(
        decision
            .resume_receipt
            .as_ref()
            .expect("terminal receipt")
            .disposition(),
        FlowResumeDisposition::RunTimedOut
    );
    let run = workflow_runs
        .find(authorities.organization_id, input.workflow_run_id)
        .await?
        .expect("timed-out WorkflowRun");
    assert_eq!(
        run.run.status,
        a3s_cloud_control_plane::modules::workflow::WorkflowRunStatus::TimedOut
    );

    let database = Database::new(PostgresDialect, executor.clone());
    let state = database
        .fetch_one_as(
            sql_query::<(String, String, i64)>(
                "select delivery.state, receipt.disposition, (select count(*) from form_submissions submission where submission.organization_id = delivery.organization_id and submission.human_task_id = delivery.human_task_id) from workflow_resume_outbox delivery join workflow_resume_receipts receipt on receipt.organization_id = delivery.organization_id and receipt.workflow_decision_id = delivery.workflow_decision_id where delivery.organization_id = ",
            )
            .bind(authorities.organization_id.as_uuid())
            .append(" and delivery.human_task_id = ")
            .bind(task.task.id.as_uuid()),
        )
        .await?;
    assert_eq!(state, ("delivered".into(), "run_timed_out".into(), 0));
    Ok(())
}

async fn drive_flow_until(
    flow_coordinator: &FlowOperationCoordinator,
    workflow_reconciler: &WorkflowRunReconciler,
    engine: &a3s_flow::FlowEngine,
    run_id: &str,
    expected: FlowRunStatus,
) -> TestResult<()> {
    for _ in 0..12 {
        flow_coordinator.run_once().await?;
        let report = workflow_reconciler.run_once(100).await?;
        assert!(report.failures.is_empty(), "{report:#?}");
        if engine.snapshot(run_id).await?.status == expected {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let snapshot = engine.snapshot(run_id).await?;
    Err(format!(
        "WorkflowRun did not reach {expected:?}; status={:?}, sequence={}, hooks={:?}, steps={:?}",
        snapshot.status, snapshot.last_sequence, snapshot.hooks, snapshot.steps
    )
    .into())
}

async fn publish_interaction_form(
    repository: Arc<dyn IFormRepository>,
    authorities: Authorities,
    created_at: DateTime<Utc>,
) -> TestResult<FormRelease> {
    publish_interaction_form_document(
        repository,
        authorities,
        created_at,
        interaction_form_document()?,
    )
    .await
}

async fn publish_interaction_form_document(
    repository: Arc<dyn IFormRepository>,
    authorities: Authorities,
    created_at: DateTime<Utc>,
    document: FormDocument,
) -> TestResult<FormRelease> {
    let draft = FormDraft::create(
        authorities.organization_id,
        authorities.project_id,
        authorities.form_id,
        "Human review".into(),
        "Authority-bound workflow approval".into(),
        document,
        authorities.actor,
        created_at,
    )?;
    repository
        .create_draft(CreateFormDraftWrite {
            event: FormDraftChanged::created(&draft, Uuid::now_v7())?,
            draft: draft.clone(),
            actor_principal_id: authorities.actor,
            request_id: Uuid::now_v7(),
            idempotency: idempotency(
                "postgres-human-task-flow-forms",
                "create",
                authorities.form_id.to_string().as_bytes(),
            ),
        })
        .await?;
    let release = FormRelease::publish(
        &draft,
        authorities.form_release_id,
        compile_form_content(&draft.document)?,
        authorities.actor,
        created_at,
    )?;
    let published = draft.record_release(1, &release)?;
    repository
        .publish_release(PublishFormReleaseWrite {
            event: FormReleasePublished::envelope(&published, &release, Uuid::now_v7())?,
            publication: FormPublicationRecord {
                draft: published,
                release: release.clone(),
            },
            expected_version: 1,
            actor_principal_id: authorities.actor,
            request_id: Uuid::now_v7(),
            idempotency: idempotency(
                "postgres-human-task-flow-form-releases",
                "publish",
                authorities.form_release_id.to_string().as_bytes(),
            ),
        })
        .await?;
    Ok(release)
}

fn interaction_form_document() -> Result<FormDocument, String> {
    let encoded = serde_json::to_vec(&serde_json::json!({
        "kind": "a3s.form",
        "apiVersion": "a3s.dev/form/v1alpha1",
        "revision": 1,
        "metadata": { "title": "Human review" },
        "schema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {
                "approved": { "type": "boolean" },
                "note": { "type": "string" }
            },
            "required": ["approved"],
            "additionalProperties": false
        },
        "ui": {
            "root": "root",
            "nodes": [
                { "id": "root", "kind": "root", "children": ["approved", "note"] },
                {
                    "id": "approved",
                    "kind": "field",
                    "schemaPath": "/properties/approved",
                    "widget": "switch"
                },
                {
                    "id": "note",
                    "kind": "field",
                    "schemaPath": "/properties/note",
                    "widget": "text"
                }
            ]
        },
        "rules": [],
        "dataSources": [],
        "actions": []
    }))
    .map_err(|error| error.to_string())?;
    FormDocument::parse(&encoded)
}

fn drifted_interaction_form_document() -> Result<FormDocument, String> {
    let document = interaction_form_document()?;
    let mut value: serde_json::Value =
        serde_json::from_str(document.canonical_json()).map_err(|error| error.to_string())?;
    value["metadata"]["title"] = serde_json::json!("Drifted human review");
    let encoded = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    FormDocument::parse(&encoded)
}

fn compile_form_content(document: &FormDocument) -> Result<FormReleaseContent, String> {
    let document: serde_json::Value =
        serde_json::from_str(document.canonical_json()).map_err(|error| error.to_string())?;
    let request = serde_json::to_vec(&serde_json::json!({
        "apiVersion": COMPILE_REQUEST_API_VERSION,
        "document": document,
    }))
    .map_err(|error| error.to_string())?;
    let response = compile_bytes(&request).map_err(|error| error.to_string())?;
    let response: serde_json::Value =
        serde_json::from_slice(&response).map_err(|error| error.to_string())?;
    if response["ok"] != true {
        return Err(format!("Form compiler rejected HumanTask form: {response}"));
    }
    let plan = canonicalize_json(
        &serde_json::to_vec(&response["formPlan"]).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    FormReleaseContent::restore(
        response["normalizedDocumentJson"]
            .as_str()
            .ok_or_else(|| "Form compiler omitted normalizedDocumentJson".to_owned())?
            .to_owned(),
        String::from_utf8(plan).map_err(|error| error.to_string())?,
        response["compilerRevision"]
            .as_str()
            .ok_or_else(|| "Form compiler omitted compilerRevision".to_owned())?
            .to_owned(),
        response["schemaProfile"]
            .as_str()
            .ok_or_else(|| "Form compiler omitted schemaProfile".to_owned())?
            .to_owned(),
        response["digest"]
            .as_str()
            .ok_or_else(|| "Form compiler omitted digest".to_owned())?,
    )
}

fn accepted_submission(
    record: &a3s_cloud_control_plane::modules::workflow::HumanTaskRecord,
    principal_id: PrincipalId,
    submitted_at: DateTime<Utc>,
) -> Result<FormSubmission, String> {
    let request = record
        .interaction_request
        .clone()
        .ok_or_else(|| "claimed HumanTask has no interaction request".to_owned())?;
    let value = parse_json(br#"{"approved":true,"note":"approved in PostgreSQL + Flow"}"#)
        .map_err(|error| error.to_string())?;
    let submission_id = FormSubmissionId::new();
    let submission = FormInteractionSubmission {
        api_version: FORM_INTERACTION_SUBMISSION_API_VERSION.into(),
        submission_id: submission_id.to_string(),
        request_id: request.request_id.clone(),
        request_digest: request.digest.clone(),
        identity: request.identity.clone(),
        form: request.form.clone(),
        assignment: FormInteractionSubmissionAssignment {
            policy_id: request.assignment.policy_id.clone(),
            policy_revision: request.assignment.policy_revision,
            policy_digest: request.assignment.policy_digest.clone(),
        },
        task_version: record.task.aggregate_version,
        principal_id: principal_id.to_string(),
        outcome: FormInteractionOutcome::Approve,
        idempotency_key: format!("approve-{}", record.task.id),
        submitted_at: form_timestamp(submitted_at),
        value: value.clone(),
        value_digest: digest_interaction_value(&value).map_err(|error| error.to_string())?,
    };
    FormSubmission::accept(AcceptedFormSubmission {
        organization_id: record.task.organization_id,
        project_id: record.task.project_id,
        id: submission_id,
        workflow_run_id: record.task.workflow_run_id,
        human_task_id: record.task.id,
        principal_id,
        authorization_decision: AuthorizationDecisionRef::new(
            format!("human-task-review-{}", record.task.id),
            Sha256Digest::parse(sha('d'))?,
        )?,
        request,
        submission,
        accepted_value: value,
        accepted_at: transition_time(submitted_at),
    })
}

fn human_decision_input(
    authorities: Authorities,
    release: &FormRelease,
    requested_at: DateTime<Utc>,
) -> Result<WorkflowRunInput, String> {
    let goal_input = serde_json::json!({
        "requestId": "REQ-PG-FLOW-42",
        "amount": 1250,
    });
    let input_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &goal_input,
        1024 * 1024,
        "PostgreSQL HumanTask Flow test input",
    )?))?;
    let data_schema =
        WorkflowPayload::from_content(WorkflowPayloadContent::DataSchema(WorkflowDataSchema {
            value_type: WorkflowDataType::Any,
            fields: Vec::new(),
        }))?;
    let input_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Input),
        ))?;
    let mut decision_configuration =
        WorkflowStepConfiguration::empty(WorkflowStepKind::HumanDecision);
    decision_configuration.message = Some("Approve request REQ-PG-FLOW-42?".into());
    decision_configuration.details =
        Some("This decision must cross the durable PostgreSQL HumanTask resume boundary.".into());
    decision_configuration.expires_after_seconds = Some(3_600);
    let decision_configuration = WorkflowPayload::from_content(
        WorkflowPayloadContent::Configuration(decision_configuration),
    )?;
    let output_configuration =
        WorkflowPayload::from_content(WorkflowPayloadContent::Configuration(
            WorkflowStepConfiguration::empty(WorkflowStepKind::Output),
        ))?;

    let schema_digest = data_schema.digest().clone();
    let mut payloads = vec![
        data_schema,
        input_configuration.clone(),
        decision_configuration.clone(),
        output_configuration.clone(),
    ];
    payloads.sort_by(|left, right| left.digest().cmp(right.digest()));
    let payload_set_digest = digest_payload_set(&payloads)?;
    let mut human_step = plan_step(
        HUMAN_STEP_ID,
        WorkflowStepKind::HumanDecision,
        &decision_configuration,
        &schema_digest,
    );
    human_step.capability = Some(CapabilityReference {
        owner: CapabilityOwner::Forms,
        capability_type: CapabilityType::FormRelease,
        resource_id: release.form_id.as_uuid(),
        revision: release.id.to_string(),
        digest: release.content.digest().clone(),
        capability: "form.interact".into(),
    });
    let plan = WorkflowPlan {
        schema: WORKFLOW_PLAN_SCHEMA.into(),
        compiler_revision: WORKFLOW_PLAN_COMPILER_REVISION.into(),
        workflow_definition_id: WorkflowDefinitionId::new(),
        workflow_revision_id: WorkflowRevisionId::new(),
        workflow_digest: Sha256Digest::parse(sha('1'))?,
        workflow_payload_set_digest: payload_set_digest,
        semantic_contract_set_digest: None,
        variable_contract_digest: None,
        composite_regions_digest: None,
        ontology_id: OntologyId::new(),
        ontology_revision_id: OntologyRevisionId::new(),
        ontology_digest: Sha256Digest::parse(sha('2'))?,
        environment_id: None,
        input_digest,
        steps: vec![
            plan_step(
                "input",
                WorkflowStepKind::Input,
                &input_configuration,
                &schema_digest,
            ),
            human_step,
            plan_step(
                "output",
                WorkflowStepKind::Output,
                &output_configuration,
                &schema_digest,
            ),
        ],
        edges: vec![
            edge("input-human-review", "input", HUMAN_STEP_ID),
            edge("human-review-output", HUMAN_STEP_ID, "output"),
        ],
    };
    plan.validate()?;
    let plan_digest = Sha256Digest::parse(sha256_digest(&canonical_json_bounded(
        &plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "PostgreSQL HumanTask Flow test plan",
    )?))?;
    let input = WorkflowRunInput {
        schema: WORKFLOW_RUN_INPUT_SCHEMA.into(),
        runtime_contract_revision: WORKFLOW_RUN_RUNTIME_CONTRACT_REVISION.into(),
        flow_workflow_name: WORKFLOW_RUN_FLOW_NAME.into(),
        flow_workflow_version: WORKFLOW_RUN_FLOW_VERSION.into(),
        organization_id: authorities.organization_id,
        project_id: authorities.project_id,
        workflow_run_id: WorkflowRunId::new(),
        workflow_goal_id: WorkflowGoalId::new(),
        plan_revision_id: PlanRevisionId::new(),
        plan_digest,
        plan,
        goal_input,
        payloads: payloads
            .iter()
            .map(ResolvedWorkflowPayload::from_payload)
            .collect(),
        variable_contract: None,
        variable_defaults: None,
        composite_regions: None,
        requested_at,
        deadline_at: requested_at + ChronoDuration::hours(1),
    };
    input.validate()?;
    Ok(input)
}

fn plan_step(
    id: &str,
    kind: WorkflowStepKind,
    configuration: &WorkflowPayload,
    schema_digest: &Sha256Digest,
) -> WorkflowPlanStep {
    WorkflowPlanStep {
        id: id.into(),
        kind,
        configuration_digest: configuration.digest().clone(),
        input_schema_digest: schema_digest.clone(),
        output_schema_digest: schema_digest.clone(),
        policy_digest: None,
        capability: None,
        descriptor: None,
        failure: None,
        default_output: None,
    }
}

fn edge(id: &str, source: &str, target: &str) -> WorkflowEdgeSpec {
    WorkflowEdgeSpec {
        id: id.into(),
        source: source.into(),
        target: target.into(),
        source_handle: None,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PayloadDigestEntry<'a> {
    kind: &'a str,
    schema: &'a str,
    digest: &'a str,
}

fn digest_payload_set(payloads: &[WorkflowPayload]) -> Result<Sha256Digest, String> {
    let entries = payloads
        .iter()
        .map(|payload| PayloadDigestEntry {
            kind: payload.kind().as_str(),
            schema: payload.schema(),
            digest: payload.digest().as_str(),
        })
        .collect::<Vec<_>>();
    let encoded = serde_json::to_vec(&entries).map_err(|error| error.to_string())?;
    Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(encoded)))
}

async fn seed_workflow_authority(
    executor: &a3s_orm::PostgresExecutor,
    input: &WorkflowRunInput,
    actor: PrincipalId,
) -> TestResult<()> {
    let input = input.clone();
    let canonical_plan = String::from_utf8(canonical_json_bounded(
        &input.plan,
        WORKFLOW_PLAN_MAX_BYTES,
        "PostgreSQL HumanTask Flow seeded plan",
    )?)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let database = SeedTransaction::new(transaction);
                database
                    .execute(
                        sql_query::<()>("insert into ontologies (organization_id, project_id, id, name, name_key, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                            .bind(input.organization_id.as_uuid())
                            .append(", ")
                            .bind(input.project_id.as_uuid())
                            .append(", ")
                            .bind(input.plan.ontology_id.as_uuid())
                            .append(", 'HumanTask Flow ontology', 'human-task-flow-ontology', '', ")
                            .bind(input.plan.ontology_revision_id.as_uuid())
                            .append(", 1, ")
                            .bind(input.plan.ontology_digest.as_str())
                            .append(", 1, ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(input.requested_at)
                            .append(", ")
                            .bind(input.requested_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into ontology_revisions (organization_id, project_id, ontology_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, migration_policy, migration_rule_id, migration_digest, created_by, created_at) values (")
                            .bind(input.organization_id.as_uuid())
                            .append(", ")
                            .bind(input.project_id.as_uuid())
                            .append(", ")
                            .bind(input.plan.ontology_id.as_uuid())
                            .append(", ")
                            .bind(input.plan.ontology_revision_id.as_uuid())
                            .append(", 1, null, null, 'cloud.workflow.ontology.v1', 1, 'ontology \"human_task_flow\" {}', ")
                            .bind(input.plan.ontology_digest.as_str())
                            .append(", 'initial', null, null, ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(input.requested_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_definitions (organization_id, project_id, id, name, name_key, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                            .bind(input.organization_id.as_uuid())
                            .append(", ")
                            .bind(input.project_id.as_uuid())
                            .append(", ")
                            .bind(input.plan.workflow_definition_id.as_uuid())
                            .append(", 'HumanTask Flow', 'human-task-flow', '', ")
                            .bind(input.plan.workflow_revision_id.as_uuid())
                            .append(", 1, ")
                            .bind(input.plan.workflow_digest.as_str())
                            .append(", 1, ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(input.requested_at)
                            .append(", ")
                            .bind(input.requested_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_revisions (organization_id, project_id, workflow_definition_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, payload_set_digest, created_by, created_at) values (")
                            .bind(input.organization_id.as_uuid())
                            .append(", ")
                            .bind(input.project_id.as_uuid())
                            .append(", ")
                            .bind(input.plan.workflow_definition_id.as_uuid())
                            .append(", ")
                            .bind(input.plan.workflow_revision_id.as_uuid())
                            .append(", 1, null, null, 'cloud.workflow.definition.v1', 1, 'workflow \"human_task_flow\" {}', ")
                            .bind(input.plan.workflow_digest.as_str())
                            .append(", ")
                            .bind(input.plan.workflow_payload_set_digest.as_str())
                            .append(", ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(input.requested_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_goals (organization_id, project_id, id, name, contract_schema, canonical_acl, contract_digest, input_digest, workflow_definition_id, workflow_revision_id, workflow_digest, ontology_id, ontology_revision_id, ontology_digest, environment_id, plan_revision_id, plan_digest, created_by, created_at) values (")
                            .bind(input.organization_id.as_uuid())
                            .append(", ")
                            .bind(input.project_id.as_uuid())
                            .append(", ")
                            .bind(input.workflow_goal_id.as_uuid())
                            .append(", 'HumanTask Flow goal', 'cloud.workflow.goal.v1', 'goal \"human_task_flow\" {}', ")
                            .bind(sha('3'))
                            .append(", ")
                            .bind(input.plan.input_digest.as_str())
                            .append(", ")
                            .bind(input.plan.workflow_definition_id.as_uuid())
                            .append(", ")
                            .bind(input.plan.workflow_revision_id.as_uuid())
                            .append(", ")
                            .bind(input.plan.workflow_digest.as_str())
                            .append(", ")
                            .bind(input.plan.ontology_id.as_uuid())
                            .append(", ")
                            .bind(input.plan.ontology_revision_id.as_uuid())
                            .append(", ")
                            .bind(input.plan.ontology_digest.as_str())
                            .append(", null, ")
                            .bind(input.plan_revision_id.as_uuid())
                            .append(", ")
                            .bind(input.plan_digest.as_str())
                            .append(", ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(input.requested_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_plan_revisions (organization_id, project_id, workflow_goal_id, id, plan_schema, compiler_revision, canonical_plan, plan_digest, created_by, created_at) values (")
                            .bind(input.organization_id.as_uuid())
                            .append(", ")
                            .bind(input.project_id.as_uuid())
                            .append(", ")
                            .bind(input.workflow_goal_id.as_uuid())
                            .append(", ")
                            .bind(input.plan_revision_id.as_uuid())
                            .append(", 'cloud.workflow.plan.v1', 'cloud.workflow.plan-compiler.v1', ")
                            .bind(canonical_plan)
                            .append(", ")
                            .bind(input.plan_digest.as_str())
                            .append(", ")
                            .bind(actor.as_uuid())
                            .append(", ")
                            .bind(input.requested_at)
                            .append(")"),
                    )
                    .await?;
                Ok::<(), a3s_orm::DatabaseError<PostgresError>>(())
            })
        })
        .await?;
    Ok(())
}

fn transition_time(previous: DateTime<Utc>) -> DateTime<Utc> {
    let now = canonical_timestamp(Utc::now());
    if now > previous {
        now
    } else {
        previous + ChronoDuration::milliseconds(1)
    }
}

fn canonical_timestamp(value: DateTime<Utc>) -> DateTime<Utc> {
    value - ChronoDuration::nanoseconds(i64::from(value.nanosecond() % 1_000))
}

fn form_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn idempotency(scope: &str, key: &str, content: &[u8]) -> IdempotencyRequest {
    IdempotencyRequest::new(scope, key, content).expect("valid idempotency request")
}

fn sha(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}
