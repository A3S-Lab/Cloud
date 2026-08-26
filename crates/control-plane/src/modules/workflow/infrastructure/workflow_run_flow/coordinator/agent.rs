use super::{
    application_unavailable, permanent_dispatch_error, unavailable_at, FlowWorkflowRunCoordinator,
};
use crate::modules::agents::{
    AgentExecution, AgentExecutionStatus, WorkflowAgentRequest, AGENT_EXECUTION_WORKFLOW_NAME,
    AGENT_EXECUTION_WORKFLOW_VERSION,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{canonical_timestamp, Sha256Digest};
use crate::modules::workflow::domain::{
    WorkflowAgentChildReferenceMetadata, WorkflowAgentHookMetadata, WorkflowAgentOutcome,
    WorkflowAgentProviderEvidence, WorkflowAgentResumePayload, WorkflowAgentStepOutput,
    WorkflowRunCoordinationError, WorkflowRunRecord, WorkflowStepKind,
    WORKFLOW_AGENT_RESULT_SCHEMA,
};
use a3s_flow::{
    ChildOperationReference, FlowError, FlowEvent, FlowEventEnvelope, HookStatus,
    WorkflowRunSnapshot,
};
use chrono::{DateTime, Utc};

impl FlowWorkflowRunCoordinator {
    pub(super) async fn coordinate_active_agent(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[FlowEventEnvelope],
    ) -> Result<(), WorkflowRunCoordinationError> {
        let hooks = agent_hooks(record, snapshot, history)?;
        let active = hooks
            .into_iter()
            .filter(|hook| hook.status == HookStatus::Active)
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "WorkflowRun replay exposed more than one active Agent hook".into(),
            ));
        }
        let Some(hook) = active.first() else {
            return Ok(());
        };
        let port = self.agents.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Agent coordination is not configured".into(),
            )
        })?;
        let request = hook.request();
        let execution = match port.start_or_adopt(&request).await {
            Ok(execution) => execution,
            Err(error) if permanent_dispatch_error(&error) => {
                let payload = WorkflowAgentResumePayload::rejected(
                    &hook.metadata,
                    agent_rejection_reason(&error),
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
                    .map_err(|error| unavailable_at("reject Agent hook", error))?;
                return Ok(());
            }
            Err(error) => return Err(application_unavailable(error)),
        };
        let linked = self
            .link_agent_child(record, snapshot, &hook.metadata, &execution)
            .await?;
        if linked && execution.status.is_terminal() {
            self.resume_terminal_agent(record, &hook.metadata, &request, &execution)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn cancel_agent_children(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[FlowEventEnvelope],
        requested_at: DateTime<Utc>,
    ) -> Result<bool, WorkflowRunCoordinationError> {
        let hooks = agent_hooks(record, snapshot, history)?;
        if hooks.is_empty() {
            return Ok(true);
        }
        let port = self.agents.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Agent coordination is not configured".into(),
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
                    Err(
                        ApplicationError::Invalid(_)
                        | ApplicationError::NotFound(_)
                        | ApplicationError::Forbidden(_),
                    ) => continue,
                    Err(error) => return Err(application_unavailable(error)),
                },
            };
            let linked = self
                .link_agent_child(record, snapshot, &hook.metadata, &execution)
                .await?;
            if !execution.status.is_terminal() {
                let cancellation_at = canonical_timestamp(requested_at.max(execution.updated_at));
                execution = port
                    .request_cancellation(&request, cancellation_at)
                    .await
                    .map_err(application_unavailable)?
                    .ok_or_else(|| {
                        WorkflowRunCoordinationError::Unavailable(
                            "Workflow child Agent execution disappeared during cancellation".into(),
                        )
                    })?;
            }
            all_terminal &= linked && execution.status.is_terminal();
        }
        Ok(all_terminal)
    }

    async fn link_agent_child(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        hook: &WorkflowAgentHookMetadata,
        execution: &AgentExecution,
    ) -> Result<bool, WorkflowRunCoordinationError> {
        let metadata = WorkflowAgentChildReferenceMetadata::new(
            hook,
            execution.conversation_id,
            execution.id,
            execution.operation_id,
        )
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let child = ChildOperationReference::new(
            hook.flow_hook_id(),
            "agent_execution",
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
                    "terminal WorkflowRun child Agent reference drifted".into(),
                )),
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
            Err(error) => {
                return Err(unavailable_at(
                    "read child Agent execution Flow identity",
                    error,
                ))
            }
        };
        if child_snapshot.run_id != execution.operation_id.to_string()
            || child_snapshot.spec.name != AGENT_EXECUTION_WORKFLOW_NAME
            || child_snapshot.spec.version != AGENT_EXECUTION_WORKFLOW_VERSION
        {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "Workflow child Agent execution Flow identity drifted".into(),
            ));
        }
        self.engine
            .link_child_operation(&record.run.flow_run_id, child)
            .await
            .map_err(|error| unavailable_at("link child Agent execution", error))
            .map(|()| true)
    }

    async fn resume_terminal_agent(
        &self,
        record: &WorkflowRunRecord,
        hook: &WorkflowAgentHookMetadata,
        request: &WorkflowAgentRequest,
        execution: &AgentExecution,
    ) -> Result<(), WorkflowRunCoordinationError> {
        let port = self.agents.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Agent coordination is not configured".into(),
            )
        })?;
        let observation = port
            .terminal_observation(request, execution)
            .await
            .map_err(application_unavailable)?
            .ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "terminal Workflow child Agent execution has no terminal observation".into(),
                )
            })?;
        if observation.execution != *execution {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "Workflow child Agent terminal observation changed its execution".into(),
            ));
        }
        let outcome = match execution.status {
            AgentExecutionStatus::Succeeded => WorkflowAgentOutcome::Succeeded,
            AgentExecutionStatus::Failed => WorkflowAgentOutcome::Failed {
                reason: execution.failure.clone().ok_or_else(|| {
                    WorkflowRunCoordinationError::Unavailable(
                        "failed Workflow child Agent execution has no reason".into(),
                    )
                })?,
            },
            AgentExecutionStatus::Cancelled => WorkflowAgentOutcome::Cancelled,
            _ => {
                return Err(WorkflowRunCoordinationError::Unavailable(
                    "non-terminal Workflow child Agent execution reached terminal resume".into(),
                ))
            }
        };
        let provider = execution
            .code
            .as_ref()
            .map(|binding| {
                let profile = binding.provider()?;
                let identity = binding.provider_identity()?;
                Ok::<WorkflowAgentProviderEvidence, String>(WorkflowAgentProviderEvidence {
                    kind: profile.kind().into(),
                    revision: profile.revision().into(),
                    protocol: profile.protocol().into(),
                    native_protocol: profile.native_protocol().into(),
                    profile_digest: Sha256Digest::parse(profile.profile_digest())?,
                    capability_digest: Sha256Digest::parse(profile.capability_digest())?,
                    session_id: identity.session_id,
                    run_id: identity.run_id,
                })
            })
            .transpose()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let output = WorkflowAgentStepOutput {
            schema: WORKFLOW_AGENT_RESULT_SCHEMA.into(),
            conversation_id: execution.conversation_id,
            agent_execution_id: execution.id,
            operation_id: execution.operation_id,
            agent_asset_id: execution.agent.asset_id(),
            agent_asset_release_id: execution.agent.asset_release_id(),
            agent_release_digest: execution.agent.artifact_digest().clone(),
            provider,
            outcome,
            text: observation.output_text,
            terminal_event_sequence: observation.terminal_event_sequence,
            finished_at: execution.finished_at.ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "terminal Workflow child Agent execution has no finish time".into(),
                )
            })?,
        };
        let payload = WorkflowAgentResumePayload::new(hook, output)
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
            .map_err(|error| unavailable_at("resume terminal child Agent execution", error))
    }
}

#[derive(Debug, Clone)]
struct ObservedAgentHook {
    metadata: WorkflowAgentHookMetadata,
    created_at: DateTime<Utc>,
    status: HookStatus,
}

impl ObservedAgentHook {
    fn request(&self) -> WorkflowAgentRequest {
        WorkflowAgentRequest {
            organization_id: self.metadata.organization_id,
            project_id: self.metadata.project_id,
            environment_id: self.metadata.environment_id,
            workflow_run_id: self.metadata.workflow_run_id,
            plan_revision_id: self.metadata.plan_revision_id,
            plan_digest: self.metadata.plan_digest.clone(),
            step_id: self.metadata.step_id.clone(),
            step_attempt: self.metadata.step_attempt,
            agent_asset_id: self.metadata.agent_asset_id,
            agent_asset_release_id: self.metadata.agent_asset_release_id,
            agent_release_digest: self.metadata.agent_release_digest.clone(),
            capability: self.metadata.capability.clone(),
            input: self.metadata.effective_input.clone(),
            requested_at: self.created_at,
        }
    }
}

fn agent_hooks(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[FlowEventEnvelope],
) -> Result<Vec<ObservedAgentHook>, WorkflowRunCoordinationError> {
    let mut hooks = Vec::new();
    for resolved in record
        .run
        .execution_input
        .resolved_steps()
        .map_err(WorkflowRunCoordinationError::Unavailable)?
    {
        if resolved.plan.kind != WorkflowStepKind::Agent {
            continue;
        }
        let Some((hook, metadata)) =
            super::super::projection::agent_hook(&record.run.execution_input, &resolved, snapshot)
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
                "Workflow Agent hook {:?} must have exactly one creation event",
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
                "Workflow Agent hook {:?} creation history is invalid",
                metadata.flow_hook_id()
            )));
        };
        if token != &metadata.flow_hook_token() || observed_metadata != &expected_metadata {
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow Agent hook {:?} creation authority drifted",
                metadata.flow_hook_id()
            )));
        }
        hooks.push(ObservedAgentHook {
            metadata,
            created_at: canonical_timestamp(matching[0].timestamp),
            status: hook.status,
        });
    }
    Ok(hooks)
}

fn agent_rejection_reason(error: &ApplicationError) -> String {
    let sanitized = error
        .to_string()
        .replace(['\0', '\r', '\n'], " ")
        .chars()
        .take(8 * 1024)
        .collect::<String>();
    format!("Agent dispatch rejected: {sanitized}")
}
