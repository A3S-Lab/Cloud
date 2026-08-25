use super::{DeploymentFlowConfig, DeploymentFlowRuntime};
use crate::modules::fleet::domain::repositories::NodeResourceInventoryRecord;
use crate::modules::shared_kernel::domain::{
    DeploymentId, NodeId, OrganizationId, RepositoryError, ResourceClaimId, WorkloadId,
    WorkloadPlacementGroupId, WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::application::project_placement_group_runtime_spec;
use crate::modules::workloads::domain::entities::{
    AtomicResourceClaimReservation, CompiledResourceRequirements, Deployment,
    DeploymentReplicaBinding, DeploymentStatus, PlacementTopology, ReplicaAntiAffinity,
    ResourceClaim, ResourceClaimReservation, ResourceClaimState, ServiceResources, WorkloadControl,
    WorkloadPlacementGroup,
};
use crate::modules::workloads::domain::repositories::{
    is_capacity_unavailable, is_placement_unavailable, PlacementGroupCancellationWrite,
    PlacementGroupMemberPlacement, PlacementGroupSchedulingWrite,
};
use crate::modules::workloads::infrastructure::replica_deployment_materialization::validate_existing_materialization;
use a3s_cloud_contracts::NodeResourceInventory;
use a3s_flow::{FlowError, RuntimeCommand, StepInvocation, WorkflowInvocation};
use a3s_runtime::contract::{RuntimeCapabilities, RuntimeUnitSpec};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const VALIDATE_MATERIALIZATION: &str = "placement_group_deployment_v2_validate_materialization";
const SCHEDULE: &str = "placement_group_deployment_v2_schedule";
const VALIDATE_SCHEDULING: &str = "placement_group_deployment_v2_validate_scheduling";
pub(super) const STEP_NAMES: &[&str] = &[VALIDATE_MATERIALIZATION, SCHEDULE, VALIDATE_SCHEDULING];

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
struct ValidateMaterializationInput {
    deployment: PlacementGroupDeploymentInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum ValidateMaterializationOutput {
    Ready {
        validated_at: DateTime<Utc>,
        scheduling_deadline: DateTime<Utc>,
    },
    Cancelled {
        cancelled_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SchedulePlacementGroupInput {
    deployment: PlacementGroupDeploymentInput,
    attempt: u32,
    scheduling_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScheduledMemberEvidence {
    ordinal: u32,
    member_id: WorkloadReplicaMemberId,
    node_id: NodeId,
    claim_id: ResourceClaimId,
    placement_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum SchedulePlacementGroupOutput {
    Ready {
        members: Vec<ScheduledMemberEvidence>,
        placement_digest: String,
        scheduled_at: DateTime<Utc>,
    },
    Pending {
        attempt: u32,
        reason: String,
        observed_at: DateTime<Utc>,
        next_poll_at: DateTime<Utc>,
        scheduling_deadline: DateTime<Utc>,
    },
    Failed {
        failed_at: DateTime<Utc>,
        reason: String,
    },
    Cancelled {
        cancelled_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ValidateScheduledGroupInput {
    deployment: PlacementGroupDeploymentInput,
    attempt: u32,
    placement_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum ValidateScheduledGroupOutput {
    AwaitingPreparation {
        attempt: u32,
        member_count: u32,
        placement_digest: String,
        observed_at: DateTime<Utc>,
        next_poll_at: DateTime<Utc>,
    },
    Cancelled {
        cancelled_at: DateTime<Utc>,
    },
}

struct GroupSchedulingContext {
    deployment: Deployment,
    group: WorkloadPlacementGroup,
    control: WorkloadControl,
    revision: crate::modules::workloads::domain::entities::WorkloadRevision,
    member_bindings: Vec<DeploymentReplicaBinding>,
}

#[derive(Clone)]
struct SchedulableNode {
    node_id: NodeId,
    capabilities: RuntimeCapabilities,
    inventory: NodeResourceInventory,
}

#[derive(Clone)]
struct MemberNodeCandidate {
    node_index: usize,
    node_id: NodeId,
    inventory: NodeResourceInventory,
    requirements: CompiledResourceRequirements,
}

pub(super) fn replay(
    config: &DeploymentFlowConfig,
    invocation: WorkflowInvocation,
) -> a3s_flow::Result<RuntimeCommand> {
    let context = invocation.context();
    let input = context.input_as::<PlacementGroupDeploymentInput>()?;
    validate_input(&input)?;

    let validation = match context
        .step_output_as::<ValidateMaterializationOutput>("placement-group-v2-materialization")?
    {
        Some(output) => output,
        None => {
            if let Some(error) = context.step_failed("placement-group-v2-materialization") {
                return Err(FlowError::Runtime(format!(
                    "placement-group Deployment validation failed: {error}"
                )));
            }
            return Ok(context.schedule_step_with_retry(
                "placement-group-v2-materialization",
                VALIDATE_MATERIALIZATION,
                serde_json::to_value(ValidateMaterializationInput { deployment: input })?,
                config.retry_policy(&context),
            ));
        }
    };
    let scheduling_deadline = match validation {
        ValidateMaterializationOutput::Ready {
            validated_at,
            scheduling_deadline,
        } => {
            if validated_at <= DateTime::<Utc>::UNIX_EPOCH
                || scheduling_deadline <= DateTime::<Utc>::UNIX_EPOCH
            {
                return Err(FlowError::Runtime(
                    "placement-group scheduling deadline is inconsistent".into(),
                ));
            }
            scheduling_deadline
        }
        ValidateMaterializationOutput::Cancelled { cancelled_at } => {
            return Ok(cancelled(&context, &input, cancelled_at));
        }
    };

    let scheduled = replay_scheduling(config, &context, &input, scheduling_deadline)?;
    let (placement_digest, members) = match scheduled {
        ScheduleReplay::Ready {
            placement_digest,
            members,
        } => (placement_digest, members),
        ScheduleReplay::Command(command) => return Ok(command),
    };
    validate_scheduled_members(&members, input.member_count, &placement_digest)?;
    replay_scheduled_validation(config, &context, &input, placement_digest)
}

enum ScheduleReplay {
    Ready {
        placement_digest: String,
        members: Vec<ScheduledMemberEvidence>,
    },
    Command(RuntimeCommand),
}

fn replay_scheduling(
    config: &DeploymentFlowConfig,
    context: &a3s_flow::WorkflowContext<'_>,
    input: &PlacementGroupDeploymentInput,
    scheduling_deadline: DateTime<Utc>,
) -> a3s_flow::Result<ScheduleReplay> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("placement-group-v2-schedule-{attempt}");
        match context.step_output_as::<SchedulePlacementGroupOutput>(&step_id)? {
            Some(SchedulePlacementGroupOutput::Ready {
                members,
                placement_digest,
                scheduled_at,
            }) => {
                if scheduled_at <= DateTime::<Utc>::UNIX_EPOCH {
                    return Err(FlowError::Runtime(
                        "placement-group scheduled time is invalid".into(),
                    ));
                }
                return Ok(ScheduleReplay::Ready {
                    placement_digest,
                    members,
                });
            }
            Some(SchedulePlacementGroupOutput::Pending {
                attempt: observed_attempt,
                reason,
                observed_at,
                next_poll_at,
                scheduling_deadline: observed_deadline,
            }) => {
                if observed_attempt != attempt
                    || reason.trim().is_empty()
                    || observed_deadline != scheduling_deadline
                    || next_poll_at <= observed_at
                    || next_poll_at > scheduling_deadline
                {
                    return Err(FlowError::Runtime(
                        "placement-group scheduling wait evidence is inconsistent".into(),
                    ));
                }
                let wait_id = format!("placement-group-v2-schedule-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(ScheduleReplay::Command(
                        context.wait_until(wait_id, next_poll_at),
                    ));
                }
                attempt = next_attempt(attempt)?;
            }
            Some(SchedulePlacementGroupOutput::Failed { failed_at, reason }) => {
                if failed_at <= DateTime::<Utc>::UNIX_EPOCH || reason.trim().is_empty() {
                    return Err(FlowError::Runtime(
                        "placement-group scheduling failure evidence is inconsistent".into(),
                    ));
                }
                return Ok(ScheduleReplay::Command(context.fail(reason)));
            }
            Some(SchedulePlacementGroupOutput::Cancelled { cancelled_at }) => {
                return Ok(ScheduleReplay::Command(cancelled(
                    context,
                    input,
                    cancelled_at,
                )));
            }
            None => {
                if let Some(error) = context.step_failed(&step_id) {
                    return Err(FlowError::Runtime(format!(
                        "placement-group scheduling failed: {error}"
                    )));
                }
                return Ok(ScheduleReplay::Command(context.schedule_step_with_retry(
                    step_id,
                    SCHEDULE,
                    serde_json::to_value(SchedulePlacementGroupInput {
                        deployment: input.clone(),
                        attempt,
                        scheduling_deadline,
                    })?,
                    config.retry_policy(context),
                )));
            }
        }
    }
}

fn replay_scheduled_validation(
    config: &DeploymentFlowConfig,
    context: &a3s_flow::WorkflowContext<'_>,
    input: &PlacementGroupDeploymentInput,
    placement_digest: String,
) -> a3s_flow::Result<RuntimeCommand> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("placement-group-v2-await-preparation-{attempt}");
        match context.step_output_as::<ValidateScheduledGroupOutput>(&step_id)? {
            Some(ValidateScheduledGroupOutput::AwaitingPreparation {
                attempt: observed_attempt,
                member_count,
                placement_digest: observed_digest,
                observed_at,
                next_poll_at,
            }) => {
                if observed_attempt != attempt
                    || member_count != input.member_count
                    || observed_digest != placement_digest
                    || next_poll_at <= observed_at
                {
                    return Err(FlowError::Runtime(
                        "placement-group preparation wait evidence is inconsistent".into(),
                    ));
                }
                let wait_id = format!("placement-group-v2-await-preparation-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(context.wait_until(wait_id, next_poll_at));
                }
                attempt = next_attempt(attempt)?;
            }
            Some(ValidateScheduledGroupOutput::Cancelled { cancelled_at }) => {
                return Ok(cancelled(context, input, cancelled_at));
            }
            None => {
                if let Some(error) = context.step_failed(&step_id) {
                    return Err(FlowError::Runtime(format!(
                        "scheduled placement-group validation failed: {error}"
                    )));
                }
                return Ok(context.schedule_step_with_retry(
                    step_id,
                    VALIDATE_SCHEDULING,
                    serde_json::to_value(ValidateScheduledGroupInput {
                        deployment: input.clone(),
                        attempt,
                        placement_digest: placement_digest.clone(),
                    })?,
                    config.retry_policy(context),
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
        VALIDATE_MATERIALIZATION => {
            let input = invocation.input_as::<ValidateMaterializationInput>()?;
            encode(validate_materialization(runtime, input).await?)
        }
        SCHEDULE => {
            let input = invocation.input_as::<SchedulePlacementGroupInput>()?;
            encode(schedule(runtime, input).await?)
        }
        VALIDATE_SCHEDULING => {
            let input = invocation.input_as::<ValidateScheduledGroupInput>()?;
            encode(validate_scheduling(runtime, input).await?)
        }
        step => Err(FlowError::Runtime(format!(
            "Cloud placement-group Deployment v2 workflow has no step {step:?}"
        ))),
    }
}

async fn validate_materialization(
    runtime: &DeploymentFlowRuntime,
    input: ValidateMaterializationInput,
) -> a3s_flow::Result<ValidateMaterializationOutput> {
    validate_input(&input.deployment)?;
    let context = load_context(runtime, &input.deployment).await?;
    if matches!(
        context.deployment.status,
        DeploymentStatus::Cancelling | DeploymentStatus::Cancelled
    ) {
        return Ok(ValidateMaterializationOutput::Cancelled {
            cancelled_at: cancel_before_preparation(runtime, &input.deployment, context).await?,
        });
    }
    if !matches!(
        context.deployment.status,
        DeploymentStatus::Queued | DeploymentStatus::Resolving | DeploymentStatus::Scheduled
    ) {
        return Err(FlowError::Runtime(format!(
            "placement-group Deployment cannot enter scheduling from {}",
            context.deployment.status.as_str()
        )));
    }
    let validated_at = Utc::now().max(context.deployment.updated_at);
    let scheduling_deadline = context
        .deployment
        .requested_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| {
            FlowError::Runtime("placement-group scheduling deadline overflowed".into())
        })?;
    Ok(ValidateMaterializationOutput::Ready {
        validated_at,
        scheduling_deadline,
    })
}

async fn schedule(
    runtime: &DeploymentFlowRuntime,
    input: SchedulePlacementGroupInput,
) -> a3s_flow::Result<SchedulePlacementGroupOutput> {
    validate_input(&input.deployment)?;
    if input.attempt == 0 || input.scheduling_deadline <= DateTime::<Utc>::UNIX_EPOCH {
        return Err(FlowError::Runtime(
            "placement-group scheduling attempt is invalid".into(),
        ));
    }
    let mut context = load_context(runtime, &input.deployment).await?;
    if matches!(
        context.deployment.status,
        DeploymentStatus::Cancelling | DeploymentStatus::Cancelled
    ) {
        return Ok(SchedulePlacementGroupOutput::Cancelled {
            cancelled_at: cancel_before_preparation(runtime, &input.deployment, context).await?,
        });
    }
    if matches!(
        context.deployment.status,
        DeploymentStatus::Failed | DeploymentStatus::Orphaned
    ) {
        return Ok(SchedulePlacementGroupOutput::Failed {
            failed_at: context.deployment.updated_at,
            reason: context
                .deployment
                .failure
                .unwrap_or_else(|| "placement-group scheduling failed".into()),
        });
    }
    if context.deployment.status == DeploymentStatus::Queued {
        runtime
            .workloads
            .mark_resolving(
                context.deployment.id,
                context.deployment.aggregate_version,
                Utc::now().max(context.deployment.updated_at),
            )
            .await
            .map_err(|error| {
                runtime_error("could not resolve placement-group Deployment", error)
            })?;
        context = load_context(runtime, &input.deployment).await?;
    }
    if context.deployment.status == DeploymentStatus::Scheduled {
        let claims = load_group_claims(runtime, &context).await?;
        return ready_output(&context, claims);
    }
    if context.deployment.status != DeploymentStatus::Resolving {
        return Err(FlowError::Runtime(format!(
            "placement-group Deployment cannot schedule from {}",
            context.deployment.status.as_str()
        )));
    }

    let now = Utc::now().max(context.deployment.updated_at);
    let claims = if let Some(claims) = find_group_claims(runtime, &context).await? {
        claims
    } else {
        let nodes = load_schedulable_nodes(runtime, &context, now).await?;
        let member_specs = member_specs(&context)?;
        let edges = member_edges(&context, &member_specs, &nodes)?;
        let Some(matching) = maximum_matching(&edges, nodes.len(), input.attempt) else {
            return pending_or_fail(
                runtime,
                context,
                input,
                now,
                "no distinct ready node set satisfies the complete placement-group plan",
            )
            .await;
        };
        let reservations = context
            .member_bindings
            .iter()
            .zip(matching)
            .map(|(binding, candidate)| {
                let proposed =
                    binding
                        .propose_assignment(candidate.node_id, now)
                        .map_err(|error| {
                            runtime_error("could not propose group member placement", error)
                        })?;
                Ok(ResourceClaimReservation {
                    id: binding.placement_group_resource_claim_id(),
                    binding: proposed,
                    node_id: candidate.node_id,
                    inventory: candidate.inventory,
                    topology_digest: candidate.requirements.topology_digest,
                    slots: candidate.requirements.slots,
                    reserved_at: now,
                })
            })
            .collect::<a3s_flow::Result<Vec<_>>>()?;
        let atomic = AtomicResourceClaimReservation::new(reservations)
            .map_err(|error| runtime_error("could not build atomic group reservation", error))?;
        match runtime.resource_claims.reserve_atomically(atomic).await {
            Ok(result) => ordered_claims(&context, result.value)?,
            Err(error) if is_capacity_unavailable(&error) || is_placement_unavailable(&error) => {
                return pending_or_fail(
                    runtime,
                    context,
                    input,
                    now,
                    "the selected placement group lost capacity or anti-affinity eligibility",
                )
                .await;
            }
            Err(RepositoryError::IdempotencyConflict) => {
                load_group_claims(runtime, &context).await?
            }
            Err(error) => {
                return Err(runtime_error(
                    "could not atomically reserve placement-group resources",
                    error,
                ))
            }
        }
    };
    validate_group_claims(&context, &claims, false)?;

    context = load_context(runtime, &input.deployment).await?;
    if matches!(
        context.deployment.status,
        DeploymentStatus::Cancelling | DeploymentStatus::Cancelled
    ) {
        return Ok(SchedulePlacementGroupOutput::Cancelled {
            cancelled_at: cancel_before_preparation(runtime, &input.deployment, context).await?,
        });
    }
    let scheduled_at = claims
        .iter()
        .fold(now.max(context.deployment.updated_at), |latest, claim| {
            latest.max(claim.created_at)
        });
    let placements = context
        .group
        .members
        .iter()
        .zip(&claims)
        .map(|(plan, claim)| PlacementGroupMemberPlacement {
            ordinal: plan.ordinal,
            member_id: plan.member_id,
            node_id: claim.node_id,
        })
        .collect();
    let write = PlacementGroupSchedulingWrite {
        organization_id: context.deployment.organization_id,
        deployment_id: context.deployment.id,
        expected_deployment_version: context.deployment.aggregate_version,
        group_id: context.group.id,
        group_plan_digest: context.group.plan_digest.clone(),
        placements,
        scheduled_at,
    };
    match runtime.workloads.schedule_placement_group(write).await {
        Ok(_) => {
            let scheduled_context = load_context(runtime, &input.deployment).await?;
            ready_output(&scheduled_context, claims)
        }
        Err(error) => {
            let latest = load_context(runtime, &input.deployment).await?;
            if matches!(
                latest.deployment.status,
                DeploymentStatus::Cancelling | DeploymentStatus::Cancelled
            ) {
                return Ok(SchedulePlacementGroupOutput::Cancelled {
                    cancelled_at: cancel_before_preparation(runtime, &input.deployment, latest)
                        .await?,
                });
            }
            Err(runtime_error(
                "could not persist atomic placement-group scheduling",
                error,
            ))
        }
    }
}

async fn validate_scheduling(
    runtime: &DeploymentFlowRuntime,
    input: ValidateScheduledGroupInput,
) -> a3s_flow::Result<ValidateScheduledGroupOutput> {
    validate_input(&input.deployment)?;
    if input.attempt == 0 || !is_sha256_digest(&input.placement_digest) {
        return Err(FlowError::Runtime(
            "scheduled placement-group validation input is invalid".into(),
        ));
    }
    let context = load_context(runtime, &input.deployment).await?;
    if matches!(
        context.deployment.status,
        DeploymentStatus::Cancelling | DeploymentStatus::Cancelled
    ) {
        return Ok(ValidateScheduledGroupOutput::Cancelled {
            cancelled_at: cancel_before_preparation(runtime, &input.deployment, context).await?,
        });
    }
    if context.deployment.status != DeploymentStatus::Scheduled {
        return Err(FlowError::Runtime(format!(
            "scheduled placement-group changed to {} before Agent preparation",
            context.deployment.status.as_str()
        )));
    }
    let claims = load_group_claims(runtime, &context).await?;
    validate_group_claims(&context, &claims, true)?;
    let members = scheduled_evidence(&context, &claims)?;
    let placement_digest = placement_digest(&members)?;
    if placement_digest != input.placement_digest {
        return Err(FlowError::Runtime(
            "scheduled placement-group changed its exact member placement".into(),
        ));
    }
    let observed_at = Utc::now().max(context.deployment.updated_at);
    let next_poll_at = observed_at
        .checked_add_signed(runtime.config.observation_poll)
        .ok_or_else(|| FlowError::Runtime("placement-group preparation poll overflowed".into()))?;
    Ok(ValidateScheduledGroupOutput::AwaitingPreparation {
        attempt: input.attempt,
        member_count: input.deployment.member_count,
        placement_digest,
        observed_at,
        next_poll_at,
    })
}

async fn load_context(
    runtime: &DeploymentFlowRuntime,
    expected: &PlacementGroupDeploymentInput,
) -> a3s_flow::Result<GroupSchedulingContext> {
    let deployment = runtime
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
    let group_binding = runtime
        .workloads
        .find_deployment_placement_group_binding(expected.organization_id, expected.deployment_id)
        .await
        .map_err(|error| runtime_error("could not load Deployment placement group", error))?;
    let group = runtime
        .workloads
        .find_placement_group(expected.organization_id, expected.group_id)
        .await
        .map_err(|error| runtime_error("could not load placement-group plan", error))?;
    let control = runtime
        .workloads
        .find_workload_control(expected.organization_id, expected.workload_id)
        .await
        .map_err(|error| runtime_error("could not load placement-group policy", error))?;
    let revision = runtime
        .workloads
        .find_revision(expected.organization_id, expected.revision_id)
        .await
        .map_err(|error| runtime_error("could not load placement-group revision", error))?;
    let replica = runtime
        .workloads
        .find_workload_replica(
            expected.organization_id,
            expected.workload_id,
            expected.replica_id,
        )
        .await
        .map_err(|error| runtime_error("could not load placement-group replica", error))?;
    let stored_members = runtime
        .workloads
        .list_workload_replica_members(expected.organization_id, expected.replica_id)
        .await
        .map_err(|error| runtime_error("could not load placement-group members", error))?;
    let member_bindings = runtime
        .workloads
        .list_deployment_replica_member_bindings(expected.organization_id, expected.deployment_id)
        .await
        .map_err(|error| runtime_error("could not load Deployment member bindings", error))?;
    let canonical_binding = member_bindings.first().ok_or_else(|| {
        FlowError::Runtime("placement-group Deployment omitted its leader binding".into())
    })?;
    validate_existing_materialization(
        &deployment,
        canonical_binding,
        &member_bindings,
        Some(&group_binding),
        PlacementTopology::MultiNode,
    )
    .map_err(FlowError::Runtime)?;
    group.validate().map_err(FlowError::Runtime)?;
    replica.validate().map_err(FlowError::Runtime)?;
    let members = group
        .members
        .iter()
        .map(|plan| {
            stored_members
                .iter()
                .find(|member| member.id == plan.member_id)
                .ok_or_else(|| {
                    FlowError::Runtime(
                        "placement-group Deployment member state is incomplete".into(),
                    )
                })
        })
        .collect::<a3s_flow::Result<Vec<_>>>()?;
    if group_binding.group_id != expected.group_id
        || group_binding.group_plan_digest != expected.group_plan_digest
        || group_binding.member_count != expected.member_count
        || group_binding.replica_id != expected.replica_id
        || group_binding.replica_generation != expected.replica_generation
        || group.id != group_binding.group_id
        || group.plan_digest != group_binding.group_plan_digest
        || control.organization_id != expected.organization_id
        || control.workload_id != expected.workload_id
        || group.revision_id != revision.id
        || group.revision_generation != revision.generation
        || replica.organization_id != expected.organization_id
        || replica.project_id != group.project_id
        || replica.environment_id != group.environment_id
        || replica.workload_id != expected.workload_id
        || replica.id != expected.replica_id
        || replica.revision_id != expected.revision_id
        || replica.revision_generation != revision.generation
        || replica.generation != expected.replica_generation
        || group.policy_generation != control.spec.placement_policy.generation()
        || group.placement_policy_digest != control.spec.placement_policy.digest()
        || control.spec.placement_policy.topology() != PlacementTopology::MultiNode
        || control.spec.placement_policy.replica_anti_affinity() != ReplicaAntiAffinity::Required
        || group.members.len() != member_bindings.len()
        || group
            .members
            .iter()
            .zip(&members)
            .zip(&member_bindings)
            .any(|((plan, member), binding)| {
                plan.member_id != binding.member_id
                    || plan.runtime_unit_id != binding.runtime_unit_id
                    || binding.replica_id != group.replica_id
                    || binding.replica_generation != group.replica_generation
                    || group.validate_replica_member_identity(member).is_err()
                    || binding
                        .validate_against_placement_group_member(
                            &deployment,
                            &revision,
                            &replica,
                            member,
                            plan,
                        )
                        .is_err()
            })
    {
        return Err(FlowError::Runtime(
            "placement-group Deployment plan changed after materialization".into(),
        ));
    }
    Ok(GroupSchedulingContext {
        deployment,
        group,
        control,
        revision,
        member_bindings,
    })
}

async fn load_schedulable_nodes(
    runtime: &DeploymentFlowRuntime,
    context: &GroupSchedulingContext,
    now: DateTime<Utc>,
) -> a3s_flow::Result<Vec<SchedulableNode>> {
    let mut nodes = runtime
        .nodes
        .list_scheduling_candidates(
            context.deployment.organization_id,
            context.control.spec.placement_policy.node_pool_id(),
            now,
        )
        .await
        .map_err(|error| runtime_error("could not list group scheduling candidates", error))?;
    nodes.sort_by_key(|node| node.id);
    let mut candidates = Vec::new();
    for node in nodes {
        if !node.accepts_new_work_at(now, runtime.heartbeat_timeout) {
            continue;
        }
        let capabilities = match serde_json::from_value::<RuntimeCapabilities>(
            node.capabilities.document().clone(),
        ) {
            Ok(capabilities) => capabilities,
            Err(error) => {
                tracing::warn!(node_id = %node.id, error = %error, "ignoring invalid Runtime capabilities during group scheduling");
                continue;
            }
        };
        let Some(NodeResourceInventoryRecord { inventory, .. }) = runtime
            .node_control
            .current_resource_inventory(node.id)
            .await
            .map_err(|error| runtime_error("could not load group node inventory", error))?
        else {
            continue;
        };
        inventory
            .validate()
            .map_err(|error| runtime_error("group node inventory is invalid", error))?;
        candidates.push(SchedulableNode {
            node_id: node.id,
            capabilities,
            inventory,
        });
    }
    Ok(candidates)
}

fn member_specs(context: &GroupSchedulingContext) -> a3s_flow::Result<Vec<RuntimeUnitSpec>> {
    context
        .member_bindings
        .iter()
        .zip(&context.group.members)
        .map(|(binding, plan)| {
            project_placement_group_runtime_spec(&context.revision, binding, plan)
                .map_err(|error| runtime_error("could not project group member Runtime", error))
        })
        .collect()
}

fn member_edges(
    context: &GroupSchedulingContext,
    specs: &[RuntimeUnitSpec],
    nodes: &[SchedulableNode],
) -> a3s_flow::Result<Vec<Vec<MemberNodeCandidate>>> {
    context
        .group
        .members
        .iter()
        .zip(specs)
        .map(|(plan, spec)| {
            let resources = ServiceResources {
                cpu_millis: plan.template.resources.cpu_millis,
                memory_bytes: plan.template.resources.memory_bytes,
                pids: plan.template.resources.pids,
                ephemeral_storage_bytes: plan.template.resources.ephemeral_storage_bytes,
            };
            nodes
                .iter()
                .enumerate()
                .filter_map(|(node_index, node)| {
                    let missing = match node.capabilities.missing_for(spec) {
                        Ok(missing) => missing,
                        Err(error) => {
                            return Some(Err(runtime_error(
                                "could not match group Runtime capabilities",
                                error,
                            )))
                        }
                    };
                    if !missing.is_empty() {
                        return None;
                    }
                    let requirements =
                        match CompiledResourceRequirements::compile(&resources, &node.inventory) {
                            Ok(requirements) => requirements,
                            Err(_) => return None,
                        };
                    Some(Ok(MemberNodeCandidate {
                        node_index,
                        node_id: node.node_id,
                        inventory: node.inventory.clone(),
                        requirements,
                    }))
                })
                .collect()
        })
        .collect()
}

fn maximum_matching(
    edges: &[Vec<MemberNodeCandidate>],
    node_count: usize,
    attempt: u32,
) -> Option<Vec<MemberNodeCandidate>> {
    if edges.is_empty() || node_count < edges.len() || edges.iter().any(Vec::is_empty) {
        return None;
    }
    let mut order = (0..edges.len()).collect::<Vec<_>>();
    order.sort_by_key(|member| (edges[*member].len(), *member));
    let mut node_owner = vec![None; node_count];
    let mut member_edge = vec![None; edges.len()];
    let shift = usize::try_from(attempt.saturating_sub(1)).unwrap_or(usize::MAX);
    for member in order {
        let mut visited_nodes = vec![false; node_count];
        if !augment_matching(
            member,
            edges,
            shift,
            &mut visited_nodes,
            &mut node_owner,
            &mut member_edge,
        ) {
            return None;
        }
    }
    member_edge
        .into_iter()
        .enumerate()
        .map(|(member, edge)| edge.map(|edge| edges[member][edge].clone()))
        .collect()
}

fn augment_matching(
    member: usize,
    edges: &[Vec<MemberNodeCandidate>],
    shift: usize,
    visited_nodes: &mut [bool],
    node_owner: &mut [Option<usize>],
    member_edge: &mut [Option<usize>],
) -> bool {
    let edge_count = edges[member].len();
    for offset in 0..edge_count {
        let edge_index = (offset + shift + member) % edge_count;
        let node_index = edges[member][edge_index].node_index;
        if visited_nodes[node_index] {
            continue;
        }
        visited_nodes[node_index] = true;
        let can_assign = match node_owner[node_index] {
            Some(owner) => {
                augment_matching(owner, edges, shift, visited_nodes, node_owner, member_edge)
            }
            None => true,
        };
        if can_assign {
            node_owner[node_index] = Some(member);
            member_edge[member] = Some(edge_index);
            return true;
        }
    }
    false
}

async fn pending_or_fail(
    runtime: &DeploymentFlowRuntime,
    context: GroupSchedulingContext,
    input: SchedulePlacementGroupInput,
    now: DateTime<Utc>,
    reason: &str,
) -> a3s_flow::Result<SchedulePlacementGroupOutput> {
    if now >= input.scheduling_deadline {
        let failed = runtime
            .workloads
            .fail(
                context.deployment.id,
                context.deployment.aggregate_version,
                reason.into(),
                now,
            )
            .await
            .map_err(|error| runtime_error("could not fail placement-group scheduling", error))?;
        return Ok(SchedulePlacementGroupOutput::Failed {
            failed_at: failed.updated_at,
            reason: failed.failure.unwrap_or_else(|| reason.into()),
        });
    }
    let next_poll_at = now
        .checked_add_signed(runtime.config.observation_poll)
        .ok_or_else(|| FlowError::Runtime("placement-group scheduling poll overflowed".into()))?
        .min(input.scheduling_deadline);
    Ok(SchedulePlacementGroupOutput::Pending {
        attempt: input.attempt,
        reason: reason.into(),
        observed_at: now,
        next_poll_at,
        scheduling_deadline: input.scheduling_deadline,
    })
}

async fn load_group_claims(
    runtime: &DeploymentFlowRuntime,
    context: &GroupSchedulingContext,
) -> a3s_flow::Result<Vec<ResourceClaim>> {
    find_group_claims(runtime, context)
        .await?
        .ok_or_else(|| FlowError::Runtime("placement-group has no durable member Claim set".into()))
}

async fn find_group_claims(
    runtime: &DeploymentFlowRuntime,
    context: &GroupSchedulingContext,
) -> a3s_flow::Result<Option<Vec<ResourceClaim>>> {
    let mut claims = Vec::with_capacity(context.member_bindings.len());
    let mut missing = 0_usize;
    for binding in &context.member_bindings {
        match runtime
            .resource_claims
            .find(
                context.deployment.organization_id,
                binding.placement_group_resource_claim_id(),
            )
            .await
        {
            Ok(claim) => claims.push(claim),
            Err(RepositoryError::NotFound) => missing += 1,
            Err(error) => {
                return Err(runtime_error("could not load group member Claim", error));
            }
        }
    }
    if missing == context.member_bindings.len() {
        return Ok(None);
    }
    if missing != 0 {
        return Err(FlowError::Runtime(
            "placement-group durable Claim set is partial".into(),
        ));
    }
    validate_group_claims(
        context,
        &claims,
        context.deployment.status == DeploymentStatus::Scheduled,
    )?;
    Ok(Some(claims))
}

fn ordered_claims(
    context: &GroupSchedulingContext,
    claims: Vec<ResourceClaim>,
) -> a3s_flow::Result<Vec<ResourceClaim>> {
    context
        .member_bindings
        .iter()
        .map(|binding| {
            claims
                .iter()
                .find(|claim| claim.member_id == binding.member_id)
                .cloned()
                .ok_or_else(|| {
                    FlowError::Runtime(
                        "atomic placement-group reservation omitted a member Claim".into(),
                    )
                })
        })
        .collect()
}

fn validate_group_claims(
    context: &GroupSchedulingContext,
    claims: &[ResourceClaim],
    require_persisted_assignment: bool,
) -> a3s_flow::Result<()> {
    if claims.len() != context.member_bindings.len() {
        return Err(FlowError::Runtime(
            "placement-group Claim set is incomplete".into(),
        ));
    }
    let mut node_ids = BTreeSet::new();
    for ((plan, binding), claim) in context
        .group
        .members
        .iter()
        .zip(&context.member_bindings)
        .zip(claims)
    {
        claim
            .validate()
            .map_err(|error| runtime_error("placement-group Claim is invalid", error))?;
        if claim.id != binding.placement_group_resource_claim_id()
            || claim.organization_id != context.deployment.organization_id
            || claim.workload_id != context.deployment.workload_id
            || claim.deployment_id != context.deployment.id
            || claim.replica_id != context.group.replica_id
            || claim.replica_generation != context.group.replica_generation
            || claim.member_id != plan.member_id
            || claim.runtime_unit_id != plan.runtime_unit_id
            || claim.runtime_generation != context.group.replica_generation
            || claim.state == ResourceClaimState::Released
            || !node_ids.insert(claim.node_id)
            || require_persisted_assignment
                && (binding.node_id != Some(claim.node_id)
                    || binding.placement_generation != claim.placement_generation)
        {
            return Err(FlowError::Runtime(
                "placement-group Claim set changed its exact member placement".into(),
            ));
        }
    }
    Ok(())
}

fn ready_output(
    context: &GroupSchedulingContext,
    claims: Vec<ResourceClaim>,
) -> a3s_flow::Result<SchedulePlacementGroupOutput> {
    if context.deployment.status != DeploymentStatus::Scheduled {
        return Err(FlowError::Runtime(
            "placement-group scheduling did not advance the Deployment".into(),
        ));
    }
    validate_group_claims(context, &claims, true)?;
    let members = scheduled_evidence(context, &claims)?;
    let placement_digest = placement_digest(&members)?;
    Ok(SchedulePlacementGroupOutput::Ready {
        members,
        placement_digest,
        scheduled_at: context.deployment.updated_at,
    })
}

fn scheduled_evidence(
    context: &GroupSchedulingContext,
    claims: &[ResourceClaim],
) -> a3s_flow::Result<Vec<ScheduledMemberEvidence>> {
    context
        .group
        .members
        .iter()
        .zip(claims)
        .map(|(plan, claim)| {
            Ok(ScheduledMemberEvidence {
                ordinal: plan.ordinal,
                member_id: plan.member_id,
                node_id: claim.node_id,
                claim_id: claim.id,
                placement_generation: claim.placement_generation,
            })
        })
        .collect()
}

fn placement_digest(members: &[ScheduledMemberEvidence]) -> a3s_flow::Result<String> {
    let encoded = serde_json::to_vec(members).map_err(|error| {
        FlowError::Runtime(format!(
            "could not encode placement-group scheduling evidence: {error}"
        ))
    })?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}

fn validate_scheduled_members(
    members: &[ScheduledMemberEvidence],
    member_count: u32,
    expected_digest: &str,
) -> a3s_flow::Result<()> {
    let mut member_ids = BTreeSet::new();
    let mut node_ids = BTreeSet::new();
    let mut claim_ids = BTreeSet::new();
    if usize::try_from(member_count).ok() != Some(members.len())
        || !is_sha256_digest(expected_digest)
        || placement_digest(members)? != expected_digest
        || members.iter().enumerate().any(|(ordinal, member)| {
            u32::try_from(ordinal).ok() != Some(member.ordinal)
                || member.placement_generation == 0
                || !member_ids.insert(member.member_id)
                || !node_ids.insert(member.node_id)
                || !claim_ids.insert(member.claim_id)
        })
    {
        return Err(FlowError::Runtime(
            "placement-group scheduling evidence is invalid".into(),
        ));
    }
    Ok(())
}

async fn cancel_before_preparation(
    runtime: &DeploymentFlowRuntime,
    expected: &PlacementGroupDeploymentInput,
    context: GroupSchedulingContext,
) -> a3s_flow::Result<DateTime<Utc>> {
    if context.deployment.status == DeploymentStatus::Cancelled {
        return Ok(context
            .deployment
            .cancelled_at
            .unwrap_or(context.deployment.updated_at));
    }
    if context.deployment.status != DeploymentStatus::Cancelling
        || context.deployment.command_id.is_some()
        || context.deployment.cleanup_command_id.is_some()
    {
        return Err(FlowError::Runtime(
            "placement-group cancellation is unsafe after Agent preparation".into(),
        ));
    }
    let mut released_at = Utc::now().max(context.deployment.updated_at);
    for binding in &context.member_bindings {
        let claim_id = binding.placement_group_resource_claim_id();
        let claim = match runtime
            .resource_claims
            .find(context.deployment.organization_id, claim_id)
            .await
        {
            Ok(claim) => claim,
            Err(RepositoryError::NotFound) if binding.node_id.is_none() => continue,
            Err(error) => {
                return Err(runtime_error(
                    "could not load placement-group Claim for cancellation",
                    error,
                ))
            }
        };
        match claim.state {
            ResourceClaimState::Released => {
                released_at = released_at.max(claim.updated_at);
            }
            ResourceClaimState::ReservedInDb => {
                let released = runtime
                    .resource_claims
                    .cancel_database_reservation(
                        claim.organization_id,
                        claim.id,
                        claim.aggregate_version,
                        released_at.max(claim.updated_at),
                    )
                    .await
                    .map_err(|error| {
                        runtime_error("could not cancel placement-group Claim", error)
                    })?;
                released_at = released_at.max(released.updated_at);
            }
            _ => {
                return Err(FlowError::Runtime(
                    "placement-group Claim requires Agent fencing before cancellation".into(),
                ))
            }
        }
    }
    let latest = runtime
        .workloads
        .find_deployment(expected.organization_id, expected.deployment_id)
        .await
        .map_err(|error| runtime_error("could not reload group cancellation", error))?;
    let result = runtime
        .workloads
        .cancel_placement_group(PlacementGroupCancellationWrite {
            organization_id: expected.organization_id,
            deployment_id: expected.deployment_id,
            expected_deployment_version: latest.aggregate_version,
            group_id: expected.group_id,
            group_plan_digest: expected.group_plan_digest.clone(),
            cancelled_at: released_at.max(latest.updated_at),
        })
        .await
        .map_err(|error| runtime_error("could not cancel placement-group Deployment", error))?;
    result
        .value
        .deployment
        .cancelled_at
        .ok_or_else(|| FlowError::Runtime("cancelled placement group omitted its time".into()))
}

fn cancelled(
    context: &a3s_flow::WorkflowContext<'_>,
    input: &PlacementGroupDeploymentInput,
    cancelled_at: DateTime<Utc>,
) -> RuntimeCommand {
    context.complete(serde_json::json!({
        "cancelledAt": cancelled_at,
        "deploymentId": input.deployment_id,
        "state": "cancelled",
    }))
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

fn next_attempt(attempt: u32) -> a3s_flow::Result<u32> {
    attempt
        .checked_add(1)
        .ok_or_else(|| FlowError::Runtime("placement-group scheduling attempt overflowed".into()))
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn encode(value: impl Serialize) -> a3s_flow::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(Into::into)
}

fn runtime_error(context: &str, error: impl std::fmt::Display) -> FlowError {
    FlowError::Runtime(format!("{context}: {error}"))
}

#[cfg(test)]
mod matching_tests {
    use super::*;

    fn candidate(member: usize, node: usize) -> MemberNodeCandidate {
        let _ = member;
        MemberNodeCandidate {
            node_index: node,
            node_id: NodeId::new(),
            inventory: NodeResourceInventory {
                schema: NodeResourceInventory::SCHEMA.into(),
                node_id: uuid::Uuid::new_v4(),
                agent_instance_id: uuid::Uuid::new_v4(),
                generation: 1,
                observed_at: Utc::now(),
                slots: Vec::new(),
                digest: format!("sha256:{}", "0".repeat(64)),
            },
            requirements: CompiledResourceRequirements {
                slots: Vec::new(),
                topology_digest: format!("sha256:{}", "1".repeat(64)),
            },
        }
    }

    #[test]
    fn maximum_matching_reassigns_greedy_choices_and_rotates_retries() {
        let edges = vec![
            vec![candidate(0, 0), candidate(0, 1)],
            vec![candidate(1, 0)],
            vec![candidate(2, 1), candidate(2, 2)],
        ];
        let first = maximum_matching(&edges, 3, 1).expect("complete matching");
        assert_eq!(first[1].node_index, 0);
        assert_eq!(
            first
                .iter()
                .map(|edge| edge.node_index)
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
        let retried = maximum_matching(&edges, 3, 2).expect("rotated matching");
        assert_eq!(
            retried
                .iter()
                .map(|edge| edge.node_index)
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
    }
}
