use super::connector::{
    attempt_authority, observed_connector_hooks, resolve_step, verify_wait_authority,
    ConnectorStepError,
};
use super::WorkflowRunFlowRuntime;
use crate::modules::connectors::domain::{
    ConnectorResponseObjectReference, MAXIMUM_CONNECTOR_BODY_BYTES,
};
use crate::modules::connectors::{
    ConnectorResponseObjectContent, IConnectorResponseObjectPort, ReadConnectorResponseObject,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{canonical_timestamp, Sha256Digest};
use crate::modules::workflow::domain::{
    flow_step_id, WorkflowConnectorAttemptEvidence, WorkflowConnectorAttemptOutcome,
    WorkflowConnectorHookMetadata, WorkflowConnectorResponseObjectReference,
    WorkflowConnectorResumePayload, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION_V8,
};
use crate::modules::workflow::test_support::{
    connector_workflow_run_input, connector_workflow_run_input_v5, connector_workflow_run_input_v6,
    routed_connector_workflow_run_input,
};
use a3s_flow::{
    FlowEngine, FlowError, HookStatus, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use chrono::{DateTime, Duration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

struct FakeResponseObjects {
    body: Vec<u8>,
    reads: AtomicUsize,
    requests: Mutex<Vec<ConnectorResponseObjectReference>>,
}

impl FakeResponseObjects {
    fn new(body: impl Into<Vec<u8>>) -> Self {
        Self {
            body: body.into(),
            reads: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl IConnectorResponseObjectPort for FakeResponseObjects {
    async fn read_response_object(
        &self,
        request: &ReadConnectorResponseObject,
    ) -> ApplicationResult<ConnectorResponseObjectContent> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().await.push(request.reference.clone());
        ConnectorResponseObjectContent::for_test(request.reference.clone(), self.body.clone())
    }
}

#[tokio::test]
async fn connector_runtime_consumes_an_authorized_immutable_json_response() -> Result<(), FlowError>
{
    let (engine, input, responses) = start_with_body(br#"{"accepted":true}"#.to_vec()).await?;
    let metadata = active_hook(&engine, &input.workflow_run_id.to_string(), 1, 1).await?;
    let authority = attempt_authority(&metadata).map_err(FlowError::Runtime)?;
    let evidence = accepted_evidence(
        &metadata,
        authority.attempt_id,
        authority.request_digest.clone(),
        authority.request_body_bytes,
        input.requested_at + Duration::seconds(1),
    )?;
    let payload = WorkflowConnectorResumePayload::completed(
        &metadata,
        evidence,
        authority.attempt_id,
        &authority.request_digest,
        authority.request_body_bytes,
    )
    .map_err(FlowError::Runtime)?;
    engine
        .resume_hook(
            &input.workflow_run_id.to_string(),
            &metadata.flow_hook_id(),
            serde_json::to_value(payload)?,
        )
        .await?;

    let snapshot = engine.snapshot(&input.workflow_run_id.to_string()).await?;
    assert_eq!(
        snapshot.status,
        WorkflowRunStatus::Completed,
        "{snapshot:#?}"
    );
    assert_eq!(snapshot.output, Some(serde_json::json!({"accepted": true})));
    assert!(snapshot.steps.contains_key(&flow_step_id("invoke")));
    assert_eq!(responses.reads.load(Ordering::SeqCst), 1);
    let requests = responses.requests.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].organization_id, input.organization_id);
    assert_eq!(requests[0].project_id, input.project_id);
    assert_eq!(requests[0].connector_attempt_id, authority.attempt_id);
    assert_eq!(
        requests[0].digest,
        Sha256Digest::from_bytes(br#"{"accepted":true}"#)
    );
    Ok(())
}

#[tokio::test]
async fn connector_v6_replay_keeps_reference_only_output_without_reading_the_object(
) -> Result<(), FlowError> {
    let input = connector_workflow_run_input_v6().map_err(FlowError::Runtime)?;
    let (engine, input, responses) =
        start_input_with_body(input, br#"{"accepted":true}"#.to_vec()).await?;
    let run_id = input.workflow_run_id.to_string();
    let metadata = active_hook(&engine, &run_id, 1, 1).await?;
    resume_accepted(
        &engine,
        &run_id,
        &metadata,
        input.requested_at + Duration::seconds(1),
    )
    .await?;

    let snapshot = engine.snapshot(&run_id).await?;
    let output = snapshot.output.expect("v6 Connector output");
    assert_eq!(output["schema"], "cloud.workflow.connector-result.v2");
    assert_eq!(responses.reads.load(Ordering::SeqCst), 0);
    assert!(!snapshot.steps.contains_key(&flow_step_id("invoke")));
    Ok(())
}

#[tokio::test]
async fn invalid_connector_json_fails_closed_without_provider_retry_or_body_disclosure(
) -> Result<(), FlowError> {
    let body = b"secret-provider-text".to_vec();
    let (engine, input, responses) = start_with_body(body.clone()).await?;
    let run_id = input.workflow_run_id.to_string();
    let metadata = active_hook(&engine, &run_id, 1, 1).await?;
    resume_accepted_body(
        &engine,
        &run_id,
        &metadata,
        input.requested_at + Duration::seconds(1),
        &body,
    )
    .await?;

    let snapshot = engine.snapshot(&run_id).await?;
    assert_eq!(snapshot.status, WorkflowRunStatus::Failed);
    let error = snapshot.error.as_deref().expect("typed response failure");
    assert!(error.contains("not one unambiguous JSON value"), "{error}");
    assert!(!error.contains("secret-provider-text"), "{error}");
    assert_eq!(responses.reads.load(Ordering::SeqCst), 1);
    assert!(!snapshot.hooks.contains_key("workflow-connector:invoke:2:1"));
    Ok(())
}

#[tokio::test]
async fn invalid_connector_json_uses_the_descriptor_bound_failure_edge() -> Result<(), FlowError> {
    let body = b"secret-provider-text".to_vec();
    let input = routed_connector_workflow_run_input().map_err(FlowError::Runtime)?;
    let (engine, input, responses) = start_input_with_body(input, body.clone()).await?;
    let run_id = input.workflow_run_id.to_string();
    let metadata = active_hook(&engine, &run_id, 1, 1).await?;
    resume_accepted_body(
        &engine,
        &run_id,
        &metadata,
        input.requested_at + Duration::seconds(1),
        &body,
    )
    .await?;

    let snapshot = engine.snapshot(&run_id).await?;
    assert_eq!(
        snapshot.status,
        WorkflowRunStatus::Completed,
        "{snapshot:#?}"
    );
    let step = input
        .resolved_steps()
        .map_err(FlowError::Runtime)?
        .into_iter()
        .find(|step| step.plan.id == "invoke")
        .expect("Connector step");
    let observed =
        observed_connector_hooks(&input, &step, &snapshot).map_err(FlowError::Runtime)?;
    let history = engine.history(&run_id).await?;
    super::connector_response::verify_step_history(&input, &step, &observed, &snapshot, &history)
        .map_err(FlowError::Runtime)?;
    let aggregate = snapshot.output.expect("routed Connector aggregate output");
    let output = &aggregate["failure_output"];
    assert_eq!(output["schema"], "cloud.workflow.step-failure.v2");
    assert_eq!(output["stepId"], "invoke");
    assert_eq!(output["classification"], "provider_response_invalid");
    assert!(!output.to_string().contains("secret-provider-text"));
    assert_eq!(responses.reads.load(Ordering::SeqCst), 1);
    assert!(!snapshot.hooks.contains_key("workflow-connector:invoke:2:1"));
    Ok(())
}

#[tokio::test]
async fn v8_connector_response_fails_closed_when_the_c11_port_is_unconfigured(
) -> Result<(), FlowError> {
    let mut input = connector_workflow_run_input().map_err(FlowError::Runtime)?;
    input.requested_at = canonical_timestamp(Utc::now());
    input.deadline_at = input.requested_at + Duration::hours(1);
    input.validate().map_err(FlowError::Runtime)?;
    let run_id = input.workflow_run_id.to_string();
    let engine = FlowEngine::in_memory(Arc::new(WorkflowRunFlowRuntime::default()));
    engine
        .start_with_id(
            run_id.clone(),
            WorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                input.flow_workflow_version.clone(),
                "a3s-cloud",
                "main",
            ),
            serde_json::to_value(&input)?,
        )
        .await?;
    let metadata = active_hook(&engine, &run_id, 1, 1).await?;
    resume_accepted(
        &engine,
        &run_id,
        &metadata,
        input.requested_at + Duration::seconds(1),
    )
    .await?;

    let snapshot = engine.snapshot(&run_id).await?;
    assert_eq!(snapshot.status, WorkflowRunStatus::Failed);
    assert!(snapshot
        .error
        .as_deref()
        .is_some_and(|error| error.contains("response consumption is not configured")));
    assert!(!snapshot.hooks.contains_key("workflow-connector:invoke:2:1"));
    Ok(())
}

#[tokio::test]
async fn connector_v5_replay_remains_body_free_and_byte_compatible() -> Result<(), FlowError> {
    let mut input = connector_workflow_run_input_v5().map_err(FlowError::Runtime)?;
    input.requested_at = canonical_timestamp(Utc::now());
    input.deadline_at = input.requested_at + Duration::hours(1);
    let (engine, input) = start_input(input).await?;
    let run_id = input.workflow_run_id.to_string();
    let metadata = active_hook(&engine, &run_id, 1, 1).await?;
    assert!(!metadata.requires_response_object());
    let authority = attempt_authority(&metadata).map_err(FlowError::Runtime)?;
    let evidence = accepted_evidence(
        &metadata,
        authority.attempt_id,
        authority.request_digest.clone(),
        authority.request_body_bytes,
        input.requested_at + Duration::seconds(1),
    )?;
    let payload = WorkflowConnectorResumePayload::completed(
        &metadata,
        evidence,
        authority.attempt_id,
        &authority.request_digest,
        authority.request_body_bytes,
    )
    .map_err(FlowError::Runtime)?;
    engine
        .resume_hook(
            &run_id,
            &metadata.flow_hook_id(),
            serde_json::to_value(payload)?,
        )
        .await?;

    let output = engine.snapshot(&run_id).await?.output.expect("v5 output");
    assert_eq!(output["schema"], "cloud.workflow.connector-result.v1");
    assert!(output.get("responseObject").is_none());
    Ok(())
}

#[tokio::test]
async fn connector_runtime_owns_retry_wait_and_next_attempt_identity() -> Result<(), FlowError> {
    let (engine, input) = start().await?;
    let run_id = input.workflow_run_id.to_string();
    let first = active_hook(&engine, &run_id, 1, 1).await?;
    let first_authority = attempt_authority(&first).map_err(FlowError::Runtime)?;
    let completed_at = input.requested_at + Duration::seconds(1);
    let evidence = WorkflowConnectorAttemptEvidence::restore_with_response_object(
        first_authority.attempt_id,
        first_authority.request_digest.clone(),
        first_authority.request_body_bytes,
        WorkflowConnectorAttemptOutcome::Retryable,
        Some(503),
        None,
        None,
        None,
        Some(7),
        completed_at - Duration::seconds(1),
        completed_at,
    )
    .map_err(FlowError::Runtime)?;
    let payload = WorkflowConnectorResumePayload::completed(
        &first,
        evidence,
        first_authority.attempt_id,
        &first_authority.request_digest,
        first_authority.request_body_bytes,
    )
    .map_err(FlowError::Runtime)?;
    engine
        .resume_hook(
            &run_id,
            &first.flow_hook_id(),
            serde_json::to_value(payload)?,
        )
        .await?;
    let waiting = engine.snapshot(&run_id).await?;
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(
        waiting.waits[&first.retry_wait_id()].resume_at,
        completed_at + Duration::seconds(7)
    );

    engine
        .resume_due_waits(completed_at + Duration::seconds(7))
        .await?;
    let second = active_hook(&engine, &run_id, 2, 1).await?;
    let second_authority = attempt_authority(&second).map_err(FlowError::Runtime)?;
    assert_ne!(second_authority.attempt_id, first_authority.attempt_id);
    resume_accepted(
        &engine,
        &run_id,
        &second,
        completed_at + Duration::seconds(8),
    )
    .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Completed
    );
    Ok(())
}

#[tokio::test]
async fn deferred_connector_observation_reuses_the_provider_attempt() -> Result<(), FlowError> {
    let (engine, input) = start().await?;
    let run_id = input.workflow_run_id.to_string();
    let first = active_hook(&engine, &run_id, 1, 1).await?;
    let first_authority = attempt_authority(&first).map_err(FlowError::Runtime)?;
    let retry_not_before = input.requested_at + Duration::seconds(2);
    let payload = WorkflowConnectorResumePayload::deferred(
        &first,
        first_authority.attempt_id,
        retry_not_before,
        &first_authority.request_digest,
        first_authority.request_body_bytes,
    )
    .map_err(FlowError::Runtime)?;
    engine
        .resume_hook(
            &run_id,
            &first.flow_hook_id(),
            serde_json::to_value(payload)?,
        )
        .await?;
    engine.resume_due_waits(retry_not_before).await?;

    let second = active_hook(&engine, &run_id, 1, 2).await?;
    let second_authority = attempt_authority(&second).map_err(FlowError::Runtime)?;
    assert_eq!(second_authority, first_authority);
    resume_accepted(
        &engine,
        &run_id,
        &second,
        retry_not_before + Duration::seconds(1),
    )
    .await?;
    assert_eq!(
        engine.snapshot(&run_id).await?.status,
        WorkflowRunStatus::Completed
    );
    Ok(())
}

#[tokio::test]
async fn indeterminate_connector_attempt_fails_closed_without_blind_retry() -> Result<(), FlowError>
{
    let (engine, input) = start().await?;
    let run_id = input.workflow_run_id.to_string();
    let metadata = active_hook(&engine, &run_id, 1, 1).await?;
    let authority = attempt_authority(&metadata).map_err(FlowError::Runtime)?;
    let payload = WorkflowConnectorResumePayload::indeterminate(
        &metadata,
        authority.attempt_id,
        input.requested_at + Duration::seconds(1),
        input.requested_at + Duration::seconds(10),
        &authority.request_digest,
        authority.request_body_bytes,
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
    assert_eq!(snapshot.status, WorkflowRunStatus::Failed);
    assert!(snapshot
        .error
        .as_deref()
        .is_some_and(|error| error.contains("provider retry is forbidden")));
    assert!(!snapshot.hooks.contains_key("workflow-connector:invoke:2:1"));
    Ok(())
}

#[tokio::test]
async fn indeterminate_connector_attempt_uses_the_descriptor_bound_failure_edge(
) -> Result<(), FlowError> {
    let input = routed_connector_workflow_run_input().map_err(FlowError::Runtime)?;
    let (engine, input) = start_input(input).await?;
    let run_id = input.workflow_run_id.to_string();
    let metadata = active_hook(&engine, &run_id, 1, 1).await?;
    let authority = attempt_authority(&metadata).map_err(FlowError::Runtime)?;
    let payload = WorkflowConnectorResumePayload::indeterminate(
        &metadata,
        authority.attempt_id,
        input.requested_at + Duration::seconds(1),
        input.requested_at + Duration::seconds(10),
        &authority.request_digest,
        authority.request_body_bytes,
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
    let aggregate = snapshot.output.expect("routed Connector aggregate output");
    let output = &aggregate["failure_output"];
    assert_eq!(output["schema"], "cloud.workflow.step-failure.v2");
    assert_eq!(output["stepId"], "invoke");
    assert_eq!(output["classification"], "provider_indeterminate");
    assert!(output["message"]
        .as_str()
        .is_some_and(|message| message.contains("provider retry is forbidden")));
    assert!(!snapshot.hooks.contains_key("workflow-connector:invoke:2:1"));
    Ok(())
}

#[tokio::test]
async fn connector_retry_budget_exhaustion_is_terminal_and_bounded() -> Result<(), FlowError> {
    let (engine, input) = start().await?;
    let run_id = input.workflow_run_id.to_string();
    for attempt in 1..=3_u32 {
        let metadata = active_hook(&engine, &run_id, attempt, 1).await?;
        let completed_at = input.requested_at + Duration::seconds(i64::from(attempt) * 10);
        resume_retryable(&engine, &run_id, &metadata, completed_at).await?;
        if attempt < 3 {
            engine
                .resume_due_waits(completed_at + Duration::seconds(5))
                .await?;
        }
    }

    let snapshot = engine.snapshot(&run_id).await?;
    assert_eq!(snapshot.status, WorkflowRunStatus::Failed);
    assert!(snapshot
        .error
        .as_deref()
        .is_some_and(|error| error.contains("exhausted 3 attempts")));
    assert!(!snapshot.hooks.contains_key("workflow-connector:invoke:4:1"));
    Ok(())
}

#[test]
fn connector_runtime_rejects_an_oversized_c6_request_before_creating_a_hook() {
    let input = connector_workflow_run_input().expect("Connector WorkflowRun input");
    let step = input
        .resolved_steps()
        .expect("resolved Connector steps")
        .into_iter()
        .find(|step| step.plan.id == "invoke")
        .expect("Connector step");
    let invocation = WorkflowInvocation::new(
        input.workflow_run_id.to_string(),
        WorkflowSpec::rust_embedded(
            WORKFLOW_RUN_FLOW_NAME,
            WORKFLOW_RUN_FLOW_VERSION_V8,
            "a3s-cloud",
            "main",
        ),
        serde_json::to_value(&input).expect("encoded WorkflowRun input"),
        Vec::new(),
    );
    let context = invocation.context();
    let result = resolve_step(
        &invocation.run_id,
        &input,
        &step,
        serde_json::Value::String("x".repeat(MAXIMUM_CONNECTOR_BODY_BYTES + 1)),
        &context,
    );
    assert!(matches!(result, Err(ConnectorStepError::Invalid(_))));
}

#[tokio::test]
async fn connector_runtime_rejects_response_evidence_beyond_the_c6_bound() -> Result<(), FlowError>
{
    let (engine, input) = start().await?;
    let run_id = input.workflow_run_id.to_string();
    let metadata = active_hook(&engine, &run_id, 1, 1).await?;
    let authority = attempt_authority(&metadata).map_err(FlowError::Runtime)?;
    let completed_at = input.requested_at + Duration::seconds(1);
    let response_digest = Sha256Digest::from_bytes(b"bounded-body");
    let response_object = response_object(
        authority.attempt_id,
        response_digest.clone(),
        MAXIMUM_CONNECTOR_BODY_BYTES as u64 + 1,
    )?;
    let evidence = WorkflowConnectorAttemptEvidence::restore_with_response_object(
        authority.attempt_id,
        authority.request_digest.clone(),
        authority.request_body_bytes,
        WorkflowConnectorAttemptOutcome::Accepted,
        Some(200),
        Some(response_digest),
        Some(MAXIMUM_CONNECTOR_BODY_BYTES as u64 + 1),
        Some(response_object),
        None,
        completed_at - Duration::seconds(1),
        completed_at,
    )
    .map_err(FlowError::Runtime)?;
    let payload = WorkflowConnectorResumePayload::completed(
        &metadata,
        evidence,
        authority.attempt_id,
        &authority.request_digest,
        authority.request_body_bytes,
    )
    .map_err(FlowError::Runtime)?;
    let error = engine
        .resume_hook(
            &run_id,
            &metadata.flow_hook_id(),
            serde_json::to_value(payload)?,
        )
        .await
        .expect_err("oversized Connector evidence must fail closed");
    assert!(matches!(error, FlowError::NonDeterministic { .. }));
    Ok(())
}

#[tokio::test]
async fn connector_projection_rejects_retry_wait_deadline_drift() -> Result<(), FlowError> {
    let (engine, input) = start().await?;
    let run_id = input.workflow_run_id.to_string();
    let metadata = active_hook(&engine, &run_id, 1, 1).await?;
    let completed_at = input.requested_at + Duration::seconds(1);
    resume_retryable(&engine, &run_id, &metadata, completed_at).await?;

    let mut snapshot = engine.snapshot(&run_id).await?;
    snapshot
        .waits
        .get_mut(&metadata.retry_wait_id())
        .expect("Connector retry wait")
        .resume_at += Duration::seconds(1);
    let step = input
        .resolved_steps()
        .map_err(FlowError::Runtime)?
        .into_iter()
        .find(|step| step.plan.id == metadata.step_id)
        .ok_or_else(|| FlowError::Runtime("missing Connector step".into()))?;
    let observed =
        observed_connector_hooks(&input, &step, &snapshot).map_err(FlowError::Runtime)?;
    assert!(verify_wait_authority(&input, &snapshot, &observed).is_err());
    Ok(())
}

async fn start() -> Result<
    (
        FlowEngine,
        crate::modules::workflow::domain::WorkflowRunInput,
    ),
    FlowError,
> {
    let (engine, input, _) = start_with_body(br#"{"accepted":true}"#.to_vec()).await?;
    Ok((engine, input))
}

async fn start_with_body(
    body: Vec<u8>,
) -> Result<
    (
        FlowEngine,
        crate::modules::workflow::domain::WorkflowRunInput,
        Arc<FakeResponseObjects>,
    ),
    FlowError,
> {
    let input = connector_workflow_run_input().map_err(FlowError::Runtime)?;
    start_input_with_body(input, body).await
}

async fn start_input_with_body(
    mut input: crate::modules::workflow::domain::WorkflowRunInput,
    body: Vec<u8>,
) -> Result<
    (
        FlowEngine,
        crate::modules::workflow::domain::WorkflowRunInput,
        Arc<FakeResponseObjects>,
    ),
    FlowError,
> {
    input.requested_at = canonical_timestamp(Utc::now());
    input.deadline_at = input.requested_at + Duration::hours(1);
    input.validate().map_err(FlowError::Runtime)?;
    let responses = Arc::new(FakeResponseObjects::new(body));
    let runtime = WorkflowRunFlowRuntime::with_connector_responses(
        responses.clone() as Arc<dyn IConnectorResponseObjectPort>
    );
    let engine = FlowEngine::in_memory(Arc::new(runtime));
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            WorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                input.flow_workflow_version.clone(),
                "a3s-cloud",
                "main",
            ),
            serde_json::to_value(&input)?,
        )
        .await?;
    Ok((engine, input, responses))
}

async fn start_input(
    input: crate::modules::workflow::domain::WorkflowRunInput,
) -> Result<
    (
        FlowEngine,
        crate::modules::workflow::domain::WorkflowRunInput,
    ),
    FlowError,
> {
    let (engine, input, _) = start_input_with_body(input, br#"{"accepted":true}"#.to_vec()).await?;
    Ok((engine, input))
}

async fn active_hook(
    engine: &FlowEngine,
    run_id: &str,
    attempt: u32,
    observation: u32,
) -> Result<WorkflowConnectorHookMetadata, FlowError> {
    let hook_id = format!("workflow-connector:invoke:{attempt}:{observation}");
    let snapshot = engine.snapshot(run_id).await?;
    let hook = snapshot
        .hooks
        .get(&hook_id)
        .ok_or_else(|| FlowError::Runtime(format!("missing Connector hook {hook_id}")))?;
    assert_eq!(hook.status, HookStatus::Active, "{snapshot:#?}");
    serde_json::from_value(hook.metadata.clone()).map_err(FlowError::Serialization)
}

fn accepted_evidence(
    metadata: &WorkflowConnectorHookMetadata,
    attempt_id: uuid::Uuid,
    request_digest: Sha256Digest,
    request_body_bytes: u64,
    completed_at: DateTime<Utc>,
) -> Result<WorkflowConnectorAttemptEvidence, FlowError> {
    accepted_evidence_for_body(
        metadata,
        attempt_id,
        request_digest,
        request_body_bytes,
        completed_at,
        br#"{"accepted":true}"#,
    )
}

fn accepted_evidence_for_body(
    metadata: &WorkflowConnectorHookMetadata,
    attempt_id: uuid::Uuid,
    request_digest: Sha256Digest,
    request_body_bytes: u64,
    completed_at: DateTime<Utc>,
    body: &[u8],
) -> Result<WorkflowConnectorAttemptEvidence, FlowError> {
    let response_digest = Sha256Digest::from_bytes(body);
    let response_object = response_object(attempt_id, response_digest.clone(), body.len() as u64)?;
    let result = if metadata.requires_response_object() {
        WorkflowConnectorAttemptEvidence::restore_with_response_object(
            attempt_id,
            request_digest,
            request_body_bytes,
            WorkflowConnectorAttemptOutcome::Accepted,
            Some(200),
            Some(response_digest),
            Some(body.len() as u64),
            Some(response_object),
            None,
            completed_at - Duration::seconds(1),
            completed_at,
        )
    } else {
        WorkflowConnectorAttemptEvidence::restore(
            attempt_id,
            request_digest,
            request_body_bytes,
            WorkflowConnectorAttemptOutcome::Accepted,
            Some(200),
            Some(response_digest),
            Some(body.len() as u64),
            None,
            completed_at - Duration::seconds(1),
            completed_at,
        )
    };
    result.map_err(FlowError::Runtime)
}

fn response_object(
    attempt_id: uuid::Uuid,
    digest: Sha256Digest,
    size_bytes: u64,
) -> Result<WorkflowConnectorResponseObjectReference, FlowError> {
    let hexadecimal = digest
        .as_str()
        .strip_prefix("sha256:")
        .ok_or_else(|| FlowError::Runtime("invalid response digest".into()))?;
    WorkflowConnectorResponseObjectReference::new(
        attempt_id,
        format!("attempts/{attempt_id}/sha256/{hexadecimal}/body"),
        digest,
        size_bytes,
    )
    .map_err(FlowError::Runtime)
}

async fn resume_accepted(
    engine: &FlowEngine,
    run_id: &str,
    metadata: &WorkflowConnectorHookMetadata,
    completed_at: DateTime<Utc>,
) -> Result<(), FlowError> {
    let authority = attempt_authority(metadata).map_err(FlowError::Runtime)?;
    let evidence = accepted_evidence(
        metadata,
        authority.attempt_id,
        authority.request_digest.clone(),
        authority.request_body_bytes,
        completed_at,
    )?;
    let payload = WorkflowConnectorResumePayload::completed(
        metadata,
        evidence,
        authority.attempt_id,
        &authority.request_digest,
        authority.request_body_bytes,
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

async fn resume_accepted_body(
    engine: &FlowEngine,
    run_id: &str,
    metadata: &WorkflowConnectorHookMetadata,
    completed_at: DateTime<Utc>,
    body: &[u8],
) -> Result<(), FlowError> {
    let authority = attempt_authority(metadata).map_err(FlowError::Runtime)?;
    let evidence = accepted_evidence_for_body(
        metadata,
        authority.attempt_id,
        authority.request_digest.clone(),
        authority.request_body_bytes,
        completed_at,
        body,
    )?;
    let payload = WorkflowConnectorResumePayload::completed(
        metadata,
        evidence,
        authority.attempt_id,
        &authority.request_digest,
        authority.request_body_bytes,
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

async fn resume_retryable(
    engine: &FlowEngine,
    run_id: &str,
    metadata: &WorkflowConnectorHookMetadata,
    completed_at: DateTime<Utc>,
) -> Result<(), FlowError> {
    let authority = attempt_authority(metadata).map_err(FlowError::Runtime)?;
    let evidence = if metadata.requires_response_object() {
        WorkflowConnectorAttemptEvidence::restore_with_response_object(
            authority.attempt_id,
            authority.request_digest.clone(),
            authority.request_body_bytes,
            WorkflowConnectorAttemptOutcome::Retryable,
            Some(503),
            None,
            None,
            None,
            None,
            completed_at - Duration::seconds(1),
            completed_at,
        )
    } else {
        WorkflowConnectorAttemptEvidence::restore(
            authority.attempt_id,
            authority.request_digest.clone(),
            authority.request_body_bytes,
            WorkflowConnectorAttemptOutcome::Retryable,
            Some(503),
            None,
            None,
            None,
            completed_at - Duration::seconds(1),
            completed_at,
        )
    }
    .map_err(FlowError::Runtime)?;
    let payload = WorkflowConnectorResumePayload::completed(
        metadata,
        evidence,
        authority.attempt_id,
        &authority.request_digest,
        authority.request_body_bytes,
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
