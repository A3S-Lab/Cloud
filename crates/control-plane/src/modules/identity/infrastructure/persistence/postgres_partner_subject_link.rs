use super::postgres::PostgresIdentityRepository;
use super::postgres_memberships::{load_active_membership_for_update, load_principal};
use super::postgres_oidc_identity::{
    decode_link, insert_link, link_select, load_exact_link_for_update, ExternalIdentityLinkRow,
};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_unique_violation, store_audit,
    store_idempotency, store_outbox, transaction_error, AuditWrite,
};
use crate::modules::identity::domain::entities::{ExternalIdentityLink, IdentityPrincipalKind};
use crate::modules::identity::domain::repositories::{
    IPartnerSubjectLinkRepository, LinkPartnerSubjectWrite, RevokePartnerSubjectLinkWrite,
};
use crate::modules::identity::domain::value_objects::{
    is_partner_provider_key, ExternalIdentitySubject, OidcIssuer,
};
use crate::modules::shared_kernel::domain::{IdempotentWrite, PrincipalId, RepositoryError};
use a3s_orm::{sql_query, Database, PostgresDialect};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
impl IPartnerSubjectLinkRepository for PostgresIdentityRepository {
    async fn link_partner_subject(
        &self,
        write: LinkPartnerSubjectWrite,
    ) -> Result<IdempotentWrite<ExternalIdentityLink>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<ExternalIdentityLink>(transaction, &write.idempotency)
                            .await?
                    {
                        return Ok(replayed);
                    }
                    if !is_partner_provider_key(&write.link.provider_key) {
                        return Err(RepositoryError::Conflict(
                            "provider key is not a partner subject link".into(),
                        )
                        .into());
                    }
                    let membership = load_active_membership_for_update(
                        transaction,
                        write.organization_id,
                        write.link.principal_id,
                    )
                    .await?;
                    let principal = load_principal(transaction, write.link.principal_id)
                        .await?
                        .filter(|principal| {
                            principal.is_active()
                                && principal.kind == IdentityPrincipalKind::Human
                        });
                    let Some(principal) = principal.filter(|_| membership.is_some()) else {
                        return Err(RepositoryError::Forbidden(
                            "partner subject link principal is not an active human member".into(),
                        )
                        .into());
                    };
                    let existing = load_exact_link_for_update(
                        transaction,
                        &write.link.issuer,
                        &write.link.subject,
                    )
                    .await?;
                    let link = if let Some(existing) = existing {
                        if existing.is_active() {
                            if existing.principal_id != write.link.principal_id
                                || existing.provider_key != write.link.provider_key
                            {
                                return Err(RepositoryError::Conflict(
                                    "partner subject is already bound to another principal".into(),
                                )
                                .into());
                            }
                            existing
                        } else {
                            let mut rebound = ExternalIdentityLink::create(
                                existing.id,
                                write.link.provider_key.clone(),
                                write.link.issuer.clone(),
                                write.link.subject.clone(),
                                &principal,
                                write.link.created_at,
                            )
                            .map_err(RepositoryError::Conflict)?;
                            rebound.aggregate_version = existing.aggregate_version + 1;
                            let rows = execute(
                                transaction,
                                sql_query::<()>(
                                    "update external_identity_links set provider_key = ",
                                )
                                .bind(rebound.provider_key.as_str())
                                .append(", principal_id = ")
                                .bind(rebound.principal_id.as_uuid())
                                .append(", aggregate_version = ")
                                .bind(rebound.aggregate_version)
                                .append(", created_at = ")
                                .bind(rebound.created_at)
                                .append(", last_verified_at = ")
                                .bind(rebound.last_verified_at)
                                .append(", revoked_at = null where id = ")
                                .bind(rebound.id.as_uuid())
                                .append(" and aggregate_version = ")
                                .bind(existing.aggregate_version)
                                .append(" and revoked_at is not null"),
                            )
                            .await?;
                            if rows != 1 {
                                return Err(RepositoryError::Conflict(
                                    "partner subject link changed during rebound".into(),
                                )
                                .into());
                            }
                            rebound
                        }
                    } else {
                        let active_issuer_count = fetch_optional::<i64, _>(
                            transaction,
                            sql_query::<i64>(
                                "select count(*) from external_identity_links where principal_id = ",
                            )
                            .bind(write.link.principal_id.as_uuid())
                            .append(" and issuer = ")
                            .bind(write.link.issuer.as_str())
                            .append(" and revoked_at is null"),
                        )
                        .await?
                        .unwrap_or_default();
                        if active_issuer_count > 0 {
                            return Err(RepositoryError::Conflict(
                                "principal already has an active partner subject for this issuer"
                                    .into(),
                            )
                            .into());
                        }
                        match insert_link(transaction, &write.link).await {
                            Ok(()) => {}
                            Err(error) if is_unique_violation(&error) => {
                                return Err(RepositoryError::Conflict(
                                    "partner subject is already bound to another principal".into(),
                                )
                                .into())
                            }
                            Err(error) => return Err(error),
                        }
                        write.link.clone()
                    };
                    for event in &write.events {
                        store_outbox(transaction, event).await?;
                    }
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            scope: AuditWrite::organization_scope(
                                write.organization_id.as_uuid(),
                            ),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "identity.external-identity.linked",
                            aggregate_id: link.id.as_uuid(),
                            occurred_at: link.last_verified_at,
                            request_id: write.request_id,
                            details: serde_json::json!({
                                "principalId": link.principal_id,
                                "providerKey": link.provider_key.as_str(),
                                "issuer": link.issuer.as_str(),
                                "subject": link.subject.as_str(),
                                "aggregateVersion": link.aggregate_version,
                                "partnerSubjectLink": true,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &link).await?;
                    Ok(IdempotentWrite {
                        value: link,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn revoke_partner_subject_link(
        &self,
        write: RevokePartnerSubjectLinkWrite,
    ) -> Result<IdempotentWrite<ExternalIdentityLink>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replayed) =
                        idempotency_replay::<ExternalIdentityLink>(transaction, &write.idempotency)
                            .await?
                    {
                        return Ok(replayed);
                    }
                    let Some(mut link) = load_exact_link_for_update(
                        transaction,
                        &write.issuer,
                        &write.subject,
                    )
                    .await?
                    .filter(|link| {
                        link.is_active() && link.provider_key == write.provider_key
                    }) else {
                        return Err(RepositoryError::NotFound.into());
                    };
                    if link.aggregate_version != write.expected_version {
                        return Err(RepositoryError::Conflict(
                            "partner subject link version conflict".into(),
                        )
                        .into());
                    }
                    if !link.revoke(write.revoked_at) {
                        return Err(RepositoryError::Conflict(
                            "partner subject link is already revoked".into(),
                        )
                        .into());
                    }
                    let rows = execute(
                        transaction,
                        sql_query::<()>("update external_identity_links set aggregate_version = ")
                            .bind(link.aggregate_version)
                            .append(", revoked_at = ")
                            .bind(link.revoked_at)
                            .append(" where id = ")
                            .bind(link.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(write.expected_version)
                            .append(" and revoked_at is null"),
                    )
                    .await?;
                    if rows != 1 {
                        return Err(RepositoryError::Conflict(
                            "partner subject link changed during revocation".into(),
                        )
                        .into());
                    }
                    for event in &write.events {
                        store_outbox(transaction, event).await?;
                    }
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: Uuid::now_v7(),
                            scope: AuditWrite::organization_scope(
                                write.organization_id.as_uuid(),
                            ),
                            actor_id: Some(write.actor_principal_id.as_uuid()),
                            action: "identity.external-identity.revoked",
                            aggregate_id: link.id.as_uuid(),
                            occurred_at: write.revoked_at,
                            request_id: write.request_id,
                            details: serde_json::json!({
                                "principalId": link.principal_id,
                                "providerKey": link.provider_key.as_str(),
                                "issuer": link.issuer.as_str(),
                                "subject": link.subject.as_str(),
                                "aggregateVersion": link.aggregate_version,
                                "partnerSubjectLink": true,
                            }),
                        },
                    )
                    .await?;
                    store_idempotency(transaction, &write.idempotency, &link).await?;
                    Ok(IdempotentWrite {
                        value: link,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_active_partner_subject_link(
        &self,
        issuer: &OidcIssuer,
        subject: &ExternalIdentitySubject,
    ) -> Result<Option<ExternalIdentityLink>, RepositoryError> {
        let issuer = issuer.clone();
        let subject = subject.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    fetch_optional::<ExternalIdentityLinkRow, _>(
                        transaction,
                        sql_query::<ExternalIdentityLinkRow>(link_select())
                            .append(" where issuer = ")
                            .bind(issuer.as_str())
                            .append(" and subject = ")
                            .bind(subject.as_str())
                            .append(" and revoked_at is null and provider_key like 'partner-%'"),
                    )
                    .await?
                    .map(decode_link)
                    .transpose()
                    .map_err(Into::into)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn list_active_partner_subject_links_for_principal(
        &self,
        principal_id: PrincipalId,
    ) -> Result<Vec<ExternalIdentityLink>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<ExternalIdentityLinkRow, _>(
                        transaction,
                        sql_query::<ExternalIdentityLinkRow>(link_select())
                            .append(" where principal_id = ")
                            .bind(principal_id.as_uuid())
                            .append(
                                " and revoked_at is null and provider_key like 'partner-%' order by created_at, id",
                            ),
                    )
                    .await?;
                    rows.into_iter()
                        .map(decode_link)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(Into::into)
                })
            })
            .await
            .map_err(transaction_error)
    }
}
