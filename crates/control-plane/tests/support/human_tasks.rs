use crate::migrate_and_connect_for_test;
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AuthorizationDecisionRef, FormId, FormReleaseId, FormSubmissionId, HumanTaskId,
    IdempotencyRequest, OrganizationId, PrincipalId, ProjectId, RepositoryError, Sha256Digest,
    WorkflowDecisionId, WorkflowRunId,
};
use a3s_cloud_control_plane::modules::workflow::{
    AcceptedHumanTaskSubmission, AssignmentPolicyRef, ChangeHumanTaskWrite, CreateHumanTaskWrite,
    DecideHumanTaskWrite, FlowResumePayload, FlowResumeReceipt, HumanTask, HumanTaskDecisionRecord,
    HumanTaskInteractionSpec, HumanTaskRecord, HumanTaskStatus, HumanTaskSubmission,
    IHumanTaskRepository, NewHumanTask, PostgresHumanTaskRepository, WorkflowDecision,
};
use a3s_form_core::{
    digest_interaction_value, parse_json, FormInteractionOutcome, FormInteractionSubmission,
    FormInteractionSubmissionAssignment, FormReleaseMode, FormReleaseRef,
    FORM_INTERACTION_SUBMISSION_API_VERSION, FORM_RELEASE_REF_API_VERSION,
};
use a3s_orm::{
    sql_query, Database, DatabaseError, Executor, PostgresDialect, PostgresError,
    PostgresTransaction, Query,
};
use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

#[path = "human_tasks/end_to_end.rs"]
mod end_to_end;
#[path = "human_tasks/resume_delivery.rs"]
mod resume_delivery;

pub(super) use end_to_end::exercise_human_task_flow_end_to_end;

#[derive(Clone, Copy)]
struct Authorities {
    organization_id: OrganizationId,
    project_id: ProjectId,
    other_organization_id: OrganizationId,
    other_project_id: ProjectId,
    actor: PrincipalId,
    claimant: PrincipalId,
    other_principal: PrincipalId,
    workflow_run_id: WorkflowRunId,
    form_id: FormId,
    form_release_id: FormReleaseId,
}

fn authority_fixture() -> Authorities {
    Authorities {
        organization_id: OrganizationId::new(),
        project_id: ProjectId::new(),
        other_organization_id: OrganizationId::new(),
        other_project_id: ProjectId::new(),
        actor: PrincipalId::new(),
        claimant: PrincipalId::new(),
        other_principal: PrincipalId::new(),
        workflow_run_id: WorkflowRunId::new(),
        form_id: FormId::new(),
        form_release_id: FormReleaseId::new(),
    }
}

pub(super) async fn exercise_human_task_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let authorities = authority_fixture();
    seed_authorities(&executor, authorities).await?;

    let repository = PostgresHumanTaskRepository::new(executor.clone());
    let task_id = HumanTaskId::new();
    let record = task_record(
        authorities,
        task_id,
        "human_review",
        1,
        "human-review",
        timestamp(10, 0),
    )?;
    let create = CreateHumanTaskWrite {
        event: event(
            &record,
            "workflow.human-task.created",
            record.task.created_at,
        ),
        hook_event_digest: sha('9'),
        hook_observed_at: record.task.created_at,
        request_id: Uuid::now_v7(),
        record: record.clone(),
    };
    assert!(!repository.create_from_hook(create.clone()).await?.replayed);
    assert!(repository.create_from_hook(create.clone()).await?.replayed);

    let mut hook_drift = record.clone();
    hook_drift.interaction.message = "A different task for the same hook".into();
    assert!(matches!(
        repository
            .create_from_hook(CreateHumanTaskWrite {
                record: hook_drift,
                ..create.clone()
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    let generation_drift = task_record(
        authorities,
        HumanTaskId::new(),
        "human_review",
        2,
        "different-hook",
        timestamp(10, 0),
    )?;
    assert!(matches!(
        repository
            .create_from_hook(CreateHumanTaskWrite {
                event: event(
                    &generation_drift,
                    "workflow.human-task.created",
                    generation_drift.task.created_at,
                ),
                hook_event_digest: sha('6'),
                hook_observed_at: generation_drift.task.created_at,
                request_id: Uuid::now_v7(),
                record: generation_drift,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert_eq!(
        repository
            .find_task(authorities.other_organization_id, task_id)
            .await?,
        None
    );

    let cross_tenant = task_record(
        Authorities {
            project_id: authorities.other_project_id,
            ..authorities
        },
        HumanTaskId::new(),
        "cross_tenant_review",
        1,
        "cross-tenant",
        timestamp(10, 0),
    )?;
    assert_eq!(
        repository
            .create_from_hook(CreateHumanTaskWrite {
                event: event(
                    &cross_tenant,
                    "workflow.human-task.created",
                    cross_tenant.task.created_at,
                ),
                hook_event_digest: sha('8'),
                hook_observed_at: cross_tenant.task.created_at,
                request_id: Uuid::now_v7(),
                record: cross_tenant,
            })
            .await
            .expect_err("cross-tenant task authority must fail closed"),
        RepositoryError::NotFound
    );

    let mut ready = record.clone();
    ready.activate(1, timestamp(8, 1))?;
    let activate = ChangeHumanTaskWrite {
        event: event(&ready, "workflow.human-task.ready", ready.task.updated_at),
        record: ready.clone(),
        expected_version: 1,
        actor_principal_id: authorities.actor,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("human-task-activate", "activate", b"ready-v2"),
    };
    assert!(!repository.change_task(activate.clone()).await?.replayed);
    assert!(repository.change_task(activate.clone()).await?.replayed);
    assert!(matches!(
        repository
            .change_task(ChangeHumanTaskWrite {
                idempotency: idempotency("human-task-activate", "stale-activate", b"ready-v2"),
                ..activate.clone()
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let mut claimed = ready.clone();
    claimed.claim(2, authorities.claimant, timestamp(8, 2))?;
    let claim = ChangeHumanTaskWrite {
        event: event(
            &claimed,
            "workflow.human-task.claimed",
            claimed.task.updated_at,
        ),
        record: claimed.clone(),
        expected_version: 2,
        actor_principal_id: authorities.claimant,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("human-task-claim", "claim", b"claimed-v3"),
    };
    assert!(repository
        .replay_change(&claim.idempotency)
        .await?
        .is_none());
    assert!(matches!(
        repository
            .change_task(ChangeHumanTaskWrite {
                actor_principal_id: authorities.other_principal,
                idempotency: idempotency("human-task-claim", "wrong-claimant", b"claimed-v3"),
                ..claim.clone()
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(!repository.change_task(claim.clone()).await?.replayed);
    assert_eq!(
        repository
            .replay_change(&claim.idempotency)
            .await?
            .expect("claim replay"),
        claimed
    );
    assert!(matches!(
        repository
            .replay_change(&idempotency("human-task-claim", "claim", b"drifted-claim"))
            .await,
        Err(RepositoryError::IdempotencyConflict)
    ));
    assert!(repository.change_task(claim.clone()).await?.replayed);
    assert!(matches!(
        repository
            .change_task(ChangeHumanTaskWrite {
                idempotency: idempotency("human-task-claim", "claim", b"drifted-claim"),
                ..claim.clone()
            })
            .await,
        Err(RepositoryError::IdempotencyConflict)
    ));

    assert_expired_claim_is_rejected(&repository, authorities).await?;

    let submission = accepted_submission(&claimed, authorities.claimant)?;
    let decision = WorkflowDecision::from_submission(
        WorkflowDecisionId::new(),
        &claimed.task,
        &submission,
        submission.accepted_output()?,
        timestamp(8, 5),
    )?;
    let mut completed = claimed.clone();
    completed.complete(3, &decision)?;
    let decision_record = HumanTaskDecisionRecord {
        task: completed.clone(),
        submission: Some(submission.clone()),
        resume_payload: FlowResumePayload::from_decision(&decision)?,
        resume_receipt: None,
        decision,
    };
    decision_record.validate()?;
    let decide = DecideHumanTaskWrite {
        event: event(
            &completed,
            "workflow.human-task.completed",
            completed.task.updated_at,
        ),
        record: decision_record.clone(),
        expected_version: 3,
        actor_principal_id: authorities.claimant,
        request_id: Uuid::now_v7(),
        idempotency: idempotency("human-task-decide", "approve", b"approved-v4"),
    };
    assert!(matches!(
        repository
            .decide_task(DecideHumanTaskWrite {
                actor_principal_id: authorities.other_principal,
                idempotency: idempotency("human-task-decide", "wrong-decider", b"approved-v4"),
                ..decide.clone()
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(!repository.decide_task(decide.clone()).await?.replayed);
    assert!(repository.decide_task(decide.clone()).await?.replayed);
    assert!(matches!(
        repository
            .decide_task(DecideHumanTaskWrite {
                idempotency: idempotency("human-task-decide", "approve", b"drifted-decision"),
                ..decide.clone()
            })
            .await,
        Err(RepositoryError::IdempotencyConflict)
    ));

    let persisted = repository
        .find_decision(authorities.organization_id, decision_record.decision.id)
        .await?
        .expect("persisted decision");
    assert_eq!(persisted, decision_record);
    assert_eq!(
        repository
            .list_tasks(
                authorities.organization_id,
                authorities.project_id,
                Some(HumanTaskStatus::Completed),
                10,
            )
            .await?,
        vec![completed]
    );

    assert_atomic_decision_rows(&executor, authorities, task_id).await?;
    let delivery_owner = resume_delivery::exercise_resume_delivery_leases(
        &repository,
        authorities,
        &decision_record,
    )
    .await?;

    let payload = &decision_record.resume_payload;
    let receipt = FlowResumeReceipt::from_hook_received(
        payload,
        &payload.flow_run_id,
        &payload.flow_hook_id,
        &payload.to_flow_value()?,
        12,
        Uuid::now_v7(),
        timestamp(8, 7),
    )?;
    let with_receipt = repository
        .record_resume_receipt(
            authorities.organization_id,
            decision_record.decision.id,
            delivery_owner,
            receipt.clone(),
            timestamp(8, 9),
        )
        .await?;
    assert_eq!(with_receipt.resume_receipt, Some(receipt.clone()));
    assert_eq!(
        repository
            .record_resume_receipt(
                authorities.organization_id,
                decision_record.decision.id,
                delivery_owner,
                receipt,
                timestamp(8, 9),
            )
            .await?,
        with_receipt
    );
    let drifted_receipt = FlowResumeReceipt::from_hook_received(
        payload,
        &payload.flow_run_id,
        &payload.flow_hook_id,
        &payload.to_flow_value()?,
        13,
        Uuid::now_v7(),
        timestamp(8, 9),
    )?;
    assert!(matches!(
        repository
            .record_resume_receipt(
                authorities.organization_id,
                decision_record.decision.id,
                delivery_owner,
                drifted_receipt,
                timestamp(8, 10),
            )
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    assert!(repository
        .claim_resume_deliveries(
            Uuid::now_v7(),
            10,
            timestamp(8, 10),
            std::time::Duration::from_secs(60),
        )
        .await?
        .is_empty());
    assert_resume_receipt_rows(&executor, authorities, decision_record.decision.id).await?;
    Ok(())
}

pub(super) async fn exercise_workflow_execution_persistence(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let executor = migrate_and_connect_for_test(&url, 4).await?;
    let authorities = authority_fixture();
    seed_authorities(&executor, authorities).await?;
    super::executions_support::exercise_workflow_execution_persistence(
        &executor,
        authorities.organization_id,
        authorities.other_organization_id,
        authorities.project_id,
        authorities.actor,
        authorities.workflow_run_id,
    )
    .await
}

async fn assert_expired_claim_is_rejected(
    repository: &PostgresHumanTaskRepository,
    authorities: Authorities,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut ready = task_record(
        authorities,
        HumanTaskId::new(),
        "expired_review",
        2,
        "expired-review",
        timestamp(8, 12),
    )?;
    let create = CreateHumanTaskWrite {
        event: event(&ready, "workflow.human-task.created", ready.task.created_at),
        hook_event_digest: sha('7'),
        hook_observed_at: ready.task.created_at,
        request_id: Uuid::now_v7(),
        record: ready.clone(),
    };
    repository.create_from_hook(create).await?;
    ready.activate(1, timestamp(8, 11))?;
    repository
        .change_task(ChangeHumanTaskWrite {
            event: event(&ready, "workflow.human-task.ready", ready.task.updated_at),
            record: ready.clone(),
            expected_version: 1,
            actor_principal_id: authorities.actor,
            request_id: Uuid::now_v7(),
            idempotency: idempotency("human-task-expired", "activate", b"ready-v2"),
        })
        .await?;

    assert!(repository
        .pending_expirations(timestamp(8, 11), 10)
        .await?
        .is_empty());
    assert_eq!(
        repository.pending_expirations(timestamp(8, 12), 10).await?,
        vec![ready.clone()]
    );
    assert!(repository
        .pending_expirations(timestamp(8, 12), 0)
        .await?
        .is_empty());

    let mut forged = ready.clone();
    forged.task.status = HumanTaskStatus::Claimed;
    forged.task.claimed_by = Some(authorities.claimant);
    forged.task.claimed_at = Some(timestamp(8, 12));
    forged.task.updated_at = timestamp(8, 12);
    forged.task.aggregate_version = 3;
    forged.interaction_request = Some(
        forged
            .interaction
            .request_for_claimed_task(&forged.task, authorities.claimant)?,
    );
    forged.validate()?;
    assert!(matches!(
        repository
            .change_task(ChangeHumanTaskWrite {
                event: event(
                    &forged,
                    "workflow.human-task.claimed",
                    forged.task.updated_at,
                ),
                record: forged,
                expected_version: 2,
                actor_principal_id: authorities.claimant,
                request_id: Uuid::now_v7(),
                idempotency: idempotency("human-task-expired", "claim-after-expiry", b"claimed-v3"),
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));
    Ok(())
}

async fn assert_atomic_decision_rows(
    executor: &a3s_orm::PostgresExecutor,
    authorities: Authorities,
    task_id: HumanTaskId,
) -> Result<(), Box<dyn std::error::Error>> {
    let database = Database::new(PostgresDialect, executor.clone());
    let counts = database
        .fetch_one_as(
            sql_query::<(i64, i64, i64, i64, i64, i64, i64)>(
                "select (select count(*) from human_tasks where organization_id = ",
            )
            .bind(authorities.organization_id.as_uuid())
            .append(" and id = ")
            .bind(task_id.as_uuid())
            .append(" and status = 'completed'), (select count(*) from form_submissions where organization_id = ")
            .bind(authorities.organization_id.as_uuid())
            .append(" and human_task_id = ")
            .bind(task_id.as_uuid())
            .append("), (select count(*) from workflow_decisions where organization_id = ")
            .bind(authorities.organization_id.as_uuid())
            .append(" and human_task_id = ")
            .bind(task_id.as_uuid())
            .append("), (select count(*) from workflow_resume_outbox where organization_id = ")
            .bind(authorities.organization_id.as_uuid())
            .append(" and human_task_id = ")
            .bind(task_id.as_uuid())
            .append(" and state = 'pending'), (select count(*) from workflow_human_task_inbox where organization_id = ")
            .bind(authorities.organization_id.as_uuid())
            .append(" and workflow_run_id = ")
            .bind(authorities.workflow_run_id.as_uuid())
            .append(" and event_id = (select hook_event_id from human_tasks where organization_id = ")
            .bind(authorities.organization_id.as_uuid())
            .append(" and id = ")
            .bind(task_id.as_uuid())
            .append(")), (select count(*) from audit_records where organization_id = ")
            .bind(authorities.organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(task_id.as_uuid())
            .append("), (select count(*) from outbox_events where organization_id = ")
            .bind(authorities.organization_id.as_uuid())
            .append(" and aggregate_id = ")
            .bind(task_id.as_uuid())
            .append(")"),
        )
        .await?;
    assert_eq!(counts, (1, 1, 1, 1, 1, 4, 4));
    Ok(())
}

async fn assert_resume_receipt_rows(
    executor: &a3s_orm::PostgresExecutor,
    authorities: Authorities,
    decision_id: WorkflowDecisionId,
) -> Result<(), Box<dyn std::error::Error>> {
    let database = Database::new(PostgresDialect, executor.clone());
    let state = database
        .fetch_one_as(
            sql_query::<(String, i32, i64)>(
                "select delivery.state, delivery.attempt_count, (select count(*) from workflow_resume_receipts receipt where receipt.organization_id = delivery.organization_id and receipt.workflow_decision_id = delivery.workflow_decision_id) from workflow_resume_outbox delivery where delivery.organization_id = ",
            )
            .bind(authorities.organization_id.as_uuid())
            .append(" and delivery.workflow_decision_id = ")
            .bind(decision_id.as_uuid()),
        )
        .await?;
    assert_eq!(state, ("delivered".into(), 3, 1));
    Ok(())
}

fn task_record(
    authorities: Authorities,
    id: HumanTaskId,
    step_id: &str,
    hook_event_sequence: u64,
    hook_id: &str,
    expires_at: DateTime<Utc>,
) -> Result<HumanTaskRecord, String> {
    let due_at = timestamp(9, 0);
    let task = HumanTask::create(NewHumanTask {
        organization_id: authorities.organization_id,
        project_id: authorities.project_id,
        id,
        workflow_run_id: authorities.workflow_run_id,
        step_id: step_id.into(),
        step_attempt: 1,
        form_release: form_release(authorities),
        assignment_policy: AssignmentPolicyRef::new("approval-policy", 1, sha('b'))?,
        flow_run_id: authorities.workflow_run_id.to_string(),
        flow_hook_id: hook_id.into(),
        due_at: (due_at <= expires_at).then_some(due_at),
        expires_at: Some(expires_at),
        created_at: timestamp(8, 0),
    })?;
    HumanTaskRecord::create(
        task,
        HumanTaskInteractionSpec::approval("Approve this change?", None, None)?,
        hook_event_sequence,
        Uuid::now_v7(),
    )
}

fn accepted_submission(
    record: &HumanTaskRecord,
    principal_id: PrincipalId,
) -> Result<HumanTaskSubmission, String> {
    let request = record
        .interaction_request
        .clone()
        .ok_or_else(|| "claimed task has no Form request".to_owned())?;
    let value = parse_json(br#"{"approved":true}"#)
        .map_err(|error| format!("could not create Form value: {error}"))?;
    let id = FormSubmissionId::new();
    let submission = FormInteractionSubmission {
        api_version: FORM_INTERACTION_SUBMISSION_API_VERSION.into(),
        submission_id: id.to_string(),
        request_id: request.request_id.clone(),
        request_digest: request.digest.clone(),
        identity: request.identity.clone(),
        form: request.form.clone(),
        assignment: FormInteractionSubmissionAssignment {
            policy_id: request.assignment.policy_id.clone(),
            policy_revision: request.assignment.policy_revision,
            policy_digest: request.assignment.policy_digest.clone(),
        },
        task_version: request.task.version,
        principal_id: principal_id.to_string(),
        outcome: FormInteractionOutcome::Approve,
        idempotency_key: format!("approve-{}", record.task.id),
        submitted_at: form_timestamp(timestamp(8, 3)),
        value: value.clone(),
        value_digest: digest_interaction_value(&value)
            .map_err(|error| format!("could not digest Form value: {error}"))?,
    };
    HumanTaskSubmission::accept(AcceptedHumanTaskSubmission {
        organization_id: record.task.organization_id,
        project_id: record.task.project_id,
        id,
        workflow_run_id: record.task.workflow_run_id,
        human_task_id: record.task.id,
        principal_id,
        authorization_decision: authorization_reference()?,
        request,
        submission,
        accepted_value: value,
        accepted_at: timestamp(8, 4),
    })
}

fn form_release(authorities: Authorities) -> FormReleaseRef {
    FormReleaseRef {
        api_version: FORM_RELEASE_REF_API_VERSION.into(),
        organization_id: authorities.organization_id.to_string(),
        project_id: authorities.project_id.to_string(),
        form_id: authorities.form_id.to_string(),
        release_id: authorities.form_release_id.to_string(),
        uri: format!(
            "a3s://forms/{}/releases/{}",
            authorities.form_id, authorities.form_release_id
        ),
        revision: 1,
        digest: digest('a'),
        compiler_revision: "a3s-form-core@0.1.0".into(),
        schema_profile: "a3s.dev/form-schema-profile/1".into(),
        mode: FormReleaseMode::Interaction,
    }
}

fn authorization_reference() -> Result<AuthorizationDecisionRef, String> {
    AuthorizationDecisionRef::new("human-task-test-authorization", sha('d'))
}

fn event(record: &HumanTaskRecord, key: &str, occurred_at: DateTime<Utc>) -> DomainEventEnvelope {
    DomainEventEnvelope {
        event_id: Uuid::now_v7(),
        event_key: key.into(),
        schema_version: 1,
        scope: a3s_cloud_contracts::CloudScopeRef::Organization {
            organization_id: record.task.organization_id.as_uuid(),
        },
        aggregate_id: record.task.id.as_uuid(),
        aggregate_version: record.task.aggregate_version,
        occurred_at,
        correlation_id: record.task.workflow_run_id.as_uuid(),
        causation_id: None,
        payload: json!({
            "humanTaskId": record.task.id,
            "status": record.task.status,
        }),
    }
}

fn idempotency(scope: &str, key: &str, content: &[u8]) -> IdempotencyRequest {
    IdempotencyRequest::new(scope, key, content).expect("valid idempotency request")
}

fn sha(character: char) -> Sha256Digest {
    Sha256Digest::parse(digest(character)).expect("valid SHA-256 digest")
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}

fn timestamp(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 10, hour, minute, 0)
        .single()
        .expect("valid timestamp")
}

fn form_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

struct SeedTransaction<'a> {
    transaction: &'a PostgresTransaction,
}

impl<'a> SeedTransaction<'a> {
    const fn new(transaction: &'a PostgresTransaction) -> Self {
        Self { transaction }
    }

    async fn execute<Q>(&self, query: Q) -> Result<(), DatabaseError<PostgresError>>
    where
        Q: Query,
    {
        let query = query
            .compile(&PostgresDialect)
            .map_err(DatabaseError::Build)?;
        self.transaction
            .execute(&query)
            .await
            .map_err(DatabaseError::Execute)?;
        Ok(())
    }
}

async fn seed_authorities(
    executor: &a3s_orm::PostgresExecutor,
    authorities: Authorities,
) -> Result<(), Box<dyn std::error::Error>> {
    let ontology_id = Uuid::now_v7();
    let ontology_revision_id = Uuid::now_v7();
    let workflow_definition_id = Uuid::now_v7();
    let workflow_revision_id = Uuid::now_v7();
    let workflow_goal_id = Uuid::now_v7();
    let plan_revision_id = Uuid::now_v7();
    let created_at = timestamp(7, 0);
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let database = SeedTransaction::new(transaction);
                for (organization_id, name, key) in [
                    (authorities.organization_id, "Human Tasks", "human-tasks"),
                    (
                        authorities.other_organization_id,
                        "Other Human Tasks",
                        "other-human-tasks",
                    ),
                ] {
                    database
                        .execute(
                            sql_query::<()>("insert into organizations (id, name, name_key, aggregate_version, created_at) values (")
                                .bind(organization_id.as_uuid())
                                .append(", ")
                                .bind(name)
                                .append(", ")
                                .bind(key)
                                .append(", 1, ")
                                .bind(created_at)
                                .append(")"),
                        )
                        .await?;
                }
                for (organization_id, project_id, name, key) in [
                    (
                        authorities.organization_id,
                        authorities.project_id,
                        "Approvals",
                        "approvals",
                    ),
                    (
                        authorities.other_organization_id,
                        authorities.other_project_id,
                        "Other Approvals",
                        "other-approvals",
                    ),
                ] {
                    database
                        .execute(
                            sql_query::<()>("insert into projects (organization_id, id, name, name_key, aggregate_version, created_at) values (")
                                .bind(organization_id.as_uuid())
                                .append(", ")
                                .bind(project_id.as_uuid())
                                .append(", ")
                                .bind(name)
                                .append(", ")
                                .bind(key)
                                .append(", 1, ")
                                .bind(created_at)
                                .append(")"),
                        )
                        .await?;
                }
                for (principal_id, name) in [
                    (authorities.actor, "Workflow coordinator"),
                    (authorities.claimant, "Human task claimant"),
                    (authorities.other_principal, "Other claimant"),
                ] {
                    database
                        .execute(
                            sql_query::<()>("insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (")
                                .bind(principal_id.as_uuid())
                                .append(", 'human', ")
                                .bind(name)
                                .append(", 1, ")
                                .bind(created_at)
                                .append(", null)"),
                        )
                        .await?;
                }
                database
                    .execute(
                        sql_query::<()>("insert into ontologies (organization_id, project_id, id, name, name_key, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                            .bind(authorities.organization_id.as_uuid())
                            .append(", ")
                            .bind(authorities.project_id.as_uuid())
                            .append(", ")
                            .bind(ontology_id)
                            .append(", 'Approval ontology', 'approval-ontology', '', ")
                            .bind(ontology_revision_id)
                            .append(", 1, ")
                            .bind(digest('a'))
                            .append(", 1, ")
                            .bind(authorities.actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into ontology_revisions (organization_id, project_id, ontology_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, migration_policy, migration_rule_id, migration_digest, created_by, created_at) values (")
                            .bind(authorities.organization_id.as_uuid())
                            .append(", ")
                            .bind(authorities.project_id.as_uuid())
                            .append(", ")
                            .bind(ontology_id)
                            .append(", ")
                            .bind(ontology_revision_id)
                            .append(", 1, null, null, 'cloud.workflow.ontology.v1', 1, ")
                            .bind("ontology \"approval\" {}")
                            .append(", ")
                            .bind(digest('a'))
                            .append(", 'initial', null, null, ")
                            .bind(authorities.actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_definitions (organization_id, project_id, id, name, name_key, description, current_revision_id, current_revision_number, current_revision_digest, aggregate_version, created_by, created_at, updated_at) values (")
                            .bind(authorities.organization_id.as_uuid())
                            .append(", ")
                            .bind(authorities.project_id.as_uuid())
                            .append(", ")
                            .bind(workflow_definition_id)
                            .append(", 'Approval workflow', 'approval-workflow', '', ")
                            .bind(workflow_revision_id)
                            .append(", 1, ")
                            .bind(digest('b'))
                            .append(", 1, ")
                            .bind(authorities.actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_revisions (organization_id, project_id, workflow_definition_id, id, revision_number, parent_revision_id, parent_digest, contract_schema, compiler_schema_version, canonical_acl, content_digest, payload_set_digest, created_by, created_at) values (")
                            .bind(authorities.organization_id.as_uuid())
                            .append(", ")
                            .bind(authorities.project_id.as_uuid())
                            .append(", ")
                            .bind(workflow_definition_id)
                            .append(", ")
                            .bind(workflow_revision_id)
                            .append(", 1, null, null, 'cloud.workflow.definition.v1', 1, ")
                            .bind("workflow \"approval\" {}")
                            .append(", ")
                            .bind(digest('b'))
                            .append(", ")
                            .bind(digest('c'))
                            .append(", ")
                            .bind(authorities.actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_goals (organization_id, project_id, id, name, contract_schema, canonical_acl, contract_digest, input_digest, workflow_definition_id, workflow_revision_id, workflow_digest, ontology_id, ontology_revision_id, ontology_digest, environment_id, plan_revision_id, plan_digest, created_by, created_at) values (")
                            .bind(authorities.organization_id.as_uuid())
                            .append(", ")
                            .bind(authorities.project_id.as_uuid())
                            .append(", ")
                            .bind(workflow_goal_id)
                            .append(", 'Approval goal', 'cloud.workflow.goal.v1', ")
                            .bind("goal \"approval\" {}")
                            .append(", ")
                            .bind(digest('d'))
                            .append(", ")
                            .bind(digest('e'))
                            .append(", ")
                            .bind(workflow_definition_id)
                            .append(", ")
                            .bind(workflow_revision_id)
                            .append(", ")
                            .bind(digest('b'))
                            .append(", ")
                            .bind(ontology_id)
                            .append(", ")
                            .bind(ontology_revision_id)
                            .append(", ")
                            .bind(digest('a'))
                            .append(", null, ")
                            .bind(plan_revision_id)
                            .append(", ")
                            .bind(digest('f'))
                            .append(", ")
                            .bind(authorities.actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_plan_revisions (organization_id, project_id, workflow_goal_id, id, plan_schema, compiler_revision, canonical_plan, plan_digest, created_by, created_at) values (")
                            .bind(authorities.organization_id.as_uuid())
                            .append(", ")
                            .bind(authorities.project_id.as_uuid())
                            .append(", ")
                            .bind(workflow_goal_id)
                            .append(", ")
                            .bind(plan_revision_id)
                            .append(", 'cloud.workflow.plan.v1', 'cloud.workflow.plan-compiler.v1', '{}', ")
                            .bind(digest('f'))
                            .append(", ")
                            .bind(authorities.actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into operation_requests (operation_id, organization_id, subject_kind, subject_id, workflow_name, workflow_version, input, requested_at) values (")
                            .bind(authorities.workflow_run_id.as_uuid())
                            .append(", ")
                            .bind(authorities.organization_id.as_uuid())
                            .append(", 'workflow_run', ")
                            .bind(authorities.workflow_run_id.as_uuid())
                            .append(", 'cloud.workflow.run', '1', '{}'::jsonb, ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into workflow_runs (organization_id, project_id, id, workflow_goal_id, plan_revision_id, plan_digest, operation_id, flow_run_id, flow_runtime_build_id, execution_input, execution_input_digest, status, last_flow_sequence, output, output_digest, error, aggregate_version, requested_by, requested_at, updated_at, started_at, cancellation_requested_at, cancellation_reason, finished_at) values (")
                            .bind(authorities.organization_id.as_uuid())
                            .append(", ")
                            .bind(authorities.project_id.as_uuid())
                            .append(", ")
                            .bind(authorities.workflow_run_id.as_uuid())
                            .append(", ")
                            .bind(workflow_goal_id)
                            .append(", ")
                            .bind(plan_revision_id)
                            .append(", ")
                            .bind(digest('f'))
                            .append(", ")
                            .bind(authorities.workflow_run_id.as_uuid())
                            .append(", ")
                            .bind(authorities.workflow_run_id.to_string())
                            .append(", null, '{}', ")
                            .bind(digest('e'))
                            .append(", 'waiting', 1, null, null, null, 1, ")
                            .bind(authorities.actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(", ")
                            .bind(created_at)
                            .append(", ")
                            .bind(created_at)
                            .append(", null, null, null)"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into form_drafts (organization_id, project_id, id, name, name_key, description, canonical_document_json, draft_digest, aggregate_version, latest_release_id, created_by, updated_by, created_at, updated_at) values (")
                            .bind(authorities.organization_id.as_uuid())
                            .append(", ")
                            .bind(authorities.project_id.as_uuid())
                            .append(", ")
                            .bind(authorities.form_id.as_uuid())
                            .append(", 'Approval form', 'approval-form', '', '{}', ")
                            .bind(digest('a'))
                            .append(", 1, null, ")
                            .bind(authorities.actor.as_uuid())
                            .append(", ")
                            .bind(authorities.actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                database
                    .execute(
                        sql_query::<()>("insert into form_releases (organization_id, project_id, form_id, id, revision, source_draft_version, name, description, normalized_document_json, form_plan_json, compiler_revision, schema_profile, content_digest, published_by, published_at) values (")
                            .bind(authorities.organization_id.as_uuid())
                            .append(", ")
                            .bind(authorities.project_id.as_uuid())
                            .append(", ")
                            .bind(authorities.form_id.as_uuid())
                            .append(", ")
                            .bind(authorities.form_release_id.as_uuid())
                            .append(", 1, 1, 'Approval form', '', '{}', '{}', 'a3s-form-core@0.1.0', 'a3s.dev/form-schema-profile/1', ")
                            .bind(digest('a'))
                            .append(", ")
                            .bind(authorities.actor.as_uuid())
                            .append(", ")
                            .bind(created_at)
                            .append(")"),
                    )
                    .await?;
                Ok::<(), a3s_orm::DatabaseError<PostgresError>>(())
            })
        })
        .await?;
    Ok(())
}
