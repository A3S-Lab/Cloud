use super::in_memory::{remember, replay, InMemoryIdentityRepository};
use super::in_memory_memberships::{actor_membership, authorize_management};
use crate::modules::identity::domain::events::ResourceGrantChanged;
use crate::modules::identity::domain::repositories::{
    CreateResourceGrantWrite, IResourceGrantRepository, RevokeResourceGrantWrite,
    MAX_ACTIVE_RESOURCE_GRANTS_PER_MEMBERSHIP,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, MembershipId, OrganizationId, RepositoryError, ResourceGrantId,
};
use async_trait::async_trait;

#[async_trait]
impl IResourceGrantRepository for InMemoryIdentityRepository {
    async fn create_resource_grant(
        &self,
        write: CreateResourceGrantWrite,
    ) -> Result<
        IdempotentWrite<crate::modules::identity::domain::entities::ResourceGrant>,
        RepositoryError,
    > {
        let mut state = self.state.write().await;
        let target = state
            .memberships
            .get(&write.grant.membership_id)
            .filter(|membership| membership.organization_id == write.grant.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let actor = actor_membership(
            &state,
            write.grant.organization_id,
            write.actor_principal_id,
        );
        authorize_management(
            actor.as_ref(),
            write.actor_is_platform_admin,
            target.role,
            None,
        )?;
        if let Some(existing) = replay(&state, &write.idempotency)? {
            return Ok(existing);
        }
        if !target.is_active() {
            return Err(RepositoryError::Conflict(
                "Resource Grants require an active membership".into(),
            ));
        }
        if !write.grant.is_active()
            || write.grant.aggregate_version != 1
            || state.resource_grants.contains_key(&write.grant.id)
        {
            return Err(RepositoryError::Conflict(
                "Resource Grant already exists or is not new".into(),
            ));
        }
        let active_grants = state
            .resource_grants
            .values()
            .filter(|grant| {
                grant.organization_id == write.grant.organization_id
                    && grant.membership_id == write.grant.membership_id
                    && grant.is_active()
            })
            .collect::<Vec<_>>();
        if active_grants.len() >= usize::from(MAX_ACTIVE_RESOURCE_GRANTS_PER_MEMBERSHIP) {
            return Err(RepositoryError::Conflict(format!(
                "membership cannot have more than {MAX_ACTIVE_RESOURCE_GRANTS_PER_MEMBERSHIP} active Resource Grants"
            )));
        }
        if active_grants
            .iter()
            .any(|grant| grant.scope == write.grant.scope)
        {
            return Err(RepositoryError::Conflict(
                "an active Resource Grant already covers this exact scope".into(),
            ));
        }
        let grant = write.grant;
        state.resource_grants.insert(grant.id, grant.clone());
        remember(&mut state, write.idempotency, &grant)?;
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: grant,
            replayed: false,
        })
    }

    async fn find_resource_grant(
        &self,
        organization_id: OrganizationId,
        resource_grant_id: ResourceGrantId,
    ) -> Result<Option<crate::modules::identity::domain::entities::ResourceGrant>, RepositoryError>
    {
        Ok(self
            .state
            .read()
            .await
            .resource_grants
            .get(&resource_grant_id)
            .filter(|grant| grant.organization_id == organization_id)
            .cloned())
    }

    async fn list_resource_grants(
        &self,
        organization_id: OrganizationId,
        membership_id: Option<MembershipId>,
    ) -> Result<Vec<crate::modules::identity::domain::entities::ResourceGrant>, RepositoryError>
    {
        let state = self.state.read().await;
        let mut grants = state
            .resource_grants
            .values()
            .filter(|grant| {
                grant.organization_id == organization_id
                    && membership_id.is_none_or(|id| grant.membership_id == id)
            })
            .cloned()
            .collect::<Vec<_>>();
        grants.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.as_uuid().cmp(&right.id.as_uuid()))
        });
        Ok(grants)
    }

    async fn list_active_resource_grants_for_membership(
        &self,
        organization_id: OrganizationId,
        membership_id: MembershipId,
    ) -> Result<Vec<crate::modules::identity::domain::entities::ResourceGrant>, RepositoryError>
    {
        Ok(self
            .list_resource_grants(organization_id, Some(membership_id))
            .await?
            .into_iter()
            .filter(|grant| grant.is_active())
            .collect())
    }

    async fn revoke_resource_grant(
        &self,
        write: RevokeResourceGrantWrite,
    ) -> Result<
        IdempotentWrite<crate::modules::identity::domain::entities::ResourceGrant>,
        RepositoryError,
    > {
        let mut state = self.state.write().await;
        let mut grant = state
            .resource_grants
            .get(&write.resource_grant_id)
            .filter(|grant| grant.organization_id == write.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let target = state
            .memberships
            .get(&grant.membership_id)
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage("Resource Grant membership is missing".into())
            })?;
        let actor = actor_membership(&state, write.organization_id, write.actor_principal_id);
        authorize_management(
            actor.as_ref(),
            write.actor_is_platform_admin,
            target.role,
            None,
        )?;
        if let Some(existing) = replay(&state, &write.idempotency)? {
            return Ok(existing);
        }
        if grant.aggregate_version != write.expected_version {
            return Err(RepositoryError::Conflict(
                "Resource Grant changed before revocation".into(),
            ));
        }
        let changed = grant.revoke(write.revoked_at);
        state.resource_grants.insert(grant.id, grant.clone());
        remember(&mut state, write.idempotency, &grant)?;
        if changed {
            state.outbox.push(
                ResourceGrantChanged::revoked(&grant, write.request_id)
                    .map_err(|error| RepositoryError::Storage(error.to_string()))?,
            );
        }
        Ok(IdempotentWrite {
            value: grant,
            replayed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::{Membership, ResourceGrant};
    use crate::modules::identity::domain::events::ResourceGrantChanged;
    use crate::modules::identity::domain::value_objects::{MembershipRole, ResourceGrantScope};
    use crate::modules::shared_kernel::domain::{IdempotencyRequest, PrincipalId, ProjectId};
    use chrono::Utc;
    use uuid::Uuid;

    async fn repository_with_memberships(
        actor_role: MembershipRole,
        target_role: MembershipRole,
    ) -> (
        InMemoryIdentityRepository,
        OrganizationId,
        PrincipalId,
        MembershipId,
    ) {
        let repository = InMemoryIdentityRepository::new();
        let organization_id = OrganizationId::new();
        let actor_principal_id = PrincipalId::new();
        let target_principal_id = PrincipalId::new();
        let actor = Membership::create(
            MembershipId::new(),
            organization_id,
            actor_principal_id,
            actor_role,
            Utc::now(),
        );
        let target = Membership::create(
            MembershipId::new(),
            organization_id,
            target_principal_id,
            target_role,
            Utc::now(),
        );
        let target_id = target.id;
        let mut state = repository.state.write().await;
        state
            .membership_subjects
            .insert((organization_id, actor_principal_id), actor.id);
        state.memberships.insert(actor.id, actor);
        state.memberships.insert(target.id, target);
        drop(state);
        (repository, organization_id, actor_principal_id, target_id)
    }

    fn create_write(
        grant: ResourceGrant,
        actor_principal_id: PrincipalId,
        key: &str,
    ) -> CreateResourceGrantWrite {
        let request_id = Uuid::now_v7();
        CreateResourceGrantWrite {
            event: ResourceGrantChanged::created(&grant, request_id).expect("event"),
            grant,
            actor_principal_id,
            actor_is_platform_admin: false,
            request_id,
            idempotency: IdempotencyRequest::new("resource-grants", key, b"canonical")
                .expect("idempotency"),
        }
    }

    #[tokio::test]
    async fn lifecycle_is_authorized_idempotent_and_terminal() {
        let (repository, organization_id, actor_principal_id, membership_id) =
            repository_with_memberships(MembershipRole::Admin, MembershipRole::Restricted).await;
        let grant = ResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            membership_id,
            ResourceGrantScope::Project {
                project_id: ProjectId::new(),
            },
            Utc::now(),
        );
        let write = create_write(grant, actor_principal_id, "create-1");

        let created = repository
            .create_resource_grant(write.clone())
            .await
            .expect("create");
        assert!(!created.replayed);
        assert!(created.value.is_active());
        let replayed = repository
            .create_resource_grant(write)
            .await
            .expect("replay");
        assert!(replayed.replayed);
        assert_eq!(replayed.value, created.value);

        let revoked = repository
            .revoke_resource_grant(RevokeResourceGrantWrite {
                organization_id,
                resource_grant_id: created.value.id,
                expected_version: 1,
                actor_principal_id,
                actor_is_platform_admin: false,
                revoked_at: Utc::now(),
                request_id: Uuid::now_v7(),
                idempotency: IdempotencyRequest::new(
                    "resource-grant-revocation",
                    "revoke-1",
                    b"canonical",
                )
                .expect("idempotency"),
            })
            .await
            .expect("revoke");
        assert!(!revoked.value.is_active());
        assert_eq!(revoked.value.aggregate_version, 2);
        assert!(repository
            .list_active_resource_grants_for_membership(organization_id, membership_id)
            .await
            .expect("active grants")
            .is_empty());
        assert_eq!(repository.outbox_events().await.len(), 2);
    }

    #[tokio::test]
    async fn non_administrative_membership_cannot_manage_grants() {
        let (repository, organization_id, actor_principal_id, membership_id) =
            repository_with_memberships(MembershipRole::Member, MembershipRole::Restricted).await;
        let grant = ResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            membership_id,
            ResourceGrantScope::Project {
                project_id: ProjectId::new(),
            },
            Utc::now(),
        );
        let error = repository
            .create_resource_grant(create_write(grant, actor_principal_id, "forbidden"))
            .await
            .expect_err("member must not manage grants");
        assert!(matches!(error, RepositoryError::Forbidden(_)));
    }

    #[tokio::test]
    async fn active_exact_scope_is_unique_but_can_be_regranted_after_revocation() {
        let (repository, organization_id, actor_principal_id, membership_id) =
            repository_with_memberships(MembershipRole::Admin, MembershipRole::Restricted).await;
        let scope = ResourceGrantScope::Project {
            project_id: ProjectId::new(),
        };
        let first = ResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            membership_id,
            scope,
            Utc::now(),
        );
        repository
            .create_resource_grant(create_write(first.clone(), actor_principal_id, "first"))
            .await
            .expect("first grant");
        let duplicate = ResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            membership_id,
            scope,
            Utc::now(),
        );
        assert!(matches!(
            repository
                .create_resource_grant(create_write(
                    duplicate.clone(),
                    actor_principal_id,
                    "duplicate"
                ))
                .await,
            Err(RepositoryError::Conflict(_))
        ));
        repository
            .revoke_resource_grant(RevokeResourceGrantWrite {
                organization_id,
                resource_grant_id: first.id,
                expected_version: 1,
                actor_principal_id,
                actor_is_platform_admin: false,
                revoked_at: Utc::now(),
                request_id: Uuid::now_v7(),
                idempotency: IdempotencyRequest::new("revocation", "first", b"canonical")
                    .expect("idempotency"),
            })
            .await
            .expect("revoke first");
        repository
            .create_resource_grant(create_write(duplicate, actor_principal_id, "replacement"))
            .await
            .expect("replacement grant");
    }
}
