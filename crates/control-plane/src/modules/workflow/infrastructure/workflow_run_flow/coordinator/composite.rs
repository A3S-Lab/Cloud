use super::FlowWorkflowRunCoordinator;
use crate::modules::shared_kernel::domain::canonical_timestamp;
use crate::modules::workflow::domain::{
    WorkflowApplicationFrameAuthority, WorkflowCompositeChildReferenceMetadata,
    WorkflowCompositeFrame, WorkflowCompositeFrameResolution, WorkflowCompositeHookMetadata,
    WorkflowCompositeRegionPolicy, WorkflowCompositeResumePayload, WorkflowRunCoordinationError,
    WorkflowRunRecord, WorkflowRunStatus,
};
use crate::modules::workflow::WorkflowCompositeExecutionRequest;
use a3s_flow::{ChildOperationReference, FlowError, FlowEvent, HookStatus, WorkflowRunSnapshot};
use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone)]
struct CoordinatedCompositeHook {
    metadata: WorkflowCompositeHookMetadata,
    created_at: DateTime<Utc>,
    region_started_at: DateTime<Utc>,
    status: HookStatus,
}

enum CompositeRequest {
    Ready(Box<WorkflowCompositeExecutionRequest>),
    Exhausted,
}

impl FlowWorkflowRunCoordinator {
    pub(super) async fn coordinate_active_composite(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
    ) -> Result<(), WorkflowRunCoordinationError> {
        let hooks = composite_hooks(record, snapshot, history)?;
        let active = hooks
            .iter()
            .filter(|hook| hook.status == HookStatus::Active)
            .collect::<Vec<_>>();
        let active_waves = self.active_composite_wave_count(record, snapshot, history)?;
        if active.len() + active_waves > 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "WorkflowRun replay exposed more than one active composite hook".into(),
            ));
        }
        if active_waves == 1 {
            return self
                .coordinate_active_composite_wave(record, snapshot, history)
                .await;
        }
        let Some(hook) = active.first() else {
            return Ok(());
        };
        let CompositeRequest::Ready(request) = composite_request(record, hook)? else {
            return self
                .resume_composite_resolution(
                    record,
                    &hook.metadata,
                    WorkflowCompositeFrameResolution::failed(
                        hook.metadata.frame.clone(),
                        "Workflow composite frame has no remaining execution budget",
                    ),
                )
                .await;
        };
        let port = self.composites.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow composite execution coordination is not configured".into(),
            )
        })?;
        let child = match port.start_or_adopt(&request).await {
            Ok(child) => child,
            Err(error) if super::permanent_dispatch_error(&error) => {
                return self
                    .resume_composite_resolution(
                        record,
                        &hook.metadata,
                        WorkflowCompositeFrameResolution::failed(
                            hook.metadata.frame.clone(),
                            composite_error("Workflow child dispatch rejected", &error.to_string()),
                        ),
                    )
                    .await
            }
            Err(error) => return Err(super::application_unavailable(error)),
        };
        let linked = self
            .link_composite_child_frame(record, snapshot, &hook.metadata.frame, &child)
            .await?;
        if linked && child.run.status.is_terminal() {
            self.resume_terminal_composite(record, &hook.metadata, &child)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn cancel_composite_children(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
    ) -> Result<bool, WorkflowRunCoordinationError> {
        let hooks = composite_hooks(record, snapshot, history)?;
        let waves_terminal = self
            .cancel_composite_wave_children(record, snapshot, history)
            .await?;
        if hooks.is_empty() {
            return Ok(waves_terminal);
        }
        let port = self.composites.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow composite execution coordination is not configured".into(),
            )
        })?;
        let (reason, requested_by, requested_at) = cancellation_authority(record)?;
        let mut all_terminal = true;
        for hook in hooks {
            let CompositeRequest::Ready(request) = composite_request(record, &hook)? else {
                continue;
            };
            let linked_reference = snapshot
                .child_operations
                .contains_key(&hook.metadata.flow_hook_id());
            let mut child = match port
                .adopt(&request)
                .await
                .map_err(super::application_unavailable)?
            {
                Some(child) => child,
                None if hook.status != HookStatus::Active && !linked_reference => continue,
                None if linked_reference => {
                    return Err(WorkflowRunCoordinationError::Unavailable(
                        "linked Workflow composite child disappeared".into(),
                    ))
                }
                None => match port.start_or_adopt(&request).await {
                    Ok(child) => child,
                    Err(error) if super::permanent_dispatch_error(&error) => continue,
                    Err(error) => return Err(super::application_unavailable(error)),
                },
            };
            let linked = self
                .link_composite_child_frame(record, snapshot, &hook.metadata.frame, &child)
                .await?;
            if !child.run.status.is_terminal() {
                let cancellation_at = canonical_timestamp(requested_at.max(child.run.updated_at));
                child = port
                    .request_cancellation(&request, reason.clone(), requested_by, cancellation_at)
                    .await
                    .map_err(super::application_unavailable)?
                    .ok_or_else(|| {
                        WorkflowRunCoordinationError::Unavailable(
                            "Workflow composite child disappeared during cancellation".into(),
                        )
                    })?;
            }
            all_terminal &= linked && child.run.status.is_terminal();
        }
        Ok(all_terminal && waves_terminal)
    }

    pub(super) async fn link_composite_child_frame(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        frame: &WorkflowCompositeFrame,
        child: &WorkflowRunRecord,
    ) -> Result<bool, WorkflowRunCoordinationError> {
        let metadata = WorkflowCompositeChildReferenceMetadata::new_for_frame(frame, child)
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let reference = ChildOperationReference::new(
            frame.child_reference_id(),
            "workflow_run",
            child.run.operation_id.to_string(),
        )
        .with_flow_run_id(child.run.flow_run_id.clone())
        .with_metadata(
            serde_json::to_value(metadata)
                .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?,
        );
        if snapshot.status.is_terminal() {
            return match snapshot.child_operations.get(&reference.reference_id) {
                Some(existing) if existing == &reference => Ok(true),
                Some(_) => Err(WorkflowRunCoordinationError::Unavailable(
                    "terminal WorkflowRun composite child reference drifted".into(),
                )),
                None => Ok(true),
            };
        }
        let child_snapshot = match self.engine.snapshot(&child.run.flow_run_id).await {
            Ok(snapshot) => snapshot,
            Err(FlowError::RunNotFound(_)) => return Ok(false),
            Err(error) => {
                return Err(super::unavailable_at(
                    "read composite child WorkflowRun Flow identity",
                    error,
                ))
            }
        };
        let child_history = self
            .engine
            .history(&child.run.flow_run_id)
            .await
            .map_err(|error| {
                super::unavailable_at("read composite child WorkflowRun Flow history", error)
            })?;
        super::super::projection::verify_flow_authority(child, &child_snapshot, &child_history)
            .map_err(|error| {
                WorkflowRunCoordinationError::Unavailable(format!(
                    "Workflow composite child Flow authority drifted: {error}"
                ))
            })?;
        self.engine
            .link_child_operation(&record.run.flow_run_id, reference)
            .await
            .map_err(|error| super::unavailable_at("link composite child WorkflowRun", error))
            .map(|()| true)
    }

    async fn resume_terminal_composite(
        &self,
        record: &WorkflowRunRecord,
        hook: &WorkflowCompositeHookMetadata,
        child: &WorkflowRunRecord,
    ) -> Result<(), WorkflowRunCoordinationError> {
        let resolution = Self::terminal_composite_resolution(record, &hook.frame, child)?;
        self.resume_composite_resolution(record, hook, resolution)
            .await
    }

    pub(super) fn terminal_composite_resolution(
        record: &WorkflowRunRecord,
        frame: &WorkflowCompositeFrame,
        child: &WorkflowRunRecord,
    ) -> Result<WorkflowCompositeFrameResolution, WorkflowRunCoordinationError> {
        Ok(match child.run.status {
            WorkflowRunStatus::Completed => {
                let output = child.run.output.clone().ok_or_else(|| {
                    WorkflowRunCoordinationError::Unavailable(
                        "completed composite child WorkflowRun has no output".into(),
                    )
                })?;
                let variables = record
                    .run
                    .execution_input
                    .variable_contract
                    .as_ref()
                    .ok_or_else(|| {
                        WorkflowRunCoordinationError::Unavailable(
                            "parent WorkflowRun lost its variable contract".into(),
                        )
                    })?
                    .restore()
                    .map_err(WorkflowRunCoordinationError::Unavailable)?;
                let regions = record
                    .run
                    .execution_input
                    .composite_regions
                    .as_ref()
                    .ok_or_else(|| {
                        WorkflowRunCoordinationError::Unavailable(
                            "parent WorkflowRun lost its composite region contract".into(),
                        )
                    })?
                    .restore()
                    .map_err(WorkflowRunCoordinationError::Unavailable)?;
                match frame.resolve(
                    &record.run.execution_input.plan,
                    &regions,
                    &variables,
                    output,
                ) {
                    Ok(result) => {
                        WorkflowCompositeFrameResolution::completed(frame.clone(), result)
                    }
                    Err(error) => WorkflowCompositeFrameResolution::failed(
                        frame.clone(),
                        composite_error("Workflow child output rejected", &error),
                    ),
                }
            }
            WorkflowRunStatus::Failed => WorkflowCompositeFrameResolution::failed(
                frame.clone(),
                composite_error(
                    "Workflow child failed",
                    child.run.error.as_deref().unwrap_or("no failure detail"),
                ),
            ),
            WorkflowRunStatus::Cancelled => WorkflowCompositeFrameResolution::failed(
                frame.clone(),
                "Workflow child was cancelled",
            ),
            WorkflowRunStatus::TimedOut => WorkflowCompositeFrameResolution::failed(
                frame.clone(),
                composite_error(
                    "Workflow child timed out",
                    child.run.error.as_deref().unwrap_or("deadline exceeded"),
                ),
            ),
            _ => {
                return Err(WorkflowRunCoordinationError::Unavailable(
                    "non-terminal Workflow composite child cannot resume its parent".into(),
                ))
            }
        })
    }

    async fn resume_composite_resolution(
        &self,
        record: &WorkflowRunRecord,
        hook: &WorkflowCompositeHookMetadata,
        resolution: WorkflowCompositeFrameResolution,
    ) -> Result<(), WorkflowRunCoordinationError> {
        let variables = record
            .run
            .execution_input
            .variable_contract
            .as_ref()
            .ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "parent WorkflowRun lost its variable contract".into(),
                )
            })?
            .restore()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let regions = record
            .run
            .execution_input
            .composite_regions
            .as_ref()
            .ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "parent WorkflowRun lost its composite region contract".into(),
                )
            })?
            .restore()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let payload = WorkflowCompositeResumePayload::new(
            hook,
            resolution,
            &record.run.execution_input.plan,
            &regions,
            &variables,
        )
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
        self.engine
            .resume_hook(
                &record.run.flow_run_id,
                &hook.flow_hook_id(),
                serde_json::to_value(payload).map_err(|error| {
                    WorkflowRunCoordinationError::Unavailable(error.to_string())
                })?,
            )
            .await
            .map_err(|error| super::unavailable_at("resume Workflow composite hook", error))
    }
}

fn composite_hooks(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
) -> Result<Vec<CoordinatedCompositeHook>, WorkflowRunCoordinationError> {
    let observed =
        super::super::composite::observed_composite_hooks(&record.run.execution_input, snapshot)
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
                "Workflow composite hook {hook_id:?} must have exactly one creation event"
            )));
        }
        let FlowEvent::HookCreated {
            token, metadata, ..
        } = &matching[0].event
        else {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "Workflow composite hook creation evidence disappeared".into(),
            ));
        };
        let expected_metadata = serde_json::to_value(&observed.metadata)
            .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
        if token != &observed.metadata.flow_hook_token() || metadata != &expected_metadata {
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow composite hook {hook_id:?} creation authority drifted"
            )));
        }
        let created_at = canonical_timestamp(matching[0].timestamp);
        let region_prefix = format!(
            "workflow-composite:{}:",
            observed.metadata.frame.region_step_id
        );
        let region_started_at = history
            .iter()
            .find_map(|envelope| match &envelope.event {
                FlowEvent::HookCreated { hook_id, .. } if hook_id.starts_with(&region_prefix) => {
                    Some(canonical_timestamp(envelope.timestamp))
                }
                _ => None,
            })
            .ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "Workflow composite region has no creation evidence".into(),
                )
            })?;
        hooks.push(CoordinatedCompositeHook {
            metadata: observed.metadata,
            created_at,
            region_started_at,
            status: observed.hook.status,
        });
    }
    Ok(hooks)
}

fn composite_request(
    record: &WorkflowRunRecord,
    hook: &CoordinatedCompositeHook,
) -> Result<CompositeRequest, WorkflowRunCoordinationError> {
    let input = &record.run.execution_input;
    let mut deadline = input.deadline_at;
    let regions = input
        .composite_regions
        .as_ref()
        .ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow composite request lost its region contract".into(),
            )
        })?
        .restore()
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
    if let Some(WorkflowCompositeRegionPolicy::Loop(policy)) =
        regions.resolve(&hook.metadata.frame.region_step_id)
    {
        let seconds = i64::try_from(policy.time_budget_seconds).map_err(|_| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow loop time budget exceeds runtime bounds".into(),
            )
        })?;
        let region_deadline = hook
            .region_started_at
            .checked_add_signed(Duration::seconds(seconds))
            .ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "Workflow loop time budget overflowed".into(),
                )
            })?;
        deadline = deadline.min(region_deadline);
    }
    let remaining = deadline
        .signed_duration_since(hook.created_at)
        .num_seconds();
    if remaining <= 0 {
        return Ok(CompositeRequest::Exhausted);
    }
    let timeout_seconds = u64::try_from(remaining).map_err(|_| {
        WorkflowRunCoordinationError::Unavailable(
            "Workflow composite child timeout conversion failed".into(),
        )
    })?;
    let request = WorkflowCompositeExecutionRequest {
        frame: hook.metadata.frame.clone(),
        ontology_id: input.plan.ontology_id,
        ontology_revision_id: input.plan.ontology_revision_id,
        ontology_digest: input.plan.ontology_digest.clone(),
        environment_id: input.plan.environment_id,
        application_frame: WorkflowApplicationFrameAuthority::from_parent(
            input,
            &hook.metadata.frame,
        )
        .map_err(WorkflowRunCoordinationError::Unavailable)?,
        requested_by: record.run.requested_by,
        requested_at: hook.created_at,
        timeout_seconds,
    };
    request
        .validate()
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
    Ok(CompositeRequest::Ready(Box::new(request)))
}

pub(super) fn cancellation_authority(
    record: &WorkflowRunRecord,
) -> Result<
    (
        Option<String>,
        crate::modules::shared_kernel::domain::PrincipalId,
        DateTime<Utc>,
    ),
    WorkflowRunCoordinationError,
> {
    if record.run.status == WorkflowRunStatus::Cancelling {
        let requested_by = record.run.cancellation_requested_by.ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "cancelling parent WorkflowRun lost its principal".into(),
            )
        })?;
        let requested_at = record.run.cancellation_requested_at.ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "cancelling parent WorkflowRun lost its request time".into(),
            )
        })?;
        Ok((
            record.run.cancellation_reason.clone(),
            requested_by,
            requested_at,
        ))
    } else {
        Ok((
            Some("Parent WorkflowRun exceeded its immutable deadline".into()),
            record.run.requested_by,
            record.run.execution_input.deadline_at,
        ))
    }
}

pub(super) fn composite_error(prefix: &str, detail: &str) -> String {
    let sanitized = detail
        .replace(['\0', '\r', '\n'], " ")
        .chars()
        .take(2_048)
        .collect::<String>();
    format!("{prefix}: {sanitized}")
}
