use super::postgres::{decode_column, decode_principal, PostgresIdentityRepository, PrincipalRow};
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, IdentityPrincipal, PlatformRoleBinding,
};
use crate::modules::identity::domain::events::{
    PlatformRoleBindingChanged, PlatformRolePolicyAccepted,
};
use crate::modules::identity::domain::repositories::{
    AcceptPlatformRolePolicyRevisionWrite, BootstrapPlatformRbacWrite,
    ChangePlatformRoleBindingWrite, CreatePlatformRoleBindingWrite, IPlatformRbacRepository,
    PlatformRbacBootstrap, RevokePlatformRoleBindingWrite,
};
use crate::modules::identity::domain::value_objects::{PlatformPermission, PlatformRole};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, InstallationId, PlatformRoleBindingId, PlatformRolePolicyId,
    PlatformRolePolicyRevisionId, PrincipalId, RepositoryError,
};
use a3s_cloud_contracts::CloudScopeRef;
use a3s_orm::{sql_query, Database, DecodeError, FromRow, PostgresDialect, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_PLATFORM_ROLE_POLICY_REVISION: &str = "select revision.installation_id, revision.policy_id, revision.id, revision.revision_number, revision.canonical_acl, revision.digest, revision.accepted_by, revision.accepted_at from platform_role_policy_revisions revision";
const SELECT_PLATFORM_ROLE_BINDING: &str = "select binding.id, binding.installation_id, binding.principal_id, binding.role, binding.aggregate_version, binding.created_by, binding.updated_by, binding.created_at, binding.updated_at, binding.revoked_at from platform_role_bindings binding";

struct PlatformRolePolicyRevisionRow {
    installation_id: Uuid,
    policy_id: Uuid,
    id: Uuid,
    revision_number: u64,
    canonical_acl: String,
    digest: String,
    accepted_by: Uuid,
    accepted_at: DateTime<Utc>,
}

impl FromRow for PlatformRolePolicyRevisionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            installation_id: decode_column(row, 0)?,
            policy_id: decode_column(row, 1)?,
            id: decode_column(row, 2)?,
            revision_number: decode_column(row, 3)?,
            canonical_acl: decode_column(row, 4)?,
            digest: decode_column(row, 5)?,
            accepted_by: decode_column(row, 6)?,
            accepted_at: decode_column(row, 7)?,
        })
    }
}

fn decode_platform_role_policy_revision(
    row: PlatformRolePolicyRevisionRow,
) -> Result<AcceptedPlatformRolePolicyRevision, RepositoryError> {
    AcceptedPlatformRolePolicyRevision::restore(
        InstallationId::from_uuid(row.installation_id),
        PlatformRolePolicyId::from_uuid(row.policy_id),
        PlatformRolePolicyRevisionId::from_uuid(row.id),
        row.revision_number,
        &row.canonical_acl,
        &row.digest,
        PrincipalId::from_uuid(row.accepted_by),
        row.accepted_at,
    )
    .map_err(|error| {
        RepositoryError::Storage(format!(
            "stored platform role policy revision is invalid: {error}"
        ))
    })
}

struct PlatformRoleBindingRow {
    id: Uuid,
    installation_id: Uuid,
    principal_id: Uuid,
    role: String,
    aggregate_version: u64,
    created_by: Uuid,
    updated_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for PlatformRoleBindingRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode_column(row, 0)?,
            installation_id: decode_column(row, 1)?,
            principal_id: decode_column(row, 2)?,
            role: decode_column(row, 3)?,
            aggregate_version: decode_column(row, 4)?,
            created_by: decode_column(row, 5)?,
            updated_by: decode_column(row, 6)?,
            created_at: decode_column(row, 7)?,
            updated_at: decode_column(row, 8)?,
            revoked_at: decode_column(row, 9)?,
        })
    }
}

fn decode_platform_role_binding(
    row: PlatformRoleBindingRow,
) -> Result<PlatformRoleBinding, RepositoryError> {
    PlatformRoleBinding::restore(
        PlatformRoleBindingId::from_uuid(row.id),
        InstallationId::from_uuid(row.installation_id),
        PrincipalId::from_uuid(row.principal_id),
        PlatformRole::parse(&row.role).map_err(|error| {
            RepositoryError::Storage(format!("stored platform role is invalid: {error}"))
        })?,
        row.aggregate_version,
        PrincipalId::from_uuid(row.created_by),
        PrincipalId::from_uuid(row.updated_by),
        row.created_at,
        row.updated_at,
        row.revoked_at,
    )
    .map_err(|error| {
        RepositoryError::Storage(format!("stored platform role binding is invalid: {error}"))
    })
}

pub(super) async fn lock_installation(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
) -> Result<(), PostgresPersistenceError> {
    let locked = fetch_optional::<Uuid, _>(
        transaction,
        sql_query::<Uuid>(
            "select installation.id from cloud_installations installation where installation.singleton_key and installation.id = ",
        )
        .bind(installation_id.as_uuid())
        .append(" for update of installation"),
    )
    .await?;
    if locked.is_none() {
        return Err(RepositoryError::NotFound.into());
    }
    Ok(())
}

pub(super) async fn load_active_principal_for_share(
    transaction: &a3s_orm::PostgresTransaction,
    principal_id: PrincipalId,
) -> Result<Option<IdentityPrincipal>, PostgresPersistenceError> {
    fetch_optional::<PrincipalRow, _>(
        transaction,
        sql_query::<PrincipalRow>(
            "select id, kind, name, aggregate_version, created_at, disabled_at from identity_principals principal where principal.id = ",
        )
        .bind(principal_id.as_uuid())
        .append(" and principal.disabled_at is null for key share of principal"),
    )
    .await?
    .map(decode_principal)
    .transpose()
    .map_err(Into::into)
}

async fn require_active_principal(
    transaction: &a3s_orm::PostgresTransaction,
    principal_id: PrincipalId,
) -> Result<(), PostgresPersistenceError> {
    if load_active_principal_for_share(transaction, principal_id)
        .await?
        .is_none()
    {
        return Err(RepositoryError::Forbidden(
            "platform RBAC requires an active identity Principal".into(),
        )
        .into());
    }
    Ok(())
}

pub(super) async fn load_current_policy_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
) -> Result<Option<AcceptedPlatformRolePolicyRevision>, PostgresPersistenceError> {
    fetch_optional::<PlatformRolePolicyRevisionRow, _>(
        transaction,
        sql_query::<PlatformRolePolicyRevisionRow>(SELECT_PLATFORM_ROLE_POLICY_REVISION)
            .append(" join platform_role_policy_heads head on head.installation_id = revision.installation_id and head.policy_id = revision.policy_id and head.revision_id = revision.id and head.revision_number = revision.revision_number where head.installation_id = ")
            .bind(installation_id.as_uuid())
            .append(" for update of head"),
    )
    .await?
    .map(decode_platform_role_policy_revision)
    .transpose()
    .map_err(Into::into)
}

async fn load_binding_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
    binding_id: PlatformRoleBindingId,
) -> Result<Option<PlatformRoleBinding>, PostgresPersistenceError> {
    fetch_optional::<PlatformRoleBindingRow, _>(
        transaction,
        sql_query::<PlatformRoleBindingRow>(SELECT_PLATFORM_ROLE_BINDING)
            .append(" where binding.installation_id = ")
            .bind(installation_id.as_uuid())
            .append(" and binding.id = ")
            .bind(binding_id.as_uuid())
            .append(" for update of binding"),
    )
    .await?
    .map(decode_platform_role_binding)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_active_actor_binding(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
    principal_id: PrincipalId,
) -> Result<PlatformRoleBinding, PostgresPersistenceError> {
    fetch_optional::<PlatformRoleBindingRow, _>(
        transaction,
        sql_query::<PlatformRoleBindingRow>(SELECT_PLATFORM_ROLE_BINDING)
            .append(" join identity_principals principal on principal.id = binding.principal_id and principal.disabled_at is null where binding.installation_id = ")
            .bind(installation_id.as_uuid())
            .append(" and binding.principal_id = ")
            .bind(principal_id.as_uuid())
            .append(" and binding.revoked_at is null for update of binding for key share of principal"),
    )
    .await?
    .map(decode_platform_role_binding)
    .transpose()?
    .ok_or_else(|| {
        RepositoryError::Forbidden(
            "actor has no active platform role binding in this Installation".into(),
        )
        .into()
    })
}

pub(super) fn require_permission(
    policy: &AcceptedPlatformRolePolicyRevision,
    actor: &PlatformRoleBinding,
    permission: PlatformPermission,
) -> Result<(), PostgresPersistenceError> {
    actor
        .validate_against_policy(policy)
        .map_err(PostgresPersistenceError::Invariant)?;
    if !actor.is_active() || !policy.admits(actor.role, permission) {
        return Err(RepositoryError::Forbidden(format!(
            "active platform role does not admit {}",
            permission.as_str()
        ))
        .into());
    }
    Ok(())
}

fn validate_replayed_policy(
    replayed: IdempotentWrite<AcceptedPlatformRolePolicyRevision>,
    installation_id: InstallationId,
) -> Result<IdempotentWrite<AcceptedPlatformRolePolicyRevision>, PostgresPersistenceError> {
    replayed
        .value
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    if replayed.value.installation_id != installation_id {
        return Err(PostgresPersistenceError::Invariant(
            "idempotent platform role policy response crossed Installation scope".into(),
        ));
    }
    Ok(replayed)
}

fn validate_replayed_binding(
    replayed: IdempotentWrite<PlatformRoleBinding>,
    installation_id: InstallationId,
) -> Result<IdempotentWrite<PlatformRoleBinding>, PostgresPersistenceError> {
    replayed
        .value
        .validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    if replayed.value.installation_id != installation_id {
        return Err(PostgresPersistenceError::Invariant(
            "idempotent platform role binding response crossed Installation scope".into(),
        ));
    }
    Ok(replayed)
}

async fn insert_policy_revision(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedPlatformRolePolicyRevision,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("insert into platform_role_policy_revisions (installation_id, policy_id, id, revision_number, canonical_acl, digest, accepted_by, accepted_at) values (")
            .bind(revision.installation_id.as_uuid())
            .append(", ")
            .bind(revision.policy_id.as_uuid())
            .append(", ")
            .bind(revision.id.as_uuid())
            .append(", ")
            .bind(revision.revision_number)
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
            "creating platform role policy revision affected {rows} rows"
        )));
    }
    Ok(())
}

async fn insert_policy_head(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedPlatformRolePolicyRevision,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("insert into platform_role_policy_heads (installation_id, policy_id, revision_id, revision_number, updated_at) values (")
            .bind(revision.installation_id.as_uuid())
            .append(", ")
            .bind(revision.policy_id.as_uuid())
            .append(", ")
            .bind(revision.id.as_uuid())
            .append(", ")
            .bind(revision.revision_number)
            .append(", ")
            .bind(revision.accepted_at)
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating platform role policy head affected {rows} rows"
        )));
    }
    Ok(())
}

async fn insert_binding(
    transaction: &a3s_orm::PostgresTransaction,
    binding: &PlatformRoleBinding,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("insert into platform_role_bindings (id, installation_id, principal_id, role, aggregate_version, created_by, updated_by, created_at, updated_at, revoked_at) values (")
            .bind(binding.id.as_uuid())
            .append(", ")
            .bind(binding.installation_id.as_uuid())
            .append(", ")
            .bind(binding.principal_id.as_uuid())
            .append(", ")
            .bind(binding.role.as_str())
            .append(", ")
            .bind(binding.aggregate_version)
            .append(", ")
            .bind(binding.created_by.as_uuid())
            .append(", ")
            .bind(binding.updated_by.as_uuid())
            .append(", ")
            .bind(binding.created_at)
            .append(", ")
            .bind(binding.updated_at)
            .append(", ")
            .bind(binding.revoked_at)
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating platform role binding affected {rows} rows"
        )));
    }
    Ok(())
}

async fn store_policy_facts(
    transaction: &a3s_orm::PostgresTransaction,
    revision: &AcceptedPlatformRolePolicyRevision,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    let event = PlatformRolePolicyAccepted::envelope(revision, request_id)?;
    store_outbox(transaction, &event).await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            scope: CloudScopeRef::Installation {
                installation_id: revision.installation_id.as_uuid(),
            },
            actor_id: Some(revision.accepted_by.as_uuid()),
            action: "identity.platform-role-policy.accepted",
            aggregate_id: revision.policy_id.as_uuid(),
            occurred_at: revision.accepted_at,
            request_id,
            details: serde_json::json!({
                "revisionId": revision.id,
                "revisionNumber": revision.revision_number,
                "digest": revision.contract.digest(),
            }),
        },
    )
    .await
}

async fn store_binding_facts(
    transaction: &a3s_orm::PostgresTransaction,
    binding: &PlatformRoleBinding,
    previous_role: Option<PlatformRole>,
    action: &'static str,
    request_id: Uuid,
) -> Result<(), PostgresPersistenceError> {
    let event = match action {
        "identity.platform-role-binding.created" => {
            PlatformRoleBindingChanged::created(binding, request_id)?
        }
        "identity.platform-role-binding.role-changed" => PlatformRoleBindingChanged::role_changed(
            binding,
            previous_role.ok_or_else(|| {
                PostgresPersistenceError::Invariant("role change fact has no previous role".into())
            })?,
            request_id,
        )?,
        "identity.platform-role-binding.revoked" => {
            PlatformRoleBindingChanged::revoked(binding, request_id)?
        }
        _ => {
            return Err(PostgresPersistenceError::Invariant(
                "platform role binding audit action is unsupported".into(),
            ))
        }
    };
    store_outbox(transaction, &event).await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            scope: CloudScopeRef::Installation {
                installation_id: binding.installation_id.as_uuid(),
            },
            actor_id: Some(binding.updated_by.as_uuid()),
            action,
            aggregate_id: binding.id.as_uuid(),
            occurred_at: binding.updated_at,
            request_id,
            details: serde_json::json!({
                "principalId": binding.principal_id,
                "role": binding.role,
                "previousRole": previous_role,
                "aggregateVersion": binding.aggregate_version,
                "revokedAt": binding.revoked_at,
            }),
        },
    )
    .await
}

fn can_assign_owner(actor: &PlatformRoleBinding) -> bool {
    actor.is_active() && actor.role == PlatformRole::PlatformOwner
}

fn permissions_are_subset(
    candidate: &[PlatformPermission],
    existing: &[PlatformPermission],
) -> bool {
    candidate
        .iter()
        .all(|permission| existing.binary_search(permission).is_ok())
}

async fn active_owner_count(
    transaction: &a3s_orm::PostgresTransaction,
    installation_id: InstallationId,
) -> Result<i64, PostgresPersistenceError> {
    Ok(fetch_optional::<i64, _>(
        transaction,
        sql_query::<i64>("select count(*) from platform_role_bindings binding join identity_principals principal on principal.id = binding.principal_id and principal.disabled_at is null where binding.installation_id = ")
            .bind(installation_id.as_uuid())
            .append(" and binding.role = 'platform_owner' and binding.revoked_at is null"),
    )
    .await?
    .unwrap_or_default())
}

#[async_trait]
impl IPlatformRbacRepository for PostgresIdentityRepository {
    async fn bootstrap_platform_rbac(
        &self,
        write: BootstrapPlatformRbacWrite,
    ) -> Result<IdempotentWrite<PlatformRbacBootstrap>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    write
                        .bootstrap
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let installation_id = write.bootstrap.policy.installation_id;
                    if write.actor_principal_id != write.bootstrap.policy.accepted_by {
                        return Err(PostgresPersistenceError::Invariant(
                            "platform RBAC bootstrap actor does not own the initial authority"
                                .into(),
                        ));
                    }
                    lock_installation(transaction, installation_id).await?;
                    require_active_principal(transaction, write.actor_principal_id).await?;

                    let current =
                        load_current_policy_for_update(transaction, installation_id).await?;
                    if let Some(current) = &current {
                        let actor = load_active_actor_binding(
                            transaction,
                            installation_id,
                            write.actor_principal_id,
                        )
                        .await?;
                        require_permission(current, &actor, PlatformPermission::RolePolicyManage)?;
                        require_permission(current, &actor, PlatformPermission::RoleBindingManage)?;
                    }
                    if let Some(replayed) =
                        idempotency_replay::<PlatformRbacBootstrap>(transaction, &write.idempotency)
                            .await?
                    {
                        replayed
                            .value
                            .validate()
                            .map_err(PostgresPersistenceError::Invariant)?;
                        if replayed.value.policy.installation_id != installation_id {
                            return Err(PostgresPersistenceError::Invariant(
                                "idempotent platform RBAC bootstrap crossed Installation scope"
                                    .into(),
                            ));
                        }
                        return Ok(replayed);
                    }
                    if current.is_some() {
                        return Err(RepositoryError::Conflict(
                            "platform RBAC has already been bootstrapped".into(),
                        )
                        .into());
                    }
                    let any_binding = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from platform_role_bindings where installation_id = ",
                        )
                        .bind(installation_id.as_uuid())
                        .append(" limit 1"),
                    )
                    .await?
                    .is_some();
                    if any_binding {
                        return Err(PostgresPersistenceError::Invariant(
                            "platform RBAC bindings exist without a current policy head".into(),
                        ));
                    }

                    if let Err(error) =
                        insert_policy_revision(transaction, &write.bootstrap.policy).await
                    {
                        if is_unique_violation(&error) {
                            return Err(RepositoryError::Conflict(
                                "initial platform role policy already exists".into(),
                            )
                            .into());
                        }
                        return Err(error);
                    }
                    insert_policy_head(transaction, &write.bootstrap.policy).await?;
                    if let Err(error) =
                        insert_binding(transaction, &write.bootstrap.owner_binding).await
                    {
                        if is_unique_violation(&error) {
                            return Err(RepositoryError::Conflict(
                                "initial platform owner binding already exists".into(),
                            )
                            .into());
                        }
                        return Err(error);
                    }
                    store_policy_facts(transaction, &write.bootstrap.policy, write.request_id)
                        .await?;
                    store_binding_facts(
                        transaction,
                        &write.bootstrap.owner_binding,
                        None,
                        "identity.platform-role-binding.created",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &write.bootstrap).await?;
                    Ok(IdempotentWrite {
                        value: write.bootstrap,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn accept_platform_role_policy_revision(
        &self,
        write: AcceptPlatformRolePolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedPlatformRolePolicyRevision>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    write
                        .revision
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let installation_id = write.revision.installation_id;
                    lock_installation(transaction, installation_id).await?;
                    let current = load_current_policy_for_update(transaction, installation_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    let actor = load_active_actor_binding(
                        transaction,
                        installation_id,
                        write.actor_principal_id,
                    )
                    .await?;
                    require_permission(&current, &actor, PlatformPermission::RolePolicyManage)?;
                    if let Some(replayed) =
                        idempotency_replay::<AcceptedPlatformRolePolicyRevision>(
                            transaction,
                            &write.idempotency,
                        )
                        .await?
                    {
                        return validate_replayed_policy(replayed, installation_id);
                    }
                    if current.id != write.expected_current_revision_id {
                        return Err(RepositoryError::Conflict(
                            "platform role policy head changed before acceptance".into(),
                        )
                        .into());
                    }
                    let next_revision =
                        current.revision_number.checked_add(1).ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "platform role policy revision is exhausted".into(),
                            )
                        })?;
                    if write.revision.policy_id != current.policy_id
                        || write.revision.revision_number != next_revision
                        || write.revision.accepted_by != write.actor_principal_id
                        || write.revision.accepted_at < current.accepted_at
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "new platform role policy is not the exact accepted successor".into(),
                        ));
                    }
                    if let Err(error) = insert_policy_revision(transaction, &write.revision).await {
                        if is_unique_violation(&error) {
                            return Err(RepositoryError::Conflict(
                                "platform role policy revision already exists".into(),
                            )
                            .into());
                        }
                        return Err(error);
                    }
                    let rows = execute(
                        transaction,
                        sql_query::<()>("update platform_role_policy_heads set revision_id = ")
                            .bind(write.revision.id.as_uuid())
                            .append(", revision_number = ")
                            .bind(write.revision.revision_number)
                            .append(", updated_at = ")
                            .bind(write.revision.accepted_at)
                            .append(" where installation_id = ")
                            .bind(installation_id.as_uuid())
                            .append(" and policy_id = ")
                            .bind(current.policy_id.as_uuid())
                            .append(" and revision_id = ")
                            .bind(write.expected_current_revision_id.as_uuid())
                            .append(" and revision_number = ")
                            .bind(current.revision_number),
                    )
                    .await?;
                    if rows != 1 {
                        return Err(RepositoryError::Conflict(
                            "platform role policy head changed before acceptance".into(),
                        )
                        .into());
                    }
                    store_policy_facts(transaction, &write.revision, write.request_id).await?;
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

    async fn current_platform_role_policy(
        &self,
        installation_id: InstallationId,
    ) -> Result<Option<AcceptedPlatformRolePolicyRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<PlatformRolePolicyRevisionRow>(SELECT_PLATFORM_ROLE_POLICY_REVISION)
                    .append(" join platform_role_policy_heads head on head.installation_id = revision.installation_id and head.policy_id = revision.policy_id and head.revision_id = revision.id and head.revision_number = revision.revision_number where head.installation_id = ")
                    .bind(installation_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_platform_role_policy_revision)
            .transpose()
    }

    async fn find_platform_role_policy_revision(
        &self,
        installation_id: InstallationId,
        revision_id: PlatformRolePolicyRevisionId,
    ) -> Result<Option<AcceptedPlatformRolePolicyRevision>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<PlatformRolePolicyRevisionRow>(SELECT_PLATFORM_ROLE_POLICY_REVISION)
                    .append(" where revision.installation_id = ")
                    .bind(installation_id.as_uuid())
                    .append(" and revision.id = ")
                    .bind(revision_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_platform_role_policy_revision)
            .transpose()
    }

    async fn create_platform_role_binding(
        &self,
        write: CreatePlatformRoleBindingWrite,
    ) -> Result<IdempotentWrite<PlatformRoleBinding>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    write
                        .binding
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let installation_id = write.binding.installation_id;
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
                    require_permission(&policy, &actor, PlatformPermission::RoleBindingManage)?;
                    if let Some(replayed) =
                        idempotency_replay::<PlatformRoleBinding>(transaction, &write.idempotency)
                            .await?
                    {
                        return validate_replayed_binding(replayed, installation_id);
                    }
                    write
                        .binding
                        .validate_against_policy(&policy)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    if write.binding.created_by != write.actor_principal_id
                        || write.binding.updated_by != write.actor_principal_id
                        || !write.binding.is_active()
                        || write.binding.aggregate_version != 1
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "new platform role binding actor or lifecycle is invalid".into(),
                        ));
                    }
                    require_active_principal(transaction, write.binding.principal_id).await?;
                    if write.binding.role == PlatformRole::PlatformOwner
                        && !can_assign_owner(&actor)
                    {
                        return Err(RepositoryError::Forbidden(
                            "only a platform owner can create another platform owner".into(),
                        )
                        .into());
                    }
                    match insert_binding(transaction, &write.binding).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Principal already has an active platform role binding".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_binding_facts(
                        transaction,
                        &write.binding,
                        None,
                        "identity.platform-role-binding.created",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &write.binding).await?;
                    Ok(IdempotentWrite {
                        value: write.binding,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn change_platform_role_binding(
        &self,
        write: ChangePlatformRoleBindingWrite,
    ) -> Result<IdempotentWrite<PlatformRoleBinding>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
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
                    require_permission(&policy, &actor, PlatformPermission::RoleBindingManage)?;
                    if let Some(replayed) =
                        idempotency_replay::<PlatformRoleBinding>(transaction, &write.idempotency)
                            .await?
                    {
                        return validate_replayed_binding(replayed, write.installation_id);
                    }
                    let mut binding = load_binding_for_update(
                        transaction,
                        write.installation_id,
                        write.binding_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if !binding.is_active() || binding.aggregate_version != write.expected_version {
                        return Err(RepositoryError::Conflict(
                            "platform role binding changed before the role update".into(),
                        )
                        .into());
                    }
                    require_active_principal(transaction, binding.principal_id).await?;
                    if (binding.role == PlatformRole::PlatformOwner
                        || write.role == PlatformRole::PlatformOwner)
                        && !can_assign_owner(&actor)
                    {
                        return Err(RepositoryError::Forbidden(
                            "only a platform owner can change an owner binding".into(),
                        )
                        .into());
                    }
                    if binding.principal_id == write.actor_principal_id {
                        let existing = policy.contract.spec().permissions_for(binding.role);
                        let candidate = policy.contract.spec().permissions_for(write.role);
                        if !permissions_are_subset(candidate, existing) {
                            return Err(RepositoryError::Forbidden(
                                "a Principal cannot escalate its own platform permissions".into(),
                            )
                            .into());
                        }
                    }
                    if binding.role == PlatformRole::PlatformOwner
                        && write.role != PlatformRole::PlatformOwner
                        && active_owner_count(transaction, write.installation_id).await? <= 1
                    {
                        return Err(RepositoryError::Conflict(
                            "the last active platform owner cannot be demoted".into(),
                        )
                        .into());
                    }
                    let previous_role = binding.role;
                    if !binding
                        .change_role(
                            write.role,
                            &policy,
                            write.actor_principal_id,
                            write.changed_at,
                        )
                        .map_err(PostgresPersistenceError::Invariant)?
                    {
                        return Err(RepositoryError::Conflict(
                            "platform role binding already has the requested role".into(),
                        )
                        .into());
                    }
                    let rows = execute(
                        transaction,
                        sql_query::<()>("update platform_role_bindings set role = ")
                            .bind(binding.role.as_str())
                            .append(", aggregate_version = ")
                            .bind(binding.aggregate_version)
                            .append(", updated_by = ")
                            .bind(binding.updated_by.as_uuid())
                            .append(", updated_at = ")
                            .bind(binding.updated_at)
                            .append(" where installation_id = ")
                            .bind(write.installation_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.binding_id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and revoked_at is null"),
                    )
                    .await?;
                    if rows != 1 {
                        return Err(RepositoryError::Conflict(
                            "platform role binding changed before the role update".into(),
                        )
                        .into());
                    }
                    store_binding_facts(
                        transaction,
                        &binding,
                        Some(previous_role),
                        "identity.platform-role-binding.role-changed",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &binding).await?;
                    Ok(IdempotentWrite {
                        value: binding,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revoke_platform_role_binding(
        &self,
        write: RevokePlatformRoleBindingWrite,
    ) -> Result<IdempotentWrite<PlatformRoleBinding>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
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
                    require_permission(&policy, &actor, PlatformPermission::RoleBindingManage)?;
                    if let Some(replayed) =
                        idempotency_replay::<PlatformRoleBinding>(transaction, &write.idempotency)
                            .await?
                    {
                        return validate_replayed_binding(replayed, write.installation_id);
                    }
                    let mut binding = load_binding_for_update(
                        transaction,
                        write.installation_id,
                        write.binding_id,
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if !binding.is_active() || binding.aggregate_version != write.expected_version {
                        return Err(RepositoryError::Conflict(
                            "platform role binding changed before revocation".into(),
                        )
                        .into());
                    }
                    if binding.role == PlatformRole::PlatformOwner {
                        if !can_assign_owner(&actor) {
                            return Err(RepositoryError::Forbidden(
                                "only a platform owner can revoke an owner binding".into(),
                            )
                            .into());
                        }
                        if active_owner_count(transaction, write.installation_id).await? <= 1 {
                            return Err(RepositoryError::Conflict(
                                "the last active platform owner cannot be revoked".into(),
                            )
                            .into());
                        }
                    }
                    binding
                        .revoke(write.actor_principal_id, write.revoked_at)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let rows = execute(
                        transaction,
                        sql_query::<()>("update platform_role_bindings set aggregate_version = ")
                            .bind(binding.aggregate_version)
                            .append(", updated_by = ")
                            .bind(binding.updated_by.as_uuid())
                            .append(", updated_at = ")
                            .bind(binding.updated_at)
                            .append(", revoked_at = ")
                            .bind(binding.revoked_at)
                            .append(" where installation_id = ")
                            .bind(write.installation_id.as_uuid())
                            .append(" and id = ")
                            .bind(write.binding_id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and revoked_at is null"),
                    )
                    .await?;
                    if rows != 1 {
                        return Err(RepositoryError::Conflict(
                            "platform role binding changed before revocation".into(),
                        )
                        .into());
                    }
                    store_binding_facts(
                        transaction,
                        &binding,
                        None,
                        "identity.platform-role-binding.revoked",
                        write.request_id,
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &binding).await?;
                    Ok(IdempotentWrite {
                        value: binding,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_platform_role_binding(
        &self,
        installation_id: InstallationId,
        binding_id: PlatformRoleBindingId,
    ) -> Result<Option<PlatformRoleBinding>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<PlatformRoleBindingRow>(SELECT_PLATFORM_ROLE_BINDING)
                    .append(" where binding.installation_id = ")
                    .bind(installation_id.as_uuid())
                    .append(" and binding.id = ")
                    .bind(binding_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_platform_role_binding)
            .transpose()
    }

    async fn find_active_platform_role_binding_for_principal(
        &self,
        installation_id: InstallationId,
        principal_id: PrincipalId,
    ) -> Result<Option<PlatformRoleBinding>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<PlatformRoleBindingRow>(SELECT_PLATFORM_ROLE_BINDING)
                    .append(" join identity_principals principal on principal.id = binding.principal_id and principal.disabled_at is null where binding.installation_id = ")
                    .bind(installation_id.as_uuid())
                    .append(" and binding.principal_id = ")
                    .bind(principal_id.as_uuid())
                    .append(" and binding.revoked_at is null"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_platform_role_binding)
            .transpose()
    }
}
