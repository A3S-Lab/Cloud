use super::types::{
    AwaitConfirmationInput, AwaitConfirmationOutput, ConfirmedPlan,
};
use super::PluginAssignmentFlowRuntime;
use a3s_flow::FlowError;
use a3s_use_core::PlanPolicyDecision;
use chrono::{Duration, Utc};

pub(super) async fn await_confirmation(
    runtime: &PluginAssignmentFlowRuntime,
    input: AwaitConfirmationInput,
) -> a3s_flow::Result<AwaitConfirmationOutput> {
    let stored = *input.stored;
    let locked = stored.planned.authorized.resolved.locked.as_ref();
    let assignment = runtime
        .assignments
        .find(locked.organization_id, locked.assignment_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "plugin assignment confirmation reload failed: {error}"
            ))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment disappeared before confirmation wait".into())
        })?;
    if assignment.organization_id != locked.organization_id
        || assignment.id != locked.assignment_id
        || assignment.current_operation_id != Some(locked.operation_id)
        || assignment.assignment_generation != locked.assignment_generation
        || assignment.registry_id != locked.registry_id
        || assignment.target_host_id != locked.target_host_id
    {
        return Ok(AwaitConfirmationOutput::Terminal {
            reason: "plugin assignment drifted before confirmation wait".into(),
        });
    }

    let projection = runtime
        .projections
        .find(locked.organization_id, stored.projection_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!("could not load plugin plan projection: {error}"))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("plugin plan projection disappeared before confirmation wait".into())
        })?;
    if projection.assignment_id != locked.assignment_id
        || projection.operation_id != locked.operation_id
        || projection.assignment_generation != locked.assignment_generation
        || projection.plan_digest.as_str() != stored.plan_digest
    {
        return Ok(AwaitConfirmationOutput::Terminal {
            reason: "plugin plan projection drifted before confirmation wait".into(),
        });
    }
    if let Some(reason) = &projection.terminal_reason {
        return Ok(AwaitConfirmationOutput::Terminal {
            reason: reason.clone(),
        });
    }

    let convergence_deadline = stored
        .planned
        .authorized
        .authorized_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment confirmation deadline overflowed".into())
        })?;
    let deadline_at = projection.expires_at.min(convergence_deadline);
    let now = Utc::now()
        .max(stored.planned.planned_at)
        .max(stored.stored_at);

    match projection.authority_decision {
        PlanPolicyDecision::Deny => {
            Ok(AwaitConfirmationOutput::Terminal {
                reason: "plugin plan projection was denied by policy".into(),
            })
        }
        PlanPolicyDecision::Allow => {
            Ok(AwaitConfirmationOutput::Ready {
                confirmed: Box::new(ConfirmedPlan {
                    stored: Box::new(stored),
                    confirmation_digest: None,
                    confirmation: None,
                    confirmed_at: now,
                }),
            })
        }
        PlanPolicyDecision::Ask => {
            if let Some(digest) = &projection.confirmation_digest {
                let Some(confirmation) = projection.confirmation.clone() else {
                    return Ok(AwaitConfirmationOutput::Terminal {
                        reason: "plugin plan confirmation envelope is missing".into(),
                    });
                };
                return Ok(AwaitConfirmationOutput::Ready {
                    confirmed: Box::new(ConfirmedPlan {
                        stored: Box::new(stored),
                        confirmation_digest: Some(digest.as_str().to_owned()),
                        confirmation: Some(confirmation),
                        confirmed_at: projection.updated_at.max(now),
                    }),
                });
            }
            if now >= deadline_at {
                return Ok(AwaitConfirmationOutput::Terminal {
                    reason: "plugin plan confirmation timed out".into(),
                });
            }
            pending(
                "waiting for digest-only plugin plan confirmation".into(),
                now,
                deadline_at,
                runtime.config.observation_poll,
            )
        }
    }
}

fn pending(
    reason: String,
    now: chrono::DateTime<Utc>,
    deadline_at: chrono::DateTime<Utc>,
    poll: Duration,
) -> a3s_flow::Result<AwaitConfirmationOutput> {
    let next_poll_at = now
        .checked_add_signed(poll)
        .ok_or_else(|| FlowError::Runtime("confirmation poll overflowed".into()))?
        .min(deadline_at);
    Ok(AwaitConfirmationOutput::Pending {
        reason,
        next_poll_at,
        deadline_at,
    })
}
