mod cleanup;
mod gateway;
mod resource_claims;
mod retirement;

use super::types::{
    ActivateStepInput, ActivateStepOutput, DeploymentFlowInput, DispatchStepInput,
    DispatchStepOutput, DispatchedRuntime, FailStepInput, FailStepOutput, ObserveStepInput,
    ObserveStepOutput, PrestartGateStepInput, PrestartGateStepOutput, PreviousRuntime,
    ResolveCancellationOutput, ResolveStepOutput, ResolveStepResult, RouteGate, ScheduleStepInput,
    ScheduleStepOutput, VerifyStepInput, VerifyStepOutput,
};
use super::DeploymentFlowRuntime;
use super::{flow_error, resource_claim_id};
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::shared_kernel::domain::{NodeCommandId, OperationId, RepositoryError};
use crate::modules::workloads::application::project_bound_runtime_spec;
use crate::modules::workloads::application::project_replica_runtime_spec;
use crate::modules::workloads::domain::entities::{
    CompiledResourceRequirements, DeploymentReplicaBinding, DeploymentStatus, ReplicaAntiAffinity,
    ResourceClaimBindingEvidence, ResourceClaimReservation, ResourceClaimState,
    SecretBindingTarget, ServiceResources, WorkloadReplica, WorkloadReplicaMember,
    WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::{
    is_capacity_unavailable, is_placement_unavailable,
};
use crate::modules::workloads::domain::services::OciRegistryCredentialReference;
use crate::modules::workloads::domain::services::{
    WorkloadPrestartGateRequest, WorkloadPrestartGateStatus,
};
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload, NodeResourceClaimBinding};
use a3s_flow::{FlowError, StepInvocation};
use a3s_runtime::contract::{
    RuntimeApplyRequest, RuntimeCapabilities, RuntimeHealthState, RuntimeUnitState,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

pub(super) const RESOLVE_DEPLOYMENT: &str = "resolve_deployment";
pub(super) const SCHEDULE_DEPLOYMENT: &str = "schedule_deployment";
pub(super) const PREPARE_RESOURCE_CLAIM: &str = "prepare_resource_claim";
pub(super) const RECONCILE_WORKLOAD_PRESTART_GATE: &str = "reconcile_workload_prestart_gate";
pub(super) const DISPATCH_RUNTIME_APPLY: &str = "dispatch_runtime_apply";
pub(super) const DISPATCH_RESOURCE_BOUND_RUNTIME_APPLY: &str =
    "dispatch_resource_bound_runtime_apply";
pub(super) const OBSERVE_RUNTIME_APPLY: &str = "observe_runtime_apply";
pub(super) const OBSERVE_RESOURCE_BOUND_RUNTIME_APPLY: &str =
    "observe_resource_bound_runtime_apply";
pub(super) const VERIFY_RUNTIME_HEALTH: &str = "verify_runtime_health";
pub(super) const STAGE_DEPLOYMENT_GATEWAY: &str = "stage_deployment_gateway";
pub(super) const OBSERVE_DEPLOYMENT_GATEWAY: &str = "observe_deployment_gateway";
pub(super) const ACTIVATE_DEPLOYMENT: &str = "activate_deployment";
pub(super) const DISPATCH_PREVIOUS_RUNTIME_RETIREMENT: &str =
    "dispatch_previous_runtime_retirement";
pub(super) const OBSERVE_PREVIOUS_RUNTIME_RETIREMENT: &str = "observe_previous_runtime_retirement";
pub(super) const COMPLETE_DEPLOYMENT_RETIREMENT: &str = "complete_deployment_retirement";
pub(super) const DISPATCH_RUNTIME_CLEANUP: &str = "dispatch_runtime_cleanup";
pub(super) const OBSERVE_RUNTIME_CLEANUP: &str = "observe_runtime_cleanup";
pub(super) const DISPATCH_RUNTIME_REMOVAL: &str = "dispatch_runtime_removal";
pub(super) const OBSERVE_RUNTIME_REMOVAL: &str = "observe_runtime_removal";
pub(super) const DISPATCH_FAILED_RUNTIME_CLEANUP: &str = "dispatch_failed_runtime_cleanup";
pub(super) const OBSERVE_FAILED_RUNTIME_CLEANUP: &str = "observe_failed_runtime_cleanup";
pub(super) const RELEASE_RESOURCE_CLAIM: &str = "release_resource_claim";
pub(super) const COMPLETE_DEPLOYMENT_CANCELLATION: &str = "complete_deployment_cancellation";
pub(super) const FAIL_DEPLOYMENT: &str = "fail_deployment";

pub(super) const STEP_NAMES: &[&str] = &[
    RESOLVE_DEPLOYMENT,
    SCHEDULE_DEPLOYMENT,
    PREPARE_RESOURCE_CLAIM,
    RECONCILE_WORKLOAD_PRESTART_GATE,
    DISPATCH_RUNTIME_APPLY,
    DISPATCH_RESOURCE_BOUND_RUNTIME_APPLY,
    OBSERVE_RUNTIME_APPLY,
    OBSERVE_RESOURCE_BOUND_RUNTIME_APPLY,
    VERIFY_RUNTIME_HEALTH,
    STAGE_DEPLOYMENT_GATEWAY,
    OBSERVE_DEPLOYMENT_GATEWAY,
    ACTIVATE_DEPLOYMENT,
    DISPATCH_PREVIOUS_RUNTIME_RETIREMENT,
    OBSERVE_PREVIOUS_RUNTIME_RETIREMENT,
    COMPLETE_DEPLOYMENT_RETIREMENT,
    DISPATCH_RUNTIME_CLEANUP,
    OBSERVE_RUNTIME_CLEANUP,
    DISPATCH_RUNTIME_REMOVAL,
    OBSERVE_RUNTIME_REMOVAL,
    DISPATCH_FAILED_RUNTIME_CLEANUP,
    OBSERVE_FAILED_RUNTIME_CLEANUP,
    RELEASE_RESOURCE_CLAIM,
    COMPLETE_DEPLOYMENT_CANCELLATION,
    FAIL_DEPLOYMENT,
];

pub(super) async fn execute(
    runtime: &DeploymentFlowRuntime,
    invocation: StepInvocation,
) -> a3s_flow::Result<serde_json::Value> {
    match invocation.step_name.as_str() {
        RESOLVE_DEPLOYMENT => {
            encode(resolve(runtime, &invocation.run_id, invocation.input_as()?).await?)
        }
        SCHEDULE_DEPLOYMENT => encode(schedule(runtime, invocation.input_as()?).await?),
        PREPARE_RESOURCE_CLAIM => {
            encode(resource_claims::prepare(runtime, invocation.input_as()?).await?)
        }
        RECONCILE_WORKLOAD_PRESTART_GATE => {
            encode(reconcile_prestart_gate(runtime, invocation.input_as()?).await?)
        }
        DISPATCH_RUNTIME_APPLY => encode(dispatch(runtime, invocation.input_as()?).await?),
        DISPATCH_RESOURCE_BOUND_RUNTIME_APPLY => {
            encode(dispatch_bound(runtime, invocation.input_as()?).await?)
        }
        OBSERVE_RUNTIME_APPLY => encode(observe(runtime, invocation.input_as()?).await?),
        OBSERVE_RESOURCE_BOUND_RUNTIME_APPLY => {
            encode(observe_bound(runtime, invocation.input_as()?).await?)
        }
        VERIFY_RUNTIME_HEALTH => encode(verify(runtime, invocation.input_as()?).await?),
        STAGE_DEPLOYMENT_GATEWAY => encode(gateway::stage(runtime, invocation.input_as()?).await?),
        OBSERVE_DEPLOYMENT_GATEWAY => {
            encode(gateway::observe(runtime, invocation.input_as()?).await?)
        }
        ACTIVATE_DEPLOYMENT => encode(activate(runtime, invocation.input_as()?).await?),
        DISPATCH_PREVIOUS_RUNTIME_RETIREMENT => {
            encode(retirement::dispatch(runtime, invocation.input_as()?).await?)
        }
        OBSERVE_PREVIOUS_RUNTIME_RETIREMENT => {
            encode(retirement::observe(runtime, invocation.input_as()?).await?)
        }
        COMPLETE_DEPLOYMENT_RETIREMENT => {
            encode(retirement::complete(runtime, invocation.input_as()?).await?)
        }
        DISPATCH_RUNTIME_CLEANUP => {
            encode(cleanup::dispatch_cleanup(runtime, invocation.input_as()?).await?)
        }
        OBSERVE_RUNTIME_CLEANUP => {
            encode(cleanup::observe_cleanup(runtime, invocation.input_as()?).await?)
        }
        DISPATCH_RUNTIME_REMOVAL => {
            encode(cleanup::dispatch_removal(runtime, invocation.input_as()?).await?)
        }
        OBSERVE_RUNTIME_REMOVAL => {
            encode(cleanup::observe_removal(runtime, invocation.input_as()?).await?)
        }
        DISPATCH_FAILED_RUNTIME_CLEANUP => {
            encode(cleanup::dispatch_failed(runtime, invocation.input_as()?).await?)
        }
        OBSERVE_FAILED_RUNTIME_CLEANUP => {
            encode(cleanup::observe_failed(runtime, invocation.input_as()?).await?)
        }
        RELEASE_RESOURCE_CLAIM => {
            encode(resource_claims::release(runtime, invocation.input_as()?).await?)
        }
        COMPLETE_DEPLOYMENT_CANCELLATION => {
            encode(cleanup::complete_cancellation(runtime, invocation.input_as()?).await?)
        }
        FAIL_DEPLOYMENT => encode(fail(runtime, invocation.input_as()?).await?),
        step => Err(FlowError::Runtime(format!(
            "Cloud deployment workflow has no step {step:?}"
        ))),
    }
}

async fn reconcile_prestart_gate(
    runtime: &DeploymentFlowRuntime,
    input: PrestartGateStepInput,
) -> a3s_flow::Result<PrestartGateStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for its pre-start gate", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if deployment.node_id != Some(input.node_id) {
        return Err(FlowError::Runtime(
            "Workload pre-start gate does not own the scheduled node".into(),
        ));
    }
    if matches!(
        deployment.status,
        DeploymentStatus::Failed | DeploymentStatus::Orphaned
    ) {
        return Ok(PrestartGateStepOutput::Failed {
            reason: deployment
                .failure
                .unwrap_or_else(|| "deployment failed before its pre-start gate".into()),
        });
    }
    let cancellation_requested = matches!(
        deployment.status,
        DeploymentStatus::Cancelling
            | DeploymentStatus::CleanupPending
            | DeploymentStatus::Cancelled
    );
    if !cancellation_requested && deployment.status != DeploymentStatus::Scheduled {
        return Err(FlowError::Runtime(format!(
            "deployment cannot run its pre-start gate from {}",
            deployment.status.as_str()
        )));
    }
    let deadline_at = if cancellation_requested {
        deployment
            .cancellation_requested_at
            .ok_or_else(|| {
                FlowError::Runtime(
                    "cancelling deployment omitted its cancellation request time".into(),
                )
            })?
            .checked_add_signed(runtime.config.cleanup_timeout)
            .ok_or_else(|| {
                FlowError::Runtime("pre-start cancellation deadline overflowed".into())
            })?
    } else {
        input.resolved.convergence_deadline
    };
    let now = Utc::now().max(deployment.updated_at);
    let status = runtime
        .prestart_gate
        .reconcile(&WorkloadPrestartGateRequest {
            organization_id: deployment.organization_id,
            deployment_id: deployment.id,
            operation_id: deployment.operation_id,
            workload_id: deployment.workload_id,
            workload_revision_id: deployment.revision_id,
            node_id: input.node_id,
            cancellation_requested,
            deadline_at,
            now,
        })
        .await
        .map_err(|error| flow_error("could not reconcile Workload pre-start gate", error))?;
    match status {
        WorkloadPrestartGateStatus::Ready { completed_at } => {
            if cancellation_requested {
                return Err(FlowError::Runtime(
                    "cancelling Workload pre-start gate returned ready".into(),
                ));
            }
            if completed_at < deployment.requested_at || completed_at > now {
                return Err(FlowError::Runtime(
                    "Workload pre-start completion time is invalid".into(),
                ));
            }
            Ok(PrestartGateStepOutput::Ready {
                node_id: input.node_id,
                completed_at,
            })
        }
        WorkloadPrestartGateStatus::CancellationReady { completed_at } => {
            if !cancellation_requested
                || completed_at < deployment.requested_at
                || completed_at > now
            {
                return Err(FlowError::Runtime(
                    "Workload pre-start cancellation completion is invalid".into(),
                ));
            }
            Ok(PrestartGateStepOutput::CancellationRequested { completed_at })
        }
        WorkloadPrestartGateStatus::Failed { reason } => Ok(PrestartGateStepOutput::Failed {
            reason: bounded_prestart_reason(reason)?,
        }),
        WorkloadPrestartGateStatus::Pending { reason } => {
            let reason = bounded_prestart_reason(reason)?;
            if now >= deadline_at {
                return Ok(PrestartGateStepOutput::Failed {
                    reason: format!("Workload pre-start gate exceeded its deadline: {reason}"),
                });
            }
            let next_poll_at = now
                .checked_add_signed(runtime.config.observation_poll)
                .ok_or_else(|| FlowError::Runtime("pre-start gate poll time overflowed".into()))?
                .min(deadline_at);
            Ok(PrestartGateStepOutput::Pending {
                reason,
                next_poll_at,
                deadline_at,
            })
        }
    }
}

fn bounded_prestart_reason(reason: String) -> a3s_flow::Result<String> {
    if reason.is_empty() || reason.len() > 16 * 1024 || reason.contains(['\0', '\r', '\n']) {
        return Err(FlowError::Runtime(
            "Workload pre-start gate returned an invalid reason".into(),
        ));
    }
    Ok(reason)
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
    let replica_binding = runtime
        .workloads
        .find_deployment_replica_binding(input.organization_id, deployment.id)
        .await
        .map_err(|error| flow_error("could not load deployment replica binding", error))?;
    let replica = runtime
        .workloads
        .find_workload_replica(
            input.organization_id,
            input.workload_id,
            replica_binding.replica_id,
        )
        .await
        .map_err(|error| flow_error("could not load deployment replica", error))?;
    let member = runtime
        .workloads
        .find_workload_replica_member(
            input.organization_id,
            replica_binding.replica_id,
            replica_binding.member_id,
        )
        .await
        .map_err(|error| flow_error("could not load deployment replica member", error))?;
    let spec = project_replica_runtime_spec(&revision, &replica)
        .map_err(|error| flow_error("could not project replica Runtime specification", error))?;
    validate_current_replica_binding(
        &replica_binding,
        &deployment,
        &revision,
        &replica,
        &member,
        &spec,
    )?;
    let previous_runtime = previous_runtime(runtime, &input, &revision, &replica_binding).await?;

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
    candidate_binding: &DeploymentReplicaBinding,
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
        return Ok(None);
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
    let mut previous = None;
    for deployment in deployments.into_iter().filter(|deployment| {
        deployment.revision_id == previous_revision.id
            && deployment.status == DeploymentStatus::Active
    }) {
        let binding = runtime
            .workloads
            .find_deployment_replica_binding(input.organization_id, deployment.id)
            .await
            .map_err(|error| flow_error("could not load previous replica binding", error))?;
        if binding.replica_id != candidate_binding.replica_id {
            continue;
        }
        if previous.replace((deployment, binding)).is_some() {
            return Err(FlowError::Runtime(
                "active replica generation has multiple previous deployments".into(),
            ));
        }
    }
    let (previous_deployment, replica_binding) = previous.ok_or_else(|| {
        FlowError::Runtime("candidate replica has no active previous deployment".into())
    })?;
    let node_id = previous_deployment
        .node_id
        .ok_or_else(|| FlowError::Runtime("active deployment omitted its node".into()))?;
    let spec =
        project_bound_runtime_spec(&previous_revision, &replica_binding).map_err(|error| {
            flow_error(
                "could not project previous replica Runtime specification",
                error,
            )
        })?;
    validate_runtime_binding(
        &replica_binding,
        &previous_deployment,
        &previous_revision,
        &spec,
    )?;
    Ok(Some(PreviousRuntime {
        deployment_id: Some(previous_deployment.id),
        revision_id: previous_revision.id,
        node_id,
        spec,
    }))
}

fn validate_current_replica_binding(
    binding: &DeploymentReplicaBinding,
    deployment: &crate::modules::workloads::domain::entities::Deployment,
    revision: &WorkloadRevision,
    replica: &WorkloadReplica,
    member: &WorkloadReplicaMember,
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> a3s_flow::Result<()> {
    binding
        .validate_against(deployment, revision, replica, member)
        .map_err(|error| flow_error("deployment replica binding is invalid", error))?;
    validate_runtime_binding(binding, deployment, revision, spec)
}

fn validate_runtime_binding(
    binding: &DeploymentReplicaBinding,
    deployment: &crate::modules::workloads::domain::entities::Deployment,
    revision: &WorkloadRevision,
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> a3s_flow::Result<()> {
    if binding.deployment_id != deployment.id
        || binding.organization_id != deployment.organization_id
        || binding.workload_id != deployment.workload_id
        || binding.revision_id != revision.id
        || binding.runtime_unit_id != spec.unit_id
        || binding.runtime_generation != spec.generation
        || binding.node_id != deployment.node_id
    {
        return Err(FlowError::Runtime(
            "deployment replica binding does not match its Runtime projection".into(),
        ));
    }
    Ok(())
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
            if let Some(claim) = scheduling_claim(runtime, &deployment, &input.resolved).await? {
                if claim.node_id != node_id {
                    return Err(FlowError::Runtime(
                        "scheduled deployment changed its reserved resource node".into(),
                    ));
                }
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
    if let Some(claim) = scheduling_claim(runtime, &deployment, &input.resolved).await? {
        let scheduled = runtime
            .workloads
            .assign_node(
                deployment.id,
                deployment.aggregate_version,
                claim.node_id,
                now.max(claim.created_at),
            )
            .await
            .map_err(|error| {
                flow_error(
                    "could not recover deployment placement from its resource claim",
                    error,
                )
            })?;
        return Ok(ScheduleStepOutput::Ready {
            node_id: scheduled.node_id.ok_or_else(|| {
                FlowError::Runtime("recovered deployment placement omitted its node".into())
            })?,
        });
    }
    let binding = runtime
        .workloads
        .find_deployment_replica_binding(deployment.organization_id, deployment.id)
        .await
        .map_err(|error| {
            flow_error(
                "could not load deployment replica binding for resource reservation",
                error,
            )
        })?;
    if binding.node_id.is_some() {
        return Err(FlowError::Runtime(
            "resolving deployment has an inconsistent placed replica binding".into(),
        ));
    }
    let control = runtime
        .workloads
        .find_workload_control(deployment.organization_id, deployment.workload_id)
        .await
        .map_err(|error| {
            flow_error(
                "could not load effective placement policy for scheduling",
                error,
            )
        })?;
    if control.organization_id != deployment.organization_id
        || control.workload_id != deployment.workload_id
        || control.spec.placement_policy.replica_anti_affinity() != ReplicaAntiAffinity::Required
    {
        return Err(FlowError::Runtime(
            "deployment has no supported required replica anti-affinity policy".into(),
        ));
    }
    let mut nodes = runtime
        .nodes
        .list_scheduling_candidates(
            deployment.organization_id,
            control.spec.placement_policy.node_pool_id(),
            now,
        )
        .await
        .map_err(|error| flow_error("could not list deployment scheduling candidates", error))?;
    nodes.sort_by_key(|node| node.id);
    let mut anti_affinity_unavailable = false;
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
        let Some(inventory) = runtime
            .node_control
            .current_resource_inventory(node.id)
            .await
            .map_err(|error| flow_error("could not load current node resource inventory", error))?
        else {
            continue;
        };
        inventory
            .validate()
            .map_err(|error| flow_error("current node resource inventory is invalid", error))?;
        let resources = ServiceResources {
            cpu_millis: input.resolved.spec.resources.cpu_millis,
            memory_bytes: input.resolved.spec.resources.memory_bytes,
            pids: input.resolved.spec.resources.pids,
            ephemeral_storage_bytes: input.resolved.spec.resources.ephemeral_storage_bytes,
        };
        let requirements =
            match CompiledResourceRequirements::compile(&resources, &inventory.inventory) {
                Ok(requirements) => requirements,
                Err(_) => continue,
            };
        let candidate_binding = binding
            .propose_assignment(node.id, now)
            .map_err(|error| flow_error("could not propose replica placement", error))?;
        let reservation = ResourceClaimReservation {
            id: resource_claim_id(deployment.id),
            binding: candidate_binding,
            node_id: node.id,
            inventory: inventory.inventory,
            topology_digest: requirements.topology_digest,
            slots: requirements.slots,
            reserved_at: now,
        };
        let claim = match runtime.resource_claims.reserve(reservation).await {
            Ok(result) => result.value,
            Err(error) if is_capacity_unavailable(&error) => continue,
            Err(error) if is_placement_unavailable(&error) => {
                anti_affinity_unavailable = true;
                continue;
            }
            Err(RepositoryError::IdempotencyConflict) => {
                let claim = scheduling_claim(runtime, &deployment, &input.resolved)
                    .await?
                    .ok_or_else(|| {
                        FlowError::Runtime(
                            "resource claim conflicted without a durable winner".into(),
                        )
                    })?;
                if claim.node_id != node.id {
                    return Err(FlowError::Runtime(
                        "concurrent scheduling selected a different reserved node".into(),
                    ));
                }
                claim
            }
            Err(error) => {
                return Err(flow_error(
                    "could not reserve deployment resource capacity",
                    error,
                ))
            }
        };
        validate_scheduling_claim(&claim, &deployment, &input.resolved)?;
        let scheduled = runtime
            .workloads
            .assign_node(
                deployment.id,
                deployment.aggregate_version,
                claim.node_id,
                now,
            )
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
            reason: if anti_affinity_unavailable {
                "no node satisfied required replica anti-affinity before the convergence deadline"
                    .into()
            } else {
                "no eligible node became available before the convergence deadline".into()
            },
        });
    }
    Ok(ScheduleStepOutput::Pending {
        reason: if input.resolved.previous_runtime.is_some() {
            "the previous Runtime node is not ready for a one-node update".into()
        } else if anti_affinity_unavailable {
            "no ready node satisfies required replica anti-affinity".into()
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

async fn scheduling_claim(
    runtime: &DeploymentFlowRuntime,
    deployment: &crate::modules::workloads::domain::entities::Deployment,
    resolved: &ResolveStepOutput,
) -> a3s_flow::Result<Option<crate::modules::workloads::domain::entities::ResourceClaim>> {
    match runtime
        .resource_claims
        .find(deployment.organization_id, resource_claim_id(deployment.id))
        .await
    {
        Ok(claim) => {
            validate_scheduling_claim(&claim, deployment, resolved)?;
            Ok(Some(claim))
        }
        Err(RepositoryError::NotFound) => Ok(None),
        Err(error) => Err(flow_error(
            "could not load deployment resource claim",
            error,
        )),
    }
}

fn validate_scheduling_claim(
    claim: &crate::modules::workloads::domain::entities::ResourceClaim,
    deployment: &crate::modules::workloads::domain::entities::Deployment,
    resolved: &ResolveStepOutput,
) -> a3s_flow::Result<()> {
    if claim.organization_id != deployment.organization_id
        || claim.workload_id != deployment.workload_id
        || claim.deployment_id != deployment.id
        || claim.runtime_unit_id != resolved.spec.unit_id
        || claim.runtime_generation != resolved.spec.generation
        || claim.state == ResourceClaimState::Released
    {
        return Err(FlowError::Runtime(
            "deployment resource claim does not match its exact Runtime placement".into(),
        ));
    }
    claim
        .validate()
        .map_err(|error| flow_error("deployment resource claim is invalid", error))
}

async fn dispatch(
    runtime: &DeploymentFlowRuntime,
    input: DispatchStepInput,
) -> a3s_flow::Result<DispatchStepOutput> {
    dispatch_with_claim(runtime, input, false).await
}

async fn dispatch_bound(
    runtime: &DeploymentFlowRuntime,
    input: DispatchStepInput,
) -> a3s_flow::Result<DispatchStepOutput> {
    dispatch_with_claim(runtime, input, true).await
}

async fn dispatch_with_claim(
    runtime: &DeploymentFlowRuntime,
    input: DispatchStepInput,
    require_resource_claim: bool,
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
    let replica_binding = runtime
        .workloads
        .find_deployment_replica_binding(deployment.organization_id, deployment.id)
        .await
        .map_err(|error| flow_error("could not load replica binding for dispatch", error))?;
    validate_resolved_replica_binding(&replica_binding, &deployment, &input.resolved)?;
    let resource_claim = if require_resource_claim {
        Some(Box::new(
            prepared_binding_for_dispatch(runtime, &input).await?,
        ))
    } else {
        None
    };
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
            if command.aggregate_id != replica_binding.replica_id.as_uuid() {
                return Err(FlowError::Runtime(
                    "dispatched Runtime apply changed its replica aggregate".into(),
                ));
            }
            let result_deadline =
                apply_result_deadline(&command, &input.resolved.spec, resource_claim.as_deref())?;
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
        resource_claim,
    };
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id: input.node_id,
            aggregate_id: replica_binding.replica_id.as_uuid(),
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
    let expected_binding = match &command.payload {
        NodeCommandPayload::RuntimeApply { resource_claim, .. } => resource_claim.as_deref(),
        _ => None,
    };
    let result_deadline = apply_result_deadline(&command, &input.resolved.spec, expected_binding)?;
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
    observe_with_claim(runtime, input, false).await
}

async fn observe_bound(
    runtime: &DeploymentFlowRuntime,
    input: ObserveStepInput,
) -> a3s_flow::Result<ObserveStepOutput> {
    observe_with_claim(runtime, input, true).await
}

async fn observe_with_claim(
    runtime: &DeploymentFlowRuntime,
    input: ObserveStepInput,
    require_resource_claim: bool,
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
    let resource_binding = if require_resource_claim {
        Some(
            dispatched_resource_binding(runtime, &input)
                .await?
                .ok_or_else(|| {
                    FlowError::Runtime(
                        "resource-bound Runtime apply omitted its prepared claim".into(),
                    )
                })?,
        )
    } else {
        None
    };

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
        if let Some(binding) = &resource_binding {
            binding
                .validate_runtime_observation(&record.observation)
                .map_err(|error| {
                    flow_error(
                        "Runtime observation omitted its exact resource allocation binding",
                        error,
                    )
                })?;
            persist_runtime_binding(runtime, &input, &record, binding).await?;
        }
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
    expected_binding: Option<&NodeResourceClaimBinding>,
) -> a3s_flow::Result<DateTime<Utc>> {
    let NodeCommandPayload::RuntimeApply {
        request,
        resource_claim,
    } = &command.payload
    else {
        return Err(FlowError::Runtime(
            "deployment command is not a Runtime apply request".into(),
        ));
    };
    if request.spec != *expected_spec || resource_claim.as_deref() != expected_binding {
        return Err(FlowError::Runtime(
            "deployment command changed its Runtime specification or resource binding".into(),
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

async fn prepared_binding_for_dispatch(
    runtime: &DeploymentFlowRuntime,
    input: &DispatchStepInput,
) -> a3s_flow::Result<NodeResourceClaimBinding> {
    let claim = runtime
        .resource_claims
        .find(
            input.resolved.organization_id,
            resource_claim_id(input.resolved.deployment_id),
        )
        .await
        .map_err(|error| {
            flow_error(
                "could not load prepared resource claim for Runtime dispatch",
                error,
            )
        })?;
    if !matches!(
        claim.state,
        ResourceClaimState::PreparedOnAgent | ResourceClaimState::BoundToRuntimeUnit
    ) || claim.node_id != input.node_id
        || claim.deployment_id != input.resolved.deployment_id
        || claim.workload_id != input.resolved.workload_id
        || claim.runtime_unit_id != input.resolved.spec.unit_id
        || claim.runtime_generation != input.resolved.spec.generation
    {
        return Err(FlowError::Runtime(
            "Runtime dispatch has no matching prepared resource claim".into(),
        ));
    }
    let binding = resource_claims::load_prepared_binding(runtime, &claim).await?;
    binding
        .validate_runtime_spec(&input.resolved.spec)
        .map_err(|error| flow_error("prepared resource claim cannot bind Runtime apply", error))?;
    let digest = binding
        .digest()
        .map_err(|error| flow_error("could not digest prepared Runtime resource binding", error))?;
    if claim.prepared_binding_digest.as_ref() != Some(&digest) {
        return Err(FlowError::Runtime(
            "prepared resource claim digest changed before Runtime dispatch".into(),
        ));
    }
    Ok(binding)
}

async fn dispatched_resource_binding(
    runtime: &DeploymentFlowRuntime,
    input: &ObserveStepInput,
) -> a3s_flow::Result<Option<NodeResourceClaimBinding>> {
    let command = runtime
        .node_control
        .find_command(input.dispatched.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not reload resource-bound Runtime apply", error))?
        .ok_or_else(|| FlowError::Runtime("resource-bound Runtime apply is missing".into()))?;
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not reload resource-bound deployment", error))?;
    let replica_binding = runtime
        .workloads
        .find_deployment_replica_binding(input.resolved.organization_id, deployment.id)
        .await
        .map_err(|error| flow_error("could not reload resource-bound replica binding", error))?;
    validate_resolved_replica_binding(&replica_binding, &deployment, &input.resolved)?;
    if command.id != input.dispatched.command_id
        || command.node_id != input.dispatched.node_id
        || command.aggregate_id != replica_binding.replica_id.as_uuid()
    {
        return Err(FlowError::Runtime(
            "resource-bound Runtime apply identity changed".into(),
        ));
    }
    let NodeCommandPayload::RuntimeApply {
        request,
        resource_claim,
    } = command.payload
    else {
        return Err(FlowError::Runtime(
            "resource-bound deployment command is not a Runtime apply".into(),
        ));
    };
    if request.spec != input.resolved.spec {
        return Err(FlowError::Runtime(
            "resource-bound Runtime apply changed its specification".into(),
        ));
    }
    Ok(resource_claim.map(|binding| *binding))
}

fn validate_resolved_replica_binding(
    binding: &DeploymentReplicaBinding,
    deployment: &crate::modules::workloads::domain::entities::Deployment,
    resolved: &ResolveStepOutput,
) -> a3s_flow::Result<()> {
    if binding.deployment_id != deployment.id
        || binding.organization_id != deployment.organization_id
        || binding.workload_id != deployment.workload_id
        || binding.revision_id != deployment.revision_id
        || binding.deployment_id != resolved.deployment_id
        || binding.organization_id != resolved.organization_id
        || binding.workload_id != resolved.workload_id
        || binding.revision_id != resolved.revision_id
        || binding.runtime_unit_id != resolved.spec.unit_id
        || binding.runtime_generation != resolved.spec.generation
        || binding.node_id != deployment.node_id
    {
        return Err(FlowError::Runtime(
            "resolved deployment changed its replica Runtime binding".into(),
        ));
    }
    Ok(())
}

async fn persist_runtime_binding(
    runtime: &DeploymentFlowRuntime,
    input: &ObserveStepInput,
    record: &crate::modules::fleet::domain::repositories::RuntimeObservationRecord,
    binding: &NodeResourceClaimBinding,
) -> a3s_flow::Result<()> {
    let claim = runtime
        .resource_claims
        .find(
            input.resolved.organization_id,
            resource_claim_id(input.resolved.deployment_id),
        )
        .await
        .map_err(|error| {
            flow_error(
                "could not load prepared resource claim for Runtime binding",
                error,
            )
        })?;
    if claim.state == ResourceClaimState::BoundToRuntimeUnit {
        if claim.bound_at.is_none()
            || claim.prepared_binding_digest.as_ref()
                != Some(
                    &binding.digest().map_err(|error| {
                        flow_error("could not verify bound resource digest", error)
                    })?,
                )
        {
            return Err(FlowError::Runtime(
                "bound resource claim has inconsistent durable evidence".into(),
            ));
        }
        return Ok(());
    }
    if claim.state != ResourceClaimState::PreparedOnAgent
        || claim.id.as_uuid() != binding.claim_id
        || claim.node_id != input.dispatched.node_id
    {
        return Err(FlowError::Runtime(
            "Runtime allocation evidence does not own a prepared resource claim".into(),
        ));
    }
    let evidence = ResourceClaimBindingEvidence {
        runtime_unit_id: binding.runtime_unit_id.clone(),
        runtime_generation: binding.runtime_generation,
        binding_digest: binding
            .digest()
            .map_err(|error| flow_error("could not digest Runtime resource binding", error))?,
        slots: claim.slot_evidence(),
        observed_at: record.observed_at,
    };
    runtime
        .resource_claims
        .bind(
            claim.organization_id,
            claim.id,
            claim.aggregate_version,
            evidence,
            record.received_at.max(claim.updated_at),
        )
        .await
        .map_err(|error| flow_error("could not persist Runtime allocation binding", error))?;
    Ok(())
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
