use super::project_workflow_run_record;
use super::projection::{
    application_answer_hook, application_variable_snapshot_hook,
    application_variable_snapshot_payload, application_variable_write_hook, execution_hook,
    verify_flow_authority,
};
use crate::modules::applications::{
    ApplicationInvocationStatus, ApplicationMessageKind, IWorkflowApplicationEffectsPort,
    WorkflowApplicationEffectRequest, WorkflowApplicationMessageRequest,
    WorkflowApplicationRunReference, WorkflowApplicationTerminalRequest,
    WorkflowApplicationVariableVersion, WorkflowApplicationVariableWriteRequest,
};
use crate::modules::executions::{
    Execution, ExecutionOutcome, IWorkflowExecutionPort, WorkflowExecutionRequest,
    EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{canonical_timestamp, Sha256Digest};
use crate::modules::workflow::domain::{
    IWorkflowRunCoordinator, WorkflowApplicationAnswerFailureResumePayload,
    WorkflowApplicationAnswerHookMetadata, WorkflowApplicationAnswerResumePayload,
    WorkflowApplicationVariableSnapshotHookMetadata,
    WorkflowApplicationVariableSnapshotResumePayload,
    WorkflowApplicationVariableWriteFailureResumePayload,
    WorkflowApplicationVariableWriteHookMetadata, WorkflowApplicationVariableWriteResumePayload,
    WorkflowExecutionChildReferenceMetadata, WorkflowExecutionHookMetadata,
    WorkflowExecutionOutcome, WorkflowExecutionResumePayload, WorkflowExecutionStepOutput,
    WorkflowRunCoordinationError, WorkflowRunRecord, WorkflowRunStatus,
    WorkflowStepFailureClassification, WorkflowStepKind, WorkflowStepProjectionStatus,
    WORKFLOW_EXECUTION_RESULT_SCHEMA, WORKFLOW_RUN_INPUT_SCHEMA_V14, WORKFLOW_RUN_INPUT_SCHEMA_V15,
    WORKFLOW_RUN_INPUT_SCHEMA_V16, WORKFLOW_RUN_INPUT_SCHEMA_V17,
};
use a3s_flow::{
    CancellationRequest, ChildOperationReference, FlowEngine, FlowError, FlowEvent, HookStatus,
    WorkflowRunSnapshot,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

mod application_answers;
mod application_variables;
mod composite;
mod connector;

use application_answers::application_answer_hooks;
use application_variables::{application_variable_hooks, ObservedApplicationVariableHook};

#[derive(Clone)]
pub struct FlowWorkflowRunCoordinator {
    engine: FlowEngine,
    executions: Option<Arc<dyn IWorkflowExecutionPort>>,
    composites: Option<Arc<dyn crate::modules::workflow::IWorkflowCompositeExecutionPort>>,
    connectors: Option<Arc<dyn crate::modules::connectors::IWorkflowConnectorPort>>,
    application_effects: Option<Arc<dyn IWorkflowApplicationEffectsPort>>,
}

impl FlowWorkflowRunCoordinator {
    pub const fn new(engine: FlowEngine) -> Self {
        Self {
            engine,
            executions: None,
            composites: None,
            connectors: None,
            application_effects: None,
        }
    }

    pub fn with_executions(
        engine: FlowEngine,
        executions: Arc<dyn IWorkflowExecutionPort>,
    ) -> Self {
        Self {
            engine,
            executions: Some(executions),
            composites: None,
            connectors: None,
            application_effects: None,
        }
    }

    pub fn with_ports(
        engine: FlowEngine,
        executions: Arc<dyn IWorkflowExecutionPort>,
        composites: Arc<dyn crate::modules::workflow::IWorkflowCompositeExecutionPort>,
    ) -> Self {
        Self {
            engine,
            executions: Some(executions),
            composites: Some(composites),
            connectors: None,
            application_effects: None,
        }
    }

    pub fn with_all_ports(
        engine: FlowEngine,
        executions: Arc<dyn IWorkflowExecutionPort>,
        composites: Arc<dyn crate::modules::workflow::IWorkflowCompositeExecutionPort>,
        connectors: Arc<dyn crate::modules::connectors::IWorkflowConnectorPort>,
    ) -> Self {
        Self {
            engine,
            executions: Some(executions),
            composites: Some(composites),
            connectors: Some(connectors),
            application_effects: None,
        }
    }

    pub fn with_all_ports_and_application_effects(
        engine: FlowEngine,
        executions: Arc<dyn IWorkflowExecutionPort>,
        composites: Arc<dyn crate::modules::workflow::IWorkflowCompositeExecutionPort>,
        connectors: Arc<dyn crate::modules::connectors::IWorkflowConnectorPort>,
        application_effects: Arc<dyn IWorkflowApplicationEffectsPort>,
    ) -> Self {
        Self {
            engine,
            executions: Some(executions),
            composites: Some(composites),
            connectors: Some(connectors),
            application_effects: Some(application_effects),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_composites(
        engine: FlowEngine,
        composites: Arc<dyn crate::modules::workflow::IWorkflowCompositeExecutionPort>,
    ) -> Self {
        Self {
            engine,
            executions: None,
            composites: Some(composites),
            connectors: None,
            application_effects: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_connectors(
        engine: FlowEngine,
        connectors: Arc<dyn crate::modules::connectors::IWorkflowConnectorPort>,
    ) -> Self {
        Self {
            engine,
            executions: None,
            composites: None,
            connectors: Some(connectors),
            application_effects: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_application_effects(
        engine: FlowEngine,
        application_effects: Arc<dyn IWorkflowApplicationEffectsPort>,
    ) -> Self {
        Self {
            engine,
            executions: None,
            composites: None,
            connectors: None,
            application_effects: Some(application_effects),
        }
    }

    async fn apply_application_lifecycle_projection(
        &self,
        record: &WorkflowRunRecord,
    ) -> Result<(), WorkflowRunCoordinationError> {
        let Some(projection) = application_lifecycle_projection(record)? else {
            return Ok(());
        };
        let port = self.application_effects.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Applications semantic-effect coordination is not configured".into(),
            )
        })?;
        if let Some(final_output) = projection.final_output.as_ref() {
            port.append_final_output(final_output)
                .await
                .map_err(|error| application_effect_unavailable("append final output", error))?;
        }
        port.observe_terminal(&projection.terminal)
            .await
            .map_err(|error| application_effect_unavailable("observe terminal state", error))?;
        Ok(())
    }

    async fn coordinate_active_execution(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
    ) -> Result<(), WorkflowRunCoordinationError> {
        let hooks = execution_hooks(record, snapshot, history)?;
        let active = hooks
            .into_iter()
            .filter(|hook| hook.status == HookStatus::Active)
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "WorkflowRun replay exposed more than one active Execution hook".into(),
            ));
        }
        let Some(hook) = active.first() else {
            return Ok(());
        };
        let port = self.executions.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Execution coordination is not configured".into(),
            )
        })?;
        let request = hook.request();
        let execution = match port.start_or_adopt(&request).await {
            Ok(execution) => execution,
            Err(error) if permanent_dispatch_error(&error) => {
                let payload = WorkflowExecutionResumePayload::rejected(
                    &hook.metadata,
                    rejection_reason(&error),
                )
                .map_err(WorkflowRunCoordinationError::Unavailable)?;
                self.engine
                    .resume_hook(
                        &record.run.flow_run_id,
                        &hook.metadata.flow_hook_id(),
                        serde_json::to_value(payload).map_err(|error| {
                            WorkflowRunCoordinationError::Unavailable(error.to_string())
                        })?,
                    )
                    .await
                    .map_err(|error| unavailable_at("reject Execution hook", error))?;
                return Ok(());
            }
            Err(error) => return Err(application_unavailable(error)),
        };
        let linked = self
            .link_child(record, snapshot, &hook.metadata, &execution)
            .await?;
        if linked && execution.status.is_terminal() {
            self.resume_terminal_execution(record, &hook.metadata, &execution)
                .await?;
        }
        Ok(())
    }

    async fn coordinate_active_application_answer(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
    ) -> Result<(), WorkflowRunCoordinationError> {
        let hooks = application_answer_hooks(record, snapshot, history)?;
        let active = hooks
            .into_iter()
            .filter(|hook| hook.status == HookStatus::Active)
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "WorkflowRun replay exposed more than one active Application Answer hook".into(),
            ));
        }
        let Some(hook) = active.first() else {
            return Ok(());
        };
        let port = self.application_effects.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Applications semantic-effect coordination is not configured".into(),
            )
        })?;
        let request = hook
            .request()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        request
            .validate()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let write = match port.append_answer(&request).await {
            Ok(write) => write,
            Err(error) => {
                let classification = application_answer_failure_classification(
                    &record.run.execution_input,
                    &hook.metadata.step_id,
                    &error,
                );
                let Some(classification) = classification else {
                    return Err(application_effect_unavailable("append Answer", error));
                };
                let payload = WorkflowApplicationAnswerFailureResumePayload::new(
                    &hook.metadata,
                    classification,
                )
                .map_err(WorkflowRunCoordinationError::Unavailable)?;
                return self
                    .engine
                    .resume_hook(
                        &record.run.flow_run_id,
                        &hook.metadata.flow_hook_id(),
                        serde_json::to_value(payload).map_err(|error| {
                            WorkflowRunCoordinationError::Unavailable(error.to_string())
                        })?,
                    )
                    .await
                    .map_err(|error| {
                        unavailable_at("resume Application Answer failure hook", error)
                    });
            }
        };
        let message = &write.value;
        message
            .validate()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let expected_effect = request
            .effect
            .effect()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        if message.organization_id != hook.metadata.organization_id
            || message.project_id != hook.metadata.project_id
            || message.kind != ApplicationMessageKind::Answer
            || message.content != hook.metadata.content
            || message.content_digest != hook.metadata.content_digest
            || message.workflow_effect.as_ref() != Some(&expected_effect)
            || message.created_at != hook.created_at
        {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "Workflow Applications Answer commit evidence drifted".into(),
            ));
        }
        let payload = WorkflowApplicationAnswerResumePayload::new(
            &hook.metadata,
            message.id,
            message.sequence,
            message.content_digest.clone(),
        )
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
        self.engine
            .resume_hook(
                &record.run.flow_run_id,
                &hook.metadata.flow_hook_id(),
                serde_json::to_value(payload).map_err(|error| {
                    WorkflowRunCoordinationError::Unavailable(error.to_string())
                })?,
            )
            .await
            .map_err(|error| unavailable_at("resume Application Answer hook", error))
    }

    async fn coordinate_active_application_variables(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
    ) -> Result<(), WorkflowRunCoordinationError> {
        let hooks = application_variable_hooks(record, snapshot, history)?;
        let active = hooks
            .into_iter()
            .filter(|hook| hook.status() == HookStatus::Active)
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "WorkflowRun replay exposed more than one active Application variable hook".into(),
            ));
        }
        let Some(hook) = active.first() else {
            return Ok(());
        };
        let port = self.application_effects.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Applications semantic-effect coordination is not configured".into(),
            )
        })?;
        match hook {
            ObservedApplicationVariableHook::Snapshot {
                metadata,
                status: _,
            } => {
                let observed = port
                    .read_conversation_variables(&WorkflowApplicationRunReference {
                        organization_id: metadata.organization_id,
                        workflow_run_id: metadata.workflow_run_id,
                    })
                    .await
                    .map_err(|error| {
                        application_effect_unavailable("read conversation variables", error)
                    })?;
                observed
                    .validate()
                    .map_err(WorkflowRunCoordinationError::Unavailable)?;
                if observed.organization_id != metadata.organization_id
                    || observed.project_id != metadata.project_id
                    || observed.workflow_run_id != metadata.workflow_run_id
                {
                    return Err(WorkflowRunCoordinationError::Unavailable(
                        "Workflow Applications variable snapshot evidence drifted".into(),
                    ));
                }
                let payload = WorkflowApplicationVariableSnapshotResumePayload::new(
                    metadata,
                    observed.application_id,
                    observed.application_release_id,
                    observed.application_release_digest,
                    observed.session_id,
                    observed.invocation_id,
                    observed.version.revision_id,
                    observed.version.revision_number,
                    observed.version.values_digest,
                    observed.values,
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
                    .map_err(|error| {
                        unavailable_at("resume Application variable snapshot hook", error)
                    })
            }
            ObservedApplicationVariableHook::Write {
                metadata,
                values,
                created_at,
                status: _,
            } => {
                let request = WorkflowApplicationVariableWriteRequest {
                    effect: WorkflowApplicationEffectRequest {
                        organization_id: metadata.organization_id,
                        workflow_run_id: metadata.workflow_run_id,
                        step_id: metadata.step_id.clone(),
                        step_attempt: metadata.step_attempt,
                        effect_ordinal: 0,
                        occurred_at: *created_at,
                    },
                    expected: WorkflowApplicationVariableVersion {
                        revision_id: metadata.expected_revision_id,
                        revision_number: metadata.expected_revision_number,
                        values_digest: metadata.expected_values_digest.clone(),
                    },
                    values: values.clone(),
                };
                request
                    .validate()
                    .map_err(WorkflowRunCoordinationError::Unavailable)?;
                let write = match port.advance_conversation_variables(&request).await {
                    Ok(write) => write,
                    Err(error) => {
                        let classification = application_variable_failure_classification(
                            &record.run.execution_input,
                            &metadata.step_id,
                            &error,
                        );
                        let Some(classification) = classification else {
                            return Err(application_effect_unavailable(
                                "advance conversation variables",
                                error,
                            ));
                        };
                        let payload = WorkflowApplicationVariableWriteFailureResumePayload::new(
                            metadata,
                            classification,
                        )
                        .map_err(WorkflowRunCoordinationError::Unavailable)?;
                        return self
                            .engine
                            .resume_hook(
                                &record.run.flow_run_id,
                                &metadata.flow_hook_id(),
                                serde_json::to_value(payload).map_err(|error| {
                                    WorkflowRunCoordinationError::Unavailable(error.to_string())
                                })?,
                            )
                            .await
                            .map_err(|error| {
                                unavailable_at(
                                    "resume Application variable write failure hook",
                                    error,
                                )
                            });
                    }
                };
                let revision = &write.value;
                revision
                    .validate()
                    .map_err(WorkflowRunCoordinationError::Unavailable)?;
                let expected_effect = request
                    .effect
                    .effect()
                    .map_err(WorkflowRunCoordinationError::Unavailable)?;
                if revision.organization_id != metadata.organization_id
                    || revision.project_id != metadata.project_id
                    || revision.application_id != metadata.application_id
                    || revision.application_release_id != metadata.application_release_id
                    || revision.application_release_digest != metadata.application_release_digest
                    || revision.session_id != metadata.session_id
                    || revision.parent_revision_id != Some(metadata.expected_revision_id)
                    || revision.parent_digest.as_ref() != Some(&metadata.expected_values_digest)
                    || revision.source_effect.as_ref() != Some(&expected_effect)
                    || revision.values != *values
                    || revision.values_digest != metadata.values_digest
                    || revision.created_at != *created_at
                {
                    return Err(WorkflowRunCoordinationError::Unavailable(
                        "Workflow Applications variable commit evidence drifted".into(),
                    ));
                }
                let parent_revision_id = revision.parent_revision_id.ok_or_else(|| {
                    WorkflowRunCoordinationError::Unavailable(
                        "Workflow Applications variable commit lost its parent".into(),
                    )
                })?;
                let parent_digest = revision.parent_digest.clone().ok_or_else(|| {
                    WorkflowRunCoordinationError::Unavailable(
                        "Workflow Applications variable commit lost its parent digest".into(),
                    )
                })?;
                let payload = WorkflowApplicationVariableWriteResumePayload::new(
                    metadata,
                    revision.id,
                    revision.revision_number,
                    parent_revision_id,
                    parent_digest,
                    revision.values_digest.clone(),
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
                    .map_err(|error| {
                        unavailable_at("resume Application variable write hook", error)
                    })
            }
        }
    }

    async fn cancel_execution_children(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
        requested_at: DateTime<Utc>,
    ) -> Result<bool, WorkflowRunCoordinationError> {
        let hooks = execution_hooks(record, snapshot, history)?;
        if hooks.is_empty() {
            return Ok(true);
        }
        let port = self.executions.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Execution coordination is not configured".into(),
            )
        })?;
        let mut all_terminal = true;
        for hook in hooks {
            let request = hook.request();
            let mut execution = match port
                .adopt(&request)
                .await
                .map_err(application_unavailable)?
            {
                Some(execution) => execution,
                None => match port.start_or_adopt(&request).await {
                    Ok(execution) => execution,
                    // No child can be admitted for this immutable request, so there is
                    // nothing to clean up. Conflicts remain fail-closed because they can
                    // also mean an existing child no longer matches the pinned authority.
                    Err(
                        ApplicationError::Invalid(_)
                        | ApplicationError::NotFound(_)
                        | ApplicationError::Forbidden(_),
                    ) => continue,
                    Err(error) => return Err(application_unavailable(error)),
                },
            };
            let linked = self
                .link_child(record, snapshot, &hook.metadata, &execution)
                .await?;
            if !execution.status.is_terminal() {
                let cancellation_at = canonical_timestamp(requested_at.max(execution.updated_at));
                execution = port
                    .request_cancellation(&request, cancellation_at)
                    .await
                    .map_err(application_unavailable)?
                    .ok_or_else(|| {
                        WorkflowRunCoordinationError::Unavailable(
                            "Workflow child Execution disappeared during cancellation".into(),
                        )
                    })?;
            }
            all_terminal &= linked && execution.status.is_terminal();
        }
        Ok(all_terminal)
    }

    async fn link_child(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        hook: &WorkflowExecutionHookMetadata,
        execution: &Execution,
    ) -> Result<bool, WorkflowRunCoordinationError> {
        let invocation_template_digest = Sha256Digest::parse(&execution.template_digest)
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let metadata =
            WorkflowExecutionChildReferenceMetadata::new(hook, invocation_template_digest)
                .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let child = ChildOperationReference::new(
            hook.flow_hook_id(),
            "execution",
            execution.operation_id.to_string(),
        )
        .with_flow_run_id(execution.operation_id.to_string())
        .with_metadata(
            serde_json::to_value(metadata)
                .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?,
        );
        if snapshot.status.is_terminal() {
            return match snapshot.child_operations.get(&child.reference_id) {
                Some(existing) if existing == &child => Ok(true),
                Some(_) => Err(WorkflowRunCoordinationError::Unavailable(
                    "terminal WorkflowRun child Execution reference drifted".into(),
                )),
                // Runs terminated by older coordinators cannot be amended. The child is still
                // cancelled and awaited before their terminal projection is committed.
                None => Ok(true),
            };
        }
        let child_snapshot = match self
            .engine
            .snapshot(&execution.operation_id.to_string())
            .await
        {
            Ok(snapshot) => snapshot,
            Err(FlowError::RunNotFound(_)) => return Ok(false),
            Err(error) => return Err(unavailable_at("read child Execution Flow identity", error)),
        };
        if child_snapshot.run_id != execution.operation_id.to_string()
            || child_snapshot.spec.name != EXECUTION_WORKFLOW_NAME
            || child_snapshot.spec.version != EXECUTION_WORKFLOW_VERSION
        {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "Workflow child Execution Flow identity drifted".into(),
            ));
        }
        self.engine
            .link_child_operation(&record.run.flow_run_id, child)
            .await
            .map_err(|error| unavailable_at("link child Execution", error))
            .map(|()| true)
    }

    async fn resume_terminal_execution(
        &self,
        record: &WorkflowRunRecord,
        hook: &WorkflowExecutionHookMetadata,
        execution: &Execution,
    ) -> Result<(), WorkflowRunCoordinationError> {
        let outcome = match execution.outcome.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "terminal Workflow child Execution has no outcome".into(),
            )
        })? {
            ExecutionOutcome::Succeeded { exit_code } => WorkflowExecutionOutcome::Succeeded {
                exit_code: *exit_code,
            },
            ExecutionOutcome::Failed { exit_code, reason } => WorkflowExecutionOutcome::Failed {
                exit_code: *exit_code,
                reason: reason.clone(),
            },
            ExecutionOutcome::Cancelled => WorkflowExecutionOutcome::Cancelled,
        };
        let output = WorkflowExecutionStepOutput {
            schema: WORKFLOW_EXECUTION_RESULT_SCHEMA.into(),
            execution_id: execution.id,
            operation_id: execution.operation_id,
            execution_template_id: hook.execution_template_id,
            execution_template_revision_id: hook.execution_template_revision_id,
            execution_template_digest: hook.execution_template_digest.clone(),
            invocation_template_digest: Sha256Digest::parse(&execution.template_digest)
                .map_err(WorkflowRunCoordinationError::Unavailable)?,
            outcome,
            finished_at: execution.finished_at.ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "terminal Workflow child Execution has no finish time".into(),
                )
            })?,
        };
        let payload = WorkflowExecutionResumePayload::new(hook, output)
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
            .map_err(|error| unavailable_at("resume terminal child Execution", error))
    }
}

#[async_trait]
impl IWorkflowRunCoordinator for FlowWorkflowRunCoordinator {
    async fn reconcile(
        &self,
        record: &WorkflowRunRecord,
        now: DateTime<Utc>,
    ) -> Result<Option<WorkflowRunRecord>, WorkflowRunCoordinationError> {
        let mut snapshot = match self.engine.snapshot(&record.run.flow_run_id).await {
            Ok(snapshot) => snapshot,
            Err(FlowError::RunNotFound(_)) => {
                return Err(WorkflowRunCoordinationError::Deferred(
                    "the correlated Operation has not started A3S Flow".into(),
                ))
            }
            Err(error) => return Err(unavailable(error)),
        };
        let mut history = self
            .engine
            .history(&record.run.flow_run_id)
            .await
            .map_err(unavailable)?;
        if let Err(error) = verify_flow_authority(record, &snapshot, &history) {
            return project_drift(record, &snapshot, &history, error).map(Some);
        }
        let cancelling = record.run.status == WorkflowRunStatus::Cancelling;
        let timed_out = !cancelling && now >= record.run.execution_input.deadline_at;
        if cancelling || timed_out {
            let execution_children_terminal = self
                .cancel_execution_children(record, &snapshot, &history, now)
                .await?;
            let composite_children_terminal = self
                .cancel_composite_children(record, &snapshot, &history)
                .await?;
            if !execution_children_terminal || !composite_children_terminal {
                return Ok(None);
            }
            if !snapshot.status.is_terminal() {
                if cancelling {
                    self.engine
                        .request_cancellation(
                            &record.run.flow_run_id,
                            CancellationRequest::new(record.run.cancellation_reason.clone()),
                        )
                        .await
                        .map_err(unavailable)?;
                } else {
                    self.engine
                        .terminate_for_timeout(
                            &record.run.flow_run_id,
                            record.run.execution_input.deadline_at,
                            Some("WorkflowRun exceeded its immutable deadline".into()),
                        )
                        .await
                        .map_err(unavailable)?;
                }
            }
        } else if !snapshot.status.is_terminal() {
            self.coordinate_active_application_variables(record, &snapshot, &history)
                .await?;
            self.coordinate_active_application_answer(record, &snapshot, &history)
                .await?;
            self.coordinate_active_execution(record, &snapshot, &history)
                .await?;
            self.coordinate_active_composite(record, &snapshot, &history)
                .await?;
            self.coordinate_active_connector(record, &snapshot, &history)
                .await?;
        }
        snapshot = self
            .engine
            .snapshot(&record.run.flow_run_id)
            .await
            .map_err(|error| unavailable_at("refresh WorkflowRun snapshot", error))?;
        history = self
            .engine
            .history(&record.run.flow_run_id)
            .await
            .map_err(|error| unavailable_at("refresh WorkflowRun history", error))?;
        let projected = match project_workflow_run_record(record, &snapshot, &history) {
            Ok(projected) => projected,
            Err(error) => Some(project_drift(record, &snapshot, &history, error)?),
        };
        if let Some(projected) = projected.as_ref() {
            self.apply_application_lifecycle_projection(projected)
                .await?;
        }
        Ok(projected)
    }
}

struct ApplicationLifecycleProjection {
    final_output: Option<WorkflowApplicationMessageRequest>,
    terminal: WorkflowApplicationTerminalRequest,
}

fn application_lifecycle_projection(
    record: &WorkflowRunRecord,
) -> Result<Option<ApplicationLifecycleProjection>, WorkflowRunCoordinationError> {
    let Some(application) = record.run.execution_input.application_projection.as_ref() else {
        return Ok(None);
    };
    application
        .validate(&record.run.execution_input.plan)
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
    if !application.projects_application_lifecycle() {
        return Ok(None);
    }
    let status = match record.run.status {
        WorkflowRunStatus::Completed => ApplicationInvocationStatus::Succeeded,
        WorkflowRunStatus::Failed | WorkflowRunStatus::TimedOut => {
            ApplicationInvocationStatus::Failed
        }
        WorkflowRunStatus::Cancelled => ApplicationInvocationStatus::Cancelled,
        WorkflowRunStatus::Pending
        | WorkflowRunStatus::Running
        | WorkflowRunStatus::Waiting
        | WorkflowRunStatus::Cancelling => return Ok(None),
    };
    let completed_at = record.run.finished_at.ok_or_else(|| {
        WorkflowRunCoordinationError::Unavailable(
            "terminal Application WorkflowRun has no finish time".into(),
        )
    })?;
    let final_output = if status == ApplicationInvocationStatus::Succeeded {
        let step = record
            .steps
            .iter()
            .find(|step| step.step_id == application.final_output_step_id)
            .ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "Application WorkflowRun lost its final Output projection".into(),
                )
            })?;
        let output = record.run.output.clone().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "completed Application WorkflowRun lost its final output".into(),
            )
        })?;
        if step.kind != WorkflowStepKind::Output
            || step.status != WorkflowStepProjectionStatus::Completed
            || step.attempt_generation == 0
            || step.result.as_ref() != Some(&output)
        {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "Application WorkflowRun final Output projection drifted".into(),
            ));
        }
        Some(WorkflowApplicationMessageRequest {
            effect: WorkflowApplicationEffectRequest {
                organization_id: record.run.organization_id,
                workflow_run_id: record.run.id,
                step_id: application.final_output_step_id.clone(),
                step_attempt: step.attempt_generation,
                effect_ordinal: 0,
                occurred_at: completed_at,
            },
            content: output,
        })
    } else {
        None
    };
    let terminal = WorkflowApplicationTerminalRequest {
        organization_id: record.run.organization_id,
        workflow_run_id: record.run.id,
        status,
        completed_at,
    };
    final_output
        .as_ref()
        .map(WorkflowApplicationMessageRequest::validate)
        .transpose()
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
    terminal
        .validate()
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
    Ok(Some(ApplicationLifecycleProjection {
        final_output,
        terminal,
    }))
}

#[derive(Debug, Clone)]
struct ObservedExecutionHook {
    metadata: WorkflowExecutionHookMetadata,
    created_at: DateTime<Utc>,
    status: HookStatus,
}

impl ObservedExecutionHook {
    fn request(&self) -> WorkflowExecutionRequest {
        WorkflowExecutionRequest {
            organization_id: self.metadata.organization_id,
            project_id: self.metadata.project_id,
            environment_id: self.metadata.environment_id,
            workflow_run_id: self.metadata.workflow_run_id,
            plan_revision_id: self.metadata.plan_revision_id,
            plan_digest: self.metadata.plan_digest.clone(),
            step_id: self.metadata.step_id.clone(),
            step_attempt: self.metadata.step_attempt,
            execution_template_id: self.metadata.execution_template_id,
            execution_template_revision_id: self.metadata.execution_template_revision_id,
            execution_template_digest: self.metadata.execution_template_digest.clone(),
            capability: self.metadata.capability.clone(),
            input: self.metadata.effective_input.clone(),
            requested_at: self.created_at,
        }
    }
}

fn execution_hooks(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
) -> Result<Vec<ObservedExecutionHook>, WorkflowRunCoordinationError> {
    let mut hooks = Vec::new();
    for resolved in record
        .run
        .execution_input
        .resolved_steps()
        .map_err(WorkflowRunCoordinationError::Unavailable)?
    {
        if resolved.plan.kind != WorkflowStepKind::Execution {
            continue;
        }
        let Some((hook, metadata)) =
            execution_hook(&record.run.execution_input, &resolved, snapshot)
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
                "Workflow execution hook {:?} must have exactly one creation event",
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
                "Workflow execution hook {:?} creation history is invalid",
                metadata.flow_hook_id()
            )));
        };
        if token != &metadata.flow_hook_token() || observed_metadata != &expected_metadata {
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow execution hook {:?} creation authority drifted",
                metadata.flow_hook_id()
            )));
        }
        hooks.push(ObservedExecutionHook {
            metadata,
            created_at: canonical_timestamp(matching[0].timestamp),
            status: hook.status,
        });
    }
    Ok(hooks)
}

fn permanent_dispatch_error(error: &ApplicationError) -> bool {
    matches!(
        error,
        ApplicationError::Invalid(_)
            | ApplicationError::NotFound(_)
            | ApplicationError::Conflict(_)
            | ApplicationError::Forbidden(_)
    )
}

fn application_variable_failure_classification(
    input: &crate::modules::workflow::domain::WorkflowRunInput,
    step_id: &str,
    error: &ApplicationError,
) -> Option<WorkflowStepFailureClassification> {
    if !matches!(
        input.schema.as_str(),
        WORKFLOW_RUN_INPUT_SCHEMA_V14
            | WORKFLOW_RUN_INPUT_SCHEMA_V15
            | WORKFLOW_RUN_INPUT_SCHEMA_V16
            | WORKFLOW_RUN_INPUT_SCHEMA_V17
    ) || !input
        .plan
        .edges
        .iter()
        .any(|edge| edge.source == step_id && edge.source_handle.as_deref() == Some("error"))
    {
        return None;
    }
    match error {
        ApplicationError::Invalid(_) => Some(WorkflowStepFailureClassification::ApplicationInvalid),
        ApplicationError::NotFound(_) => {
            Some(WorkflowStepFailureClassification::ApplicationNotFound)
        }
        ApplicationError::Conflict(_) => {
            Some(WorkflowStepFailureClassification::ApplicationConflict)
        }
        ApplicationError::Forbidden(_) => {
            Some(WorkflowStepFailureClassification::ApplicationForbidden)
        }
        ApplicationError::Unavailable(_) | ApplicationError::Internal(_) => None,
    }
}

fn application_answer_failure_classification(
    input: &crate::modules::workflow::domain::WorkflowRunInput,
    step_id: &str,
    error: &ApplicationError,
) -> Option<WorkflowStepFailureClassification> {
    if !matches!(
        input.schema.as_str(),
        WORKFLOW_RUN_INPUT_SCHEMA_V15
            | WORKFLOW_RUN_INPUT_SCHEMA_V16
            | WORKFLOW_RUN_INPUT_SCHEMA_V17
    ) || !input
        .plan
        .edges
        .iter()
        .any(|edge| edge.source == step_id && edge.source_handle.as_deref() == Some("error"))
    {
        return None;
    }
    match error {
        ApplicationError::Invalid(_) => Some(WorkflowStepFailureClassification::ApplicationInvalid),
        ApplicationError::NotFound(_) => {
            Some(WorkflowStepFailureClassification::ApplicationNotFound)
        }
        ApplicationError::Conflict(_) => {
            Some(WorkflowStepFailureClassification::ApplicationConflict)
        }
        ApplicationError::Forbidden(_) => {
            Some(WorkflowStepFailureClassification::ApplicationForbidden)
        }
        ApplicationError::Unavailable(_) | ApplicationError::Internal(_) => None,
    }
}

fn rejection_reason(error: &ApplicationError) -> String {
    let sanitized = error
        .to_string()
        .replace(['\0', '\r', '\n'], " ")
        .chars()
        .take(8 * 1024)
        .collect::<String>();
    format!("Execution dispatch rejected: {sanitized}")
}

fn application_unavailable(error: ApplicationError) -> WorkflowRunCoordinationError {
    WorkflowRunCoordinationError::Unavailable(error.to_string())
}

fn application_effect_unavailable(
    operation: &str,
    error: ApplicationError,
) -> WorkflowRunCoordinationError {
    WorkflowRunCoordinationError::Unavailable(format!(
        "could not {operation} for Application WorkflowRun: {error}"
    ))
}

fn project_drift(
    record: &WorkflowRunRecord,
    snapshot: &a3s_flow::WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
    error: String,
) -> Result<WorkflowRunRecord, WorkflowRunCoordinationError> {
    let observed_at = history
        .last()
        .map(|event| event.timestamp)
        .unwrap_or_else(Utc::now);
    let started_at = history.iter().find_map(|event| {
        matches!(event.event, a3s_flow::FlowEvent::RunStarted).then_some(event.timestamp)
    });
    let build_id = snapshot
        .spec
        .runtime_build_id
        .as_ref()
        .map(|value| value.as_str().to_owned())
        .unwrap_or_else(|| "unpinned-flow-runtime".into());
    let mut projected = record.clone();
    projected
        .run
        .project_flow(crate::modules::workflow::domain::WorkflowRunFlowState {
            status: WorkflowRunStatus::Failed,
            flow_runtime_build_id: build_id,
            last_flow_sequence: snapshot.last_sequence.max(1),
            output: None,
            error: Some(format!("WorkflowRun replay drift: {error}")),
            started_at,
            finished_at: Some(observed_at),
            observed_at,
        })
        .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
    for step in &mut projected.steps {
        if step.status.is_terminal() {
            continue;
        }
        step.project_flow(crate::modules::workflow::domain::WorkflowStepFlowState {
            status: crate::modules::workflow::domain::WorkflowStepProjectionStatus::Cancelled,
            attempt_generation: step.attempt_generation,
            selected_handle: None,
            result: None,
            error: None,
            default_output_evidence: None,
            last_flow_sequence: snapshot.last_sequence.max(1),
            observed_at,
        })
        .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
    }
    projected
        .validate()
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
    Ok(projected)
}

fn unavailable(error: FlowError) -> WorkflowRunCoordinationError {
    WorkflowRunCoordinationError::Unavailable(error.to_string())
}

fn unavailable_at(operation: &str, error: FlowError) -> WorkflowRunCoordinationError {
    WorkflowRunCoordinationError::Unavailable(format!("could not {operation}: {error}"))
}

#[cfg(test)]
mod application_tests;
#[cfg(test)]
mod connector_tests;
#[cfg(test)]
mod tests;
