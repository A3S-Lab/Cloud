use a3s_cloud_control_plane::modules::executions::domain::events::{
    ExecutionCancellationRequested, ExecutionRequested, ExecutionTemplatePublished,
};
use a3s_cloud_control_plane::modules::executions::domain::{
    CreateExecution, CreateExecutionTemplateRevision, Execution, ExecutionArtifact,
    ExecutionOutcome, ExecutionProcess, ExecutionResources, ExecutionStatus, ExecutionTemplate,
    ExecutionTemplateDefinition, ExecutionTemplateDefinitionSpec, ExecutionTemplateRevision,
    IExecutionRepository, IExecutionTemplateRepository, TransitionExecution,
    WorkflowExecutionBinding,
};
use a3s_cloud_control_plane::modules::executions::{
    PostgresExecutionRepository, PostgresExecutionTemplateRepository,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, ExecutionTemplateId, ExecutionTemplateRevisionId,
    IdempotencyRequest, OrganizationId, PlanRevisionId, PrincipalId, ProjectId, RepositoryError,
    Sha256Digest, WorkflowRunId,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub async fn exercise_execution_persistence(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    other_organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
) -> TestResult {
    let repository = PostgresExecutionRepository::new(executor.clone());
    let execution = execution(organization_id, project_id, environment_id, Utc::now())?;
    let create = CreateExecution {
        execution: execution.clone(),
        idempotency: IdempotencyRequest::new(
            format!(
                "organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions"
            ),
            "postgres-execution-create",
            b"postgres-execution-request",
        )?,
        event: ExecutionRequested::envelope(&execution, Uuid::now_v7())?,
    };
    let (left, right) = tokio::join!(
        repository.create(create.clone()),
        repository.create(create.clone()),
    );
    let created = [left?, right?];
    assert_eq!(created.iter().filter(|write| write.replayed).count(), 1);
    assert!(created.iter().all(|write| write.execution == execution));

    let mut conflicting = create;
    conflicting.idempotency = IdempotencyRequest::new(
        conflicting.idempotency.scope.clone(),
        conflicting.idempotency.key.clone(),
        b"changed postgres execution request",
    )?;
    assert_eq!(
        repository.create(conflicting).await,
        Err(RepositoryError::IdempotencyConflict)
    );
    assert_eq!(
        repository.find(organization_id, execution.id).await?,
        Some(execution.clone())
    );
    assert_eq!(
        repository.find(other_organization_id, execution.id).await?,
        None
    );
    assert_eq!(
        repository
            .list(organization_id, project_id, environment_id, 10)
            .await?,
        vec![execution.clone()]
    );
    assert!(repository
        .list(other_organization_id, project_id, environment_id, 10)
        .await?
        .is_empty());
    assert!(repository
        .pending_operation_starts(100)
        .await?
        .contains(&execution));

    let mut cancelling = execution.clone();
    let expected_version = cancelling.aggregate_version;
    cancelling.request_cancellation(execution.updated_at + Duration::milliseconds(1))?;
    let cancellation = TransitionExecution {
        execution: cancelling.clone(),
        expected_version,
        idempotency: IdempotencyRequest::new(
            format!(
                "organizations/{organization_id}/executions/{}/cancellation",
                execution.id
            ),
            "postgres-execution-cancel",
            b"postgres-execution-cancellation",
        )?,
        event: ExecutionCancellationRequested::envelope(&cancelling, Uuid::now_v7())?,
    };
    let (left, right) = tokio::join!(
        repository.request_cancellation(cancellation.clone()),
        repository.request_cancellation(cancellation),
    );
    let cancelled = [left?, right?];
    assert_eq!(cancelled.iter().filter(|write| write.replayed).count(), 1);
    assert!(cancelled.iter().all(|write| write.execution == cancelling));

    let expected_version = cancelling.aggregate_version;
    cancelling.begin_cleanup(
        ExecutionOutcome::Cancelled,
        cancelling.updated_at + Duration::milliseconds(1),
    )?;
    let mut cleaning = repository.save(cancelling, expected_version).await?;
    assert_eq!(cleaning.status, ExecutionStatus::CleanupPending);
    let expected_version = cleaning.aggregate_version;
    cleaning.complete_cleanup(cleaning.updated_at + Duration::milliseconds(1))?;
    let completed = repository.save(cleaning, expected_version).await?;
    assert_eq!(completed.status, ExecutionStatus::Cancelled);
    assert!(completed.finished_at.is_some());
    assert!(!repository
        .pending_operation_starts(100)
        .await?
        .contains(&completed));

    let database = Database::new(PostgresDialect, executor.clone());
    let events = database
        .fetch_one_as(
            sql_query::<i64>(
                "select count(*) from outbox_events where aggregate_id = ",
            )
            .bind(execution.id.as_uuid())
            .append(
                " and event_key in ('execution.run.requested', 'execution.run.cancellation-requested')",
            ),
        )
        .await?;
    assert_eq!(events, 2);
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(super) async fn exercise_workflow_execution_persistence(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    other_organization_id: OrganizationId,
    project_id: ProjectId,
    actor: PrincipalId,
    workflow_run_id: WorkflowRunId,
) -> TestResult {
    let database = Database::new(PostgresDialect, executor.clone());
    let environment_id = EnvironmentId::new();
    let created_at = Utc::now();
    database
        .execute(
            sql_query::<()>(
                "insert into environments (organization_id, project_id, id, name, name_key, aggregate_version, created_at) values (",
            )
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(project_id.as_uuid())
            .append(", ")
            .bind(environment_id.as_uuid())
            .append(", 'Workflow executions', 'workflow-executions', 1, ")
            .bind(created_at)
            .append(")"),
        )
        .await?;
    let (plan_revision_id, plan_digest) = database
        .fetch_one_as(
            sql_query::<(Uuid, String)>(
                "select plan_revision_id, plan_digest from workflow_runs where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(workflow_run_id.as_uuid()),
        )
        .await?;

    let template_repository = PostgresExecutionTemplateRepository::new(executor.clone());
    let definition = workflow_execution_template_definition()?;
    let revision = ExecutionTemplateRevision::create(
        organization_id,
        project_id,
        ExecutionTemplateId::new(),
        ExecutionTemplateRevisionId::new(),
        definition,
        actor,
        created_at,
    )?;
    let template_idempotency = IdempotencyRequest::new(
        format!("organizations/{organization_id}/projects/{project_id}/execution-templates"),
        "postgres-workflow-execution-template",
        revision.definition.canonical_acl().as_bytes(),
    )?;
    let publish_request_id = Uuid::now_v7();
    let publish = CreateExecutionTemplateRevision {
        revision: revision.clone(),
        event: ExecutionTemplatePublished::envelope(&revision, publish_request_id)?,
        actor_principal_id: actor,
        request_id: publish_request_id,
        idempotency: template_idempotency.clone(),
    };
    let (left, right) = tokio::join!(
        template_repository.create(publish.clone()),
        template_repository.create(publish.clone()),
    );
    let published = [left?, right?];
    assert_eq!(published.iter().filter(|write| write.replayed).count(), 1);
    assert!(published.iter().all(|write| write.value == revision));
    assert_eq!(
        template_repository
            .replay_create(&template_idempotency)
            .await?
            .map(|write| write.value),
        Some(revision.clone())
    );

    let mut conflicting_publish = publish;
    conflicting_publish.idempotency = IdempotencyRequest::new(
        conflicting_publish.idempotency.scope.clone(),
        conflicting_publish.idempotency.key.clone(),
        b"changed workflow execution template",
    )?;
    assert_eq!(
        template_repository.create(conflicting_publish).await,
        Err(RepositoryError::IdempotencyConflict)
    );
    assert_eq!(
        template_repository
            .find(
                organization_id,
                project_id,
                revision.template_id,
                revision.revision_id,
            )
            .await?,
        Some(revision.clone())
    );
    assert!(template_repository
        .find(
            other_organization_id,
            project_id,
            revision.template_id,
            revision.revision_id,
        )
        .await?
        .is_none());
    assert!(template_repository
        .find(
            organization_id,
            ProjectId::new(),
            revision.template_id,
            revision.revision_id,
        )
        .await?
        .is_none());
    assert_eq!(
        template_repository
            .list(organization_id, project_id, 1)
            .await?,
        vec![revision.clone()]
    );
    assert!(template_repository
        .list(other_organization_id, project_id, 10)
        .await?
        .is_empty());
    assert!(template_repository
        .list(organization_id, project_id, 0)
        .await?
        .is_empty());

    let stored_template = database
        .fetch_one_as(
            sql_query::<(String, String, Uuid)>(
                "select canonical_acl, definition_digest, created_by from execution_template_revisions where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and template_id = ")
            .bind(revision.template_id.as_uuid())
            .append(" and revision_id = ")
            .bind(revision.revision_id.as_uuid()),
        )
        .await?;
    assert_eq!(
        stored_template,
        (
            revision.definition.canonical_acl().to_owned(),
            revision.definition.digest().to_string(),
            actor.as_uuid(),
        )
    );
    let (template_events, template_audits) = database
        .fetch_one_as(
            sql_query::<(i64, i64)>(
                "select (select count(*) from outbox_events where aggregate_id = ",
            )
            .bind(revision.template_id.as_uuid())
            .append(" and event_key = 'execution.template.published'), (select count(*) from audit_records where aggregate_id = ")
            .bind(revision.template_id.as_uuid())
            .append(" and action = 'execution.template.published')"),
        )
        .await?;
    assert_eq!((template_events, template_audits), (1, 1));

    let step_attempt = u64::from(u32::MAX) + 1;
    let binding = WorkflowExecutionBinding {
        workflow_run_id,
        plan_revision_id: PlanRevisionId::from_uuid(plan_revision_id),
        plan_digest: Sha256Digest::parse(plan_digest)?,
        step_id: "execute_release_check".into(),
        step_attempt,
        execution_template_id: revision.template_id,
        execution_template_revision_id: revision.revision_id,
        execution_template_digest: revision.definition.digest().clone(),
    };
    let execution_repository = PostgresExecutionRepository::new(executor.clone());
    let execution = Execution::create_with_workflow(
        organization_id,
        project_id,
        environment_id,
        ExecutionId::new(),
        revision
            .definition
            .materialize(serde_json::json!({"release": "2026.08"}))?,
        Some(binding.clone()),
        created_at + Duration::seconds(1),
    )?;
    let execution_idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{organization_id}/workflow-runs/{workflow_run_id}/steps/{}",
            binding.step_id
        ),
        format!("attempt-{}", binding.step_attempt),
        execution.template_digest.as_bytes(),
    )?;
    let request = CreateExecution {
        execution: execution.clone(),
        idempotency: execution_idempotency,
        event: ExecutionRequested::envelope(&execution, Uuid::now_v7())?,
    };
    let (left, right) = tokio::join!(
        execution_repository.create(request.clone()),
        execution_repository.create(request),
    );
    let created = [left?, right?];
    assert_eq!(created.iter().filter(|write| write.replayed).count(), 1);
    assert!(created.iter().all(|write| write.execution == execution));
    assert_eq!(
        execution_repository
            .find_for_workflow(
                organization_id,
                workflow_run_id,
                &binding.step_id,
                binding.step_attempt,
            )
            .await?,
        Some(execution.clone())
    );
    assert!(execution_repository
        .find_for_workflow(
            other_organization_id,
            workflow_run_id,
            &binding.step_id,
            binding.step_attempt,
        )
        .await?
        .is_none());

    let competing = Execution::create_with_workflow(
        organization_id,
        project_id,
        environment_id,
        ExecutionId::new(),
        revision
            .definition
            .materialize(serde_json::json!({"release": "competing"}))?,
        Some(binding.clone()),
        created_at + Duration::seconds(2),
    )?;
    assert!(matches!(
        execution_repository
            .create(CreateExecution {
                event: ExecutionRequested::envelope(&competing, Uuid::now_v7())?,
                idempotency: IdempotencyRequest::new(
                    "postgres-workflow-execution-unique-step",
                    competing.id.to_string(),
                    competing.template_digest.as_bytes(),
                )?,
                execution: competing,
            })
            .await,
        Err(RepositoryError::Conflict(_))
    ));

    let missing_parent = Execution::create_with_workflow(
        organization_id,
        project_id,
        environment_id,
        ExecutionId::new(),
        revision
            .definition
            .materialize(serde_json::json!({"release": "orphan"}))?,
        Some(WorkflowExecutionBinding {
            workflow_run_id: WorkflowRunId::new(),
            step_id: "orphan_release_check".into(),
            step_attempt: 1,
            ..binding.clone()
        }),
        created_at + Duration::seconds(3),
    )?;
    assert_eq!(
        execution_repository
            .create(CreateExecution {
                event: ExecutionRequested::envelope(&missing_parent, Uuid::now_v7())?,
                idempotency: IdempotencyRequest::new(
                    "postgres-workflow-execution-parent-fk",
                    missing_parent.id.to_string(),
                    missing_parent.template_digest.as_bytes(),
                )?,
                execution: missing_parent,
            })
            .await,
        Err(RepositoryError::NotFound)
    );

    let wrong_plan = Execution::create_with_workflow(
        organization_id,
        project_id,
        environment_id,
        ExecutionId::new(),
        revision
            .definition
            .materialize(serde_json::json!({"release": "wrong-plan"}))?,
        Some(WorkflowExecutionBinding {
            plan_digest: Sha256Digest::parse(format!("sha256:{}", "1".repeat(64)))?,
            step_id: "wrong_plan_release_check".into(),
            step_attempt: 1,
            ..binding.clone()
        }),
        created_at + Duration::seconds(4),
    )?;
    assert_eq!(
        execution_repository
            .create(CreateExecution {
                event: ExecutionRequested::envelope(&wrong_plan, Uuid::now_v7())?,
                idempotency: IdempotencyRequest::new(
                    "postgres-workflow-execution-plan-authority",
                    wrong_plan.id.to_string(),
                    wrong_plan.template_digest.as_bytes(),
                )?,
                execution: wrong_plan,
            })
            .await,
        Err(RepositoryError::NotFound)
    );

    let wrong_template_digest = Execution::create_with_workflow(
        organization_id,
        project_id,
        environment_id,
        ExecutionId::new(),
        revision
            .definition
            .materialize(serde_json::json!({"release": "wrong-template"}))?,
        Some(WorkflowExecutionBinding {
            execution_template_digest: Sha256Digest::parse(format!("sha256:{}", "2".repeat(64)))?,
            step_id: "wrong_template_release_check".into(),
            step_attempt: 1,
            ..binding.clone()
        }),
        created_at + Duration::seconds(5),
    )?;
    assert_eq!(
        execution_repository
            .create(CreateExecution {
                event: ExecutionRequested::envelope(&wrong_template_digest, Uuid::now_v7())?,
                idempotency: IdempotencyRequest::new(
                    "postgres-workflow-execution-template-authority",
                    wrong_template_digest.id.to_string(),
                    wrong_template_digest.template_digest.as_bytes(),
                )?,
                execution: wrong_template_digest,
            })
            .await,
        Err(RepositoryError::NotFound)
    );

    let stored_binding = database
        .fetch_one_as(
            sql_query::<(Uuid, Uuid, String, String, u64, Uuid, Uuid, String)>(
                "select workflow_run_id, workflow_plan_revision_id, workflow_plan_digest, workflow_step_id, workflow_step_attempt, execution_template_id, execution_template_revision_id, execution_template_definition_digest from executions where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and id = ")
            .bind(execution.id.as_uuid()),
        )
        .await?;
    assert_eq!(
        stored_binding,
        (
            binding.workflow_run_id.as_uuid(),
            binding.plan_revision_id.as_uuid(),
            binding.plan_digest.to_string(),
            binding.step_id.clone(),
            binding.step_attempt,
            binding.execution_template_id.as_uuid(),
            binding.execution_template_revision_id.as_uuid(),
            binding.execution_template_digest.to_string(),
        )
    );
    let execution_events = database
        .fetch_one_as(
            sql_query::<i64>("select count(*) from outbox_events where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and event_key = 'execution.run.requested'"),
        )
        .await?;
    assert_eq!(execution_events, 1);

    assert!(database
        .execute(
            sql_query::<()>(
                "update execution_template_revisions set canonical_acl = canonical_acl where organization_id = ",
            )
            .bind(organization_id.as_uuid())
            .append(" and template_id = ")
            .bind(revision.template_id.as_uuid()),
        )
        .await
        .is_err());
    assert!(database
        .execute(
            sql_query::<()>("delete from execution_template_revisions where organization_id = ",)
                .bind(organization_id.as_uuid())
                .append(" and template_id = ")
                .bind(revision.template_id.as_uuid()),
        )
        .await
        .is_err());
    Ok(())
}

fn workflow_execution_template_definition() -> Result<ExecutionTemplateDefinition, String> {
    let artifact_digest = format!("sha256:{}", "e".repeat(64));
    ExecutionTemplateDefinition::from_spec(ExecutionTemplateDefinitionSpec {
        name: "release-check".into(),
        description: "Runs one bounded Workflow release check".into(),
        artifact: ExecutionArtifact {
            uri: format!("oci://registry.example/tasks/release-check@{artifact_digest}"),
            digest: artifact_digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ExecutionProcess {
            command: vec!["/app/release-check".into()],
            args: vec!["verify".into()],
            working_directory: Some("/workspace".into()),
            environment: BTreeMap::from([("MODE".into(), "workflow".into())]),
        },
        resources: ExecutionResources {
            cpu_millis: 250,
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: Some(16 * 1024 * 1024),
            timeout_ms: 30_000,
        },
    })
}

fn execution(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    requested_at: chrono::DateTime<Utc>,
) -> Result<Execution, String> {
    let digest = format!("sha256:{}", "d".repeat(64));
    Execution::create(
        organization_id,
        project_id,
        environment_id,
        ExecutionId::new(),
        ExecutionTemplate {
            artifact: ExecutionArtifact {
                uri: format!("oci://registry.example/tasks/postgres@{digest}"),
                digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ExecutionProcess {
                command: vec!["/app/task".into()],
                args: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
            input: serde_json::json!({"fixture": "postgres"}),
            resources: ExecutionResources {
                cpu_millis: 250,
                memory_bytes: 128 * 1024 * 1024,
                pids: 64,
                ephemeral_storage_bytes: None,
                timeout_ms: 5_000,
            },
        },
        requested_at,
    )
}
