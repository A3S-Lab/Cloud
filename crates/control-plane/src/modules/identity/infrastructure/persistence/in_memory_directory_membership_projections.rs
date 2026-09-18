use super::in_memory::{remember, replay, InMemoryIdentityRepository};
use super::in_memory_memberships::actor_membership;
use crate::modules::identity::domain::entities::DirectoryMembershipProjectionBinding;
use crate::modules::identity::domain::repositories::{
    ClearDirectoryMembershipProjectionWrite, IDirectoryMembershipProjectionRepository,
    ReplaceDirectoryMembershipProjectionWrite,
    MAX_DIRECTORY_MEMBERSHIP_PROJECTION_PRINCIPALS_PER_SUBJECT,
};
use crate::modules::identity::domain::services::MembershipAdministration;
use crate::modules::identity::domain::value_objects::{
    DirectoryGrantSubjectRef, MembershipRole,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, OrganizationId, PrincipalId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
impl IDirectoryMembershipProjectionRepository for InMemoryIdentityRepository {
    async fn replace_bindings(
        &self,
        write: ReplaceDirectoryMembershipProjectionWrite,
    ) -> Result<IdempotentWrite<Vec<DirectoryMembershipProjectionBinding>>, RepositoryError> {
        let mut state = self.state.write().await;
        if !state.organizations.contains_key(&write.organization_id) {
            return Err(RepositoryError::NotFound);
        }
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
        if write.bindings.len()
            > usize::from(MAX_DIRECTORY_MEMBERSHIP_PROJECTION_PRINCIPALS_PER_SUBJECT)
        {
            return Err(RepositoryError::Conflict(format!(
                "subject cannot bind more than {MAX_DIRECTORY_MEMBERSHIP_PROJECTION_PRINCIPALS_PER_SUBJECT} principals"
            )));
        }
        for binding in &write.bindings {
            if binding.organization_id != write.organization_id
                || binding.subject != write.subject
            {
                return Err(RepositoryError::Conflict(
                    "directory membership projection binding does not match replace subject".into(),
                ));
            }
            let principal_active = state
                .principals
                .get(&binding.principal_id)
                .is_some_and(|principal| principal.is_active());
            let member_active = state
                .membership_subjects
                .get(&(write.organization_id, binding.principal_id))
                .and_then(|id| state.memberships.get(id))
                .is_some_and(|membership| membership.is_active());
            if !principal_active || !member_active {
                return Err(RepositoryError::Forbidden(
                    "directory membership projection principal is not an active organization member"
                        .into(),
                ));
            }
        }
        let keys_to_remove = state
            .directory_membership_projections
            .keys()
            .filter(|(organization_id, subject, _)| {
                *organization_id == write.organization_id && *subject == write.subject
            })
            .cloned()
            .collect::<Vec<_>>();
        for key in keys_to_remove {
            state.directory_membership_projections.remove(&key);
        }
        let mut bindings = write.bindings;
        bindings.sort_by(|left, right| left.principal_id.cmp(&right.principal_id));
        bindings.dedup_by(|left, right| left.principal_id == right.principal_id);
        for binding in &bindings {
            state.directory_membership_projections.insert(
                (
                    binding.organization_id,
                    binding.subject.clone(),
                    binding.principal_id,
                ),
                binding.clone(),
            );
        }
        remember(&mut state, write.idempotency, &bindings)?;
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: bindings,
            replayed: false,
        })
    }

    async fn list_bindings_for_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Vec<DirectoryGrantSubjectRef>, RepositoryError> {
        Ok(self
            .list_projection_bindings_for_principal(organization_id, principal_id)
            .await?
            .into_iter()
            .map(|binding| binding.subject)
            .collect())
    }

    async fn list_bindings_for_subject(
        &self,
        organization_id: OrganizationId,
        subject: &DirectoryGrantSubjectRef,
    ) -> Result<Vec<PrincipalId>, RepositoryError> {
        Ok(self
            .list_projection_bindings_for_subject(organization_id, subject)
            .await?
            .into_iter()
            .map(|binding| binding.principal_id)
            .collect())
    }

    async fn list_projection_bindings_for_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Vec<DirectoryMembershipProjectionBinding>, RepositoryError> {
        let state = self.state.read().await;
        let mut bindings = state
            .directory_membership_projections
            .values()
            .filter(|binding| {
                binding.organization_id == organization_id
                    && binding.principal_id == principal_id
            })
            .cloned()
            .collect::<Vec<_>>();
        bindings.sort_by(|left, right| left.subject.cmp(&right.subject));
        Ok(bindings)
    }

    async fn list_projection_bindings_for_subject(
        &self,
        organization_id: OrganizationId,
        subject: &DirectoryGrantSubjectRef,
    ) -> Result<Vec<DirectoryMembershipProjectionBinding>, RepositoryError> {
        let state = self.state.read().await;
        let mut bindings = state
            .directory_membership_projections
            .values()
            .filter(|binding| {
                binding.organization_id == organization_id && binding.subject == *subject
            })
            .cloned()
            .collect::<Vec<_>>();
        bindings.sort_by_key(|binding| binding.principal_id);
        Ok(bindings)
    }

    async fn clear_bindings_for_subject(
        &self,
        write: ClearDirectoryMembershipProjectionWrite,
    ) -> Result<IdempotentWrite<()>, RepositoryError> {
        let mut state = self.state.write().await;
        if !state.organizations.contains_key(&write.organization_id) {
            return Err(RepositoryError::NotFound);
        }
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
        let keys_to_remove = state
            .directory_membership_projections
            .keys()
            .filter(|(organization_id, subject, _)| {
                *organization_id == write.organization_id && *subject == write.subject
            })
            .cloned()
            .collect::<Vec<_>>();
        for key in keys_to_remove {
            state.directory_membership_projections.remove(&key);
        }
        remember(&mut state, write.idempotency, &())?;
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: (),
            replayed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::{
        IdentityPrincipal, IdentityPrincipalKind, Membership, Organization,
    };
    use crate::modules::identity::domain::events::DirectoryMembershipProjectionChanged;
    use crate::modules::identity::domain::value_objects::{
        DirectoryGrantSubjectKind, OidcIssuer, OrganizationName,
    };
    use crate::modules::shared_kernel::domain::{IdempotencyRequest, ResourceName};
    use chrono::Utc;
    use uuid::Uuid;

    async fn repository_with_admin_and_member() -> (
        InMemoryIdentityRepository,
        OrganizationId,
        PrincipalId,
        PrincipalId,
    ) {
        let repository = InMemoryIdentityRepository::new();
        let organization_id = OrganizationId::new();
        let actor_principal_id = PrincipalId::new();
        let member_principal_id = PrincipalId::new();
        let organization = Organization::create(
            organization_id,
            OrganizationName::parse("directory-membership").expect("name"),
            Utc::now(),
        );
        let actor = Membership::create(
            crate::modules::shared_kernel::domain::MembershipId::new(),
            organization_id,
            actor_principal_id,
            MembershipRole::Admin,
            Utc::now(),
        );
        let member = Membership::create(
            crate::modules::shared_kernel::domain::MembershipId::new(),
            organization_id,
            member_principal_id,
            MembershipRole::Restricted,
            Utc::now(),
        );
        let principal = IdentityPrincipal::create(
            member_principal_id,
            IdentityPrincipalKind::Human,
            ResourceName::parse("restricted member").expect("name"),
            Utc::now(),
        );
        let mut state = repository.state.write().await;
        state.organizations.insert(organization_id, organization);
        state
            .membership_subjects
            .insert((organization_id, actor_principal_id), actor.id);
        state.memberships.insert(actor.id, actor);
        state
            .membership_subjects
            .insert((organization_id, member_principal_id), member.id);
        state.memberships.insert(member.id, member);
        state.principals.insert(member_principal_id, principal);
        drop(state);
        (
            repository,
            organization_id,
            actor_principal_id,
            member_principal_id,
        )
    }

    fn subject() -> DirectoryGrantSubjectRef {
        DirectoryGrantSubjectRef::new(
            DirectoryGrantSubjectKind::Department,
            OidcIssuer::parse("https://kense.example/directory").expect("issuer"),
            Uuid::nil(),
        )
    }

    #[tokio::test]
    async fn replace_lists_and_clears_projection_bindings() {
        let (repository, organization_id, actor_principal_id, member_principal_id) =
            repository_with_admin_and_member().await;
        let subject = subject();
        let now = Utc::now();
        let request_id = Uuid::now_v7();
        let replaced = repository
            .replace_bindings(ReplaceDirectoryMembershipProjectionWrite {
                organization_id,
                subject: subject.clone(),
                bindings: vec![DirectoryMembershipProjectionBinding::new(
                    organization_id,
                    subject.clone(),
                    member_principal_id,
                    now,
                )],
                actor_principal_id,
                request_id,
                idempotency: IdempotencyRequest::new(
                    "directory-membership-projections",
                    "replace-1",
                    b"canonical",
                )
                .expect("idempotency"),
                event: DirectoryMembershipProjectionChanged::replaced(
                    organization_id,
                    &subject,
                    &[member_principal_id],
                    now,
                    request_id,
                )
                .expect("event"),
            })
            .await
            .expect("replace");
        assert!(!replaced.replayed);
        assert_eq!(replaced.value.len(), 1);

        let for_subject = repository
            .list_bindings_for_subject(organization_id, &subject)
            .await
            .expect("list subject");
        assert_eq!(for_subject, vec![member_principal_id]);
        let for_principal = repository
            .list_bindings_for_principal(organization_id, member_principal_id)
            .await
            .expect("list principal");
        assert_eq!(for_principal, vec![subject.clone()]);

        let cleared = repository
            .clear_bindings_for_subject(ClearDirectoryMembershipProjectionWrite {
                organization_id,
                subject: subject.clone(),
                actor_principal_id,
                request_id: Uuid::now_v7(),
                idempotency: IdempotencyRequest::new(
                    "directory-membership-projections",
                    "clear-1",
                    b"canonical",
                )
                .expect("idempotency"),
                event: DirectoryMembershipProjectionChanged::cleared(
                    organization_id,
                    &subject,
                    Utc::now(),
                    Uuid::now_v7(),
                )
                .expect("event"),
            })
            .await
            .expect("clear");
        assert!(!cleared.replayed);
        assert!(repository
            .list_bindings_for_subject(organization_id, &subject)
            .await
            .expect("empty")
            .is_empty());
    }
}
