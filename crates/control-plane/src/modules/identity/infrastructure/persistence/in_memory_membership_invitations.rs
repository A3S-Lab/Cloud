use super::in_memory::{remember, replay, InMemoryIdentityRepository};
use super::in_memory_memberships::{actor_membership, authorize_management};
use crate::modules::identity::domain::entities::{Membership, MembershipInvitation};
use crate::modules::identity::domain::events::{MembershipChanged, MembershipInvitationChanged};
use crate::modules::identity::domain::repositories::{
    AcceptMembershipInvitationWrite, CreateMembershipInvitationWrite,
    IMembershipInvitationRepository, MembershipInvitationAcceptance, MembershipRecord,
    RevokeMembershipInvitationWrite,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, MembershipInvitationId, OrganizationId, PrincipalId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
impl IMembershipInvitationRepository for InMemoryIdentityRepository {
    async fn create_membership_invitation(
        &self,
        write: CreateMembershipInvitationWrite,
    ) -> Result<IdempotentWrite<MembershipInvitation>, RepositoryError> {
        let mut state = self.state.write().await;
        let actor = actor_membership(
            &state,
            write.invitation.organization_id,
            write.invitation.invited_by_principal_id,
        );
        authorize_management(
            actor.as_ref(),
            write.actor_is_platform_admin,
            write.invitation.role,
            None,
        )?;
        if let Some(replayed) = replay(&state, &write.idempotency)? {
            return Ok(replayed);
        }
        if !state
            .organizations
            .contains_key(&write.invitation.organization_id)
        {
            return Err(RepositoryError::NotFound);
        }
        let principal = state
            .principals
            .get(&write.invitation.principal_id)
            .filter(|principal| principal.is_active())
            .ok_or(RepositoryError::NotFound)?;
        if state
            .membership_subjects
            .contains_key(&(write.invitation.organization_id, principal.id))
        {
            return Err(RepositoryError::Conflict(
                "principal already has an organization membership".into(),
            ));
        }
        if state.membership_invitations.values().any(|invitation| {
            invitation.organization_id == write.invitation.organization_id
                && invitation.principal_id == write.invitation.principal_id
                && invitation
                    .status_at(write.invitation.created_at)
                    == crate::modules::identity::domain::entities::MembershipInvitationStatus::Pending
        }) {
            return Err(RepositoryError::Conflict(
                "principal already has a pending membership invitation".into(),
            ));
        }
        state
            .membership_invitations
            .insert(write.invitation.id, write.invitation.clone());
        remember(&mut state, write.idempotency, &write.invitation)?;
        state.outbox.push(write.event);
        Ok(IdempotentWrite {
            value: write.invitation,
            replayed: false,
        })
    }

    async fn find_membership_invitation(
        &self,
        organization_id: OrganizationId,
        invitation_id: MembershipInvitationId,
    ) -> Result<Option<MembershipInvitation>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .membership_invitations
            .get(&invitation_id)
            .filter(|invitation| invitation.organization_id == organization_id)
            .cloned())
    }

    async fn list_membership_invitations(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<MembershipInvitation>, RepositoryError> {
        let mut invitations = self
            .state
            .read()
            .await
            .membership_invitations
            .values()
            .filter(|invitation| invitation.organization_id == organization_id)
            .cloned()
            .collect::<Vec<_>>();
        invitations.sort_by_key(|invitation| (invitation.created_at, invitation.id.as_uuid()));
        Ok(invitations)
    }

    async fn list_membership_invitations_for_principal(
        &self,
        principal_id: PrincipalId,
    ) -> Result<Vec<MembershipInvitation>, RepositoryError> {
        let mut invitations = self
            .state
            .read()
            .await
            .membership_invitations
            .values()
            .filter(|invitation| invitation.principal_id == principal_id)
            .cloned()
            .collect::<Vec<_>>();
        invitations.sort_by_key(|invitation| (invitation.created_at, invitation.id.as_uuid()));
        Ok(invitations)
    }

    async fn accept_membership_invitation(
        &self,
        write: AcceptMembershipInvitationWrite,
    ) -> Result<IdempotentWrite<MembershipInvitationAcceptance>, RepositoryError> {
        let mut state = self.state.write().await;
        let mut invitation = state
            .membership_invitations
            .get(&write.invitation_id)
            .filter(|invitation| invitation.principal_id == write.actor_principal_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if let Some(replayed) = replay(&state, &write.idempotency)? {
            return Ok(replayed);
        }
        if invitation.aggregate_version != write.expected_version {
            return Err(RepositoryError::Conflict(
                "membership invitation changed before acceptance".into(),
            ));
        }
        if state
            .membership_subjects
            .contains_key(&(invitation.organization_id, invitation.principal_id))
        {
            return Err(RepositoryError::Conflict(
                "principal already has an organization membership".into(),
            ));
        }
        let principal = state
            .principals
            .get(&invitation.principal_id)
            .filter(|principal| principal.is_active())
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        invitation
            .accept(
                write.actor_principal_id,
                write.membership_id,
                write.accepted_at,
            )
            .map_err(RepositoryError::Conflict)?;
        let membership = Membership::create(
            write.membership_id,
            invitation.organization_id,
            invitation.principal_id,
            invitation.role,
            invitation.updated_at,
        );
        let record = MembershipRecord {
            principal,
            membership,
        };
        state.membership_subjects.insert(
            (invitation.organization_id, invitation.principal_id),
            record.membership.id,
        );
        state
            .memberships
            .insert(record.membership.id, record.membership.clone());
        state
            .membership_invitations
            .insert(invitation.id, invitation.clone());
        let acceptance = MembershipInvitationAcceptance {
            invitation: invitation.clone(),
            membership: record.clone(),
        };
        remember(&mut state, write.idempotency, &acceptance)?;
        state.outbox.push(
            MembershipChanged::created(&record.membership, write.request_id)
                .map_err(|error| RepositoryError::Storage(error.to_string()))?,
        );
        state.outbox.push(
            MembershipInvitationChanged::accepted(&invitation, write.request_id)
                .map_err(|error| RepositoryError::Storage(error.to_string()))?,
        );
        Ok(IdempotentWrite {
            value: acceptance,
            replayed: false,
        })
    }

    async fn revoke_membership_invitation(
        &self,
        write: RevokeMembershipInvitationWrite,
    ) -> Result<IdempotentWrite<MembershipInvitation>, RepositoryError> {
        let mut state = self.state.write().await;
        let actor = actor_membership(&state, write.organization_id, write.actor_principal_id);
        let mut invitation = state
            .membership_invitations
            .get(&write.invitation_id)
            .filter(|invitation| invitation.organization_id == write.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        authorize_management(
            actor.as_ref(),
            write.actor_is_platform_admin,
            invitation.role,
            None,
        )?;
        if let Some(replayed) = replay(&state, &write.idempotency)? {
            return Ok(replayed);
        }
        if invitation.aggregate_version != write.expected_version {
            return Err(RepositoryError::Conflict(
                "membership invitation changed before revocation".into(),
            ));
        }
        let changed = invitation.revoke(write.revoked_at);
        state
            .membership_invitations
            .insert(invitation.id, invitation.clone());
        remember(&mut state, write.idempotency, &invitation)?;
        if changed {
            state.outbox.push(
                MembershipInvitationChanged::revoked(&invitation, write.request_id)
                    .map_err(|error| RepositoryError::Storage(error.to_string()))?,
            );
        }
        Ok(IdempotentWrite {
            value: invitation,
            replayed: false,
        })
    }
}
