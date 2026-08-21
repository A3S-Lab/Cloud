use a3s_flow::{RetryPolicy, WorkflowContext};
use std::time::Duration;

pub(crate) const BOUNDED_STEP_RETRY_PATCH_ID: &str = "cloud.flow.bounded-step-retries-v1";
pub(crate) const FLOW_STEP_MAX_ATTEMPTS: u32 = 8;
pub(crate) const FLOW_STEP_MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

pub(crate) fn flow_step_retry_policy(
    context: &WorkflowContext<'_>,
    initial_delay: Duration,
) -> RetryPolicy {
    if context.has_patch_marker(BOUNDED_STEP_RETRY_PATCH_ID) {
        RetryPolicy::exponential(
            FLOW_STEP_MAX_ATTEMPTS,
            initial_delay.min(FLOW_STEP_MAX_RETRY_DELAY),
            FLOW_STEP_MAX_RETRY_DELAY,
        )
        .continue_workflow_on_failure()
    } else {
        RetryPolicy::fixed(u32::MAX, initial_delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_flow::{
        RetryBackoff, StepFailureAction, WorkflowInvocation, WorkflowPatchId, WorkflowSpec,
    };
    use serde_json::json;

    #[test]
    fn unmarked_histories_keep_the_exact_unbounded_fixed_retry_contract() {
        let invocation = workflow_invocation(WorkflowSpec::rust_embedded(
            "cloud.test",
            "1",
            "a3s-cloud",
            "main",
        ));
        let policy = flow_step_retry_policy(&invocation.context(), Duration::from_secs(2));

        assert_eq!(policy.max_attempts, u32::MAX);
        assert_eq!(policy.delay_ms, 2_000);
        assert_eq!(policy.backoff, RetryBackoff::Fixed);
        assert_eq!(policy.max_delay_ms, 0);
        assert_eq!(policy.on_exhausted, StepFailureAction::FailRun);
    }

    #[test]
    fn marked_histories_use_a_finite_capped_policy_and_replay_after_exhaustion() {
        let spec = WorkflowSpec::rust_embedded("cloud.test", "1", "a3s-cloud", "main")
            .with_patch_marker(
                WorkflowPatchId::new(BOUNDED_STEP_RETRY_PATCH_ID)
                    .expect("bounded retry patch ID must remain valid"),
            );
        let invocation = workflow_invocation(spec);
        let policy = flow_step_retry_policy(&invocation.context(), Duration::from_secs(60));

        assert_eq!(policy.max_attempts, FLOW_STEP_MAX_ATTEMPTS);
        assert_eq!(
            policy.delay_ms,
            FLOW_STEP_MAX_RETRY_DELAY.as_millis() as u64
        );
        assert_eq!(policy.backoff, RetryBackoff::Exponential);
        assert_eq!(
            policy.max_delay_ms,
            FLOW_STEP_MAX_RETRY_DELAY.as_millis() as u64
        );
        assert_eq!(policy.on_exhausted, StepFailureAction::ContinueWorkflow);
    }

    fn workflow_invocation(spec: WorkflowSpec) -> WorkflowInvocation {
        WorkflowInvocation::new("flow-retry-test", spec, json!({}), Vec::new())
    }
}
