use super::*;

#[tokio::test]
async fn runtime_v22_dispatches_bounded_iteration_waves_and_reduces_in_ordinal_order(
) -> Result<(), FlowError> {
    let mut input = composite_workflow_run_input(
        WorkflowCompositeRegionPolicy::Iteration(WorkflowIterationRegionPolicy {
            step_id: "batch".into(),
            maximum_items: 3,
            maximum_concurrency: 2,
            failure_mode: WorkflowIterationFailureMode::Terminate,
        }),
        json!([{"item": 1}, {"item": 2}, {"item": 3}]),
    )
    .map_err(FlowError::Runtime)?;
    input.requested_at = chrono::Utc::now();
    input.deadline_at = input.requested_at + chrono::Duration::hours(1);
    input.validate().map_err(FlowError::Runtime)?;
    let run_id = input.workflow_run_id.to_string();
    let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime::default()));
    engine
        .start_with_id(
            &run_id,
            WorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                WORKFLOW_RUN_FLOW_VERSION_V22,
                "a3s-cloud",
                "main",
            ),
            serde_json::to_value(&input)?,
        )
        .await?;

    let first = composite_wave_hook(&engine, &run_id, "batch", 0, 2).await?;
    let frames = first
        .frames(
            &input.plan,
            &input
                .composite_regions
                .as_ref()
                .ok_or_else(|| FlowError::Runtime("missing composite regions".into()))?
                .restore()
                .map_err(FlowError::Runtime)?,
            &input
                .variable_contract
                .as_ref()
                .ok_or_else(|| FlowError::Runtime("missing variable contract".into()))?
                .restore()
                .map_err(FlowError::Runtime)?,
            None,
        )
        .map_err(FlowError::Runtime)?;
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].child_input, json!({"item": 1}));
    assert_eq!(frames[1].child_input, json!({"item": 2}));
    resume_completed_composite_wave(
        &engine,
        &run_id,
        &input,
        first,
        vec![json!({"value": 10}), json!({"value": 20})],
    )
    .await?;

    let second = composite_wave_hook(&engine, &run_id, "batch", 2, 1).await?;
    assert_eq!(second.first_ordinal, 2);
    resume_completed_composite_wave(&engine, &run_id, &input, second, vec![json!({"value": 30})])
        .await?;

    let snapshot = engine.snapshot(&run_id).await?;
    assert_eq!(
        snapshot.status,
        WorkflowRunStatus::Completed,
        "{snapshot:#?}"
    );
    assert_eq!(
        snapshot.output,
        Some(json!([{"value": 10}, {"value": 20}, {"value": 30}]))
    );
    let history = engine.history(&run_id).await?;
    let created_waves = history
        .iter()
        .filter(|event| {
            matches!(
                &event.event,
                FlowEvent::HookCreated { hook_id, .. }
                    if hook_id.starts_with("workflow-composite-wave:batch:")
            )
        })
        .count();
    assert_eq!(created_waves, 2);
    Ok(())
}

#[tokio::test]
async fn runtime_v22_parallel_iteration_preserves_continue_null_and_remove_failed_order(
) -> Result<(), FlowError> {
    for (failure_mode, expected) in [
        (
            WorkflowIterationFailureMode::ContinueNull,
            json!([null, {"value": 20}]),
        ),
        (
            WorkflowIterationFailureMode::RemoveFailed,
            json!([{"value": 20}]),
        ),
    ] {
        let mut input = composite_workflow_run_input(
            WorkflowCompositeRegionPolicy::Iteration(WorkflowIterationRegionPolicy {
                step_id: "batch".into(),
                maximum_items: 2,
                maximum_concurrency: 2,
                failure_mode,
            }),
            json!([{"item": 1}, {"item": 2}]),
        )
        .map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime::default()));
        engine
            .start_with_id(
                &run_id,
                WorkflowSpec::rust_embedded(
                    WORKFLOW_RUN_FLOW_NAME,
                    WORKFLOW_RUN_FLOW_VERSION_V22,
                    "a3s-cloud",
                    "main",
                ),
                serde_json::to_value(&input)?,
            )
            .await?;
        let metadata = composite_wave_hook(&engine, &run_id, "batch", 0, 2).await?;
        let variables = input
            .variable_contract
            .as_ref()
            .ok_or_else(|| FlowError::Runtime("missing variable contract".into()))?
            .restore()
            .map_err(FlowError::Runtime)?;
        let regions = input
            .composite_regions
            .as_ref()
            .ok_or_else(|| FlowError::Runtime("missing composite regions".into()))?
            .restore()
            .map_err(FlowError::Runtime)?;
        let frames = metadata
            .frames(&input.plan, &regions, &variables, None)
            .map_err(FlowError::Runtime)?;
        let second_result = frames[1]
            .resolve(&input.plan, &regions, &variables, json!({"value": 20}))
            .map_err(FlowError::Runtime)?;
        let payload = WorkflowCompositeWaveResumePayload::new(
            &metadata,
            vec![
                WorkflowCompositeWaveFrameResolution::failed(
                    &frames[0],
                    "child WorkflowRun failed",
                ),
                WorkflowCompositeWaveFrameResolution::completed(&frames[1], second_result),
            ],
            &input.plan,
            &regions,
            &variables,
            None,
        )
        .map_err(FlowError::Runtime)?;
        engine
            .resume_hook(
                &run_id,
                &metadata.flow_hook_id(),
                serde_json::to_value(payload)?,
            )
            .await?;
        let snapshot = engine.snapshot(&run_id).await?;
        assert_eq!(
            snapshot.status,
            WorkflowRunStatus::Completed,
            "{snapshot:#?}"
        );
        assert_eq!(snapshot.output, Some(expected));
    }
    Ok(())
}

async fn composite_wave_hook(
    engine: &FlowEngine,
    run_id: &str,
    step_id: &str,
    first_ordinal: u32,
    frame_count: usize,
) -> Result<WorkflowCompositeWaveHookMetadata, FlowError> {
    let hook_id = format!("workflow-composite-wave:{step_id}:{first_ordinal}:{frame_count}");
    let snapshot = engine.snapshot(run_id).await?;
    let hook = snapshot
        .hooks
        .get(&hook_id)
        .ok_or_else(|| FlowError::Runtime(format!("missing composite wave hook {hook_id}")))?;
    assert_eq!(hook.status, HookStatus::Active, "{snapshot:#?}");
    serde_json::from_value(hook.metadata.clone()).map_err(FlowError::Serialization)
}

async fn resume_completed_composite_wave(
    engine: &FlowEngine,
    run_id: &str,
    input: &crate::modules::workflow::domain::WorkflowRunInput,
    metadata: WorkflowCompositeWaveHookMetadata,
    outputs: Vec<serde_json::Value>,
) -> Result<(), FlowError> {
    let variables = input
        .variable_contract
        .as_ref()
        .ok_or_else(|| FlowError::Runtime("missing variable contract".into()))?
        .restore()
        .map_err(FlowError::Runtime)?;
    let defaults = input
        .variable_defaults
        .as_ref()
        .map(|resolved| resolved.restore())
        .transpose()
        .map_err(FlowError::Runtime)?;
    let regions = input
        .composite_regions
        .as_ref()
        .ok_or_else(|| FlowError::Runtime("missing composite regions".into()))?
        .restore()
        .map_err(FlowError::Runtime)?;
    let frames = metadata
        .frames(&input.plan, &regions, &variables, defaults.as_ref())
        .map_err(FlowError::Runtime)?;
    if frames.len() != outputs.len() {
        return Err(FlowError::Runtime(
            "composite wave test output count drifted".into(),
        ));
    }
    let mut resolutions = frames
        .iter()
        .zip(outputs)
        .map(|(frame, output)| {
            frame
                .resolve(&input.plan, &regions, &variables, output)
                .map(|result| WorkflowCompositeWaveFrameResolution::completed(frame, result))
        })
        .collect::<Result<Vec<_>, String>>()
        .map_err(FlowError::Runtime)?;
    resolutions.reverse();
    let payload = WorkflowCompositeWaveResumePayload::new(
        &metadata,
        resolutions,
        &input.plan,
        &regions,
        &variables,
        defaults.as_ref(),
    )
    .map_err(FlowError::Runtime)?;
    engine
        .resume_hook(
            run_id,
            &metadata.flow_hook_id(),
            serde_json::to_value(payload)?,
        )
        .await
}
