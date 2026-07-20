use super::{bounded_reason, next_poll, validate_resolved_deployment};
use crate::modules::workloads::domain::entities::DeploymentStatus;
use crate::modules::workloads::domain::services::{
    DeploymentRouteObservation, DeploymentRouteStage, DeploymentRouteUpdateRequest,
};
use crate::modules::workloads::infrastructure::deployment_flow::types::{
    ObserveGatewayStepInput, ObserveGatewayStepOutput, StageGatewayStepInput,
    StageGatewayStepOutput, VerifyStepOutput,
};
use crate::modules::workloads::infrastructure::deployment_flow::{
    flow_error, DeploymentFlowRuntime,
};
use a3s_flow::FlowError;
use chrono::Utc;

pub(super) async fn stage(
    runtime: &DeploymentFlowRuntime,
    input: StageGatewayStepInput,
) -> a3s_flow::Result<StageGatewayStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for Gateway staging", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if matches!(
        deployment.status,
        DeploymentStatus::Cancelling
            | DeploymentStatus::CleanupPending
            | DeploymentStatus::Cancelled
    ) {
        return Ok(StageGatewayStepOutput::CancellationRequested);
    }
    if deployment.status != DeploymentStatus::Verifying {
        return Err(FlowError::Runtime(format!(
            "deployment cannot stage Gateway routing from {}",
            deployment.status.as_str()
        )));
    }
    let VerifyStepOutput::Verified { verified_at } = input.verification else {
        return Ok(StageGatewayStepOutput::CancellationRequested);
    };
    let now = Utc::now().max(deployment.updated_at).max(verified_at);
    let Some(previous) = input.resolved.previous_runtime.as_ref() else {
        return Ok(StageGatewayStepOutput::NotRequired { gated_at: now });
    };
    if input.dispatched.node_id != previous.node_id
        || deployment.node_id != Some(previous.node_id)
        || deployment.command_id != Some(input.dispatched.command_id)
    {
        return Err(FlowError::Runtime(
            "one-node Gateway update does not match its Runtime dispatch".into(),
        ));
    }
    let workload = runtime
        .workloads
        .find_workload(input.resolved.organization_id, input.resolved.workload_id)
        .await
        .map_err(|error| flow_error("could not load workload for Gateway staging", error))?;
    if workload.active_revision_id != Some(previous.revision_id) {
        return Err(FlowError::Runtime(
            "previous workload revision changed before Gateway staging".into(),
        ));
    }
    let request = DeploymentRouteUpdateRequest {
        deployment_id: deployment.id,
        operation_id: deployment.operation_id,
        organization_id: deployment.organization_id,
        project_id: workload.project_id,
        environment_id: workload.environment_id,
        workload_id: deployment.workload_id,
        previous_revision_id: previous.revision_id,
        candidate_revision_id: deployment.revision_id,
        node_id: previous.node_id,
        runtime_command_id: input.dispatched.command_id,
        spec: input.resolved.spec,
        verified_at,
        convergence_deadline: input.resolved.convergence_deadline,
    };
    match runtime
        .route_updates
        .stage(&request, now)
        .await
        .map_err(|error| flow_error("could not stage deployment Gateway update", error))?
    {
        DeploymentRouteStage::NotRequired { checked_at } => {
            Ok(StageGatewayStepOutput::NotRequired {
                gated_at: checked_at,
            })
        }
        DeploymentRouteStage::Staged { publication } => {
            Ok(StageGatewayStepOutput::Ready { publication })
        }
        DeploymentRouteStage::Failed { reason } => Ok(StageGatewayStepOutput::Failed {
            reason: bounded_reason(reason),
        }),
        DeploymentRouteStage::Blocked { reason } => {
            if now >= request.convergence_deadline {
                return Ok(StageGatewayStepOutput::Failed {
                    reason: bounded_reason(reason),
                });
            }
            Ok(StageGatewayStepOutput::Pending {
                reason: bounded_reason(reason),
                next_poll_at: next_poll(
                    now,
                    runtime.config.observation_poll,
                    request.convergence_deadline,
                )?,
                deadline_at: request.convergence_deadline,
            })
        }
    }
}

pub(super) async fn observe(
    runtime: &DeploymentFlowRuntime,
    input: ObserveGatewayStepInput,
) -> a3s_flow::Result<ObserveGatewayStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| {
            flow_error(
                "could not load deployment for Gateway acknowledgement",
                error,
            )
        })?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if matches!(
        deployment.status,
        DeploymentStatus::Cancelling
            | DeploymentStatus::CleanupPending
            | DeploymentStatus::Cancelled
    ) {
        return Ok(ObserveGatewayStepOutput::CancellationRequested);
    }
    if deployment.status != DeploymentStatus::Verifying
        || input.publication.deployment_id != deployment.id
        || deployment.node_id != Some(input.publication.node_id)
    {
        return Err(FlowError::Runtime(
            "Gateway acknowledgement does not match the verifying deployment".into(),
        ));
    }
    let now = Utc::now().max(deployment.updated_at);
    let deadline = input
        .resolved
        .convergence_deadline
        .min(input.publication.command_not_after);
    match runtime
        .route_updates
        .observe(deployment.organization_id, &input.publication, now)
        .await
        .map_err(|error| flow_error("could not observe deployment Gateway update", error))?
    {
        DeploymentRouteObservation::Applied { acknowledged_at } => {
            Ok(ObserveGatewayStepOutput::Ready { acknowledged_at })
        }
        DeploymentRouteObservation::Rejected { reason, .. } => {
            Ok(ObserveGatewayStepOutput::Failed {
                reason: bounded_reason(format!("Gateway rejected the candidate route: {reason}")),
            })
        }
        DeploymentRouteObservation::Expired => Ok(ObserveGatewayStepOutput::Failed {
            reason: "Gateway did not acknowledge the candidate route before its deadline".into(),
        }),
        DeploymentRouteObservation::Pending if now >= deadline => {
            Ok(ObserveGatewayStepOutput::Failed {
                reason: "Gateway did not acknowledge the candidate route before its deadline"
                    .into(),
            })
        }
        DeploymentRouteObservation::Pending => Ok(ObserveGatewayStepOutput::Pending {
            reason: "waiting for the exact Gateway revision and snapshot acknowledgement".into(),
            next_poll_at: next_poll(now, runtime.config.observation_poll, deadline)?,
            deadline_at: deadline,
        }),
    }
}
