use super::FlowWorkflowRunCoordinator;
use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::workflow::domain::{
    WorkflowApplicationFrameAuthority, WorkflowCompositeFrame, WorkflowCompositeFrameResolution,
    WorkflowCompositeRegionPolicy, WorkflowCompositeWaveFrameResolution,
    WorkflowCompositeWaveHookMetadata, WorkflowCompositeWaveResumePayload,
    WorkflowIterationFailureMode, WorkflowRunCoordinationError, WorkflowRunRecord,
    WorkflowRunStatus,
};
use crate::modules::workflow::WorkflowCompositeExecutionRequest;
use a3s_flow::{FlowEvent, HookStatus, WorkflowRunSnapshot};
use chrono::{DateTime, Utc};
use futures_util::future::join_all;

const SIBLING_FAILURE_CANCELLATION_REASON: &str = "Sibling Workflow composite frame failed";

#[derive(Debug, Clone)]
struct CoordinatedCompositeWaveHook {
    metadata: WorkflowCompositeWaveHookMetadata,
    frames: Vec<WorkflowCompositeFrame>,
    created_at: DateTime<Utc>,
    status: HookStatus,
}

enum CompositeWaveRequests {
    Ready(Vec<WorkflowCompositeExecutionRequest>),
    Exhausted,
}

impl FlowWorkflowRunCoordinator {
    pub(super) fn active_composite_wave_count(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
    ) -> Result<usize, WorkflowRunCoordinationError> {
        Ok(composite_wave_hooks(record, snapshot, history)?
            .into_iter()
            .filter(|hook| hook.status == HookStatus::Active)
            .count())
    }

    pub(super) async fn coordinate_active_composite_wave(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
    ) -> Result<(), WorkflowRunCoordinationError> {
        let hooks = composite_wave_hooks(record, snapshot, history)?;
        let active = hooks
            .iter()
            .filter(|hook| hook.status == HookStatus::Active)
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "WorkflowRun replay exposed more than one active composite wave hook".into(),
            ));
        }
        let Some(hook) = active.first() else {
            return Ok(());
        };
        let CompositeWaveRequests::Ready(requests) = composite_wave_requests(record, hook)? else {
            let resolutions = hook
                .frames
                .iter()
                .map(|frame| {
                    WorkflowCompositeWaveFrameResolution::failed(
                        frame,
                        "Workflow composite frame has no remaining execution budget",
                    )
                })
                .collect();
            return self
                .resume_composite_wave(record, &hook.metadata, resolutions)
                .await;
        };
        let port = self.composites.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow composite execution coordination is not configured".into(),
            )
        })?;
        let outcomes = join_all(requests.iter().map(|request| port.start_or_adopt(request))).await;
        let mut children = Vec::with_capacity(requests.len());
        let mut resolutions = std::iter::repeat_with(|| None)
            .take(requests.len())
            .collect::<Vec<Option<WorkflowCompositeWaveFrameResolution>>>();
        let mut all_linked = true;
        for (index, ((request, frame), outcome)) in
            requests.iter().zip(&hook.frames).zip(outcomes).enumerate()
        {
            let child = match outcome {
                Ok(child) => child,
                Err(error) if super::permanent_dispatch_error(&error) => {
                    resolutions[index] = Some(WorkflowCompositeWaveFrameResolution::failed(
                        frame,
                        super::composite::composite_error(
                            "Workflow child dispatch rejected",
                            &error.to_string(),
                        ),
                    ));
                    children.push(None);
                    continue;
                }
                Err(error) => return Err(super::application_unavailable(error)),
            };
            let linked = self
                .link_composite_child_frame(record, snapshot, frame, &child)
                .await?;
            all_linked &= linked;
            if linked && child.run.status.is_terminal() {
                resolutions[index] = Some(wave_terminal_resolution(record, frame, &child)?);
            }
            if child.run.id != request.workflow_run_id() {
                return Err(WorkflowRunCoordinationError::Unavailable(
                    "Workflow composite wave child identity drifted".into(),
                ));
            }
            children.push(Some(child));
        }

        let policy = iteration_policy(record, &hook.metadata.region_step_id)?;
        let terminate_wave = policy.failure_mode == WorkflowIterationFailureMode::Terminate
            && resolutions
                .iter()
                .flatten()
                .any(WorkflowCompositeWaveFrameResolution::is_primary_failure);
        if terminate_wave {
            let reason = Some(SIBLING_FAILURE_CANCELLATION_REASON.into());
            for (index, ((request, frame), child)) in requests
                .iter()
                .zip(&hook.frames)
                .zip(children.iter_mut())
                .enumerate()
            {
                let Some(current) = child.as_mut() else {
                    continue;
                };
                if !current.run.status.is_terminal() {
                    let requested_at =
                        canonical_timestamp(hook.created_at.max(current.run.updated_at));
                    *current = port
                        .request_cancellation(
                            request,
                            reason.clone(),
                            record.run.requested_by,
                            requested_at,
                        )
                        .await
                        .map_err(super::application_unavailable)?
                        .ok_or_else(|| {
                            WorkflowRunCoordinationError::Unavailable(
                                "Workflow composite wave child disappeared during cancellation"
                                    .into(),
                            )
                        })?;
                }
                if current.run.status.is_terminal() {
                    resolutions[index] = Some(wave_terminal_resolution(record, frame, current)?);
                }
            }
        }

        if all_linked && resolutions.iter().all(Option::is_some) {
            self.resume_composite_wave(
                record,
                &hook.metadata,
                resolutions.into_iter().flatten().collect(),
            )
            .await?;
        }
        Ok(())
    }

    pub(super) async fn cancel_composite_wave_children(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
    ) -> Result<bool, WorkflowRunCoordinationError> {
        let hooks = composite_wave_hooks(record, snapshot, history)?;
        if hooks.is_empty() {
            return Ok(true);
        }
        let port = self.composites.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow composite execution coordination is not configured".into(),
            )
        })?;
        let (reason, requested_by, requested_at) =
            super::composite::cancellation_authority(record)?;
        let mut all_terminal = true;
        for hook in hooks {
            let CompositeWaveRequests::Ready(requests) = composite_wave_requests(record, &hook)?
            else {
                continue;
            };
            for (request, frame) in requests.iter().zip(&hook.frames) {
                let reference_id = frame.child_reference_id();
                let linked_reference = snapshot.child_operations.contains_key(&reference_id);
                let mut child = match port
                    .adopt(request)
                    .await
                    .map_err(super::application_unavailable)?
                {
                    Some(child) => child,
                    None if hook.status != HookStatus::Active && !linked_reference => continue,
                    None if linked_reference => {
                        return Err(WorkflowRunCoordinationError::Unavailable(
                            "linked Workflow composite wave child disappeared".into(),
                        ))
                    }
                    None => match port.start_or_adopt(request).await {
                        Ok(child) => child,
                        Err(error) if super::permanent_dispatch_error(&error) => continue,
                        Err(error) => return Err(super::application_unavailable(error)),
                    },
                };
                let linked = self
                    .link_composite_child_frame(record, snapshot, frame, &child)
                    .await?;
                if !child.run.status.is_terminal() {
                    let cancellation_at =
                        canonical_timestamp(requested_at.max(child.run.updated_at));
                    child = port
                        .request_cancellation(
                            request,
                            reason.clone(),
                            requested_by,
                            cancellation_at,
                        )
                        .await
                        .map_err(super::application_unavailable)?
                        .ok_or_else(|| {
                            WorkflowRunCoordinationError::Unavailable(
                                "Workflow composite wave child disappeared during cancellation"
                                    .into(),
                            )
                        })?;
                }
                all_terminal &= linked && child.run.status.is_terminal();
            }
        }
        Ok(all_terminal)
    }

    async fn resume_composite_wave(
        &self,
        record: &WorkflowRunRecord,
        metadata: &WorkflowCompositeWaveHookMetadata,
        resolutions: Vec<WorkflowCompositeWaveFrameResolution>,
    ) -> Result<(), WorkflowRunCoordinationError> {
        let input = &record.run.execution_input;
        let variables = input
            .variable_contract
            .as_ref()
            .ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "parent WorkflowRun lost its variable contract".into(),
                )
            })?
            .restore()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let defaults = input
            .variable_defaults
            .as_ref()
            .map(|resolved| resolved.restore())
            .transpose()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let regions = input
            .composite_regions
            .as_ref()
            .ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "parent WorkflowRun lost its composite region contract".into(),
                )
            })?
            .restore()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let payload = WorkflowCompositeWaveResumePayload::new(
            metadata,
            resolutions,
            &input.plan,
            &regions,
            &variables,
            defaults.as_ref(),
        )
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
        self.engine
            .resume_hook(
                &record.run.flow_run_id,
                &metadata.flow_hook_id(),
                serde_json::to_value(payload).map_err(|error| {
                    WorkflowRunCoordinationError::Unavailable(error.to_string())
                })?,
            )
            .await
            .map_err(|error| super::unavailable_at("resume Workflow composite wave hook", error))
    }
}

fn composite_wave_hooks(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
) -> Result<Vec<CoordinatedCompositeWaveHook>, WorkflowRunCoordinationError> {
    let observed = super::super::composite_wave::observed_composite_wave_hooks(
        &record.run.execution_input,
        snapshot,
    )
    .map_err(WorkflowRunCoordinationError::Unavailable)?;
    let mut hooks = Vec::with_capacity(observed.len());
    for observed in observed {
        let hook_id = observed.metadata.flow_hook_id();
        let matching = history
            .iter()
            .filter(|envelope| {
                matches!(
                    &envelope.event,
                    FlowEvent::HookCreated { hook_id: created, .. } if created == &hook_id
                )
            })
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow composite wave hook {hook_id:?} must have exactly one creation event"
            )));
        }
        let FlowEvent::HookCreated {
            token, metadata, ..
        } = &matching[0].event
        else {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "Workflow composite wave creation evidence disappeared".into(),
            ));
        };
        let expected_metadata = serde_json::to_value(&observed.metadata)
            .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
        if token != &observed.metadata.flow_hook_token() || metadata != &expected_metadata {
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow composite wave hook {hook_id:?} creation authority drifted"
            )));
        }
        hooks.push(CoordinatedCompositeWaveHook {
            metadata: observed.metadata,
            frames: observed.frames,
            created_at: canonical_timestamp(matching[0].timestamp),
            status: observed.hook.status,
        });
    }
    Ok(hooks)
}

fn composite_wave_requests(
    record: &WorkflowRunRecord,
    hook: &CoordinatedCompositeWaveHook,
) -> Result<CompositeWaveRequests, WorkflowRunCoordinationError> {
    let input = &record.run.execution_input;
    let remaining = input
        .deadline_at
        .signed_duration_since(hook.created_at)
        .num_seconds();
    if remaining <= 0 {
        return Ok(CompositeWaveRequests::Exhausted);
    }
    let timeout_seconds = u64::try_from(remaining).map_err(|_| {
        WorkflowRunCoordinationError::Unavailable(
            "Workflow composite wave child timeout conversion failed".into(),
        )
    })?;
    let requests = hook
        .frames
        .iter()
        .map(|frame| {
            let request = WorkflowCompositeExecutionRequest {
                frame: frame.clone(),
                ontology_id: input.plan.ontology_id,
                ontology_revision_id: input.plan.ontology_revision_id,
                ontology_digest: input.plan.ontology_digest.clone(),
                environment_id: input.plan.environment_id,
                application_frame: WorkflowApplicationFrameAuthority::from_parent(input, frame)
                    .map_err(WorkflowRunCoordinationError::Unavailable)?,
                requested_by: record.run.requested_by,
                requested_at: hook.created_at,
                timeout_seconds,
            };
            request
                .validate()
                .map_err(WorkflowRunCoordinationError::Unavailable)?;
            Ok(request)
        })
        .collect::<Result<Vec<_>, WorkflowRunCoordinationError>>()?;
    Ok(CompositeWaveRequests::Ready(requests))
}

fn iteration_policy(
    record: &WorkflowRunRecord,
    step_id: &str,
) -> Result<
    crate::modules::workflow::domain::WorkflowIterationRegionPolicy,
    WorkflowRunCoordinationError,
> {
    let regions = record
        .run
        .execution_input
        .composite_regions
        .as_ref()
        .ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow composite wave lost its region contract".into(),
            )
        })?
        .restore()
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
    match regions.resolve(step_id) {
        Some(WorkflowCompositeRegionPolicy::Iteration(policy)) => Ok(policy.clone()),
        _ => Err(WorkflowRunCoordinationError::Unavailable(
            "Workflow composite wave lost its Iteration policy".into(),
        )),
    }
}

fn wave_terminal_resolution(
    record: &WorkflowRunRecord,
    frame: &WorkflowCompositeFrame,
    child: &WorkflowRunRecord,
) -> Result<WorkflowCompositeWaveFrameResolution, WorkflowRunCoordinationError> {
    if child.run.status == WorkflowRunStatus::Cancelled
        && child.run.cancellation_reason.as_deref() == Some(SIBLING_FAILURE_CANCELLATION_REASON)
    {
        return Ok(WorkflowCompositeWaveFrameResolution::cancelled_after_primary_failure(frame));
    }
    Ok(
        match FlowWorkflowRunCoordinator::terminal_composite_resolution(record, frame, child)? {
            WorkflowCompositeFrameResolution::Completed { result, .. } => {
                WorkflowCompositeWaveFrameResolution::completed(frame, result)
            }
            WorkflowCompositeFrameResolution::Failed { error, .. } => {
                WorkflowCompositeWaveFrameResolution::failed(frame, error)
            }
        },
    )
}
