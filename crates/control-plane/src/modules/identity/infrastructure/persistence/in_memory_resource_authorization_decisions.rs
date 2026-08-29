use super::in_memory::InMemoryIdentityRepository;
use super::in_memory_memberships::actor_membership;
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
impl IResourceAuthorizationDecisionRepository for InMemoryIdentityRepository {
    async fn authorize_resource(
        &self,
        request: ResourceAuthorizationDecisionRequest,
    ) -> Result<AuthorizationDecisionRef, RepositoryError> {
        request.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let principal_is_active = state
            .principals
            .get(&request.principal_id)
            .is_some_and(|principal| principal.is_active());
        if !principal_is_active {
            return Err(RepositoryError::Forbidden(
                "authorization principal is not active".into(),
            ));
        }
        let credential = state
            .tokens
            .get(&request.credential_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Forbidden("authorization credential is not active".into())
            })?;
        let membership = actor_membership(&state, request.organization_id, request.principal_id)
            .ok_or_else(|| {
                RepositoryError::Forbidden(
                    "authorization principal is not an active organization member".into(),
                )
            })?;
        let grants = if membership.role == MembershipRole::Restricted {
            state
                .resource_grants
                .values()
                .filter(|grant| {
                    grant.organization_id == membership.organization_id
                        && grant.membership_id == membership.id
                        && grant.is_active()
                })
                .cloned()
                .collect::<Vec<_>>()
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
        let reference = decision.reference().map_err(RepositoryError::Storage)?;
        if state
            .resource_authorization_decisions
            .insert(decision.id, decision)
            .is_some()
        {
            return Err(RepositoryError::Conflict(
                "resource authorization decision already exists".into(),
            ));
        }
        Ok(reference)
    }
}
