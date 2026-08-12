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
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::domain::{AuthorizationDecisionRef, RepositoryError};
use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

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
                    let decision = if request.actor_is_platform_admin {
                        ResourceAuthorizationDecision::issue_platform_administrator(
                            Uuid::now_v7(),
                            request,
                            &credential,
                            Utc::now(),
                        )
                    } else {
                        let membership = load_active_membership_for_update(
                            transaction,
                            request.organization_id,
                            request.principal_id,
                        )
                        .await?
                        .ok_or_else(|| {
                            RepositoryError::Forbidden(
                                "authorization principal is not an active organization member"
                                    .into(),
                            )
                        })?;
                        let grants = if membership.role == MembershipRole::Restricted {
                            load_active_resource_grants_for_membership(
                                transaction,
                                membership.organization_id,
                                membership.id,
                            )
                            .await?
                        } else {
                            Vec::new()
                        };
                        ResourceAuthorizationDecision::issue_membership(
                            Uuid::now_v7(),
                            request,
                            &credential,
                            &membership,
                            grants,
                            Utc::now(),
                        )
                    }
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
                            organization_id: decision.organization_id.as_uuid(),
                            actor_id: Some(decision.principal_id.as_uuid()),
                            action: ResourceAuthorizationDecision::audit_action(),
                            aggregate_id: decision.aggregate_id(),
                            occurred_at: decision.decided_at,
                            request_id: decision.request_id,
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
