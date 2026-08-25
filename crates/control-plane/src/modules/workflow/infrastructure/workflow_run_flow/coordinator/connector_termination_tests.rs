use super::connector_tests::{fixture, FakeConnectorPort};
use super::FlowWorkflowRunCoordinator;
use crate::modules::connectors::IWorkflowConnectorPort;
use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId};
use crate::modules::workflow::domain::{
    IWorkflowRunCoordinator, WorkflowRunStatus, WorkflowStepProjectionStatus,
};
use crate::modules::workflow::test_support::TEST_CONNECTOR_STEP_ID;
use a3s_flow::{FlowEvent, HookStatus, WaitStatus, WorkflowRunStatus as FlowRunStatus};
use chrono::{Duration, Utc};
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[tokio::test]
async fn parent_cancellation_fences_a_deferred_connector_without_provider_redispatch() {
    let (engine, record, now) = fixture().await;
    let retry_not_before = now + Duration::seconds(2);
    let port = Arc::new(FakeConnectorPort::deferred_once(retry_not_before));
    let coordinator = FlowWorkflowRunCoordinator::with_connectors(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowConnectorPort>,
    );

    let mut waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate deferred Connector")
        .expect("waiting projection");
    let attempt_id = port.requests.lock().await[0]
        .connector_attempt_authority()
        .expect("Connector attempt authority")
        .attempt_id;
    let cancellation_at = canonical_timestamp(Utc::now().max(waiting.run.updated_at));
    waiting
        .run
        .request_cancellation(
            Some("operator cancelled deferred Connector".into()),
            PrincipalId::new(),
            cancellation_at,
        )
        .expect("request parent cancellation");

    let cancelled = coordinator
        .reconcile(&waiting, cancellation_at + Duration::milliseconds(1))
        .await
        .expect("cancel deferred Connector")
        .expect("cancelled projection");
    assert_eq!(
        cancelled.run.status,
        WorkflowRunStatus::Cancelled,
        "{cancelled:#?}"
    );
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    assert_eq!(port.requests.lock().await.len(), 1);
    let connector = cancelled
        .steps
        .iter()
        .find(|step| step.step_id == TEST_CONNECTOR_STEP_ID)
        .expect("Connector projection");
    assert_eq!(connector.status, WorkflowStepProjectionStatus::Cancelled);
    assert_eq!(
        connector.evidence_references,
        [format!("urn:a3s:cloud:connectors:attempt:{attempt_id}")]
    );

    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("cancelled Flow snapshot");
    assert_eq!(snapshot.status, FlowRunStatus::Cancelled);
    assert_eq!(connector.last_flow_sequence, snapshot.last_sequence);
    assert_eq!(
        snapshot.hooks["workflow-connector:invoke:1:1"].status,
        HookStatus::Received
    );
    assert_eq!(snapshot.waits.len(), 1);
    assert!(snapshot
        .waits
        .values()
        .all(|wait| wait.status == WaitStatus::Cancelled));
    let history = engine
        .history(&record.run.flow_run_id)
        .await
        .expect("cancelled Flow history");
    let history_length = history.len();
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::HookCreated { .. }))
            .count(),
        1
    );
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::RunCancellationRequested { .. }))
            .count(),
        1
    );

    assert!(engine
        .resume_due_waits(retry_not_before + Duration::seconds(1))
        .await
        .expect("cancelled Connector wait stays inert")
        .is_empty());
    let replacement = FlowWorkflowRunCoordinator::with_connectors(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowConnectorPort>,
    );
    assert!(replacement
        .reconcile(&cancelled, retry_not_before + Duration::seconds(1))
        .await
        .expect("replacement coordinator observes terminal cancellation")
        .is_none());
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    assert_eq!(port.requests.lock().await.len(), 1);
    assert_eq!(
        engine
            .history(&record.run.flow_run_id)
            .await
            .expect("replacement cancellation history")
            .len(),
        history_length
    );
}

#[tokio::test]
async fn immutable_deadline_fences_a_deferred_connector_without_provider_redispatch() {
    let (engine, record, now) = fixture().await;
    let deadline = record.run.execution_input.deadline_at;
    let retry_not_before = deadline + Duration::seconds(1);
    let port = Arc::new(FakeConnectorPort::deferred_once(retry_not_before));
    let coordinator = FlowWorkflowRunCoordinator::with_connectors(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowConnectorPort>,
    );

    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate deferred Connector")
        .expect("waiting projection");
    let attempt_id = port.requests.lock().await[0]
        .connector_attempt_authority()
        .expect("Connector attempt authority")
        .attempt_id;
    let timed_out = coordinator
        .reconcile(&waiting, deadline)
        .await
        .expect("terminate deferred Connector at its immutable deadline")
        .expect("timed-out projection");

    assert_eq!(timed_out.run.status, WorkflowRunStatus::TimedOut);
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    assert_eq!(port.requests.lock().await.len(), 1);
    let connector = timed_out
        .steps
        .iter()
        .find(|step| step.step_id == TEST_CONNECTOR_STEP_ID)
        .expect("Connector projection");
    assert_eq!(connector.status, WorkflowStepProjectionStatus::Failed);
    assert!(connector
        .error
        .as_deref()
        .is_some_and(|error| error.contains("immutable deadline")));
    assert_eq!(
        connector.evidence_references,
        [format!("urn:a3s:cloud:connectors:attempt:{attempt_id}")]
    );

    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("timed-out Flow snapshot");
    assert_eq!(snapshot.status, FlowRunStatus::Failed);
    assert_eq!(connector.last_flow_sequence, snapshot.last_sequence);
    assert!(matches!(
        snapshot.terminal_outcome,
        Some(a3s_flow::WorkflowTerminalOutcome::TimedOut {
            deadline: observed,
            ..
        }) if observed == deadline
    ));
    assert!(engine
        .list_due_waits(retry_not_before + Duration::seconds(1))
        .await
        .expect("terminal Connector wait lookup")
        .is_empty());

    let replacement = FlowWorkflowRunCoordinator::with_connectors(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowConnectorPort>,
    );
    assert!(replacement
        .reconcile(&timed_out, retry_not_before + Duration::seconds(1))
        .await
        .expect("replacement coordinator observes terminal timeout")
        .is_none());
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    assert_eq!(port.requests.lock().await.len(), 1);
    assert_eq!(
        engine
            .history(&record.run.flow_run_id)
            .await
            .expect("timed-out Flow history")
            .iter()
            .filter(|event| matches!(event.event, FlowEvent::RunTimedOut { .. }))
            .count(),
        1
    );
}
