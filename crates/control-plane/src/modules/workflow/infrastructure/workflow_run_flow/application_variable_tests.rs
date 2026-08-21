use super::*;

#[tokio::test]
async fn v12_application_variable_assignment_requires_snapshot_then_exact_commit_evidence(
) -> Result<(), FlowError> {
    let mut input = application_variable_workflow_run_input().map_err(FlowError::Runtime)?;
    input.requested_at = chrono::Utc::now();
    input.deadline_at = input.requested_at + chrono::Duration::hours(1);
    input.validate().map_err(FlowError::Runtime)?;
    let run_id = input.workflow_run_id.to_string();
    let spec = WorkflowSpec::rust_embedded(
        WORKFLOW_RUN_FLOW_NAME,
        WORKFLOW_RUN_FLOW_VERSION_V12,
        "a3s-cloud",
        "main",
    );
    let encoded = serde_json::to_value(&input)?;
    let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime::default()));
    engine
        .start_with_id(run_id.clone(), spec.clone(), encoded.clone())
        .await?;

    let waiting_snapshot = engine.snapshot(&run_id).await?;
    assert_eq!(waiting_snapshot.status, WorkflowRunStatus::Suspended);
    let snapshot_hook = waiting_snapshot
        .hooks
        .values()
        .find(|hook| {
            hook.status == HookStatus::Active
                && hook
                    .hook_id
                    .starts_with("workflow-application-variable-snapshot:")
        })
        .expect("Application variable snapshot hook");
    let snapshot_metadata = serde_json::from_value::<
        WorkflowApplicationVariableSnapshotHookMetadata,
    >(snapshot_hook.metadata.clone())?;
    assert_eq!(snapshot_metadata.step_id, TEST_APPLICATION_VARIABLE_STEP_ID);
    let values = json!({"locale": "private-locale-sentinel"});
    let values_digest = Sha256Digest::from_bytes(
        &canonical_json_bounded(
            &values,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow Application variable test snapshot",
        )
        .map_err(FlowError::Runtime)?,
    );
    let snapshot_revision_id = ConversationVariableRevisionId::new();
    let snapshot_payload = WorkflowApplicationVariableSnapshotResumePayload::new(
        &snapshot_metadata,
        ApplicationId::new(),
        ApplicationReleaseId::new(),
        Sha256Digest::parse(digest('a')).map_err(FlowError::Runtime)?,
        ApplicationSessionId::new(),
        ApplicationInvocationId::new(),
        snapshot_revision_id,
        1,
        values_digest.clone(),
        values,
    )
    .map_err(FlowError::Runtime)?;
    engine
        .resume_hook(
            &run_id,
            &snapshot_metadata.flow_hook_id(),
            serde_json::to_value(snapshot_payload)?,
        )
        .await?;

    let waiting_write = engine.snapshot(&run_id).await?;
    assert_eq!(waiting_write.status, WorkflowRunStatus::Suspended);
    let write_hook = waiting_write
        .hooks
        .values()
        .find(|hook| {
            hook.status == HookStatus::Active
                && hook
                    .hook_id
                    .starts_with("workflow-application-variable-write:")
        })
        .expect("Application variable write hook");
    let write_metadata = serde_json::from_value::<WorkflowApplicationVariableWriteHookMetadata>(
        write_hook.metadata.clone(),
    )?;
    assert_eq!(write_metadata.step_id, TEST_APPLICATION_VARIABLE_STEP_ID);
    assert_eq!(write_metadata.expected_revision_id, snapshot_revision_id);
    assert_eq!(write_metadata.expected_revision_number, 1);
    assert_eq!(write_metadata.expected_values_digest, values_digest);
    let committed = json!({
        "conversation_topic": "high",
        "locale": "private-locale-sentinel"
    });
    let committed_digest = Sha256Digest::from_bytes(
        &canonical_json_bounded(
            &committed,
            WORKFLOW_RUN_OUTPUT_MAX_BYTES,
            "Workflow Application variable test commit",
        )
        .map_err(FlowError::Runtime)?,
    );
    assert_eq!(write_metadata.values_digest, committed_digest);
    let write_payload = WorkflowApplicationVariableWriteResumePayload::new(
        &write_metadata,
        ConversationVariableRevisionId::new(),
        2,
        snapshot_revision_id,
        values_digest,
        committed_digest,
    )
    .map_err(FlowError::Runtime)?;
    engine
        .resume_hook(
            &run_id,
            &write_metadata.flow_hook_id(),
            serde_json::to_value(write_payload)?,
        )
        .await?;

    let completed = engine.snapshot(&run_id).await?;
    assert_eq!(
        completed.status,
        WorkflowRunStatus::Completed,
        "{completed:#?}"
    );
    assert_eq!(completed.output, Some(json!({"result": input.goal_input})));
    assert!(!completed
        .steps
        .contains_key(&flow_step_id(TEST_APPLICATION_VARIABLE_STEP_ID)));

    let history_length = engine.history(&run_id).await?.len();
    engine.start_with_id(run_id.clone(), spec, encoded).await?;
    assert_eq!(engine.history(&run_id).await?.len(), history_length);

    let public_history = WorkflowRunHistoryReader::new(engine.clone())
        .read(&run_id, 0, 100)
        .await
        .map_err(FlowError::Runtime)?;
    let public_history = serde_json::to_string(&public_history)?;
    assert!(public_history.contains("redacted"));
    assert!(!public_history.contains("private-locale-sentinel"));

    let (run, steps) =
        WorkflowRun::create(input, PrincipalId::new()).map_err(FlowError::Runtime)?;
    let inspection = WorkflowRunVariableReader::new(engine)
        .inspect(&WorkflowRunRecord { run, steps })
        .await
        .map_err(FlowError::Runtime)?;
    let conversation_topic = inspection
        .variables
        .iter()
        .find(|variable| variable.name == "conversation_topic")
        .ok_or_else(|| {
            FlowError::Runtime("Application variable inspection lost conversation_topic".into())
        })?;
    assert_eq!(
        conversation_topic.state,
        WorkflowRunVariableState::Materialized
    );
    assert_eq!(conversation_topic.value, Some(json!("high")));
    Ok(())
}
