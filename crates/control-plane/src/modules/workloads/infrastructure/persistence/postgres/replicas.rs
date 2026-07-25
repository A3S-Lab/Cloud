use super::schema::{
    DeploymentReplicaBindings, WorkloadControls, WorkloadReplicaMembers, WorkloadReplicas,
};
use crate::infrastructure::{execute, fetch_optional, require_one_row, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, NodeId, OrganizationId, ProjectId, RepositoryError, WorkloadId,
    WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentReplicaBinding, EffectivePlacementPolicy, ManagedOwnerKind,
    ManagedOwnerReference, Workload, WorkloadControl, WorkloadControlSpec, WorkloadReplica,
    WorkloadReplicaMember, WorkloadRevision,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, update_table, Database, DecodeError, Expression, FromRow, FromValue,
    PostgresDialect, PostgresExecutor, PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct ControlSelection;
struct ReplicaSelection;
struct MemberSelection;
struct BindingSelection;

impl Selection for ControlSelection {
    type Output = ControlRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            WorkloadControls::workload_id().expression(),
            WorkloadControls::organization_id().expression(),
            WorkloadControls::project_id().expression(),
            WorkloadControls::environment_id().expression(),
            WorkloadControls::managed_owner_kind().expression(),
            WorkloadControls::managed_owner_id().expression(),
            WorkloadControls::managed_owner_generation().expression(),
            WorkloadControls::managed_owner_spec_digest().expression(),
            WorkloadControls::placement_policy().expression(),
            WorkloadControls::placement_policy_digest().expression(),
            WorkloadControls::aggregate_version().expression(),
            WorkloadControls::created_at().expression(),
            WorkloadControls::updated_at().expression(),
        ]
    }
}

impl Selection for ReplicaSelection {
    type Output = ReplicaRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            WorkloadReplicas::id().expression(),
            WorkloadReplicas::organization_id().expression(),
            WorkloadReplicas::project_id().expression(),
            WorkloadReplicas::environment_id().expression(),
            WorkloadReplicas::workload_id().expression(),
            WorkloadReplicas::ordinal().expression(),
            WorkloadReplicas::revision_id().expression(),
            WorkloadReplicas::generation().expression(),
            WorkloadReplicas::aggregate_version().expression(),
            WorkloadReplicas::created_at().expression(),
            WorkloadReplicas::updated_at().expression(),
        ]
    }
}

impl Selection for MemberSelection {
    type Output = MemberRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            WorkloadReplicaMembers::id().expression(),
            WorkloadReplicaMembers::organization_id().expression(),
            WorkloadReplicaMembers::project_id().expression(),
            WorkloadReplicaMembers::environment_id().expression(),
            WorkloadReplicaMembers::workload_id().expression(),
            WorkloadReplicaMembers::replica_id().expression(),
            WorkloadReplicaMembers::ordinal().expression(),
            WorkloadReplicaMembers::node_id().expression(),
            WorkloadReplicaMembers::placement_generation().expression(),
            WorkloadReplicaMembers::aggregate_version().expression(),
            WorkloadReplicaMembers::created_at().expression(),
            WorkloadReplicaMembers::updated_at().expression(),
        ]
    }
}

impl Selection for BindingSelection {
    type Output = BindingRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            DeploymentReplicaBindings::deployment_id().expression(),
            DeploymentReplicaBindings::organization_id().expression(),
            DeploymentReplicaBindings::project_id().expression(),
            DeploymentReplicaBindings::environment_id().expression(),
            DeploymentReplicaBindings::workload_id().expression(),
            DeploymentReplicaBindings::revision_id().expression(),
            DeploymentReplicaBindings::replica_id().expression(),
            DeploymentReplicaBindings::replica_generation().expression(),
            DeploymentReplicaBindings::member_id().expression(),
            DeploymentReplicaBindings::node_id().expression(),
            DeploymentReplicaBindings::placement_generation().expression(),
            DeploymentReplicaBindings::runtime_unit_id().expression(),
            DeploymentReplicaBindings::runtime_generation().expression(),
            DeploymentReplicaBindings::created_at().expression(),
            DeploymentReplicaBindings::updated_at().expression(),
        ]
    }
}

struct ControlRow {
    workload_id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    managed_owner_kind: Option<String>,
    managed_owner_id: Option<Uuid>,
    managed_owner_generation: Option<u64>,
    managed_owner_spec_digest: Option<String>,
    placement_policy: serde_json::Value,
    placement_policy_digest: String,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct ReplicaRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    workload_id: Uuid,
    ordinal: u32,
    revision_id: Uuid,
    generation: u64,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct MemberRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    workload_id: Uuid,
    replica_id: Uuid,
    ordinal: u32,
    node_id: Option<Uuid>,
    placement_generation: u64,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct BindingRow {
    deployment_id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    workload_id: Uuid,
    revision_id: Uuid,
    replica_id: Uuid,
    replica_generation: u64,
    member_id: Uuid,
    node_id: Option<Uuid>,
    placement_generation: u64,
    runtime_unit_id: String,
    runtime_generation: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

macro_rules! from_row {
    ($row:ty, { $($field:ident: $index:literal),+ $(,)? }) => {
        impl FromRow for $row {
            fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
                Ok(Self { $($field: decode(row, $index)?,)+ })
            }
        }
    };
}

from_row!(ControlRow, {
    workload_id: 0, organization_id: 1, project_id: 2, environment_id: 3,
    managed_owner_kind: 4, managed_owner_id: 5, managed_owner_generation: 6,
    managed_owner_spec_digest: 7, placement_policy: 8, placement_policy_digest: 9,
    aggregate_version: 10, created_at: 11, updated_at: 12,
});
from_row!(ReplicaRow, {
    id: 0, organization_id: 1, project_id: 2, environment_id: 3, workload_id: 4,
    ordinal: 5, revision_id: 6, generation: 7, aggregate_version: 8, created_at: 9,
    updated_at: 10,
});
from_row!(MemberRow, {
    id: 0, organization_id: 1, project_id: 2, environment_id: 3, workload_id: 4,
    replica_id: 5, ordinal: 6, node_id: 7, placement_generation: 8,
    aggregate_version: 9, created_at: 10, updated_at: 11,
});
from_row!(BindingRow, {
    deployment_id: 0, organization_id: 1, project_id: 2, environment_id: 3,
    workload_id: 4, revision_id: 5, replica_id: 6, replica_generation: 7,
    member_id: 8, node_id: 9, placement_generation: 10, runtime_unit_id: 11,
    runtime_generation: 12, created_at: 13, updated_at: 14,
});

pub(super) async fn record_generation(
    transaction: &PostgresTransaction,
    workload: &Workload,
    control_spec: &WorkloadControlSpec,
    revision: &WorkloadRevision,
    deployment: &Deployment,
) -> Result<DeploymentReplicaBinding, PostgresPersistenceError> {
    let (replica, member) =
        match control_in_transaction(transaction, workload.organization_id, workload.id).await? {
            Some(control) => {
                control.validate_against(workload).map_err(invariant)?;
                control
                    .require_authority(control_spec)
                    .map_err(RepositoryError::Conflict)?;
                let replica_id = WorkloadReplicaId::from_uuid(workload.id.as_uuid());
                let mut replica = replica_in_transaction(
                    transaction,
                    workload.organization_id,
                    workload.id,
                    replica_id,
                )
                .await?
                .ok_or_else(|| invariant("Workload is missing its canonical replica"))?;
                let previous_version = replica.aggregate_version;
                replica
                    .advance(revision, revision.created_at)
                    .map_err(RepositoryError::Conflict)?;
                persist_replica(transaction, &replica, previous_version).await?;
                let member_id = WorkloadReplicaMemberId::from_uuid(workload.id.as_uuid());
                let member = member_in_transaction(
                    transaction,
                    workload.organization_id,
                    replica.id,
                    member_id,
                )
                .await?
                .ok_or_else(|| invariant("Workload is missing its canonical replica member"))?;
                (replica, member)
            }
            None => {
                let control =
                    WorkloadControl::create(workload, control_spec.clone()).map_err(invariant)?;
                let replica = WorkloadReplica::canonical(workload, revision).map_err(invariant)?;
                let member =
                    WorkloadReplicaMember::canonical(workload, &replica).map_err(invariant)?;
                insert_control(transaction, &control).await?;
                insert_replica(transaction, &replica).await?;
                insert_member(transaction, &member).await?;
                (replica, member)
            }
        };
    let binding = DeploymentReplicaBinding::create(deployment, revision, &replica, &member)
        .map_err(invariant)?;
    insert_binding(transaction, &binding).await?;
    Ok(binding)
}

pub(super) async fn place(
    transaction: &PostgresTransaction,
    deployment: &Deployment,
) -> Result<DeploymentReplicaBinding, PostgresPersistenceError> {
    let node_id = deployment
        .node_id
        .ok_or_else(|| invariant("scheduled deployment omitted its node"))?;
    let mut binding =
        binding_in_transaction(transaction, deployment.organization_id, deployment.id)
            .await?
            .ok_or_else(|| invariant("deployment is missing its canonical replica binding"))?;
    let mut member = member_in_transaction(
        transaction,
        deployment.organization_id,
        binding.replica_id,
        binding.member_id,
    )
    .await?
    .ok_or_else(|| invariant("deployment replica binding references a missing member"))?;
    let previous_member_version = member.aggregate_version;
    member
        .place(node_id, deployment.updated_at)
        .map_err(RepositoryError::Conflict)?;
    binding
        .assign(deployment, &member)
        .map_err(RepositoryError::Conflict)?;
    let member_rows = execute(
        transaction,
        update_table::<WorkloadReplicaMembers>()
            .set(
                WorkloadReplicaMembers::node_id(),
                member.node_id.map(NodeId::as_uuid),
            )
            .set(
                WorkloadReplicaMembers::placement_generation(),
                member.placement_generation,
            )
            .set(
                WorkloadReplicaMembers::aggregate_version(),
                member.aggregate_version,
            )
            .set(WorkloadReplicaMembers::updated_at(), member.updated_at)
            .filter(WorkloadReplicaMembers::id().eq(member.id.as_uuid()))
            .filter(WorkloadReplicaMembers::aggregate_version().eq(previous_member_version)),
    )
    .await?;
    require_one_row("Workload replica member placement", member_rows)?;
    let binding_rows = execute(
        transaction,
        update_table::<DeploymentReplicaBindings>()
            .set(
                DeploymentReplicaBindings::node_id(),
                binding.node_id.map(NodeId::as_uuid),
            )
            .set(
                DeploymentReplicaBindings::placement_generation(),
                binding.placement_generation,
            )
            .set(DeploymentReplicaBindings::updated_at(), binding.updated_at)
            .filter(DeploymentReplicaBindings::deployment_id().eq(binding.deployment_id.as_uuid())),
    )
    .await?;
    require_one_row("deployment replica placement binding", binding_rows)?;
    Ok(binding)
}

pub(super) async fn require_direct_mutation(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<(), PostgresPersistenceError> {
    control_in_transaction(transaction, organization_id, workload_id)
        .await?
        .ok_or_else(|| invariant("Workload is missing its durable control record"))?
        .require_direct_mutation()
        .map_err(RepositoryError::Conflict)?;
    Ok(())
}

pub(super) async fn control_spec_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<WorkloadControlSpec, PostgresPersistenceError> {
    Ok(
        control_in_transaction(transaction, organization_id, workload_id)
            .await?
            .ok_or_else(|| invariant("Workload is missing its durable control record"))?
            .spec,
    )
}

pub(super) async fn find_control(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<WorkloadControl, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<WorkloadControls>()
                .select(ControlSelection)
                .filter(WorkloadControls::organization_id().eq(organization_id.as_uuid()))
                .filter(WorkloadControls::workload_id().eq(workload_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)
        .and_then(ControlRow::control)
}

pub(super) async fn find_replica(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    replica_id: WorkloadReplicaId,
) -> Result<WorkloadReplica, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<WorkloadReplicas>()
                .select(ReplicaSelection)
                .filter(WorkloadReplicas::organization_id().eq(organization_id.as_uuid()))
                .filter(WorkloadReplicas::workload_id().eq(workload_id.as_uuid()))
                .filter(WorkloadReplicas::id().eq(replica_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)
        .and_then(ReplicaRow::replica)
}

pub(super) async fn find_member(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    replica_id: WorkloadReplicaId,
    member_id: WorkloadReplicaMemberId,
) -> Result<WorkloadReplicaMember, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<WorkloadReplicaMembers>()
                .select(MemberSelection)
                .filter(WorkloadReplicaMembers::organization_id().eq(organization_id.as_uuid()))
                .filter(WorkloadReplicaMembers::replica_id().eq(replica_id.as_uuid()))
                .filter(WorkloadReplicaMembers::id().eq(member_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)
        .and_then(MemberRow::member)
}

pub(super) async fn find_binding(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> Result<DeploymentReplicaBinding, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<DeploymentReplicaBindings>()
                .select(BindingSelection)
                .filter(DeploymentReplicaBindings::organization_id().eq(organization_id.as_uuid()))
                .filter(DeploymentReplicaBindings::deployment_id().eq(deployment_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)
        .and_then(BindingRow::binding)
}

async fn control_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
) -> Result<Option<WorkloadControl>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        select_from::<WorkloadControls>()
            .select(ControlSelection)
            .filter(WorkloadControls::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkloadControls::workload_id().eq(workload_id.as_uuid())),
    )
    .await?
    .map(ControlRow::control)
    .transpose()
    .map_err(Into::into)
}

async fn replica_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    replica_id: WorkloadReplicaId,
) -> Result<Option<WorkloadReplica>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        select_from::<WorkloadReplicas>()
            .select(ReplicaSelection)
            .filter(WorkloadReplicas::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkloadReplicas::workload_id().eq(workload_id.as_uuid()))
            .filter(WorkloadReplicas::id().eq(replica_id.as_uuid())),
    )
    .await?
    .map(ReplicaRow::replica)
    .transpose()
    .map_err(Into::into)
}

async fn member_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    replica_id: WorkloadReplicaId,
    member_id: WorkloadReplicaMemberId,
) -> Result<Option<WorkloadReplicaMember>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        select_from::<WorkloadReplicaMembers>()
            .select(MemberSelection)
            .filter(WorkloadReplicaMembers::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkloadReplicaMembers::replica_id().eq(replica_id.as_uuid()))
            .filter(WorkloadReplicaMembers::id().eq(member_id.as_uuid())),
    )
    .await?
    .map(MemberRow::member)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn binding_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> Result<Option<DeploymentReplicaBinding>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        select_from::<DeploymentReplicaBindings>()
            .select(BindingSelection)
            .filter(DeploymentReplicaBindings::organization_id().eq(organization_id.as_uuid()))
            .filter(DeploymentReplicaBindings::deployment_id().eq(deployment_id.as_uuid())),
    )
    .await?
    .map(BindingRow::binding)
    .transpose()
    .map_err(Into::into)
}

async fn insert_control(
    transaction: &PostgresTransaction,
    control: &WorkloadControl,
) -> Result<(), PostgresPersistenceError> {
    let (owner_kind, owner_id, owner_generation, owner_spec_digest) =
        match &control.spec.managed_owner {
            Some(owner) => (
                Some(owner.kind().as_str().to_owned()),
                Some(owner.owner_id()),
                Some(owner.owner_generation()),
                Some(owner.owner_spec_digest().to_owned()),
            ),
            None => (None, None, None, None),
        };
    let rows = execute(
        transaction,
        insert_into::<WorkloadControls>()
            .value(
                WorkloadControls::workload_id(),
                control.workload_id.as_uuid(),
            )
            .value(
                WorkloadControls::organization_id(),
                control.organization_id.as_uuid(),
            )
            .value(WorkloadControls::project_id(), control.project_id.as_uuid())
            .value(
                WorkloadControls::environment_id(),
                control.environment_id.as_uuid(),
            )
            .value(WorkloadControls::managed_owner_kind(), owner_kind)
            .value(WorkloadControls::managed_owner_id(), owner_id)
            .value(
                WorkloadControls::managed_owner_generation(),
                owner_generation,
            )
            .value(
                WorkloadControls::managed_owner_spec_digest(),
                owner_spec_digest,
            )
            .value(
                WorkloadControls::placement_policy(),
                control
                    .spec
                    .placement_policy
                    .document()
                    .map_err(invariant)?,
            )
            .value(
                WorkloadControls::placement_policy_digest(),
                control.spec.placement_policy.digest(),
            )
            .value(
                WorkloadControls::aggregate_version(),
                control.aggregate_version,
            )
            .value(WorkloadControls::created_at(), control.created_at)
            .value(WorkloadControls::updated_at(), control.updated_at),
    )
    .await?;
    require_one_row("Workload control", rows)
}

async fn insert_replica(
    transaction: &PostgresTransaction,
    replica: &WorkloadReplica,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        insert_into::<WorkloadReplicas>()
            .value(WorkloadReplicas::id(), replica.id.as_uuid())
            .value(
                WorkloadReplicas::organization_id(),
                replica.organization_id.as_uuid(),
            )
            .value(WorkloadReplicas::project_id(), replica.project_id.as_uuid())
            .value(
                WorkloadReplicas::environment_id(),
                replica.environment_id.as_uuid(),
            )
            .value(
                WorkloadReplicas::workload_id(),
                replica.workload_id.as_uuid(),
            )
            .value(WorkloadReplicas::ordinal(), replica.ordinal)
            .value(
                WorkloadReplicas::revision_id(),
                replica.revision_id.as_uuid(),
            )
            .value(WorkloadReplicas::generation(), replica.generation)
            .value(
                WorkloadReplicas::aggregate_version(),
                replica.aggregate_version,
            )
            .value(WorkloadReplicas::created_at(), replica.created_at)
            .value(WorkloadReplicas::updated_at(), replica.updated_at),
    )
    .await?;
    require_one_row("Workload replica", rows)
}

async fn insert_member(
    transaction: &PostgresTransaction,
    member: &WorkloadReplicaMember,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        insert_into::<WorkloadReplicaMembers>()
            .value(WorkloadReplicaMembers::id(), member.id.as_uuid())
            .value(
                WorkloadReplicaMembers::organization_id(),
                member.organization_id.as_uuid(),
            )
            .value(
                WorkloadReplicaMembers::project_id(),
                member.project_id.as_uuid(),
            )
            .value(
                WorkloadReplicaMembers::environment_id(),
                member.environment_id.as_uuid(),
            )
            .value(
                WorkloadReplicaMembers::workload_id(),
                member.workload_id.as_uuid(),
            )
            .value(
                WorkloadReplicaMembers::replica_id(),
                member.replica_id.as_uuid(),
            )
            .value(WorkloadReplicaMembers::ordinal(), member.ordinal)
            .value(
                WorkloadReplicaMembers::node_id(),
                member.node_id.map(NodeId::as_uuid),
            )
            .value(
                WorkloadReplicaMembers::placement_generation(),
                member.placement_generation,
            )
            .value(
                WorkloadReplicaMembers::aggregate_version(),
                member.aggregate_version,
            )
            .value(WorkloadReplicaMembers::created_at(), member.created_at)
            .value(WorkloadReplicaMembers::updated_at(), member.updated_at),
    )
    .await?;
    require_one_row("Workload replica member", rows)
}

async fn persist_replica(
    transaction: &PostgresTransaction,
    replica: &WorkloadReplica,
    previous_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        update_table::<WorkloadReplicas>()
            .set(
                WorkloadReplicas::revision_id(),
                replica.revision_id.as_uuid(),
            )
            .set(WorkloadReplicas::generation(), replica.generation)
            .set(
                WorkloadReplicas::aggregate_version(),
                replica.aggregate_version,
            )
            .set(WorkloadReplicas::updated_at(), replica.updated_at)
            .filter(WorkloadReplicas::id().eq(replica.id.as_uuid()))
            .filter(WorkloadReplicas::aggregate_version().eq(previous_version)),
    )
    .await?;
    require_one_row("Workload replica generation", rows)
}

async fn insert_binding(
    transaction: &PostgresTransaction,
    binding: &DeploymentReplicaBinding,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        insert_into::<DeploymentReplicaBindings>()
            .value(
                DeploymentReplicaBindings::deployment_id(),
                binding.deployment_id.as_uuid(),
            )
            .value(
                DeploymentReplicaBindings::organization_id(),
                binding.organization_id.as_uuid(),
            )
            .value(
                DeploymentReplicaBindings::project_id(),
                binding.project_id.as_uuid(),
            )
            .value(
                DeploymentReplicaBindings::environment_id(),
                binding.environment_id.as_uuid(),
            )
            .value(
                DeploymentReplicaBindings::workload_id(),
                binding.workload_id.as_uuid(),
            )
            .value(
                DeploymentReplicaBindings::revision_id(),
                binding.revision_id.as_uuid(),
            )
            .value(
                DeploymentReplicaBindings::replica_id(),
                binding.replica_id.as_uuid(),
            )
            .value(
                DeploymentReplicaBindings::replica_generation(),
                binding.replica_generation,
            )
            .value(
                DeploymentReplicaBindings::member_id(),
                binding.member_id.as_uuid(),
            )
            .value(
                DeploymentReplicaBindings::node_id(),
                binding.node_id.map(NodeId::as_uuid),
            )
            .value(
                DeploymentReplicaBindings::placement_generation(),
                binding.placement_generation,
            )
            .value(
                DeploymentReplicaBindings::runtime_unit_id(),
                binding.runtime_unit_id.as_str(),
            )
            .value(
                DeploymentReplicaBindings::runtime_generation(),
                binding.runtime_generation,
            )
            .value(DeploymentReplicaBindings::created_at(), binding.created_at)
            .value(DeploymentReplicaBindings::updated_at(), binding.updated_at),
    )
    .await?;
    require_one_row("deployment replica binding", rows)
}

impl ControlRow {
    fn control(self) -> Result<WorkloadControl, RepositoryError> {
        let managed_owner = match (
            self.managed_owner_kind,
            self.managed_owner_id,
            self.managed_owner_generation,
            self.managed_owner_spec_digest,
        ) {
            (None, None, None, None) => None,
            (Some(kind), Some(owner_id), Some(generation), Some(spec_digest)) => Some(
                ManagedOwnerReference::new(
                    ManagedOwnerKind::parse(kind).map_err(RepositoryError::Storage)?,
                    owner_id,
                    generation,
                    spec_digest,
                )
                .map_err(RepositoryError::Storage)?,
            ),
            _ => {
                return Err(RepositoryError::Storage(
                    "stored managed owner reference is incomplete".into(),
                ))
            }
        };
        let placement_policy: EffectivePlacementPolicy =
            serde_json::from_value(self.placement_policy)
                .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        placement_policy
            .validate()
            .map_err(RepositoryError::Storage)?;
        if placement_policy.digest() != self.placement_policy_digest {
            return Err(RepositoryError::Storage(
                "stored effective placement policy digest is inconsistent".into(),
            ));
        }
        WorkloadControl::restore(
            OrganizationId::from_uuid(self.organization_id),
            ProjectId::from_uuid(self.project_id),
            EnvironmentId::from_uuid(self.environment_id),
            WorkloadId::from_uuid(self.workload_id),
            WorkloadControlSpec {
                managed_owner,
                placement_policy,
            },
            self.aggregate_version,
            self.created_at,
            self.updated_at,
        )
        .map_err(RepositoryError::Storage)
    }
}

impl ReplicaRow {
    fn replica(self) -> Result<WorkloadReplica, RepositoryError> {
        let replica = WorkloadReplica {
            id: WorkloadReplicaId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            workload_id: WorkloadId::from_uuid(self.workload_id),
            ordinal: self.ordinal,
            revision_id: WorkloadRevisionId::from_uuid(self.revision_id),
            generation: self.generation,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
        };
        replica.validate().map_err(RepositoryError::Storage)?;
        Ok(replica)
    }
}

impl MemberRow {
    fn member(self) -> Result<WorkloadReplicaMember, RepositoryError> {
        let member = WorkloadReplicaMember {
            id: WorkloadReplicaMemberId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            workload_id: WorkloadId::from_uuid(self.workload_id),
            replica_id: WorkloadReplicaId::from_uuid(self.replica_id),
            ordinal: self.ordinal,
            node_id: self.node_id.map(NodeId::from_uuid),
            placement_generation: self.placement_generation,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
        };
        member.validate().map_err(RepositoryError::Storage)?;
        Ok(member)
    }
}

impl BindingRow {
    fn binding(self) -> Result<DeploymentReplicaBinding, RepositoryError> {
        if self.deployment_id.is_nil()
            || self.organization_id.is_nil()
            || self.project_id.is_nil()
            || self.environment_id.is_nil()
            || self.workload_id.is_nil()
            || self.revision_id.is_nil()
            || self.replica_id.is_nil()
            || self.replica_generation == 0
            || self.member_id.is_nil()
            || self.runtime_unit_id.trim().is_empty()
            || self.runtime_unit_id.len() > 512
            || self.runtime_unit_id.contains(['\0', '\r', '\n'])
            || self.runtime_generation == 0
            || self.runtime_generation != self.replica_generation
            || self.node_id.is_some() && self.placement_generation == 0
            || self.updated_at < self.created_at
        {
            return Err(RepositoryError::Storage(
                "stored deployment replica binding is invalid".into(),
            ));
        }
        Ok(DeploymentReplicaBinding {
            deployment_id: DeploymentId::from_uuid(self.deployment_id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            workload_id: WorkloadId::from_uuid(self.workload_id),
            revision_id: WorkloadRevisionId::from_uuid(self.revision_id),
            replica_id: WorkloadReplicaId::from_uuid(self.replica_id),
            replica_generation: self.replica_generation,
            member_id: WorkloadReplicaMemberId::from_uuid(self.member_id),
            node_id: self.node_id.map(NodeId::from_uuid),
            placement_generation: self.placement_generation,
            runtime_unit_id: self.runtime_unit_id,
            runtime_generation: self.runtime_generation,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn invariant(error: impl Into<String>) -> PostgresPersistenceError {
    PostgresPersistenceError::Invariant(error.into())
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
