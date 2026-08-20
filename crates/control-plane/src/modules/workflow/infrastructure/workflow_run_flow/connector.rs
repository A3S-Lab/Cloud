use super::WorkflowLocalStepResult;
use crate::modules::connectors::domain::MAXIMUM_CONNECTOR_BODY_BYTES;
use crate::modules::connectors::{
    WorkflowConnectorAttemptAuthority, WorkflowConnectorAttemptRequest,
    WorkflowConnectorResponseMode,
};
use crate::modules::workflow::domain::{
    ResolvedWorkflowRunStep, WorkflowConnectorAttemptOutcome, WorkflowConnectorHookMetadata,
    WorkflowConnectorResumePayload, WorkflowConnectorResumeResolution, WorkflowConnectorStepOutput,
    WorkflowRetryPolicy, WorkflowRunInput, WorkflowStepKind,
    WORKFLOW_CONNECTOR_MAX_OBSERVATIONS_PER_ATTEMPT,
};
use a3s_flow::{FlowError, HookSnapshot, HookStatus, WorkflowContext, WorkflowRunSnapshot};
use chrono::{DateTime, Duration, Utc};

pub(super) enum ConnectorStepResolution {
    Await(Box<WorkflowConnectorHookMetadata>),
    Wait {
        wait_id: String,
        resume_at: DateTime<Utc>,
    },
    Complete(Box<WorkflowLocalStepResult>),
    Consume(Box<super::connector_response::WorkflowConnectorResponseStepInput>),
    Failed(String),
}

pub(super) enum ConnectorStepError {
    Invalid(String),
    NonDeterministic(String),
}

pub(super) fn resolve_step(
    run_id: &str,
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    effective_input: serde_json::Value,
    context: &WorkflowContext<'_>,
) -> Result<ConnectorStepResolution, ConnectorStepError> {
    let retry_policy = step
        .policy
        .as_ref()
        .and_then(|policy| policy.retry)
        .ok_or_else(|| {
            ConnectorStepError::Invalid(
                "Workflow Connector step lost its immutable retry policy".into(),
            )
        })?;
    retry_policy
        .validate()
        .map_err(ConnectorStepError::Invalid)?;
    for step_attempt in 1..=retry_policy.maximum_attempts {
        let mut observation = 1_u32;
        loop {
            let metadata = WorkflowConnectorHookMetadata::from_run_step(
                input,
                step,
                effective_input.clone(),
                step_attempt,
                observation,
            )
            .map_err(ConnectorStepError::Invalid)?;
            let authority = attempt_authority(&metadata).map_err(ConnectorStepError::Invalid)?;
            let hook_id = metadata.flow_hook_id();
            if context.hook_disposed(&hook_id) {
                return Ok(ConnectorStepResolution::Failed(format!(
                    "Workflow Connector hook for step {:?}, attempt {step_attempt}, observation {observation} was disposed",
                    step.plan.id
                )));
            }
            let Some(observed) = context.hook_payload(&hook_id) else {
                return Ok(ConnectorStepResolution::Await(Box::new(metadata)));
            };
            let payload = decode_payload(run_id, &metadata, observed, &authority)?;
            match payload.resolution {
                WorkflowConnectorResumeResolution::Completed { evidence } => {
                    match evidence.outcome {
                        WorkflowConnectorAttemptOutcome::Accepted => {
                            return accepted_resolution(
                                input, step, &metadata, &evidence, &authority,
                            )
                            .map_err(ConnectorStepError::Invalid);
                        }
                        WorkflowConnectorAttemptOutcome::Rejected => {
                            return Ok(ConnectorStepResolution::Failed(format!(
                                "Workflow Connector step {:?} was rejected at attempt {step_attempt}{}",
                                step.plan.id,
                                status_suffix(evidence.response_status)
                            )));
                        }
                        WorkflowConnectorAttemptOutcome::Retryable => {
                            if step_attempt == retry_policy.maximum_attempts {
                                return Ok(ConnectorStepResolution::Failed(format!(
                                    "Workflow Connector step {:?} exhausted {} attempts{}",
                                    step.plan.id,
                                    retry_policy.maximum_attempts,
                                    status_suffix(evidence.response_status)
                                )));
                            }
                            let resume_at =
                                retry_resume_at(&evidence, retry_policy, input.deadline_at)
                                    .map_err(ConnectorStepError::Invalid)?;
                            let wait_id = metadata.retry_wait_id();
                            if !context.wait_completed(&wait_id) {
                                return Ok(ConnectorStepResolution::Wait { wait_id, resume_at });
                            }
                            break;
                        }
                    }
                }
                WorkflowConnectorResumeResolution::Deferred {
                    retry_not_before, ..
                } => {
                    if observation == WORKFLOW_CONNECTOR_MAX_OBSERVATIONS_PER_ATTEMPT {
                        return Ok(ConnectorStepResolution::Failed(format!(
                            "Workflow Connector step {:?} exceeded {} observations for attempt {step_attempt}",
                            step.plan.id, WORKFLOW_CONNECTOR_MAX_OBSERVATIONS_PER_ATTEMPT
                        )));
                    }
                    let wait_id = metadata.observation_wait_id();
                    let resume_at = bounded_resume_at(retry_not_before, input.deadline_at);
                    if !context.wait_completed(&wait_id) {
                        return Ok(ConnectorStepResolution::Wait { wait_id, resume_at });
                    }
                    observation = observation.checked_add(1).ok_or_else(|| {
                        ConnectorStepError::Invalid(
                            "Workflow Connector observation generation overflowed".into(),
                        )
                    })?;
                }
                WorkflowConnectorResumeResolution::Indeterminate { .. } => {
                    return Ok(ConnectorStepResolution::Failed(format!(
                        "Workflow Connector step {:?} became indeterminate at attempt {step_attempt}; provider retry is forbidden",
                        step.plan.id
                    )));
                }
                WorkflowConnectorResumeResolution::Rejected { reason } => {
                    return Ok(ConnectorStepResolution::Failed(reason));
                }
            }
        }
    }
    Err(ConnectorStepError::Invalid(
        "Workflow Connector retry state became unreachable".into(),
    ))
}

pub(super) fn attempt_request(
    metadata: &WorkflowConnectorHookMetadata,
) -> WorkflowConnectorAttemptRequest {
    WorkflowConnectorAttemptRequest {
        organization_id: metadata.organization_id,
        project_id: metadata.project_id,
        environment_id: metadata.environment_id,
        workflow_run_id: metadata.workflow_run_id,
        plan_revision_id: metadata.plan_revision_id,
        plan_digest: metadata.plan_digest.clone(),
        step_id: metadata.step_id.clone(),
        step_attempt: metadata.step_attempt,
        connector_profile_id: metadata.connector_profile_id,
        connector_revision_id: metadata.connector_revision_id,
        connector_revision_digest: metadata.connector_revision_digest.clone(),
        capability: metadata.capability.clone(),
        input: metadata.effective_input.clone(),
        response_mode: if metadata.requires_response_object() {
            WorkflowConnectorResponseMode::ImmutableObjectReference
        } else {
            WorkflowConnectorResponseMode::DigestOnly
        },
    }
}

pub(super) fn attempt_authority(
    metadata: &WorkflowConnectorHookMetadata,
) -> Result<WorkflowConnectorAttemptAuthority, String> {
    metadata.validate()?;
    attempt_request(metadata).connector_attempt_authority()
}

pub(super) struct ObservedConnectorHook<'a> {
    pub hook: &'a HookSnapshot,
    pub metadata: WorkflowConnectorHookMetadata,
}

pub(super) enum ConnectorProjectionResolution {
    Running,
    Completed(Box<WorkflowLocalStepResult>),
    Failed(String),
}

pub(super) fn project_received_hook(
    step: &ResolvedWorkflowRunStep,
    observed: &ObservedConnectorHook<'_>,
) -> Result<ConnectorProjectionResolution, String> {
    let authority = attempt_authority(&observed.metadata)?;
    let payload = received_payload(observed, &authority)?
        .ok_or_else(|| "received Workflow Connector hook has no payload".to_owned())?;
    match payload.resolution {
        WorkflowConnectorResumeResolution::Completed { evidence } => match evidence.outcome {
            WorkflowConnectorAttemptOutcome::Accepted
                if observed.metadata.requires_typed_response() => {
                    validate_accepted_evidence(
                        &observed.metadata,
                        &evidence,
                        &authority,
                    )?;
                    Ok(ConnectorProjectionResolution::Running)
                }
            WorkflowConnectorAttemptOutcome::Accepted => accepted_result(
                step,
                &observed.metadata,
                &evidence,
                &authority,
            )
            .map(ConnectorProjectionResolution::Completed),
            WorkflowConnectorAttemptOutcome::Rejected => {
                Ok(ConnectorProjectionResolution::Failed(format!(
                    "Workflow Connector step {:?} was rejected at attempt {}{}",
                    step.plan.id,
                    observed.metadata.step_attempt,
                    status_suffix(evidence.response_status)
                )))
            }
            WorkflowConnectorAttemptOutcome::Retryable
                if observed.metadata.step_attempt
                    == observed.metadata.retry_policy.maximum_attempts =>
            {
                Ok(ConnectorProjectionResolution::Failed(format!(
                    "Workflow Connector step {:?} exhausted {} attempts{}",
                    step.plan.id,
                    observed.metadata.retry_policy.maximum_attempts,
                    status_suffix(evidence.response_status)
                )))
            }
            WorkflowConnectorAttemptOutcome::Retryable => {
                Ok(ConnectorProjectionResolution::Running)
            }
        },
        WorkflowConnectorResumeResolution::Deferred { .. } => {
            Ok(ConnectorProjectionResolution::Running)
        }
        WorkflowConnectorResumeResolution::Indeterminate { .. } => {
            Ok(ConnectorProjectionResolution::Failed(format!(
                "Workflow Connector step {:?} became indeterminate at attempt {}; provider retry is forbidden",
                step.plan.id, observed.metadata.step_attempt
            )))
        }
        WorkflowConnectorResumeResolution::Rejected { reason } => {
            Ok(ConnectorProjectionResolution::Failed(reason))
        }
    }
}

pub(super) fn accepted_response_step_input(
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    observed: &ObservedConnectorHook<'_>,
) -> Result<Option<super::connector_response::WorkflowConnectorResponseStepInput>, String> {
    if !observed.metadata.requires_typed_response() || observed.hook.status != HookStatus::Received
    {
        return Ok(None);
    }
    let authority = attempt_authority(&observed.metadata)?;
    let Some(payload) = received_payload(observed, &authority)? else {
        return Ok(None);
    };
    let WorkflowConnectorResumeResolution::Completed { evidence } = payload.resolution else {
        return Ok(None);
    };
    if evidence.outcome != WorkflowConnectorAttemptOutcome::Accepted {
        return Ok(None);
    }
    super::connector_response::WorkflowConnectorResponseStepInput::new(
        &input.runtime_contract_revision,
        step,
        &observed.metadata,
        &evidence,
    )
    .map(Some)
}

pub(super) fn observed_connector_hooks<'a>(
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    snapshot: &'a WorkflowRunSnapshot,
) -> Result<Vec<ObservedConnectorHook<'a>>, String> {
    let prefix = format!("workflow-connector:{}:", step.plan.id);
    let mut observed = Vec::new();
    for hook in snapshot
        .hooks
        .values()
        .filter(|hook| hook.hook_id.starts_with(&prefix))
    {
        let metadata =
            serde_json::from_value::<WorkflowConnectorHookMetadata>(hook.metadata.clone())
                .map_err(|error| format!("Workflow Connector hook metadata is invalid: {error}"))?;
        metadata.validate()?;
        let expected = WorkflowConnectorHookMetadata::from_run_step(
            input,
            step,
            metadata.effective_input.clone(),
            metadata.step_attempt,
            metadata.observation,
        )?;
        if hook.hook_id != expected.flow_hook_id()
            || hook.token != expected.flow_hook_token()
            || metadata != expected
        {
            return Err("Workflow Connector hook authority drifted".into());
        }
        observed.push(ObservedConnectorHook { hook, metadata });
    }
    observed.sort_by_key(|item| (item.metadata.step_attempt, item.metadata.observation));
    validate_hook_sequence(snapshot, &observed)?;
    Ok(observed)
}

pub(super) fn verify_hook_history(
    observed: &[ObservedConnectorHook<'_>],
    history: &[a3s_flow::FlowEventEnvelope],
) -> Result<(), String> {
    for item in observed {
        let expected_metadata = serde_json::to_value(&item.metadata)
            .map_err(|error| format!("could not encode Workflow Connector metadata: {error}"))?;
        let created = history
            .iter()
            .filter(|event| {
                matches!(
                    &event.event,
                    a3s_flow::FlowEvent::HookCreated { hook_id, .. }
                        if hook_id == &item.hook.hook_id
                )
            })
            .collect::<Vec<_>>();
        if created.len() != 1 {
            return Err(format!(
                "Workflow Connector hook {:?} must have exactly one creation event",
                item.hook.hook_id
            ));
        }
        let a3s_flow::FlowEvent::HookCreated {
            token, metadata, ..
        } = &created[0].event
        else {
            return Err("Workflow Connector hook creation history is invalid".into());
        };
        if token != &item.metadata.flow_hook_token() || metadata != &expected_metadata {
            return Err("Workflow Connector hook creation authority drifted".into());
        }
    }
    Ok(())
}

pub(super) fn verify_wait_authority(
    input: &WorkflowRunInput,
    snapshot: &WorkflowRunSnapshot,
    observed: &[ObservedConnectorHook<'_>],
) -> Result<(), String> {
    let mut allowed = std::collections::BTreeMap::new();
    for item in observed {
        let authority = attempt_authority(&item.metadata)?;
        let Some(payload) = received_payload(item, &authority)? else {
            continue;
        };
        match payload.resolution {
            WorkflowConnectorResumeResolution::Deferred {
                retry_not_before, ..
            } => {
                allowed.insert(
                    item.metadata.observation_wait_id(),
                    bounded_resume_at(retry_not_before, input.deadline_at),
                );
            }
            WorkflowConnectorResumeResolution::Completed { evidence }
                if evidence.outcome == WorkflowConnectorAttemptOutcome::Retryable
                    && item.metadata.step_attempt < item.metadata.retry_policy.maximum_attempts =>
            {
                allowed.insert(
                    item.metadata.retry_wait_id(),
                    retry_resume_at(&evidence, item.metadata.retry_policy, input.deadline_at)?,
                );
            }
            _ => {}
        }
    }
    for (wait_id, wait) in snapshot
        .waits
        .iter()
        .filter(|(wait_id, _)| wait_id.starts_with("workflow-connector-"))
    {
        let expected_resume_at = allowed.get(wait_id).ok_or_else(|| {
            "WorkflowRun correlated Flow contains an unexpected Connector wait".to_owned()
        })?;
        if wait.wait_id != wait_id.as_str() || &wait.resume_at != expected_resume_at {
            return Err("Workflow Connector wait authority drifted".into());
        }
    }
    Ok(())
}

fn validate_hook_sequence(
    snapshot: &WorkflowRunSnapshot,
    observed: &[ObservedConnectorHook<'_>],
) -> Result<(), String> {
    let mut previous: Option<&ObservedConnectorHook<'_>> = None;
    for current in observed {
        if let Some(previous) = previous {
            let authority = attempt_authority(&previous.metadata)?;
            let payload = received_payload(previous, &authority)?;
            let valid_successor = if current.metadata.step_attempt == previous.metadata.step_attempt
                && current.metadata.observation == previous.metadata.observation + 1
            {
                matches!(
                    payload.as_ref().map(|payload| &payload.resolution),
                    Some(WorkflowConnectorResumeResolution::Deferred { .. })
                ) && wait_completed(snapshot, &previous.metadata.observation_wait_id())
            } else if current.metadata.step_attempt == previous.metadata.step_attempt + 1
                && current.metadata.observation == 1
            {
                matches!(
                    payload.as_ref().map(|payload| &payload.resolution),
                    Some(WorkflowConnectorResumeResolution::Completed { evidence })
                        if evidence.outcome == WorkflowConnectorAttemptOutcome::Retryable
                ) && wait_completed(snapshot, &previous.metadata.retry_wait_id())
            } else {
                false
            };
            if !valid_successor {
                return Err("Workflow Connector hook sequence is not contiguous".into());
            }
        } else if current.metadata.step_attempt != 1 || current.metadata.observation != 1 {
            return Err("Workflow Connector hook sequence does not start at attempt one".into());
        }
        previous = Some(current);
    }
    Ok(())
}

fn received_payload(
    observed: &ObservedConnectorHook<'_>,
    authority: &WorkflowConnectorAttemptAuthority,
) -> Result<Option<WorkflowConnectorResumePayload>, String> {
    let Some(payload) = observed.hook.payload.as_ref() else {
        return Ok(None);
    };
    let payload = serde_json::from_value::<WorkflowConnectorResumePayload>(payload.clone())
        .map_err(|error| format!("Workflow Connector resume payload is invalid: {error}"))?;
    payload.validate(
        &observed.metadata,
        authority.attempt_id,
        &authority.request_digest,
        authority.request_body_bytes,
    )?;
    validate_connector_evidence_bounds(&payload)?;
    Ok(Some(payload))
}

fn wait_completed(snapshot: &WorkflowRunSnapshot, wait_id: &str) -> bool {
    snapshot
        .waits
        .get(wait_id)
        .is_some_and(|wait| wait.status == a3s_flow::WaitStatus::Completed)
}

fn decode_payload(
    run_id: &str,
    metadata: &WorkflowConnectorHookMetadata,
    observed: &serde_json::Value,
    authority: &WorkflowConnectorAttemptAuthority,
) -> Result<WorkflowConnectorResumePayload, ConnectorStepError> {
    let drift = || ConnectorStepError::NonDeterministic(connector_payload_drift(metadata));
    let payload = serde_json::from_value::<WorkflowConnectorResumePayload>(observed.clone())
        .map_err(|_| drift())?;
    payload
        .validate(
            metadata,
            authority.attempt_id,
            &authority.request_digest,
            authority.request_body_bytes,
        )
        .map_err(|_| drift())?;
    validate_connector_evidence_bounds(&payload).map_err(|_| drift())?;
    if payload.flow_run_id != run_id || payload.flow_hook_id != metadata.flow_hook_id() {
        return Err(drift());
    }
    Ok(payload)
}

fn accepted_result(
    step: &ResolvedWorkflowRunStep,
    metadata: &WorkflowConnectorHookMetadata,
    evidence: &crate::modules::workflow::domain::WorkflowConnectorAttemptEvidence,
    authority: &WorkflowConnectorAttemptAuthority,
) -> Result<Box<WorkflowLocalStepResult>, String> {
    let output = validate_accepted_evidence(metadata, evidence, authority)?;
    let output = serde_json::to_value(output)
        .map_err(|error| format!("could not encode Workflow Connector output: {error}"))?;
    let result = WorkflowLocalStepResult {
        step_id: step.plan.id.clone(),
        kind: WorkflowStepKind::Service,
        output_digest: super::execution::value_digest(&output, "Workflow Connector step output")?,
        output,
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    result.validate(step)?;
    Ok(Box::new(result))
}

fn accepted_resolution(
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    metadata: &WorkflowConnectorHookMetadata,
    evidence: &crate::modules::workflow::domain::WorkflowConnectorAttemptEvidence,
    authority: &WorkflowConnectorAttemptAuthority,
) -> Result<ConnectorStepResolution, String> {
    validate_accepted_evidence(metadata, evidence, authority)?;
    if metadata.requires_typed_response() {
        return super::connector_response::WorkflowConnectorResponseStepInput::new(
            &input.runtime_contract_revision,
            step,
            metadata,
            evidence,
        )
        .map(Box::new)
        .map(ConnectorStepResolution::Consume);
    }
    accepted_result(step, metadata, evidence, authority).map(ConnectorStepResolution::Complete)
}

fn validate_accepted_evidence(
    metadata: &WorkflowConnectorHookMetadata,
    evidence: &crate::modules::workflow::domain::WorkflowConnectorAttemptEvidence,
    authority: &WorkflowConnectorAttemptAuthority,
) -> Result<WorkflowConnectorStepOutput, String> {
    WorkflowConnectorStepOutput::from_evidence(
        metadata,
        evidence,
        authority.attempt_id,
        &authority.request_digest,
        authority.request_body_bytes,
    )
}

fn bounded_resume_at(value: DateTime<Utc>, deadline_at: DateTime<Utc>) -> DateTime<Utc> {
    value.min(deadline_at)
}

fn retry_resume_at(
    evidence: &crate::modules::workflow::domain::WorkflowConnectorAttemptEvidence,
    policy: WorkflowRetryPolicy,
    deadline_at: DateTime<Utc>,
) -> Result<DateTime<Utc>, String> {
    let delay_seconds = evidence
        .retry_after_seconds
        .unwrap_or(policy.default_delay_seconds);
    let delay = i64::try_from(delay_seconds)
        .map_err(|_| "Workflow Connector retry delay exceeds runtime bounds".to_owned())?;
    let retry_at = evidence
        .completed_at
        .checked_add_signed(Duration::seconds(delay))
        .ok_or_else(|| "Workflow Connector retry time overflowed".to_owned())?;
    Ok(bounded_resume_at(retry_at, deadline_at))
}

fn validate_connector_evidence_bounds(
    payload: &WorkflowConnectorResumePayload,
) -> Result<(), String> {
    let WorkflowConnectorResumeResolution::Completed { evidence } = &payload.resolution else {
        return Ok(());
    };
    if evidence.request_body_bytes > MAXIMUM_CONNECTOR_BODY_BYTES as u64
        || evidence
            .response_body_bytes
            .is_some_and(|bytes| bytes > MAXIMUM_CONNECTOR_BODY_BYTES as u64)
    {
        return Err("Workflow Connector evidence exceeds the C6 body bound".into());
    }
    Ok(())
}

fn status_suffix(status: Option<u16>) -> String {
    status
        .map(|status| format!(" (HTTP status {status})"))
        .unwrap_or_default()
}

fn connector_payload_drift(metadata: &WorkflowConnectorHookMetadata) -> String {
    format!(
        "Workflow Connector step {:?}, attempt {}, observation {} received an invalid authority-bound payload",
        metadata.step_id, metadata.step_attempt, metadata.observation
    )
}

pub(super) fn flow_error(run_id: &str, error: ConnectorStepError) -> FlowError {
    match error {
        ConnectorStepError::Invalid(error) => FlowError::InvalidWorkflow(error),
        ConnectorStepError::NonDeterministic(reason) => FlowError::NonDeterministic {
            run_id: run_id.into(),
            reason,
        },
    }
}
