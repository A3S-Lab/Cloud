use super::postgres::{decode_column, PostgresIdentityRepository};
use super::postgres_platform_rbac::{
    load_active_actor_binding, load_active_principal_for_share, load_current_policy_for_update,
    lock_installation, require_permission,
};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, store_audit, store_idempotency, store_outbox, transaction_error,
    AuditWrite, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{
    IdentityPrincipalKind, TenantSupportGrant, TenantSupportGrantApproval,
    TenantSupportGrantApprovalOutcome, TenantSupportGrantProposal,
};
use crate::modules::identity::domain::events::{
    TenantSupportGrantApproved, TenantSupportGrantChanged, TenantSupportGrantProposed,
};
use crate::modules::identity::domain::repositories::{
    ApproveTenantSupportGrantWrite, ITenantSupportGrantRepository, ProposeTenantSupportGrantWrite,
    RevokeTenantSupportGrantWrite,
};
use crate::modules::identity::domain::value_objects::{
    PlatformPermission, TenantSupportGrantContract,
};
use crate::modules::shared_kernel::domain::{
    DecisionEvidenceRef, IdempotentWrite, InstallationId, PlatformRoleBindingId,
    PlatformRolePolicyRevisionId, PrincipalId, RepositoryError, Sha256Digest, TenantSupportGrantId,
};
use a3s_orm::{sql_query, Database, DecodeError, FromRow, PostgresDialect, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_TENANT_SUPPORT_PROPOSAL: &str = "select intent.id, intent.installation_id, intent.scope_kind, intent.organization_id, intent.project_id, intent.environment_id, intent.principal_id, intent.canonical_acl, intent.digest, intent.required_approval_count, intent.requested_by, intent.authentication_id, intent.authentication_digest, intent.requested_at, intent.starts_at, intent.expires_at from tenant_support_grant_intents intent";
const SELECT_TENANT_SUPPORT_APPROVAL: &str = "select approval.grant_id, approval.approver_id, approval.contract_digest, approval.authentication_id, approval.authentication_digest, approval.policy_revision_id, approval.policy_digest, approval.binding_id, approval.binding_version, approval.approved_at, approval.evidence_digest from tenant_support_grant_approvals approval";
const SELECT_TENANT_SUPPORT_GRANT: &str = "select intent.id, intent.installation_id, intent.scope_kind, intent.organization_id, intent.project_id, intent.environment_id, intent.principal_id, intent.canonical_acl, intent.digest, intent.required_approval_count, intent.requested_by, intent.authentication_id, intent.authentication_digest, intent.requested_at, intent.starts_at, intent.expires_at, accepted_grant.aggregate_version, accepted_grant.revocation_generation, accepted_grant.accepted_at, accepted_grant.revoked_at, accepted_grant.revoked_by from tenant_support_grants accepted_grant join tenant_support_grant_intents intent on intent.id = accepted_grant.id";

struct TenantSupportProposalRow {
    id: Uuid,
    installation_id: Uuid,
    scope_kind: String,
    organization_id: Uuid,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    principal_id: Uuid,
    canonical_acl: String,
    digest: String,
    required_approval_count: i16,
    requested_by: Uuid,
    authentication_id: String,
    authentication_digest: String,
    requested_at: DateTime<Utc>,
    starts_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl FromRow for TenantSupportProposalRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode_column(row, 0)?,
            installation_id: decode_column(row, 1)?,
            scope_kind: decode_column(row, 2)?,
            organization_id: decode_column(row, 3)?,
            project_id: decode_column(row, 4)?,
            environment_id: decode_column(row, 5)?,
            principal_id: decode_column(row, 6)?,
            canonical_acl: decode_column(row, 7)?,
            digest: decode_column(row, 8)?,
            required_approval_count: decode_column(row, 9)?,
            requested_by: decode_column(row, 10)?,
            authentication_id: decode_column(row, 11)?,
            authentication_digest: decode_column(row, 12)?,
            requested_at: decode_column(row, 13)?,
            starts_at: decode_column(row, 14)?,
            expires_at: decode_column(row, 15)?,
        })
    }
}

fn decode_tenant_support_proposal(
    row: TenantSupportProposalRow,
) -> Result<TenantSupportGrantProposal, RepositoryError> {
    let contract =
        TenantSupportGrantContract::restore(&row.canonical_acl, &row.digest).map_err(|error| {
            RepositoryError::Storage(format!(
                "stored tenant support grant ACL is invalid: {error}"
            ))
        })?;
    let spec = contract.spec();
    let scope = spec.scope;
    if row.id != spec.grant_id.as_uuid()
        || row.installation_id != spec.installation_id().as_uuid()
        || row.scope_kind != scope.kind()
        || row.organization_id
            != scope
                .organization_id()
                .map_or(Uuid::nil(), |id| id.as_uuid())
        || row.project_id != scope.project_id().map(|id| id.as_uuid())
        || row.environment_id != scope.environment_id().map(|id| id.as_uuid())
        || row.principal_id != spec.principal_id.as_uuid()
        || usize::try_from(row.required_approval_count).ok() != Some(spec.approver_ids.len())
        || row.starts_at != spec.starts_at
        || row.expires_at != spec.expires_at
    {
        return Err(RepositoryError::Storage(
            "stored tenant support grant index projections drifted from canonical ACL".into(),
        ));
    }
    TenantSupportGrantProposal::propose(
        contract,
        PrincipalId::from_uuid(row.requested_by),
        DecisionEvidenceRef::new(
            row.authentication_id,
            Sha256Digest::parse(row.authentication_digest).map_err(RepositoryError::Storage)?,
        )
        .map_err(RepositoryError::Storage)?,
        row.requested_at,
    )
    .map_err(|error| {
        RepositoryError::Storage(format!(
            "stored tenant support grant proposal is invalid: {error}"
        ))
    })
}

struct TenantSupportApprovalRow {
    grant_id: Uuid,
    approver_id: Uuid,
    contract_digest: String,
    authentication_id: String,
    authentication_digest: String,
    policy_revision_id: Uuid,
    policy_digest: String,
    binding_id: Uuid,
    binding_version: u64,
    approved_at: DateTime<Utc>,
    evidence_digest: String,
}

impl FromRow for TenantSupportApprovalRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            grant_id: decode_column(row, 0)?,
            approver_id: decode_column(row, 1)?,
            contract_digest: decode_column(row, 2)?,
            authentication_id: decode_column(row, 3)?,
            authentication_digest: decode_column(row, 4)?,
            policy_revision_id: decode_column(row, 5)?,
            policy_digest: decode_column(row, 6)?,
            binding_id: decode_column(row, 7)?,
            binding_version: decode_column(row, 8)?,
            approved_at: decode_column(row, 9)?,
            evidence_digest: decode_column(row, 10)?,
        })
    }
}

fn decode_tenant_support_approval(
    row: TenantSupportApprovalRow,
    proposal: &TenantSupportGrantProposal,
) -> Result<TenantSupportGrantApproval, RepositoryError> {
    let approval = TenantSupportGrantApproval {
        grant_id: TenantSupportGrantId::from_uuid(row.grant_id),
        contract_digest: Sha256Digest::parse(row.contract_digest)
            .map_err(RepositoryError::Storage)?,
        approver_id: PrincipalId::from_uuid(row.approver_id),
        authentication: DecisionEvidenceRef::new(
            row.authentication_id,
            Sha256Digest::parse(row.authentication_digest).map_err(RepositoryError::Storage)?,
        )
        .map_err(RepositoryError::Storage)?,
        policy_revision_id: PlatformRolePolicyRevisionId::from_uuid(row.policy_revision_id),
        policy_digest: Sha256Digest::parse(row.policy_digest).map_err(RepositoryError::Storage)?,
        binding_id: PlatformRoleBindingId::from_uuid(row.binding_id),
        binding_version: row.binding_version,
        approved_at: row.approved_at,
        digest: Sha256Digest::parse(row.evidence_digest).map_err(RepositoryError::Storage)?,
    };
    approval.validate_against(proposal).map_err(|error| {
        RepositoryError::Storage(format!(
            "stored tenant support grant approval is invalid: {error}"
        ))
    })?;
    Ok(approval)
}

struct TenantSupportGrantRow {
    proposal: TenantSupportProposalRow,
    aggregate_version: u64,
    revocation_generation: u64,
    accepted_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    revoked_by: Option<Uuid>,
}

impl FromRow for TenantSupportGrantRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            proposal: TenantSupportProposalRow {
                id: decode_column(row, 0)?,
                installation_id: decode_column(row, 1)?,
                scope_kind: decode_column(row, 2)?,
                organization_id: decode_column(row, 3)?,
                project_id: decode_column(row, 4)?,
                environment_id: decode_column(row, 5)?,
                principal_id: decode_column(row, 6)?,
                canonical_acl: decode_column(row, 7)?,
                digest: decode_column(row, 8)?,
                required_approval_count: decode_column(row, 9)?,
                requested_by: decode_column(row, 10)?,
                authentication_id: decode_column(row, 11)?,
                authentication_digest: decode_column(row, 12)?,
                requested_at: decode_column(row, 13)?,
                starts_at: decode_column(row, 14)?,
                expires_at: decode_column(row, 15)?,
            },
            aggregate_version: decode_column(row, 16)?,
            revocation_generation: decode_column(row, 17)?,
            accepted_at: decode_column(row, 18)?,
            revoked_at: decode_column(row, 19)?,
            revoked_by: decode_column(row, 20)?,
        })
    }
}

fn decode_tenant_support_grant(
    row: TenantSupportGrantRow,
) -> Result<TenantSupportGrant, RepositoryError> {
    let proposal = decode_tenant_support_proposal(row.proposal)?;
    TenantSupportGrant::restore(
        proposal.id,
        proposal.contract.canonical_acl(),
        proposal.contract.digest().as_str(),
        row.aggregate_version,
        row.revocation_generation,
        row.accepted_at,
        row.revoked_at,
        row.revoked_by.map(PrincipalId::from_uuid),
    )
    .map_err(|error| {
        RepositoryError::Storage(format!("stored tenant support grant is invalid: {error}"))
    })
}

async fn load_proposal_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
    grant_id: TenantSupportGrantId,
) -> Result<Option<TenantSupportGrantProposal>, PostgresPersistenceError> {
    fetch_optional::<TenantSupportProposalRow, _>(
        transaction,
        sql_query::<TenantSupportProposalRow>(SELECT_TENANT_SUPPORT_PROPOSAL)
            .append(" where intent.installation_id = ")
            .bind(installation_id.as_uuid())
            .append(" and intent.id = ")
            .bind(grant_id.as_uuid())
            .append(" for update of intent"),
    )
    .await?
    .map(decode_tenant_support_proposal)
    .transpose()
    .map_err(Into::into)
}

async fn load_approvals(
    transaction: &a3s_orm::PostgresTransaction,
    proposal: &TenantSupportGrantProposal,
) -> Result<Vec<TenantSupportGrantApproval>, PostgresPersistenceError> {
    fetch_all::<TenantSupportApprovalRow, _>(
        transaction,
        sql_query::<TenantSupportApprovalRow>(SELECT_TENANT_SUPPORT_APPROVAL)
            .append(" where approval.grant_id = ")
            .bind(proposal.id.as_uuid())
            .append(" order by approval.approved_at asc, approval.approver_id asc"),
    )
    .await?
    .into_iter()
    .map(|row| decode_tenant_support_approval(row, proposal))
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

async fn load_grant_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
    grant_id: TenantSupportGrantId,
) -> Result<Option<TenantSupportGrant>, PostgresPersistenceError> {
    fetch_optional::<TenantSupportGrantRow, _>(
        transaction,
        sql_query::<TenantSupportGrantRow>(SELECT_TENANT_SUPPORT_GRANT)
            .append(" where intent.installation_id = ")
            .bind(installation_id.as_uuid())
            .append(" and accepted_grant.id = ")
            .bind(grant_id.as_uuid())
            .append(" for update of accepted_grant"),
    )
    .await?
    .map(decode_tenant_support_grant)
    .transpose()
    .map_err(Into::into)
}

async fn require_active_human(
    transaction: &a3s_orm::PostgresTransaction,
    principal_id: PrincipalId,
    purpose: &str,
) -> Result<(), PostgresPersistenceError> {
    let principal = load_active_principal_for_share(transaction, principal_id)
        .await?
        .ok_or_else(|| {
            RepositoryError::Forbidden(format!(
                "tenant support {purpose} must be an active human Principal"
            ))
        })?;
    if principal.kind != IdentityPrincipalKind::Human {
        return Err(RepositoryError::Forbidden(format!(
            "tenant support {purpose} must be an active human Principal"
        ))
        .into());
    }
    Ok(())
}

async fn require_current_support_manager(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
    principal_id: PrincipalId,
    policy: &crate::modules::identity::domain::entities::AcceptedPlatformRolePolicyRevision,
) -> Result<crate::modules::identity::domain::entities::PlatformRoleBinding, PostgresPersistenceError>
{
    require_active_human(transaction, principal_id, "approver").await?;
    let binding = load_active_actor_binding(transaction, installation_id, principal_id).await?;
    require_permission(policy, &binding, PlatformPermission::TenantSupportManage)?;
    Ok(binding)
}

async fn insert_proposal(
    transaction: &a3s_orm::PostgresTransaction,
    proposal: &TenantSupportGrantProposal,
) -> Result<(), PostgresPersistenceError> {
    let spec = proposal.contract.spec();
    let scope = spec.scope;
    let rows = execute(
        transaction,
        sql_query::<()>("insert into tenant_support_grant_intents (id, installation_id, scope_kind, organization_id, project_id, environment_id, principal_id, canonical_acl, digest, required_approval_count, requested_by, authentication_id, authentication_digest, requested_at, starts_at, expires_at) values (")
            .bind(proposal.id.as_uuid())
            .append(", ")
            .bind(spec.installation_id().as_uuid())
            .append(", ")
            .bind(scope.kind())
            .append(", ")
            .bind(scope.organization_id().map(|id| id.as_uuid()))
            .append(", ")
            .bind(scope.project_id().map(|id| id.as_uuid()))
            .append(", ")
            .bind(scope.environment_id().map(|id| id.as_uuid()))
            .append(", ")
            .bind(spec.principal_id.as_uuid())
            .append(", ")
            .bind(proposal.contract.canonical_acl())
            .append(", ")
            .bind(proposal.contract.digest().as_str())
            .append(", ")
            .bind(i16::try_from(spec.approver_ids.len()).map_err(|_| PostgresPersistenceError::Invariant("tenant support approver count is not portable".into()))?)
            .append(", ")
            .bind(proposal.requested_by.as_uuid())
            .append(", ")
            .bind(proposal.authentication.id.as_str())
            .append(", ")
            .bind(proposal.authentication.digest.as_str())
            .append(", ")
            .bind(proposal.requested_at)
            .append(", ")
            .bind(spec.starts_at)
            .append(", ")
            .bind(spec.expires_at)
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating tenant support intent affected {rows} rows"
        )));
    }
    for approver_id in &spec.approver_ids {
        let rows = execute(
            transaction,
            sql_query::<()>("insert into tenant_support_grant_required_approvers (grant_id, approver_id) values (")
                .bind(proposal.id.as_uuid())
                .append(", ")
                .bind(approver_id.as_uuid())
                .append(")"),
        )
        .await?;
        if rows != 1 {
            return Err(PostgresPersistenceError::Invariant(format!(
                "creating tenant support required approver affected {rows} rows"
            )));
        }
    }
    Ok(())
}

async fn insert_approval(
    transaction: &a3s_orm::PostgresTransaction,
    approval: &TenantSupportGrantApproval,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("insert into tenant_support_grant_approvals (grant_id, approver_id, contract_digest, authentication_id, authentication_digest, policy_revision_id, policy_digest, binding_id, binding_version, approved_at, evidence_digest) values (")
            .bind(approval.grant_id.as_uuid())
            .append(", ")
            .bind(approval.approver_id.as_uuid())
            .append(", ")
            .bind(approval.contract_digest.as_str())
            .append(", ")
            .bind(approval.authentication.id.as_str())
            .append(", ")
            .bind(approval.authentication.digest.as_str())
            .append(", ")
            .bind(approval.policy_revision_id.as_uuid())
            .append(", ")
            .bind(approval.policy_digest.as_str())
            .append(", ")
            .bind(approval.binding_id.as_uuid())
            .append(", ")
            .bind(approval.binding_version)
            .append(", ")
            .bind(approval.approved_at)
            .append(", ")
            .bind(approval.digest.as_str())
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating tenant support approval affected {rows} rows"
        )));
    }
    Ok(())
}

async fn insert_grant(
    transaction: &a3s_orm::PostgresTransaction,
    grant: &TenantSupportGrant,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("insert into tenant_support_grants (id, aggregate_version, revocation_generation, accepted_at, revoked_at, revoked_by) values (")
            .bind(grant.id.as_uuid())
            .append(", ")
            .bind(grant.aggregate_version)
            .append(", ")
            .bind(grant.revocation_generation)
            .append(", ")
            .bind(grant.accepted_at)
            .append(", ")
            .bind(grant.revoked_at)
            .append(", ")
            .bind(grant.revoked_by.map(|id| id.as_uuid()))
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "accepting tenant support grant affected {rows} rows"
        )));
    }
    Ok(())
}

async fn store_proposal_facts(
    transaction: &a3s_orm::PostgresTransaction,
    proposal: &TenantSupportGrantProposal,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_outbox(
        transaction,
        &TenantSupportGrantProposed::envelope(proposal, request_id)?,
    )
    .await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            scope: proposal.contract.spec().scope.reference(),
            actor_id: Some(proposal.requested_by.as_uuid()),
            action: "identity.tenant-support-grant.proposed",
            aggregate_id: proposal.id.as_uuid(),
            occurred_at: proposal.requested_at,
            request_id,
            details: serde_json::json!({
                "principalId": proposal.contract.spec().principal_id,
                "contractDigest": proposal.contract.digest(),
                "requiredApproverIds": proposal.contract.spec().approver_ids,
                "authentication": proposal.authentication,
            }),
        },
    )
    .await
}

async fn store_approval_facts(
    transaction: &a3s_orm::PostgresTransaction,
    proposal: &TenantSupportGrantProposal,
    approval: &TenantSupportGrantApproval,
    ordinal: u64,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    store_outbox(
        transaction,
        &TenantSupportGrantApproved::envelope(proposal, approval, ordinal, request_id)?,
    )
    .await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            scope: proposal.contract.spec().scope.reference(),
            actor_id: Some(approval.approver_id.as_uuid()),
            action: "identity.tenant-support-grant.approved",
            aggregate_id: proposal.id.as_uuid(),
            occurred_at: approval.approved_at,
            request_id,
            details: serde_json::json!({
                "contractDigest": approval.contract_digest,
                "policyRevisionId": approval.policy_revision_id,
                "bindingId": approval.binding_id,
                "bindingVersion": approval.binding_version,
                "approvalEvidenceDigest": approval.digest,
                "authentication": approval.authentication,
                "approvalOrdinal": ordinal,
            }),
        },
    )
    .await
}

async fn store_grant_facts(
    transaction: &a3s_orm::PostgresTransaction,
    grant: &TenantSupportGrant,
    action: &'static str,
    actor_id: PrincipalId,
    authentication: Option<&DecisionEvidenceRef>,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    let event = match action {
        "identity.tenant-support-grant.accepted" => {
            TenantSupportGrantChanged::accepted(grant, request_id)?
        }
        "identity.tenant-support-grant.revoked" => {
            TenantSupportGrantChanged::revoked(grant, request_id)?
        }
        _ => {
            return Err(PostgresPersistenceError::Invariant(
                "tenant support grant audit action is unsupported".into(),
            ))
        }
    };
    store_outbox(transaction, &event).await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            scope: grant.contract.spec().scope.reference(),
            actor_id: Some(actor_id.as_uuid()),
            action,
            aggregate_id: grant.id.as_uuid(),
            occurred_at: grant.revoked_at.unwrap_or(grant.accepted_at),
            request_id,
            details: serde_json::json!({
                "principalId": grant.contract.spec().principal_id,
                "contractDigest": grant.contract.digest(),
                "aggregateVersion": grant.aggregate_version,
                "revocationGeneration": grant.revocation_generation,
                "revokedBy": grant.revoked_by,
                "authentication": authentication,
            }),
        },
    )
    .await
}

fn validate_replayed_proposal(
    replayed: IdempotentWrite<TenantSupportGrantProposal>,
    installation_id: InstallationId,
) -> Result<IdempotentWrite<TenantSupportGrantProposal>, PostgresPersistenceError> {
    replayed
        .value
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    if replayed.value.contract.spec().installation_id() != installation_id {
        return Err(PostgresPersistenceError::Invariant(
            "idempotent tenant support proposal crossed Installation scope".into(),
        ));
    }
    Ok(replayed)
}

fn validate_replayed_approval(
    replayed: IdempotentWrite<TenantSupportGrantApprovalOutcome>,
    installation_id: InstallationId,
) -> Result<IdempotentWrite<TenantSupportGrantApprovalOutcome>, PostgresPersistenceError> {
    replayed
        .value
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    if replayed.value.proposal.contract.spec().installation_id() != installation_id {
        return Err(PostgresPersistenceError::Invariant(
            "idempotent tenant support approval crossed Installation scope".into(),
        ));
    }
    Ok(replayed)
}

fn validate_replayed_grant(
    replayed: IdempotentWrite<TenantSupportGrant>,
    installation_id: InstallationId,
) -> Result<IdempotentWrite<TenantSupportGrant>, PostgresPersistenceError> {
    replayed
        .value
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    if replayed.value.contract.spec().installation_id() != installation_id {
        return Err(PostgresPersistenceError::Invariant(
            "idempotent tenant support grant crossed Installation scope".into(),
        ));
    }
    Ok(replayed)
}

#[async_trait]
impl ITenantSupportGrantRepository for PostgresIdentityRepository {
    async fn propose_tenant_support_grant(
        &self,
        write: ProposeTenantSupportGrantWrite,
    ) -> Result<IdempotentWrite<TenantSupportGrantProposal>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    write
                        .proposal
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let installation_id = write.proposal.contract.spec().installation_id();
                    if write.actor_principal_id != write.proposal.requested_by {
                        return Err(PostgresPersistenceError::Invariant(
                            "tenant support proposal actor does not match its requester".into(),
                        ));
                    }
                    lock_installation(transaction, installation_id).await?;
                    let policy = load_current_policy_for_update(transaction, installation_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    let actor = load_active_actor_binding(
                        transaction,
                        installation_id,
                        write.actor_principal_id,
                    )
                    .await?;
                    require_permission(&policy, &actor, PlatformPermission::TenantSupportManage)?;
                    if let Some(replayed) = idempotency_replay::<TenantSupportGrantProposal>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return validate_replayed_proposal(replayed, installation_id);
                    }
                    require_active_human(
                        transaction,
                        write.proposal.contract.spec().principal_id,
                        "subject",
                    )
                    .await?;
                    for approver_id in &write.proposal.contract.spec().approver_ids {
                        require_current_support_manager(
                            transaction,
                            installation_id,
                            *approver_id,
                            &policy,
                        )
                        .await?;
                    }
                    match insert_proposal(transaction, &write.proposal).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "tenant support grant intent already exists".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_proposal_facts(transaction, &write.proposal, write.request_id).await?;
                    store_idempotency(transaction, &write.idempotency, &write.proposal).await?;
                    Ok(IdempotentWrite {
                        value: write.proposal,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn approve_tenant_support_grant(
        &self,
        write: ApproveTenantSupportGrantWrite,
    ) -> Result<IdempotentWrite<TenantSupportGrantApprovalOutcome>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    write
                        .authentication
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    Sha256Digest::parse(write.expected_contract_digest.as_str())
                        .map_err(PostgresPersistenceError::Invariant)?;
                    lock_installation(transaction, write.installation_id).await?;
                    let policy = load_current_policy_for_update(transaction, write.installation_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    let actor = require_current_support_manager(
                        transaction,
                        write.installation_id,
                        write.actor_principal_id,
                        &policy,
                    )
                    .await?;
                    if let Some(replayed) = idempotency_replay::<TenantSupportGrantApprovalOutcome>(
                        transaction,
                        &write.idempotency,
                    )
                    .await?
                    {
                        return validate_replayed_approval(replayed, write.installation_id);
                    }
                    let proposal = load_proposal_for_update(
                        transaction,
                        write.installation_id,
                        write.grant_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if proposal.contract.digest() != &write.expected_contract_digest {
                        return Err(RepositoryError::Conflict(
                            "tenant support grant intent digest changed before approval".into(),
                        )
                        .into());
                    }
                    if !proposal
                        .contract
                        .spec()
                        .approver_ids
                        .contains(&write.actor_principal_id)
                    {
                        return Err(RepositoryError::Forbidden(
                            "actor is not a required approver for this tenant support grant".into(),
                        )
                        .into());
                    }
                    if write.approved_at >= proposal.contract.spec().expires_at {
                        return Err(RepositoryError::Conflict(
                            "tenant support grant intent expired before approval".into(),
                        )
                        .into());
                    }
                    let approval = TenantSupportGrantApproval::record(
                        &proposal,
                        write.actor_principal_id,
                        write.authentication,
                        &policy,
                        &actor,
                        write.approved_at,
                    )
                    .map_err(RepositoryError::Forbidden)?;
                    match insert_approval(transaction, &approval).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "tenant support approver already approved this intent".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    let approvals = load_approvals(transaction, &proposal).await?;
                    let approval_ordinal = u64::try_from(approvals.len()).map_err(|_| {
                        PostgresPersistenceError::Invariant(
                            "tenant support approval count is not portable".into(),
                        )
                    })?;
                    store_approval_facts(
                        transaction,
                        &proposal,
                        &approval,
                        approval_ordinal,
                        write.request_id,
                    )
                    .await?;

                    let grant = if approvals.len() == proposal.contract.spec().approver_ids.len() {
                        require_active_human(
                            transaction,
                            proposal.contract.spec().principal_id,
                            "subject",
                        )
                        .await?;
                        for approver_id in &proposal.contract.spec().approver_ids {
                            require_current_support_manager(
                                transaction,
                                write.installation_id,
                                *approver_id,
                                &policy,
                            )
                            .await?;
                        }
                        let accepted_at = approvals
                            .iter()
                            .map(|recorded| recorded.approved_at)
                            .max()
                            .ok_or_else(|| {
                                PostgresPersistenceError::Invariant(
                                    "tenant support grant acceptance has no approval time".into(),
                                )
                            })?;
                        let grant =
                            TenantSupportGrant::accept(proposal.contract.clone(), accepted_at)
                                .map_err(PostgresPersistenceError::Invariant)?;
                        match insert_grant(transaction, &grant).await {
                            Ok(()) => {}
                            Err(error) if is_unique_violation(&error) => {
                                return Err(RepositoryError::Conflict(
                                    "tenant support grant was already accepted".into(),
                                )
                                .into())
                            }
                            Err(error) => return Err(error),
                        }
                        store_grant_facts(
                            transaction,
                            &grant,
                            "identity.tenant-support-grant.accepted",
                            write.actor_principal_id,
                            Some(&approval.authentication),
                            write.request_id,
                        )
                        .await?;
                        Some(grant)
                    } else {
                        None
                    };
                    let outcome = TenantSupportGrantApprovalOutcome {
                        proposal,
                        approval,
                        grant,
                    };
                    outcome
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    store_idempotency(transaction, &write.idempotency, &outcome).await?;
                    Ok(IdempotentWrite {
                        value: outcome,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revoke_tenant_support_grant(
        &self,
        write: RevokeTenantSupportGrantWrite,
    ) -> Result<IdempotentWrite<TenantSupportGrant>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    write
                        .authentication
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    lock_installation(transaction, write.installation_id).await?;
                    let policy = load_current_policy_for_update(transaction, write.installation_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    let actor = load_active_actor_binding(
                        transaction,
                        write.installation_id,
                        write.actor_principal_id,
                    )
                    .await?;
                    require_permission(&policy, &actor, PlatformPermission::TenantSupportManage)?;
                    if let Some(replayed) =
                        idempotency_replay::<TenantSupportGrant>(transaction, &write.idempotency)
                            .await?
                    {
                        return validate_replayed_grant(replayed, write.installation_id);
                    }
                    let mut grant =
                        load_grant_for_update(transaction, write.installation_id, write.grant_id)
                            .await?
                            .ok_or(RepositoryError::NotFound)?;
                    if grant.revoked_at.is_some()
                        || grant.aggregate_version != write.expected_version
                    {
                        return Err(RepositoryError::Conflict(
                            "tenant support grant changed before revocation".into(),
                        )
                        .into());
                    }
                    grant
                        .revoke(write.actor_principal_id, write.revoked_at)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let rows = execute(
                        transaction,
                        sql_query::<()>("update tenant_support_grants set aggregate_version = ")
                            .bind(grant.aggregate_version)
                            .append(", revocation_generation = ")
                            .bind(grant.revocation_generation)
                            .append(", revoked_at = ")
                            .bind(grant.revoked_at)
                            .append(", revoked_by = ")
                            .bind(grant.revoked_by.map(|id| id.as_uuid()))
                            .append(" where id = ")
                            .bind(grant.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and revoked_at is null"),
                    )
                    .await?;
                    if rows != 1 {
                        return Err(RepositoryError::Conflict(
                            "tenant support grant changed before revocation".into(),
                        )
                        .into());
                    }
                    store_grant_facts(
                        transaction,
                        &grant,
                        "identity.tenant-support-grant.revoked",
                        write.actor_principal_id,
                        Some(&write.authentication),
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &grant).await?;
                    Ok(IdempotentWrite {
                        value: grant,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_tenant_support_grant_proposal(
        &self,
        installation_id: InstallationId,
        grant_id: TenantSupportGrantId,
    ) -> Result<Option<TenantSupportGrantProposal>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<TenantSupportProposalRow>(SELECT_TENANT_SUPPORT_PROPOSAL)
                    .append(" where intent.installation_id = ")
                    .bind(installation_id.as_uuid())
                    .append(" and intent.id = ")
                    .bind(grant_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_tenant_support_proposal)
            .transpose()
    }

    async fn list_tenant_support_grant_approvals(
        &self,
        installation_id: InstallationId,
        grant_id: TenantSupportGrantId,
    ) -> Result<Vec<TenantSupportGrantApproval>, RepositoryError> {
        let proposal = self
            .find_tenant_support_grant_proposal(installation_id, grant_id)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<TenantSupportApprovalRow>(SELECT_TENANT_SUPPORT_APPROVAL)
                    .append(" where approval.grant_id = ")
                    .bind(grant_id.as_uuid())
                    .append(" order by approval.approved_at asc, approval.approver_id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(|row| decode_tenant_support_approval(row, &proposal))
            .collect()
    }

    async fn find_tenant_support_grant(
        &self,
        installation_id: InstallationId,
        grant_id: TenantSupportGrantId,
    ) -> Result<Option<TenantSupportGrant>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<TenantSupportGrantRow>(SELECT_TENANT_SUPPORT_GRANT)
                    .append(" where intent.installation_id = ")
                    .bind(installation_id.as_uuid())
                    .append(" and accepted_grant.id = ")
                    .bind(grant_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_tenant_support_grant)
            .transpose()
    }
}
