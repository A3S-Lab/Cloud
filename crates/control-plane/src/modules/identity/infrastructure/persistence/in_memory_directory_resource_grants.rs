use super::in_memory::{remember, replay, InMemoryIdentityRepository};
use super::in_memory_memberships::actor_membership;
use crate::modules::identity::domain::events::DirectoryResourceGrantChanged;
use crate::modules::identity::domain::repositories::{
    CreateDirectoryResourceGrantWrite, IDirectoryResourceGrantRepository,
    RevokeDirectoryResourceGrantWrite, MAX_ACTIVE_DIRECTORY_RESOURCE_GRANTS_PER_ORG,
};
use crate::modules::identity::domain::services::MembershipAdministration;
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, RepositoryError, ResourceGrantId,
};
use async_trait::async_trait;

#[async_trait]
impl IDirectoryResourceGrantRepository for InMemoryIdentityRepository {
    async fn create_directory_resource_grant(
        &self,
        write: CreateDirectoryResourceGrantWrite,
    ) -> Result<
        IdempotentWrite<crate::modules::identity::domain::entities::DirectoryResourceGrant>,
        RepositoryError,
    > {
        let mut state = self.state.write().await;
        if !state
            .organizations
            .contains_key(&write.grant.organization_id)
        {
            return Err(RepositoryError::NotFound);
        }
        let actor = actor_membership(
            &state,
            write.grant.organization_id,
            write.actor_principal_id,
        );
        MembershipAdministration::authorize(
            actor.as_ref(),
            write.grant.organization_id,
            MembershipRole::Restricted,
            None,
        )
        .map_err(RepositoryError::Forbidden)?;
        if let Some(existing) = replay(&state, &write.idempotency)? {
            return Ok(existing);
        }
        if !write.grant.is_active()
            || write.grant.aggregate_version != 1
            || state
                .directory_resource_grants
                .contains_key(&write.grant.id)
        {
            return Err(RepositoryError::Conflict(
                "Directory Resource Grant already exists or is not new".into(),
            ));
        }
        let active_grants = state
            .directory_resource_grants
            .values()
            .filter(|grant| {
                grant.organization_id == write.grant.organization_id && grant.is_active()
            })
            .collect::<Vec<_>>();
        if active_grants.len() >= usize::from(MAX_ACTIVE_DIRECTORY_RESOURCE_GRANTS_PER_ORG) {
            return Err(RepositoryError::Conflict(format!(
                "organization cannot have more than {MAX_ACTIVE_DIRECTORY_RESOURCE_GRANTS_PER_ORG} active Directory Resource Grants"
            )));
        }
        if active_grants.iter().any(|grant| {
            grant.subject == write.grant.subject && grant.scope == write.grant.scope
        }) {
            return Err(RepositoryError::Conflict(
                "an active Directory Resource Grant already covers this exact subject and scope"
                    .into(),
            ));
        }
        let grant = write.grant;
        state
            .directory_resource_grants
            .insert(grant.id, grant.clone());
        remember(&mut state, write.idempotency, &grant)?;
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: grant,
            replayed: false,
        })
    }

    async fn find_directory_resource_grant(
        &self,
        organization_id: OrganizationId,
        resource_grant_id: ResourceGrantId,
    ) -> Result<
        Option<crate::modules::identity::domain::entities::DirectoryResourceGrant>,
        RepositoryError,
    > {
        Ok(self
            .state
            .read()
            .await
            .directory_resource_grants
            .get(&resource_grant_id)
            .filter(|grant| grant.organization_id == organization_id)
            .cloned())
    }

    async fn list_directory_resource_grants_by_organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<
        Vec<crate::modules::identity::domain::entities::DirectoryResourceGrant>,
        RepositoryError,
    > {
        let state = self.state.read().await;
        let mut grants = state
            .directory_resource_grants
            .values()
            .filter(|grant| grant.organization_id == organization_id)
            .cloned()
            .collect::<Vec<_>>();
        grants.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.as_uuid().cmp(&right.id.as_uuid()))
        });
        Ok(grants)
    }

    async fn list_active_directory_resource_grants_for_subjects(
        &self,
        organization_id: OrganizationId,
        subjects: &[crate::modules::identity::domain::value_objects::DirectoryGrantSubjectRef],
    ) -> Result<
        Vec<crate::modules::identity::domain::entities::DirectoryResourceGrant>,
        RepositoryError,
    > {
        if subjects.is_empty() {
            return Ok(Vec::new());
        }
        let subject_set = subjects.iter().cloned().collect::<std::collections::BTreeSet<_>>();
        let state = self.state.read().await;
        let mut grants = state
            .directory_resource_grants
            .values()
            .filter(|grant| {
                grant.organization_id == organization_id
                    && grant.is_active()
                    && subject_set.contains(&grant.subject)
            })
            .cloned()
            .collect::<Vec<_>>();
        grants.sort_by_key(|grant| grant.id);
        Ok(grants)
    }

    async fn revoke_directory_resource_grant(
        &self,
        write: RevokeDirectoryResourceGrantWrite,
    ) -> Result<
        IdempotentWrite<crate::modules::identity::domain::entities::DirectoryResourceGrant>,
        RepositoryError,
    > {
        let mut state = self.state.write().await;
        let mut grant = state
            .directory_resource_grants
            .get(&write.resource_grant_id)
            .filter(|grant| grant.organization_id == write.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let actor = actor_membership(&state, write.organization_id, write.actor_principal_id);
        MembershipAdministration::authorize(
            actor.as_ref(),
            write.organization_id,
            MembershipRole::Restricted,
            None,
        )
        .map_err(RepositoryError::Forbidden)?;
        if let Some(existing) = replay(&state, &write.idempotency)? {
            return Ok(existing);
        }
        if grant.aggregate_version != write.expected_version {
            return Err(RepositoryError::Conflict(
                "Directory Resource Grant changed before revocation".into(),
            ));
        }
        let changed = grant.revoke(write.revoked_at);
        state
            .directory_resource_grants
            .insert(grant.id, grant.clone());
        remember(&mut state, write.idempotency, &grant)?;
        if changed {
            state.outbox.push(
                DirectoryResourceGrantChanged::revoked(&grant, write.request_id)
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
    use crate::modules::identity::domain::entities::{
        DirectoryResourceGrant, Membership, Organization,
    };
    use crate::modules::identity::domain::events::DirectoryResourceGrantChanged;
    use crate::modules::identity::domain::value_objects::{
        DirectoryGrantSubjectKind, DirectoryGrantSubjectRef, MembershipRole, OidcIssuer,
        OrganizationName, ResourceGrantScope,
    };
    use crate::modules::shared_kernel::domain::{IdempotencyRequest, PrincipalId, ProjectId};
    use chrono::Utc;
    use uuid::Uuid;

    async fn repository_with_admin() -> (
        InMemoryIdentityRepository,
        OrganizationId,
        PrincipalId,
    ) {
        let repository = InMemoryIdentityRepository::new();
        let organization_id = OrganizationId::new();
        let actor_principal_id = PrincipalId::new();
        let organization = Organization::create(
            organization_id,
            OrganizationName::parse("directory-grants").expect("name"),
            Utc::now(),
        );
        let actor = Membership::create(
            crate::modules::shared_kernel::domain::MembershipId::new(),
            organization_id,
            actor_principal_id,
            MembershipRole::Admin,
            Utc::now(),
        );
        let mut state = repository.state.write().await;
        state.organizations.insert(organization_id, organization);
        state
            .membership_subjects
            .insert((organization_id, actor_principal_id), actor.id);
        state.memberships.insert(actor.id, actor);
        drop(state);
        (repository, organization_id, actor_principal_id)
    }

    fn subject() -> DirectoryGrantSubjectRef {
        DirectoryGrantSubjectRef::new(
            DirectoryGrantSubjectKind::Department,
            OidcIssuer::parse("https://kense.example/directory").expect("issuer"),
            Uuid::nil(),
        )
    }

    fn create_write(
        grant: DirectoryResourceGrant,
        actor_principal_id: PrincipalId,
        key: &str,
    ) -> CreateDirectoryResourceGrantWrite {
        let request_id = Uuid::now_v7();
        CreateDirectoryResourceGrantWrite {
            event: DirectoryResourceGrantChanged::created(&grant, request_id).expect("event"),
            grant,
            actor_principal_id,
            request_id,
            idempotency: IdempotencyRequest::new("directory-resource-grants", key, b"canonical")
                .expect("idempotency"),
        }
    }

    #[tokio::test]
    async fn lifecycle_is_authorized_idempotent_and_terminal() {
        let (repository, organization_id, actor_principal_id) = repository_with_admin().await;
        let grant = DirectoryResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            subject(),
            ResourceGrantScope::Project {
                project_id: ProjectId::new(),
            },
            Utc::now(),
        );
        let write = create_write(grant, actor_principal_id, "create-1");

        let created = repository
            .create_directory_resource_grant(write.clone())
            .await
            .expect("create");
        assert!(!created.replayed);
        assert!(created.value.is_active());
        let replayed = repository
            .create_directory_resource_grant(write)
            .await
            .expect("replay");
        assert!(replayed.replayed);
        assert_eq!(replayed.value, created.value);

        let revoked = repository
            .revoke_directory_resource_grant(RevokeDirectoryResourceGrantWrite {
                organization_id,
                resource_grant_id: created.value.id,
                expected_version: 1,
                actor_principal_id,
                revoked_at: Utc::now(),
                request_id: Uuid::now_v7(),
                idempotency: IdempotencyRequest::new(
                    "directory-resource-grant-revocation",
                    "revoke-1",
                    b"canonical",
                )
                .expect("idempotency"),
            })
            .await
            .expect("revoke");
        assert!(!revoked.value.is_active());
        assert_eq!(revoked.value.aggregate_version, 2);
        assert_eq!(
            repository
                .list_directory_resource_grants_by_organization(organization_id)
                .await
                .expect("list")
                .len(),
            1
        );
        assert_eq!(repository.outbox_events().await.len(), 2);
    }

    #[tokio::test]
    async fn non_administrative_membership_cannot_manage_grants() {
        let repository = InMemoryIdentityRepository::new();
        let organization_id = OrganizationId::new();
        let actor_principal_id = PrincipalId::new();
        let organization = Organization::create(
            organization_id,
            OrganizationName::parse("directory-grants-member").expect("name"),
            Utc::now(),
        );
        let actor = Membership::create(
            crate::modules::shared_kernel::domain::MembershipId::new(),
            organization_id,
            actor_principal_id,
            MembershipRole::Member,
            Utc::now(),
        );
        {
            let mut state = repository.state.write().await;
            state.organizations.insert(organization_id, organization);
            state
                .membership_subjects
                .insert((organization_id, actor_principal_id), actor.id);
            state.memberships.insert(actor.id, actor);
        }
        let grant = DirectoryResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            subject(),
            ResourceGrantScope::Project {
                project_id: ProjectId::new(),
            },
            Utc::now(),
        );
        let error = repository
            .create_directory_resource_grant(create_write(grant, actor_principal_id, "forbidden"))
            .await
            .expect_err("member must not manage directory grants");
        assert!(matches!(error, RepositoryError::Forbidden(_)));
    }

    #[tokio::test]
    async fn active_exact_subject_and_scope_is_unique() {
        let (repository, organization_id, actor_principal_id) = repository_with_admin().await;
        let scope = ResourceGrantScope::Project {
            project_id: ProjectId::new(),
        };
        let first = DirectoryResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            subject(),
            scope,
            Utc::now(),
        );
        repository
            .create_directory_resource_grant(create_write(
                first.clone(),
                actor_principal_id,
                "first",
            ))
            .await
            .expect("first grant");
        let duplicate = DirectoryResourceGrant::create(
            ResourceGrantId::new(),
            organization_id,
            subject(),
            scope,
            Utc::now(),
        );
        assert!(matches!(
            repository
                .create_directory_resource_grant(create_write(
                    duplicate,
                    actor_principal_id,
                    "duplicate"
                ))
                .await,
            Err(RepositoryError::Conflict(_))
        ));
    }
}
