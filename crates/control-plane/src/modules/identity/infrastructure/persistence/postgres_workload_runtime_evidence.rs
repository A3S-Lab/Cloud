use super::postgres::PostgresIdentityRepository;
use super::postgres_platform_rbac::lock_installation_for_authorization;
use super::postgres_workload_runtime_evidence_schema::WorkloadRuntimeEvidenceHistory;
use super::postgres_workload_trust::load_current_runtime_policy_under_installation_fence;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, require_one_row, store_idempotency,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{
    WorkloadRuntimeEvidenceBinding, WorkloadRuntimeEvidenceBindingId,
    WorkloadRuntimeEvidenceCandidate, WorkloadRuntimeEvidenceRecord,
};
use crate::modules::identity::domain::repositories::{
    IWorkloadRuntimeEvidenceRepository, ListWorkloadRuntimeEvidenceHistory,
    ReadWorkloadRuntimeEvidence, RecordWorkloadRuntimeEvidenceWrite,
    ReplayWorkloadRuntimeEvidenceAdmission,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotentWrite, InstallationId, NodeId, NodePoolId, OrganizationId, ProjectId,
    RepositoryError, ResourceClaimId, Sha256Digest, WorkloadId, WorkloadIdentityPolicyId,
    WorkloadIdentityPolicyRevisionId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{RuntimeIsolationLevel, RuntimeUnitClass, RuntimeUnitState};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, DecodeError, Expression, FromRow, FromValue, OrderDirection,
    PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct EvidenceSelection;

impl Selection for EvidenceSelection {
    type Output = EvidenceRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            WorkloadRuntimeEvidenceHistory::record_schema().expression(),
            WorkloadRuntimeEvidenceHistory::binding_schema().expression(),
            WorkloadRuntimeEvidenceHistory::binding_id().expression(),
            WorkloadRuntimeEvidenceHistory::installation_id().expression(),
            WorkloadRuntimeEvidenceHistory::organization_id().expression(),
            WorkloadRuntimeEvidenceHistory::project_id().expression(),
            WorkloadRuntimeEvidenceHistory::environment_id().expression(),
            WorkloadRuntimeEvidenceHistory::workload_id().expression(),
            WorkloadRuntimeEvidenceHistory::workload_revision_id().expression(),
            WorkloadRuntimeEvidenceHistory::policy_id().expression(),
            WorkloadRuntimeEvidenceHistory::policy_revision_id().expression(),
            WorkloadRuntimeEvidenceHistory::policy_revision_number().expression(),
            WorkloadRuntimeEvidenceHistory::policy_digest().expression(),
            WorkloadRuntimeEvidenceHistory::resource_claim_id().expression(),
            WorkloadRuntimeEvidenceHistory::resource_claim_generation().expression(),
            WorkloadRuntimeEvidenceHistory::resource_claim_aggregate_version().expression(),
            WorkloadRuntimeEvidenceHistory::resource_claim_digest().expression(),
            WorkloadRuntimeEvidenceHistory::resource_binding_digest().expression(),
            WorkloadRuntimeEvidenceHistory::node_pool_id().expression(),
            WorkloadRuntimeEvidenceHistory::node_pool_aggregate_version().expression(),
            WorkloadRuntimeEvidenceHistory::node_pool_spec_digest().expression(),
            WorkloadRuntimeEvidenceHistory::node_id().expression(),
            WorkloadRuntimeEvidenceHistory::node_aggregate_version().expression(),
            WorkloadRuntimeEvidenceHistory::agent_instance_id().expression(),
            WorkloadRuntimeEvidenceHistory::node_capabilities_digest().expression(),
            WorkloadRuntimeEvidenceHistory::node_last_observed_at().expression(),
            WorkloadRuntimeEvidenceHistory::runtime_report_id().expression(),
            WorkloadRuntimeEvidenceHistory::runtime_unit_id().expression(),
            WorkloadRuntimeEvidenceHistory::runtime_generation().expression(),
            WorkloadRuntimeEvidenceHistory::runtime_class().expression(),
            WorkloadRuntimeEvidenceHistory::isolation_level().expression(),
            WorkloadRuntimeEvidenceHistory::semantics_profile_digest().expression(),
            WorkloadRuntimeEvidenceHistory::identity_attachment_digest().expression(),
            WorkloadRuntimeEvidenceHistory::runtime_spec_digest().expression(),
            WorkloadRuntimeEvidenceHistory::runtime_attestation_binding_digest().expression(),
            WorkloadRuntimeEvidenceHistory::provider_attestation_digest().expression(),
            WorkloadRuntimeEvidenceHistory::provider_resource_id().expression(),
            WorkloadRuntimeEvidenceHistory::provider_build().expression(),
            WorkloadRuntimeEvidenceHistory::runtime_state().expression(),
            WorkloadRuntimeEvidenceHistory::runtime_observed_at().expression(),
            WorkloadRuntimeEvidenceHistory::runtime_received_at().expression(),
            WorkloadRuntimeEvidenceHistory::node_attestation_binding_digest().expression(),
            WorkloadRuntimeEvidenceHistory::binding_digest().expression(),
            WorkloadRuntimeEvidenceHistory::admitted_at().expression(),
        ]
    }
}

struct EvidenceRow {
    record_schema: String,
    binding_schema: String,
    binding_id: Uuid,
    installation_id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    workload_id: Uuid,
    workload_revision_id: Uuid,
    policy_id: Uuid,
    policy_revision_id: Uuid,
    policy_revision_number: u64,
    policy_digest: String,
    resource_claim_id: Uuid,
    resource_claim_generation: u64,
    resource_claim_aggregate_version: u64,
    resource_claim_digest: String,
    resource_binding_digest: String,
    node_pool_id: Uuid,
    node_pool_aggregate_version: u64,
    node_pool_spec_digest: String,
    node_id: Uuid,
    node_aggregate_version: u64,
    agent_instance_id: Uuid,
    node_capabilities_digest: String,
    node_last_observed_at: DateTime<Utc>,
    runtime_report_id: Uuid,
    runtime_unit_id: String,
    runtime_generation: u64,
    runtime_class: String,
    isolation_level: String,
    semantics_profile_digest: String,
    identity_attachment_digest: String,
    runtime_spec_digest: String,
    runtime_attestation_binding_digest: String,
    provider_attestation_digest: String,
    provider_resource_id: String,
    provider_build: String,
    runtime_state: String,
    runtime_observed_at: DateTime<Utc>,
    runtime_received_at: DateTime<Utc>,
    node_attestation_binding_digest: Option<String>,
    binding_digest: String,
    admitted_at: DateTime<Utc>,
}

impl FromRow for EvidenceRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            record_schema: decode(row, 0)?,
            binding_schema: decode(row, 1)?,
            binding_id: decode(row, 2)?,
            installation_id: decode(row, 3)?,
            organization_id: decode(row, 4)?,
            project_id: decode(row, 5)?,
            environment_id: decode(row, 6)?,
            workload_id: decode(row, 7)?,
            workload_revision_id: decode(row, 8)?,
            policy_id: decode(row, 9)?,
            policy_revision_id: decode(row, 10)?,
            policy_revision_number: decode(row, 11)?,
            policy_digest: decode(row, 12)?,
            resource_claim_id: decode(row, 13)?,
            resource_claim_generation: decode(row, 14)?,
            resource_claim_aggregate_version: decode(row, 15)?,
            resource_claim_digest: decode(row, 16)?,
            resource_binding_digest: decode(row, 17)?,
            node_pool_id: decode(row, 18)?,
            node_pool_aggregate_version: decode(row, 19)?,
            node_pool_spec_digest: decode(row, 20)?,
            node_id: decode(row, 21)?,
            node_aggregate_version: decode(row, 22)?,
            agent_instance_id: decode(row, 23)?,
            node_capabilities_digest: decode(row, 24)?,
            node_last_observed_at: decode(row, 25)?,
            runtime_report_id: decode(row, 26)?,
            runtime_unit_id: decode(row, 27)?,
            runtime_generation: decode(row, 28)?,
            runtime_class: decode(row, 29)?,
            isolation_level: decode(row, 30)?,
            semantics_profile_digest: decode(row, 31)?,
            identity_attachment_digest: decode(row, 32)?,
            runtime_spec_digest: decode(row, 33)?,
            runtime_attestation_binding_digest: decode(row, 34)?,
            provider_attestation_digest: decode(row, 35)?,
            provider_resource_id: decode(row, 36)?,
            provider_build: decode(row, 37)?,
            runtime_state: decode(row, 38)?,
            runtime_observed_at: decode(row, 39)?,
            runtime_received_at: decode(row, 40)?,
            node_attestation_binding_digest: decode(row, 41)?,
            binding_digest: decode(row, 42)?,
            admitted_at: decode(row, 43)?,
        })
    }
}

impl EvidenceRow {
    fn record(self) -> Result<WorkloadRuntimeEvidenceRecord, PostgresPersistenceError> {
        let candidate = WorkloadRuntimeEvidenceCandidate {
            installation_id: InstallationId::from_uuid(self.installation_id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            workload_id: WorkloadId::from_uuid(self.workload_id),
            workload_revision_id: WorkloadRevisionId::from_uuid(self.workload_revision_id),
            resource_claim_id: ResourceClaimId::from_uuid(self.resource_claim_id),
            resource_claim_generation: self.resource_claim_generation,
            resource_claim_aggregate_version: self.resource_claim_aggregate_version,
            resource_claim_digest: digest(self.resource_claim_digest)?,
            resource_binding_digest: digest(self.resource_binding_digest)?,
            node_pool_id: NodePoolId::from_uuid(self.node_pool_id),
            node_pool_aggregate_version: self.node_pool_aggregate_version,
            node_pool_spec_digest: digest(self.node_pool_spec_digest)?,
            node_id: NodeId::from_uuid(self.node_id),
            node_aggregate_version: self.node_aggregate_version,
            agent_instance_id: self.agent_instance_id,
            node_capabilities_digest: digest(self.node_capabilities_digest)?,
            node_last_observed_at: self.node_last_observed_at,
            runtime_report_id: self.runtime_report_id,
            runtime_unit_id: self.runtime_unit_id,
            runtime_generation: self.runtime_generation,
            runtime_class: parse_runtime_class(&self.runtime_class)?,
            isolation_level: parse_isolation(&self.isolation_level)?,
            semantics_profile_digest: digest(self.semantics_profile_digest)?,
            identity_attachment_digest: digest(self.identity_attachment_digest)?,
            runtime_spec_digest: digest(self.runtime_spec_digest)?,
            runtime_attestation_binding_digest: digest(self.runtime_attestation_binding_digest)?,
            provider_attestation_digest: digest(self.provider_attestation_digest)?,
            provider_resource_id: self.provider_resource_id,
            provider_build: self.provider_build,
            runtime_state: parse_runtime_state(&self.runtime_state)?,
            runtime_observed_at: self.runtime_observed_at,
            runtime_received_at: self.runtime_received_at,
        };
        let binding = WorkloadRuntimeEvidenceBinding::restore(
            self.binding_schema,
            WorkloadRuntimeEvidenceBindingId::from_uuid(self.binding_id),
            WorkloadIdentityPolicyId::from_uuid(self.policy_id),
            WorkloadIdentityPolicyRevisionId::from_uuid(self.policy_revision_id),
            self.policy_revision_number,
            digest(self.policy_digest)?,
            candidate,
            self.node_attestation_binding_digest
                .map(digest)
                .transpose()?,
            digest(self.binding_digest)?,
        )
        .map_err(PostgresPersistenceError::Invariant)?;
        WorkloadRuntimeEvidenceRecord::restore(self.record_schema, binding, self.admitted_at)
            .map_err(PostgresPersistenceError::Invariant)
    }
}

#[async_trait]
impl IWorkloadRuntimeEvidenceRepository for PostgresIdentityRepository {
    async fn replay_admission(
        &self,
        replay: ReplayWorkloadRuntimeEvidenceAdmission,
    ) -> Result<Option<IdempotentWrite<WorkloadRuntimeEvidenceRecord>>, RepositoryError> {
        replay.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(replayed) = idempotency_replay::<WorkloadRuntimeEvidenceRecord>(
                        transaction,
                        &replay.idempotency,
                    )
                    .await?
                    else {
                        return Ok(None);
                    };
                    validate_replay(&replayed, &replay)?;
                    Ok(Some(replayed))
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn record(
        &self,
        write: RecordWorkloadRuntimeEvidenceWrite,
    ) -> Result<IdempotentWrite<WorkloadRuntimeEvidenceRecord>, RepositoryError> {
        write.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) = idempotency_replay::<WorkloadRuntimeEvidenceRecord>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        let context = replay_context(&write);
                        validate_replay(&replayed, &context)?;
                        return Ok(replayed);
                    }

                    let binding = write.record.binding();
                    let candidate = &binding.candidate;
                    lock_installation_for_authorization(transaction, candidate.installation_id)
                        .await?;
                    let current = load_current_runtime_policy_under_installation_fence(
                        transaction,
                        candidate.installation_id,
                        candidate.organization_id,
                        candidate.workload_id,
                    )
                    .await?
                    .ok_or_else(|| {
                        RepositoryError::Conflict(
                            "workload Runtime evidence policy is no longer current".into(),
                        )
                    })?;
                    if current != write.expected_policy {
                        return Err(RepositoryError::Conflict(
                            "workload Runtime evidence policy changed before commit".into(),
                        )
                        .into());
                    }
                    write
                        .record
                        .validate_against_policy(&current)
                        .map_err(RepositoryError::Conflict)?;

                    transaction
                        .advisory_xact_lock(
                            "a3s.cloud.identity-workload-runtime-evidence",
                            &binding.id.as_uuid().to_string(),
                        )
                        .await?;
                    if let Some(existing) = load_record(
                        transaction,
                        candidate.installation_id,
                        candidate.organization_id,
                        candidate.workload_id,
                        binding.id,
                    )
                    .await?
                    {
                        if existing != write.record {
                            return Err(RepositoryError::IdempotencyConflict.into());
                        }
                        store_idempotency(transaction, &write.idempotency, &existing).await?;
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }

                    insert_record(transaction, &write.record).await?;
                    store_idempotency(transaction, &write.idempotency, &write.record).await?;
                    Ok(IdempotentWrite {
                        value: write.record,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn read(
        &self,
        read: ReadWorkloadRuntimeEvidence,
    ) -> Result<Option<WorkloadRuntimeEvidenceRecord>, RepositoryError> {
        read.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    load_record(
                        transaction,
                        read.installation_id,
                        read.organization_id,
                        read.workload_id,
                        read.binding_id,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_history(
        &self,
        read: ListWorkloadRuntimeEvidenceHistory,
    ) -> Result<Vec<WorkloadRuntimeEvidenceRecord>, RepositoryError> {
        read.validate().map_err(RepositoryError::Storage)?;
        let limit = u64::try_from(read.limit).map_err(|_| {
            RepositoryError::Storage("workload Runtime evidence limit is not portable".into())
        })?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_all::<EvidenceRow, _>(
                        transaction,
                        select_from::<WorkloadRuntimeEvidenceHistory>()
                            .select(EvidenceSelection)
                            .filter(
                                WorkloadRuntimeEvidenceHistory::installation_id()
                                    .eq(read.installation_id.as_uuid()),
                            )
                            .filter(
                                WorkloadRuntimeEvidenceHistory::organization_id()
                                    .eq(read.organization_id.as_uuid()),
                            )
                            .filter(
                                WorkloadRuntimeEvidenceHistory::workload_id()
                                    .eq(read.workload_id.as_uuid()),
                            )
                            .order_by(
                                WorkloadRuntimeEvidenceHistory::admitted_at(),
                                OrderDirection::Desc,
                            )
                            .order_by(
                                WorkloadRuntimeEvidenceHistory::binding_id(),
                                OrderDirection::Desc,
                            )
                            .limit(limit),
                    )
                    .await?
                    .into_iter()
                    .map(EvidenceRow::record)
                    .collect::<Result<Vec<_>, _>>()
                })
            })
            .await
            .map_err(transaction_error)
    }
}

fn replay_context(
    write: &RecordWorkloadRuntimeEvidenceWrite,
) -> ReplayWorkloadRuntimeEvidenceAdmission {
    let candidate = &write.record.binding().candidate;
    ReplayWorkloadRuntimeEvidenceAdmission {
        installation_id: candidate.installation_id,
        organization_id: candidate.organization_id,
        workload_id: candidate.workload_id,
        resource_claim_id: candidate.resource_claim_id,
        evaluated_at: write.record.admitted_at(),
        admission_id: write.admission_id,
        idempotency: write.idempotency.clone(),
    }
}

fn validate_replay(
    replayed: &IdempotentWrite<WorkloadRuntimeEvidenceRecord>,
    context: &ReplayWorkloadRuntimeEvidenceAdmission,
) -> Result<(), PostgresPersistenceError> {
    context
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    replayed
        .value
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let candidate = &replayed.value.binding().candidate;
    if candidate.installation_id != context.installation_id
        || candidate.organization_id != context.organization_id
        || candidate.workload_id != context.workload_id
        || candidate.resource_claim_id != context.resource_claim_id
        || replayed.value.admitted_at() != context.evaluated_at
    {
        return Err(PostgresPersistenceError::Invariant(
            "idempotent workload Runtime evidence crossed its historic scope".into(),
        ));
    }
    Ok(())
}

async fn load_record(
    transaction: &PostgresTransaction,
    installation_id: InstallationId,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    binding_id: WorkloadRuntimeEvidenceBindingId,
) -> Result<Option<WorkloadRuntimeEvidenceRecord>, PostgresPersistenceError> {
    fetch_optional::<EvidenceRow, _>(
        transaction,
        select_from::<WorkloadRuntimeEvidenceHistory>()
            .select(EvidenceSelection)
            .filter(WorkloadRuntimeEvidenceHistory::installation_id().eq(installation_id.as_uuid()))
            .filter(WorkloadRuntimeEvidenceHistory::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkloadRuntimeEvidenceHistory::workload_id().eq(workload_id.as_uuid()))
            .filter(WorkloadRuntimeEvidenceHistory::binding_id().eq(binding_id.as_uuid())),
    )
    .await?
    .map(EvidenceRow::record)
    .transpose()
}

async fn insert_record(
    transaction: &PostgresTransaction,
    record: &WorkloadRuntimeEvidenceRecord,
) -> Result<(), PostgresPersistenceError> {
    let binding = record.binding();
    let candidate = &binding.candidate;
    let rows = execute(
        transaction,
        insert_into::<WorkloadRuntimeEvidenceHistory>()
            .value(
                WorkloadRuntimeEvidenceHistory::record_schema(),
                record.schema(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::binding_schema(),
                binding.schema.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::binding_id(),
                binding.id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::installation_id(),
                candidate.installation_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::organization_id(),
                candidate.organization_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::project_id(),
                candidate.project_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::environment_id(),
                candidate.environment_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::workload_id(),
                candidate.workload_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::workload_revision_id(),
                candidate.workload_revision_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::policy_id(),
                binding.policy_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::policy_revision_id(),
                binding.policy_revision_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::policy_revision_number(),
                binding.policy_revision_number,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::policy_digest(),
                binding.policy_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::resource_claim_id(),
                candidate.resource_claim_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::resource_claim_generation(),
                candidate.resource_claim_generation,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::resource_claim_aggregate_version(),
                candidate.resource_claim_aggregate_version,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::resource_claim_digest(),
                candidate.resource_claim_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::resource_binding_digest(),
                candidate.resource_binding_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::node_pool_id(),
                candidate.node_pool_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::node_pool_aggregate_version(),
                candidate.node_pool_aggregate_version,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::node_pool_spec_digest(),
                candidate.node_pool_spec_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::node_id(),
                candidate.node_id.as_uuid(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::node_aggregate_version(),
                candidate.node_aggregate_version,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::agent_instance_id(),
                candidate.agent_instance_id,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::node_capabilities_digest(),
                candidate.node_capabilities_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::node_last_observed_at(),
                candidate.node_last_observed_at,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::runtime_report_id(),
                candidate.runtime_report_id,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::runtime_unit_id(),
                candidate.runtime_unit_id.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::runtime_generation(),
                candidate.runtime_generation,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::runtime_class(),
                runtime_class_name(candidate.runtime_class),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::isolation_level(),
                isolation_name(candidate.isolation_level),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::semantics_profile_digest(),
                candidate.semantics_profile_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::identity_attachment_digest(),
                candidate.identity_attachment_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::runtime_spec_digest(),
                candidate.runtime_spec_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::runtime_attestation_binding_digest(),
                candidate.runtime_attestation_binding_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::provider_attestation_digest(),
                candidate.provider_attestation_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::provider_resource_id(),
                candidate.provider_resource_id.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::provider_build(),
                candidate.provider_build.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::runtime_state(),
                runtime_state_name(candidate.runtime_state),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::runtime_observed_at(),
                candidate.runtime_observed_at,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::runtime_received_at(),
                candidate.runtime_received_at,
            )
            .value(
                WorkloadRuntimeEvidenceHistory::node_attestation_binding_digest(),
                binding
                    .node_attestation_binding_digest
                    .as_ref()
                    .map(|digest| digest.as_str().to_owned()),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::binding_digest(),
                binding.binding_digest.as_str(),
            )
            .value(
                WorkloadRuntimeEvidenceHistory::admitted_at(),
                record.admitted_at(),
            ),
    )
    .await?;
    require_one_row("workload Runtime evidence history", rows)
}

fn digest(value: String) -> Result<Sha256Digest, PostgresPersistenceError> {
    Sha256Digest::parse(value).map_err(PostgresPersistenceError::Invariant)
}

fn runtime_class_name(value: RuntimeUnitClass) -> &'static str {
    match value {
        RuntimeUnitClass::Task => "task",
        RuntimeUnitClass::Service => "service",
    }
}

fn parse_runtime_class(value: &str) -> Result<RuntimeUnitClass, PostgresPersistenceError> {
    match value {
        "task" => Ok(RuntimeUnitClass::Task),
        "service" => Ok(RuntimeUnitClass::Service),
        _ => Err(PostgresPersistenceError::Invariant(
            "stored workload Runtime evidence class is invalid".into(),
        )),
    }
}

fn isolation_name(value: RuntimeIsolationLevel) -> &'static str {
    match value {
        RuntimeIsolationLevel::Process => "process",
        RuntimeIsolationLevel::Container => "container",
        RuntimeIsolationLevel::Sandbox => "sandbox",
        RuntimeIsolationLevel::Confidential => "confidential",
    }
}

fn parse_isolation(value: &str) -> Result<RuntimeIsolationLevel, PostgresPersistenceError> {
    match value {
        "process" => Ok(RuntimeIsolationLevel::Process),
        "container" => Ok(RuntimeIsolationLevel::Container),
        "sandbox" => Ok(RuntimeIsolationLevel::Sandbox),
        "confidential" => Ok(RuntimeIsolationLevel::Confidential),
        _ => Err(PostgresPersistenceError::Invariant(
            "stored workload Runtime evidence isolation is invalid".into(),
        )),
    }
}

fn runtime_state_name(value: RuntimeUnitState) -> &'static str {
    match value {
        RuntimeUnitState::Running => "running",
        _ => "invalid",
    }
}

fn parse_runtime_state(value: &str) -> Result<RuntimeUnitState, PostgresPersistenceError> {
    match value {
        "running" => Ok(RuntimeUnitState::Running),
        _ => Err(PostgresPersistenceError::Invariant(
            "stored workload Runtime evidence state is invalid".into(),
        )),
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
