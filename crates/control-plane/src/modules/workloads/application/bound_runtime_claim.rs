use super::{
    project_bound_runtime_spec_with_execution, project_placement_group_runtime_spec_with_execution,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, ResourceClaimId, WorkloadId,
    WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    DeploymentPlacementGroupBinding, DeploymentReplicaBinding, DeploymentRuntimeExecutionBinding,
    ResourceClaim, ResourceClaimState, WorkloadPlacementGroup, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::{
    IResourceClaimRepository, IWorkloadPlacementGroupRepository, IWorkloadRepository,
};
use crate::modules::workloads::published::{
    BoundRuntimeClaim, ValidatedBoundRuntimeClaimProjection,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Exact consumer request accepted by the Workloads owner port. Runtime
/// execution semantics are loaded from the Workloads-owned immutable
/// Deployment binding and cannot be supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundRuntimeClaimQuery {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
    resource_claim_id: ResourceClaimId,
}

impl BoundRuntimeClaimQuery {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_id: WorkloadId,
        workload_revision_id: WorkloadRevisionId,
        resource_claim_id: ResourceClaimId,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            environment_id,
            workload_id,
            workload_revision_id,
            resource_claim_id,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.resource_claim_id.as_uuid().is_nil()
        {
            return Err("bound Runtime Claim query identity is invalid".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IBoundRuntimeClaimQueryPort: Send + Sync {
    async fn find_bound_runtime_claim(
        &self,
        query: BoundRuntimeClaimQuery,
    ) -> Result<Option<BoundRuntimeClaim>, RepositoryError>;
}

/// Owner-side query service. It is the only component allowed to interpret a
/// ResourceClaim state and reconstruct the exact bound Runtime specification.
pub struct BoundRuntimeClaimQueryService {
    claims: Arc<dyn IResourceClaimRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
    placement_groups: Arc<dyn IWorkloadPlacementGroupRepository>,
}

impl BoundRuntimeClaimQueryService {
    pub fn new<R>(claims: Arc<dyn IResourceClaimRepository>, workloads: Arc<R>) -> Self
    where
        R: IWorkloadRepository + IWorkloadPlacementGroupRepository + 'static,
    {
        Self {
            claims,
            workloads: workloads.clone(),
            placement_groups: workloads,
        }
    }

    async fn require_stable_owner_snapshot(
        &self,
        claim: &ResourceClaim,
        binding: &DeploymentReplicaBinding,
        revision: &WorkloadRevision,
        runtime_execution_binding: &DeploymentRuntimeExecutionBinding,
        placement_group: Option<(&WorkloadPlacementGroup, &DeploymentPlacementGroupBinding)>,
    ) -> Result<(), RepositoryError> {
        let current_claim = self
            .claims
            .find(claim.organization_id, claim.id)
            .await
            .map_err(|error| concurrent_projection_error("ResourceClaim", error))?;
        let current_bindings = self
            .workloads
            .list_deployment_replica_member_bindings(claim.organization_id, claim.deployment_id)
            .await
            .map_err(|error| concurrent_projection_error("replica member binding", error))?;
        let current_binding =
            exact_deployment_replica_member_binding(current_bindings, claim.member_id)
                .map_err(|_| owner_snapshot_changed())?;
        let current_revision = self
            .workloads
            .find_revision(claim.organization_id, revision.id)
            .await
            .map_err(|error| concurrent_projection_error("Workload revision", error))?;
        let current_runtime_execution_binding = self
            .workloads
            .find_deployment_runtime_execution_binding(claim.organization_id, claim.deployment_id)
            .await
            .map_err(|error| concurrent_projection_error("Runtime execution binding", error))?
            .ok_or_else(owner_snapshot_changed)?;
        if current_claim != *claim
            || current_binding != *binding
            || current_revision != *revision
            || current_runtime_execution_binding != *runtime_execution_binding
        {
            return Err(owner_snapshot_changed());
        }

        match placement_group {
            Some((group, group_binding)) => {
                let current_group = self
                    .placement_groups
                    .find_placement_group_for_replica_generation(
                        claim.organization_id,
                        claim.replica_id,
                        claim.replica_generation,
                    )
                    .await
                    .map_err(|error| concurrent_projection_error("placement group", error))?;
                let current_group_binding = self
                    .workloads
                    .find_deployment_placement_group_binding(
                        claim.organization_id,
                        claim.deployment_id,
                    )
                    .await
                    .map_err(|error| {
                        concurrent_projection_error("Deployment group binding", error)
                    })?;
                if current_group != *group || current_group_binding != *group_binding {
                    return Err(owner_snapshot_changed());
                }
            }
            None => match self
                .placement_groups
                .find_placement_group_for_replica_generation(
                    claim.organization_id,
                    claim.replica_id,
                    claim.replica_generation,
                )
                .await
            {
                Err(RepositoryError::NotFound) => {}
                Ok(_) => return Err(owner_snapshot_changed()),
                Err(error) => return Err(error),
            },
        }
        Ok(())
    }
}

#[async_trait]
impl IBoundRuntimeClaimQueryPort for BoundRuntimeClaimQueryService {
    async fn find_bound_runtime_claim(
        &self,
        query: BoundRuntimeClaimQuery,
    ) -> Result<Option<BoundRuntimeClaim>, RepositoryError> {
        query.validate().map_err(RepositoryError::Conflict)?;
        let claim = match self
            .claims
            .find(query.organization_id, query.resource_claim_id)
            .await
        {
            Ok(claim) => claim,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        claim.validate().map_err(owner_projection_error)?;
        if claim.state != ResourceClaimState::BoundToRuntimeUnit {
            return Ok(None);
        }
        if !claim_matches_query(&claim, &query) {
            return Ok(None);
        }

        let runtime_execution_binding = match self
            .workloads
            .find_deployment_runtime_execution_binding(claim.organization_id, claim.deployment_id)
            .await?
        {
            Some(binding) => binding,
            None => return Ok(None),
        };
        runtime_execution_binding
            .validate()
            .map_err(owner_projection_error)?;
        if !runtime_execution_binding.is_bound() {
            return Ok(None);
        }
        if runtime_execution_binding.deployment_id() != claim.deployment_id
            || runtime_execution_binding.organization_id() != claim.organization_id
            || runtime_execution_binding.project_id() != claim.project_id
            || runtime_execution_binding.environment_id() != claim.environment_id
            || runtime_execution_binding.workload_id() != claim.workload_id
            || runtime_execution_binding.workload_revision_id() != query.workload_revision_id
        {
            return Err(owner_projection_error(
                "bound ResourceClaim drifted from its Runtime execution binding".into(),
            ));
        }

        let bindings = self
            .workloads
            .list_deployment_replica_member_bindings(claim.organization_id, claim.deployment_id)
            .await
            .map_err(|error| match error {
                RepositoryError::NotFound => owner_projection_error(
                    "bound ResourceClaim has no deployment replica member binding".into(),
                ),
                error => error,
            })?;
        let binding = exact_deployment_replica_member_binding(bindings, claim.member_id)?;
        if binding.organization_id != claim.organization_id
            || binding.project_id != claim.project_id
            || binding.environment_id != claim.environment_id
            || binding.workload_id != claim.workload_id
            || binding.revision_id != query.workload_revision_id
            || binding.deployment_id != claim.deployment_id
            || binding.replica_id != claim.replica_id
            || binding.replica_generation != claim.replica_generation
            || binding.member_id != claim.member_id
            || binding.placement_generation != claim.placement_generation
            || binding.node_id != Some(claim.node_id)
            || binding.runtime_unit_id != claim.runtime_unit_id
            || binding.runtime_generation != claim.runtime_generation
        {
            return Err(owner_projection_error(
                "bound ResourceClaim drifted from its deployment replica binding".into(),
            ));
        }

        let revision = self
            .workloads
            .find_revision(claim.organization_id, binding.revision_id)
            .await
            .map_err(|error| match error {
                RepositoryError::NotFound => {
                    owner_projection_error("bound ResourceClaim has no Workload revision".into())
                }
                error => error,
            })?;
        if revision.id != query.workload_revision_id || revision.workload_id != claim.workload_id {
            return Err(owner_projection_error(
                "bound ResourceClaim drifted from its Workload revision".into(),
            ));
        }
        let (runtime_spec, placement_group_evidence) = match self
            .placement_groups
            .find_placement_group_for_replica_generation(
                claim.organization_id,
                claim.replica_id,
                claim.replica_generation,
            )
            .await
        {
            Ok(group) => {
                group.validate().map_err(owner_projection_error)?;
                let group_binding = self
                    .workloads
                    .find_deployment_placement_group_binding(
                        claim.organization_id,
                        claim.deployment_id,
                    )
                    .await
                    .map_err(|error| match error {
                        RepositoryError::NotFound => owner_projection_error(
                            "bound placement-group Claim has no Deployment group binding".into(),
                        ),
                        error => error,
                    })?;
                validate_placement_group_lineage(
                    &claim,
                    &revision,
                    &binding,
                    &group,
                    &group_binding,
                )?;
                let plan = group
                    .members
                    .iter()
                    .find(|plan| plan.member_id == claim.member_id)
                    .ok_or_else(|| {
                        owner_projection_error(
                            "bound placement-group Claim has no exact member plan".into(),
                        )
                    })?;
                (
                    project_placement_group_runtime_spec_with_execution(
                        &revision,
                        &binding,
                        plan,
                        Some(&runtime_execution_binding),
                    )
                    .map_err(owner_projection_error)?,
                    Some((group, group_binding)),
                )
            }
            Err(RepositoryError::NotFound) => (
                project_bound_runtime_spec_with_execution(
                    &revision,
                    &binding,
                    Some(&runtime_execution_binding),
                )
                .map_err(owner_projection_error)?,
                None,
            ),
            Err(error) => return Err(error),
        };
        self.require_stable_owner_snapshot(
            &claim,
            &binding,
            &revision,
            &runtime_execution_binding,
            placement_group_evidence
                .as_ref()
                .map(|(group, group_binding)| (group, group_binding)),
        )
        .await?;
        let resource_binding_digest = claim
            .prepared_binding_digest
            .clone()
            .ok_or_else(|| owner_projection_error("bound ResourceClaim omitted evidence".into()))?;

        BoundRuntimeClaim::from_validated_claim(ValidatedBoundRuntimeClaimProjection {
            organization_id: claim.organization_id,
            project_id: claim.project_id,
            environment_id: claim.environment_id,
            workload_id: claim.workload_id,
            workload_revision_id: binding.revision_id,
            resource_claim_id: claim.id,
            resource_claim_generation: claim.claim_generation,
            resource_claim_aggregate_version: claim.aggregate_version,
            resource_claim_digest: claim.claim_digest,
            resource_binding_digest,
            node_id: claim.node_id,
            runtime_spec,
        })
        .map(Some)
        .map_err(owner_projection_error)
    }
}

fn claim_matches_query(claim: &ResourceClaim, query: &BoundRuntimeClaimQuery) -> bool {
    claim.organization_id == query.organization_id
        && claim.project_id == query.project_id
        && claim.environment_id == query.environment_id
        && claim.workload_id == query.workload_id
        && claim.id == query.resource_claim_id
}

fn exact_deployment_replica_member_binding(
    bindings: Vec<DeploymentReplicaBinding>,
    member_id: WorkloadReplicaMemberId,
) -> Result<DeploymentReplicaBinding, RepositoryError> {
    let mut matching = bindings
        .into_iter()
        .filter(|binding| binding.member_id == member_id);
    let binding = matching.next().ok_or_else(|| {
        owner_projection_error(
            "bound ResourceClaim has no deployment replica member binding".into(),
        )
    })?;
    if matching.next().is_some() {
        return Err(owner_projection_error(
            "bound ResourceClaim has ambiguous deployment replica member bindings".into(),
        ));
    }
    Ok(binding)
}

fn validate_placement_group_lineage(
    claim: &ResourceClaim,
    revision: &WorkloadRevision,
    binding: &DeploymentReplicaBinding,
    group: &WorkloadPlacementGroup,
    group_binding: &DeploymentPlacementGroupBinding,
) -> Result<(), RepositoryError> {
    group_binding.validate().map_err(owner_projection_error)?;
    if group.organization_id != claim.organization_id
        || group.project_id != claim.project_id
        || group.environment_id != claim.environment_id
        || group.workload_id != claim.workload_id
        || group.revision_id != revision.id
        || group.revision_generation != revision.generation
        || group.replica_id != claim.replica_id
        || group.replica_generation != claim.replica_generation
        || group_binding.deployment_id != claim.deployment_id
        || group_binding.organization_id != claim.organization_id
        || group_binding.project_id != claim.project_id
        || group_binding.environment_id != claim.environment_id
        || group_binding.workload_id != claim.workload_id
        || group_binding.revision_id != revision.id
        || group_binding.revision_generation != revision.generation
        || group_binding.replica_id != claim.replica_id
        || group_binding.replica_generation != claim.replica_generation
        || group_binding.group_id != group.id
        || group_binding.group_plan_digest != group.plan_digest
        || usize::try_from(group_binding.member_count).ok() != Some(group.members.len())
        || binding.member_id != claim.member_id
    {
        return Err(owner_projection_error(
            "bound ResourceClaim drifted from its placement-group plan".into(),
        ));
    }
    Ok(())
}

fn concurrent_projection_error(label: &str, error: RepositoryError) -> RepositoryError {
    match error {
        RepositoryError::NotFound | RepositoryError::Conflict(_) => RepositoryError::Conflict(
            format!("Workloads {label} changed during Runtime evidence projection"),
        ),
        error => error,
    }
}

fn owner_snapshot_changed() -> RepositoryError {
    RepositoryError::Conflict(
        "Workloads owner state changed during Runtime evidence projection".into(),
    )
}

fn owner_projection_error(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "invalid Workloads Runtime Claim projection: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        DeploymentId, EnvironmentId, NodeId, OrganizationId, ProjectId, WorkloadId,
        WorkloadReplicaId, WorkloadRevisionId,
    };
    use chrono::Utc;

    fn binding(member_id: WorkloadReplicaMemberId) -> DeploymentReplicaBinding {
        let at = Utc::now();
        DeploymentReplicaBinding {
            deployment_id: DeploymentId::new(),
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            workload_id: WorkloadId::new(),
            revision_id: WorkloadRevisionId::new(),
            replica_id: WorkloadReplicaId::new(),
            replica_generation: 1,
            member_id,
            node_id: Some(NodeId::new()),
            placement_generation: 1,
            runtime_unit_id: format!("runtime-unit-{member_id}"),
            runtime_generation: 1,
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn selects_the_exact_non_leader_placement_group_member() {
        let leader = binding(WorkloadReplicaMemberId::new());
        let expected = binding(WorkloadReplicaMemberId::new());

        let selected = exact_deployment_replica_member_binding(
            vec![leader, expected.clone()],
            expected.member_id,
        )
        .expect("select exact placement-group member");

        assert_eq!(selected, expected);
    }

    #[test]
    fn rejects_ambiguous_replica_member_evidence() {
        let expected = binding(WorkloadReplicaMemberId::new());
        let duplicate = expected.clone();

        let error = exact_deployment_replica_member_binding(
            vec![expected.clone(), duplicate],
            expected.member_id,
        )
        .expect_err("ambiguous member evidence must fail closed");

        assert!(error
            .to_string()
            .contains("ambiguous deployment replica member bindings"));
    }
}
