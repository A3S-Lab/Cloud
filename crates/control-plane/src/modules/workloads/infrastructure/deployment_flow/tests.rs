use super::{DeploymentFlowConfig, DeploymentFlowRuntime};
use crate::modules::fleet::domain::entities::EnrollmentToken;
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeRepository, NodeEnrollmentDraft, NodeHeartbeatUpdate,
};
use crate::modules::fleet::domain::value_objects::{
    EnrollmentTokenCredential, NodeCapabilities, NodeName,
};
use crate::modules::fleet::infrastructure::persistence::InMemoryNodeRepository;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnrollmentTokenId, EnvironmentId, IdempotencyRequest, OperationId,
    OrganizationId, ProjectId, ResourceName, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentStatus, HttpHealthCheck, OciArtifact, OciArtifactReference,
    RequestedServiceTemplate, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate,
    Workload, WorkloadDesiredState, WorkloadRevision,
};
use crate::modules::workloads::domain::events::{DeploymentRequested, WorkloadStopRequested};
use crate::modules::workloads::domain::repositories::{
    CreateDeploymentBundle, IWorkloadRepository, RequestWorkloadStopBundle,
};
use crate::modules::workloads::domain::services::{
    IOciArtifactResolver, OciArtifactResolutionError,
};
use crate::modules::workloads::infrastructure::{project_runtime_spec, InMemoryWorkloadRepository};
use a3s_cloud_contracts::{
    DomainEventEnvelope, NodeCommandLeaseRequest, NodeHeartbeat, NodeObservationBatch,
    RuntimeObservationReport,
};
use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore, InMemoryEventStore,
    WorkflowRunStatus, WorkflowSpec,
};
use a3s_runtime::contract::{
    HealthCheckKind, IsolationLevel, NetworkMode, ResourceControl, RuntimeCapabilities,
    RuntimeFeature, RuntimeHealthObservation, RuntimeHealthState, RuntimeObservation,
    RuntimeUnitClass, RuntimeUnitState,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

mod support;

use support::*;

#[tokio::test]
async fn mutable_tag_is_resolved_once_and_replay_keeps_the_persisted_digest(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, _) = ready_node(&nodes, organization_id, base).await?;
    let first_digest = format!("sha256:{}", "1".repeat(64));
    let second_digest = format!("sha256:{}", "2".repeat(64));
    let resolver = Arc::new(MovingArtifactResolver::new(first_digest.clone()));
    let workload_port: Arc<dyn IWorkloadRepository> = workloads.clone();
    let node_port: Arc<dyn INodeRepository> = nodes.clone();
    let control_port: Arc<dyn INodeControlRepository> = nodes.clone();
    let runtime = DeploymentFlowRuntime::new(
        workload_port,
        resolver.clone(),
        node_port,
        control_port,
        Duration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 1, 10_000, 5_000, 1, 10_000)?,
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = requested_deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("mutable tag fixture")?,
            base,
        ),
        base,
        "mutable-tag",
    )?;
    let revision_id = bundle.revision.id;
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;

    engine
        .start_with_id(
            operation.id.to_string(),
            workflow_spec(),
            operation.input.clone(),
        )
        .await?;
    let lease = lease(&nodes, node_id, agent_instance_id, 0).await?;
    let apply = lease
        .commands
        .first()
        .ok_or("Runtime apply was not dispatched")?;
    let runtime_artifact = match &apply.payload {
        a3s_cloud_contracts::NodeCommandPayload::RuntimeApply { request } => &request.spec.artifact,
        _ => return Err("deployment dispatched a non-apply command".into()),
    };
    assert_eq!(runtime_artifact.digest, first_digest);
    assert!(runtime_artifact.uri.contains("@sha256:"));
    assert!(!runtime_artifact.uri.ends_with(":stable"));
    assert_eq!(resolver.calls(), 1);

    resolver.move_tag(second_digest);
    let history_length = engine.history(&operation.id.to_string()).await?.len();
    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;
    assert_eq!(
        engine.history(&operation.id.to_string()).await?.len(),
        history_length
    );
    assert_eq!(resolver.calls(), 1);
    let revision = workloads
        .find_revision(organization_id, revision_id)
        .await?;
    assert_eq!(
        revision.resolved_template()?.artifact.digest,
        runtime_artifact.digest
    );
    Ok(())
}

#[tokio::test]
async fn active_workload_stop_waits_for_stopped_evidence_and_clears_active_revision(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&nodes, organization_id, base).await?;
    let runtime = runtime(&workloads, &nodes, Duration::seconds(10))?;
    let store = Arc::new(FailOnceStepCompletionStore::new("stop-dispatch"));
    let engine = FlowEngine::new(store.clone(), Arc::new(runtime.clone()));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("active stop fixture")?,
            base,
        ),
        1,
        '9',
        base,
        "active-stop-deploy",
    )?;
    let revision = bundle.revision.clone();
    let deployment_operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    engine
        .start_with_id(
            deployment_operation.id.to_string(),
            workflow_spec(),
            deployment_operation.input,
        )
        .await?;
    let apply_lease = lease(&nodes, node_id, agent_instance_id, 0).await?;
    let spec = project_runtime_spec(&revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        apply_lease
            .commands
            .first()
            .ok_or("missing apply command")?,
        healthy_observation(&spec, RuntimeHealthState::Healthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;

    let requested_at = Utc::now();
    let mut workload = workloads
        .find_workload(organization_id, revision.workload_id)
        .await?;
    let expected_version = workload.aggregate_version;
    workload.request_stop(requested_at)?;
    let stop_operation_id = OperationId::new();
    let stop_operation = OperationRequest::new(
        stop_operation_id,
        organization_id,
        OperationSubject::new("workload", workload.id.as_uuid())?,
        WorkflowIdentity::new("cloud.workload.stop", "1")?,
        serde_json::json!({
            "operationId": stop_operation_id,
            "organizationId": organization_id,
            "requestedAt": requested_at,
            "workloadId": workload.id,
        }),
        requested_at,
    );
    let stop_request = RequestWorkloadStopBundle {
        event: WorkloadStopRequested::envelope(&workload, &stop_operation, Uuid::now_v7())?,
        idempotency: IdempotencyRequest::new("test.workload.stop", "active-stop", b"active-stop")?,
        operation: stop_operation.clone(),
        workload,
        expected_version,
    };
    let accepted = workloads
        .request_workload_stop(stop_request.clone())
        .await?;
    let replayed = workloads.request_workload_stop(stop_request).await?;
    assert!(!accepted.replayed);
    assert!(replayed.replayed);
    assert_eq!(accepted.operation.id, replayed.operation.id);

    let stop_input = stop_operation.input.clone();
    let failure = engine
        .start_with_id(
            stop_operation.id.to_string(),
            stop_workflow_spec(),
            stop_input.clone(),
        )
        .await
        .expect_err("injected crash must interrupt stop dispatch persistence");
    assert!(matches!(failure, FlowError::Store(_)));
    let stop_history = store.list(&stop_operation.id.to_string()).await?;
    assert!(stop_history.iter().any(|event| matches!(
        &event.event,
        FlowEvent::StepStarted { step_id, .. } if step_id == "stop-dispatch"
    )));
    assert!(!stop_history.iter().any(|event| matches!(
        &event.event,
        FlowEvent::StepCompleted { step_id, .. } if step_id == "stop-dispatch"
    )));
    let expected_stop_command_id = crate::modules::shared_kernel::domain::NodeCommandId::from_uuid(
        stop_operation.id.as_uuid(),
    );
    let command_before_restart = nodes
        .find_command(node_id, expected_stop_command_id)
        .await?
        .ok_or("stop command side effect was not persisted before the injected crash")?;

    drop(engine);
    let engine = FlowEngine::new(store, Arc::new(runtime));
    engine
        .start_with_id(
            stop_operation.id.to_string(),
            stop_workflow_spec(),
            stop_input,
        )
        .await?;
    assert_eq!(
        nodes
            .find_command(node_id, expected_stop_command_id)
            .await?
            .ok_or("stop command disappeared after Flow restart")?,
        command_before_restart
    );
    let stop_lease = lease(
        &nodes,
        node_id,
        agent_instance_id,
        apply_lease.commands[0].sequence,
    )
    .await?;
    assert_eq!(stop_lease.commands.len(), 1);
    let stop_command = stop_lease.commands.first().ok_or("missing stop command")?;
    assert_eq!(stop_command.command_id, expected_stop_command_id.as_uuid());
    assert!(matches!(
        stop_command.payload,
        a3s_cloud_contracts::NodeCommandPayload::RuntimeStop { .. }
    ));
    assert!(workloads
        .find_workload(organization_id, revision.workload_id)
        .await?
        .active_revision_id
        .is_some());
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        stop_command,
        stopped_observation(&spec)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        engine
            .snapshot(&stop_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Completed
    );
    let stopped = workloads
        .find_workload(organization_id, revision.workload_id)
        .await?;
    assert_eq!(stopped.desired_state, WorkloadDesiredState::Stopped);
    assert_eq!(stopped.active_revision_id, None);
    Ok(())
}

#[tokio::test]
async fn healthy_observation_activates_once_and_unhealthy_update_preserves_previous_revision(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&nodes, organization_id, base).await?;
    let runtime = runtime(&workloads, &nodes, Duration::seconds(10))?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let workload = Workload::create(
        WorkloadId::new(),
        organization_id,
        ProjectId::new(),
        EnvironmentId::new(),
        ResourceName::parse("health fixture")?,
        base,
    );

    let first = deployment_bundle(workload, 1, 'a', base, "healthy-first")?;
    let first_revision = first.revision.clone();
    let first_deployment = first.deployment.clone();
    let first_operation = first.operation.clone();
    workloads.create_deployment(first).await?;
    let spec = workflow_spec();
    engine
        .start_with_id(
            first_operation.id.to_string(),
            spec.clone(),
            first_operation.input.clone(),
        )
        .await?;
    assert_eq!(
        engine
            .snapshot(&first_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Suspended
    );
    let first_lease = lease(&nodes, node_id, agent_instance_id, 0).await?;
    assert_eq!(first_lease.commands.len(), 1);
    assert_eq!(
        first_lease.commands[0].command_id,
        first_deployment.id.as_uuid()
    );
    let first_runtime_spec = project_runtime_spec(&first_revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        &first_lease.commands[0],
        healthy_observation(&first_runtime_spec, RuntimeHealthState::Healthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        engine
            .snapshot(&first_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Completed
    );
    let active = workloads
        .find_deployment(organization_id, first_deployment.id)
        .await?;
    assert_eq!(active.status, DeploymentStatus::Active);
    assert_eq!(
        workloads
            .find_workload(organization_id, first_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(first_revision.id)
    );
    let history_length = engine.history(&first_operation.id.to_string()).await?.len();
    engine
        .start_with_id(
            first_operation.id.to_string(),
            spec.clone(),
            first_operation.input,
        )
        .await?;
    assert_eq!(
        engine.history(&first_operation.id.to_string()).await?.len(),
        history_length
    );

    let selected_workload = workloads
        .find_workload(organization_id, first_deployment.workload_id)
        .await?;
    let second = deployment_bundle(selected_workload, 2, 'b', Utc::now(), "unhealthy-update")?;
    let second_revision = second.revision.clone();
    let second_deployment = second.deployment.clone();
    let second_operation = second.operation.clone();
    workloads.create_deployment(second).await?;
    engine
        .start_with_id(
            second_operation.id.to_string(),
            spec,
            second_operation.input.clone(),
        )
        .await?;
    let second_lease = lease(
        &nodes,
        node_id,
        agent_instance_id,
        first_lease.commands[0].sequence,
    )
    .await?;
    assert_eq!(second_lease.commands.len(), 1);
    let second_runtime_spec = project_runtime_spec(&second_revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        &second_lease.commands[0],
        healthy_observation(&second_runtime_spec, RuntimeHealthState::Unhealthy)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        engine
            .snapshot(&second_operation.id.to_string())
            .await?
            .status,
        WorkflowRunStatus::Failed
    );
    assert_eq!(
        workloads
            .find_deployment(organization_id, second_deployment.id)
            .await?
            .status,
        DeploymentStatus::Failed
    );
    assert_eq!(
        workloads
            .find_workload(organization_id, first_deployment.workload_id)
            .await?
            .active_revision_id,
        Some(first_revision.id)
    );
    Ok(())
}

#[tokio::test]
async fn no_eligible_node_reaches_a_persisted_failure_without_dispatch(
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let runtime = runtime(&workloads, &nodes, Duration::milliseconds(2))?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("unschedulable fixture")?,
            Utc::now(),
        ),
        1,
        'c',
        Utc::now(),
        "no-node",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    assert_eq!(
        engine.snapshot(&operation.id.to_string()).await?.status,
        WorkflowRunStatus::Failed
    );
    let failed = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(failed.status, DeploymentStatus::Failed);
    assert!(failed.command_id.is_none());
    assert!(failed
        .failure
        .as_deref()
        .is_some_and(|reason| reason.contains("no eligible node")));
    Ok(())
}

#[tokio::test]
async fn cancellation_before_dispatch_completes_without_creating_a_runtime_child(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let runtime = runtime(&workloads, &nodes, Duration::seconds(10))?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("cancel before dispatch")?,
            base,
        ),
        1,
        'd',
        base,
        "cancel-before-dispatch",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    workloads
        .mark_cancellation_requested(deployment.id, 1, Utc::now())
        .await?;

    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;

    let snapshot = engine.snapshot(&operation.id.to_string()).await?;
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(
        snapshot.output.as_ref().and_then(|output| output
            .get("operationStatus")
            .and_then(serde_json::Value::as_str)),
        Some("cancelled")
    );
    let cancelled = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert!(cancelled.command_id.is_none());
    assert!(cancelled.cleanup_command_id.is_none());
    Ok(())
}

#[tokio::test]
async fn cancellation_while_artifact_resolution_retries_completes_without_a_runtime_child(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let workload_port: Arc<dyn IWorkloadRepository> = workloads.clone();
    let node_port: Arc<dyn INodeRepository> = nodes.clone();
    let control_port: Arc<dyn INodeControlRepository> = nodes;
    let runtime = DeploymentFlowRuntime::new(
        workload_port,
        Arc::new(UnusedArtifactResolver),
        node_port,
        control_port,
        Duration::seconds(5),
        DeploymentFlowConfig::from_milliseconds(10_000, 5_000, 1, 10_000, 5_000, 1, 10_000)?,
    )?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = requested_deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("cancel resolving artifact")?,
            base,
        ),
        base,
        "cancel-resolving-artifact",
    )?;
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;

    engine
        .start_with_id(operation.id.to_string(), workflow_spec(), operation.input)
        .await?;
    assert_eq!(
        engine.snapshot(&operation.id.to_string()).await?.status,
        WorkflowRunStatus::Suspended
    );
    let resolving = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(resolving.status, DeploymentStatus::Resolving);
    workloads
        .mark_cancellation_requested(
            deployment.id,
            resolving.aggregate_version,
            Utc::now().max(resolving.updated_at),
        )
        .await?;

    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    engine.resume_due_retries(Utc::now()).await?;

    let snapshot = engine.snapshot(&operation.id.to_string()).await?;
    assert_eq!(snapshot.status, WorkflowRunStatus::Completed);
    assert_eq!(
        snapshot.output.as_ref().and_then(|output| output
            .get("operationStatus")
            .and_then(serde_json::Value::as_str)),
        Some("cancelled")
    );
    let cancelled = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert!(cancelled.node_id.is_none());
    assert!(cancelled.command_id.is_none());
    assert!(cancelled.cleanup_command_id.is_none());
    Ok(())
}

#[tokio::test]
async fn cancellation_after_dispatch_waits_for_durable_stopped_evidence(
) -> Result<(), Box<dyn std::error::Error>> {
    let base = Utc::now() - Duration::seconds(1);
    let organization_id = OrganizationId::new();
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let nodes = Arc::new(InMemoryNodeRepository::new());
    let (node_id, agent_instance_id, capabilities) =
        ready_node(&nodes, organization_id, base).await?;
    let runtime = runtime(&workloads, &nodes, Duration::seconds(10))?;
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    let bundle = deployment_bundle(
        Workload::create(
            WorkloadId::new(),
            organization_id,
            ProjectId::new(),
            EnvironmentId::new(),
            ResourceName::parse("cancel dispatched child")?,
            base,
        ),
        1,
        'e',
        base,
        "cancel-dispatched-child",
    )?;
    let revision = bundle.revision.clone();
    let deployment = bundle.deployment.clone();
    let operation = bundle.operation.clone();
    workloads.create_deployment(bundle).await?;
    engine
        .start_with_id(
            operation.id.to_string(),
            workflow_spec(),
            operation.input.clone(),
        )
        .await?;
    let apply_lease = lease(&nodes, node_id, agent_instance_id, 0).await?;
    assert_eq!(apply_lease.commands.len(), 1);
    let applying = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    workloads
        .mark_cancellation_requested(deployment.id, applying.aggregate_version, Utc::now())
        .await?;

    engine
        .resume_due_waits(Utc::now() + Duration::seconds(1))
        .await?;
    let cleanup_lease = lease(
        &nodes,
        node_id,
        agent_instance_id,
        apply_lease.commands[0].sequence,
    )
    .await?;
    assert_eq!(cleanup_lease.commands.len(), 1);
    assert!(matches!(
        cleanup_lease.commands[0].payload,
        a3s_cloud_contracts::NodeCommandPayload::RuntimeStop { .. }
    ));
    assert_eq!(
        workloads
            .find_deployment(organization_id, deployment.id)
            .await?
            .status,
        DeploymentStatus::CleanupPending
    );
    let spec = project_runtime_spec(&revision)?;
    record_observation(
        &nodes,
        node_id,
        agent_instance_id,
        &capabilities,
        &cleanup_lease.commands[0],
        stopped_observation(&spec)?,
    )
    .await?;
    engine
        .resume_due_waits(Utc::now() + Duration::seconds(2))
        .await?;

    assert_eq!(
        engine.snapshot(&operation.id.to_string()).await?.status,
        WorkflowRunStatus::Completed
    );
    let cancelled = workloads
        .find_deployment(organization_id, deployment.id)
        .await?;
    assert_eq!(cancelled.status, DeploymentStatus::Cancelled);
    assert_eq!(
        cancelled.cleanup_command_id.map(|id| id.as_uuid()),
        Some(cleanup_lease.commands[0].command_id)
    );
    assert!(cancelled.cancelled_at.is_some());
    Ok(())
}
