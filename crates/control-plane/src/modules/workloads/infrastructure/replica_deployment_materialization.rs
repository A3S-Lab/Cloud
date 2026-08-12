use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, OperationId, RepositoryError,
};
use crate::modules::workloads::application::{
    DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION,
    PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME, PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentPlacementGroupBinding, DeploymentReplicaBinding,
    EffectivePlacementPolicy, Workload, WorkloadDesiredState, WorkloadPlacementGroup,
    WorkloadReplica, WorkloadReplicaLifecycle, WorkloadReplicaMember, WorkloadRevision,
};
use crate::modules::workloads::domain::events::DeploymentRequested;
use crate::modules::workloads::domain::repositories::{
    IWorkloadReplicaDeploymentRepository, ReplicaDeploymentCandidate,
    ReplicaDeploymentMaterialization,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

const DEPLOYMENT_ID_DOMAIN: &str = "a3s.cloud.replica-deployment.v1";
const OPERATION_ID_DOMAIN: &str = "a3s.cloud.replica-deployment-operation.v1";
const CORRELATION_ID_DOMAIN: &str = "a3s.cloud.replica-deployment-materialization.v1";

pub(crate) struct ReplicaDeploymentWrite {
    pub deployment: Deployment,
    pub operation: OperationRequest,
    pub binding: DeploymentReplicaBinding,
    pub member_bindings: Vec<DeploymentReplicaBinding>,
    pub placement_group_binding: Option<DeploymentPlacementGroupBinding>,
    pub event: DomainEventEnvelope,
}

pub(crate) struct PlacementGroupDeploymentContext<'a> {
    pub workload: &'a Workload,
    pub policy: &'a EffectivePlacementPolicy,
    pub revision: &'a WorkloadRevision,
    pub replica: &'a WorkloadReplica,
    pub group: &'a WorkloadPlacementGroup,
    pub members: &'a [WorkloadReplicaMember],
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplicaDeploymentMaterializationReport {
    pub candidates: usize,
    pub created: usize,
    pub replayed: usize,
    pub skipped: usize,
    pub failures: Vec<ReplicaDeploymentMaterializationFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplicaDeploymentMaterializationFailure {
    pub candidate: ReplicaDeploymentCandidate,
    pub message: String,
}

pub struct ReplicaDeploymentMaterializer {
    repository: Arc<dyn IWorkloadReplicaDeploymentRepository>,
    reconcile_interval: Duration,
    batch_size: usize,
}

impl ReplicaDeploymentMaterializer {
    pub fn new(
        repository: Arc<dyn IWorkloadReplicaDeploymentRepository>,
        reconcile_interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if reconcile_interval.is_zero() || batch_size == 0 || batch_size > 10_000 {
            return Err("replica deployment materialization policy is invalid".into());
        }
        Ok(Self {
            repository,
            reconcile_interval,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<ReplicaDeploymentMaterializationReport, RepositoryError> {
        let candidates = self
            .repository
            .pending_replica_deployments(self.batch_size)
            .await?;
        let mut report = ReplicaDeploymentMaterializationReport {
            candidates: candidates.len(),
            ..ReplicaDeploymentMaterializationReport::default()
        };
        for candidate in candidates {
            match self
                .repository
                .materialize_replica_deployment(candidate, now)
                .await
            {
                Ok(Some(materialization)) if materialization.created => report.created += 1,
                Ok(Some(_)) => report.replayed += 1,
                Ok(None) => report.skipped += 1,
                Err(error) => report
                    .failures
                    .push(ReplicaDeploymentMaterializationFailure {
                        candidate,
                        message: error.to_string(),
                    }),
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.reconcile_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(Utc::now()).await {
                        Ok(report) => {
                            for failure in &report.failures {
                                tracing::warn!(
                                    workload_id = %failure.candidate.workload_id,
                                    replica_id = %failure.candidate.replica_id,
                                    replica_generation = failure.candidate.replica_generation,
                                    error = %failure.message,
                                    "replica deployment materialization failed"
                                );
                            }
                            tracing::debug!(
                                candidates = report.candidates,
                                created = report.created,
                                replayed = report.replayed,
                                skipped = report.skipped,
                                failures = report.failures.len(),
                                "replica deployment materialization cycle completed"
                            );
                        }
                        Err(error) => tracing::error!(error = %error, "replica deployment materialization cycle failed"),
                    }
                }
            }
        }
    }
}

pub(crate) fn build_replica_deployment_write(
    candidate: ReplicaDeploymentCandidate,
    workload: &Workload,
    revision: &WorkloadRevision,
    replica: &WorkloadReplica,
    member: &WorkloadReplicaMember,
    requested_at: DateTime<Utc>,
) -> Result<ReplicaDeploymentWrite, String> {
    validate_materialization_context(candidate, workload, revision, replica, member)?;
    let requested_at = canonical_timestamp(
        requested_at
            .max(workload.updated_at)
            .max(revision.created_at)
            .max(replica.updated_at)
            .max(member.updated_at),
    );
    let deployment = Deployment::create(
        replica_deployment_id(candidate),
        workload.organization_id,
        workload.id,
        revision.id,
        replica_operation_id(candidate),
        requested_at,
    );
    let operation = replica_operation(&deployment)?;
    let binding = DeploymentReplicaBinding::create(&deployment, revision, replica, member)?;
    let event = DeploymentRequested::envelope(
        &deployment,
        revision,
        replica_materialization_correlation_id(candidate),
    )
    .map_err(|error| error.to_string())?;
    Ok(ReplicaDeploymentWrite {
        deployment,
        operation,
        binding: binding.clone(),
        member_bindings: vec![binding],
        placement_group_binding: None,
        event,
    })
}

pub(crate) fn build_group_deployment_write(
    candidate: ReplicaDeploymentCandidate,
    context: PlacementGroupDeploymentContext<'_>,
    requested_at: DateTime<Utc>,
) -> Result<ReplicaDeploymentWrite, String> {
    let PlacementGroupDeploymentContext {
        workload,
        policy,
        revision,
        replica,
        group,
        members,
    } = context;
    group.validate_context(workload, policy, revision, replica)?;
    if members.len() != group.members.len() {
        return Err("placement-group Deployment member set is incomplete".into());
    }
    for (member, plan) in members.iter().zip(&group.members) {
        validate_materialization_context(candidate, workload, revision, replica, member)?;
        group.validate_replica_member_identity(member)?;
        if member.id != plan.member_id || member.ordinal != plan.ordinal {
            return Err("placement-group Deployment member plan is inconsistent".into());
        }
    }
    let requested_at = members.iter().fold(
        requested_at
            .max(workload.updated_at)
            .max(revision.created_at)
            .max(replica.updated_at)
            .max(group.updated_at),
        |latest, member| latest.max(member.updated_at),
    );
    let deployment = Deployment::create(
        replica_deployment_id(candidate),
        workload.organization_id,
        workload.id,
        revision.id,
        replica_operation_id(candidate),
        requested_at,
    );
    let member_bindings = members
        .iter()
        .zip(&group.members)
        .map(|(member, plan)| {
            DeploymentReplicaBinding::create_for_placement_group_member(
                &deployment,
                revision,
                replica,
                member,
                plan,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let binding = member_bindings
        .first()
        .cloned()
        .ok_or_else(|| "placement-group Deployment omitted its leader binding".to_string())?;
    let placement_group_binding = DeploymentPlacementGroupBinding::create(
        &deployment,
        revision,
        replica,
        group,
        members,
        &member_bindings,
    )?;
    let operation = group_operation(&deployment, &placement_group_binding)?;
    let event = DeploymentRequested::envelope(
        &deployment,
        revision,
        replica_materialization_correlation_id(candidate),
    )
    .map_err(|error| error.to_string())?;
    Ok(ReplicaDeploymentWrite {
        deployment,
        operation,
        binding,
        member_bindings,
        placement_group_binding: Some(placement_group_binding),
        event,
    })
}

pub(crate) fn materialization_from_existing(
    candidate: ReplicaDeploymentCandidate,
    deployment: Deployment,
    member_bindings: Vec<DeploymentReplicaBinding>,
    placement_group_binding: Option<DeploymentPlacementGroupBinding>,
) -> Result<ReplicaDeploymentMaterialization, String> {
    if deployment.id != replica_deployment_id(candidate)
        || deployment.operation_id != replica_operation_id(candidate)
        || deployment.organization_id != candidate.organization_id
        || deployment.workload_id != candidate.workload_id
        || deployment.revision_id != candidate.revision_id
    {
        return Err("stored replica deployment does not match its deterministic identity".into());
    }
    let operation = match &placement_group_binding {
        Some(binding) => group_operation(&deployment, binding)?,
        None => replica_operation(&deployment)?,
    };
    Ok(ReplicaDeploymentMaterialization {
        candidate,
        deployment,
        operation,
        member_bindings,
        placement_group_binding,
        created: false,
    })
}

pub(crate) fn created_materialization(
    candidate: ReplicaDeploymentCandidate,
    write: ReplicaDeploymentWrite,
) -> ReplicaDeploymentMaterialization {
    ReplicaDeploymentMaterialization {
        candidate,
        deployment: write.deployment,
        operation: write.operation,
        member_bindings: write.member_bindings,
        placement_group_binding: write.placement_group_binding,
        created: true,
    }
}

fn validate_materialization_context(
    candidate: ReplicaDeploymentCandidate,
    workload: &Workload,
    revision: &WorkloadRevision,
    replica: &WorkloadReplica,
    member: &WorkloadReplicaMember,
) -> Result<(), String> {
    replica.validate()?;
    member.validate()?;
    if candidate.organization_id != workload.organization_id
        || candidate.workload_id != workload.id
        || candidate.replica_id != replica.id
        || candidate.replica_ordinal != replica.ordinal
        || candidate.revision_id != revision.id
        || candidate.revision_generation != revision.generation
        || candidate.replica_generation != replica.generation
        || workload.desired_state != WorkloadDesiredState::Running
        || revision.workload_id != workload.id
        || replica.organization_id != workload.organization_id
        || replica.project_id != workload.project_id
        || replica.environment_id != workload.environment_id
        || replica.workload_id != workload.id
        || replica.revision_id != revision.id
        || replica.revision_generation != revision.generation
        || replica.lifecycle != WorkloadReplicaLifecycle::Desired
        || member.organization_id != workload.organization_id
        || member.project_id != workload.project_id
        || member.environment_id != workload.environment_id
        || member.workload_id != workload.id
        || member.replica_id != replica.id
        || member.node_id.is_some()
    {
        return Err("replica deployment materialization context is inconsistent".into());
    }
    Ok(())
}

fn replica_operation(deployment: &Deployment) -> Result<OperationRequest, String> {
    Ok(OperationRequest::new(
        deployment.operation_id,
        deployment.organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid())?,
        WorkflowIdentity::new(DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION)?,
        serde_json::json!({
            "deploymentId": deployment.id,
            "organizationId": deployment.organization_id,
            "revisionId": deployment.revision_id,
            "workloadId": deployment.workload_id,
        }),
        deployment.requested_at,
    ))
}

pub(crate) fn validate_existing_materialization(
    deployment: &Deployment,
    canonical_binding: &DeploymentReplicaBinding,
    member_bindings: &[DeploymentReplicaBinding],
    placement_group_binding: Option<&DeploymentPlacementGroupBinding>,
    topology: crate::modules::workloads::domain::entities::PlacementTopology,
) -> Result<(), String> {
    let first = member_bindings
        .first()
        .ok_or_else(|| "stored Deployment omitted its replica member binding".to_string())?;
    if first != canonical_binding {
        return Err("stored Deployment leader binding is inconsistent".into());
    }
    match (topology, placement_group_binding) {
        (crate::modules::workloads::domain::entities::PlacementTopology::SingleNode, None)
            if member_bindings.len() == 1 =>
        {
            Ok(())
        }
        (
            crate::modules::workloads::domain::entities::PlacementTopology::MultiNode,
            Some(group_binding),
        ) => {
            group_binding.validate()?;
            if group_binding.deployment_id != deployment.id
                || group_binding.organization_id != deployment.organization_id
                || group_binding.workload_id != deployment.workload_id
                || group_binding.revision_id != deployment.revision_id
                || group_binding.replica_id != canonical_binding.replica_id
                || group_binding.replica_generation != canonical_binding.replica_generation
                || usize::try_from(group_binding.member_count).ok() != Some(member_bindings.len())
            {
                return Err("stored placement-group Deployment identity is inconsistent".into());
            }
            let mut member_ids = BTreeSet::new();
            let mut runtime_unit_ids = BTreeSet::new();
            let mut node_ids = BTreeSet::new();
            for binding in member_bindings {
                if binding.deployment_id != deployment.id
                    || binding.organization_id != group_binding.organization_id
                    || binding.project_id != group_binding.project_id
                    || binding.environment_id != group_binding.environment_id
                    || binding.workload_id != group_binding.workload_id
                    || binding.revision_id != group_binding.revision_id
                    || binding.replica_id != group_binding.replica_id
                    || binding.replica_generation != group_binding.replica_generation
                    || binding.runtime_generation != group_binding.replica_generation
                    || !member_ids.insert(binding.member_id)
                    || !runtime_unit_ids.insert(binding.runtime_unit_id.as_str())
                    || binding
                        .node_id
                        .is_some_and(|node_id| !node_ids.insert(node_id))
                {
                    return Err(
                        "stored placement-group Deployment member bindings are inconsistent".into(),
                    );
                }
            }
            let leader_node_id = member_bindings.first().and_then(|binding| binding.node_id);
            let assigned_members = member_bindings
                .iter()
                .filter(|binding| binding.node_id.is_some())
                .count();
            if deployment.node_id != leader_node_id
                || assigned_members != 0 && assigned_members != member_bindings.len()
                || assigned_members == 0 && deployment.node_id.is_some()
                || assigned_members == member_bindings.len()
                    && node_ids.len() != member_bindings.len()
            {
                return Err(
                    "stored placement-group Deployment scheduling shape is inconsistent".into(),
                );
            }
            Ok(())
        }
        _ => Err("stored Deployment execution shape is inconsistent".into()),
    }
}

pub(crate) fn validate_existing_group_materialization_context(
    deployment: &Deployment,
    context: PlacementGroupDeploymentContext<'_>,
    member_bindings: &[DeploymentReplicaBinding],
    group_binding: &DeploymentPlacementGroupBinding,
) -> Result<(), String> {
    let PlacementGroupDeploymentContext {
        workload,
        policy,
        revision,
        replica,
        group,
        members,
    } = context;
    group.validate_context(workload, policy, revision, replica)?;
    group_binding.validate_against(deployment, group)?;
    if members.len() != group.members.len() || member_bindings.len() != group.members.len() {
        return Err("stored placement-group Deployment member set is incomplete".into());
    }
    for ((plan, member), binding) in group.members.iter().zip(members).zip(member_bindings) {
        group.validate_replica_member_identity(member)?;
        if plan.member_id != member.id
            || plan.ordinal != member.ordinal
            || binding.member_id != member.id
        {
            return Err("stored placement-group Deployment member order is inconsistent".into());
        }
        binding
            .validate_against_placement_group_member(deployment, revision, replica, member, plan)?;
    }
    Ok(())
}

fn group_operation(
    deployment: &Deployment,
    binding: &DeploymentPlacementGroupBinding,
) -> Result<OperationRequest, String> {
    binding.validate()?;
    Ok(OperationRequest::new(
        deployment.operation_id,
        deployment.organization_id,
        OperationSubject::new("deployment", deployment.id.as_uuid())?,
        WorkflowIdentity::new(
            PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME,
            PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
        )?,
        serde_json::json!({
            "deploymentId": deployment.id,
            "groupId": binding.group_id,
            "groupPlanDigest": binding.group_plan_digest,
            "memberCount": binding.member_count,
            "organizationId": deployment.organization_id,
            "replicaGeneration": binding.replica_generation,
            "replicaId": binding.replica_id,
            "revisionId": deployment.revision_id,
            "workloadId": deployment.workload_id,
        }),
        deployment.requested_at,
    ))
}

fn replica_deployment_id(candidate: ReplicaDeploymentCandidate) -> DeploymentId {
    let name = format!("{DEPLOYMENT_ID_DOMAIN}:{}", candidate.replica_generation);
    DeploymentId::from_uuid(Uuid::new_v5(
        &candidate.replica_id.as_uuid(),
        name.as_bytes(),
    ))
}

fn replica_operation_id(candidate: ReplicaDeploymentCandidate) -> OperationId {
    let deployment_id = replica_deployment_id(candidate);
    OperationId::from_uuid(Uuid::new_v5(
        &deployment_id.as_uuid(),
        OPERATION_ID_DOMAIN.as_bytes(),
    ))
}

fn replica_materialization_correlation_id(candidate: ReplicaDeploymentCandidate) -> Uuid {
    let deployment_id = replica_deployment_id(candidate);
    Uuid::new_v5(&deployment_id.as_uuid(), CORRELATION_ID_DOMAIN.as_bytes())
}
