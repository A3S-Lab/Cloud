use super::connector::attempt_authority;
use super::WorkflowRunFlowRuntime;
use crate::modules::connectors::domain::ConnectorResponseObjectReference;
use crate::modules::connectors::{
    ConnectorResponseObjectContent, IConnectorResponseObjectPort, ReadConnectorResponseObject,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{canonical_timestamp, Sha256Digest};
use crate::modules::workflow::domain::{
    flow_step_id, WorkflowConnectorAttemptEvidence, WorkflowConnectorAttemptOutcome,
    WorkflowConnectorHookMetadata, WorkflowConnectorResponseObjectReference,
    WorkflowConnectorResumePayload, WORKFLOW_RUN_FLOW_NAME,
};
use crate::modules::workflow::test_support::compensating_connector_workflow_run_input;
use a3s_flow::{FlowEngine, FlowError, HookStatus, WorkflowRunStatus, WorkflowSpec};
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
