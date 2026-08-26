use crate::modules::workflow::domain::{
    WorkflowCompositeFrame, WorkflowCompositeFrameResolution, WorkflowCompositeWaveFrameResolution,
    WorkflowCompositeWaveHookMetadata, WorkflowCompositeWaveResumePayload, WorkflowRunInput,
    WORKFLOW_RUN_INPUT_SCHEMA_V22, WORKFLOW_RUN_INPUT_SCHEMA_V23,
};
use a3s_flow::{HookSnapshot, WorkflowContext, WorkflowRunSnapshot};

const WORKFLOW_COMPOSITE_WAVE_HOOK_PREFIX: &str = "workflow-composite-wave:";

pub(super) struct ObservedCompositeWaveHook<'a> {
    pub hook: &'a HookSnapshot,
    pub metadata: WorkflowCompositeWaveHookMetadata,
    pub frames: Vec<WorkflowCompositeFrame>,
}

pub(super) struct ObservedCompositeFrame<'a> {
    pub hook: &'a HookSnapshot,
    pub frame: WorkflowCompositeFrame,
    pub child_reference_id: String,
}

pub(super) fn observed_composite_wave_hooks<'a>(
    input: &WorkflowRunInput,
    snapshot: &'a WorkflowRunSnapshot,
) -> Result<Vec<ObservedCompositeWaveHook<'a>>, String> {
    let prefixed = snapshot
        .hooks
        .values()
        .filter(|hook| {
            hook.hook_id
                .starts_with(WORKFLOW_COMPOSITE_WAVE_HOOK_PREFIX)
        })
        .collect::<Vec<_>>();
    if prefixed.is_empty() {
        return Ok(Vec::new());
    }
    if !matches!(
        input.schema.as_str(),
        WORKFLOW_RUN_INPUT_SCHEMA_V22 | WORKFLOW_RUN_INPUT_SCHEMA_V23
    ) {
        return Err(
            "Workflow composite wave hook is incompatible with its runtime generation".into(),
        );
    }
    let variables = input
        .variable_contract
        .as_ref()
        .ok_or_else(|| "Workflow composite wave lost its variable contract".to_owned())?
        .restore()?;
    let defaults = input
        .variable_defaults
        .as_ref()
        .map(|resolved| resolved.restore())
        .transpose()?;
    let regions = input
        .composite_regions
        .as_ref()
        .ok_or_else(|| "Workflow composite wave lost its region contract".to_owned())?
        .restore()?;
    let mut observed = Vec::with_capacity(prefixed.len());
    for hook in prefixed {
        let metadata =
            serde_json::from_value::<WorkflowCompositeWaveHookMetadata>(hook.metadata.clone())
                .map_err(|error| format!("Workflow composite wave metadata is invalid: {error}"))?;
        let frames = metadata.frames(&input.plan, &regions, &variables, defaults.as_ref())?;
        if metadata.organization_id != input.organization_id
            || metadata.project_id != input.project_id
            || metadata.workflow_run_id != input.workflow_run_id
            || metadata.plan_revision_id != input.plan_revision_id
            || metadata.plan_digest != input.plan_digest
            || hook.hook_id != metadata.flow_hook_id()
            || hook.token != metadata.flow_hook_token()
        {
            return Err("Workflow composite wave authority drifted".into());
        }
        observed.push(ObservedCompositeWaveHook {
            hook,
            metadata,
            frames,
        });
    }
    observed.sort_by(|left, right| {
        left.metadata
            .region_step_id
            .cmp(&right.metadata.region_step_id)
            .then_with(|| {
                left.metadata
                    .first_ordinal
                    .cmp(&right.metadata.first_ordinal)
            })
    });
    let mut previous: Option<(&str, u32)> = None;
    for item in &observed {
        let step_id = item.metadata.region_step_id.as_str();
        let first = item.metadata.first_ordinal;
        match previous {
            Some((previous_step, previous_last)) if previous_step == step_id => {
                if first
                    != previous_last
                        .checked_add(1)
                        .ok_or_else(|| "Workflow composite wave ordinal overflowed".to_owned())?
                {
                    return Err(
                        "Workflow composite waves are not contiguous from ordinal zero".into(),
                    );
                }
            }
            _ if first != 0 => {
                return Err("Workflow composite waves do not start at ordinal zero".into())
            }
            _ => {}
        }
        previous = Some((step_id, item.metadata.last_ordinal()?));
    }
    Ok(observed)
}

pub(super) fn observed_composite_frames<'a>(
    input: &WorkflowRunInput,
    snapshot: &'a WorkflowRunSnapshot,
) -> Result<Vec<ObservedCompositeFrame<'a>>, String> {
    let mut observed = super::composite::observed_composite_hooks(input, snapshot)?
        .into_iter()
        .map(|observed| ObservedCompositeFrame {
            hook: observed.hook,
            child_reference_id: observed.metadata.frame.child_reference_id(),
            frame: observed.metadata.frame,
        })
        .collect::<Vec<_>>();
    for wave in observed_composite_wave_hooks(input, snapshot)? {
        observed.extend(wave.frames.into_iter().map(|frame| ObservedCompositeFrame {
            hook: wave.hook,
            child_reference_id: frame.child_reference_id(),
            frame,
        }));
    }
    observed.sort_by(|left, right| {
        left.frame
            .region_step_id
            .cmp(&right.frame.region_step_id)
            .then_with(|| left.frame.ordinal.cmp(&right.frame.ordinal))
    });
    let mut previous: Option<(&str, u32)> = None;
    for frame in &observed {
        let step_id = frame.frame.region_step_id.as_str();
        let ordinal = frame.frame.ordinal;
        match previous {
            Some((previous_step, previous_ordinal)) if previous_step == step_id => {
                if ordinal
                    != previous_ordinal
                        .checked_add(1)
                        .ok_or_else(|| "Workflow composite frame ordinal overflowed".to_owned())?
                {
                    return Err(
                        "Workflow composite frames are not contiguous from ordinal zero".into(),
                    );
                }
            }
            _ if ordinal != 0 => {
                return Err("Workflow composite frames do not start at ordinal zero".into())
            }
            _ => {}
        }
        previous = Some((step_id, ordinal));
    }
    Ok(observed)
}

pub(super) enum WaveObservation {
    Await(WorkflowCompositeWaveHookMetadata),
    Resolved {
        resolutions: Vec<WorkflowCompositeFrameResolution>,
        primary_failure: Option<(u32, String)>,
    },
}

pub(super) fn observe_wave(
    context: &WorkflowContext<'_>,
    metadata: WorkflowCompositeWaveHookMetadata,
    input: &WorkflowRunInput,
    regions: &crate::modules::workflow::domain::WorkflowCompositeRegions,
    variables: &crate::modules::workflow::domain::WorkflowVariableContract,
    defaults: Option<&crate::modules::workflow::domain::WorkflowVariableDefaults>,
) -> Result<WaveObservation, super::composite::CompositeStepError> {
    let hook_id = metadata.flow_hook_id();
    if context.hook_disposed(&hook_id) {
        return Err(super::composite::CompositeStepError::NonDeterministic(
            format!(
                "Workflow composite wave beginning at frame {} was disposed",
                metadata.first_ordinal
            ),
        ));
    }
    let Some(observed) = context.hook_payload(&hook_id) else {
        return Ok(WaveObservation::Await(metadata));
    };
    let payload = serde_json::from_value::<WorkflowCompositeWaveResumePayload>(observed.clone())
        .map_err(|_| {
            super::composite::CompositeStepError::NonDeterministic(wave_payload_drift(&metadata))
        })?;
    let primary_failure = payload
        .resolutions
        .iter()
        .find_map(WorkflowCompositeWaveFrameResolution::primary_failure)
        .map(|(ordinal, error)| (ordinal, error.to_owned()));
    let resolutions = payload
        .frame_resolutions(&metadata, &input.plan, regions, variables, defaults)
        .map_err(|_| {
            super::composite::CompositeStepError::NonDeterministic(wave_payload_drift(&metadata))
        })?;
    Ok(WaveObservation::Resolved {
        resolutions,
        primary_failure,
    })
}

fn wave_payload_drift(metadata: &WorkflowCompositeWaveHookMetadata) -> String {
    format!(
        "Workflow composite wave beginning at frame {} received an invalid authority-bound payload",
        metadata.first_ordinal
    )
}
