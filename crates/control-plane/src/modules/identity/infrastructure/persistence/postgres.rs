use super::postgres_memberships::{load_active_membership_for_update, lock_membership_set};
use super::postgres_platform_rbac::{
    load_active_principal_for_authorization, lock_installation,
    lock_installation_for_authorization, persist_platform_rbac_bootstrap_under_installation_lock,
    platform_authorization_request,
};
use super::postgres_privileged_authorization_decisions::issue_privileged_authorization;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_unique_violation, store_audit,
    store_idempotency, store_outbox, transaction_error, AuditWrite, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{
    ApiToken, AuthenticatedApiToken, IdentityBootstrap, IdentityPrincipal, IdentityPrincipalKind,
    Membership, Organization,
};
use crate::modules::identity::domain::repositories::{
    BootstrapIdentityWrite, CreateApiTokenWrite, CreateOrganizationWrite, IApiTokenRepository,
    IIdentityBootstrapRepository, IOrganizationRepository, ReadOrganizationCatalog,
};
use crate::modules::identity::domain::services::MembershipAdministration;
use crate::modules::identity::domain::value_objects::{
    ApiTokenDigest, ApiTokenName, ApiTokenScope, MembershipRole, OrganizationName,
    PlatformPermission,
};
use crate::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, IdempotentWrite, InstallationId, MembershipId, OrganizationId,
    PrincipalId, RepositoryError, ResourceName,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresIdentityRepository {
    pub(super) executor: PostgresExecutor,
}

impl PostgresIdentityRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

const ORGANIZATION_CATALOG_READ_ACTION: &str = "identity.organization-catalog.read";
type OrganizationRow = (Uuid, String, u64, DateTime<Utc>);

fn decode_organization(row: OrganizationRow) -> Result<Organization, RepositoryError> {
    let (id, name, aggregate_version, created_at) = row;
    let name = OrganizationName::parse(name).map_err(|error| {
        RepositoryError::Storage(format!("stored organization name is invalid: {error}"))
    })?;
    Ok(Organization {
        id: OrganizationId::from_uuid(id),
        name,
        aggregate_version,
        created_at,
    })
}

#[async_trait]
impl IOrganizationRepository for PostgresIdentityRepository {
    async fn create(
        &self,
        write: CreateOrganizationWrite,
    ) -> Result<IdempotentWrite<Organization>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let CreateOrganizationWrite {
                        organization,
                        owner_membership,
                        events,
                        actor_principal_id,
                        request_id,
                        idempotency,
                    } = write;
                    if let Some(replayed) =
                        idempotency_replay::<Organization>(transaction, &idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    if owner_membership.organization_id != organization.id
                        || owner_membership.principal_id != actor_principal_id
                        || owner_membership.role != MembershipRole::Owner
                        || !owner_membership.is_active()
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "organization owner membership does not bind its creator".into(),
                        ));
                    }
                    let actor_exists = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from identity_principals where id = ",
                        )
                        .bind(actor_principal_id.as_uuid())
                        .append(" and disabled_at is null"),
                    )
                    .await?
                    .is_some();
                    if !actor_exists {
                        return Err(RepositoryError::Forbidden(
                            "organization creator is not an active identity principal".into(),
                        )
                        .into());
                    }
                    let inserted = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
                        )
                        .bind(organization.id.as_uuid())
                        .append(", ")
                        .bind(organization.name.as_str())
                        .append(", ")
                        .bind(organization.name.key())
                        .append(", ")
                        .bind(organization.aggregate_version)
                        .append(", ")
                        .bind(organization.created_at)
                        .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => {}
                        Ok(rows) => {
                            return Err(PostgresPersistenceError::Invariant(format!(
                                "creating organization affected {rows} rows"
                            )))
                        }
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "organization name is already in use".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    insert_membership(transaction, &owner_membership).await?;
                    for event in &events {
                        store_outbox(transaction, event).await?;
                    }
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            scope: AuditWrite::organization_scope(organization.id.as_uuid()),
                            actor_id: Some(actor_principal_id.as_uuid()),
                            action: "identity.organization.created",
                            aggregate_id: organization.id.as_uuid(),
                            occurred_at: organization.created_at,
                            request_id,
                            details: serde_json::json!({
                                "ownerMembershipId": owner_membership.id,
                                "ownerPrincipalId": owner_membership.principal_id,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &idempotency, &organization).await?;
                    Ok(IdempotentWrite {
                        value: organization,
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
    ) -> Result<Option<Organization>, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<(Uuid, String, u64, DateTime<Utc>)>(
                    "select id, name, aggregate_version, created_at from organizations where id = ",
                )
                .bind(organization_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        row.map(decode_organization).transpose()
    }

    async fn list_visible(
        &self,
        read: ReadOrganizationCatalog,
    ) -> Result<Vec<Organization>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_installation_for_authorization(transaction, read.installation_id).await?;
                    let authorization = issue_privileged_authorization(
                        transaction,
                        platform_authorization_request(
                            read.installation_id,
                            read.actor_principal_id,
                            read.credential_id,
                            PlatformPermission::TenantLifecycleRead,
                            ORGANIZATION_CATALOG_READ_ACTION,
                            read.installation_id.as_uuid(),
                            read.request_id,
                        )?,
                    )
                    .await;
                    let credential_organization_id = match authorization {
                        Ok(_) => None,
                        Err(PostgresPersistenceError::Repository(
                            RepositoryError::Forbidden(_),
                        )) => {
                            let decided_at = Utc::now();
                            let principal = load_active_principal_for_authorization(
                                transaction,
                                read.actor_principal_id,
                            )
                            .await?
                            .ok_or_else(|| {
                                RepositoryError::Forbidden(
                                    "organization catalog principal is not active".into(),
                                )
                            })?;
                            let credential = load_api_token_by_id_for_authorization(
                                transaction,
                                read.credential_id,
                            )
                            .await?
                            .filter(|credential| {
                                credential.principal_id == principal.id
                                    && credential.is_active_at(decided_at)
                                    && credential.grants_scope(ApiTokenScope::CLOUD_READ)
                            })
                            .ok_or_else(|| {
                                RepositoryError::Forbidden(
                                    "organization catalog credential is not active or lacks cloud:read"
                                        .into(),
                                )
                            })?;
                            Some(credential.organization_id)
                        }
                        Err(error) => return Err(error),
                    };
                    let rows = match credential_organization_id {
                        Some(organization_id) => {
                            fetch_all::<OrganizationRow, _>(
                                transaction,
                                sql_query::<OrganizationRow>(
                                    "select id, name, aggregate_version, created_at from organizations where id = ",
                                )
                                .bind(organization_id.as_uuid()),
                            )
                            .await?
                        }
                        None => {
                            fetch_all::<OrganizationRow, _>(
                                transaction,
                                sql_query::<OrganizationRow>(
                                    "select id, name, aggregate_version, created_at from organizations order by created_at asc, id asc",
                                ),
                            )
                            .await?
                        }
                    };
                    rows.into_iter()
                        .map(decode_organization)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(Into::into)
                })
            })
            .await
            .map_err(transaction_error)
    }
}

struct ApiTokenRow {
    id: Uuid,
    organization_id: Uuid,
    principal_id: Uuid,
    name: String,
    scopes: serde_json::Value,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for ApiTokenRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode_column(row, 0)?,
            organization_id: decode_column(row, 1)?,
            principal_id: decode_column(row, 2)?,
            name: decode_column(row, 3)?,
            scopes: decode_column(row, 4)?,
            aggregate_version: decode_column(row, 5)?,
            created_at: decode_column(row, 6)?,
            expires_at: decode_column(row, 7)?,
            revoked_at: decode_column(row, 8)?,
        })
    }
}

pub(super) fn decode_column<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn decode_token(row: ApiTokenRow) -> Result<ApiToken, RepositoryError> {
    let name = ApiTokenName::parse(row.name).map_err(|error| {
        RepositoryError::Storage(format!("stored API token name is invalid: {error}"))
    })?;
    let scopes = serde_json::from_value::<Vec<String>>(row.scopes)
        .map_err(|error| {
            RepositoryError::Storage(format!("stored API token scopes are invalid: {error}"))
        })?
        .into_iter()
        .map(ApiTokenScope::parse)
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| {
            RepositoryError::Storage(format!("stored API token scope is invalid: {error}"))
        })?;
    Ok(ApiToken {
        id: ApiTokenId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        principal_id: PrincipalId::from_uuid(row.principal_id),
        name,
        scopes,
        aggregate_version: row.aggregate_version,
        created_at: row.created_at,
        expires_at: row.expires_at,
        revoked_at: row.revoked_at,
    })
}

pub(super) async fn load_api_token_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    token_id: ApiTokenId,
) -> Result<Option<ApiToken>, PostgresPersistenceError> {
    fetch_optional::<ApiTokenRow, _>(
        transaction,
        sql_query::<ApiTokenRow>(
            "select id, organization_id, principal_id, name, scopes, aggregate_version, created_at, expires_at, revoked_at from api_tokens where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and id = ")
        .bind(token_id.as_uuid())
        .append(" for update"),
    )
    .await?
    .map(decode_token)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn load_api_token_by_id_for_authorization(
    transaction: &a3s_orm::PostgresTransaction,
    token_id: ApiTokenId,
) -> Result<Option<ApiToken>, PostgresPersistenceError> {
    fetch_optional::<ApiTokenRow, _>(
        transaction,
        sql_query::<ApiTokenRow>(
            "select id, organization_id, principal_id, name, scopes, aggregate_version, created_at, expires_at, revoked_at from api_tokens where id = ",
        )
        .bind(token_id.as_uuid())
        .append(" for share"),
    )
    .await?
    .map(decode_token)
    .transpose()
    .map_err(Into::into)
}

pub(super) type PrincipalRow = (
    Uuid,
    String,
    String,
    u64,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

pub(super) fn decode_principal(row: PrincipalRow) -> Result<IdentityPrincipal, RepositoryError> {
    let (id, kind, name, aggregate_version, created_at, disabled_at) = row;
    Ok(IdentityPrincipal {
        id: PrincipalId::from_uuid(id),
        kind: IdentityPrincipalKind::parse(&kind).map_err(|error| {
            RepositoryError::Storage(format!(
                "stored identity principal kind is invalid: {error}"
            ))
        })?,
        name: ResourceName::parse(name).map_err(|error| {
            RepositoryError::Storage(format!(
                "stored identity principal name is invalid: {error}"
            ))
        })?,
        aggregate_version,
        created_at,
        disabled_at,
    })
}

pub(super) type MembershipRow = (
    Uuid,
    Uuid,
    Uuid,
    String,
    u64,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

pub(super) fn decode_membership(row: MembershipRow) -> Result<Membership, RepositoryError> {
    let (
        id,
        organization_id,
        principal_id,
        role,
        aggregate_version,
        created_at,
        updated_at,
        revoked_at,
    ) = row;
    Ok(Membership {
        id: MembershipId::from_uuid(id),
        organization_id: OrganizationId::from_uuid(organization_id),
        principal_id: PrincipalId::from_uuid(principal_id),
        role: MembershipRole::parse(&role).map_err(|error| {
            RepositoryError::Storage(format!("stored membership role is invalid: {error}"))
        })?,
        aggregate_version,
        created_at,
        updated_at,
        revoked_at,
    })
}

pub(super) async fn insert_principal(
    transaction: &a3s_orm::PostgresTransaction,
    principal: &IdentityPrincipal,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into identity_principals (id, kind, name, aggregate_version, created_at, disabled_at) values (",
        )
        .bind(principal.id.as_uuid())
        .append(", ")
        .bind(principal.kind.as_str())
        .append(", ")
        .bind(principal.name.as_str())
        .append(", ")
        .bind(principal.aggregate_version)
        .append(", ")
        .bind(principal.created_at)
        .append(", ")
        .bind(principal.disabled_at)
        .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating identity principal affected {rows} rows"
        )));
    }
    Ok(())
}

pub(super) async fn insert_membership(
    transaction: &a3s_orm::PostgresTransaction,
    membership: &Membership,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into organization_memberships (id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at) values (",
        )
        .bind(membership.id.as_uuid())
        .append(", ")
        .bind(membership.organization_id.as_uuid())
        .append(", ")
        .bind(membership.principal_id.as_uuid())
        .append(", ")
        .bind(membership.role.as_str())
        .append(", ")
        .bind(membership.aggregate_version)
        .append(", ")
        .bind(membership.created_at)
        .append(", ")
        .bind(membership.updated_at)
        .append(", ")
        .bind(membership.revoked_at)
        .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating organization membership affected {rows} rows"
        )));
    }
    Ok(())
}

pub(super) async fn insert_token(
    transaction: &a3s_orm::PostgresTransaction,
    token: &ApiToken,
    digest: &ApiTokenDigest,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into api_tokens (id, organization_id, principal_id, name, name_key, token_hash, scopes, aggregate_version, created_at, expires_at, revoked_at) values (",
        )
        .bind(token.id.as_uuid())
        .append(", ")
        .bind(token.organization_id.as_uuid())
        .append(", ")
        .bind(token.principal_id.as_uuid())
        .append(", ")
        .bind(token.name.as_str())
        .append(", ")
        .bind(token.name.key())
        .append(", ")
        .bind(digest.as_str())
        .append(", ")
        .bind(serde_json::to_value(&token.scopes)?)
        .append(", ")
        .bind(token.aggregate_version)
        .append(", ")
        .bind(token.created_at)
        .append(", ")
        .bind(token.expires_at)
        .append(", ")
        .bind(token.revoked_at)
        .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating API token affected {rows} rows"
        )));
    }
    Ok(())
}

#[async_trait]
impl IIdentityBootstrapRepository for PostgresIdentityRepository {
    async fn installation_id(&self) -> Result<InstallationId, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_one_as(sql_query::<Uuid>(
                "select installation.id from cloud_installations installation where installation.singleton_key",
            ))
            .await
            .map(InstallationId::from_uuid)
            .map_err(|error| RepositoryError::Storage(error.to_string()))
    }

    async fn bootstrap_identity(
        &self,
        write: BootstrapIdentityWrite,
    ) -> Result<IdempotentWrite<IdentityBootstrap>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let BootstrapIdentityWrite {
                        bootstrap,
                        token_digest,
                        identity_events,
                        request_id,
                        idempotency,
                    } = write;
                    bootstrap
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let locked = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from (select pg_advisory_xact_lock(hashtext(",
                        )
                        .bind("a3s-cloud.identity.bootstrap")
                        .append("))) as locked"),
                    )
                    .await?;
                    if locked != Some(1) {
                        return Err(PostgresPersistenceError::Invariant(
                            "identity bootstrap lock did not return a row".into(),
                        ));
                    }
                    lock_installation(
                        transaction,
                        bootstrap.platform_rbac.policy.installation_id,
                    )
                    .await?;
                    if let Some(replayed) =
                        idempotency_replay::<IdentityBootstrap>(transaction, &idempotency).await?
                    {
                        replayed
                            .value
                            .validate()
                            .map_err(PostgresPersistenceError::Invariant)?;
                        return Ok(replayed);
                    }
                    let organization_count = fetch_optional::<i64, _>(
                        transaction,
                        sql_query::<i64>("select count(*) from organizations"),
                    )
                    .await?
                    .unwrap_or_default();
                    if organization_count != 0 {
                        return Err(RepositoryError::Conflict(
                            "Cloud identity has already been bootstrapped".into(),
                        )
                        .into());
                    }
                    let organization = &bootstrap.organization;
                    let organization_rows = execute(
                        transaction,
                        sql_query::<()>(
                            "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
                        )
                        .bind(organization.id.as_uuid())
                        .append(", ")
                        .bind(organization.name.as_str())
                        .append(", ")
                        .bind(organization.name.key())
                        .append(", ")
                        .bind(organization.aggregate_version)
                        .append(", ")
                        .bind(organization.created_at)
                        .append(")"),
                    )
                    .await?;
                    if organization_rows != 1 {
                        return Err(PostgresPersistenceError::Invariant(format!(
                            "bootstrapping organization affected {organization_rows} rows"
                        )));
                    }
                    insert_principal(transaction, &bootstrap.principal).await?;
                    insert_membership(transaction, &bootstrap.membership).await?;
                    insert_token(transaction, &bootstrap.api_token, &token_digest).await?;
                    persist_platform_rbac_bootstrap_under_installation_lock(
                        transaction,
                        &bootstrap.platform_rbac,
                        request_id,
                    )
                    .await?;
                    for event in &identity_events {
                        store_outbox(transaction, event).await?;
                    }
                    store_idempotency(transaction, &idempotency, &bootstrap).await?;
                    Ok(IdempotentWrite {
                        value: bootstrap,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

#[async_trait]
impl IApiTokenRepository for PostgresIdentityRepository {
    async fn create(
        &self,
        write: CreateApiTokenWrite,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError> {
        let CreateApiTokenWrite {
            token,
            digest,
            event,
            issuer_principal_id,
            idempotency,
        } = write;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_membership_set(transaction, token.organization_id).await?;
                    let target_membership = load_active_membership_for_update(
                        transaction,
                        token.organization_id,
                        token.principal_id,
                    )
                    .await?;
                    let Some(target_membership) = target_membership else {
                        return Err(RepositoryError::Forbidden(
                            "API token principal is not an active organization member".into(),
                        )
                        .into());
                    };
                    if token.principal_id != issuer_principal_id {
                        let issuer = load_active_membership_for_update(
                            transaction,
                            token.organization_id,
                            issuer_principal_id,
                        )
                        .await?;
                        MembershipAdministration::authorize(
                            issuer.as_ref(),
                            token.organization_id,
                            target_membership.role,
                            None,
                        )
                        .map_err(RepositoryError::Forbidden)?;
                    }
                    if let Some(replayed) =
                        idempotency_replay::<ApiToken>(transaction, &idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    let organization_exists = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>("select 1 from organizations where id = ")
                            .bind(token.organization_id.as_uuid()),
                    )
                    .await?
                    .is_some();
                    if !organization_exists {
                        return Err(RepositoryError::NotFound.into());
                    }
                    let membership_exists = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from identity_principals p join organization_memberships m on m.principal_id = p.id where p.id = ",
                        )
                        .bind(token.principal_id.as_uuid())
                        .append(" and p.disabled_at is null and m.organization_id = ")
                        .bind(token.organization_id.as_uuid())
                        .append(" and m.revoked_at is null"),
                    )
                    .await?
                    .is_some();
                    if !membership_exists {
                        return Err(RepositoryError::Forbidden(
                            "API token principal is not an active organization member".into(),
                        )
                        .into());
                    }
                    match insert_token(transaction, &token, &digest).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "API token name or credential is already in use".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(transaction, &event).await?;
                    store_idempotency(transaction, &idempotency, &token).await?;
                    Ok(IdempotentWrite {
                        value: token,
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
        token_id: ApiTokenId,
    ) -> Result<Option<ApiToken>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<ApiTokenRow>(
                    "select id, organization_id, principal_id, name, scopes, aggregate_version, created_at, expires_at, revoked_at from api_tokens where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" and id = ")
                .bind(token_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_token)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<ApiToken>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<ApiTokenRow>(
                    "select id, organization_id, principal_id, name, scopes, aggregate_version, created_at, expires_at, revoked_at from api_tokens where organization_id = ",
                )
                .bind(organization_id.as_uuid())
                .append(" order by created_at asc, id asc"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(decode_token)
            .collect()
    }

    async fn authenticate(
        &self,
        digest: &ApiTokenDigest,
        now: DateTime<Utc>,
    ) -> Result<Option<AuthenticatedApiToken>, RepositoryError> {
        let token = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<ApiTokenRow>(
                    "select id, organization_id, principal_id, name, scopes, aggregate_version, created_at, expires_at, revoked_at from api_tokens where token_hash = ",
                )
                .bind(digest.as_str())
                .append(" and revoked_at is null and (expires_at is null or expires_at > ")
                .bind(now)
                .append(")"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_token)
            .transpose()?;
        let Some(api_token) = token else {
            return Ok(None);
        };
        let principal = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<PrincipalRow>(
                    "select id, kind, name, aggregate_version, created_at, disabled_at from identity_principals where id = ",
                )
                .bind(api_token.principal_id.as_uuid())
                .append(" and disabled_at is null"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_principal)
            .transpose()?;
        let Some(principal) = principal else {
            return Ok(None);
        };
        let membership = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<MembershipRow>(
                    "select id, organization_id, principal_id, role, aggregate_version, created_at, updated_at, revoked_at from organization_memberships where organization_id = ",
                )
                .bind(api_token.organization_id.as_uuid())
                .append(" and principal_id = ")
                .bind(api_token.principal_id.as_uuid())
                .append(" and revoked_at is null"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_membership)
            .transpose()?;
        let is_platform_token = api_token
            .scopes
            .iter()
            .any(|scope| scope.as_str() == ApiTokenScope::PLATFORM_WRITE);
        if membership.is_none() && !is_platform_token {
            return Ok(None);
        }
        Ok(Some(AuthenticatedApiToken {
            api_token,
            principal,
            membership,
        }))
    }

    async fn revoke(
        &self,
        token: ApiToken,
        event: Option<DomainEventEnvelope>,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<ApiToken>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<ApiToken>(transaction, &idempotency).await?
                    {
                        return Ok(replayed);
                    }
                    if event.is_some() {
                        let previous_version =
                            token.aggregate_version.checked_sub(1).ok_or_else(|| {
                                PostgresPersistenceError::Invariant(
                                    "revoked API token has no previous aggregate version".into(),
                                )
                            })?;
                        let rows = execute(
                            transaction,
                            sql_query::<()>("update api_tokens set revoked_at = ")
                                .bind(token.revoked_at)
                                .append(", aggregate_version = ")
                                .bind(token.aggregate_version)
                                .append(" where organization_id = ")
                                .bind(token.organization_id.as_uuid())
                                .append(" and id = ")
                                .bind(token.id.as_uuid())
                                .append(" and aggregate_version = ")
                                .bind(previous_version)
                                .append(" and revoked_at is null"),
                        )
                        .await?;
                        if rows != 1 {
                            return Err(RepositoryError::Conflict(
                                "API token changed while it was being revoked".into(),
                            )
                            .into());
                        }
                    } else {
                        let exists = fetch_optional::<i32, _>(
                            transaction,
                            sql_query::<i32>("select 1 from api_tokens where organization_id = ")
                                .bind(token.organization_id.as_uuid())
                                .append(" and id = ")
                                .bind(token.id.as_uuid()),
                        )
                        .await?
                        .is_some();
                        if !exists {
                            return Err(RepositoryError::NotFound.into());
                        }
                    }
                    if let Some(event) = &event {
                        store_outbox(transaction, event).await?;
                    }
                    store_idempotency(transaction, &idempotency, &token).await?;
                    Ok(IdempotentWrite {
                        value: token,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}
