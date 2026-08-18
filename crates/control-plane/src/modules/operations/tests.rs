use super::*;
use crate::infrastructure::{
    cloud_runtime_build_compatibility, CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID,
    REPLAY_COMPATIBLE_CLOUD_FLOW_RUNTIME_BUILD_IDS,
};
use crate::modules::shared_kernel::domain::{OperationId, OrganizationId};
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeBuildId, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

#[test]
fn operation_projection_has_no_autonomous_scheduling_authority() {
    let source = include_str!("application/reconciler.rs");
    for forbidden in [
        "tokio::time",
        "tokio::sync::watch",
        "watch::Receiver",
        "pub async fn run(",
    ] {
        assert!(
            !source.contains(forbidden),
            "OperationReconciler contains autonomous scheduling {forbidden:?}; FlowOperationCoordinator must remain the sole clock and queue owner"
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct CompletingRuntime;

#[async_trait]
impl FlowRuntime for CompletingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let output = invocation.input.clone();
        Ok(invocation.context().complete(output))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        Err(FlowError::Runtime(format!(
            "test runtime does not support step {:?}",
            invocation.step_name
        )))
    }
}

fn operation_request(
    operation_id: OperationId,
    input: serde_json::Value,
) -> Result<OperationRequest, String> {
    Ok(OperationRequest::new(
        operation_id,
        OrganizationId::new(),
        OperationSubject::new("deployment", Uuid::now_v7())?,
        WorkflowIdentity::new("cloud.deployment", "2")?,
        input,
        Utc::now(),
    ))
}

fn flow_engine() -> Result<FlowEngine, FlowError> {
    Ok(FlowEngine::builder(Arc::new(CompletingRuntime))
        .with_runtime_build_compatibility(cloud_runtime_build_compatibility()?)
        .build())
}

#[tokio::test]
async fn operation_reconciliation_repairs_start_and_rebuilds_projection(
) -> Result<(), Box<dyn std::error::Error>> {
    let operation_id = OperationId::new();
    let request = operation_request(operation_id, json!({"generation": 1}))?;
    let repository = Arc::new(InMemoryOperationRepository::new());
    let first = repository.enqueue(request.clone()).await?;
    let replay = repository.enqueue(request.clone()).await?;
    assert!(!first.replayed);
    assert!(replay.replayed);
    let conflicting = repository
        .enqueue(operation_request(operation_id, json!({"generation": 2}))?)
        .await;
    assert!(conflicting.is_err());

    let engine = flow_engine()?;
    let operation_engine = Arc::new(FlowOperationEngine::new(engine.clone()));
    let handler = ReconcileOperationsHandler::new(repository.clone(), operation_engine.clone());
    let (left, right) = tokio::join!(handler.execute(10), handler.execute(10));
    let left = left?;
    let right = right?;
    assert!(left.failures.is_empty());
    assert!(right.failures.is_empty());
    assert!(left.projected + right.projected >= 1);
    assert_eq!(engine.list_run_ids().await?, vec![operation_id.to_string()]);
    assert_eq!(engine.history(&operation_id.to_string()).await?.len(), 3);
    assert_eq!(
        engine
            .snapshot(&operation_id.to_string())
            .await?
            .spec
            .runtime_build_id
            .as_ref()
            .map(RuntimeBuildId::as_str),
        Some(CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID)
    );
    let projection = repository
        .find_projection(operation_id)
        .await?
        .ok_or("operation projection was not written")?;
    assert_eq!(projection.status, OperationStatus::Succeeded);
    assert_eq!(handler.execute(10).await?.inspected, 0);

    let rebuilt_repository = Arc::new(InMemoryOperationRepository::new());
    rebuilt_repository.enqueue(request).await?;
    let rebuilder =
        RebuildOperationProjectionsHandler::new(rebuilt_repository.clone(), operation_engine);
    let report = rebuilder.execute().await?;
    assert_eq!(report.inspected, 1);
    assert_eq!(report.rebuilt, 1);
    assert!(report.orphaned.is_empty());
    assert_eq!(
        rebuilt_repository
            .find_projection(operation_id)
            .await?
            .ok_or("rebuilt projection was not written")?
            .status,
        OperationStatus::Succeeded
    );
    Ok(())
}

#[tokio::test]
async fn operation_engine_replays_legacy_unpinned_history_without_creating_new_unpinned_runs(
) -> Result<(), Box<dyn std::error::Error>> {
    let operation_id = OperationId::new();
    let input = json!({"generation": 1});
    let request = operation_request(operation_id, input.clone())?;
    let engine = flow_engine()?;
    engine
        .start_with_id(
            operation_id.to_string(),
            WorkflowSpec::rust_embedded("cloud.deployment", "2", "a3s-cloud", "main"),
            input,
        )
        .await?;

    let projection = FlowOperationEngine::new(engine.clone())
        .ensure(&request)
        .await?;

    assert_eq!(projection.status, OperationStatus::Succeeded);
    assert_eq!(
        engine
            .snapshot(&operation_id.to_string())
            .await?
            .spec
            .runtime_build_id,
        None
    );
    Ok(())
}

#[tokio::test]
async fn operation_engine_replays_only_an_explicitly_compatible_pinned_generation(
) -> Result<(), Box<dyn std::error::Error>> {
    let operation_id = OperationId::new();
    let input = json!({"generation": 1});
    let request = operation_request(operation_id, input.clone())?;
    let legacy_build_id = RuntimeBuildId::new(
        REPLAY_COMPATIBLE_CLOUD_FLOW_RUNTIME_BUILD_IDS
            .first()
            .copied()
            .ok_or("missing legacy Flow runtime build fixture")?,
    )?;
    let engine = flow_engine()?;
    engine
        .start_with_id(
            operation_id.to_string(),
            WorkflowSpec::rust_embedded("cloud.deployment", "2", "a3s-cloud", "main")
                .with_runtime_build(legacy_build_id.clone()),
            input,
        )
        .await?;

    let projection = FlowOperationEngine::new(engine.clone())
        .ensure(&request)
        .await?;

    assert_eq!(projection.status, OperationStatus::Succeeded);
    assert_eq!(
        engine
            .snapshot(&operation_id.to_string())
            .await?
            .spec
            .runtime_build_id,
        Some(legacy_build_id)
    );
    Ok(())
}

#[tokio::test]
async fn cloud_flow_policy_rejects_an_unknown_pinned_generation_before_history_mutation(
) -> Result<(), Box<dyn std::error::Error>> {
    let operation_id = OperationId::new();
    let engine = flow_engine()?;
    let error = engine
        .start_with_id(
            operation_id.to_string(),
            WorkflowSpec::rust_embedded("cloud.deployment", "2", "a3s-cloud", "main")
                .with_runtime_build(RuntimeBuildId::new("a3s-cloud-workflows@unknown")?),
            json!({"generation": "unknown"}),
        )
        .await
        .expect_err("an unknown runtime generation must fail closed");

    assert!(matches!(error, FlowError::RuntimeBuildUnavailable { .. }));
    assert!(engine.list_run_ids().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn operation_engine_rejects_start_without_runtime_build_policy(
) -> Result<(), Box<dyn std::error::Error>> {
    let operation_id = OperationId::new();
    let engine = FlowEngine::in_memory(Arc::new(CompletingRuntime));
    let error = FlowOperationEngine::new(engine.clone())
        .ensure(&operation_request(operation_id, json!({"generation": 1}))?)
        .await
        .expect_err("operation start must require a runtime build policy");

    assert!(matches!(error, OperationEngineError::Unavailable(_)));
    assert!(engine.list_run_ids().await?.is_empty());
    Ok(())
}
