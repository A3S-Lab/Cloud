use super::postgres::{load_api_token_for_update, PostgresIdentityRepository};
use super::postgres_memberships::{
    load_active_membership_for_update, load_principal, lock_membership_set,
};
use super::postgres_resource_grants::load_active_resource_grants_for_membership;
use crate::infrastructure::{store_audit, transaction_error, AuditWrite};
use crate::modules::identity::domain::repositories::IResourceAuthorizationDecisionRepository;
use crate::modules::identity::domain::services::{
    ResourceAuthorizationDecision, ResourceAuthorizationDecisionRequest,
};
use crate::modules::identity::domain::value_objects::{
    DirectoryGrantSubjectKind, DirectoryGrantSubjectRef, MembershipRole, OidcIssuer,
};
use crate::modules::shared_kernel::domain::{
    AuthorizationDecisionRef, OrganizationId, PrincipalId, RepositoryError,
};
use a3s_orm::{sql_query, DecodeError, FromRow, Row};
use async_trait::async_trait;
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

use super::postgres::decode_column;
use super::postgres_directory_resource_grants::load_active_directory_resource_grants_for_organization;

struct DirectoryMembershipSubjectRow {
    subject_kind: String,
    directory_issuer: String,
    directory_subject_id: Uuid,
}

impl FromRow for DirectoryMembershipSubjectRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            subject_kind: decode_column(row, 0)?,
            directory_issuer: decode_column(row, 1)?,
            directory_subject_id: decode_column(row, 2)?,
        })
    }
}

fn decode_subject(
    row: DirectoryMembershipSubjectRow,
) -> Result<DirectoryGrantSubjectRef, RepositoryError> {
    let kind =
        DirectoryGrantSubjectKind::parse(&row.subject_kind).map_err(RepositoryError::Storage)?;
    let issuer = OidcIssuer::parse(row.directory_issuer).map_err(|error| {
        RepositoryError::Storage(format!("stored directory issuer is invalid: {error}"))
    })?;
    Ok(DirectoryGrantSubjectRef::new(
        kind,
        issuer,
        row.directory_subject_id,
    ))
}

async fn load_subjects_for_principal(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
) -> Result<Vec<DirectoryGrantSubjectRef>, crate::infrastructure::PostgresPersistenceError> {
    use crate::infrastructure::fetch_all;
    fetch_all::<DirectoryMembershipSubjectRow, _>(
        transaction,
        sql_query::<DirectoryMembershipSubjectRow>(
            "select subject_kind, directory_issuer, directory_subject_id from directory_membership_projections where organization_id = ",
        )
        .bind(organization_id.as_uuid())
        .append(" and principal_id = ")
        .bind(principal_id.as_uuid()),
    )
    .await?
    .into_iter()
    .map(decode_subject)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

#[async_trait]
impl IResourceAuthorizationDecisionRepository for PostgresIdentityRepository {
    async fn authorize_resource(
        &self,
        request: ResourceAuthorizationDecisionRequest,
    ) -> Result<AuthorizationDecisionRef, RepositoryError> {
        request.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    lock_membership_set(transaction, request.organization_id).await?;
                    let principal = load_principal(transaction, request.principal_id)
                        .await?
                        .filter(|principal| principal.is_active())
                        .ok_or_else(|| {
                            RepositoryError::Forbidden(
                                "authorization principal is not active".into(),
                            )
                        })?;
                    let credential = load_api_token_for_update(
                        transaction,
                        request.organization_id,
                        request.credential_id,
                    )
                    .await?
                    .ok_or_else(|| {
                        RepositoryError::Forbidden("authorization credential is not active".into())
                    })?;
                    if principal.id != credential.principal_id {
                        return Err(RepositoryError::Forbidden(
                            "authorization credential belongs to another principal".into(),
                        )
                        .into());
                    }
                    let membership = load_active_membership_for_update(
                        transaction,
                        request.organization_id,
                        request.principal_id,
                    )
                    .await?
                    .ok_or_else(|| {
                        RepositoryError::Forbidden(
                            "authorization principal is not an active organization member".into(),
                        )
                    })?;
                    let grants = if membership.role == MembershipRole::Restricted {
                        let mut grants = load_active_resource_grants_for_membership(
                            transaction,
                            membership.organization_id,
                            membership.id,
                        )
                        .await?;
                        let subjects = load_subjects_for_principal(
                            transaction,
                            membership.organization_id,
                            membership.principal_id,
                        )
                        .await?;
                        if !subjects.is_empty() {
                            let subject_set = subjects.into_iter().collect::<BTreeSet<_>>();
                            let directory_grants =
                                load_active_directory_resource_grants_for_organization(
                                    transaction,
                                    membership.organization_id,
                                )
                                .await?;
                            let mut by_id = grants
                                .iter()
                                .map(|grant| (grant.id, grant.clone()))
                                .collect::<BTreeMap<_, _>>();
                            for grant in directory_grants
                                .into_iter()
                                .filter(|grant| subject_set.contains(&grant.subject))
                            {
                                by_id.entry(grant.id).or_insert_with(|| {
                                    grant.as_effective_membership_grant(membership.id)
                                });
                            }
                            grants = by_id.into_values().collect();
                        }
                        grants
                    } else {
                        Vec::new()
                    };
                    let decision = ResourceAuthorizationDecision::issue_membership(
                        Uuid::now_v7(),
                        request,
                        &credential,
                        &membership,
                        grants,
                        Utc::now(),
                    )
                    .map_err(RepositoryError::Forbidden)?;
                    let reference = decision.reference().map_err(|error| {
                        crate::infrastructure::PostgresPersistenceError::Invariant(error)
                    })?;
                    let details = serde_json::to_value(&decision).map_err(|error| {
                        crate::infrastructure::PostgresPersistenceError::Invariant(format!(
                            "resource authorization decision could not be encoded: {error}"
                        ))
                    })?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: decision.id,
                            actor_id: Some(decision.principal_id.as_uuid()),
                            action: ResourceAuthorizationDecision::audit_action(),
                            aggregate_id: decision.aggregate_id(),
                            occurred_at: decision.decided_at,
                            request_id: decision.request_id,
                            scope: decision.resource.project_id().map_or_else(
                                || {
                                    AuditWrite::organization_scope(
                                        decision.organization_id.as_uuid(),
                                    )
                                },
                                |project_id| {
                                    AuditWrite::resource_scope(
                                        decision.organization_id.as_uuid(),
                                        project_id,
                                        decision.resource.environment_id(),
                                    )
                                },
                            ),
                            details,
                        },
                    )
                    .await?;
                    Ok(reference)
                })
            })
            .await
            .map_err(transaction_error)
    }
}
