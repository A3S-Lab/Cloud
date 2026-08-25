use super::{application_unavailable, permanent_dispatch_error, unavailable_at};
use crate::modules::connectors::{ConnectorExecutionOutcome, WorkflowConnectorAttemptResult};
use crate::modules::workflow::domain::{
    WorkflowConnectorAttemptEvidence, WorkflowConnectorAttemptOutcome,
    WorkflowConnectorHookMetadata, WorkflowConnectorResponseObjectReference,
    WorkflowConnectorResumePayload, WorkflowRunCoordinationError, WorkflowRunRecord,
    WorkflowStepKind,
};
use a3s_flow::{FlowEvent, HookStatus, WorkflowRunSnapshot};

struct CoordinatedConnectorHook {
    metadata: WorkflowConnectorHookMetadata,
    status: HookStatus,
}

impl super::FlowWorkflowRunCoordinator {
    pub(super) async fn coordinate_active_connector(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
    ) -> Result<(), WorkflowRunCoordinationError> {
        let active = coordinated_hooks(record, snapshot, history)?
            .into_iter()
            .filter(|hook| hook.status == HookStatus::Active)
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "WorkflowRun replay exposed more than one active Connector hook".into(),
            ));
        }
        let Some(hook) = active.first() else {
            return Ok(());
        };
        let port = self.connectors.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Connector coordination is not configured".into(),
            )
        })?;
        let request = super::super::connector::attempt_request(&hook.metadata);
        let authority = request
            .connector_attempt_authority()
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let payload = match port.execute_attempt(&request).await {
            Ok(WorkflowConnectorAttemptResult::Completed {
                evidence,
                response_object,
            }) => {
                let outcome = match evidence.outcome() {
                    ConnectorExecutionOutcome::Accepted => {
                        WorkflowConnectorAttemptOutcome::Accepted
                    }
                    ConnectorExecutionOutcome::Retryable => {
                        WorkflowConnectorAttemptOutcome::Retryable
                    }
                    ConnectorExecutionOutcome::Rejected => {
                        WorkflowConnectorAttemptOutcome::Rejected
                    }
                    ConnectorExecutionOutcome::Indeterminate => {
                        return Err(WorkflowRunCoordinationError::Unavailable(
                            "terminal indeterminate Connector evidence bypassed its attempt projection"
                                .into(),
                        ))
                    }
                };
                let response_object = response_object
                    .map(|reference| {
                        WorkflowConnectorResponseObjectReference::new(
                            reference.connector_attempt_id,
                            reference.object_ref,
                            reference.digest,
                            reference.size_bytes,
                        )
                    })
                    .transpose()
                    .map_err(WorkflowRunCoordinationError::Unavailable)?;
                let evidence = if hook.metadata.requires_response_object() {
                    WorkflowConnectorAttemptEvidence::restore_with_response_object(
                        evidence.attempt_id(),
                        evidence.request_digest().clone(),
                        evidence.request_body_bytes(),
                        outcome,
                        evidence.response_status(),
                        evidence.response_digest().cloned(),
                        evidence.response_body_bytes(),
                        response_object,
                        evidence.retry_after().map(|delay| delay.as_secs()),
                        evidence.started_at(),
                        evidence.completed_at(),
                    )
                } else {
                    if response_object.is_some() {
                        return Err(WorkflowRunCoordinationError::Unavailable(
                            "legacy Workflow Connector result exposed a response object".into(),
                        ));
                    }
                    WorkflowConnectorAttemptEvidence::restore(
                        evidence.attempt_id(),
                        evidence.request_digest().clone(),
                        evidence.request_body_bytes(),
                        outcome,
                        evidence.response_status(),
                        evidence.response_digest().cloned(),
                        evidence.response_body_bytes(),
                        evidence.retry_after().map(|delay| delay.as_secs()),
                        evidence.started_at(),
                        evidence.completed_at(),
                    )
                }
                .map_err(WorkflowRunCoordinationError::Unavailable)?;
                WorkflowConnectorResumePayload::completed(
                    &hook.metadata,
                    evidence,
                    authority.attempt_id,
                    &authority.request_digest,
                    authority.request_body_bytes,
                )
                .map_err(WorkflowRunCoordinationError::Unavailable)?
            }
            Ok(WorkflowConnectorAttemptResult::Deferred {
                attempt_id,
                retry_not_before,
            }) => {
                if attempt_id != authority.attempt_id {
                    return Err(WorkflowRunCoordinationError::Unavailable(
                        "Workflow Connector deferred result changed its attempt identity".into(),
                    ));
                }
                WorkflowConnectorResumePayload::deferred(
                    &hook.metadata,
                    authority.attempt_id,
                    retry_not_before,
                    &authority.request_digest,
                    authority.request_body_bytes,
                )
                .map_err(WorkflowRunCoordinationError::Unavailable)?
            }
            Ok(WorkflowConnectorAttemptResult::Indeterminate {
                attempt_id,
                dispatch_started_at,
                outcome_deadline_at,
            }) => {
                if attempt_id != authority.attempt_id {
                    return Err(WorkflowRunCoordinationError::Unavailable(
                        "Workflow Connector indeterminate result changed its attempt identity"
                            .into(),
                    ));
                }
                WorkflowConnectorResumePayload::indeterminate(
                    &hook.metadata,
                    authority.attempt_id,
                    dispatch_started_at,
                    outcome_deadline_at,
                    &authority.request_digest,
                    authority.request_body_bytes,
                )
                .map_err(WorkflowRunCoordinationError::Unavailable)?
            }
            Err(error) if permanent_dispatch_error(&error) => {
                WorkflowConnectorResumePayload::rejected(
                    &hook.metadata,
                    connector_rejection_reason(&error),
                    authority.attempt_id,
                    &authority.request_digest,
                    authority.request_body_bytes,
                )
                .map_err(WorkflowRunCoordinationError::Unavailable)?
            }
            Err(error) => return Err(application_unavailable(error)),
        };
        self.engine
            .resume_hook(
                &record.run.flow_run_id,
                &hook.metadata.flow_hook_id(),
                serde_json::to_value(payload).map_err(|error| {
                    WorkflowRunCoordinationError::Unavailable(error.to_string())
                })?,
            )
            .await
            .map_err(|error| unavailable_at("resume Workflow Connector hook", error))
    }
}

fn coordinated_hooks(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
) -> Result<Vec<CoordinatedConnectorHook>, WorkflowRunCoordinationError> {
    let input = &record.run.execution_input;
    let mut hooks = Vec::new();
    for step in input
        .resolved_steps()
        .map_err(WorkflowRunCoordinationError::Unavailable)?
    {
        if step.plan.kind != WorkflowStepKind::Service {
            continue;
        }
        for observed in super::super::connector::observed_connector_hooks(input, &step, snapshot)
            .map_err(WorkflowRunCoordinationError::Unavailable)?
        {
            verify_creation(history, observed.hook, &observed.metadata)?;
            hooks.push(CoordinatedConnectorHook {
                metadata: observed.metadata,
                status: observed.hook.status,
            });
        }
    }
    Ok(hooks)
}

fn verify_creation(
    history: &[a3s_flow::FlowEventEnvelope],
    hook: &a3s_flow::HookSnapshot,
    metadata: &WorkflowConnectorHookMetadata,
) -> Result<(), WorkflowRunCoordinationError> {
    let matching = history
        .iter()
        .filter(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::HookCreated { hook_id, .. } if hook_id == &hook.hook_id
            )
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(WorkflowRunCoordinationError::Unavailable(format!(
            "Workflow Connector hook {:?} must have exactly one creation event",
            hook.hook_id
        )));
    }
    let FlowEvent::HookCreated {
        token,
        metadata: observed_metadata,
        ..
    } = &matching[0].event
    else {
        return Err(WorkflowRunCoordinationError::Unavailable(
            "Workflow Connector creation history is invalid".into(),
        ));
    };
    let expected_metadata = serde_json::to_value(metadata)
        .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
    if token != &metadata.flow_hook_token() || observed_metadata != &expected_metadata {
        return Err(WorkflowRunCoordinationError::Unavailable(format!(
            "Workflow Connector hook {:?} creation authority drifted",
            hook.hook_id
        )));
    }
    Ok(())
}

fn connector_rejection_reason(
    error: &crate::modules::shared_kernel::application::ApplicationError,
) -> &'static str {
    use crate::modules::shared_kernel::application::ApplicationError;
    match error {
        ApplicationError::Invalid(_) => {
            "Connector dispatch rejected by immutable request validation"
        }
        ApplicationError::NotFound(_) => {
            "Connector dispatch rejected because the revision is unavailable"
        }
        ApplicationError::Conflict(_) => "Connector dispatch rejected by an authority conflict",
        ApplicationError::Forbidden(_) => "Connector dispatch rejected by resource authorization",
        _ => "Connector dispatch rejected",
    }
}
