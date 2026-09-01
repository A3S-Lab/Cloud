use super::owner_snapshot::{
    concurrent_owner_projection_error, owner_snapshot_changed, require_unchanged_owner_snapshot,
};
use crate::modules::shared_kernel::domain::{
    NodeId, OrganizationId, RepositoryError, SecretId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentReplicaBinding, DeploymentStatus, Workload, WorkloadDesiredState,
    WorkloadReplica, WorkloadReplicaMember, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use crate::modules::workloads::published::{
    AuthorizedWorkloadSecretMaterialization, ValidatedSecretMaterializationProjection,
};
use async_trait::async_trait;
use std::collections::BTreeSet;
use std::sync::Arc;

const SECRET_AUTHORIZATION_PROJECTION: &str = "Secret authorization projection";

/// Exact question accepted by the Workloads owner boundary. The caller cannot
/// supply project, environment, Workload, Deployment, or placement evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkloadSecretMaterializationAuthorizationQuery {
    organization_id: OrganizationId,
    node_id: NodeId,
    workload_revision_id: WorkloadRevisionId,
    secret_id: SecretId,
    secret_version: u64,
}

impl WorkloadSecretMaterializationAuthorizationQuery {
    pub fn new(
        organization_id: OrganizationId,
        node_id: NodeId,
        workload_revision_id: WorkloadRevisionId,
        secret_id: SecretId,
        secret_version: u64,
    ) -> Result<Self, String> {
        let query = Self {
            organization_id,
            node_id,
            workload_revision_id,
            secret_id,
            secret_version,
        };
        query.validate()?;
        Ok(query)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.secret_id.as_uuid().is_nil()
            || self.secret_version == 0
        {
            return Err("Workload Secret materialization authorization query is invalid".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub const fn workload_revision_id(&self) -> WorkloadRevisionId {
        self.workload_revision_id
    }

    pub const fn secret_id(&self) -> SecretId {
        self.secret_id
    }

    pub const fn secret_version(&self) -> u64 {
        self.secret_version
    }
}

#[async_trait]
pub trait IWorkloadSecretMaterializationAuthorizationQueryPort: Send + Sync {
    async fn find_authorization(
        &self,
        query: WorkloadSecretMaterializationAuthorizationQuery,
    ) -> Result<Option<AuthorizedWorkloadSecretMaterialization>, RepositoryError>;
}

/// Workloads owner-side policy service. It is the sole component that
/// interprets revision Secret bindings, Deployment lifecycle, and current
/// replica-member assignments for Secret materialization.
pub struct WorkloadSecretMaterializationAuthorizationQueryService {
    workloads: Arc<dyn IWorkloadRepository>,
}

/// Exact Workloads state that granted one node-scoped materialization. The
/// query service reads this state again before publishing evidence so that a
/// mixed or concurrently changing owner projection fails closed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SecretMaterializationOwnerSnapshot {
    revision: WorkloadRevision,
    workload: Workload,
    deployment: Deployment,
    member_bindings: Vec<DeploymentReplicaBinding>,
    replica: WorkloadReplica,
    member: WorkloadReplicaMember,
}

impl SecretMaterializationOwnerSnapshot {
    fn new(
        revision: WorkloadRevision,
        workload: Workload,
        deployment: Deployment,
        mut member_bindings: Vec<DeploymentReplicaBinding>,
        replica: WorkloadReplica,
        member: WorkloadReplicaMember,
    ) -> Self {
        member_bindings.sort_by_key(|binding| binding.member_id);
        Self {
            revision,
            workload,
            deployment,
            member_bindings,
            replica,
            member,
        }
    }
}

impl WorkloadSecretMaterializationAuthorizationQueryService {
    pub fn new(workloads: Arc<dyn IWorkloadRepository>) -> Self {
        Self { workloads }
    }

    async fn require_stable_owner_snapshot(
        &self,
        expected: &SecretMaterializationOwnerSnapshot,
    ) -> Result<(), RepositoryError> {
        let organization_id = expected.workload.organization_id;
        let current_revision = self
            .workloads
            .find_revision(organization_id, expected.revision.id)
            .await
            .map_err(|error| {
                concurrent_owner_projection_error(
                    SECRET_AUTHORIZATION_PROJECTION,
                    "Workload revision",
                    error,
                )
            })?;
        let current_workload = self
            .workloads
            .find_workload(organization_id, expected.workload.id)
            .await
            .map_err(|error| {
                concurrent_owner_projection_error(
                    SECRET_AUTHORIZATION_PROJECTION,
                    "Workload",
                    error,
                )
            })?;
        let current_deployment = self
            .workloads
            .find_deployment(organization_id, expected.deployment.id)
            .await
            .map_err(|error| {
                concurrent_owner_projection_error(
                    SECRET_AUTHORIZATION_PROJECTION,
                    "Deployment",
                    error,
                )
            })?;
        let current_member_bindings = self
            .workloads
            .list_deployment_replica_member_bindings(organization_id, expected.deployment.id)
            .await
            .map_err(|error| {
                concurrent_owner_projection_error(
                    SECRET_AUTHORIZATION_PROJECTION,
                    "replica member bindings",
                    error,
                )
            })?;
        if current_member_bindings.is_empty() {
            return Err(owner_snapshot_changed(SECRET_AUTHORIZATION_PROJECTION));
        }
        let current_replica = self
            .workloads
            .find_workload_replica(organization_id, expected.workload.id, expected.replica.id)
            .await
            .map_err(|error| {
                concurrent_owner_projection_error(
                    SECRET_AUTHORIZATION_PROJECTION,
                    "Workload replica",
                    error,
                )
            })?;
        let current_member = self
            .workloads
            .find_workload_replica_member(organization_id, expected.replica.id, expected.member.id)
            .await
            .map_err(|error| {
                concurrent_owner_projection_error(
                    SECRET_AUTHORIZATION_PROJECTION,
                    "Workload replica member",
                    error,
                )
            })?;
        let current = SecretMaterializationOwnerSnapshot::new(
            current_revision,
            current_workload,
            current_deployment,
            current_member_bindings,
            current_replica,
            current_member,
        );
        ensure_stable_owner_snapshot(expected, &current)
    }

    async fn member_bindings(
        &self,
        deployment: &Deployment,
    ) -> Result<Vec<DeploymentReplicaBinding>, RepositoryError> {
        let mut bindings = match self
            .workloads
            .list_deployment_replica_member_bindings(deployment.organization_id, deployment.id)
            .await
        {
            Ok(bindings) => bindings,
            Err(RepositoryError::NotFound) => Vec::new(),
            Err(error) => return Err(error),
        };
        if bindings.is_empty() {
            return Err(owner_projection_error(
                "runtime-capable Deployment has no replica member binding".into(),
            ));
        }
        bindings.sort_by_key(|binding| binding.member_id);
        Ok(bindings)
    }

    async fn current_node_assignment(
        &self,
        query: &WorkloadSecretMaterializationAuthorizationQuery,
        workload: &Workload,
        revision: &WorkloadRevision,
        deployment: &Deployment,
        bindings: &[DeploymentReplicaBinding],
    ) -> Result<Option<(WorkloadReplica, WorkloadReplicaMember)>, RepositoryError> {
        for binding in bindings
            .iter()
            .filter(|binding| binding.node_id == Some(query.node_id))
        {
            let replica = self
                .workloads
                .find_workload_replica(query.organization_id, workload.id, binding.replica_id)
                .await
                .map_err(|error| referenced_owner_record_error("Workload replica", error))?;
            let member = self
                .workloads
                .find_workload_replica_member(
                    query.organization_id,
                    binding.replica_id,
                    binding.member_id,
                )
                .await
                .map_err(|error| referenced_owner_record_error("Workload replica member", error))?;
            if binding
                .is_current_runtime_assignment(deployment, revision, &replica, &member)
                .map_err(owner_projection_error)?
            {
                return Ok(Some((replica, member)));
            }
        }
        Ok(None)
    }
}

#[async_trait]
impl IWorkloadSecretMaterializationAuthorizationQueryPort
    for WorkloadSecretMaterializationAuthorizationQueryService
{
    async fn find_authorization(
        &self,
        query: WorkloadSecretMaterializationAuthorizationQuery,
    ) -> Result<Option<AuthorizedWorkloadSecretMaterialization>, RepositoryError> {
        query.validate().map_err(owner_query_error)?;
        let revision = match self
            .workloads
            .find_revision(query.organization_id, query.workload_revision_id)
            .await
        {
            Ok(revision) => revision,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        validate_revision(&revision, &query)?;
        if !revision.request.secrets.iter().any(|binding| {
            binding.secret_id == query.secret_id && binding.version == query.secret_version
        }) {
            return Ok(None);
        }

        let workload = match self
            .workloads
            .find_workload(query.organization_id, revision.workload_id)
            .await
        {
            Ok(workload) => workload,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        validate_workload(&workload, &revision, &query)?;
        let deployments = match self
            .workloads
            .list_deployments(query.organization_id, workload.id)
            .await
        {
            Ok(deployments) => deployments,
            Err(RepositoryError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        validate_deployments(&deployments, &workload)?;

        let mut owner_snapshot = None;
        for deployment in deployments.iter().filter(|deployment| {
            deployment.revision_id == revision.id
                && deployment_state_allows_materialization(deployment, &workload, &revision)
        }) {
            let bindings = self.member_bindings(deployment).await?;
            validate_member_bindings(&bindings, deployment, &workload, &revision)?;
            if let Some((replica, member)) = self
                .current_node_assignment(&query, &workload, &revision, deployment, &bindings)
                .await?
            {
                owner_snapshot = Some(SecretMaterializationOwnerSnapshot::new(
                    revision.clone(),
                    workload.clone(),
                    deployment.clone(),
                    bindings,
                    replica,
                    member,
                ));
                break;
            }
        }
        let Some(owner_snapshot) = owner_snapshot else {
            return Ok(None);
        };
        self.require_stable_owner_snapshot(&owner_snapshot).await?;

        AuthorizedWorkloadSecretMaterialization::from_validated_workload(
            ValidatedSecretMaterializationProjection {
                organization_id: owner_snapshot.workload.organization_id,
                project_id: owner_snapshot.workload.project_id,
                environment_id: owner_snapshot.workload.environment_id,
                workload_id: owner_snapshot.workload.id,
                workload_revision_id: owner_snapshot.revision.id,
                node_id: query.node_id,
                secret_id: query.secret_id,
                secret_version: query.secret_version,
            },
        )
        .map(Some)
        .map_err(owner_projection_error)
    }
}

fn deployment_state_allows_materialization(
    deployment: &Deployment,
    workload: &Workload,
    revision: &WorkloadRevision,
) -> bool {
    match deployment.status {
        DeploymentStatus::Scheduled | DeploymentStatus::Applying | DeploymentStatus::Verifying => {
            true
        }
        DeploymentStatus::Retiring | DeploymentStatus::Active => {
            workload.desired_state == WorkloadDesiredState::Running
                && workload.active_revision_id == Some(revision.id)
        }
        _ => false,
    }
}

fn validate_revision(
    revision: &WorkloadRevision,
    query: &WorkloadSecretMaterializationAuthorizationQuery,
) -> Result<(), RepositoryError> {
    revision
        .request
        .validate_request()
        .map_err(owner_projection_error)?;
    let digest = revision
        .request
        .request_digest()
        .map_err(owner_projection_error)?;
    if revision.id != query.workload_revision_id
        || revision.id.as_uuid().is_nil()
        || revision.workload_id.as_uuid().is_nil()
        || revision.generation == 0
        || revision.request_digest != digest
    {
        return Err(owner_projection_error(
            "Workload revision identity or request digest is inconsistent".into(),
        ));
    }
    Ok(())
}

fn validate_workload(
    workload: &Workload,
    revision: &WorkloadRevision,
    query: &WorkloadSecretMaterializationAuthorizationQuery,
) -> Result<(), RepositoryError> {
    if workload.organization_id != query.organization_id
        || workload.id != revision.workload_id
        || workload.id.as_uuid().is_nil()
        || workload.project_id.as_uuid().is_nil()
        || workload.environment_id.as_uuid().is_nil()
        || workload.aggregate_version == 0
    {
        return Err(owner_projection_error(
            "Workload ownership projection is inconsistent".into(),
        ));
    }
    Ok(())
}

fn validate_deployments(
    deployments: &[Deployment],
    workload: &Workload,
) -> Result<(), RepositoryError> {
    let mut deployment_ids = BTreeSet::new();
    if deployments.iter().any(|deployment| {
        deployment.id.as_uuid().is_nil()
            || deployment.organization_id != workload.organization_id
            || deployment.workload_id != workload.id
            || deployment.revision_id.as_uuid().is_nil()
            || deployment.aggregate_version == 0
            || !deployment_ids.insert(deployment.id)
    }) {
        return Err(owner_projection_error(
            "Deployment ownership projection is inconsistent".into(),
        ));
    }
    Ok(())
}

fn validate_member_bindings(
    bindings: &[DeploymentReplicaBinding],
    deployment: &Deployment,
    workload: &Workload,
    revision: &WorkloadRevision,
) -> Result<(), RepositoryError> {
    let mut member_ids = BTreeSet::new();
    let mut runtime_unit_ids = BTreeSet::new();
    if bindings.iter().any(|binding| {
        binding.deployment_id != deployment.id
            || binding.organization_id != workload.organization_id
            || binding.project_id != workload.project_id
            || binding.environment_id != workload.environment_id
            || binding.workload_id != workload.id
            || binding.revision_id != revision.id
            || binding.replica_id.as_uuid().is_nil()
            || binding.replica_generation == 0
            || binding.member_id.as_uuid().is_nil()
            || binding
                .node_id
                .is_some_and(|node_id| node_id.as_uuid().is_nil())
            || binding.placement_generation == 0
            || binding.runtime_unit_id.trim().is_empty()
            || binding.runtime_generation == 0
            || binding.runtime_generation != binding.replica_generation
            || binding.updated_at < binding.created_at
            || !member_ids.insert(binding.member_id)
            || !runtime_unit_ids.insert(binding.runtime_unit_id.as_str())
    }) {
        return Err(owner_projection_error(
            "Deployment member assignment projection is inconsistent".into(),
        ));
    }
    Ok(())
}

fn ensure_stable_owner_snapshot(
    expected: &SecretMaterializationOwnerSnapshot,
    current: &SecretMaterializationOwnerSnapshot,
) -> Result<(), RepositoryError> {
    require_unchanged_owner_snapshot(SECRET_AUTHORIZATION_PROJECTION, expected, current)
}

fn referenced_owner_record_error(label: &str, error: RepositoryError) -> RepositoryError {
    match error {
        RepositoryError::NotFound => owner_projection_error(format!(
            "Deployment replica binding references a missing {label}"
        )),
        error => error,
    }
}

fn owner_query_error(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "Workload Secret materialization authorization query rejected: {error}"
    ))
}

fn owner_projection_error(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "invalid Workloads Secret materialization projection: {error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_runtime_capable_deployment_states_are_authorized() {
        let now = chrono::Utc::now();
        let revision = WorkloadRevision::create(
            WorkloadRevisionId::new(),
            crate::modules::shared_kernel::domain::WorkloadId::new(),
            1,
            test_template(),
            now,
        )
        .expect("revision");
        let mut workload = Workload::create(
            revision.workload_id,
            OrganizationId::new(),
            crate::modules::shared_kernel::domain::ProjectId::new(),
            crate::modules::shared_kernel::domain::EnvironmentId::new(),
            crate::modules::shared_kernel::domain::ResourceName::parse("api").expect("name"),
            now,
        );
        let mut deployment = Deployment::create(
            crate::modules::shared_kernel::domain::DeploymentId::new(),
            workload.organization_id,
            workload.id,
            revision.id,
            crate::modules::shared_kernel::domain::OperationId::new(),
            now,
        );

        for status in [
            DeploymentStatus::Scheduled,
            DeploymentStatus::Applying,
            DeploymentStatus::Verifying,
        ] {
            deployment.status = status;
            assert!(deployment_state_allows_materialization(
                &deployment,
                &workload,
                &revision
            ));
        }
        deployment.status = DeploymentStatus::Active;
        assert!(!deployment_state_allows_materialization(
            &deployment,
            &workload,
            &revision
        ));
        workload.active_revision_id = Some(revision.id);
        assert!(deployment_state_allows_materialization(
            &deployment,
            &workload,
            &revision
        ));
        workload.desired_state = WorkloadDesiredState::Stopped;
        assert!(!deployment_state_allows_materialization(
            &deployment,
            &workload,
            &revision
        ));
    }

    #[test]
    fn placement_group_secondary_requires_a_current_live_assignment_and_stable_snapshot() {
        use crate::modules::workloads::domain::entities::{
            WorkloadPlacementGroupMemberPlan, WorkloadPlacementGroupMemberRole,
        };

        let now = chrono::Utc::now();
        let organization_id = OrganizationId::new();
        let project_id = crate::modules::shared_kernel::domain::ProjectId::new();
        let environment_id = crate::modules::shared_kernel::domain::EnvironmentId::new();
        let workload_id = crate::modules::shared_kernel::domain::WorkloadId::new();
        let workload = Workload::create(
            workload_id,
            organization_id,
            project_id,
            environment_id,
            crate::modules::shared_kernel::domain::ResourceName::parse("grouped-api")
                .expect("name"),
            now,
        );
        let revision = WorkloadRevision::create(
            WorkloadRevisionId::new(),
            workload_id,
            1,
            test_template(),
            now,
        )
        .expect("revision");
        let replica = WorkloadReplica::canonical(&workload, &revision).expect("replica");
        let mut member =
            WorkloadReplicaMember::for_ordinal(&workload, &replica, 1).expect("secondary member");
        let leader_id = NodeId::new();
        let secondary_id = NodeId::new();
        let mut deployment = Deployment::create(
            crate::modules::shared_kernel::domain::DeploymentId::new(),
            organization_id,
            workload_id,
            revision.id,
            crate::modules::shared_kernel::domain::OperationId::new(),
            now,
        );
        let member_template = test_template();
        let plan = WorkloadPlacementGroupMemberPlan {
            member_id: member.id,
            ordinal: member.ordinal,
            role: WorkloadPlacementGroupMemberRole::Worker,
            runtime_unit_id: replica
                .runtime_unit_id_for_member(&revision, &member)
                .expect("member Runtime identity"),
            template_digest: member_template.digest().expect("member template digest"),
            template: member_template,
        };
        let mut binding = DeploymentReplicaBinding::create_for_placement_group_member(
            &deployment,
            &revision,
            &replica,
            &member,
            &plan,
        )
        .expect("unassigned member binding");
        deployment
            .resolve(now + chrono::Duration::milliseconds(1))
            .expect("resolve deployment");
        deployment
            .schedule(leader_id, now + chrono::Duration::milliseconds(2))
            .expect("schedule leader");
        member
            .place(secondary_id, now + chrono::Duration::milliseconds(2))
            .expect("place secondary");
        binding
            .assign_placement_group_member(&deployment, &member, &plan)
            .expect("bind secondary");

        assert!(binding
            .is_current_runtime_assignment(&deployment, &revision, &replica, &member)
            .expect("live assignment"));
        let expected = SecretMaterializationOwnerSnapshot::new(
            revision.clone(),
            workload.clone(),
            deployment.clone(),
            vec![binding.clone()],
            replica.clone(),
            member.clone(),
        );
        let equivalent = SecretMaterializationOwnerSnapshot::new(
            revision.clone(),
            workload.clone(),
            deployment.clone(),
            vec![binding.clone()],
            replica.clone(),
            member.clone(),
        );
        assert!(ensure_stable_owner_snapshot(&expected, &equivalent).is_ok());

        member
            .release_after_fencing(secondary_id, now + chrono::Duration::milliseconds(3))
            .expect("release fenced secondary");
        assert!(!binding
            .is_current_runtime_assignment(&deployment, &revision, &replica, &member)
            .expect("historical assignment"));
        let changed = SecretMaterializationOwnerSnapshot::new(
            revision,
            workload,
            deployment,
            vec![binding],
            replica,
            member,
        );
        assert!(matches!(
            ensure_stable_owner_snapshot(&expected, &changed),
            Err(RepositoryError::Conflict(_))
        ));
    }

    fn test_template() -> crate::modules::workloads::domain::entities::ServiceTemplate {
        use crate::modules::workloads::domain::entities::{
            OciArtifact, ServiceProcess, ServiceResources, ServiceTemplate,
        };
        ServiceTemplate {
            artifact: OciArtifact {
                uri: format!("oci://registry.example/api@sha256:{}", "a".repeat(64)),
                digest: format!("sha256:{}", "a".repeat(64)),
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: ServiceProcess {
                command: Vec::new(),
                args: Vec::new(),
                working_directory: None,
                environment: Default::default(),
            },
            secrets: Vec::new(),
            resources: ServiceResources {
                cpu_millis: 100,
                memory_bytes: 1024,
                pids: 16,
                ephemeral_storage_bytes: None,
            },
            ports: Vec::new(),
            health: None,
        }
    }
}
