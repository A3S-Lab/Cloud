use a3s_cloud_contracts::{
    AutomationConcurrencyModeV1, AutomationConcurrencyPolicyV1, AUTOMATION_MAX_CONCURRENCY,
};

/// The bounded action a future scheduler owner may take for one candidate.
///
/// `Replace` is only a decision. It does not cancel an existing invocation;
/// cancellation and durable state transitions remain with their owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationConcurrencyDecision {
    Admit,
    Queue,
    Drop,
    Replace,
}

/// Stateless evaluator for one exact active-invocation count.
///
/// The caller supplies a transactionally observed count. This component only
/// applies the immutable policy and does not own counters, leases, queues,
/// cancellation, persistence, or workers.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutomationConcurrencyEvaluator;

impl AutomationConcurrencyEvaluator {
    /// Decide whether a candidate may enter an automation's concurrency set.
    ///
    /// A count below the maximum always admits. At the exact maximum, the
    /// policy mode chooses queue, drop, or replace. Counts above the policy
    /// maximum fail closed because they indicate stale or inconsistent owner
    /// state that this pure evaluator cannot repair.
    pub fn decide(
        policy: &AutomationConcurrencyPolicyV1,
        active_count: u64,
    ) -> Result<AutomationConcurrencyDecision, String> {
        if policy.maximum == 0 || policy.maximum > AUTOMATION_MAX_CONCURRENCY {
            return Err("Automation maximum concurrency is outside its closed bound".into());
        }
        if active_count > policy.maximum {
            return Err("Automation active invocation count exceeds its policy maximum".into());
        }
        if active_count < policy.maximum {
            return Ok(AutomationConcurrencyDecision::Admit);
        }

        Ok(match policy.mode {
            AutomationConcurrencyModeV1::Queue => AutomationConcurrencyDecision::Queue,
            AutomationConcurrencyModeV1::Drop => AutomationConcurrencyDecision::Drop,
            AutomationConcurrencyModeV1::Replace => AutomationConcurrencyDecision::Replace,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(mode: AutomationConcurrencyModeV1, maximum: u64) -> AutomationConcurrencyPolicyV1 {
        AutomationConcurrencyPolicyV1 { maximum, mode }
    }

    #[test]
    fn admits_below_capacity_independent_of_mode() {
        for mode in [
            AutomationConcurrencyModeV1::Queue,
            AutomationConcurrencyModeV1::Drop,
            AutomationConcurrencyModeV1::Replace,
        ] {
            assert_eq!(
                AutomationConcurrencyEvaluator::decide(&policy(mode, 3), 2)
                    .expect("admission decision"),
                AutomationConcurrencyDecision::Admit
            );
        }
    }

    #[test]
    fn applies_mode_at_exact_capacity() {
        assert_eq!(
            AutomationConcurrencyEvaluator::decide(
                &policy(AutomationConcurrencyModeV1::Queue, 2),
                2,
            )
            .expect("queue decision"),
            AutomationConcurrencyDecision::Queue
        );
        assert_eq!(
            AutomationConcurrencyEvaluator::decide(
                &policy(AutomationConcurrencyModeV1::Drop, 2),
                2,
            )
            .expect("drop decision"),
            AutomationConcurrencyDecision::Drop
        );
        assert_eq!(
            AutomationConcurrencyEvaluator::decide(
                &policy(AutomationConcurrencyModeV1::Replace, 2),
                2,
            )
            .expect("replace decision"),
            AutomationConcurrencyDecision::Replace
        );
    }

    #[test]
    fn rejects_invalid_policy_and_over_capacity_owner_state() {
        assert!(AutomationConcurrencyEvaluator::decide(
            &policy(AutomationConcurrencyModeV1::Queue, 0),
            0,
        )
        .is_err());
        assert!(AutomationConcurrencyEvaluator::decide(
            &policy(
                AutomationConcurrencyModeV1::Queue,
                AUTOMATION_MAX_CONCURRENCY + 1
            ),
            0,
        )
        .is_err());
        assert!(AutomationConcurrencyEvaluator::decide(
            &policy(AutomationConcurrencyModeV1::Queue, 2),
            3,
        )
        .is_err());
    }
}
