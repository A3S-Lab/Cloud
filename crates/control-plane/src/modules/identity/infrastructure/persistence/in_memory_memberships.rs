use super::in_memory::{remember, replay, InMemoryIdentityRepository, State};
use crate::modules::identity::application::{
    ActiveHumanMembershipScope, IActiveHumanMembershipQueryPort,
};
use crate::modules::identity::domain::entities::{IdentityPrincipalKind, Membership};
use crate::modules::identity::domain::events::MembershipChanged;
use crate::modules::identity::domain::repositories::{
    ChangeMembershipRoleWrite, CreateMembershipWrite, IMembershipRepository, MembershipRecord,
    RevokeMembershipWrite,
};
use crate::modules::identity::domain::services::MembershipAdministration;
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, MembershipId, OrganizationId, PrincipalId, RepositoryError,
};
use async_trait::async_trait;

fn record(state: &State, membership: &Membership) -> Result<MembershipRecord, RepositoryError> {
    let principal = state
        .principals
        .get(&membership.principal_id)
        .cloned()
        .ok_or_else(|| RepositoryError::Storage("membership principal is missing".into()))?;
    Ok(MembershipRecord {
        principal,
        membership: membership.clone(),
    })
}

pub(super) fn actor_membership(
    state: &State,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
) -> Option<Membership> {
    state
        .membership_subjects
        .get(&(organization_id, principal_id))
        .and_then(|id| state.memberships.get(id))
        .filter(|membership| membership.is_active())
        .cloned()
}

fn require_another_owner(state: &State, membership: &Membership) -> Result<(), RepositoryError> {
    if membership.role != MembershipRole::Owner || !membership.is_active() {
        return Ok(());
    }
    let owners = state
        .memberships
        .values()
        .filter(|candidate| {
            candidate.organization_id == membership.organization_id
                && candidate.role == MembershipRole::Owner
                && candidate.is_active()
        })
        .count();
    if owners <= 1 {
        return Err(RepositoryError::Conflict(
            "organization must retain at least one active owner".into(),
        ));
    }
    Ok(())
}

#[async_trait]
impl IActiveHumanMembershipQueryPort for InMemoryIdentityRepository {
    async fn active_human_membership_exists(
        &self,
        scope: ActiveHumanMembershipScope,
    ) -> Result<bool, RepositoryError> {
        let state = self.state.read().await;
        let membership_exists =
            actor_membership(&state, scope.organization_id(), scope.principal_id()).is_some();
        let active_human = state
            .principals
            .get(&scope.principal_id())
            .is_some_and(|principal| {
                principal.kind == IdentityPrincipalKind::Human && principal.is_active()
            });
        Ok(membership_exists && active_human)
    }
}

#[async_trait]
impl IMembershipRepository for InMemoryIdentityRepository {
    async fn create_membership(
        &self,
        write: CreateMembershipWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        let actor = actor_membership(
            &state,
            write.membership.organization_id,
            write.actor_principal_id,
        );
        MembershipAdministration::authorize(
            actor.as_ref(),
            write.membership.organization_id,
            write.membership.role,
            None,
        )
        .map_err(RepositoryError::Forbidden)?;
        if let Some(existing) = replay(&state, &write.idempotency)? {
            return Ok(existing);
        }
        if !state
            .organizations
            .contains_key(&write.membership.organization_id)
        {
            return Err(RepositoryError::NotFound);
        }
        if write.principal.id != write.membership.principal_id
            || state.principals.contains_key(&write.principal.id)
            || state.memberships.contains_key(&write.membership.id)
            || state.membership_subjects.contains_key(&(
                write.membership.organization_id,
                write.membership.principal_id,
            ))
        {
            return Err(RepositoryError::Conflict(
                "identity principal or membership already exists".into(),
            ));
        }
        let record = MembershipRecord {
            principal: write.principal,
            membership: write.membership,
        };
        state
            .principals
            .insert(record.principal.id, record.principal.clone());
        state.membership_subjects.insert(
            (
                record.membership.organization_id,
                record.membership.principal_id,
            ),
            record.membership.id,
        );
        state
            .memberships
            .insert(record.membership.id, record.membership.clone());
        remember(&mut state, write.idempotency, &record)?;
        state.outbox.extend(write.events);
        Ok(IdempotentWrite {
            value: record,
            replayed: false,
        })
    }

    async fn find_membership(
        &self,
        organization_id: OrganizationId,
        membership_id: MembershipId,
    ) -> Result<Option<MembershipRecord>, RepositoryError> {
        let state = self.state.read().await;
        state
            .memberships
            .get(&membership_id)
            .filter(|membership| membership.organization_id == organization_id)
            .map(|membership| record(&state, membership))
            .transpose()
    }

    async fn list_memberships(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<MembershipRecord>, RepositoryError> {
        let state = self.state.read().await;
        let mut memberships = state
            .memberships
            .values()
            .filter(|membership| membership.organization_id == organization_id)
            .map(|membership| record(&state, membership))
            .collect::<Result<Vec<_>, _>>()?;
        memberships.sort_by(|left, right| {
            left.membership
                .created_at
                .cmp(&right.membership.created_at)
                .then_with(|| {
                    left.membership
                        .id
                        .as_uuid()
                        .cmp(&right.membership.id.as_uuid())
                })
        });
        Ok(memberships)
    }

    async fn find_active_membership_by_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<Membership>, RepositoryError> {
        Ok(actor_membership(
            &*self.state.read().await,
            organization_id,
            principal_id,
        ))
    }

    async fn change_membership_role(
        &self,
        write: ChangeMembershipRoleWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        let actor = actor_membership(&state, write.organization_id, write.actor_principal_id);
        let mut membership = state
            .memberships
            .get(&write.membership_id)
            .filter(|membership| membership.organization_id == write.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        MembershipAdministration::authorize(
            actor.as_ref(),
            write.organization_id,
            membership.role,
            Some(write.role),
        )
        .map_err(RepositoryError::Forbidden)?;
        if let Some(existing) = replay(&state, &write.idempotency)? {
            return Ok(existing);
        }
        if membership.aggregate_version != write.expected_version {
            return Err(RepositoryError::Conflict(
                "membership changed before its role update".into(),
            ));
        }
        if membership.role == MembershipRole::Owner && write.role != MembershipRole::Owner {
            require_another_owner(&state, &membership)?;
        }
        let changed = membership.change_role(write.role, write.changed_at);
        let value = record(&state, &membership)?;
        state.memberships.insert(membership.id, membership.clone());
        remember(&mut state, write.idempotency, &value)?;
        if changed {
            state.outbox.push(
                MembershipChanged::role_changed(&membership, write.request_id)
                    .map_err(|error| RepositoryError::Storage(error.to_string()))?,
            );
        }
        Ok(IdempotentWrite {
            value,
            replayed: false,
        })
    }

    async fn revoke_membership(
        &self,
        write: RevokeMembershipWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        let actor = actor_membership(&state, write.organization_id, write.actor_principal_id);
        let mut membership = state
            .memberships
            .get(&write.membership_id)
            .filter(|membership| membership.organization_id == write.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        MembershipAdministration::authorize(
            actor.as_ref(),
            write.organization_id,
            membership.role,
            None,
        )
        .map_err(RepositoryError::Forbidden)?;
        if let Some(existing) = replay(&state, &write.idempotency)? {
            return Ok(existing);
        }
        if membership.aggregate_version != write.expected_version {
            return Err(RepositoryError::Conflict(
                "membership changed before revocation".into(),
            ));
        }
        require_another_owner(&state, &membership)?;
        let changed = membership.revoke(write.revoked_at);
        let value = record(&state, &membership)?;
        state.memberships.insert(membership.id, membership.clone());
        remember(&mut state, write.idempotency, &value)?;
        if changed {
            state.outbox.push(
                MembershipChanged::revoked(&membership, write.request_id)
                    .map_err(|error| RepositoryError::Storage(error.to_string()))?,
            );
        }
        Ok(IdempotentWrite {
            value,
            replayed: false,
        })
    }
}

#[cfg(test)]
mod active_human_membership_tests {
    use super::*;
    use crate::modules::identity::domain::entities::IdentityPrincipal;
    use crate::modules::shared_kernel::domain::{MembershipId, ResourceName};
    use chrono::Utc;

    #[tokio::test]
    async fn owner_query_requires_the_same_active_human_and_membership_scope() {
        let repository = InMemoryIdentityRepository::new();
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let membership = Membership::create(
            MembershipId::new(),
            organization_id,
            principal_id,
            MembershipRole::Member,
            Utc::now(),
        );
        {
            let mut state = repository.state.write().await;
            state.principals.insert(
                principal_id,
                IdentityPrincipal::create(
                    principal_id,
                    IdentityPrincipalKind::Human,
                    ResourceName::parse("Plugin operator").expect("principal name"),
                    Utc::now(),
                ),
            );
            state
                .membership_subjects
                .insert((organization_id, principal_id), membership.id);
            state.memberships.insert(membership.id, membership);
        }

        assert!(repository
            .active_human_membership_exists(
                ActiveHumanMembershipScope::new(organization_id, principal_id).expect("scope"),
            )
            .await
            .expect("membership query"));
        assert!(!repository
            .active_human_membership_exists(
                ActiveHumanMembershipScope::new(OrganizationId::new(), principal_id)
                    .expect("foreign scope"),
            )
            .await
            .expect("foreign membership query"));

        repository
            .state
            .write()
            .await
            .principals
            .get_mut(&principal_id)
            .expect("principal")
            .kind = IdentityPrincipalKind::Service;
        assert!(!repository
            .active_human_membership_exists(
                ActiveHumanMembershipScope::new(organization_id, principal_id).expect("scope"),
            )
            .await
            .expect("service membership query"));
    }
}
