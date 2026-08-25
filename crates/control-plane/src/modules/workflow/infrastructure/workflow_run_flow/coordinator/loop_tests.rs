use super::*;

#[tokio::test]
async fn loop_dispatches_sequential_children_and_feeds_each_terminal_output_forward() {
    let (engine, record, now) = loop_workflow_fixture(3, 3_600, 2).await;
    let port = Arc::new(FakeWorkflowCompositePort::terminal(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );

    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate first loop child")
        .expect("waiting loop projection");
    assert_eq!(waiting.run.status, WorkflowRunStatus::Waiting);
    assert_eq!(port.create_count(), 1);

    let completed = coordinator
        .reconcile(&waiting, now + chrono::Duration::milliseconds(1))
        .await
        .expect("coordinate second loop child")
        .expect("completed loop projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(
        completed.run.output,
        Some(serde_json::json!({
            "done": true,
            "iteration": 2,
            "terminateAt": 2,
        }))
    );
    assert_eq!(port.create_count(), 2);

    let requests = port.requests().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].frame.ordinal, 0);
    assert_eq!(requests[0].frame.child_input["iteration"], 0);
    assert_eq!(requests[1].frame.ordinal, 1);
    assert_eq!(requests[1].frame.child_input["iteration"], 1);
    assert!(requests[0].timeout_seconds <= 3_600);
    assert!(requests[1].timeout_seconds <= requests[0].timeout_seconds);
    assert_eq!(
        engine
            .snapshot(&record.run.flow_run_id)
            .await
            .expect("completed loop snapshot")
            .child_operations
            .len(),
        2
    );
}

#[tokio::test]
async fn replacement_coordinator_adopts_the_same_loop_child_before_advancing() {
    let (engine, record, now) = loop_workflow_fixture(3, 3_600, 2).await;
    let port = Arc::new(FakeWorkflowCompositePort::queued(engine.clone()));
    let first = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );
    let waiting = first
        .reconcile(&record, now)
        .await
        .expect("initial loop coordination")
        .expect("waiting loop projection");
    assert_eq!(port.create_count(), 1);
    drop(first);

    port.refresh_terminal_children().await;
    let replacement = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );
    let advanced = replacement
        .reconcile(&waiting, now + chrono::Duration::milliseconds(1))
        .await
        .expect("replacement loop coordination")
        .expect("advanced loop projection");
    assert_eq!(advanced.run.status, WorkflowRunStatus::Waiting);
    let advanced_step = advanced
        .steps
        .iter()
        .find(|step| step.step_id == "refine")
        .expect("loop step projection");
    assert_eq!(advanced_step.status, WorkflowStepProjectionStatus::Running,);
    let advanced_step_sequence = advanced_step.last_flow_sequence;
    assert_eq!(port.create_count(), 1, "the first child must be adopted");

    let second_wait = replacement
        .reconcile(&advanced, now + chrono::Duration::milliseconds(2))
        .await
        .expect("start second loop child")
        .expect("second loop wait projection");
    assert_eq!(
        second_wait.run.status,
        WorkflowRunStatus::Waiting,
        "unexpected second loop projection error: {:?}",
        second_wait.run.error,
    );
    assert_eq!(port.create_count(), 2);
    port.refresh_terminal_children().await;
    assert_eq!(
        engine
            .snapshot(&record.run.flow_run_id)
            .await
            .expect("waiting parent snapshot before the final child resume")
            .status,
        a3s_flow::WorkflowRunStatus::Suspended,
    );
    let completed = replacement
        .reconcile(&second_wait, now + chrono::Duration::milliseconds(3))
        .await
        .expect("finish replacement loop coordination")
        .expect("completed loop projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(port.create_count(), 2);
    let completed_step = completed
        .steps
        .iter()
        .find(|step| step.step_id == "refine")
        .expect("completed loop step projection");
    assert_eq!(
        completed_step.attempt_generation, 1,
        "two Loop frames remain part of one Flow step attempt",
    );
    assert_eq!(completed_step.evidence_references.len(), 4);
    assert!(completed_step.last_flow_sequence > advanced_step_sequence);
}

#[tokio::test]
async fn loop_fails_after_its_exact_maximum_iteration_count() {
    let (engine, record, now) = loop_workflow_fixture(2, 3_600, 99).await;
    let port = Arc::new(FakeWorkflowCompositePort::terminal(engine));
    let coordinator = FlowWorkflowRunCoordinator::with_composites(
        port.engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );

    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate first bounded loop child")
        .expect("waiting bounded loop projection");
    let failed = coordinator
        .reconcile(&waiting, now + chrono::Duration::milliseconds(1))
        .await
        .expect("coordinate final bounded loop child")
        .expect("failed bounded loop projection");

    assert_eq!(failed.run.status, WorkflowRunStatus::Failed);
    assert!(failed.run.error.as_deref().is_some_and(|error| {
        error.contains("exhausted its immutable maximum iteration count")
    }));
    assert_eq!(port.create_count(), 2);
}

#[tokio::test]
async fn loop_child_requests_never_outlive_the_immutable_region_time_budget() {
    let (engine, record, now) = loop_workflow_fixture(3, 7, 2).await;
    let port = Arc::new(FakeWorkflowCompositePort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_composites(
        engine,
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );

    coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate time-bounded loop")
        .expect("waiting time-bounded loop projection");
    let requests = port.requests().await;
    let [request] = requests.as_slice() else {
        panic!("expected one loop child request, got {requests:#?}")
    };
    assert!(request.timeout_seconds > 0);
    assert!(request.timeout_seconds <= 7);
    assert_eq!(
        request.requested_at + chrono::Duration::seconds(request.timeout_seconds as i64),
        request.requested_at + chrono::Duration::seconds(7),
    );
}
