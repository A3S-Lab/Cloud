use super::postgres::{load_api_token_by_id_for_authorization, PostgresIdentityRepository};
use super::postgres_platform_rbac::{
    load_active_actor_binding_for_authorization, load_active_principal_for_authorization,
    load_current_policy_for_authorization, lock_installation_for_authorization,
};
use super::postgres_tenant_support_grants::load_grant_for_authorization;
use crate::infrastructure::{store_audit, transaction_error, AuditWrite};
use crate::modules::identity::domain::repositories::IPrivilegedAuthorizationDecisionRepository;
use crate::modules::identity::domain::services::{
    PrivilegedAuthorizationDecision, PrivilegedAuthorizationDecisionRequest,
};
use crate::modules::shared_kernel::domain::{
    AuthorizationDecisionRef, PrivilegedAuthorizationDecisionId, RepositoryError,
};
use async_trait::async_trait;
use chrono::Utc;

#[async_trait]
impl IPrivilegedAuthorizationDecisionRepository for PostgresIdentityRepository {
    async fn authorize_privileged(
        &self,
        request: PrivilegedAuthorizationDecisionRequest,
    ) -> Result<AuthorizationDecisionRef, RepositoryError> {
        request.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let installation_id = request.scope.installation_id();
                    lock_installation_for_authorization(transaction, installation_id).await?;
                    let decided_at = Utc::now();
                    let principal =
                        load_active_principal_for_authorization(transaction, request.principal_id)
                            .await?
                            .ok_or_else(|| {
                                RepositoryError::Forbidden(
                                    "privileged authorization principal is not active".into(),
                                )
                            })?;
                    let credential =
                        load_api_token_by_id_for_authorization(transaction, request.credential_id)
                            .await?
                            .filter(|credential| {
                                credential.principal_id == request.principal_id
                                    && credential.is_active_at(decided_at)
                            })
                            .ok_or_else(|| {
                                RepositoryError::Forbidden(
                                    "privileged authorization credential is not active".into(),
                                )
                            })?;
                    let policy =
                        load_current_policy_for_authorization(transaction, installation_id)
                            .await?
                            .ok_or_else(|| {
                                RepositoryError::Forbidden(
                                    "privileged authorization has no current platform policy"
                                        .into(),
                                )
                            })?;
                    let binding = load_active_actor_binding_for_authorization(
                        transaction,
                        installation_id,
                        request.principal_id,
                    )
                    .await?;
                    let support_grant = match request.support_grant_id {
                        Some(grant_id) => Some(
                            load_grant_for_authorization(transaction, installation_id, grant_id)
                                .await?
                                .ok_or_else(|| {
                                    RepositoryError::Forbidden(
                                        "privileged authorization support grant is unavailable"
                                            .into(),
                                    )
                                })?,
                        ),
                        None => None,
                    };
                    let decision = match support_grant.as_ref() {
                        Some(grant) => PrivilegedAuthorizationDecision::issue_tenant_support(
                            PrivilegedAuthorizationDecisionId::new(),
                            request,
                            &principal,
                            &credential,
                            &policy,
                            &[binding],
                            grant,
                            decided_at,
                        ),
                        None => PrivilegedAuthorizationDecision::issue_platform(
                            PrivilegedAuthorizationDecisionId::new(),
                            request,
                            &principal,
                            &credential,
                            &policy,
                            &[binding],
                            decided_at,
                        ),
                    }
                    .map_err(RepositoryError::Forbidden)?;
                    let reference = decision.reference().map_err(|error| {
                        crate::infrastructure::PostgresPersistenceError::Invariant(error)
                    })?;
                    let details = serde_json::to_value(&decision).map_err(|error| {
                        crate::infrastructure::PostgresPersistenceError::Invariant(format!(
                            "privileged authorization decision could not be encoded: {error}"
                        ))
                    })?;
                    store_audit(
                        transaction,
                        &AuditWrite {
                            audit_id: decision.id.as_uuid(),
                            actor_id: Some(decision.principal_id.as_uuid()),
                            action: PrivilegedAuthorizationDecision::audit_action(),
                            aggregate_id: decision.resource_id,
                            occurred_at: decision.decided_at,
                            request_id: decision.request_id,
                            scope: decision.scope.reference(),
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
