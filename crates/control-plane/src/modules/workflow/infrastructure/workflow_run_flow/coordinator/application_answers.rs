use super::*;

#[derive(Debug, Clone)]
pub(super) struct ObservedApplicationAnswerHook {
    pub(super) metadata: WorkflowApplicationAnswerHookMetadata,
    pub(super) created_at: DateTime<Utc>,
    pub(super) status: HookStatus,
}

impl ObservedApplicationAnswerHook {
    pub(super) fn request(&self) -> WorkflowApplicationMessageRequest {
        WorkflowApplicationMessageRequest {
            effect: WorkflowApplicationEffectRequest {
                organization_id: self.metadata.organization_id,
                workflow_run_id: self.metadata.workflow_run_id,
                step_id: self.metadata.step_id.clone(),
                step_attempt: self.metadata.step_attempt,
                effect_ordinal: 0,
                occurred_at: self.created_at,
            },
            content: self.metadata.content.clone(),
        }
    }
}

pub(super) fn application_answer_hooks(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
) -> Result<Vec<ObservedApplicationAnswerHook>, WorkflowRunCoordinationError> {
    let Some(application) = record.run.execution_input.application_projection.as_ref() else {
        return Ok(Vec::new());
    };
    let mut hooks = Vec::new();
    for resolved in record
        .run
        .execution_input
        .resolved_steps()
        .map_err(WorkflowRunCoordinationError::Unavailable)?
    {
        if !application.is_answer_step(&resolved.plan.id) {
            continue;
        }
        let Some((hook, metadata)) =
            application_answer_hook(&record.run.execution_input, &resolved, snapshot)
                .map_err(WorkflowRunCoordinationError::Unavailable)?
        else {
            continue;
        };
        let expected_metadata = serde_json::to_value(&metadata)
            .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
        let matching = history
            .iter()
            .filter(|envelope| {
                matches!(
                    &envelope.event,
                    FlowEvent::HookCreated { hook_id, .. }
                        if hook_id == &metadata.flow_hook_id()
                )
            })
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow Application Answer hook {:?} must have exactly one creation event",
                metadata.flow_hook_id()
            )));
        }
        let FlowEvent::HookCreated {
            token,
            metadata: observed_metadata,
            ..
        } = &matching[0].event
        else {
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow Application Answer hook {:?} creation history is invalid",
                metadata.flow_hook_id()
            )));
        };
        if token != &metadata.flow_hook_token() || observed_metadata != &expected_metadata {
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow Application Answer hook {:?} creation authority drifted",
                metadata.flow_hook_id()
            )));
        }
        hooks.push(ObservedApplicationAnswerHook {
            metadata,
            created_at: canonical_timestamp(matching[0].timestamp),
            status: hook.status,
        });
    }
    Ok(hooks)
}
