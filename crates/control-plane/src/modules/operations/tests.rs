use super::*;
use crate::modules::shared_kernel::domain::{OperationId, OrganizationId};
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

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
        WorkflowIdentity::new("cloud.deployment", "1")?,
        input,
        Utc::now(),
    ))
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

    let engine = FlowEngine::in_memory(Arc::new(CompletingRuntime));
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
