use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, store_audit,
    store_idempotency, store_outbox, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::developer_workflows::domain::{
    AcceptBuildPlanWrite, AcceptedBuildPlan, BuildPlanWriteReference, IBuildPlanRepository,
};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId,
    ProjectId, RepositoryError, SourceRevisionId,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row, SqlQuery,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_PLANS: &str = "select organization_id, project_id, environment_id, id, source_revision_id, project_root, contract_schema, canonical_acl, contract_digest, proposal_digest, source_identity_digest, commit_sha, source_content_digest, detector_kind, detector_revision, evidence_path, evidence_digest, recipe_digest, aggregate_version, accepted_by, accepted_at from developer_build_plans";

#[derive(Clone)]
pub struct PostgresBuildPlanRepository {
    executor: PostgresExecutor,
}

impl PostgresBuildPlanRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IBuildPlanRepository for PostgresBuildPlanRepository {
    async fn replay_acceptance(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let Some(reference) =
                        idempotency_replay::<BuildPlanWriteReference>(transaction, &idempotency)
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
        write: AcceptBuildPlanWrite,
    ) -> Result<IdempotentWrite<AcceptedBuildPlan>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(reference) = idempotency_replay::<BuildPlanWriteReference>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return Ok(IdempotentWrite {
                            value: load_reference(transaction, reference.value).await?,
                            replayed: true,
                        });
                    }
                    let inserted = insert_plan(transaction, &write.plan).await;
                    let inserted = match inserted {
                        Ok(rows) => rows,
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    };
                    if inserted == 0 {
                        let project_root = &write.plan.contract.spec().proposal.spec().project_root;
                        let Some(existing) = load_for_source_root(
                            transaction,
                            write.plan.organization_id,
                            write.plan.project_id,
                            write.plan.environment_id,
                            write.plan.source_revision_id,
                            project_root,
                        )
                        .await?
                        else {
                            return Err(RepositoryError::Conflict(
                                "accepted BuildPlan identity is already in use".into(),
                            )
                            .into());
                        };
                        if existing.contract != write.plan.contract {
                            return Err(RepositoryError::Conflict(
                                "Source revision project root already accepted another BuildPlan"
                                    .into(),
                            )
                            .into());
                        }
                        let reference = BuildPlanWriteReference::from(&existing);
                        store_idempotency(transaction, &write.idempotency, &reference).await?;
                        return Ok(IdempotentWrite {
                            value: existing,
                            replayed: true,
                        });
                    }
                    if inserted != 1 {
                        return Err(PostgresPersistenceError::Invariant(format!(
                            "accepting BuildPlan affected {inserted} rows"
                        )));
                    }
                    store_outbox(transaction, &write.event).await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: write.plan.organization_id.as_uuid(),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "developer.build-plan.accepted",
                            aggregate_id: write.plan.id.as_uuid(),
                            occurred_at: write.plan.accepted_at,
                            request_id: write.request_id,
                            attribution_scope: AuditWrite::project_attribution(
                                write.plan.project_id,
                                Some(write.plan.environment_id),
                            ),
                            details: audit_details(&write.plan)?,
                        },
                    )
                    .await?;
                    let reference = BuildPlanWriteReference::from(&write.plan);
                    store_idempotency(transaction, &write.idempotency, &reference).await?;
                    Ok(IdempotentWrite {
                        value: write.plan,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        build_plan_id: BuildPlanId,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                plan_query()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and id = ")
                    .bind(build_plan_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_plan)
            .transpose()
            .map_err(storage)
    }

    async fn find_for_source_root(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
        project_root: &str,
    ) -> Result<Option<AcceptedBuildPlan>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(source_root_query(
                organization_id,
                project_id,
                environment_id,
                source_revision_id,
                project_root,
            ))
            .await
            .map_err(storage)?
            .map(decode_plan)
            .transpose()
            .map_err(storage)
    }

    async fn list_for_source(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
        limit: usize,
    ) -> Result<Vec<AcceptedBuildPlan>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                plan_query()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" and source_revision_id = ")
                    .bind(source_revision_id.as_uuid())
                    .append(" order by project_root asc, id asc limit ")
                    .bind(limit),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(|row| decode_plan(row).map_err(storage))
            .collect()
    }
}

async fn insert_plan(
    transaction: &PostgresTransaction,
    plan: &AcceptedBuildPlan,
) -> Result<u64, PostgresPersistenceError> {
    let proposal = &plan.contract.spec().proposal;
    let spec = proposal.spec();
    execute(
        transaction,
        sql_query::<()>("insert into developer_build_plans (organization_id, project_id, environment_id, id, source_revision_id, project_root, contract_schema, canonical_acl, contract_digest, proposal_digest, source_identity_digest, commit_sha, source_content_digest, detector_kind, detector_revision, evidence_path, evidence_digest, recipe_digest, aggregate_version, accepted_by, accepted_at) values (")
            .bind(plan.organization_id.as_uuid())
            .append(", ")
            .bind(plan.project_id.as_uuid())
            .append(", ")
            .bind(plan.environment_id.as_uuid())
            .append(", ")
            .bind(plan.id.as_uuid())
            .append(", ")
            .bind(plan.source_revision_id.as_uuid())
            .append(", ")
            .bind(spec.project_root.as_str())
            .append(", ")
            .bind(plan.contract.schema())
            .append(", ")
            .bind(plan.contract.canonical_acl())
            .append(", ")
            .bind(plan.contract.digest().as_str())
            .append(", ")
            .bind(proposal.digest().as_str())
            .append(", ")
            .bind(spec.source.source_identity_digest.as_str())
            .append(", ")
            .bind(spec.source.commit_sha.as_str())
            .append(", ")
            .bind(spec.source.content_digest.as_str())
            .append(", ")
            .bind(spec.detector.as_str())
            .append(", ")
            .bind(spec.detector_revision.as_str())
            .append(", ")
            .bind(spec.evidence_path.as_str())
            .append(", ")
            .bind(spec.evidence_digest.as_str())
            .append(", ")
            .bind(spec.recipe.digest().map_err(PostgresPersistenceError::Invariant)?)
            .append(", ")
            .bind(plan.aggregate_version)
            .append(", ")
            .bind(plan.accepted_by.as_uuid())
            .append(", ")
            .bind(plan.accepted_at)
            .append(") on conflict do nothing"),
    )
    .await
}

async fn load_reference(
    transaction: &PostgresTransaction,
    reference: BuildPlanWriteReference,
) -> Result<AcceptedBuildPlan, PostgresPersistenceError> {
    fetch_optional::<BuildPlanRow, _>(
        transaction,
        plan_query()
            .append(" where organization_id = ")
            .bind(reference.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(reference.project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(reference.environment_id.as_uuid())
            .append(" and id = ")
            .bind(reference.build_plan_id.as_uuid()),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "accepted BuildPlan idempotency points to a missing record".into(),
        )
    })
    .and_then(decode_plan)
}

async fn load_for_source_root(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    project_root: &str,
) -> Result<Option<AcceptedBuildPlan>, PostgresPersistenceError> {
    fetch_optional::<BuildPlanRow, _>(
        transaction,
        source_root_query(
            organization_id,
            project_id,
            environment_id,
            source_revision_id,
            project_root,
        ),
    )
    .await?
    .map(decode_plan)
    .transpose()
}

fn source_root_query(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    source_revision_id: SourceRevisionId,
    project_root: &str,
) -> SqlQuery<BuildPlanRow> {
    plan_query()
        .append(" where organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and project_id = ")
        .bind(project_id.as_uuid())
        .append(" and environment_id = ")
        .bind(environment_id.as_uuid())
        .append(" and source_revision_id = ")
        .bind(source_revision_id.as_uuid())
        .append(" and project_root = ")
        .bind(project_root)
}

fn audit_details(plan: &AcceptedBuildPlan) -> Result<serde_json::Value, PostgresPersistenceError> {
    let proposal = &plan.contract.spec().proposal;
    let spec = proposal.spec();
    Ok(serde_json::json!({
        "buildPlanId": plan.id,
        "sourceRevisionId": plan.source_revision_id,
        "projectRoot": spec.project_root,
        "planDigest": plan.contract.digest(),
        "proposalDigest": proposal.digest(),
        "sourceIdentityDigest": spec.source.source_identity_digest,
        "sourceContentDigest": spec.source.content_digest,
        "commitSha": spec.source.commit_sha,
        "detector": spec.detector.as_str(),
        "detectorRevision": spec.detector_revision,
        "recipeDigest": spec.recipe.digest().map_err(PostgresPersistenceError::Invariant)?,
    }))
}

fn plan_query() -> SqlQuery<BuildPlanRow> {
    sql_query::<BuildPlanRow>(SELECT_PLANS)
}

struct BuildPlanRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    id: Uuid,
    source_revision_id: Uuid,
    project_root: String,
    contract_schema: String,
    canonical_acl: String,
    contract_digest: String,
    proposal_digest: String,
    source_identity_digest: String,
    commit_sha: String,
    source_content_digest: String,
    detector_kind: String,
    detector_revision: String,
    evidence_path: String,
    evidence_digest: String,
    recipe_digest: String,
    aggregate_version: u64,
    accepted_by: Uuid,
    accepted_at: DateTime<Utc>,
}

impl FromRow for BuildPlanRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            id: decode(row, 3)?,
            source_revision_id: decode(row, 4)?,
            project_root: decode(row, 5)?,
            contract_schema: decode(row, 6)?,
            canonical_acl: decode(row, 7)?,
            contract_digest: decode(row, 8)?,
            proposal_digest: decode(row, 9)?,
            source_identity_digest: decode(row, 10)?,
            commit_sha: decode(row, 11)?,
            source_content_digest: decode(row, 12)?,
            detector_kind: decode(row, 13)?,
            detector_revision: decode(row, 14)?,
            evidence_path: decode(row, 15)?,
            evidence_digest: decode(row, 16)?,
            recipe_digest: decode(row, 17)?,
            aggregate_version: decode(row, 18)?,
            accepted_by: decode(row, 19)?,
            accepted_at: decode(row, 20)?,
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

fn decode_plan(row: BuildPlanRow) -> Result<AcceptedBuildPlan, PostgresPersistenceError> {
    let plan = AcceptedBuildPlan::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        EnvironmentId::from_uuid(row.environment_id),
        BuildPlanId::from_uuid(row.id),
        SourceRevisionId::from_uuid(row.source_revision_id),
        &row.canonical_acl,
        &row.contract_digest,
        row.aggregate_version,
        PrincipalId::from_uuid(row.accepted_by),
        row.accepted_at,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored accepted BuildPlan is invalid: {error}"
        ))
    })?;
    let proposal = &plan.contract.spec().proposal;
    let spec = proposal.spec();
    let recipe_digest = spec
        .recipe
        .digest()
        .map_err(PostgresPersistenceError::Invariant)?;
    if row.contract_schema != plan.contract.schema()
        || row.project_root != spec.project_root
        || row.proposal_digest != proposal.digest().as_str()
        || row.source_identity_digest != spec.source.source_identity_digest.as_str()
        || row.commit_sha != spec.source.commit_sha.as_str()
        || row.source_content_digest != spec.source.content_digest.as_str()
        || row.detector_kind != spec.detector.as_str()
        || row.detector_revision != spec.detector_revision
        || row.evidence_path != spec.evidence_path
        || row.evidence_digest != spec.evidence_digest.as_str()
        || row.recipe_digest != recipe_digest
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored accepted BuildPlan columns drifted from its canonical ACL".into(),
        ));
    }
    Ok(plan)
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(format!("could not query accepted BuildPlans: {error}"))
}

#[cfg(test)]
mod migration_tests {
    const MIGRATION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/146_developer_build_plans.sql"
    ));

    #[test]
    fn migration_keeps_acceptance_immutable_and_inside_existing_owner_boundaries() {
        for expected in [
            "create table developer_build_plans",
            "a3s.cloud.build-plan.v1",
            "references external_source_revisions",
            "validate_developer_build_plan_source",
            "sha256(convert_to",
            "accepted BuildPlan does not match its exact Source revision",
            "developer_build_plans_immutable",
            "accepted BuildPlans are immutable",
            "no BuildRun, Workload, Route, or scheduler authority",
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
            "create table schedules",
        ] {
            assert!(
                !MIGRATION.contains(forbidden),
                "migration crossed owning context with {forbidden}"
            );
        }
    }
}
