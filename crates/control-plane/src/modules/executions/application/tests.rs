use super::{
    CancelExecution, CancelExecutionHandler, CreateExecutionCommand, CreateExecutionHandler,
    ExecutionReconciler, EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION,
};
use crate::modules::executions::domain::{
    ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionTemplate,
    IExecutionRepository,
};
use crate::modules::executions::infrastructure::InMemoryExecutionRepository;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::InMemoryOperationRepository;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::projects::infrastructure::persistence::InMemoryProjectsRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotencyRequest, OrganizationId, ProjectId,
};
use a3s_boot::{CommandHandler, CqrsContext, ModuleRef};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::Utc;
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
                organization_id: organization_id.as_uuid(),
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
