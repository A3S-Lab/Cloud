use super::types::{
    AppliedAssignment, AuthorizeTrustInput, AuthorizeTrustOutput, AuthorizedTrust,
    AwaitConfirmationInput, AwaitConfirmationOutput, AlreadyConvergedAssignment, ConfirmedPlan,
    EnqueueApplyInput, EnqueueApplyOutput, EnqueuePlanInput, EnqueuePlanOutput, LockOutput,
    LockedPluginAssignment, ObserveInput, ObserveOutput, PlannedAssignment,
    PluginAssignmentFlowInput, ResolveHostInput, ResolveHostOutput, ResolvedHost, StorePlanInput,
    StorePlanOutput, StoredPlan,
};
use super::{
    PluginAssignmentFlowRuntime, PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST,
    PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION, PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
    PLUGIN_ASSIGNMENT_ENQUEUE_PLAN, PLUGIN_ASSIGNMENT_LOCK, PLUGIN_ASSIGNMENT_OBSERVE,
    PLUGIN_ASSIGNMENT_RESOLVE_HOST, PLUGIN_ASSIGNMENT_STORE_PLAN,
};
use crate::modules::plugins::application::{
    PLUGIN_ASSIGNMENT_WORKFLOW_NAME, PLUGIN_ASSIGNMENT_WORKFLOW_VERSION,
};
use a3s_flow::{FlowError, RuntimeCommand, WorkflowContext, WorkflowInvocation};
use chrono::{DateTime, Utc};
use serde::Serialize;

const LOCK_STEP_ID: &str = "lock";
const RESOLVE_HOST_STEP_ID: &str = "resolve-host";
const AUTHORIZE_TRUST_STEP_ID: &str = "authorize-trust";
const ENQUEUE_PLAN_STEP_ID: &str = "enqueue-plan";
const STORE_PLAN_STEP_ID: &str = "store-plan";
const AWAIT_CONFIRMATION_STEP_ID: &str = "await-confirmation";
const ENQUEUE_APPLY_STEP_ID: &str = "enqueue-apply";
const OBSERVE_STEP_ID: &str = "observe";

pub(super) fn replay(
    runtime: &PluginAssignmentFlowRuntime,
    invocation: WorkflowInvocation,
) -> a3s_flow::Result<RuntimeCommand> {
    if invocation.spec.name != PLUGIN_ASSIGNMENT_WORKFLOW_NAME
        || invocation.spec.version != PLUGIN_ASSIGNMENT_WORKFLOW_VERSION
    {
        return Err(FlowError::Runtime(format!(
            "Cloud has no plugin assignment workflow runtime for {}@{}",
            invocation.spec.name, invocation.spec.version
        )));
    }
    let context = invocation.context();
    let input = context.input_as::<PluginAssignmentFlowInput>()?;
    let locked = match lock_stage(runtime, &context, &input)? {
        Progress::Ready(locked) => locked,
        Progress::Terminal(reason) => return Ok(context.fail(reason)),
        Progress::Command(command) => return Ok(command),
    };
    let resolved = match resolve_host_stage(runtime, &context, locked)? {
        Progress::Ready(resolved) => resolved,
        Progress::Terminal(reason) => return Ok(context.fail(reason)),
        Progress::Command(command) => return Ok(command),
    };
    let authorized = match authorize_trust_stage(runtime, &context, resolved)? {
        Progress::Ready(authorized) => authorized,
        Progress::Terminal(reason) => return Ok(context.fail(reason)),
        Progress::Command(command) => return Ok(command),
    };
    let planned = match enqueue_plan_stage(runtime, &context, authorized)? {
        PlanProgress::Planned(planned) => planned,
        PlanProgress::AlreadyConverged(converged) => {
            return Ok(context.complete(serde_json::json!({
                "alreadyConverged": true,
                "observedDesired": converged.observed_desired,
                "observedState": converged.observed_state,
                "capabilityGeneration": converged.capability_generation,
                "packageGeneration": converged.package_generation,
            })));
        }
        PlanProgress::Terminal(reason) => return Ok(context.fail(reason)),
        PlanProgress::Command(command) => return Ok(command),
    };
    let stored = match store_plan_stage(runtime, &context, planned)? {
        Progress::Ready(stored) => stored,
        Progress::Terminal(reason) => return Ok(context.fail(reason)),
        Progress::Command(command) => return Ok(command),
    };
    let confirmed = match await_confirmation_stage(runtime, &context, stored)? {
        Progress::Ready(confirmed) => confirmed,
        Progress::Terminal(reason) => return Ok(context.fail(reason)),
        Progress::Command(command) => return Ok(command),
    };
    let applied = match enqueue_apply_stage(runtime, &context, confirmed)? {
        Progress::Ready(applied) => applied,
        Progress::Terminal(reason) => return Ok(context.fail(reason)),
        Progress::Command(command) => return Ok(command),
    };
    observe_stage(runtime, &context, applied)
}

fn lock_stage(
    runtime: &PluginAssignmentFlowRuntime,
    context: &WorkflowContext<'_>,
    input: &PluginAssignmentFlowInput,
) -> a3s_flow::Result<Progress<LockedPluginAssignment>> {
    match context.step_output_as::<LockOutput>(LOCK_STEP_ID)? {
        Some(LockOutput::Ready { locked }) => Ok(Progress::Ready(*locked)),
        Some(LockOutput::Terminal { reason }) => Ok(Progress::Terminal(reason)),
        None => stage(
            runtime,
            context,
            LOCK_STEP_ID,
            PLUGIN_ASSIGNMENT_LOCK,
            input,
        )
        .map(Progress::Command),
    }
}

fn resolve_host_stage(
    runtime: &PluginAssignmentFlowRuntime,
    context: &WorkflowContext<'_>,
    locked: LockedPluginAssignment,
) -> a3s_flow::Result<Progress<ResolvedHost>> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("{RESOLVE_HOST_STEP_ID}-{attempt}");
        match context.step_output_as::<ResolveHostOutput>(&step_id)? {
            Some(ResolveHostOutput::Ready { resolved }) => return Ok(Progress::Ready(*resolved)),
            Some(ResolveHostOutput::Terminal { reason }) => {
                return Ok(Progress::Terminal(reason))
            }
            Some(ResolveHostOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll("host resolution", next_poll_at, deadline_at)?;
                let wait_id = format!("resolve-host-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = next_attempt(attempt, "resolve-host")?;
            }
            None => {
                return stage(
                    runtime,
                    context,
                    &step_id,
                    PLUGIN_ASSIGNMENT_RESOLVE_HOST,
                    &ResolveHostInput {
                        locked: Box::new(locked),
                    },
                )
                .map(Progress::Command);
            }
        }
    }
}

fn authorize_trust_stage(
    runtime: &PluginAssignmentFlowRuntime,
    context: &WorkflowContext<'_>,
    resolved: ResolvedHost,
) -> a3s_flow::Result<Progress<AuthorizedTrust>> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("{AUTHORIZE_TRUST_STEP_ID}-{attempt}");
        match context.step_output_as::<AuthorizeTrustOutput>(&step_id)? {
            Some(AuthorizeTrustOutput::Ready { authorized }) => {
                return Ok(Progress::Ready(*authorized))
            }
            Some(AuthorizeTrustOutput::Terminal { reason }) => {
                return Ok(Progress::Terminal(reason))
            }
            Some(AuthorizeTrustOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll("trust authorization", next_poll_at, deadline_at)?;
                let wait_id = format!("authorize-trust-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = next_attempt(attempt, "authorize-trust")?;
            }
            None => {
                return stage(
                    runtime,
                    context,
                    &step_id,
                    PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST,
                    &AuthorizeTrustInput {
                        resolved: Box::new(resolved),
                    },
                )
                .map(Progress::Command);
            }
        }
    }
}

fn enqueue_plan_stage(
    runtime: &PluginAssignmentFlowRuntime,
    context: &WorkflowContext<'_>,
    authorized: AuthorizedTrust,
) -> a3s_flow::Result<PlanProgress> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("{ENQUEUE_PLAN_STEP_ID}-{attempt}");
        match context.step_output_as::<EnqueuePlanOutput>(&step_id)? {
            Some(EnqueuePlanOutput::Ready { planned }) => {
                return Ok(PlanProgress::Planned(*planned))
            }
            Some(EnqueuePlanOutput::AlreadyConverged { converged }) => {
                return Ok(PlanProgress::AlreadyConverged(*converged))
            }
            Some(EnqueuePlanOutput::Terminal { reason }) => {
                return Ok(PlanProgress::Terminal(reason))
            }
            Some(EnqueuePlanOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll("plan enqueue", next_poll_at, deadline_at)?;
                let wait_id = format!("enqueue-plan-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(PlanProgress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = next_attempt(attempt, "enqueue-plan")?;
            }
            None => {
                return stage(
                    runtime,
                    context,
                    &step_id,
                    PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
                    &EnqueuePlanInput {
                        authorized: Box::new(authorized),
                    },
                )
                .map(PlanProgress::Command);
            }
        }
    }
}

fn store_plan_stage(
    runtime: &PluginAssignmentFlowRuntime,
    context: &WorkflowContext<'_>,
    planned: PlannedAssignment,
) -> a3s_flow::Result<Progress<StoredPlan>> {
    match context.step_output_as::<StorePlanOutput>(STORE_PLAN_STEP_ID)? {
        Some(StorePlanOutput::Ready { stored }) => Ok(Progress::Ready(*stored)),
        Some(StorePlanOutput::Terminal { reason }) => Ok(Progress::Terminal(reason)),
        None => stage(
            runtime,
            context,
            STORE_PLAN_STEP_ID,
            PLUGIN_ASSIGNMENT_STORE_PLAN,
            &StorePlanInput {
                planned: Box::new(planned),
            },
        )
        .map(Progress::Command),
    }
}

fn await_confirmation_stage(
    runtime: &PluginAssignmentFlowRuntime,
    context: &WorkflowContext<'_>,
    stored: StoredPlan,
) -> a3s_flow::Result<Progress<ConfirmedPlan>> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("{AWAIT_CONFIRMATION_STEP_ID}-{attempt}");
        match context.step_output_as::<AwaitConfirmationOutput>(&step_id)? {
            Some(AwaitConfirmationOutput::Ready { confirmed }) => {
                return Ok(Progress::Ready(*confirmed))
            }
            Some(AwaitConfirmationOutput::Terminal { reason }) => {
                return Ok(Progress::Terminal(reason))
            }
            Some(AwaitConfirmationOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll("plan confirmation", next_poll_at, deadline_at)?;
                let wait_id = format!("await-confirmation-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = next_attempt(attempt, "await-confirmation")?;
            }
            None => {
                return stage(
                    runtime,
                    context,
                    &step_id,
                    PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
                    &AwaitConfirmationInput {
                        stored: Box::new(stored),
                    },
                )
                .map(Progress::Command);
            }
        }
    }
}

fn enqueue_apply_stage(
    runtime: &PluginAssignmentFlowRuntime,
    context: &WorkflowContext<'_>,
    confirmed: ConfirmedPlan,
) -> a3s_flow::Result<Progress<AppliedAssignment>> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("{ENQUEUE_APPLY_STEP_ID}-{attempt}");
        match context.step_output_as::<EnqueueApplyOutput>(&step_id)? {
            Some(EnqueueApplyOutput::Ready { applied }) => {
                return Ok(Progress::Ready(*applied));
            }
            Some(EnqueueApplyOutput::Terminal { reason }) => {
                return Ok(Progress::Terminal(reason))
            }
            Some(EnqueueApplyOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll("apply enqueue", next_poll_at, deadline_at)?;
                let wait_id = format!("enqueue-apply-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = next_attempt(attempt, "enqueue-apply")?;
            }
            None => {
                return stage(
                    runtime,
                    context,
                    &step_id,
                    PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
                    &EnqueueApplyInput {
                        confirmed: Box::new(confirmed),
                    },
                )
                .map(Progress::Command);
            }
        }
    }
}

fn observe_stage(
    runtime: &PluginAssignmentFlowRuntime,
    context: &WorkflowContext<'_>,
    applied: AppliedAssignment,
) -> a3s_flow::Result<RuntimeCommand> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("{OBSERVE_STEP_ID}-{attempt}");
        match context.step_output_as::<ObserveOutput>(&step_id)? {
            Some(ObserveOutput::Ready { observed }) => {
                return Ok(context.complete(serde_json::to_value(*observed)?));
            }
            Some(ObserveOutput::Terminal { reason }) => return Ok(context.fail(reason)),
            Some(ObserveOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll("observation", next_poll_at, deadline_at)?;
                let wait_id = format!("observe-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(context.wait_until(wait_id, next_poll_at));
                }
                attempt = next_attempt(attempt, "observe")?;
            }
            None => {
                return stage(
                    runtime,
                    context,
                    &step_id,
                    PLUGIN_ASSIGNMENT_OBSERVE,
                    &ObserveInput {
                        applied: Box::new(applied),
                        observation_attempt: attempt,
                    },
                );
            }
        }
    }
}

fn stage<T: Serialize>(
    runtime: &PluginAssignmentFlowRuntime,
    context: &WorkflowContext<'_>,
    step_id: &str,
    step_name: &str,
    input: &T,
) -> a3s_flow::Result<RuntimeCommand> {
    if let Some(error) = context.step_failed(step_id) {
        return Ok(context.fail(format!("plugin assignment stage {step_name} failed: {error}")));
    }
    Ok(context.schedule_step_with_retry(
        step_id,
        step_name,
        serde_json::to_value(input)?,
        runtime.retry_policy(context),
    ))
}

fn validate_poll(
    label: &str,
    next_poll_at: DateTime<Utc>,
    deadline_at: DateTime<Utc>,
) -> a3s_flow::Result<()> {
    if next_poll_at > deadline_at {
        return Err(FlowError::Runtime(format!(
            "plugin assignment {label} poll exceeds its deadline"
        )));
    }
    Ok(())
}

fn next_attempt(value: u32, label: &str) -> a3s_flow::Result<u32> {
    value
        .checked_add(1)
        .ok_or_else(|| FlowError::Runtime(format!("{label} attempt overflowed")))
}

enum Progress<T> {
    Ready(T),
    Terminal(String),
    Command(RuntimeCommand),
}

enum PlanProgress {
    Planned(PlannedAssignment),
    AlreadyConverged(AlreadyConvergedAssignment),
    Terminal(String),
    Command(RuntimeCommand),
}
