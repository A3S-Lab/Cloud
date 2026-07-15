use super::{flow_error, DeploymentFlowConfig, DeploymentFlowRuntime};
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::shared_kernel::domain::{
    NodeCommandId, NodeId, OperationId, OrganizationId, WorkloadId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{DeploymentStatus, WorkloadDesiredState};
use crate::modules::workloads::infrastructure::project_runtime_spec;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload};
use a3s_flow::{FlowError, RuntimeCommand, StepInvocation, WorkflowContext, WorkflowInvocation};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeInspection, RuntimeUnitSpec, RuntimeUnitState,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const RESOLVE_STEP: &str = "stop-resolve";
const DISPATCH_STEP: &str = "stop-dispatch";
const COMPLETE_STEP: &str = "stop-complete";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopInput {
    operation_id: OperationId,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopChild {
    revision_id: WorkloadRevisionId,
    node_id: NodeId,
    spec: RuntimeUnitSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResolveOutput {
    operation_id: OperationId,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    child: Option<StopChild>,
    issued_at: DateTime<Utc>,
    stop_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DispatchInput {
    resolved: ResolveOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DispatchedStop {
    node_id: NodeId,
    command_id: NodeCommandId,
    result_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum DispatchOutput {
    NotRequired { stopped_at: DateTime<Utc> },
    Ready { dispatched: DispatchedStop },
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObserveInput {
    resolved: ResolveOutput,
    dispatched: DispatchedStop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum ObserveOutput {
    Pending {
        next_poll_at: DateTime<Utc>,
        deadline_at: DateTime<Utc>,
    },
    Ready {
        stopped_at: DateTime<Utc>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompleteInput {
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    stopped_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompleteOutput {
    workload_id: WorkloadId,
    stopped_at: DateTime<Utc>,
    operation_status: String,
}

pub(super) fn replay(
    config: &DeploymentFlowConfig,
    invocation: WorkflowInvocation,
) -> a3s_flow::Result<RuntimeCommand> {
    let context = invocation.context();
    let input = context.input_as::<StopInput>()?;
    let resolved = match context.step_output_as::<ResolveOutput>(RESOLVE_STEP)? {
        Some(output) => output,
        None => {
            return stage(
                config,
                &context,
                RESOLVE_STEP,
                "stop_workload_resolve",
                &input,
            )
        }
    };
    if resolved.operation_id != input.operation_id
        || resolved.organization_id != input.organization_id
        || resolved.workload_id != input.workload_id
    {
        return Err(FlowError::Runtime(
            "workload stop resolution changed its identity".into(),
        ));
    }
    let stopped_at = match context.step_output_as::<DispatchOutput>(DISPATCH_STEP)? {
        Some(DispatchOutput::NotRequired { stopped_at }) => stopped_at,
        Some(DispatchOutput::Failed { reason }) => return Ok(context.fail(reason)),
        Some(DispatchOutput::Ready { dispatched }) => {
            match observe(config, &context, &resolved, &dispatched)? {
                StopProgress::Ready(stopped_at) => stopped_at,
                StopProgress::Command(command) => return Ok(command),
                StopProgress::Failed(reason) => return Ok(context.fail(reason)),
            }
        }
        None => {
            return stage(
                config,
                &context,
                DISPATCH_STEP,
                "stop_workload_dispatch",
                &DispatchInput { resolved },
            )
        }
    };
    match context.step_output_as::<CompleteOutput>(COMPLETE_STEP)? {
        Some(output) => Ok(context.complete(serde_json::to_value(output)?)),
        None => stage(
            config,
            &context,
            COMPLETE_STEP,
            "stop_workload_complete",
            &CompleteInput {
                organization_id: input.organization_id,
                workload_id: input.workload_id,
                stopped_at,
            },
        ),
    }
}

fn observe(
    config: &DeploymentFlowConfig,
    context: &WorkflowContext<'_>,
    resolved: &ResolveOutput,
    dispatched: &DispatchedStop,
) -> a3s_flow::Result<StopProgress> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("stop-observe-{attempt}");
        match context.step_output_as::<ObserveOutput>(&step_id)? {
            Some(ObserveOutput::Ready { stopped_at }) => {
                return Ok(StopProgress::Ready(stopped_at))
            }
            Some(ObserveOutput::Failed { reason }) => return Ok(StopProgress::Failed(reason)),
            Some(ObserveOutput::Pending {
                next_poll_at,
                deadline_at,
            }) => {
                if next_poll_at > deadline_at {
                    return Err(FlowError::Runtime(
                        "workload stop poll exceeds its deadline".into(),
                    ));
                }
                let wait_id = format!("stop-observe-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(StopProgress::Command(
                        context.wait_until(wait_id, next_poll_at),
                    ));
                }
                attempt = attempt
                    .checked_add(1)
                    .ok_or_else(|| FlowError::Runtime("workload stop poll overflowed".into()))?;
            }
            None => {
                return stage(
                    config,
                    context,
                    &step_id,
                    "stop_workload_observe",
                    &ObserveInput {
                        resolved: resolved.clone(),
                        dispatched: dispatched.clone(),
                    },
                )
                .map(StopProgress::Command)
            }
        }
    }
}

fn stage<T: Serialize>(
    config: &DeploymentFlowConfig,
    context: &WorkflowContext<'_>,
    step_id: &str,
    step_name: &str,
    input: &T,
) -> a3s_flow::Result<RuntimeCommand> {
    if let Some(error) = context.step_failed(step_id) {
        return Ok(context.fail(format!("workload stop stage {step_name} failed: {error}")));
    }
    Ok(context.schedule_step_with_retry(
        step_id,
        step_name,
        serde_json::to_value(input)?,
        config.retry_policy(),
    ))
}

pub(super) async fn execute(
    runtime: &DeploymentFlowRuntime,
    invocation: StepInvocation,
) -> a3s_flow::Result<serde_json::Value> {
    match invocation.step_name.as_str() {
        "stop_workload_resolve" => {
            encode(resolve(runtime, &invocation.run_id, invocation.input_as()?).await?)
        }
        "stop_workload_dispatch" => encode(dispatch(runtime, invocation.input_as()?).await?),
        "stop_workload_observe" => encode(observe_step(runtime, invocation.input_as()?).await?),
        "stop_workload_complete" => encode(complete(runtime, invocation.input_as()?).await?),
        step => Err(FlowError::Runtime(format!(
            "Cloud workload stop workflow has no step {step:?}"
        ))),
    }
}

async fn resolve(
    runtime: &DeploymentFlowRuntime,
    run_id: &str,
    input: StopInput,
) -> a3s_flow::Result<ResolveOutput> {
    let operation_id = OperationId::from_uuid(
        uuid::Uuid::parse_str(run_id)
            .map_err(|error| FlowError::Runtime(format!("invalid stop Flow run ID: {error}")))?,
    );
    if operation_id != input.operation_id {
        return Err(FlowError::Runtime(
            "workload stop Flow run does not match its operation".into(),
        ));
    }
    let workload = runtime
        .workloads
        .find_workload(input.organization_id, input.workload_id)
        .await
        .map_err(|error| flow_error("could not load workload for stop", error))?;
    if workload.desired_state != WorkloadDesiredState::Stopped {
        return Err(FlowError::Runtime(
            "workload stop Flow has no persisted stop intent".into(),
        ));
    }
    let stop_deadline = input
        .requested_at
        .checked_add_signed(runtime.config.cleanup_timeout)
        .ok_or_else(|| FlowError::Runtime("workload stop deadline overflowed".into()))?;
    let child = match workload.active_revision_id {
        None => None,
        Some(revision_id) => {
            let revision = runtime
                .workloads
                .find_revision(input.organization_id, revision_id)
                .await
                .map_err(|error| flow_error("could not load active revision for stop", error))?;
            let deployment = runtime
                .workloads
                .list_deployments(input.organization_id, workload.id)
                .await
                .map_err(|error| flow_error("could not load active deployment for stop", error))?
                .into_iter()
                .find(|deployment| {
                    deployment.revision_id == revision_id
                        && deployment.status == DeploymentStatus::Active
                })
                .ok_or_else(|| {
                    FlowError::Runtime("active workload revision has no active deployment".into())
                })?;
            Some(StopChild {
                revision_id,
                node_id: deployment.node_id.ok_or_else(|| {
                    FlowError::Runtime("active deployment omitted its node".into())
                })?,
                spec: project_runtime_spec(&revision)
                    .map_err(|error| flow_error("could not project active Runtime spec", error))?,
            })
        }
    };
    Ok(ResolveOutput {
        operation_id,
        organization_id: input.organization_id,
        workload_id: input.workload_id,
        child,
        issued_at: input.requested_at,
        stop_deadline,
    })
}

async fn dispatch(
    runtime: &DeploymentFlowRuntime,
    input: DispatchInput,
) -> a3s_flow::Result<DispatchOutput> {
    let Some(child) = &input.resolved.child else {
        return Ok(DispatchOutput::NotRequired {
            stopped_at: Utc::now(),
        });
    };
    let now = Utc::now();
    if now >= input.resolved.stop_deadline {
        return Ok(DispatchOutput::Failed {
            reason: "workload stop deadline expired before dispatch".into(),
        });
    }
    let command_id = NodeCommandId::from_uuid(input.resolved.operation_id.as_uuid());
    let runtime_deadline = input
        .resolved
        .issued_at
        .checked_add_signed(runtime.config.runtime_stop_timeout)
        .ok_or_else(|| FlowError::Runtime("Runtime stop deadline overflowed".into()))?
        .min(input.resolved.stop_deadline);
    let not_after = input
        .resolved
        .issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("stop command deadline overflowed".into()))?
        .min(input.resolved.stop_deadline);
    let result_deadline = runtime_deadline.min(not_after);
    if now >= result_deadline {
        return Ok(DispatchOutput::Failed {
            reason: "workload stop command deadline expired before dispatch".into(),
        });
    }
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id: child.node_id,
            aggregate_id: input.resolved.workload_id.as_uuid(),
            payload: NodeCommandPayload::RuntimeStop {
                request: RuntimeActionRequest {
                    schema: RuntimeActionRequest::SCHEMA.into(),
                    request_id: format!("workload:{}:stop", input.resolved.workload_id),
                    unit_id: child.spec.unit_id.clone(),
                    generation: child.spec.generation,
                    deadline_at_ms: Some(timestamp_millis(runtime_deadline)?),
                },
            },
            issued_at: input.resolved.issued_at,
            not_after,
            correlation_id: input.resolved.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue workload Runtime stop", error))?
        .value;
    Ok(DispatchOutput::Ready {
        dispatched: DispatchedStop {
            node_id: command.node_id,
            command_id: command.id,
            result_deadline,
        },
    })
}

async fn observe_step(
    runtime: &DeploymentFlowRuntime,
    input: ObserveInput,
) -> a3s_flow::Result<ObserveOutput> {
    let child = input.resolved.child.as_ref().ok_or_else(|| {
        FlowError::Runtime("workload stop observation omitted its Runtime child".into())
    })?;
    if child.node_id != input.dispatched.node_id {
        return Err(FlowError::Runtime(
            "workload stop observation changed its node".into(),
        ));
    }
    if let Some(record) = runtime
        .node_control
        .latest_runtime_observation(child.node_id, &child.spec.unit_id, child.spec.generation)
        .await
        .map_err(|error| flow_error("could not load workload stop observation", error))?
    {
        if record.command_id == Some(input.dispatched.command_id)
            && record.observation.state == RuntimeUnitState::Stopped
        {
            return Ok(ObserveOutput::Ready {
                stopped_at: record.received_at,
            });
        }
    }
    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(child.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not load workload stop result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
                a3s_cloud_contracts::NodeCommandResult::RuntimeStopped {
                    inspection: RuntimeInspection::NotFound { .. },
                } => {
                    return Ok(ObserveOutput::Ready {
                        stopped_at: acknowledgement.completed_at,
                    })
                }
                a3s_cloud_contracts::NodeCommandResult::RuntimeStopped {
                    inspection: RuntimeInspection::Found { observation },
                } if observation.state == RuntimeUnitState::Stopped => {
                    return Ok(ObserveOutput::Ready {
                        stopped_at: acknowledgement.completed_at,
                    })
                }
                _ => {
                    return Ok(ObserveOutput::Failed {
                        reason: "Runtime stop completed without stopped or absent evidence".into(),
                    })
                }
            },
            NodeCommandOutcome::Rejected { failure }
                if matches!(failure.code.as_str(), "not_found" | "stale_generation") =>
            {
                return Ok(ObserveOutput::Ready {
                    stopped_at: acknowledgement.completed_at,
                })
            }
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                return Ok(ObserveOutput::Failed {
                    reason: format!("{}: {}", failure.code, failure.message),
                })
            }
        }
    }
    let now = Utc::now();
    if now >= input.dispatched.result_deadline || now >= input.resolved.stop_deadline {
        return Ok(ObserveOutput::Failed {
            reason: "Runtime stop did not produce durable evidence before its deadline".into(),
        });
    }
    let deadline_at = input
        .dispatched
        .result_deadline
        .min(input.resolved.stop_deadline);
    Ok(ObserveOutput::Pending {
        next_poll_at: now
            .checked_add_signed(runtime.config.cleanup_poll)
            .ok_or_else(|| FlowError::Runtime("workload stop poll overflowed".into()))?
            .min(deadline_at),
        deadline_at,
    })
}

async fn complete(
    runtime: &DeploymentFlowRuntime,
    input: CompleteInput,
) -> a3s_flow::Result<CompleteOutput> {
    let workload = runtime
        .workloads
        .find_workload(input.organization_id, input.workload_id)
        .await
        .map_err(|error| flow_error("could not load workload stop completion", error))?;
    let stopped = runtime
        .workloads
        .complete_workload_stop(
            input.organization_id,
            input.workload_id,
            workload.aggregate_version,
            input.stopped_at.max(workload.updated_at),
        )
        .await
        .map_err(|error| flow_error("could not persist workload stop completion", error))?;
    Ok(CompleteOutput {
        workload_id: stopped.id,
        stopped_at: stopped.updated_at,
        operation_status: "succeeded".into(),
    })
}

fn encode(value: impl Serialize) -> a3s_flow::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(Into::into)
}

fn timestamp_millis(value: DateTime<Utc>) -> a3s_flow::Result<u64> {
    u64::try_from(value.timestamp_millis())
        .map_err(|_| FlowError::Runtime("Runtime stop deadline is before the Unix epoch".into()))
}

enum StopProgress {
    Ready(DateTime<Utc>),
    Failed(String),
    Command(RuntimeCommand),
}
