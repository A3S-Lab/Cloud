use super::connector::attempt_authority;
use super::{WorkflowLocalStepResult, WorkflowRunFlowRuntime};
use crate::infrastructure::CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID;
use crate::modules::connectors::domain::ConnectorResponseObjectReference;
use crate::modules::connectors::{
    ConnectorResponseObjectContent, IConnectorResponseObjectPort, ReadConnectorResponseObject,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId, Sha256Digest};
use crate::modules::workflow::domain::{
    flow_step_id, WorkflowConnectorAttemptEvidence, WorkflowConnectorAttemptOutcome,
    WorkflowConnectorHookMetadata, WorkflowConnectorInvocationPurpose,
    WorkflowConnectorResponseObjectReference, WorkflowConnectorResumePayload, WorkflowRun,
    WorkflowRunRecord, WorkflowStepKind, WorkflowStepProjectionStatus, WORKFLOW_RUN_FLOW_NAME,
};
use crate::modules::workflow::test_support::{
    cancellation_compensating_connector_workflow_run_input,
    compensating_connector_workflow_run_input,
    multiple_cancellation_compensating_connector_workflow_run_input,
};
use a3s_flow::{
    CancellationRequest, FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, FlowEventStore,
    HookStatus, InMemoryEventStore, RetryPolicy, RuntimeBuildCompatibility, RuntimeBuildId,
    RuntimeCommand, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default)]
struct AttemptBoundResponseObjects {
    bodies: Mutex<BTreeMap<uuid::Uuid, Vec<u8>>>,
    reads: AtomicUsize,
    requests: Mutex<Vec<ConnectorResponseObjectReference>>,
}

impl AttemptBoundResponseObjects {
    async fn bind(&self, attempt_id: uuid::Uuid, body: &[u8]) {
        self.bodies.lock().await.insert(attempt_id, body.to_vec());
    }
}

#[async_trait::async_trait]
impl IConnectorResponseObjectPort for AttemptBoundResponseObjects {
    async fn read_response_object(
        &self,
        request: &ReadConnectorResponseObject,
    ) -> ApplicationResult<ConnectorResponseObjectContent> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().await.push(request.reference.clone());
        let body = self
            .bodies
            .lock()
            .await
            .get(&request.reference.connector_attempt_id)
            .cloned()
            .ok_or_else(|| {
                ApplicationError::Internal(
                    "Connector compensation test response is not bound to its attempt".into(),
                )
            })?;
        ConnectorResponseObjectContent::for_test(request.reference.clone(), body)
    }
}

#[tokio::test]
async fn domain_failure_runs_one_durable_connector_compensation_before_completion(
) -> Result<(), FlowError> {
    let mut input = compensating_connector_workflow_run_input().map_err(FlowError::Runtime)?;
    input.requested_at = canonical_timestamp(Utc::now());
    input.deadline_at = input.requested_at + Duration::hours(1);
    input.validate().map_err(FlowError::Runtime)?;
    let run_id = input.workflow_run_id.to_string();
    let responses = Arc::new(AttemptBoundResponseObjects::default());
    let runtime = WorkflowRunFlowRuntime::with_connector_responses(
        responses.clone() as Arc<dyn IConnectorResponseObjectPort>
    );
    let engine = FlowEngine::in_memory(Arc::new(runtime));
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

    let reserve_body = br#"{"ok":true,"reservationId":"resv-0001"}"#;
    let charge_body = br#"{"ok":false,"reservationId":"resv-0001","reason":"card_declined"}"#;
    let release_body = br#"{"released":true,"reservationId":"resv-0001"}"#;

    let reserve = active_hook(&engine, &run_id, "reserve").await?;
    let reserve_authority = attempt_authority(&reserve).map_err(FlowError::Runtime)?;
    responses
        .bind(reserve_authority.attempt_id, reserve_body)
        .await;
    resume_accepted_body(
        &engine,
        &run_id,
        &reserve,
        input.requested_at + Duration::seconds(1),
        reserve_body,
    )
    .await?;

    let charge = active_hook(&engine, &run_id, "charge").await?;
    assert_eq!(
        charge.effective_input,
        serde_json::json!({"ok": true, "reservationId": "resv-0001"})
    );
    let charge_authority = attempt_authority(&charge).map_err(FlowError::Runtime)?;
    responses
        .bind(charge_authority.attempt_id, charge_body)
        .await;
    resume_accepted_body(
        &engine,
        &run_id,
        &charge,
        input.requested_at + Duration::seconds(2),
        charge_body,
    )
    .await?;

    let release = active_hook(&engine, &run_id, "release").await?;
    assert_eq!(
        release.effective_input,
        serde_json::json!({
            "ok": false,
            "reservationId": "resv-0001",
            "reason": "card_declined",
        })
    );
    let release_authority = attempt_authority(&release).map_err(FlowError::Runtime)?;
    assert_ne!(reserve_authority.attempt_id, charge_authority.attempt_id);
    assert_ne!(reserve_authority.attempt_id, release_authority.attempt_id);
    assert_ne!(charge_authority.attempt_id, release_authority.attempt_id);
    responses
        .bind(release_authority.attempt_id, release_body)
        .await;
    resume_accepted_body(
        &engine,
        &run_id,
        &release,
        input.requested_at + Duration::seconds(3),
        release_body,
    )
    .await?;

    let completed = engine.snapshot(&run_id).await?;
    assert_eq!(
        completed.status,
        WorkflowRunStatus::Completed,
        "{completed:#?}"
    );
    assert_eq!(
        completed.output,
        Some(serde_json::json!({
            "compensation_output": {
                "released": true,
                "reservationId": "resv-0001",
            },
            "failure_output": {
                "ok": false,
                "reservationId": "resv-0001",
                "reason": "card_declined",
            },
        }))
    );
    for step_id in ["reserve", "charge", "release"] {
        assert!(completed.steps.contains_key(&flow_step_id(step_id)));
        assert_eq!(
            completed.hooks[&format!("workflow-connector:{step_id}:1:1")].status,
            HookStatus::Received
        );
    }
    assert!(!completed
        .hooks
        .contains_key("workflow-connector:release:2:1"));
    assert_eq!(responses.reads.load(Ordering::SeqCst), 3);
    assert_eq!(
        responses
            .requests
            .lock()
            .await
            .iter()
            .map(|request| request.connector_attempt_id)
            .collect::<Vec<_>>(),
        [
            reserve_authority.attempt_id,
            charge_authority.attempt_id,
            release_authority.attempt_id,
        ]
    );

    let history_length = engine.history(&run_id).await?.len();
    resume_accepted_body(
        &engine,
        &run_id,
        &release,
        input.requested_at + Duration::seconds(3),
        release_body,
    )
    .await?;
    assert_eq!(engine.history(&run_id).await?.len(), history_length);
    assert_eq!(responses.reads.load(Ordering::SeqCst), 3);
    assert!(!engine
        .snapshot(&run_id)
        .await?
        .hooks
        .contains_key("workflow-connector:release:2:1"));
    Ok(())
}

#[tokio::test]
async fn cancellation_runs_one_durable_connector_compensation_before_cancelled(
) -> Result<(), FlowError> {
    let mut input =
        cancellation_compensating_connector_workflow_run_input().map_err(FlowError::Runtime)?;
    input.requested_at = canonical_timestamp(Utc::now());
    input.deadline_at = input.requested_at + Duration::hours(1);
    input.validate().map_err(FlowError::Runtime)?;
    let run_id = input.workflow_run_id.to_string();
    let (run, steps) = WorkflowRun::create(input.clone(), PrincipalId::new())
        .map_err(FlowError::InvalidWorkflow)?;
    let mut record = WorkflowRunRecord { run, steps };
    let responses = Arc::new(AttemptBoundResponseObjects::default());
    let runtime = WorkflowRunFlowRuntime::with_connector_responses(
        responses.clone() as Arc<dyn IConnectorResponseObjectPort>
    );
    let runtime_build = RuntimeBuildId::new(CURRENT_CLOUD_FLOW_RUNTIME_BUILD_ID)?;
    let engine = FlowEngine::builder(Arc::new(runtime))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build.clone()))
        .build();
    engine
        .start_with_id(
            run_id.clone(),
            WorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                input.flow_workflow_version.clone(),
                "a3s-cloud",
                "main",
            )
            .with_runtime_build(runtime_build),
            serde_json::to_value(&input)?,
        )
        .await?;

    let reserve_body = br#"{"ok":true,"reservationId":"resv-cancel-0001"}"#;
    let release_body = br#"{"released":true,"reservationId":"resv-cancel-0001"}"#;
    let reserve = active_hook(&engine, &run_id, "reserve").await?;
    let reserve_authority = attempt_authority(&reserve).map_err(FlowError::Runtime)?;
    responses
        .bind(reserve_authority.attempt_id, reserve_body)
        .await;
    resume_accepted_body(
        &engine,
        &run_id,
        &reserve,
        input.requested_at + Duration::seconds(1),
        reserve_body,
    )
    .await?;
    assert_eq!(
        active_hook(&engine, &run_id, "charge").await?.purpose,
        WorkflowConnectorInvocationPurpose::Normal
    );

    let cancelling = engine
        .request_cancellation(
            &run_id,
            CancellationRequest::new(Some("operator request".into())),
        )
        .await?;
    assert_eq!(cancelling.status, WorkflowRunStatus::Cancelling);
    assert_eq!(
        cancelling.hooks["workflow-connector:charge:1:1"].status,
        HookStatus::Cancelled
    );
    let compensation_id = "workflow-connector-cancellation-compensation:reserve:release:1:1";
    let compensation_hook = cancelling
        .hooks
        .get(compensation_id)
        .ok_or_else(|| FlowError::Runtime("missing cancellation compensation hook".into()))?;
    assert_eq!(compensation_hook.status, HookStatus::Active);
    let compensation = serde_json::from_value::<WorkflowConnectorHookMetadata>(
        compensation_hook.metadata.clone(),
    )?;
    assert_eq!(
        compensation.purpose,
        WorkflowConnectorInvocationPurpose::CancellationCompensation {
            source_step_id: "reserve".into(),
        }
    );
    assert_eq!(
        compensation.effective_input,
        serde_json::json!({"ok": true, "reservationId": "resv-cancel-0001"})
    );
    record
        .run
        .request_cancellation(
            Some("operator request".into()),
            PrincipalId::new(),
            input.requested_at,
        )
        .map_err(FlowError::InvalidWorkflow)?;
    let cancelling_history = engine.history(&run_id).await?;
    let cancellation_sequence = cancelling_history
        .iter()
        .find_map(|event| {
            matches!(&event.event, FlowEvent::RunCancellationRequested { .. })
                .then_some(event.sequence)
        })
        .ok_or_else(|| FlowError::Runtime("cancellation request history is missing".into()))?;
    let cancelling_record =
        super::project_workflow_run_record(&record, &cancelling, &cancelling_history)
            .map_err(FlowError::Runtime)?
            .ok_or_else(|| FlowError::Runtime("cancelling projection did not advance".into()))?;
    let cancelling_charge = cancelling_record
        .steps
        .iter()
        .find(|step| step.step_id == "charge")
        .ok_or_else(|| FlowError::Runtime("cancelling charge projection is missing".into()))?;
    assert_eq!(
        cancelling_charge.status,
        WorkflowStepProjectionStatus::Cancelled
    );
    assert_eq!(cancelling_charge.last_flow_sequence, cancellation_sequence);
    let compensating_history_length = engine.history(&run_id).await?.len();
    let repeated_cancelling = engine
        .request_cancellation(
            &run_id,
            CancellationRequest::new(Some("operator request".into())),
        )
        .await?;
    assert_eq!(repeated_cancelling.status, WorkflowRunStatus::Cancelling);
    assert_eq!(
        repeated_cancelling.hooks[compensation_id].status,
        HookStatus::Active
    );
    assert_eq!(
        engine.history(&run_id).await?.len(),
        compensating_history_length
    );
    let compensation_authority = attempt_authority(&compensation).map_err(FlowError::Runtime)?;
    responses
        .bind(compensation_authority.attempt_id, release_body)
        .await;
    resume_accepted_body(
        &engine,
        &run_id,
        &compensation,
        input.requested_at + Duration::seconds(2),
        release_body,
    )
    .await?;

    let cancelled = engine.snapshot(&run_id).await?;
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    assert_eq!(
        cancelled.hooks[compensation_id].status,
        HookStatus::Received
    );
    assert_eq!(responses.reads.load(Ordering::SeqCst), 2);
    assert!(!cancelled
        .hooks
        .contains_key("workflow-connector-cancellation-compensation:reserve:release:2:1"));
    let cancelled_history = engine.history(&run_id).await?;
    let cancelled_record =
        super::project_workflow_run_record(&cancelling_record, &cancelled, &cancelled_history)
            .map_err(FlowError::Runtime)?
            .ok_or_else(|| FlowError::Runtime("cancelled projection did not advance".into()))?;
    let cancelled_charge = cancelled_record
        .steps
        .iter()
        .find(|step| step.step_id == "charge")
        .ok_or_else(|| FlowError::Runtime("cancelled charge projection is missing".into()))?;
    assert_eq!(
        cancelled_charge.status,
        WorkflowStepProjectionStatus::Cancelled
    );
    assert_eq!(cancelled_charge.last_flow_sequence, cancellation_sequence);
    let cancelled_release = cancelled_record
        .steps
        .iter()
        .find(|step| step.step_id == "release")
        .ok_or_else(|| FlowError::Runtime("cancelled compensation projection is missing".into()))?;
    assert_eq!(
        cancelled_release.status,
        WorkflowStepProjectionStatus::Completed
    );
    let history_length = engine.history(&run_id).await?.len();
    engine
        .request_cancellation(
            &run_id,
            CancellationRequest::new(Some("operator request".into())),
        )
        .await?;
    assert_eq!(engine.history(&run_id).await?.len(), history_length);
    assert_eq!(responses.reads.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn cancellation_compensation_runs_completed_connectors_in_reverse_plan_order(
) -> Result<(), FlowError> {
    let mut input = multiple_cancellation_compensating_connector_workflow_run_input()
        .map_err(FlowError::Runtime)?;
    input.requested_at = canonical_timestamp(Utc::now());
    input.deadline_at = input.requested_at + Duration::hours(1);
    input.validate().map_err(FlowError::Runtime)?;
    let run_id = input.workflow_run_id.to_string();
    let responses = Arc::new(AttemptBoundResponseObjects::default());
    let runtime = WorkflowRunFlowRuntime::with_connector_responses(
        responses.clone() as Arc<dyn IConnectorResponseObjectPort>
    );
    let engine = FlowEngine::in_memory(Arc::new(runtime));
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

    let reserve_body = br#"{"ok":true,"reservationId":"resv-first"}"#;
    let reserve_second_body = br#"{"ok":true,"reservationId":"resv-second"}"#;
    let release_second_body = br#"{"released":true,"reservationId":"resv-second"}"#;
    let release_body = br#"{"released":true,"reservationId":"resv-first"}"#;
    for (step_id, body, completed_after) in [
        ("reserve", reserve_body.as_slice(), 1),
        ("reserve_second", reserve_second_body.as_slice(), 2),
    ] {
        let metadata = active_hook(&engine, &run_id, step_id).await?;
        let authority = attempt_authority(&metadata).map_err(FlowError::Runtime)?;
        responses.bind(authority.attempt_id, body).await;
        resume_accepted_body(
            &engine,
            &run_id,
            &metadata,
            input.requested_at + Duration::seconds(completed_after),
            body,
        )
        .await?;
    }
    assert_eq!(
        active_hook(&engine, &run_id, "charge").await?.purpose,
        WorkflowConnectorInvocationPurpose::Normal
    );

    let cancelling = engine
        .request_cancellation(
            &run_id,
            CancellationRequest::new(Some("reverse-order test".into())),
        )
        .await?;
    assert_eq!(cancelling.status, WorkflowRunStatus::Cancelling);

    let release_second =
        active_compensation_hook(&engine, &run_id, "reserve_second", "release_second").await?;
    assert_eq!(
        release_second.effective_input,
        serde_json::json!({
            "ok": true,
            "reservationId": "resv-second",
        })
    );
    let release_second_authority =
        attempt_authority(&release_second).map_err(FlowError::Runtime)?;
    responses
        .bind(release_second_authority.attempt_id, release_second_body)
        .await;
    resume_accepted_body(
        &engine,
        &run_id,
        &release_second,
        input.requested_at + Duration::seconds(3),
        release_second_body,
    )
    .await?;

    let release = active_compensation_hook(&engine, &run_id, "reserve", "release").await?;
    assert_eq!(
        release.effective_input,
        serde_json::json!({
            "ok": true,
            "reservationId": "resv-first",
        })
    );
    let release_authority = attempt_authority(&release).map_err(FlowError::Runtime)?;
    assert_ne!(
        release_authority.attempt_id,
        release_second_authority.attempt_id
    );
    responses
        .bind(release_authority.attempt_id, release_body)
        .await;
    resume_accepted_body(
        &engine,
        &run_id,
        &release,
        input.requested_at + Duration::seconds(4),
        release_body,
    )
    .await?;

    let cancelled = engine.snapshot(&run_id).await?;
    assert_eq!(cancelled.status, WorkflowRunStatus::Cancelled);
    assert_eq!(responses.reads.load(Ordering::SeqCst), 4);
    assert!(!cancelled.hooks.contains_key(
        "workflow-connector-cancellation-compensation:reserve_second:release_second:2:1"
    ));
    assert!(!cancelled
        .hooks
        .contains_key("workflow-connector-cancellation-compensation:reserve:release:2:1"));
    Ok(())
}

#[tokio::test]
async fn cancellation_compensation_materializes_an_accepted_source_before_ordinary_step_creation(
) -> Result<(), FlowError> {
    assert_cancellation_source_response_materialization(false).await
}

#[tokio::test]
async fn cancellation_compensation_materializes_an_accepted_source_while_ordinary_step_is_pending(
) -> Result<(), FlowError> {
    assert_cancellation_source_response_materialization(true).await
}

async fn assert_cancellation_source_response_materialization(
    ordinary_materializer_created: bool,
) -> Result<(), FlowError> {
    let input =
        cancellation_compensating_connector_workflow_run_input().map_err(FlowError::Runtime)?;
    let source = input
        .resolved_steps()
        .map_err(FlowError::Runtime)?
        .into_iter()
        .find(|step| step.plan.id == "reserve")
        .ok_or_else(|| FlowError::Runtime("missing cancellation source".into()))?;
    let metadata = WorkflowConnectorHookMetadata::from_run_step(
        &input,
        &source,
        input.goal_input.clone(),
        1,
        1,
    )
    .map_err(FlowError::Runtime)?;
    let body = br#"{"ok":true,"reservationId":"resv-race-0001"}"#;
    let authority = attempt_authority(&metadata).map_err(FlowError::Runtime)?;
    let response_digest = Sha256Digest::from_bytes(body);
    let evidence = WorkflowConnectorAttemptEvidence::restore_with_response_object(
        authority.attempt_id,
        authority.request_digest.clone(),
        authority.request_body_bytes,
        WorkflowConnectorAttemptOutcome::Accepted,
        Some(200),
        Some(response_digest.clone()),
        Some(body.len() as u64),
        Some(response_object(
            authority.attempt_id,
            response_digest,
            body.len() as u64,
        )?),
        None,
        input.requested_at,
        input.requested_at + Duration::seconds(1),
    )
    .map_err(FlowError::Runtime)?;
    let payload = WorkflowConnectorResumePayload::completed(
        &metadata,
        evidence.clone(),
        authority.attempt_id,
        &authority.request_digest,
        authority.request_body_bytes,
    )
    .map_err(FlowError::Runtime)?;
    let response_step = super::connector_response::WorkflowConnectorResponseStepInput::new(
        &input.runtime_contract_revision,
        &source,
        &metadata,
        &evidence,
    )
    .map_err(FlowError::Runtime)?;
    let mut history = vec![
        envelope(
            &input,
            1,
            FlowEvent::HookCreated {
                hook_id: metadata.flow_hook_id(),
                token: metadata.flow_hook_token(),
                metadata: serde_json::to_value(&metadata)?,
            },
        ),
        envelope(
            &input,
            2,
            FlowEvent::HookReceived {
                hook_id: metadata.flow_hook_id(),
                payload: serde_json::to_value(payload)?,
            },
        ),
    ];
    if ordinary_materializer_created {
        history.push(envelope(
            &input,
            3,
            FlowEvent::StepCreated {
                step_id: flow_step_id("reserve"),
                step_name: super::connector_response::WORKFLOW_CONNECTOR_RESPONSE_STEP_NAME.into(),
                input: serde_json::to_value(&response_step)?,
                retry: RetryPolicy::none(),
            },
        ));
    }
    let cancellation_sequence = history.len() as u64 + 1;
    history.push(envelope(
        &input,
        cancellation_sequence,
        FlowEvent::RunCancellationRequested {
            request: CancellationRequest::new(Some("response race".into())),
        },
    ));
    let command = super::workflow::run_workflow(invocation(&input, history.clone()))?;
    let RuntimeCommand::ScheduleSteps { steps } = command else {
        return Err(FlowError::Runtime(format!(
            "expected cancellation-source response materialization, got {command:?}"
        )));
    };
    assert_eq!(steps.len(), 1);
    let cleanup = &steps[0];
    assert_eq!(
        cleanup.step_id,
        super::cancellation::cancellation_source_response_step_id("reserve")
    );
    assert_ne!(cleanup.step_id, flow_step_id("reserve"));
    assert_eq!(
        cleanup.step_name,
        super::connector_response::WORKFLOW_CONNECTOR_RESPONSE_STEP_NAME
    );
    assert_eq!(cleanup.retry, RetryPolicy::none());
    assert_eq!(cleanup.input, serde_json::to_value(&response_step)?);

    let output: serde_json::Value = serde_json::from_slice(body)?;
    let result = WorkflowLocalStepResult {
        step_id: "reserve".into(),
        kind: WorkflowStepKind::Service,
        output_digest: super::execution::value_digest(
            &output,
            "cancellation-source response test output",
        )
        .map_err(FlowError::Runtime)?,
        output,
        selected_handle: None,
        composite_region_result: None,
        default_output_evidence: None,
    };
    result.validate(&source).map_err(FlowError::Runtime)?;
    history.push(envelope(
        &input,
        cancellation_sequence + 1,
        FlowEvent::StepCreated {
            step_id: cleanup.step_id.clone(),
            step_name: cleanup.step_name.clone(),
            input: cleanup.input.clone(),
            retry: cleanup.retry,
        },
    ));
    history.push(envelope(
        &input,
        cancellation_sequence + 2,
        FlowEvent::StepCompleted {
            step_id: cleanup.step_id.clone(),
            output: serde_json::to_value(&result)?,
        },
    ));

    let command = super::workflow::run_workflow(invocation(&input, history))?;
    assert!(matches!(
        command,
        RuntimeCommand::CreateHook { hook_id, .. }
            if hook_id == "workflow-connector-cancellation-compensation:reserve:release:1:1"
    ));

    let run_id = input.workflow_run_id.to_string();
    let runtime_build_id = RuntimeBuildId::new("a3s-cloud-cancellation-race-test@1")?;
    let spec = WorkflowSpec::rust_embedded(
        WORKFLOW_RUN_FLOW_NAME,
        input.flow_workflow_version.clone(),
        "a3s-cloud",
        "main",
    )
    .with_runtime_build(runtime_build_id);
    let store = Arc::new(InMemoryEventStore::new());
    store
        .append(
            &run_id,
            FlowEvent::RunCreated {
                spec,
                input: serde_json::to_value(&input)?,
            },
        )
        .await?;
    store.append(&run_id, FlowEvent::RunStarted).await?;
    store
        .append(
            &run_id,
            FlowEvent::HookCreated {
                hook_id: metadata.flow_hook_id(),
                token: metadata.flow_hook_token(),
                metadata: serde_json::to_value(&metadata)?,
            },
        )
        .await?;
    store
        .append(
            &run_id,
            FlowEvent::HookReceived {
                hook_id: metadata.flow_hook_id(),
                payload: serde_json::to_value(
                    WorkflowConnectorResumePayload::completed(
                        &metadata,
                        evidence,
                        authority.attempt_id,
                        &authority.request_digest,
                        authority.request_body_bytes,
                    )
                    .map_err(FlowError::Runtime)?,
                )?,
            },
        )
        .await?;
    if ordinary_materializer_created {
        store
            .append(
                &run_id,
                FlowEvent::StepCreated {
                    step_id: flow_step_id("reserve"),
                    step_name: super::connector_response::WORKFLOW_CONNECTOR_RESPONSE_STEP_NAME
                        .into(),
                    input: serde_json::to_value(&response_step)?,
                    retry: RetryPolicy::none(),
                },
            )
            .await?;
    }
    store
        .append(
            &run_id,
            FlowEvent::RunCancellationRequested {
                request: CancellationRequest::new(Some("response race".into())),
            },
        )
        .await?;
    store
        .append(
            &run_id,
            FlowEvent::StepCreated {
                step_id: cleanup.step_id.clone(),
                step_name: cleanup.step_name.clone(),
                input: cleanup.input.clone(),
                retry: cleanup.retry,
            },
        )
        .await?;
    store
        .append(
            &run_id,
            FlowEvent::StepStarted {
                step_id: cleanup.step_id.clone(),
                attempt: 1,
            },
        )
        .await?;
    store
        .append(
            &run_id,
            FlowEvent::StepCompleted {
                step_id: cleanup.step_id.clone(),
                output: serde_json::to_value(result)?,
            },
        )
        .await?;
    let projection_engine = FlowEngine::new(store, Arc::new(WorkflowRunFlowRuntime::default()));
    let snapshot = projection_engine.snapshot(&run_id).await?;
    let projection_history = projection_engine.history(&run_id).await?;
    let cancellation_requested_at = input.requested_at + Duration::seconds(2);
    let (mut run, steps) =
        WorkflowRun::create(input, PrincipalId::new()).map_err(FlowError::InvalidWorkflow)?;
    run.request_cancellation(
        Some("response race".into()),
        PrincipalId::new(),
        cancellation_requested_at,
    )
    .map_err(FlowError::InvalidWorkflow)?;
    let projected = super::project_workflow_run_record(
        &WorkflowRunRecord { run, steps },
        &snapshot,
        &projection_history,
    )
    .map_err(FlowError::Runtime)?
    .ok_or_else(|| FlowError::Runtime("cancellation race projection did not advance".into()))?;
    let source_projection = projected
        .steps
        .iter()
        .find(|step| step.step_id == "reserve")
        .ok_or_else(|| FlowError::Runtime("cancellation source projection is missing".into()))?;
    assert_eq!(
        source_projection.status,
        WorkflowStepProjectionStatus::Completed
    );
    assert_eq!(
        source_projection.result,
        Some(serde_json::json!({
            "ok": true,
            "reservationId": "resv-race-0001",
        }))
    );
    assert_eq!(source_projection.last_flow_sequence, snapshot.last_sequence);
    Ok(())
}

async fn active_hook(
    engine: &FlowEngine,
    run_id: &str,
    step_id: &str,
) -> Result<WorkflowConnectorHookMetadata, FlowError> {
    let hook_id = format!("workflow-connector:{step_id}:1:1");
    let snapshot = engine.snapshot(run_id).await?;
    let hook = snapshot
        .hooks
        .get(&hook_id)
        .ok_or_else(|| FlowError::Runtime(format!("missing Connector hook {hook_id}")))?;
    assert_eq!(hook.status, HookStatus::Active, "{snapshot:#?}");
    serde_json::from_value(hook.metadata.clone()).map_err(FlowError::Serialization)
}

async fn active_compensation_hook(
    engine: &FlowEngine,
    run_id: &str,
    source_step_id: &str,
    target_step_id: &str,
) -> Result<WorkflowConnectorHookMetadata, FlowError> {
    let hook_id = format!(
        "workflow-connector-cancellation-compensation:{source_step_id}:{target_step_id}:1:1"
    );
    let snapshot = engine.snapshot(run_id).await?;
    let hook = snapshot
        .hooks
        .get(&hook_id)
        .ok_or_else(|| FlowError::Runtime(format!("missing Connector hook {hook_id}")))?;
    assert_eq!(hook.status, HookStatus::Active, "{snapshot:#?}");
    serde_json::from_value(hook.metadata.clone()).map_err(FlowError::Serialization)
}

async fn resume_accepted_body(
    engine: &FlowEngine,
    run_id: &str,
    metadata: &WorkflowConnectorHookMetadata,
    completed_at: DateTime<Utc>,
    body: &[u8],
) -> Result<(), FlowError> {
    let authority = attempt_authority(metadata).map_err(FlowError::Runtime)?;
    let response_digest = Sha256Digest::from_bytes(body);
    let response_object = response_object(
        authority.attempt_id,
        response_digest.clone(),
        body.len() as u64,
    )?;
    let evidence = WorkflowConnectorAttemptEvidence::restore_with_response_object(
        authority.attempt_id,
        authority.request_digest.clone(),
        authority.request_body_bytes,
        WorkflowConnectorAttemptOutcome::Accepted,
        Some(200),
        Some(response_digest),
        Some(body.len() as u64),
        Some(response_object),
        None,
        completed_at - Duration::seconds(1),
        completed_at,
    )
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

fn invocation(
    input: &crate::modules::workflow::domain::WorkflowRunInput,
    history: Vec<FlowEventEnvelope>,
) -> WorkflowInvocation {
    WorkflowInvocation::new(
        input.workflow_run_id.to_string(),
        WorkflowSpec::rust_embedded(
            WORKFLOW_RUN_FLOW_NAME,
            input.flow_workflow_version.clone(),
            "a3s-cloud",
            "main",
        ),
        serde_json::to_value(input).expect("WorkflowRun input JSON"),
        history,
    )
}

fn envelope(
    input: &crate::modules::workflow::domain::WorkflowRunInput,
    sequence: u64,
    event: FlowEvent,
) -> FlowEventEnvelope {
    FlowEventEnvelope::new(
        input.workflow_run_id.to_string(),
        sequence,
        uuid::Uuid::now_v7(),
        input.requested_at + Duration::milliseconds(sequence as i64),
        event,
    )
}
