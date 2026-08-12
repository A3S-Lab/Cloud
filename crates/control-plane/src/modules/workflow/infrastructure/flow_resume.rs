use crate::modules::workflow::domain::{FlowResumePayload, FlowResumeReceipt};
use a3s_flow::{FlowEvent, FlowEventEnvelope};

pub fn observe_flow_resume_receipt(
    payload: &FlowResumePayload,
    envelope: &FlowEventEnvelope,
) -> Result<FlowResumeReceipt, String> {
    match &envelope.event {
        FlowEvent::HookReceived {
            hook_id,
            payload: observed_payload,
        } => FlowResumeReceipt::from_hook_received(
            payload,
            &envelope.run_id,
            hook_id,
            observed_payload,
            envelope.sequence,
            envelope.event_id,
            envelope.timestamp,
        ),
        FlowEvent::RunTimedOut { deadline, reason } => FlowResumeReceipt::from_run_timed_out(
            payload,
            &envelope.run_id,
            *deadline,
            reason.clone(),
            envelope.sequence,
            envelope.event_id,
            envelope.timestamp,
        ),
        FlowEvent::RunCancelled { reason } => FlowResumeReceipt::from_run_cancelled(
            payload,
            &envelope.run_id,
            reason.clone(),
            envelope.sequence,
            envelope.event_id,
            envelope.timestamp,
        ),
        _ => Err("Flow event is not resume settlement evidence".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::WorkflowDecisionId;
    use crate::modules::workflow::domain::{FlowResumePayload, WorkflowDecision};
    use crate::modules::workflow::test_support::{
        accepted_submission, claimed_task, digest, timestamp, TEST_HOOK_ID,
    };
    use a3s_flow::{
        FlowEngine, FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
        WorkflowSpec,
    };
    use serde_json::json;
    use std::sync::Arc;

    struct DecisionHookRuntime;

    #[async_trait::async_trait]
    impl FlowRuntime for DecisionHookRuntime {
        async fn run_workflow(
            &self,
            invocation: WorkflowInvocation,
        ) -> a3s_flow::Result<RuntimeCommand> {
            let context = invocation.context();
            if let Some(payload) = context.hook_payload(TEST_HOOK_ID) {
                return Ok(context.complete(json!({
                    "decisionDigest": payload["decisionDigest"],
                    "outcome": payload["outcome"],
                })));
            }
            Ok(context.create_hook(
                TEST_HOOK_ID,
                "internal-human-review-token",
                json!({"kind": "human_task"}),
            ))
        }

        async fn run_step(
            &self,
            _invocation: StepInvocation,
        ) -> a3s_flow::Result<serde_json::Value> {
            unreachable!("decision hook runtime does not execute steps")
        }
    }

    #[tokio::test]
    async fn flow_resume_redelivery_is_identical_or_conflicting_by_payload() {
        let (task, principal_id) = claimed_task();
        let submission = accepted_submission(&task, principal_id);
        let decision = WorkflowDecision::from_submission(
            WorkflowDecisionId::new(),
            &task,
            &submission,
            submission.accepted_output().expect("accepted output"),
            timestamp(8, 31),
        )
        .expect("decision");
        let payload = FlowResumePayload::from_decision(&decision).expect("resume payload");
        let flow_value = payload.to_flow_value().expect("Flow payload");
        let engine = FlowEngine::in_memory(Arc::new(DecisionHookRuntime));
        engine
            .start_with_id(
                &task.flow_run_id,
                WorkflowSpec::rust_embedded(
                    "cloud.human-task-contract",
                    "1",
                    "cloud::workflow",
                    "human_task",
                ),
                json!({}),
            )
            .await
            .expect("Flow run should start");

        engine
            .resume_hook(&task.flow_run_id, &task.flow_hook_id, flow_value.clone())
            .await
            .expect("first resume should commit");
        let committed_history = engine
            .history(&task.flow_run_id)
            .await
            .expect("Flow history");
        let hook_received = committed_history
            .iter()
            .find(|envelope| matches!(envelope.event, FlowEvent::HookReceived { .. }))
            .expect("HookReceived evidence");
        observe_flow_resume_receipt(&payload, hook_received).expect("resume receipt");
        assert!(observe_flow_resume_receipt(&payload, &committed_history[0]).is_err());

        engine
            .resume_hook(&task.flow_run_id, &task.flow_hook_id, flow_value.clone())
            .await
            .expect("identical terminal redelivery should be idempotent");
        assert_eq!(
            engine
                .history(&task.flow_run_id)
                .await
                .expect("Flow history after redelivery"),
            committed_history
        );

        let mut drifted = flow_value;
        drifted["digest"] = json!(digest('f'));
        let error = engine
            .resume_hook(&task.flow_run_id, &task.flow_hook_id, drifted)
            .await
            .expect_err("different redelivery must conflict");
        assert!(matches!(error, FlowError::HookConflict { .. }));
    }
}
