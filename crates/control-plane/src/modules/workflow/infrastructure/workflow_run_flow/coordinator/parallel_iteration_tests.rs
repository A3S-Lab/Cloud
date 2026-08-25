use super::*;

#[tokio::test]
async fn bounded_parallel_iteration_starts_one_wave_concurrently_and_reduces_by_ordinal() {
    let (engine, record, now) = composite_workflow_fixture_with_concurrency(
        serde_json::json!([
            {"ticketId": "A", "priority": "high"},
            {"ticketId": "B", "priority": "high"}
        ]),
        2,
    )
    .await;
    let port = Arc::new(FakeWorkflowCompositePort::terminal_with_barrier(
        engine.clone(),
        2,
    ));
    let coordinator = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );

    let completed = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        coordinator.reconcile(&record, now),
    )
    .await
    .expect("parallel dispatch did not reach both child starts")
    .expect("coordinate parallel composite wave")
    .expect("completed parent projection");

    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(
        completed.run.output,
        Some(serde_json::json!(["HIGH A", "HIGH B"]))
    );
    assert_eq!(port.create_count(), 2);
    assert_eq!(port.maximum_starts_in_flight(), 2);
    let requests = port.requests().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].frame.ordinal, 0);
    assert_eq!(requests[1].frame.ordinal, 1);
    assert_eq!(requests[0].requested_at, requests[1].requested_at);
    assert_eq!(requests[0].timeout_seconds, requests[1].timeout_seconds);
    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("parallel parent snapshot");
    assert_eq!(snapshot.child_operations.len(), 2);
    assert!(snapshot
        .hooks
        .contains_key("workflow-composite-wave:batch:0:2"));
}

#[tokio::test]
async fn parallel_iteration_starts_the_next_wave_only_after_the_prior_wave_is_durable() {
    let (engine, record, now) = composite_workflow_fixture_with_concurrency(
        serde_json::json!([
            {"ticketId": "A", "priority": "high"},
            {"ticketId": "B", "priority": "high"},
            {"ticketId": "C", "priority": "high"}
        ]),
        2,
    )
    .await;
    let port = Arc::new(FakeWorkflowCompositePort::terminal(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );

    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate first composite wave")
        .expect("second wave projection");
    assert_eq!(waiting.run.status, WorkflowRunStatus::Waiting);
    assert_eq!(port.create_count(), 2);
    let between_waves = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("between-wave parent snapshot");
    assert_eq!(between_waves.child_operations.len(), 2);
    assert_eq!(
        between_waves.hooks["workflow-composite-wave:batch:2:1"].status,
        a3s_flow::HookStatus::Active
    );

    let completed = coordinator
        .reconcile(
            &waiting,
            canonical_timestamp(waiting.run.updated_at + chrono::Duration::milliseconds(1)),
        )
        .await
        .expect("coordinate second composite wave")
        .expect("completed parent projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(
        completed.run.output,
        Some(serde_json::json!(["HIGH A", "HIGH B", "HIGH C"]))
    );
    assert_eq!(port.create_count(), 3);
    assert_eq!(
        engine
            .snapshot(&record.run.flow_run_id)
            .await
            .expect("completed parallel parent snapshot")
            .child_operations
            .len(),
        3
    );
}

#[tokio::test]
async fn replacement_coordinator_adopts_every_parallel_iteration_child_after_process_death() {
    let (engine, record, now) = composite_workflow_fixture_with_concurrency(
        serde_json::json!([
            {"ticketId": "A", "priority": "high"},
            {"ticketId": "B", "priority": "high"}
        ]),
        2,
    )
    .await;
    let port = Arc::new(FakeWorkflowCompositePort::queued(engine.clone()));
    let first = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );
    let waiting = first
        .reconcile(&record, now)
        .await
        .expect("initial parallel coordination")
        .expect("waiting parallel projection");
    assert_eq!(waiting.run.status, WorkflowRunStatus::Waiting);
    assert_eq!(port.create_count(), 2);
    drop(first);

    port.refresh_terminal_children().await;
    let replacement = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );
    let completed = replacement
        .reconcile(&waiting, now + chrono::Duration::milliseconds(1))
        .await
        .expect("replacement parallel coordination")
        .expect("completed parallel projection");

    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(port.create_count(), 2);
    assert_eq!(
        engine
            .snapshot(&record.run.flow_run_id)
            .await
            .expect("parallel parent snapshot")
            .child_operations
            .len(),
        2
    );
}

#[tokio::test]
async fn terminating_parallel_iteration_cancels_and_awaits_in_flight_siblings() {
    let (engine, record, now) = composite_workflow_fixture_with_concurrency(
        serde_json::json!([
            {"ticketId": "A", "priority": "high"},
            {"ticketId": "FAIL", "priority": "high"}
        ]),
        2,
    )
    .await;
    let port = Arc::new(FakeWorkflowCompositePort::terminal_ordinals(
        engine.clone(),
        [1],
    ));
    let coordinator = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );

    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate failing composite wave")
        .expect("waiting parent projection");
    assert_eq!(waiting.run.status, WorkflowRunStatus::Waiting);
    assert_eq!(port.status_for_ordinal(1).await, WorkflowRunStatus::Failed);
    assert_eq!(
        port.status_for_ordinal(0).await,
        WorkflowRunStatus::Cancelling
    );
    let active = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("active parent snapshot");
    assert_eq!(
        active.hooks["workflow-composite-wave:batch:0:2"].status,
        a3s_flow::HookStatus::Active
    );

    let cancellation_at = canonical_timestamp(
        port.latest_updated_at().await + chrono::Duration::milliseconds(1),
    );
    port.finish_cancellation(cancellation_at).await;
    let failed = coordinator
        .reconcile(
            &waiting,
            cancellation_at + chrono::Duration::milliseconds(1),
        )
        .await
        .expect("finish failing composite wave")
        .expect("failed parent projection");
    assert_eq!(failed.run.status, WorkflowRunStatus::Failed);
    assert!(failed
        .run
        .error
        .as_deref()
        .is_some_and(|error| error.contains("test composite child failed")));
    assert!(port
        .statuses()
        .await
        .into_iter()
        .all(|status| status.is_terminal()));
    assert_eq!(port.create_count(), 2);
}

#[tokio::test]
async fn parent_cancellation_waits_for_every_parallel_iteration_child() {
    let (engine, record, now) = composite_workflow_fixture_with_concurrency(
        serde_json::json!([
            {"ticketId": "A", "priority": "high"},
            {"ticketId": "B", "priority": "high"}
        ]),
        2,
    )
    .await;
    let port = Arc::new(FakeWorkflowCompositePort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );
    let mut waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate parallel children")
        .expect("waiting parent projection");
    assert_eq!(port.create_count(), 2);
    let cancellation_at =
        canonical_timestamp(waiting.run.updated_at + chrono::Duration::milliseconds(1));
    waiting
        .run
        .request_cancellation(
            Some("operator requested cancellation".into()),
            PrincipalId::new(),
            cancellation_at,
        )
        .expect("request parent cancellation");

    assert!(coordinator
        .reconcile(
            &waiting,
            cancellation_at + chrono::Duration::milliseconds(1),
        )
        .await
        .expect("coordinate parallel child cancellation")
        .is_none());
    assert_eq!(
        port.statuses().await,
        vec![WorkflowRunStatus::Cancelling, WorkflowRunStatus::Cancelling]
    );
    port.finish_cancellation(cancellation_at + chrono::Duration::milliseconds(2))
        .await;
    let cancelled = coordinator
        .reconcile(
            &waiting,
            cancellation_at + chrono::Duration::milliseconds(3),
        )
        .await
        .expect("finish parallel parent cancellation")
        .expect("cancelled parent projection");
    assert_eq!(cancelled.run.status, WorkflowRunStatus::Cancelled);
    assert_eq!(port.create_count(), 2);
}
