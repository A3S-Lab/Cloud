use super::postgres::{decode_column, PostgresIdentityRepository};
use super::postgres_platform_rbac::{
    lock_installation, lock_installation_for_authorization, platform_authorization_request,
};
use super::postgres_privileged_authorization_decisions::issue_privileged_authorization;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, store_audit, store_idempotency, store_outbox, transaction_error,
    AuditWrite, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{
    AcceptedTrustDomainRevision, AcceptedWorkloadIdentityPolicyRevision,
};
use crate::modules::identity::domain::events::{
    TrustDomainRevisionAccepted, WorkloadIdentityPolicyRevisionAccepted,
};
use crate::modules::identity::domain::repositories::{
    AcceptTrustDomainRevisionWrite, AcceptWorkloadIdentityPolicyRevisionWrite,
    ITrustDomainRepository, IWorkloadIdentityPolicyRepository, ListTrustDomainRevisions,
    ListWorkloadIdentityPolicyRevisions, ReadCurrentTrustDomain, ReadCurrentWorkloadIdentityPolicy,
    ReadCurrentWorkloadIdentityPolicyForWorkload, ReadTrustDomainRevision,
    ReadWorkloadIdentityPolicyRevision, MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE,
};
use crate::modules::identity::domain::value_objects::PlatformPermission;
use crate::modules::shared_kernel::domain::{
    AuthorizationDecisionRef, IdempotentWrite, InstallationId, OrganizationId, PrincipalId,
    RepositoryError, TrustDomainId, TrustDomainRevisionId, WorkloadIdentityPolicyId,
    WorkloadIdentityPolicyRevisionId,
};
use a3s_cloud_contracts::CloudScopeRef;
use a3s_orm::{sql_query, DecodeError, FromRow, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const TRUST_DOMAIN_ACCEPTED_ACTION: &str = "identity.trust-domain.revision-accepted";
const WORKLOAD_POLICY_ACCEPTED_ACTION: &str = "identity.workload-identity-policy.revision-accepted";
const SELECT_TRUST_DOMAIN_REVISION: &str = "select revision.installation_id, revision.trust_domain_id, revision.id, revision.revision_number, revision.name, revision.canonical_acl, revision.digest, revision.accepted_by, revision.accepted_at from trust_domain_revisions revision";
const SELECT_WORKLOAD_POLICY_REVISION: &str = "select revision.installation_id, revision.organization_id, revision.project_id, revision.environment_id, revision.policy_id, revision.id, revision.revision_number, revision.trust_domain_id, revision.trust_domain_revision_id, revision.workload_id, revision.workload_revision_id, revision.node_pool_id, revision.canonical_acl, revision.digest, revision.accepted_by, revision.accepted_at from workload_identity_policy_revisions revision";

struct TrustDomainRevisionRow {
    installation_id: Uuid,
    trust_domain_id: Uuid,
    id: Uuid,
    revision_number: u64,
    name: String,
    canonical_acl: String,
    digest: String,
    accepted_by: Uuid,
    accepted_at: DateTime<Utc>,
}

impl FromRow for TrustDomainRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            installation_id: decode_column(row, 0)?,
            trust_domain_id: decode_column(row, 1)?,
            id: decode_column(row, 2)?,
            revision_number: decode_column(row, 3)?,
            name: decode_column(row, 4)?,
            canonical_acl: decode_column(row, 5)?,
            digest: decode_column(row, 6)?,
            accepted_by: decode_column(row, 7)?,
            accepted_at: decode_column(row, 8)?,
        })
    }
}

fn decode_trust_domain_revision(
    row: TrustDomainRevisionRow,
) -> Result<AcceptedTrustDomainRevision, RepositoryError> {
    let revision = AcceptedTrustDomainRevision::restore(
        InstallationId::from_uuid(row.installation_id),
        TrustDomainId::from_uuid(row.trust_domain_id),
        TrustDomainRevisionId::from_uuid(row.id),
        row.revision_number,
        &row.canonical_acl,
        &row.digest,
        PrincipalId::from_uuid(row.accepted_by),
        row.accepted_at,
    )
    .map_err(|error| {
        RepositoryError::Storage(format!("stored trust-domain revision is invalid: {error}"))
    })?;
    if revision.contract.spec().name.as_str() != row.name {
        return Err(RepositoryError::Storage(
            "stored trust-domain name projection drifted from canonical ACL".into(),
        ));
    }
    Ok(revision)
}

struct WorkloadPolicyRevisionRow {
    installation_id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    policy_id: Uuid,
    id: Uuid,
    revision_number: u64,
    trust_domain_id: Uuid,
    trust_domain_revision_id: Uuid,
    workload_id: Uuid,
    workload_revision_id: Uuid,
    node_pool_id: Uuid,
    canonical_acl: String,
    digest: String,
    accepted_by: Uuid,
    accepted_at: DateTime<Utc>,
}

impl FromRow for WorkloadPolicyRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            installation_id: decode_column(row, 0)?,
            organization_id: decode_column(row, 1)?,
            project_id: decode_column(row, 2)?,
            environment_id: decode_column(row, 3)?,
            policy_id: decode_column(row, 4)?,
            id: decode_column(row, 5)?,
            revision_number: decode_column(row, 6)?,
            trust_domain_id: decode_column(row, 7)?,
            trust_domain_revision_id: decode_column(row, 8)?,
            workload_id: decode_column(row, 9)?,
            workload_revision_id: decode_column(row, 10)?,
            node_pool_id: decode_column(row, 11)?,
            canonical_acl: decode_column(row, 12)?,
            digest: decode_column(row, 13)?,
            accepted_by: decode_column(row, 14)?,
            accepted_at: decode_column(row, 15)?,
        })
    }
}

fn decode_workload_policy_revision(
    row: WorkloadPolicyRevisionRow,
) -> Result<AcceptedWorkloadIdentityPolicyRevision, RepositoryError> {
    let revision = AcceptedWorkloadIdentityPolicyRevision::restore(
        InstallationId::from_uuid(row.installation_id),
        WorkloadIdentityPolicyId::from_uuid(row.policy_id),
        WorkloadIdentityPolicyRevisionId::from_uuid(row.id),
        row.revision_number,
        &row.canonical_acl,
        &row.digest,
        PrincipalId::from_uuid(row.accepted_by),
        row.accepted_at,
    )
    .map_err(|error| {
        RepositoryError::Storage(format!(
            "stored workload identity policy revision is invalid: {error}"
        ))
    })?;
    let spec = revision.contract.spec();
    if spec.organization_id.as_uuid() != row.organization_id
        || spec.project_id.as_uuid() != row.project_id
        || spec.environment_id.as_uuid() != row.environment_id
        || spec.trust_domain_id.as_uuid() != row.trust_domain_id
        || spec.trust_domain_revision_id.as_uuid() != row.trust_domain_revision_id
        || spec.workload_id.as_uuid() != row.workload_id
        || spec.workload_revision_id.as_uuid() != row.workload_revision_id
        || spec.node_pool_id.as_uuid() != row.node_pool_id
    {
        return Err(RepositoryError::Storage(
            "stored workload identity policy owner projection drifted from canonical ACL".into(),
        ));
    }
    Ok(revision)
}

async fn load_current_trust_domain(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
    trust_domain_id: TrustDomainId,
    lock_clause: &'static str,
) -> Result<Option<AcceptedTrustDomainRevision>, PostgresPersistenceError> {
    fetch_optional::<TrustDomainRevisionRow, _>(
        transaction,
        sql_query::<TrustDomainRevisionRow>(SELECT_TRUST_DOMAIN_REVISION)
            .append(" join trust_domain_heads head on head.installation_id = revision.installation_id and head.trust_domain_id = revision.trust_domain_id and head.revision_id = revision.id and head.revision_number = revision.revision_number where head.installation_id = ")
            .bind(installation_id.as_uuid())
            .append(" and head.trust_domain_id = ")
            .bind(trust_domain_id.as_uuid())
            .append(lock_clause),
    )
    .await?
    .map(decode_trust_domain_revision)
    .transpose()
    .map_err(Into::into)
}

async fn load_trust_domain_revision(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
    trust_domain_id: TrustDomainId,
    revision_id: TrustDomainRevisionId,
) -> Result<Option<AcceptedTrustDomainRevision>, PostgresPersistenceError> {
    fetch_optional::<TrustDomainRevisionRow, _>(
        transaction,
        sql_query::<TrustDomainRevisionRow>(SELECT_TRUST_DOMAIN_REVISION)
            .append(" where revision.installation_id = ")
            .bind(installation_id.as_uuid())
            .append(" and revision.trust_domain_id = ")
            .bind(trust_domain_id.as_uuid())
            .append(" and revision.id = ")
            .bind(revision_id.as_uuid())
            .append(" for share of revision"),
    )
    .await?
    .map(decode_trust_domain_revision)
    .transpose()
    .map_err(Into::into)
}

async fn load_current_workload_policy(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
    organization_id: OrganizationId,
    policy_id: WorkloadIdentityPolicyId,
    lock_clause: &'static str,
) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, PostgresPersistenceError> {
    fetch_optional::<WorkloadPolicyRevisionRow, _>(
        transaction,
        sql_query::<WorkloadPolicyRevisionRow>(SELECT_WORKLOAD_POLICY_REVISION)
            .append(" join workload_identity_policy_heads head on head.organization_id = revision.organization_id and head.policy_id = revision.policy_id and head.revision_id = revision.id and head.revision_number = revision.revision_number where head.installation_id = ")
            .bind(installation_id.as_uuid())
            .append(" and head.organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and head.policy_id = ")
            .bind(policy_id.as_uuid())
            .append(lock_clause),
    )
    .await?
    .map(decode_workload_policy_revision)
    .transpose()
    .map_err(Into::into)
}

async fn load_workload_policy_revision(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
    organization_id: OrganizationId,
    policy_id: WorkloadIdentityPolicyId,
    revision_id: WorkloadIdentityPolicyRevisionId,
) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, PostgresPersistenceError> {
    fetch_optional::<WorkloadPolicyRevisionRow, _>(
        transaction,
        sql_query::<WorkloadPolicyRevisionRow>(SELECT_WORKLOAD_POLICY_REVISION)
            .append(" where revision.installation_id = ")
            .bind(installation_id.as_uuid())
            .append(" and revision.organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and revision.policy_id = ")
            .bind(policy_id.as_uuid())
            .append(" and revision.id = ")
            .bind(revision_id.as_uuid())
            .append(" for share of revision"),
    )
    .await?
    .map(decode_workload_policy_revision)
    .transpose()
    .map_err(Into::into)
}

async fn insert_trust_domain_revision(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedTrustDomainRevision,
    previous_revision_id: Option<TrustDomainRevisionId>,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("insert into trust_domain_revisions (installation_id, trust_domain_id, id, revision_number, previous_revision_id, name, canonical_acl, digest, accepted_by, accepted_at) values (")
            .bind(revision.installation_id.as_uuid())
            .append(", ")
            .bind(revision.trust_domain_id.as_uuid())
            .append(", ")
            .bind(revision.id.as_uuid())
            .append(", ")
            .bind(revision.revision_number)
            .append(", ")
            .bind(previous_revision_id.map(TrustDomainRevisionId::as_uuid))
            .append(", ")
            .bind(revision.contract.spec().name.as_str())
            .append(", ")
            .bind(revision.contract.canonical_acl())
            .append(", ")
            .bind(revision.contract.digest().as_str())
            .append(", ")
            .bind(revision.accepted_by.as_uuid())
            .append(", ")
            .bind(revision.accepted_at)
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating trust-domain revision affected {rows} rows"
        )));
    }
    Ok(())
}

async fn insert_trust_domain_head(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedTrustDomainRevision,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("insert into trust_domain_heads (installation_id, trust_domain_id, revision_id, revision_number, name, updated_at) values (")
            .bind(revision.installation_id.as_uuid())
            .append(", ")
            .bind(revision.trust_domain_id.as_uuid())
            .append(", ")
            .bind(revision.id.as_uuid())
            .append(", ")
            .bind(revision.revision_number)
            .append(", ")
            .bind(revision.contract.spec().name.as_str())
            .append(", ")
            .bind(revision.accepted_at)
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating trust-domain head affected {rows} rows"
        )));
    }
    Ok(())
}

async fn advance_trust_domain_head(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedTrustDomainRevision,
    previous_revision_id: TrustDomainRevisionId,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("update trust_domain_heads set revision_id = ")
            .bind(revision.id.as_uuid())
            .append(", revision_number = ")
            .bind(revision.revision_number)
            .append(", updated_at = ")
            .bind(revision.accepted_at)
            .append(" where installation_id = ")
            .bind(revision.installation_id.as_uuid())
            .append(" and trust_domain_id = ")
            .bind(revision.trust_domain_id.as_uuid())
            .append(" and revision_id = ")
            .bind(previous_revision_id.as_uuid()),
    )
    .await?;
    if rows != 1 {
        return Err(RepositoryError::Conflict(
            "trust-domain head changed before acceptance".into(),
        )
        .into());
    }
    Ok(())
}

async fn insert_workload_policy_revision(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedWorkloadIdentityPolicyRevision,
    previous_revision_id: Option<WorkloadIdentityPolicyRevisionId>,
) -> Result<(), PostgresPersistenceError> {
    let spec = revision.contract.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into workload_identity_policy_revisions (installation_id, organization_id, project_id, environment_id, policy_id, id, revision_number, previous_revision_id, trust_domain_id, trust_domain_revision_id, workload_id, workload_revision_id, node_pool_id, canonical_acl, digest, accepted_by, accepted_at) values (")
            .bind(revision.installation_id.as_uuid())
            .append(", ")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.project_id.as_uuid())
            .append(", ")
            .bind(spec.environment_id.as_uuid())
            .append(", ")
            .bind(revision.policy_id.as_uuid())
            .append(", ")
            .bind(revision.id.as_uuid())
            .append(", ")
            .bind(revision.revision_number)
            .append(", ")
            .bind(previous_revision_id.map(WorkloadIdentityPolicyRevisionId::as_uuid))
            .append(", ")
            .bind(spec.trust_domain_id.as_uuid())
            .append(", ")
            .bind(spec.trust_domain_revision_id.as_uuid())
            .append(", ")
            .bind(spec.workload_id.as_uuid())
            .append(", ")
            .bind(spec.workload_revision_id.as_uuid())
            .append(", ")
            .bind(spec.node_pool_id.as_uuid())
            .append(", ")
            .bind(revision.contract.canonical_acl())
            .append(", ")
            .bind(revision.contract.digest().as_str())
            .append(", ")
            .bind(revision.accepted_by.as_uuid())
            .append(", ")
            .bind(revision.accepted_at)
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating workload identity policy revision affected {rows} rows"
        )));
    }
    Ok(())
}

async fn insert_workload_policy_head(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedWorkloadIdentityPolicyRevision,
) -> Result<(), PostgresPersistenceError> {
    let spec = revision.contract.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into workload_identity_policy_heads (installation_id, organization_id, project_id, environment_id, policy_id, workload_id, revision_id, revision_number, trust_domain_id, trust_domain_revision_id, updated_at) values (")
            .bind(revision.installation_id.as_uuid())
            .append(", ")
            .bind(spec.organization_id.as_uuid())
            .append(", ")
            .bind(spec.project_id.as_uuid())
            .append(", ")
            .bind(spec.environment_id.as_uuid())
            .append(", ")
            .bind(revision.policy_id.as_uuid())
            .append(", ")
            .bind(spec.workload_id.as_uuid())
            .append(", ")
            .bind(revision.id.as_uuid())
            .append(", ")
            .bind(revision.revision_number)
            .append(", ")
            .bind(spec.trust_domain_id.as_uuid())
            .append(", ")
            .bind(spec.trust_domain_revision_id.as_uuid())
            .append(", ")
            .bind(revision.accepted_at)
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating workload identity policy head affected {rows} rows"
        )));
    }
    Ok(())
}

async fn advance_workload_policy_head(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedWorkloadIdentityPolicyRevision,
    previous_revision_id: WorkloadIdentityPolicyRevisionId,
) -> Result<(), PostgresPersistenceError> {
    let spec = revision.contract.spec();
    let rows = execute(
        transaction,
        sql_query::<()>("update workload_identity_policy_heads set revision_id = ")
            .bind(revision.id.as_uuid())
            .append(", revision_number = ")
            .bind(revision.revision_number)
            .append(", trust_domain_id = ")
            .bind(spec.trust_domain_id.as_uuid())
            .append(", trust_domain_revision_id = ")
            .bind(spec.trust_domain_revision_id.as_uuid())
            .append(", updated_at = ")
            .bind(revision.accepted_at)
            .append(" where installation_id = ")
            .bind(revision.installation_id.as_uuid())
            .append(" and organization_id = ")
            .bind(spec.organization_id.as_uuid())
            .append(" and policy_id = ")
            .bind(revision.policy_id.as_uuid())
            .append(" and revision_id = ")
            .bind(previous_revision_id.as_uuid()),
    )
    .await?;
    if rows != 1 {
        return Err(RepositoryError::Conflict(
            "workload identity policy head changed before acceptance".into(),
        )
        .into());
    }
    Ok(())
}

async fn store_trust_domain_facts(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedTrustDomainRevision,
    previous_revision_id: Option<TrustDomainRevisionId>,
    authorization: &AuthorizationDecisionRef,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_outbox(
        transaction,
        &TrustDomainRevisionAccepted::envelope(revision, request_id)?,
    )
    .await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            scope: CloudScopeRef::Installation {
                installation_id: revision.installation_id.as_uuid(),
            },
            actor_id: Some(revision.accepted_by.as_uuid()),
            action: TRUST_DOMAIN_ACCEPTED_ACTION,
            aggregate_id: revision.trust_domain_id.as_uuid(),
            occurred_at: revision.accepted_at,
            request_id,
            details: serde_json::json!({
                "revisionId": revision.id,
                "revisionNumber": revision.revision_number,
                "previousRevisionId": previous_revision_id,
                "name": revision.contract.spec().name,
                "digest": revision.contract.digest(),
                "authorizationDecision": authorization,
            }),
        },
    )
    .await
}

async fn store_workload_policy_facts(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedWorkloadIdentityPolicyRevision,
    previous_revision_id: Option<WorkloadIdentityPolicyRevisionId>,
    authorization: &AuthorizationDecisionRef,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_outbox(
        transaction,
        &WorkloadIdentityPolicyRevisionAccepted::envelope(revision, request_id)?,
    )
    .await?;
    let spec = revision.contract.spec();
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            scope: CloudScopeRef::Environment {
                organization_id: spec.organization_id.as_uuid(),
                project_id: spec.project_id.as_uuid(),
                environment_id: spec.environment_id.as_uuid(),
            },
            actor_id: Some(revision.accepted_by.as_uuid()),
            action: WORKLOAD_POLICY_ACCEPTED_ACTION,
            aggregate_id: revision.policy_id.as_uuid(),
            occurred_at: revision.accepted_at,
            request_id,
            details: serde_json::json!({
                "revisionId": revision.id,
                "revisionNumber": revision.revision_number,
                "previousRevisionId": previous_revision_id,
                "trustDomainId": spec.trust_domain_id,
                "trustDomainRevisionId": spec.trust_domain_revision_id,
                "workloadId": spec.workload_id,
                "workloadRevisionId": spec.workload_revision_id,
                "digest": revision.contract.digest(),
                "authorizationDecision": authorization,
            }),
        },
    )
    .await
}

fn validate_replayed_trust_domain(
    replayed: IdempotentWrite<AcceptedTrustDomainRevision>,
    installation_id: InstallationId,
    trust_domain_id: TrustDomainId,
) -> Result<IdempotentWrite<AcceptedTrustDomainRevision>, PostgresPersistenceError> {
    replayed
        .value
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    if replayed.value.installation_id != installation_id
        || replayed.value.trust_domain_id != trust_domain_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "idempotent trust-domain response crossed its aggregate scope".into(),
        ));
    }
    Ok(replayed)
}

fn validate_replayed_workload_policy(
    replayed: IdempotentWrite<AcceptedWorkloadIdentityPolicyRevision>,
    installation_id: InstallationId,
    organization_id: OrganizationId,
    policy_id: WorkloadIdentityPolicyId,
) -> Result<IdempotentWrite<AcceptedWorkloadIdentityPolicyRevision>, PostgresPersistenceError> {
    replayed
        .value
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    if replayed.value.installation_id != installation_id
        || replayed.value.contract.spec().organization_id != organization_id
        || replayed.value.policy_id != policy_id
    {
        return Err(PostgresPersistenceError::Invariant(
            "idempotent workload identity policy response crossed its aggregate scope".into(),
        ));
    }
    Ok(replayed)
}

fn validate_page_limit(limit: usize) -> Result<u64, RepositoryError> {
    if !(1..=MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE).contains(&limit) {
        return Err(RepositoryError::Storage(
            "workload identity revision page limit is outside bounds".into(),
        ));
    }
    u64::try_from(limit)
        .map_err(|_| RepositoryError::Storage("revision page limit cannot be represented".into()))
}

#[async_trait]
impl ITrustDomainRepository for PostgresIdentityRepository {
    async fn accept(
        &self,
        write: AcceptTrustDomainRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedTrustDomainRevision>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let installation_id = write.revision.installation_id;
                    let trust_domain_id = write.revision.trust_domain_id;
                    lock_installation(transaction, installation_id).await?;
                    let authorization = issue_privileged_authorization(
                        transaction,
                        platform_authorization_request(
                            installation_id,
                            write.actor_principal_id,
                            write.credential_id,
                            PlatformPermission::WorkloadTrustManage,
                            TRUST_DOMAIN_ACCEPTED_ACTION,
                            trust_domain_id.as_uuid(),
                            write.request_id,
                        )?,
                    )
                    .await?;
                    if let Some(replayed) = idempotency_replay::<AcceptedTrustDomainRevision>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return validate_replayed_trust_domain(
                            replayed,
                            installation_id,
                            trust_domain_id,
                        );
                    }
                    let current = load_current_trust_domain(
                        transaction,
                        installation_id,
                        trust_domain_id,
                        " for update of head",
                    )
                    .await?;
                    match current.as_ref() {
                        None => {
                            if write.revision.revision_number != 1
                                || write.expected_previous_revision_id.is_some()
                            {
                                return Err(RepositoryError::Conflict(
                                    "initial trust-domain revision requires an empty head".into(),
                                )
                                .into());
                            }
                        }
                        Some(previous) => {
                            if write.expected_previous_revision_id != Some(previous.id)
                                || write.revision.revision_number
                                    != previous.revision_number.checked_add(1).ok_or_else(|| {
                                        PostgresPersistenceError::Invariant(
                                            "trust-domain revision is exhausted".into(),
                                        )
                                    })?
                                || write.revision.contract.spec().name
                                    != previous.contract.spec().name
                                || write.revision.accepted_at < previous.accepted_at
                            {
                                return Err(RepositoryError::Conflict(
                                    "trust-domain revision is not the exact current successor"
                                        .into(),
                                )
                                .into());
                            }
                        }
                    }
                    if let Err(error) = insert_trust_domain_revision(
                        transaction,
                        &write.revision,
                        write.expected_previous_revision_id,
                    )
                    .await
                    {
                        if is_unique_violation(&error) {
                            return Err(RepositoryError::Conflict(
                                "trust-domain revision or name already exists".into(),
                            )
                            .into());
                        }
                        if is_foreign_key_violation(&error) {
                            return Err(RepositoryError::NotFound.into());
                        }
                        return Err(error);
                    }
                    match write.expected_previous_revision_id {
                        Some(previous_revision_id) => {
                            advance_trust_domain_head(
                                transaction,
                                &write.revision,
                                previous_revision_id,
                            )
                            .await?;
                        }
                        None => {
                            if let Err(error) =
                                insert_trust_domain_head(transaction, &write.revision).await
                            {
                                if is_unique_violation(&error) {
                                    return Err(RepositoryError::Conflict(
                                        "trust-domain head or name already exists".into(),
                                    )
                                    .into());
                                }
                                return Err(error);
                            }
                        }
                    }
                    store_trust_domain_facts(
                        transaction,
                        &write.revision,
                        write.expected_previous_revision_id,
                        &authorization.reference,
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &write.revision).await?;
                    Ok(IdempotentWrite {
                        value: write.revision,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn read_revision(
        &self,
        read: ReadTrustDomainRevision,
    ) -> Result<Option<AcceptedTrustDomainRevision>, RepositoryError> {
        read.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_installation_for_authorization(transaction, read.installation_id).await?;
                    issue_privileged_authorization(
                        transaction,
                        platform_authorization_request(
                            read.installation_id,
                            read.actor_principal_id,
                            read.credential_id,
                            PlatformPermission::WorkloadTrustRead,
                            "identity.trust-domain.revision-read",
                            read.revision_id.as_uuid(),
                            read.request_id,
                        )?,
                    )
                    .await?;
                    load_trust_domain_revision(
                        transaction,
                        read.installation_id,
                        read.trust_domain_id,
                        read.revision_id,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn read_current(
        &self,
        read: ReadCurrentTrustDomain,
    ) -> Result<Option<AcceptedTrustDomainRevision>, RepositoryError> {
        read.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_installation_for_authorization(transaction, read.installation_id).await?;
                    issue_privileged_authorization(
                        transaction,
                        platform_authorization_request(
                            read.installation_id,
                            read.actor_principal_id,
                            read.credential_id,
                            PlatformPermission::WorkloadTrustRead,
                            "identity.trust-domain.current-read",
                            read.trust_domain_id.as_uuid(),
                            read.request_id,
                        )?,
                    )
                    .await?;
                    load_current_trust_domain(
                        transaction,
                        read.installation_id,
                        read.trust_domain_id,
                        " for share of head",
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_revisions(
        &self,
        read: ListTrustDomainRevisions,
    ) -> Result<Vec<AcceptedTrustDomainRevision>, RepositoryError> {
        read.validate().map_err(RepositoryError::Storage)?;
        let limit = validate_page_limit(read.limit)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_installation_for_authorization(transaction, read.installation_id).await?;
                    issue_privileged_authorization(
                        transaction,
                        platform_authorization_request(
                            read.installation_id,
                            read.actor_principal_id,
                            read.credential_id,
                            PlatformPermission::WorkloadTrustRead,
                            "identity.trust-domain.revisions-read",
                            read.trust_domain_id.as_uuid(),
                            read.request_id,
                        )?,
                    )
                    .await?;
                    fetch_all::<TrustDomainRevisionRow, _>(
                        transaction,
                        sql_query::<TrustDomainRevisionRow>(SELECT_TRUST_DOMAIN_REVISION)
                            .append(" where revision.installation_id = ")
                            .bind(read.installation_id.as_uuid())
                            .append(" and revision.trust_domain_id = ")
                            .bind(read.trust_domain_id.as_uuid())
                            .append(
                                " order by revision.revision_number desc, revision.id desc limit ",
                            )
                            .bind(limit)
                            .append(" for share of revision"),
                    )
                    .await?
                    .into_iter()
                    .map(decode_trust_domain_revision)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(Into::into)
                })
            })
            .await
            .map_err(transaction_error)
    }
}

#[async_trait]
impl IWorkloadIdentityPolicyRepository for PostgresIdentityRepository {
    async fn accept(
        &self,
        write: AcceptWorkloadIdentityPolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    write
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let installation_id = write.revision.installation_id;
                    let organization_id = write.revision.contract.spec().organization_id;
                    let policy_id = write.revision.policy_id;
                    lock_installation(transaction, installation_id).await?;
                    let authorization = issue_privileged_authorization(
                        transaction,
                        platform_authorization_request(
                            installation_id,
                            write.actor_principal_id,
                            write.credential_id,
                            PlatformPermission::WorkloadTrustManage,
                            WORKLOAD_POLICY_ACCEPTED_ACTION,
                            policy_id.as_uuid(),
                            write.request_id,
                        )?,
                    )
                    .await?;
                    if let Some(replayed) = idempotency_replay::<
                        AcceptedWorkloadIdentityPolicyRevision,
                    >(transaction, &write.idempotency)
                    .await?
                    {
                        return validate_replayed_workload_policy(
                            replayed,
                            installation_id,
                            organization_id,
                            policy_id,
                        );
                    }
                    let spec = write.revision.contract.spec();
                    let trust_domain = load_current_trust_domain(
                        transaction,
                        installation_id,
                        spec.trust_domain_id,
                        " for update of head",
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if spec.trust_domain_revision_id != trust_domain.id {
                        return Err(RepositoryError::Conflict(
                            "workload identity policy references a stale trust-domain revision"
                                .into(),
                        )
                        .into());
                    }
                    write
                        .revision
                        .validate_against_trust_domain(&trust_domain)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let current = load_current_workload_policy(
                        transaction,
                        installation_id,
                        organization_id,
                        policy_id,
                        " for update of head",
                    )
                    .await?;
                    match current.as_ref() {
                        None => {
                            if write.revision.revision_number != 1
                                || write.expected_previous_revision_id.is_some()
                            {
                                return Err(RepositoryError::Conflict(
                                    "initial workload identity policy requires an empty head"
                                        .into(),
                                )
                                .into());
                            }
                        }
                        Some(previous) => {
                            let previous_spec = previous.contract.spec();
                            if write.expected_previous_revision_id != Some(previous.id)
                                || write.revision.revision_number
                                    != previous.revision_number.checked_add(1).ok_or_else(|| {
                                        PostgresPersistenceError::Invariant(
                                            "workload identity policy revision is exhausted".into(),
                                        )
                                    })?
                                || spec.organization_id != previous_spec.organization_id
                                || spec.project_id != previous_spec.project_id
                                || spec.environment_id != previous_spec.environment_id
                                || spec.workload_id != previous_spec.workload_id
                                || write.revision.accepted_at < previous.accepted_at
                            {
                                return Err(RepositoryError::Conflict(
                                    "workload identity policy is not the exact current successor"
                                        .into(),
                                )
                                .into());
                            }
                        }
                    }
                    if let Err(error) = insert_workload_policy_revision(
                        transaction,
                        &write.revision,
                        write.expected_previous_revision_id,
                    )
                    .await
                    {
                        if is_unique_violation(&error) {
                            return Err(RepositoryError::Conflict(
                                "workload identity policy revision already exists".into(),
                            )
                            .into());
                        }
                        if is_foreign_key_violation(&error) {
                            return Err(RepositoryError::NotFound.into());
                        }
                        return Err(error);
                    }
                    match write.expected_previous_revision_id {
                        Some(previous_revision_id) => {
                            advance_workload_policy_head(
                                transaction,
                                &write.revision,
                                previous_revision_id,
                            )
                            .await?;
                        }
                        None => {
                            if let Err(error) =
                                insert_workload_policy_head(transaction, &write.revision).await
                            {
                                if is_unique_violation(&error) {
                                    return Err(RepositoryError::Conflict(
                                        "logical Workload already has a current identity policy"
                                            .into(),
                                    )
                                    .into());
                                }
                                return Err(error);
                            }
                        }
                    }
                    store_workload_policy_facts(
                        transaction,
                        &write.revision,
                        write.expected_previous_revision_id,
                        &authorization.reference,
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &write.revision).await?;
                    Ok(IdempotentWrite {
                        value: write.revision,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn read_revision(
        &self,
        read: ReadWorkloadIdentityPolicyRevision,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError> {
        read.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_installation_for_authorization(transaction, read.installation_id).await?;
                    issue_privileged_authorization(
                        transaction,
                        platform_authorization_request(
                            read.installation_id,
                            read.actor_principal_id,
                            read.credential_id,
                            PlatformPermission::WorkloadTrustRead,
                            "identity.workload-identity-policy.revision-read",
                            read.revision_id.as_uuid(),
                            read.request_id,
                        )?,
                    )
                    .await?;
                    load_workload_policy_revision(
                        transaction,
                        read.installation_id,
                        read.organization_id,
                        read.policy_id,
                        read.revision_id,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn read_current(
        &self,
        read: ReadCurrentWorkloadIdentityPolicy,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError> {
        read.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_installation_for_authorization(transaction, read.installation_id).await?;
                    issue_privileged_authorization(
                        transaction,
                        platform_authorization_request(
                            read.installation_id,
                            read.actor_principal_id,
                            read.credential_id,
                            PlatformPermission::WorkloadTrustRead,
                            "identity.workload-identity-policy.current-read",
                            read.policy_id.as_uuid(),
                            read.request_id,
                        )?,
                    )
                    .await?;
                    load_current_workload_policy(
                        transaction,
                        read.installation_id,
                        read.organization_id,
                        read.policy_id,
                        " for share of head",
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn read_current_for_workload(
        &self,
        read: ReadCurrentWorkloadIdentityPolicyForWorkload,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError> {
        read.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_installation_for_authorization(transaction, read.installation_id).await?;
                    issue_privileged_authorization(
                        transaction,
                        platform_authorization_request(
                            read.installation_id,
                            read.actor_principal_id,
                            read.credential_id,
                            PlatformPermission::WorkloadTrustRead,
                            "identity.workload-identity-policy.workload-current-read",
                            read.workload_id.as_uuid(),
                            read.request_id,
                        )?,
                    )
                    .await?;
                    fetch_optional::<WorkloadPolicyRevisionRow, _>(
                        transaction,
                        sql_query::<WorkloadPolicyRevisionRow>(SELECT_WORKLOAD_POLICY_REVISION)
                            .append(" join workload_identity_policy_heads head on head.organization_id = revision.organization_id and head.policy_id = revision.policy_id and head.revision_id = revision.id and head.revision_number = revision.revision_number where head.installation_id = ")
                            .bind(read.installation_id.as_uuid())
                            .append(" and head.organization_id = ")
                            .bind(read.organization_id.as_uuid())
                            .append(" and head.workload_id = ")
                            .bind(read.workload_id.as_uuid())
                            .append(" for share of head, revision"),
                    )
                    .await?
                    .map(decode_workload_policy_revision)
                    .transpose()
                    .map_err(Into::into)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_revisions(
        &self,
        read: ListWorkloadIdentityPolicyRevisions,
    ) -> Result<Vec<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError> {
        read.validate().map_err(RepositoryError::Storage)?;
        let limit = validate_page_limit(read.limit)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_installation_for_authorization(transaction, read.installation_id).await?;
                    issue_privileged_authorization(
                        transaction,
                        platform_authorization_request(
                            read.installation_id,
                            read.actor_principal_id,
                            read.credential_id,
                            PlatformPermission::WorkloadTrustRead,
                            "identity.workload-identity-policy.revisions-read",
                            read.policy_id.as_uuid(),
                            read.request_id,
                        )?,
                    )
                    .await?;
                    fetch_all::<WorkloadPolicyRevisionRow, _>(
                        transaction,
                        sql_query::<WorkloadPolicyRevisionRow>(SELECT_WORKLOAD_POLICY_REVISION)
                            .append(" where revision.installation_id = ")
                            .bind(read.installation_id.as_uuid())
                            .append(" and revision.organization_id = ")
                            .bind(read.organization_id.as_uuid())
                            .append(" and revision.policy_id = ")
                            .bind(read.policy_id.as_uuid())
                            .append(
                                " order by revision.revision_number desc, revision.id desc limit ",
                            )
                            .bind(limit)
                            .append(" for share of revision"),
                    )
                    .await?
                    .into_iter()
                    .map(decode_workload_policy_revision)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(Into::into)
                })
            })
            .await
            .map_err(transaction_error)
    }
}
