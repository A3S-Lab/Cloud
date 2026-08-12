use super::in_memory::{remember, replay, InMemoryIdentityRepository, State};
use crate::modules::identity::domain::entities::Membership;
use crate::modules::identity::domain::events::MembershipChanged;
use crate::modules::identity::domain::repositories::{
    ChangeMembershipRoleWrite, CreateServiceMembershipWrite, IMembershipRepository,
    MembershipRecord, RevokeMembershipWrite,
};
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

pub(super) fn authorize_management(
    actor: Option<&Membership>,
    actor_is_platform_admin: bool,
    current_role: MembershipRole,
    next_role: Option<MembershipRole>,
) -> Result<(), RepositoryError> {
    if actor_is_platform_admin {
        return Ok(());
    }
    let actor = actor.ok_or_else(|| {
        RepositoryError::Forbidden("actor is not an active organization member".into())
    })?;
    if !actor.role.can_manage_memberships()
        || !actor.role.can_manage_role(current_role)
        || next_role.is_some_and(|role| !actor.role.can_manage_role(role))
    {
        return Err(RepositoryError::Forbidden(
            "membership role does not permit this administration action".into(),
        ));
    }
    Ok(())
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
impl IMembershipRepository for InMemoryIdentityRepository {
    async fn create_service_membership(
        &self,
        write: CreateServiceMembershipWrite,
    ) -> Result<IdempotentWrite<MembershipRecord>, RepositoryError> {
        let mut state = self.state.write().await;
        let actor = actor_membership(
            &state,
            write.membership.organization_id,
            write.actor_principal_id,
        );
        authorize_management(
            actor.as_ref(),
            write.actor_is_platform_admin,
            write.membership.role,
            None,
        )?;
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
        authorize_management(
            actor.as_ref(),
            write.actor_is_platform_admin,
            membership.role,
            Some(write.role),
        )?;
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
        authorize_management(
            actor.as_ref(),
            write.actor_is_platform_admin,
            membership.role,
            None,
        )?;
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
