use super::postgres::{decode_column, insert_token, PostgresIdentityRepository};
use super::postgres_memberships::{load_active_membership_for_update, load_principal};
use crate::infrastructure::{
    execute, fetch_optional, is_unique_violation, store_audit, store_outbox, transaction_error,
    AuditWrite, PostgresPersistenceError,
};
use crate::modules::identity::domain::entities::{
    ApiToken, ExternalIdentityLink, IdentityPrincipalKind, OidcFlow, OidcFlowPurpose,
};
use crate::modules::identity::domain::events::{ApiTokenCreated, ExternalIdentityChanged};
use crate::modules::identity::domain::repositories::{
    CompleteOidcLinkWrite, CompleteOidcLoginWrite, IOidcIdentityRepository,
};
use crate::modules::identity::domain::value_objects::{
    ExternalIdentitySubject, OidcIssuer, OidcProviderKey,
};
use crate::modules::shared_kernel::domain::{
    ExternalIdentityLinkId, OidcFlowId, OrganizationId, PrincipalId, RepositoryError, Sha256Digest,
};
use a3s_orm::{sql_query, Database, DecodeError, FromRow, PostgresDialect, Row};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct OidcFlowRow {
    id: Uuid,
    organization_id: Uuid,
    provider_key: String,
    issuer: String,
    provider_config_digest: String,
    purpose: String,
    principal_id: Option<Uuid>,
    state_digest: String,
    nonce_digest: String,
    pkce_verifier_digest: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
}

impl FromRow for OidcFlowRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode_column(row, 0)?,
            organization_id: decode_column(row, 1)?,
            provider_key: decode_column(row, 2)?,
            issuer: decode_column(row, 3)?,
            provider_config_digest: decode_column(row, 4)?,
            purpose: decode_column(row, 5)?,
            principal_id: decode_column(row, 6)?,
            state_digest: decode_column(row, 7)?,
            nonce_digest: decode_column(row, 8)?,
            pkce_verifier_digest: decode_column(row, 9)?,
            created_at: decode_column(row, 10)?,
            expires_at: decode_column(row, 11)?,
            consumed_at: decode_column(row, 12)?,
        })
    }
}

fn decode_flow(row: OidcFlowRow) -> Result<OidcFlow, RepositoryError> {
    Ok(OidcFlow {
        id: OidcFlowId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        provider_key: OidcProviderKey::parse(row.provider_key).map_err(stored("provider key"))?,
        issuer: OidcIssuer::parse(row.issuer).map_err(stored("issuer"))?,
        provider_config_digest: Sha256Digest::parse(row.provider_config_digest)
            .map_err(stored("provider configuration digest"))?,
        purpose: OidcFlowPurpose::parse(&row.purpose).map_err(stored("purpose"))?,
        principal_id: row.principal_id.map(PrincipalId::from_uuid),
        state_digest: Sha256Digest::parse(row.state_digest).map_err(stored("state digest"))?,
        nonce_digest: Sha256Digest::parse(row.nonce_digest).map_err(stored("nonce digest"))?,
        pkce_verifier_digest: Sha256Digest::parse(row.pkce_verifier_digest)
            .map_err(stored("PKCE verifier digest"))?,
        created_at: row.created_at,
        expires_at: row.expires_at,
        consumed_at: row.consumed_at,
    })
}

struct ExternalIdentityLinkRow {
    id: Uuid,
    provider_key: String,
    issuer: String,
    subject: String,
    principal_id: Uuid,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    last_verified_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

impl FromRow for ExternalIdentityLinkRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode_column(row, 0)?,
            provider_key: decode_column(row, 1)?,
            issuer: decode_column(row, 2)?,
            subject: decode_column(row, 3)?,
            principal_id: decode_column(row, 4)?,
            aggregate_version: decode_column(row, 5)?,
            created_at: decode_column(row, 6)?,
            last_verified_at: decode_column(row, 7)?,
            revoked_at: decode_column(row, 8)?,
        })
    }
}

fn decode_link(row: ExternalIdentityLinkRow) -> Result<ExternalIdentityLink, RepositoryError> {
    Ok(ExternalIdentityLink {
        id: ExternalIdentityLinkId::from_uuid(row.id),
        provider_key: OidcProviderKey::parse(row.provider_key).map_err(stored("provider key"))?,
        issuer: OidcIssuer::parse(row.issuer).map_err(stored("issuer"))?,
        subject: ExternalIdentitySubject::parse(row.subject).map_err(stored("subject"))?,
        principal_id: PrincipalId::from_uuid(row.principal_id),
        aggregate_version: row.aggregate_version,
        created_at: row.created_at,
        last_verified_at: row.last_verified_at,
        revoked_at: row.revoked_at,
    })
}

fn stored(field: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored OIDC {field} is invalid: {error}"))
}

fn flow_select() -> &'static str {
    "select id, organization_id, provider_key, issuer, provider_config_digest, purpose, principal_id, state_digest, nonce_digest, pkce_verifier_digest, created_at, expires_at, consumed_at from oidc_flows"
}

fn link_select() -> &'static str {
    "select id, provider_key, issuer, subject, principal_id, aggregate_version, created_at, last_verified_at, revoked_at from external_identity_links"
}

async fn load_flow_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    flow_id: OidcFlowId,
) -> Result<Option<OidcFlow>, PostgresPersistenceError> {
    fetch_optional::<OidcFlowRow, _>(
        transaction,
        sql_query::<OidcFlowRow>(flow_select())
            .append(" where id = ")
            .bind(flow_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_flow)
    .transpose()
    .map_err(Into::into)
}

async fn consume_flow(
    transaction: &a3s_orm::PostgresTransaction,
    flow: &mut OidcFlow,
    state_digest: &Sha256Digest,
    nonce_digest: &Sha256Digest,
    pkce_verifier_digest: &Sha256Digest,
    completed_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    flow.consume(
        state_digest,
        nonce_digest,
        pkce_verifier_digest,
        completed_at,
    )
    .map_err(|error| RepositoryError::Conflict(error.to_string()))?;
    let rows = execute(
        transaction,
        sql_query::<()>("update oidc_flows set consumed_at = ")
            .bind(flow.consumed_at)
            .append(" where id = ")
            .bind(flow.id.as_uuid())
            .append(" and consumed_at is null"),
    )
    .await?;
    if rows != 1 {
        return Err(RepositoryError::Conflict("OIDC flow was already consumed".into()).into());
    }
    Ok(())
}

async fn load_exact_link_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    issuer: &OidcIssuer,
    subject: &ExternalIdentitySubject,
) -> Result<Option<ExternalIdentityLink>, PostgresPersistenceError> {
    fetch_optional::<ExternalIdentityLinkRow, _>(
        transaction,
        sql_query::<ExternalIdentityLinkRow>(link_select())
            .append(" where issuer = ")
            .bind(issuer.as_str())
            .append(" and subject = ")
            .bind(subject.as_str())
            .append(" for update"),
    )
    .await?
    .map(decode_link)
    .transpose()
    .map_err(Into::into)
}

async fn insert_link(
    transaction: &a3s_orm::PostgresTransaction,
    link: &ExternalIdentityLink,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("insert into external_identity_links (id, provider_key, issuer, subject, principal_id, aggregate_version, created_at, last_verified_at, revoked_at) values (")
            .bind(link.id.as_uuid())
            .append(", ")
            .bind(link.provider_key.as_str())
            .append(", ")
            .bind(link.issuer.as_str())
            .append(", ")
            .bind(link.subject.as_str())
            .append(", ")
            .bind(link.principal_id.as_uuid())
            .append(", ")
            .bind(link.aggregate_version)
            .append(", ")
            .bind(link.created_at)
            .append(", ")
            .bind(link.last_verified_at)
            .append(", ")
            .bind(link.revoked_at)
            .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating external identity link affected {rows} rows"
        )));
    }
    Ok(())
}

#[async_trait]
impl IOidcIdentityRepository for PostgresIdentityRepository {
    async fn begin_oidc_flow(&self, flow: OidcFlow) -> Result<OidcFlow, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let organization_exists = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>("select 1 from organizations where id = ")
                            .bind(flow.organization_id.as_uuid()),
                    )
                    .await?
                    .is_some();
                    if !organization_exists {
                        return Err(RepositoryError::NotFound.into());
                    }
                    if let Some(principal_id) = flow.principal_id {
                        let membership = load_active_membership_for_update(
                            transaction,
                            flow.organization_id,
                            principal_id,
                        )
                        .await?;
                        let principal = load_principal(transaction, principal_id)
                            .await?
                            .filter(|principal| {
                                principal.is_active()
                                    && principal.kind == IdentityPrincipalKind::Human
                            });
                        if membership.is_none() || principal.is_none() {
                            return Err(RepositoryError::Forbidden(
                                "OIDC link flow requires an active human organization member"
                                    .into(),
                            )
                            .into());
                        }
                    }
                    let inserted = execute(
                        transaction,
                        sql_query::<()>("insert into oidc_flows (id, organization_id, provider_key, issuer, provider_config_digest, purpose, principal_id, state_digest, nonce_digest, pkce_verifier_digest, created_at, expires_at, consumed_at) values (")
                            .bind(flow.id.as_uuid())
                            .append(", ")
                            .bind(flow.organization_id.as_uuid())
                            .append(", ")
                            .bind(flow.provider_key.as_str())
                            .append(", ")
                            .bind(flow.issuer.as_str())
                            .append(", ")
                            .bind(flow.provider_config_digest.as_str())
                            .append(", ")
                            .bind(flow.purpose.as_str())
                            .append(", ")
                            .bind(flow.principal_id.map(PrincipalId::as_uuid))
                            .append(", ")
                            .bind(flow.state_digest.as_str())
                            .append(", ")
                            .bind(flow.nonce_digest.as_str())
                            .append(", ")
                            .bind(flow.pkce_verifier_digest.as_str())
                            .append(", ")
                            .bind(flow.created_at)
                            .append(", ")
                            .bind(flow.expires_at)
                            .append(", ")
                            .bind(flow.consumed_at)
                            .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(1) => Ok(flow),
                        Ok(rows) => Err(PostgresPersistenceError::Invariant(format!(
                            "creating OIDC flow affected {rows} rows"
                        ))),
                        Err(error) if is_unique_violation(&error) => Err(
                            RepositoryError::Conflict("OIDC flow identity is already in use".into())
                                .into(),
                        ),
                        Err(error) => Err(error),
                    }
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_pending_oidc_flow(
        &self,
        state_digest: &Sha256Digest,
        now: DateTime<Utc>,
    ) -> Result<Option<OidcFlow>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<OidcFlowRow>(flow_select())
                    .append(" where state_digest = ")
                    .bind(state_digest.as_str())
                    .append(" and consumed_at is null and expires_at > ")
                    .bind(now),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(decode_flow)
            .transpose()
    }

    async fn complete_oidc_link(
        &self,
        write: CompleteOidcLinkWrite,
    ) -> Result<ExternalIdentityLink, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let mut flow = load_flow_for_update(transaction, write.flow_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    if flow.purpose != OidcFlowPurpose::Link {
                        return Err(RepositoryError::Conflict(
                            "OIDC flow purpose does not permit identity linking".into(),
                        )
                        .into());
                    }
                    if flow.provider_config_digest != write.provider_config_digest {
                        return Err(RepositoryError::Conflict(
                            "OIDC provider configuration changed during the flow".into(),
                        )
                        .into());
                    }
                    consume_flow(
                        transaction,
                        &mut flow,
                        &write.state_digest,
                        &write.nonce_digest,
                        &write.pkce_verifier_digest,
                        write.completed_at,
                    )
                    .await?;
                    let principal_id = flow.principal_id.ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "OIDC link flow lost its principal binding".into(),
                        )
                    })?;
                    let membership = load_active_membership_for_update(
                        transaction,
                        flow.organization_id,
                        principal_id,
                    )
                    .await?;
                    let principal =
                        load_principal(transaction, principal_id)
                            .await?
                            .filter(|principal| {
                                principal.is_active()
                                    && principal.kind == IdentityPrincipalKind::Human
                            });
                    let Some(principal) = principal.filter(|_| membership.is_some()) else {
                        return Err(RepositoryError::Forbidden(
                            "OIDC link principal is not an active organization member".into(),
                        )
                        .into());
                    };
                    let existing =
                        load_exact_link_for_update(transaction, &flow.issuer, &write.subject)
                            .await?;
                    let (link, event_kind) = if let Some(mut link) = existing {
                        if !link.is_active()
                            || link.principal_id != principal_id
                            || link.provider_key != flow.provider_key
                        {
                            return Err(RepositoryError::Conflict(
                                "external identity is already bound and cannot be reassigned"
                                    .into(),
                            )
                            .into());
                        }
                        let expected_version = link.aggregate_version;
                        let changed = link
                            .record_verification(write.completed_at)
                            .map_err(RepositoryError::Conflict)?;
                        if changed {
                            let rows = execute(
                                transaction,
                                sql_query::<()>(
                                    "update external_identity_links set aggregate_version = ",
                                )
                                .bind(link.aggregate_version)
                                .append(", last_verified_at = ")
                                .bind(link.last_verified_at)
                                .append(" where id = ")
                                .bind(link.id.as_uuid())
                                .append(" and aggregate_version = ")
                                .bind(expected_version)
                                .append(" and revoked_at is null"),
                            )
                            .await?;
                            if rows != 1 {
                                return Err(RepositoryError::Conflict(
                                    "external identity changed during verification".into(),
                                )
                                .into());
                            }
                        }
                        (link, changed.then_some(false))
                    } else {
                        let link = ExternalIdentityLink::create(
                            ExternalIdentityLinkId::new(),
                            flow.provider_key.clone(),
                            flow.issuer.clone(),
                            write.subject,
                            &principal,
                            write.completed_at,
                        )
                        .map_err(RepositoryError::Conflict)?;
                        match insert_link(transaction, &link).await {
                            Ok(()) => {}
                            Err(error) if is_unique_violation(&error) => {
                                return Err(RepositoryError::Conflict(
                                    "external identity or active issuer binding already exists"
                                        .into(),
                                )
                                .into())
                            }
                            Err(error) => return Err(error),
                        }
                        (link, Some(true))
                    };
                    if let Some(newly_linked) = event_kind {
                        let event = if newly_linked {
                            ExternalIdentityChanged::linked(
                                &link,
                                flow.organization_id,
                                write.request_id,
                            )
                        } else {
                            ExternalIdentityChanged::verified(
                                &link,
                                flow.organization_id,
                                write.request_id,
                            )
                        }?;
                        store_outbox(transaction, &event).await?;
                        store_audit(
                            transaction,
                            &AuditWrite {
                                audit_id: Uuid::now_v7(),
                                organization_id: flow.organization_id.as_uuid(),
                                actor_id: Some(principal_id.as_uuid()),
                                action: if newly_linked {
                                    "identity.external-identity.linked"
                                } else {
                                    "identity.external-identity.verified"
                                },
                                aggregate_id: link.id.as_uuid(),
                                occurred_at: link.last_verified_at,
                                request_id: write.request_id,
                                details: serde_json::json!({
                                    "principalId": link.principal_id,
                                    "providerKey": link.provider_key.as_str(),
                                    "issuer": link.issuer.as_str(),
                                    "aggregateVersion": link.aggregate_version,
                                }),
                            },
                        )
                        .await?;
                    }
                    Ok(link)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn complete_oidc_login(
        &self,
        write: CompleteOidcLoginWrite,
    ) -> Result<ApiToken, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let mut flow = load_flow_for_update(transaction, write.flow_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    if flow.purpose != OidcFlowPurpose::Login {
                        return Err(RepositoryError::Conflict(
                            "OIDC flow purpose does not permit login".into(),
                        )
                        .into());
                    }
                    if flow.provider_config_digest != write.provider_config_digest {
                        return Err(RepositoryError::Conflict(
                            "OIDC provider configuration changed during the flow".into(),
                        )
                        .into());
                    }
                    consume_flow(
                        transaction,
                        &mut flow,
                        &write.state_digest,
                        &write.nonce_digest,
                        &write.pkce_verifier_digest,
                        write.completed_at,
                    )
                    .await?;
                    let link =
                        load_exact_link_for_update(transaction, &flow.issuer, &write.subject)
                            .await?
                            .filter(|link| {
                                link.is_active() && link.provider_key == flow.provider_key
                            })
                            .ok_or(RepositoryError::NotFound)?;
                    let membership = load_active_membership_for_update(
                        transaction,
                        flow.organization_id,
                        link.principal_id,
                    )
                    .await?;
                    let principal = load_principal(transaction, link.principal_id)
                        .await?
                        .filter(|principal| {
                            principal.is_active() && principal.kind == IdentityPrincipalKind::Human
                        });
                    if membership.is_none() || principal.is_none() {
                        return Err(RepositoryError::NotFound.into());
                    }
                    let token = ApiToken::issue_oidc_login(
                        write.token_id,
                        flow.organization_id,
                        link.principal_id,
                        write.token_name,
                        write.completed_at,
                        write.token_expires_at,
                    )
                    .map_err(RepositoryError::Conflict)?;
                    if token.scopes.iter().any(|scope| {
                        matches!(
                            scope.as_str(),
                            crate::modules::identity::domain::value_objects::ApiTokenScope::PLATFORM_WRITE
                                | crate::modules::identity::domain::value_objects::ApiTokenScope::TOKEN_WRITE
                        )
                    }) {
                        return Err(PostgresPersistenceError::Invariant(
                            "OIDC login token unexpectedly received platform or token-minting authority"
                                .into(),
                        ));
                    }
                    match insert_token(transaction, &token, &write.token_digest).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "OIDC login credential identity is already in use".into(),
                            )
                            .into())
                        }
                        Err(error) => return Err(error),
                    }
                    store_outbox(
                        transaction,
                        &ApiTokenCreated::envelope(&token, write.request_id)?,
                    )
                    .await?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            organization_id: flow.organization_id.as_uuid(),
                            actor_id: Some(link.principal_id.as_uuid()),
                            action: "identity.oidc.login",
                            aggregate_id: token.id.as_uuid(),
                            occurred_at: token.created_at,
                            request_id: write.request_id,
                            details: serde_json::json!({
                                "principalId": link.principal_id,
                                "providerKey": flow.provider_key.as_str(),
                                "issuer": flow.issuer.as_str(),
                                "tokenExpiresAt": token.expires_at,
                            }),
                        },
                    )
                    .await?;
                    Ok(token)
                })
            })
            .await
            .map_err(transaction_error)
    }
}
