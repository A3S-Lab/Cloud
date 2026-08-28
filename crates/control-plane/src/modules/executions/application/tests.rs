use super::{
    CancelExecution, CancelExecutionHandler, CreateExecutionCommand, CreateExecutionHandler,
    ExecutionReconciler, GetExecution, GetExecutionHandler, IWorkflowExecutionPort,
    WorkflowExecutionApplicationService, WorkflowExecutionRequest, EXECUTION_WORKFLOW_NAME,
    EXECUTION_WORKFLOW_VERSION,
};
use crate::modules::executions::domain::events::{ExecutionRequested, ExecutionTemplatePublished};
use crate::modules::executions::domain::{
    CreateExecution, CreateExecutionTemplateRevision, Execution, ExecutionArtifact,
    ExecutionProcess, ExecutionResources, ExecutionStatus, ExecutionTaskAuthority,
    ExecutionTaskPolicy, ExecutionTemplate, ExecutionTemplateDefinition,
    ExecutionTemplateDefinitionSpec, ExecutionTemplateRevision, IExecutionRepository,
    IExecutionTemplateRepository, EXECUTION_TEMPLATE_CAPABILITY,
};
use crate::modules::executions::infrastructure::{
    InMemoryExecutionRepository, InMemoryExecutionTemplateRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::InMemoryOperationRepository;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::projects::infrastructure::persistence::InMemoryProjectsRepository;
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, ExecutionId, ExecutionTemplateId,
    ExecutionTemplateRevisionId, IdempotencyRequest, NodeId, OrganizationId, PlanRevisionId,
    PrincipalId, ProjectId, Sha256Digest, WorkflowRunId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_contracts::{
    artifact_uri, CloudSecretReference, DomainEventEnvelope, DURABLE_CELL_BUNDLE_MEDIA_TYPE,
};
use a3s_runtime::contract::{
    ArtifactRef, RuntimeMount, RuntimeMountSource, SecretReference, SecretTarget,
};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

fn template(value: u64) -> ExecutionTemplate {
    let digest = format!("sha256:{}", "a".repeat(64));
    ExecutionTemplate {
        artifact: ExecutionArtifact {
            uri: format!("oci://registry.example/cloud/function@{digest}"),
            digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ExecutionProcess {
            command: vec!["/app/function".into()],
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
        },
        input: serde_json::json!({"value": value}),
        resources: ExecutionResources {
            cpu_millis: 250,
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: None,
            timeout_ms: 5_000,
        },
    }
}

fn bound_task(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    requested_at: chrono::DateTime<Utc>,
) -> Execution {
    let subject_id = Uuid::now_v7();
    let bundle_digest = format!("sha256:{}", "b".repeat(64));
    Execution::create_bound_task(
        organization_id,
        project_id,
        environment_id,
        ExecutionId::new(),
        template(99),
        NodeId::new(),
        ExecutionTaskPolicy {
            authority: ExecutionTaskAuthority {
                kind: "workload.prestart".into(),
                subject_id,
                digest: Sha256Digest::parse(format!("sha256:{}", "c".repeat(64)))
                    .expect("authority digest"),
            },
            mounts: vec![RuntimeMount {
                name: "application-bundle".into(),
                source: RuntimeMountSource::Artifact {
                    artifact: ArtifactRef {
                        uri: artifact_uri(&bundle_digest).expect("artifact URI"),
                        digest: bundle_digest,
                        media_type: DURABLE_CELL_BUNDLE_MEDIA_TYPE.into(),
                    },
                },
                target: "/workspace/bundle".into(),
                read_only: true,
            }],
            secrets: vec![SecretReference {
                name: "s0-access-key-id".into(),
                reference: CloudSecretReference::new(subject_id, Uuid::now_v7(), 1)
                    .expect("Secret reference")
                    .to_string(),
                target: SecretTarget::Environment {
                    variable: "AWS_ACCESS_KEY_ID".into(),
                },
            }],
            semantics_profile_digest: Sha256Digest::parse(format!("sha256:{}", "d".repeat(64)))
                .expect("semantics digest"),
        },
        requested_at,
    )
    .expect("bound Task")
}

async fn environment() -> (
    OrganizationId,
    ProjectId,
    EnvironmentId,
    Arc<InMemoryProjectsRepository>,
) {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let repository = Arc::new(InMemoryProjectsRepository::new());
    let created_at = Utc::now();
    let environment = Environment::create(
        organization_id,
        project_id,
        environment_id,
        EnvironmentName::parse("Production").expect("environment name"),
        created_at,
    );
    repository
        .create(
            environment,
            DomainEventEnvelope {
                event_id: Uuid::now_v7(),
                event_key: "project.environment.created".into(),
                schema_version: 1,
                scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                    organization_id: organization_id.as_uuid(),
                },
                aggregate_id: environment_id.as_uuid(),
                aggregate_version: 1,
                occurred_at: created_at,
                correlation_id: Uuid::now_v7(),
                causation_id: None,
                payload: serde_json::json!({}),
            },
            IdempotencyRequest::new("test/environments", "environment-1", b"environment")
                .expect("idempotency"),
        )
        .await
        .expect("create environment");
    (organization_id, project_id, environment_id, repository)
}

async fn publish_execution_template(
    repository: &InMemoryExecutionTemplateRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
) -> ExecutionTemplateRevision {
    let source = template(0);
    let revision = ExecutionTemplateRevision::create(
        organization_id,
        project_id,
        ExecutionTemplateId::new(),
        ExecutionTemplateRevisionId::new(),
        ExecutionTemplateDefinition::from_spec(ExecutionTemplateDefinitionSpec {
            name: "workflow-function".into(),
            description: "One bounded Workflow function".into(),
            artifact: source.artifact,
            process: source.process,
            resources: source.resources,
        })
        .expect("template definition"),
        PrincipalId::new(),
        canonical_timestamp(Utc::now()),
    )
    .expect("template revision");
    repository
        .create(CreateExecutionTemplateRevision {
            event: ExecutionTemplatePublished::envelope(&revision, Uuid::now_v7())
                .expect("template event"),
            actor_principal_id: revision.created_by,
            request_id: Uuid::now_v7(),
            idempotency: IdempotencyRequest::new(
                format!(
                    "organizations/{organization_id}/projects/{project_id}/execution-templates"
                ),
                "workflow-function",
                revision.definition.canonical_acl().as_bytes(),
            )
            .expect("template idempotency"),
            revision: revision.clone(),
        })
        .await
        .expect("publish template");
    revision
}

#[tokio::test]
async fn create_and_cancel_are_idempotent_and_emit_cloud_events() {
    let (organization_id, project_id, environment_id, environments) = environment().await;
    let executions = Arc::new(InMemoryExecutionRepository::new());
    let create = CreateExecutionHandler::new(environments, executions.clone());
    let requested_at = Utc::now();
    let command = CreateExecutionCommand {
        organization_id,
        project_id,
        environment_id,
        template: template(1),
        idempotency_key: "invoke-1".into(),
        request_id: Uuid::now_v7(),
        requested_at,
    };
    let first = create
        .execute(command.clone(), context())
        .await
        .expect("framework")
        .expect("create");
    assert!(!first.replayed);
    let replay = create
        .execute(command, context())
        .await
        .expect("framework")
        .expect("replay");
    assert!(replay.replayed);
    assert_eq!(first.execution, replay.execution);

    let changed = create
        .execute(
            CreateExecutionCommand {
                organization_id,
                project_id,
                environment_id,
                template: template(2),
                idempotency_key: "invoke-1".into(),
                request_id: Uuid::now_v7(),
                requested_at,
            },
            context(),
        )
        .await
        .expect("framework");
    assert!(changed.is_err());

    let cancel = CancelExecutionHandler::new(executions.clone());
    let cancellation = CancelExecution {
        organization_id,
        execution_id: first.execution.id,
        resource_access: ResourceAccessEvaluator::organization_wide(),
        idempotency_key: "cancel-1".into(),
        request_id: Uuid::now_v7(),
        requested_at,
    };
    let cancelled = cancel
        .execute(cancellation.clone(), context())
        .await
        .expect("framework")
        .expect("cancel");
    assert!(!cancelled.replayed);
    let replayed = cancel
        .execute(cancellation, context())
        .await
        .expect("framework")
        .expect("cancel replay");
    assert!(replayed.replayed);
    assert_eq!(cancelled.execution, replayed.execution);

    let events = executions.outbox_events().await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_key, "execution.run.requested");
    assert_eq!(events[1].event_key, "execution.run.cancellation-requested");
}

#[tokio::test]
async fn indirect_access_uses_execution_environment_and_authorizes_before_replay() {
    let (organization_id, project_id, environment_id, environments) = environment().await;
    let executions = Arc::new(InMemoryExecutionRepository::new());
    let requested_at = Utc::now();
    let execution = CreateExecutionHandler::new(environments, executions.clone())
        .execute(
            CreateExecutionCommand {
                organization_id,
                project_id,
                environment_id,
                template: template(1),
                idempotency_key: "restricted-invoke".into(),
                request_id: Uuid::now_v7(),
                requested_at,
            },
            context(),
        )
        .await
        .expect("framework")
        .expect("create")
        .execution;
    let environment_access =
        ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
            project_id,
            environment_id,
        }]);
    let project_access =
        ResourceAccessEvaluator::restricted([ResourceGrantScope::Project { project_id }]);
    let revoked_access = ResourceAccessEvaluator::restricted([ResourceGrantScope::Project {
        project_id: ProjectId::new(),
    }]);
    let get = GetExecutionHandler::new(executions.clone());
    for resource_access in [environment_access.clone(), project_access] {
        assert_eq!(
            get.execute(
                GetExecution {
                    organization_id,
                    execution_id: execution.id,
                    resource_access,
                },
                context(),
            )
            .await
            .expect("framework")
            .expect("authorized execution"),
            execution
        );
    }
    let denied = get
        .execute(
            GetExecution {
                organization_id,
                execution_id: execution.id,
                resource_access: revoked_access.clone(),
            },
            context(),
        )
        .await
        .expect("framework")
        .expect_err("revoked access");
    let missing = get
        .execute(
            GetExecution {
                organization_id,
                execution_id: crate::modules::shared_kernel::domain::ExecutionId::new(),
                resource_access: environment_access.clone(),
            },
            context(),
        )
        .await
        .expect("framework")
        .expect_err("missing execution");
    assert_eq!(denied, missing);

    let cancel = CancelExecutionHandler::new(executions);
    let command = CancelExecution {
        organization_id,
        execution_id: execution.id,
        resource_access: environment_access,
        idempotency_key: "restricted-cancel".into(),
        request_id: Uuid::now_v7(),
        requested_at,
    };
    assert!(
        !cancel
            .execute(command.clone(), context())
            .await
            .expect("framework")
            .expect("cancel")
            .replayed
    );
    let replay_after_revocation = cancel
        .execute(
            CancelExecution {
                resource_access: revoked_access,
                ..command
            },
            context(),
        )
        .await
        .expect("framework")
        .expect_err("revocation must block cancellation replay");
    assert_eq!(replay_after_revocation, missing);
}

#[tokio::test]
async fn public_execution_queries_and_cancellation_hide_internal_bound_tasks() {
    let (organization_id, project_id, environment_id, _) = environment().await;
    let executions = Arc::new(InMemoryExecutionRepository::new());
    let requested_at = Utc::now();
    let execution = bound_task(organization_id, project_id, environment_id, requested_at);
    executions
        .create(CreateExecution {
            execution: execution.clone(),
            idempotency: IdempotencyRequest::new(
                "test/executions/internal-bound",
                execution.id.to_string(),
                b"internal-bound",
            )
            .expect("idempotency"),
            event: ExecutionRequested::envelope(&execution, Uuid::now_v7()).expect("event"),
        })
        .await
        .expect("create bound Task");
    let access = ResourceAccessEvaluator::organization_wide();
    let hidden = GetExecutionHandler::new(executions.clone())
        .execute(
            GetExecution {
                organization_id,
                execution_id: execution.id,
                resource_access: access.clone(),
            },
            context(),
        )
        .await
        .expect("framework")
        .expect_err("bound Task must be hidden");
    assert_eq!(
        hidden,
        ApplicationError::NotFound("execution not found".into())
    );
    let cancellation = CancelExecutionHandler::new(executions.clone())
        .execute(
            CancelExecution {
                organization_id,
                execution_id: execution.id,
                resource_access: access,
                idempotency_key: "forbidden-bound-cancel".into(),
                request_id: Uuid::now_v7(),
                requested_at,
            },
            context(),
        )
        .await
        .expect("framework")
        .expect_err("bound Task cancellation must be hidden");
    assert_eq!(cancellation, hidden);
    assert!(executions
        .list(organization_id, project_id, environment_id, 100)
        .await
        .expect("public list")
        .is_empty());
    assert_eq!(
        executions
            .find(organization_id, execution.id)
            .await
            .expect("internal lookup")
            .expect("bound Task")
            .status,
        ExecutionStatus::Queued
    );
}

#[tokio::test]
async fn reconciler_enqueues_the_versioned_execution_workflow_once() {
    let (organization_id, project_id, environment_id, environments) = environment().await;
    let executions = Arc::new(InMemoryExecutionRepository::new());
    let operations = Arc::new(InMemoryOperationRepository::new());
    let created = CreateExecutionHandler::new(environments, executions.clone())
        .execute(
            CreateExecutionCommand {
                organization_id,
                project_id,
                environment_id,
                template: template(1),
                idempotency_key: "invoke-1".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .expect("framework")
        .expect("create")
        .execution;
    let reconciler = ExecutionReconciler::new(executions.clone(), operations.clone());
    let first = reconciler.run_once(100).await.expect("reconcile");
    assert_eq!(first.started, 1);
    assert_eq!(first.replayed, 0);
    assert!(first.failures.is_empty());

    let operation = operations
        .find_request(created.operation_id)
        .await
        .expect("find operation")
        .expect("operation");
    assert_eq!(operation.workflow.name(), EXECUTION_WORKFLOW_NAME);
    assert_eq!(operation.workflow.version(), EXECUTION_WORKFLOW_VERSION);
    assert_eq!(operation.subject.kind(), "execution");
    assert_eq!(operation.subject.id(), created.id.as_uuid());

    let second = reconciler.run_once(100).await.expect("reconcile replay");
    assert_eq!(second.started, 0);
    assert_eq!(second.replayed, 1);
    executions.mark_operation_started(created.id).await;
    assert_eq!(
        reconciler.run_once(100).await.expect("reconciled").started,
        0
    );
}

#[tokio::test]
async fn create_requires_an_existing_environment() {
    let executions: Arc<dyn IExecutionRepository> = Arc::new(InMemoryExecutionRepository::new());
    let environments: Arc<dyn IEnvironmentRepository> = Arc::new(InMemoryProjectsRepository::new());
    let result = CreateExecutionHandler::new(environments, executions)
        .execute(
            CreateExecutionCommand {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                template: template(1),
                idempotency_key: "invoke-1".into(),
                request_id: Uuid::now_v7(),
                requested_at: Utc::now(),
            },
            context(),
        )
        .await
        .expect("framework");
    assert!(result.is_err());
}

#[tokio::test]
async fn workflow_execution_start_adopts_exact_child_and_cancels_idempotently() {
    let (organization_id, project_id, environment_id, environments) = environment().await;
    let templates = Arc::new(InMemoryExecutionTemplateRepository::new());
    let revision = publish_execution_template(&templates, organization_id, project_id).await;
    let executions = Arc::new(InMemoryExecutionRepository::new());
    let service = WorkflowExecutionApplicationService::new(
        environments,
        templates.clone(),
        executions.clone(),
    );
    let requested_at = canonical_timestamp(Utc::now());
    let request = WorkflowExecutionRequest {
        organization_id,
        project_id,
        environment_id,
        workflow_run_id: WorkflowRunId::new(),
        plan_revision_id: PlanRevisionId::new(),
        plan_digest: Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))
            .expect("plan digest"),
        step_id: "run_function".into(),
        step_attempt: u64::from(u32::MAX) + 1,
        execution_template_id: revision.template_id,
        execution_template_revision_id: revision.revision_id,
        execution_template_digest: revision.definition.digest().clone(),
        capability: EXECUTION_TEMPLATE_CAPABILITY.into(),
        input: serde_json::json!({"value": 42}),
        requested_at,
    };

    let (left, right) = tokio::join!(
        service.start_or_adopt(&request),
        service.start_or_adopt(&request)
    );
    let left = left.expect("start Workflow child");
    let right = right.expect("adopt Workflow child");
    assert_eq!(left, right);
    assert_eq!(left.template.input, request.input);
    assert_eq!(
        left.workflow.as_ref().expect("binding").step_attempt,
        request.step_attempt
    );
    assert_eq!(executions.outbox_events().await.len(), 1);
    assert_eq!(templates.outbox_events().await.len(), 1);

    let drifted_input = WorkflowExecutionRequest {
        input: serde_json::json!({"value": 43}),
        ..request.clone()
    };
    assert!(matches!(
        service.adopt(&drifted_input).await,
        Err(ApplicationError::Conflict(_))
    ));
    let drifted_digest = WorkflowExecutionRequest {
        execution_template_digest: Sha256Digest::parse(format!("sha256:{}", "c".repeat(64)))
            .expect("other template digest"),
        ..request.clone()
    };
    assert!(matches!(
        service.start_or_adopt(&drifted_digest).await,
        Err(ApplicationError::Conflict(_))
    ));
    let wrong_capability = WorkflowExecutionRequest {
        capability: "agent.run".into(),
        ..request.clone()
    };
    assert!(matches!(
        service.start_or_adopt(&wrong_capability).await,
        Err(ApplicationError::Invalid(_))
    ));

    let cancelled = service
        .request_cancellation(&request, requested_at + Duration::milliseconds(1))
        .await
        .expect("cancel Workflow child")
        .expect("existing Workflow child");
    assert_eq!(cancelled.status, ExecutionStatus::Cancelling);
    assert_eq!(
        service
            .request_cancellation(&request, requested_at + Duration::milliseconds(2))
            .await
            .expect("repeat cancellation"),
        Some(cancelled)
    );
    let events = executions.outbox_events().await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_key, "execution.run.requested");
    assert_eq!(events[1].event_key, "execution.run.cancellation-requested");
}
