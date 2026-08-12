use super::DeploymentFlowConfig;
use super::DeploymentFlowRuntime;
use crate::modules::shared_kernel::domain::{
    DeploymentId, OrganizationId, WorkloadId, WorkloadPlacementGroupId, WorkloadReplicaId,
    WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{DeploymentStatus, PlacementTopology};
use crate::modules::workloads::infrastructure::replica_deployment_materialization::validate_existing_materialization;
use a3s_flow::{FlowError, RuntimeCommand, StepInvocation, WorkflowInvocation};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

const MINIMUM_PLACEMENT_POLL: Duration = Duration::seconds(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlacementGroupDeploymentInput {
    deployment_id: DeploymentId,
    group_id: WorkloadPlacementGroupId,
    group_plan_digest: String,
    member_count: u32,
    organization_id: OrganizationId,
    replica_generation: u64,
    replica_id: WorkloadReplicaId,
    revision_id: WorkloadRevisionId,
    workload_id: WorkloadId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ValidatePlacementGroupInput {
    deployment: PlacementGroupDeploymentInput,
    attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum ValidatePlacementGroupOutput {
    AwaitingPlacement {
        attempt: u32,
        group_plan_digest: String,
        member_count: u32,
        observed_at: DateTime<Utc>,
        next_poll_at: DateTime<Utc>,
    },
    Cancelled {
        cancelled_at: DateTime<Utc>,
    },
}

pub(super) fn replay(
    config: &DeploymentFlowConfig,
    invocation: WorkflowInvocation,
) -> a3s_flow::Result<RuntimeCommand> {
    let context = invocation.context();
    let input = context.input_as::<PlacementGroupDeploymentInput>()?;
    validate_input(&input)?;
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("placement-group-materialization-{attempt}");
        match context.step_output_as::<ValidatePlacementGroupOutput>(&step_id)? {
            Some(ValidatePlacementGroupOutput::Cancelled { cancelled_at }) => {
                return Ok(context.complete(serde_json::json!({
                    "deploymentId": input.deployment_id,
                    "state": "cancelled",
                    "cancelledAt": cancelled_at,
                })));
            }
            Some(ValidatePlacementGroupOutput::AwaitingPlacement {
                attempt: observed_attempt,
                group_plan_digest,
                member_count,
                observed_at,
                next_poll_at,
            }) => {
                if observed_attempt != attempt
                    || group_plan_digest != input.group_plan_digest
                    || member_count != input.member_count
                    || next_poll_at <= observed_at
                {
                    return Err(FlowError::Runtime(
                        "placement-group Deployment poll evidence is inconsistent".into(),
                    ));
                }
                let wait_id = format!("placement-group-materialization-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(context.wait_until(wait_id, next_poll_at));
                }
                attempt = attempt.checked_add(1).ok_or_else(|| {
                    FlowError::Runtime("placement-group Deployment poll overflowed".into())
                })?;
            }
            None => {
                if let Some(error) = context.step_failed(&step_id) {
                    return Err(FlowError::Runtime(format!(
                        "placement-group Deployment validation failed: {error}"
                    )));
                }
                return Ok(context.schedule_step_with_retry(
                    step_id,
                    "placement_group_deployment_validate_materialization",
                    serde_json::to_value(ValidatePlacementGroupInput {
                        deployment: input.clone(),
                        attempt,
                    })?,
                    config.retry_policy(),
                ));
            }
        }
    }
}

pub(super) async fn execute(
    runtime: &DeploymentFlowRuntime,
    invocation: StepInvocation,
) -> a3s_flow::Result<serde_json::Value> {
    match invocation.step_name.as_str() {
        "placement_group_deployment_validate_materialization" => {
            let input = invocation.input_as::<ValidatePlacementGroupInput>()?;
            serde_json::to_value(validate_materialization(runtime, input).await?)
                .map_err(Into::into)
        }
        step => Err(FlowError::Runtime(format!(
            "Cloud placement-group Deployment workflow has no step {step:?}"
        ))),
    }
}

async fn validate_materialization(
    runtime: &DeploymentFlowRuntime,
    input: ValidatePlacementGroupInput,
) -> a3s_flow::Result<ValidatePlacementGroupOutput> {
    validate_input(&input.deployment)?;
    if input.attempt == 0 {
        return Err(FlowError::Runtime(
            "placement-group Deployment validation attempt is invalid".into(),
        ));
    }
    let expected = input.deployment;
    let mut deployment = runtime
        .workloads
        .find_deployment(expected.organization_id, expected.deployment_id)
        .await
        .map_err(|error| runtime_error("could not load placement-group Deployment", error))?;
    if deployment.organization_id != expected.organization_id
        || deployment.workload_id != expected.workload_id
        || deployment.revision_id != expected.revision_id
    {
        return Err(FlowError::Runtime(
            "placement-group Deployment identity is inconsistent".into(),
        ));
    }
    if deployment.status == DeploymentStatus::Cancelling
        && deployment.node_id.is_none()
        && deployment.command_id.is_none()
        && deployment.cleanup_command_id.is_none()
    {
        let at = Utc::now().max(deployment.updated_at);
        deployment = runtime
            .workloads
            .cancel(deployment.id, deployment.aggregate_version, at)
            .await
            .map_err(|error| {
                runtime_error(
                    "could not cancel unplaced placement-group Deployment",
                    error,
                )
            })?;
        return Ok(ValidatePlacementGroupOutput::Cancelled {
            cancelled_at: deployment.cancelled_at.unwrap_or(at),
        });
    }
    if deployment.status == DeploymentStatus::Cancelled {
        return Ok(ValidatePlacementGroupOutput::Cancelled {
            cancelled_at: deployment.cancelled_at.unwrap_or(deployment.updated_at),
        });
    }
    if deployment.status != DeploymentStatus::Queued
        || deployment.node_id.is_some()
        || deployment.command_id.is_some()
        || deployment.cleanup_command_id.is_some()
        || deployment.retirement_command_id.is_some()
    {
        return Err(FlowError::Runtime(
            "unplaced placement-group Deployment changed state before group scheduling".into(),
        ));
    }
    let canonical_binding = runtime
        .workloads
        .find_deployment_replica_binding(expected.organization_id, expected.deployment_id)
        .await
        .map_err(|error| runtime_error("could not load Deployment leader binding", error))?;
    let member_bindings = runtime
        .workloads
        .list_deployment_replica_member_bindings(expected.organization_id, expected.deployment_id)
        .await
        .map_err(|error| runtime_error("could not load Deployment member bindings", error))?;
    let group_binding = runtime
        .workloads
        .find_deployment_placement_group_binding(expected.organization_id, expected.deployment_id)
        .await
        .map_err(|error| runtime_error("could not load Deployment placement group", error))?;
    validate_existing_materialization(
        &deployment,
        &canonical_binding,
        &member_bindings,
        Some(&group_binding),
        PlacementTopology::MultiNode,
    )
    .map_err(FlowError::Runtime)?;
    if group_binding.group_id != expected.group_id
        || group_binding.group_plan_digest != expected.group_plan_digest
        || group_binding.member_count != expected.member_count
        || group_binding.replica_id != expected.replica_id
        || group_binding.replica_generation != expected.replica_generation
        || member_bindings
            .iter()
            .any(|binding| binding.node_id.is_some())
    {
        return Err(FlowError::Runtime(
            "placement-group Deployment plan changed after materialization".into(),
        ));
    }
    let observed_at = Utc::now().max(deployment.updated_at);
    let poll_delay = runtime.config.observation_poll.max(MINIMUM_PLACEMENT_POLL);
    let next_poll_at = observed_at.checked_add_signed(poll_delay).ok_or_else(|| {
        FlowError::Runtime("placement-group Deployment poll deadline overflowed".into())
    })?;
    Ok(ValidatePlacementGroupOutput::AwaitingPlacement {
        attempt: input.attempt,
        group_plan_digest: expected.group_plan_digest,
        member_count: expected.member_count,
        observed_at,
        next_poll_at,
    })
}

fn validate_input(input: &PlacementGroupDeploymentInput) -> a3s_flow::Result<()> {
    if input.deployment_id.as_uuid().is_nil()
        || input.group_id.as_uuid().is_nil()
        || input.organization_id.as_uuid().is_nil()
        || input.replica_id.as_uuid().is_nil()
        || input.revision_id.as_uuid().is_nil()
        || input.workload_id.as_uuid().is_nil()
        || input.replica_generation == 0
        || !(2..=crate::modules::workloads::domain::entities::MAX_WORKLOAD_PLACEMENT_GROUP_MEMBERS)
            .contains(&input.member_count)
        || !is_sha256_digest(&input.group_plan_digest)
    {
        return Err(FlowError::Runtime(
            "placement-group Deployment input is invalid".into(),
        ));
    }
    Ok(())
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn runtime_error(context: &str, error: impl std::fmt::Display) -> FlowError {
    FlowError::Runtime(format!("{context}: {error}"))
}
