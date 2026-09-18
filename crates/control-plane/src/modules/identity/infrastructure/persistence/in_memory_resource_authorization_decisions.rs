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
use std::collections::BTreeMap;
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
            let mut grants = state
                .resource_grants
                .values()
                .filter(|grant| {
                    grant.organization_id == membership.organization_id
                        && grant.membership_id == membership.id
                        && grant.is_active()
                })
                .cloned()
                .collect::<Vec<_>>();
            let subjects = state
                .directory_membership_projections
                .values()
                .filter(|binding| {
                    binding.organization_id == membership.organization_id
                        && binding.principal_id == membership.principal_id
                })
                .map(|binding| binding.subject.clone())
                .collect::<std::collections::BTreeSet<_>>();
            if !subjects.is_empty() {
                let mut by_id = grants
                    .iter()
                    .map(|grant| (grant.id, grant.clone()))
                    .collect::<BTreeMap<_, _>>();
                for grant in state.directory_resource_grants.values().filter(|grant| {
                    grant.organization_id == membership.organization_id
                        && grant.is_active()
                        && subjects.contains(&grant.subject)
                }) {
                    by_id
                        .entry(grant.id)
                        .or_insert_with(|| grant.as_effective_membership_grant(membership.id));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::{
        ApiToken, DirectoryMembershipProjectionBinding, DirectoryResourceGrant, IdentityPrincipal,
        IdentityPrincipalKind, Membership, Organization,
    };
    use crate::modules::identity::domain::value_objects::{
        ApiTokenName, ApiTokenScope, DirectoryGrantSubjectKind, DirectoryGrantSubjectRef,
        OidcIssuer, OrganizationName, ResourceGrantScope,
    };
    use crate::modules::shared_kernel::domain::{
        ApiTokenId, MembershipId, OrganizationId, PrincipalId, ProjectId, ResourceGrantId,
        ResourceName,
    };
    use chrono::Utc;
    use std::collections::BTreeSet;

    fn timestamp() -> chrono::DateTime<Utc> {
        Utc::now()
    }

    #[tokio::test]
    async fn expands_directory_grants_through_membership_projection() {
        let repository = InMemoryIdentityRepository::new();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let project_id = ProjectId::new();
        let credential_id = ApiTokenId::new();
        let membership_id = MembershipId::new();
        let subject = DirectoryGrantSubjectRef::new(
            DirectoryGrantSubjectKind::Department,
            OidcIssuer::parse("https://kense.example/directory").expect("issuer"),
            Uuid::nil(),
        );
        let now = timestamp();
        {
            let mut state = repository.state.write().await;
            state.organizations.insert(
                organization_id,
                Organization::create(
                    organization_id,
                    OrganizationName::parse("auth-expansion").expect("name"),
                    now,
                ),
            );
            state.principals.insert(
                principal_id,
                IdentityPrincipal::create(
                    principal_id,
                    IdentityPrincipalKind::Human,
                    ResourceName::parse("restricted").expect("name"),
                    now,
                ),
            );
            let membership = Membership::create(
                membership_id,
                organization_id,
                principal_id,
                MembershipRole::Restricted,
                now,
            );
            state
                .membership_subjects
                .insert((organization_id, principal_id), membership.id);
            state.memberships.insert(membership.id, membership);
            state.tokens.insert(
                credential_id,
                ApiToken::issue(
                    credential_id,
                    organization_id,
                    principal_id,
                    ApiTokenName::parse("token").expect("name"),
                    BTreeSet::from([
                        ApiTokenScope::parse(ApiTokenScope::WORKFLOW_WRITE).expect("scope")
                    ]),
                    now,
                    None,
                )
                .expect("token"),
            );
            state.directory_membership_projections.insert(
                (organization_id, subject.clone(), principal_id),
                DirectoryMembershipProjectionBinding::new(
                    organization_id,
                    subject.clone(),
                    principal_id,
                    now,
                ),
            );
            let directory_grant = DirectoryResourceGrant::create(
                ResourceGrantId::new(),
                organization_id,
                subject,
                ResourceGrantScope::Project { project_id },
                now,
            );
            state
                .directory_resource_grants
                .insert(directory_grant.id, directory_grant);
        }

        let reference = repository
            .authorize_resource(ResourceAuthorizationDecisionRequest {
                organization_id,
                principal_id,
                credential_id,
                required_scope: ApiTokenScope::parse(ApiTokenScope::WORKFLOW_WRITE)
                    .expect("scope"),
                action: "workflow.human-task.submit".into(),
                resource: ResourceGrantScope::Project { project_id },
                request_id: Uuid::now_v7(),
            })
            .await
            .expect("authorized via directory grant + projection");
        assert!(reference
            .id
            .starts_with("urn:a3s:cloud:identity:resource-authorization-decision:"));
    }

    #[tokio::test]
    async fn refuses_without_membership_grant_or_directory_projection() {
        let repository = InMemoryIdentityRepository::new();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let project_id = ProjectId::new();
        let credential_id = ApiTokenId::new();
        let now = timestamp();
        {
            let mut state = repository.state.write().await;
            state.organizations.insert(
                organization_id,
                Organization::create(
                    organization_id,
                    OrganizationName::parse("auth-deny").expect("name"),
                    now,
                ),
            );
            state.principals.insert(
                principal_id,
                IdentityPrincipal::create(
                    principal_id,
                    IdentityPrincipalKind::Human,
                    ResourceName::parse("restricted").expect("name"),
                    now,
                ),
            );
            let membership = Membership::create(
                MembershipId::new(),
                organization_id,
                principal_id,
                MembershipRole::Restricted,
                now,
            );
            state
                .membership_subjects
                .insert((organization_id, principal_id), membership.id);
            state.memberships.insert(membership.id, membership);
            state.tokens.insert(
                credential_id,
                ApiToken::issue(
                    credential_id,
                    organization_id,
                    principal_id,
                    ApiTokenName::parse("token").expect("name"),
                    BTreeSet::from([
                        ApiTokenScope::parse(ApiTokenScope::WORKFLOW_WRITE).expect("scope")
                    ]),
                    now,
                    None,
                )
                .expect("token"),
            );
        }

        let error = repository
            .authorize_resource(ResourceAuthorizationDecisionRequest {
                organization_id,
                principal_id,
                credential_id,
                required_scope: ApiTokenScope::parse(ApiTokenScope::WORKFLOW_WRITE)
                    .expect("scope"),
                action: "workflow.human-task.submit".into(),
                resource: ResourceGrantScope::Project { project_id },
                request_id: Uuid::now_v7(),
            })
            .await
            .expect_err("must remain fail-closed without grants");
        assert!(matches!(error, RepositoryError::Forbidden(_)));
    }
}
