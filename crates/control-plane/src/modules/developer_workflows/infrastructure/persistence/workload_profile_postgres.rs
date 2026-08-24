use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, store_audit,
    store_idempotency, store_outbox, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::developer_workflows::domain::{
    AcceptWorkloadProfileRevisionWrite, AcceptedWorkloadProfileRevision,
    IWorkloadProfileRepository, WorkloadProfileRevisionWriteReference,
    MAX_WORKLOAD_PROFILE_REVISIONS_PAGE,
};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId,
    ProjectId, RepositoryError, SourceRevisionId, WorkloadProfileId, WorkloadProfileRevisionId,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row, SqlQuery,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_REVISIONS: &str = "select organization_id, project_id, environment_id, profile_id, id, revision_number, build_plan_id, source_revision_id, project_root, profile_name, profile_kind, contract_schema, canonical_acl, contract_digest, build_plan_digest, accepted_by, accepted_at from developer_workload_profile_revisions";

#[derive(Clone)]
pub struct PostgresWorkloadProfileRepository {
    executor: PostgresExecutor,
}

impl PostgresWorkloadProfileRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IWorkloadProfileRepository for PostgresWorkloadProfileRepository {
    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) =
                        idempotency_replay::<WorkloadProfileRevisionWriteReference>(
                            transaction,
                            &idempotency,
                        )
                        .await?
                    else {
                        return Ok(None);
                    };
                    load_reference(transaction, reference.value).await.map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn accept(
        &self,
        write: AcceptWorkloadProfileRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedWorkloadProfileRevision>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    transaction
                        .advisory_xact_lock(
                            "a3s.cloud.developer-workload-profile",
                            &format!(
                                "{}:{}",
                                write.revision.organization_id, write.revision.profile_id
                            ),
                        )
                        .await?;
                    if let Some(reference) = idempotency_replay::<
                        WorkloadProfileRevisionWriteReference,
                    >(transaction, &write.idempotency)
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_reference(transaction, reference.value).await?,
                            replayed: true,
                        });
                    }

                    let current = load_current(
                        transaction,
                        write.revision.organization_id,
                        write.revision.project_id,
                        write.revision.environment_id,
                        write.revision.profile_id,
                    )
                    .await?;
                    if let Some(existing) = current.as_ref() {
                        ensure_same_profile(existing, &write.revision)?;
                        if existing.contract == write.revision.contract
                            && existing.accepted_by == write.revision.accepted_by
                        {
                            let reference = WorkloadProfileRevisionWriteReference::from(existing);
                            store_idempotency(transaction, &write.idempotency, &reference).await?;
                            return Ok(IdempotentWrite {
                                value: existing.clone(),
                                replayed: true,
                            });
                        }
                    }
                    let actual_previous = current.as_ref().map(|revision| revision.id);
                    let expected_number = current
                        .as_ref()
                        .map_or(Some(1), |revision| revision.revision_number.checked_add(1))
                        .ok_or_else(|| {
                            RepositoryError::Conflict(
                                "workload profile revision number overflowed".into(),
                            )
                        })?;
                    if actual_previous != write.expected_previous_revision_id
                        || write.revision.revision_number != expected_number
                    {
                        return Err(RepositoryError::Conflict(
                            "workload profile head advanced before acceptance".into(),
                        )
                        .into());
                    }

                    let inserted = match insert_revision(transaction, &write.revision).await {
                        Ok(rows) => rows,
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    };
                    if inserted != 1 {
                        return Err(RepositoryError::Conflict(
                            "workload profile revision identity is already in use".into(),
                        )
                        .into());
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: write.revision.organization_id.as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "developer.workload-profile.revision-accepted",
                            aggregate_id: write.revision.profile_id.as_uuid(),
                            occurred_at: write.revision.accepted_at,
                            request_id: write.request_id,
                            attribution_scope: AuditWrite::project_attribution(
                                write.revision.project_id,
                                Some(write.revision.environment_id),
                            ),
                            details: audit_details(&write.revision),
                        },
                    )
                    .await?;
                    let reference = WorkloadProfileRevisionWriteReference::from(&write.revision);
                    store_idempotency(transaction, &write.idempotency, &reference).await?;
                    Ok(IdempotentWrite {
                        value: write.revision,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_profile_id: WorkloadProfileId,
        workload_profile_revision_id: WorkloadProfileRevisionId,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                revision_query()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and profile_id = ")
                    .bind(workload_profile_id.as_uuid())
                    .append(" and id = ")
                    .bind(workload_profile_revision_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_revision)
            .transpose()
            .map_err(storage)
    }

    async fn find_current(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_profile_id: WorkloadProfileId,
    ) -> Result<Option<AcceptedWorkloadProfileRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(current_query(
                organization_id,
                project_id,
                environment_id,
                workload_profile_id,
            ))
            .await
            .map_err(storage)?
            .map(decode_revision)
            .transpose()
            .map_err(storage)
    }

    async fn list_revisions(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        workload_profile_id: WorkloadProfileId,
        limit: usize,
    ) -> Result<Vec<AcceptedWorkloadProfileRevision>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                revision_query()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and profile_id = ")
                    .bind(workload_profile_id.as_uuid())
                    .append(" order by revision_number asc limit ")
                    .bind(limit.min(MAX_WORKLOAD_PROFILE_REVISIONS_PAGE)),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(|row| decode_revision(row).map_err(storage))
            .collect()
    }
}

async fn insert_revision(
    transaction: &PostgresTransaction,
    revision: &AcceptedWorkloadProfileRevision,
) -> Result<u64, PostgresPersistenceError> {
    let spec = revision.contract.spec();
    execute(
        transaction,
        sql_query::<()>("insert into developer_workload_profile_revisions (organization_id, project_id, environment_id, profile_id, id, revision_number, build_plan_id, source_revision_id, project_root, profile_name, profile_kind, contract_schema, canonical_acl, contract_digest, build_plan_digest, accepted_by, accepted_at) values (")
            .bind(revision.organization_id.as_uuid())
            .append(", ")
            .bind(revision.project_id.as_uuid())
            .append(", ")
            .bind(revision.environment_id.as_uuid())
            .append(", ")
            .bind(revision.profile_id.as_uuid())
            .append(", ")
            .bind(revision.id.as_uuid())
            .append(", ")
            .bind(revision.revision_number)
            .append(", ")
            .bind(revision.build_plan_id.as_uuid())
            .append(", ")
            .bind(revision.source_revision_id.as_uuid())
            .append(", ")
            .bind(spec.project_root.as_str())
            .append(", ")
            .bind(spec.profile.name.as_str())
            .append(", ")
            .bind(spec.profile.kind.as_str())
            .append(", ")
            .bind(revision.contract.schema())
            .append(", ")
            .bind(revision.contract.canonical_acl())
            .append(", ")
            .bind(revision.contract.digest().as_str())
            .append(", ")
            .bind(spec.build_plan_digest.as_str())
            .append(", ")
            .bind(revision.accepted_by.as_uuid())
            .append(", ")
            .bind(revision.accepted_at)
            .append(") on conflict do nothing"),
    )
    .await
}

async fn load_reference(
    transaction: &PostgresTransaction,
    reference: WorkloadProfileRevisionWriteReference,
) -> Result<AcceptedWorkloadProfileRevision, PostgresPersistenceError> {
    fetch_optional::<WorkloadProfileRevisionRow, _>(
        transaction,
        revision_query()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(reference.environment_id.as_uuid())
            .append(" and profile_id = ")
            .bind(reference.workload_profile_id.as_uuid())
            .append(" and id = ")
            .bind(reference.workload_profile_revision_id.as_uuid()),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "workload profile idempotency points to a missing revision".into(),
        )
    })
    .and_then(decode_revision)
}

async fn load_current(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_profile_id: WorkloadProfileId,
) -> Result<Option<AcceptedWorkloadProfileRevision>, PostgresPersistenceError> {
    fetch_optional::<WorkloadProfileRevisionRow, _>(
        transaction,
        current_query(
            organization_id,
            project_id,
            environment_id,
            workload_profile_id,
        ),
    )
    .await?
    .map(decode_revision)
    .transpose()
}

fn current_query(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_profile_id: WorkloadProfileId,
) -> SqlQuery<WorkloadProfileRevisionRow> {
    revision_query()
        .append(" where organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and project_id = ")
        .bind(project_id.as_uuid())
        .append(" and environment_id = ")
        .bind(environment_id.as_uuid())
        .append(" and profile_id = ")
        .bind(workload_profile_id.as_uuid())
        .append(" order by revision_number desc limit 1")
}

fn ensure_same_profile(
    existing: &AcceptedWorkloadProfileRevision,
    candidate: &AcceptedWorkloadProfileRevision,
) -> Result<(), PostgresPersistenceError> {
    if existing.organization_id != candidate.organization_id
        || existing.project_id != candidate.project_id
        || existing.environment_id != candidate.environment_id
        || existing.profile_id != candidate.profile_id
        || existing.contract.spec().project_root != candidate.contract.spec().project_root
        || existing.contract.spec().profile.name != candidate.contract.spec().profile.name
    {
        return Err(RepositoryError::Conflict(
            "workload profile identity collided with another logical profile".into(),
        )
        .into());
    }
    Ok(())
}

fn audit_details(revision: &AcceptedWorkloadProfileRevision) -> serde_json::Value {
    let spec = revision.contract.spec();
    serde_json::json!({
        "workloadProfileId": revision.profile_id,
        "workloadProfileRevisionId": revision.id,
        "revisionNumber": revision.revision_number,
        "buildPlanId": revision.build_plan_id,
        "sourceRevisionId": revision.source_revision_id,
        "buildPlanDigest": spec.build_plan_digest,
        "profileDigest": revision.contract.digest(),
        "projectRoot": spec.project_root,
        "profileName": spec.profile.name,
        "profileKind": spec.profile.kind.as_str(),
    })
}

fn revision_query() -> SqlQuery<WorkloadProfileRevisionRow> {
    sql_query::<WorkloadProfileRevisionRow>(SELECT_REVISIONS)
}

struct WorkloadProfileRevisionRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    profile_id: Uuid,
    id: Uuid,
    revision_number: u64,
    build_plan_id: Uuid,
    source_revision_id: Uuid,
    project_root: String,
    profile_name: String,
    profile_kind: String,
    contract_schema: String,
    canonical_acl: String,
    contract_digest: String,
    build_plan_digest: String,
    accepted_by: Uuid,
    accepted_at: DateTime<Utc>,
}

impl FromRow for WorkloadProfileRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            profile_id: decode(row, 3)?,
            id: decode(row, 4)?,
            revision_number: decode(row, 5)?,
            build_plan_id: decode(row, 6)?,
            source_revision_id: decode(row, 7)?,
            project_root: decode(row, 8)?,
            profile_name: decode(row, 9)?,
            profile_kind: decode(row, 10)?,
            contract_schema: decode(row, 11)?,
            canonical_acl: decode(row, 12)?,
            contract_digest: decode(row, 13)?,
            build_plan_digest: decode(row, 14)?,
            accepted_by: decode(row, 15)?,
            accepted_at: decode(row, 16)?,
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

fn decode_revision(
    row: WorkloadProfileRevisionRow,
) -> Result<AcceptedWorkloadProfileRevision, PostgresPersistenceError> {
    let revision = AcceptedWorkloadProfileRevision::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        EnvironmentId::from_uuid(row.environment_id),
        WorkloadProfileId::from_uuid(row.profile_id),
        WorkloadProfileRevisionId::from_uuid(row.id),
        row.revision_number,
        BuildPlanId::from_uuid(row.build_plan_id),
        SourceRevisionId::from_uuid(row.source_revision_id),
        &row.canonical_acl,
        &row.contract_digest,
        PrincipalId::from_uuid(row.accepted_by),
        row.accepted_at,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored workload profile revision is invalid: {error}"
        ))
    })?;
    let spec = revision.contract.spec();
    if row.project_root != spec.project_root
        || row.profile_name != spec.profile.name
        || row.profile_kind != spec.profile.kind.as_str()
        || row.contract_schema != revision.contract.schema()
        || row.build_plan_digest != spec.build_plan_digest.as_str()
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored workload profile columns drifted from canonical ACL".into(),
        ));
    }
    Ok(revision)
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(format!(
        "could not query workload profile revisions: {error}"
    ))
}

#[cfg(test)]
mod migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/147_developer_workload_profile_revisions.sql"
    ));

    #[test]
    fn migration_keeps_revisions_append_only_and_inside_owner_boundaries() {
        for expected in [
            "create table developer_workload_profile_revisions",
            "a3s.cloud.workload-profile.v1",
            "references developer_build_plans",
            "validate_developer_workload_profile_revision",
            "accepted workload profile revision does not match its exact BuildPlan",
            "workload profile revision sequence is not monotonic",
            "developer_workload_profile_revisions_immutable",
            "accepted workload profile revisions are immutable",
            "no BuildRun, Workload, Route, Execution, Automation, or scheduler authority",
        ] {
            assert!(
                MIGRATION.contains(expected),
                "missing migration guard {expected}"
            );
        }
        for forbidden in [
            "create table build_runs",
            "create table workloads",
            "create table routes",
            "create table executions",
            "create table automation",
            "create table schedules",
        ] {
            assert!(
                !MIGRATION.to_ascii_lowercase().contains(forbidden),
                "migration crossed owning context with {forbidden}"
            );
        }
    }
}
