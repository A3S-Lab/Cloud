mod cleanup;
mod gateway;
mod retirement;

use super::flow_error;
use super::types::{
    ActivateStepInput, ActivateStepOutput, DeploymentFlowInput, DispatchStepInput,
    DispatchStepOutput, DispatchedRuntime, FailStepInput, FailStepOutput, ObserveStepInput,
    ObserveStepOutput, PreviousRuntime, ResolveCancellationOutput, ResolveStepOutput,
    ResolveStepResult, RouteGate, ScheduleStepInput, ScheduleStepOutput, VerifyStepInput,
    VerifyStepOutput,
};
use super::DeploymentFlowRuntime;
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::shared_kernel::domain::{NodeCommandId, OperationId};
use crate::modules::workloads::domain::entities::{
    DeploymentStatus, SecretBindingTarget, WorkloadRevision,
};
use crate::modules::workloads::domain::services::OciRegistryCredentialReference;
use crate::modules::workloads::infrastructure::project_runtime_spec;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload};
use a3s_flow::{FlowError, StepInvocation};
use a3s_runtime::contract::{
    RuntimeApplyRequest, RuntimeCapabilities, RuntimeHealthState, RuntimeUnitState,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub(super) async fn execute(
    runtime: &DeploymentFlowRuntime,
    invocation: StepInvocation,
) -> a3s_flow::Result<serde_json::Value> {
    match invocation.step_name.as_str() {
        "resolve_deployment" => {
            encode(resolve(runtime, &invocation.run_id, invocation.input_as()?).await?)
        }
        "schedule_deployment" => encode(schedule(runtime, invocation.input_as()?).await?),
        "dispatch_runtime_apply" => encode(dispatch(runtime, invocation.input_as()?).await?),
        "observe_runtime_apply" => encode(observe(runtime, invocation.input_as()?).await?),
        "verify_runtime_health" => encode(verify(runtime, invocation.input_as()?).await?),
        "stage_deployment_gateway" => {
            encode(gateway::stage(runtime, invocation.input_as()?).await?)
        }
        "observe_deployment_gateway" => {
            encode(gateway::observe(runtime, invocation.input_as()?).await?)
        }
        "activate_deployment" => encode(activate(runtime, invocation.input_as()?).await?),
        "dispatch_previous_runtime_retirement" => {
            encode(retirement::dispatch(runtime, invocation.input_as()?).await?)
        }
        "observe_previous_runtime_retirement" => {
            encode(retirement::observe(runtime, invocation.input_as()?).await?)
        }
        "complete_deployment_retirement" => {
            encode(retirement::complete(runtime, invocation.input_as()?).await?)
        }
        "dispatch_runtime_cleanup" => {
            encode(cleanup::dispatch_cleanup(runtime, invocation.input_as()?).await?)
        }
        "observe_runtime_cleanup" => {
            encode(cleanup::observe_cleanup(runtime, invocation.input_as()?).await?)
        }
        "complete_deployment_cancellation" => {
            encode(cleanup::complete_cancellation(runtime, invocation.input_as()?).await?)
        }
        "fail_deployment" => encode(fail(runtime, invocation.input_as()?).await?),
        step => Err(FlowError::Runtime(format!(
            "Cloud deployment workflow has no step {step:?}"
        ))),
    }
}

async fn resolve(
    runtime: &DeploymentFlowRuntime,
    run_id: &str,
    input: DeploymentFlowInput,
) -> a3s_flow::Result<ResolveStepResult> {
    let mut deployment = runtime
        .workloads
        .find_deployment(input.organization_id, input.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment", error))?;
    let run_operation_id = operation_id_from_run(run_id)?;
    validate_flow_identity(&input, &deployment)?;
    if deployment.operation_id != run_operation_id {
        return Err(FlowError::Runtime(
            "deployment Flow run does not match the persisted operation".into(),
        ));
    }
    if deployment.status == DeploymentStatus::Cancelling
        && deployment.node_id.is_none()
        && deployment.command_id.is_none()
        && deployment.cleanup_command_id.is_none()
    {
        return Ok(ResolveStepResult::CancellationRequested(
            ResolveCancellationOutput {
                cleaned_at: Utc::now().max(deployment.updated_at),
            },
        ));
    }
    match deployment.status {
        DeploymentStatus::Queued => {
            let transitioned_at = Utc::now().max(deployment.updated_at);
            deployment = runtime
                .workloads
                .mark_resolving(deployment.id, deployment.aggregate_version, transitioned_at)
                .await
                .map_err(|error| flow_error("could not mark deployment resolving", error))?;
        }
        DeploymentStatus::Resolving
        | DeploymentStatus::Scheduled
        | DeploymentStatus::Applying
        | DeploymentStatus::Verifying
        | DeploymentStatus::Retiring
        | DeploymentStatus::Cancelling
        | DeploymentStatus::CleanupPending
        | DeploymentStatus::Active => {}
        DeploymentStatus::Failed | DeploymentStatus::Orphaned | DeploymentStatus::Cancelled => {
            return Err(FlowError::Runtime(format!(
                "deployment {} is already {}",
                deployment.id,
                deployment.status.as_str()
            )))
        }
    }
    let mut revision = runtime
        .workloads
        .find_revision(input.organization_id, input.revision_id)
        .await
        .map_err(|error| flow_error("could not load workload revision", error))?;
    validate_revision_identity(&input, &revision)?;
    if revision.template.is_none() {
        let registry_credential = registry_credential_reference(runtime, &input, &revision).await?;
        let artifact = runtime
            .artifacts
            .resolve(&revision.request.artifact, registry_credential.as_ref())
            .await
            .map_err(|error| flow_error("could not resolve OCI artifact", error))?;
        revision = runtime
            .workloads
            .resolve_revision(
                input.organization_id,
                revision.id,
                artifact,
                Utc::now().max(revision.created_at),
            )
            .await
            .map_err(|error| flow_error("could not persist resolved OCI artifact", error))?;
    }
    validate_rollback_source(runtime, &input, &revision).await?;
    let spec = project_runtime_spec(&revision)
        .map_err(|error| flow_error("could not project Runtime specification", error))?;
    let previous_runtime = previous_runtime(runtime, &input, &revision).await?;

    let convergence_deadline = deployment
        .requested_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| FlowError::Runtime("deployment convergence deadline overflowed".into()))?;
    Ok(ResolveStepResult::Resolved(Box::new(ResolveStepOutput {
        deployment_id: deployment.id,
        organization_id: deployment.organization_id,
        revision_id: deployment.revision_id,
        workload_id: deployment.workload_id,
        spec,
        convergence_deadline,
        previous_runtime,
    })))
}

async fn validate_rollback_source(
    runtime: &DeploymentFlowRuntime,
    input: &DeploymentFlowInput,
    candidate: &WorkloadRevision,
) -> a3s_flow::Result<()> {
    let Some(source_revision_id) = input.rollback_source_revision_id else {
        return Ok(());
    };
    if source_revision_id == candidate.id {
        return Err(FlowError::Runtime(
            "rollback source cannot be the candidate revision".into(),
        ));
    }
    let source = runtime
        .workloads
        .find_revision(input.organization_id, source_revision_id)
        .await
        .map_err(|error| flow_error("could not load rollback source revision", error))?;
    if source.workload_id != candidate.workload_id
        || source.generation >= candidate.generation
        || source.template != candidate.template
        || source.template_digest != candidate.template_digest
    {
        return Err(FlowError::Runtime(
            "rollback candidate does not clone its declared source revision".into(),
        ));
    }
    let deployments = runtime
        .workloads
        .list_deployments(input.organization_id, candidate.workload_id)
        .await
        .map_err(|error| flow_error("could not load rollback source deployment", error))?;
    if !deployments.iter().any(|deployment| {
        deployment.revision_id == source.id
            && deployment.status == DeploymentStatus::Active
            && deployment.activated_at.is_some()
    }) {
        return Err(FlowError::Runtime(
            "rollback source revision was never activated successfully".into(),
        ));
    }
    Ok(())
}

async fn previous_runtime(
    runtime: &DeploymentFlowRuntime,
    input: &DeploymentFlowInput,
    candidate: &WorkloadRevision,
) -> a3s_flow::Result<Option<PreviousRuntime>> {
    let workload = runtime
        .workloads
        .find_workload(input.organization_id, input.workload_id)
        .await
        .map_err(|error| flow_error("could not load deployment workload", error))?;
    if workload.id != candidate.workload_id || workload.organization_id != input.organization_id {
        return Err(FlowError::Runtime(
            "deployment workload does not own its candidate revision".into(),
        ));
    }
    let Some(previous_revision_id) = workload.active_revision_id else {
        return Ok(None);
    };
    if previous_revision_id == candidate.id {
        return Err(FlowError::Runtime(
            "deployment candidate is already the active immutable revision".into(),
        ));
    }
    let previous_revision = runtime
        .workloads
        .find_revision(input.organization_id, previous_revision_id)
        .await
        .map_err(|error| flow_error("could not load previous workload revision", error))?;
    if previous_revision.workload_id != workload.id
        || previous_revision.generation >= candidate.generation
    {
        return Err(FlowError::Runtime(
            "previous workload revision is inconsistent with the update generation".into(),
        ));
    }
    let deployments = runtime
        .workloads
        .list_deployments(input.organization_id, workload.id)
        .await
        .map_err(|error| flow_error("could not load previous deployment", error))?;
    let previous_deployment = deployments
        .into_iter()
        .find(|deployment| {
            deployment.revision_id == previous_revision.id
                && deployment.status == DeploymentStatus::Active
        })
        .ok_or_else(|| {
            FlowError::Runtime("active workload revision has no active deployment".into())
        })?;
    let node_id = previous_deployment
        .node_id
        .ok_or_else(|| FlowError::Runtime("active deployment omitted its node".into()))?;
    let spec = project_runtime_spec(&previous_revision)
        .map_err(|error| flow_error("could not project previous Runtime specification", error))?;
    Ok(Some(PreviousRuntime {
        revision_id: previous_revision.id,
        node_id,
        spec,
    }))
}

async fn registry_credential_reference(
    runtime: &DeploymentFlowRuntime,
    input: &DeploymentFlowInput,
    revision: &WorkloadRevision,
) -> a3s_flow::Result<Option<OciRegistryCredentialReference>> {
    let Some(binding) = revision
        .request
        .secrets
        .iter()
        .find(|binding| matches!(binding.target, SecretBindingTarget::RegistryCredential))
    else {
        return Ok(None);
    };
    let workload = runtime
        .workloads
        .find_workload(input.organization_id, input.workload_id)
        .await
        .map_err(|error| {
            flow_error(
                "could not load workload for OCI registry authentication",
                error,
            )
        })?;
    if workload.id != revision.workload_id || workload.organization_id != input.organization_id {
        return Err(FlowError::Runtime(
            "OCI registry credential does not belong to the deployment workload".into(),
        ));
    }
    let reference = OciRegistryCredentialReference {
        organization_id: input.organization_id,
        project_id: workload.project_id,
        environment_id: workload.environment_id,
        secret_id: binding.secret_id,
        version: binding.version,
    };
    reference
        .validate()
        .map_err(|error| flow_error("could not bind OCI registry credential", error))?;
    Ok(Some(reference))
}

async fn schedule(
    runtime: &DeploymentFlowRuntime,
    input: ScheduleStepInput,
) -> a3s_flow::Result<ScheduleStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for scheduling", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if matches!(
        deployment.status,
        DeploymentStatus::Cancelling
            | DeploymentStatus::CleanupPending
            | DeploymentStatus::Cancelled
    ) {
        return Ok(ScheduleStepOutput::CancellationRequested);
    }
    if let Some(node_id) = deployment.node_id {
        if matches!(
            deployment.status,
            DeploymentStatus::Scheduled
                | DeploymentStatus::Applying
                | DeploymentStatus::Verifying
                | DeploymentStatus::Retiring
                | DeploymentStatus::Active
        ) {
            if input
                .resolved
                .previous_runtime
                .as_ref()
                .is_some_and(|previous| previous.node_id != node_id)
            {
                return Err(FlowError::Runtime(
                    "one-node update changed the previous Runtime node".into(),
                ));
            }
            return Ok(ScheduleStepOutput::Ready { node_id });
        }
    }
    if matches!(
        deployment.status,
        DeploymentStatus::Failed | DeploymentStatus::Orphaned
    ) {
        return Ok(ScheduleStepOutput::Failed {
            reason: deployment
                .failure
                .unwrap_or_else(|| "deployment failed before scheduling".into()),
        });
    }
    if deployment.status != DeploymentStatus::Resolving {
        return Err(FlowError::Runtime(format!(
            "deployment cannot schedule from {}",
            deployment.status.as_str()
        )));
    }

    let now = Utc::now().max(deployment.updated_at);
    let mut nodes = runtime
        .nodes
        .list(deployment.organization_id)
        .await
        .map_err(|error| flow_error("could not list deployment nodes", error))?;
    nodes.sort_by_key(|node| node.id);
    for node in nodes {
        if input
            .resolved
            .previous_runtime
            .as_ref()
            .is_some_and(|previous| previous.node_id != node.id)
        {
            continue;
        }
        if !node.accepts_new_work_at(now, runtime.heartbeat_timeout) {
            continue;
        }
        let capabilities = match serde_json::from_value::<RuntimeCapabilities>(
            node.capabilities.document().clone(),
        ) {
            Ok(capabilities) => capabilities,
            Err(error) => {
                tracing::warn!(node_id = %node.id, error = %error, "ignoring invalid Runtime capabilities during scheduling");
                continue;
            }
        };
        let missing = capabilities
            .missing_for(&input.resolved.spec)
            .map_err(|error| flow_error("could not match Runtime capabilities", error))?;
        if !missing.is_empty() {
            continue;
        }
        let scheduled = runtime
            .workloads
            .assign_node(deployment.id, deployment.aggregate_version, node.id, now)
            .await
            .map_err(|error| flow_error("could not assign deployment node", error))?;
        return Ok(ScheduleStepOutput::Ready {
            node_id: scheduled.node_id.ok_or_else(|| {
                FlowError::Runtime("scheduled deployment omitted its node".into())
            })?,
        });
    }

    if now >= input.resolved.convergence_deadline {
        return Ok(ScheduleStepOutput::Failed {
            reason: "no eligible node became available before the convergence deadline".into(),
        });
    }
    Ok(ScheduleStepOutput::Pending {
        reason: if input.resolved.previous_runtime.is_some() {
            "the previous Runtime node is not ready for a one-node update".into()
        } else {
            "no ready node satisfies the Runtime specification".into()
        },
        next_poll_at: next_poll(
            now,
            runtime.config.observation_poll,
            input.resolved.convergence_deadline,
        )?,
        deadline_at: input.resolved.convergence_deadline,
    })
}

async fn dispatch(
    runtime: &DeploymentFlowRuntime,
    input: DispatchStepInput,
) -> a3s_flow::Result<DispatchStepOutput> {
    let mut deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for dispatch", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if matches!(
        deployment.status,
        DeploymentStatus::Cancelling
            | DeploymentStatus::CleanupPending
            | DeploymentStatus::Cancelled
    ) {
        return Ok(DispatchStepOutput::CancellationRequested);
    }
    if deployment.node_id != Some(input.node_id) {
        return Err(FlowError::Runtime(
            "deployment dispatch does not match its scheduled node".into(),
        ));
    }
    if let Some(command_id) = deployment.command_id {
        if matches!(
            deployment.status,
            DeploymentStatus::Applying
                | DeploymentStatus::Verifying
                | DeploymentStatus::Retiring
                | DeploymentStatus::Active
        ) {
            let command = runtime
                .node_control
                .find_command(input.node_id, command_id)
                .await
                .map_err(|error| flow_error("could not reload Runtime apply command", error))?
                .ok_or_else(|| {
                    FlowError::Runtime("dispatched Runtime apply command is missing".into())
                })?;
            let result_deadline = apply_result_deadline(&command, &input.resolved.spec)?;
            return Ok(DispatchStepOutput::Ready {
                dispatched: DispatchedRuntime {
                    node_id: input.node_id,
                    command_id,
                    result_deadline,
                },
            });
        }
    }
    if deployment.status != DeploymentStatus::Scheduled {
        return Err(FlowError::Runtime(format!(
            "deployment cannot dispatch from {}",
            deployment.status.as_str()
        )));
    }

    // The schedule transition is the durable issuance clock. Re-execution after
    // command insertion therefore rebuilds byte-identical command input.
    let issued_at = deployment.updated_at;
    let not_after = issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("node command deadline overflowed".into()))?;
    let runtime_deadline = issued_at
        .checked_add_signed(runtime.config.runtime_apply_timeout)
        .ok_or_else(|| FlowError::Runtime("Runtime apply deadline overflowed".into()))?;
    let now = Utc::now();
    if now >= runtime_deadline || now >= not_after {
        return Ok(DispatchStepOutput::Failed {
            reason: "Runtime apply deadline expired before the command could be dispatched".into(),
        });
    }
    let command_id = NodeCommandId::from_uuid(deployment.id.as_uuid());
    let payload = NodeCommandPayload::RuntimeApply {
        request: Box::new(RuntimeApplyRequest {
            schema: RuntimeApplyRequest::SCHEMA.into(),
            request_id: format!("deployment:{}:apply", deployment.id),
            deadline_at_ms: Some(timestamp_millis(runtime_deadline)?),
            spec: input.resolved.spec.clone(),
        }),
    };
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id: input.node_id,
            aggregate_id: deployment.workload_id.as_uuid(),
            payload,
            issued_at,
            not_after,
            correlation_id: deployment.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue Runtime apply", error))?
        .value;
    if command.id != command_id || command.node_id != input.node_id {
        return Err(FlowError::Runtime(
            "node command repository changed the deployment command identity".into(),
        ));
    }
    let result_deadline = apply_result_deadline(&command, &input.resolved.spec)?;
    deployment = runtime
        .workloads
        .mark_dispatched(
            deployment.id,
            deployment.aggregate_version,
            command.id,
            Utc::now().max(deployment.updated_at),
        )
        .await
        .map_err(|error| flow_error("could not mark deployment dispatched", error))?;
    Ok(DispatchStepOutput::Ready {
        dispatched: DispatchedRuntime {
            node_id: deployment.node_id.ok_or_else(|| {
                FlowError::Runtime("dispatched deployment omitted its node".into())
            })?,
            command_id: deployment.command_id.ok_or_else(|| {
                FlowError::Runtime("dispatched deployment omitted its command".into())
            })?,
            result_deadline,
        },
    })
}

async fn observe(
    runtime: &DeploymentFlowRuntime,
    input: ObserveStepInput,
) -> a3s_flow::Result<ObserveStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for observation", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if matches!(
        deployment.status,
        DeploymentStatus::Cancelling
            | DeploymentStatus::CleanupPending
            | DeploymentStatus::Cancelled
    ) {
        return Ok(ObserveStepOutput::CancellationRequested);
    }
    if deployment.node_id != Some(input.dispatched.node_id)
        || deployment.command_id != Some(input.dispatched.command_id)
    {
        return Err(FlowError::Runtime(
            "deployment observation identity does not match dispatch".into(),
        ));
    }

    let record = runtime
        .node_control
        .latest_runtime_observation(
            input.dispatched.node_id,
            &input.resolved.spec.unit_id,
            input.resolved.spec.generation,
        )
        .await
        .map_err(|error| flow_error("could not load Runtime observation", error))?;
    if let Some(record) = record {
        if record.command_id != Some(input.dispatched.command_id) {
            return Err(FlowError::Runtime(
                "Runtime observation does not belong to the deployment command".into(),
            ));
        }
        record
            .observation
            .validate_against(&input.resolved.spec)
            .map_err(|error| flow_error("Runtime observation is inconsistent", error))?;
        if record.observation.converges(&input.resolved.spec) {
            return Ok(ObserveStepOutput::Ready {
                observed_at: record.observed_at,
                received_at: record.received_at,
                spec_digest: record.observation.spec_digest,
            });
        }
        if record.observation.state == RuntimeUnitState::Failed
            || record
                .observation
                .health
                .as_ref()
                .is_some_and(|health| health.state == RuntimeHealthState::Unhealthy)
        {
            let reason = record
                .observation
                .failure
                .map(|failure| format!("{}: {}", failure.code, failure.message))
                .or_else(|| record.observation.health.and_then(|health| health.message))
                .unwrap_or_else(|| "Runtime service did not pass its health policy".into());
            return Ok(ObserveStepOutput::Failed {
                reason: bounded_reason(reason),
            });
        }
    } else if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(input.dispatched.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not load node command result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                return Ok(ObserveStepOutput::Failed {
                    reason: bounded_reason(format!("{}: {}", failure.code, failure.message)),
                })
            }
            NodeCommandOutcome::Succeeded { .. } => {
                return Err(FlowError::Runtime(
                    "Runtime apply was acknowledged before its observation was persisted".into(),
                ))
            }
        }
    }

    let now = Utc::now();
    let observation_deadline = input
        .resolved
        .convergence_deadline
        .min(input.dispatched.result_deadline);
    if now >= observation_deadline {
        return Ok(ObserveStepOutput::Failed {
            reason: "Runtime service did not converge before its apply deadline".into(),
        });
    }
    Ok(ObserveStepOutput::Pending {
        reason: "waiting for the requested Runtime generation and health evidence".into(),
        next_poll_at: next_poll(now, runtime.config.observation_poll, observation_deadline)?,
        deadline_at: observation_deadline,
    })
}

async fn verify(
    runtime: &DeploymentFlowRuntime,
    input: VerifyStepInput,
) -> a3s_flow::Result<VerifyStepOutput> {
    if matches!(&input.observation, ObserveStepOutput::CancellationRequested) {
        return Ok(VerifyStepOutput::CancellationRequested);
    }
    let ObserveStepOutput::Ready {
        received_at,
        spec_digest,
        ..
    } = input.observation
    else {
        return Err(FlowError::Runtime(
            "deployment verification requires persisted healthy observation output".into(),
        ));
    };
    let expected_digest = input
        .resolved
        .spec
        .digest()
        .map_err(|error| flow_error("could not digest Runtime specification", error))?;
    if spec_digest != expected_digest {
        return Err(FlowError::Runtime(
            "verified observation changed the Runtime specification digest".into(),
        ));
    }
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for verification", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if matches!(
        deployment.status,
        DeploymentStatus::Cancelling
            | DeploymentStatus::CleanupPending
            | DeploymentStatus::Cancelled
    ) {
        return Ok(VerifyStepOutput::CancellationRequested);
    }
    let verified = runtime
        .workloads
        .mark_verifying(
            deployment.id,
            deployment.aggregate_version,
            received_at.max(deployment.updated_at),
        )
        .await
        .map_err(|error| flow_error("could not persist deployment verification", error))?;
    Ok(VerifyStepOutput::Verified {
        verified_at: verified.updated_at,
    })
}

async fn activate(
    runtime: &DeploymentFlowRuntime,
    input: ActivateStepInput,
) -> a3s_flow::Result<ActivateStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for activation", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if matches!(
        deployment.status,
        DeploymentStatus::Cancelling
            | DeploymentStatus::CleanupPending
            | DeploymentStatus::Cancelled
    ) {
        return Ok(ActivateStepOutput::CancellationRequested);
    }
    let VerifyStepOutput::Verified { verified_at } = input.verification else {
        return Ok(ActivateStepOutput::CancellationRequested);
    };
    let mut gated_at = verified_at;
    if let Some(routing) = &input.routing {
        match routing {
            RouteGate::NotRequired {
                gated_at: route_gated_at,
            } => gated_at = gated_at.max(*route_gated_at),
            RouteGate::Acknowledged {
                publication,
                acknowledged_at,
            } => {
                let previous = input.resolved.previous_runtime.as_ref().ok_or_else(|| {
                    FlowError::Runtime(
                        "initial deployment unexpectedly required a Gateway cutover".into(),
                    )
                })?;
                if publication.deployment_id != deployment.id
                    || publication.node_id != previous.node_id
                    || deployment.node_id != Some(publication.node_id)
                {
                    return Err(FlowError::Runtime(
                        "activation Gateway acknowledgement changed deployment identity".into(),
                    ));
                }
                gated_at = gated_at.max(*acknowledged_at);
            }
        }
    }
    let retirement_required = input.routing.is_some() && input.resolved.previous_runtime.is_some();
    let (_, active) = runtime
        .workloads
        .activate(
            deployment.id,
            deployment.aggregate_version,
            retirement_required,
            gated_at.max(deployment.updated_at),
        )
        .await
        .map_err(|error| flow_error("could not activate deployment", error))?;
    Ok(ActivateStepOutput::Active {
        deployment_id: active.id,
        workload_id: active.workload_id,
        revision_id: active.revision_id,
        activated_at: active
            .activated_at
            .ok_or_else(|| FlowError::Runtime("active deployment has no activation time".into()))?,
        retired_at: None,
    })
}

async fn fail(
    runtime: &DeploymentFlowRuntime,
    input: FailStepInput,
) -> a3s_flow::Result<FailStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.organization_id, input.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for failure", error))?;
    let reason = bounded_reason(input.reason);
    let failed = runtime
        .workloads
        .fail(
            deployment.id,
            deployment.aggregate_version,
            reason.clone(),
            Utc::now().max(deployment.updated_at),
        )
        .await
        .map_err(|error| flow_error("could not persist deployment failure", error))?;
    Ok(FailStepOutput {
        deployment_id: failed.id,
        failed_at: failed.updated_at,
        reason,
    })
}

fn validate_flow_identity(
    input: &DeploymentFlowInput,
    deployment: &crate::modules::workloads::domain::entities::Deployment,
) -> a3s_flow::Result<()> {
    if deployment.id != input.deployment_id
        || deployment.organization_id != input.organization_id
        || deployment.workload_id != input.workload_id
        || deployment.revision_id != input.revision_id
    {
        return Err(FlowError::Runtime(
            "deployment Flow input does not match persisted deployment identity".into(),
        ));
    }
    Ok(())
}

fn validate_resolved_deployment(
    resolved: &ResolveStepOutput,
    deployment: &crate::modules::workloads::domain::entities::Deployment,
) -> a3s_flow::Result<()> {
    if deployment.id != resolved.deployment_id
        || deployment.organization_id != resolved.organization_id
        || deployment.workload_id != resolved.workload_id
        || deployment.revision_id != resolved.revision_id
    {
        return Err(FlowError::Runtime(
            "resolved deployment identity no longer matches persistence".into(),
        ));
    }
    Ok(())
}

fn validate_revision_identity(
    input: &DeploymentFlowInput,
    revision: &WorkloadRevision,
) -> a3s_flow::Result<()> {
    if revision.id != input.revision_id || revision.workload_id != input.workload_id {
        return Err(FlowError::Runtime(
            "workload revision does not belong to the deployment".into(),
        ));
    }
    Ok(())
}

fn next_poll(
    now: DateTime<Utc>,
    interval: chrono::Duration,
    deadline: DateTime<Utc>,
) -> a3s_flow::Result<DateTime<Utc>> {
    Ok(now
        .checked_add_signed(interval)
        .ok_or_else(|| FlowError::Runtime("deployment poll time overflowed".into()))?
        .min(deadline))
}

fn timestamp_millis(value: DateTime<Utc>) -> a3s_flow::Result<u64> {
    u64::try_from(value.timestamp_millis())
        .map_err(|_| FlowError::Runtime("deployment deadline predates the Unix epoch".into()))
}

fn apply_result_deadline(
    command: &crate::modules::fleet::domain::entities::NodeCommand,
    expected_spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> a3s_flow::Result<DateTime<Utc>> {
    let NodeCommandPayload::RuntimeApply { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "deployment command is not a Runtime apply request".into(),
        ));
    };
    if request.spec != *expected_spec {
        return Err(FlowError::Runtime(
            "deployment command changed its Runtime specification".into(),
        ));
    }
    let deadline_ms = request
        .deadline_at_ms
        .ok_or_else(|| FlowError::Runtime("Runtime apply command omitted its deadline".into()))?;
    let deadline_ms = i64::try_from(deadline_ms)
        .map_err(|_| FlowError::Runtime("Runtime apply deadline exceeds supported range".into()))?;
    DateTime::from_timestamp_millis(deadline_ms)
        .map(|deadline| deadline.min(command.not_after))
        .ok_or_else(|| FlowError::Runtime("Runtime apply deadline is invalid".into()))
}

fn bounded_reason(value: String) -> String {
    let normalized = value
        .chars()
        .map(|character| {
            if character == '\0' || character == '\r' || character == '\n' {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut end = normalized.len().min(16 * 1024);
    while !normalized.is_char_boundary(end) {
        end -= 1;
    }
    let bounded = normalized[..end].trim();
    if bounded.is_empty() {
        "deployment failed without a usable diagnostic".into()
    } else {
        bounded.into()
    }
}

fn encode(value: impl Serialize) -> a3s_flow::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(Into::into)
}

fn operation_id_from_run(run_id: &str) -> a3s_flow::Result<OperationId> {
    Uuid::parse_str(run_id)
        .map(OperationId::from_uuid)
        .map_err(|error| flow_error("deployment Flow run ID is invalid", error))
}
