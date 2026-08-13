use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, MembershipId, MembershipInvitationId, OrganizationId, PrincipalId,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const MAX_MEMBERSHIP_INVITATION_LIFETIME: Duration = Duration::days(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipInvitationStatus {
    Pending,
    Accepted,
    Revoked,
    Expired,
}

impl MembershipInvitationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipInvitation {
    pub id: MembershipInvitationId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub role: MembershipRole,
    pub invited_by_principal_id: PrincipalId,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accepted_membership_id: Option<MembershipId>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl MembershipInvitation {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: MembershipInvitationId,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        role: MembershipRole,
        invited_by_principal_id: PrincipalId,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        let expires_at = canonical_timestamp(expires_at);
        if expires_at <= created_at {
            return Err("membership invitation expiry must be after creation".into());
        }
        if expires_at > created_at + MAX_MEMBERSHIP_INVITATION_LIFETIME {
            return Err("membership invitation lifetime cannot exceed 30 days".into());
        }
        Ok(Self {
            id,
            organization_id,
            principal_id,
            role,
            invited_by_principal_id,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            expires_at,
            accepted_membership_id: None,
            accepted_at: None,
            revoked_at: None,
        })
    }

    pub fn status_at(&self, now: DateTime<Utc>) -> MembershipInvitationStatus {
        if self.accepted_at.is_some() {
            MembershipInvitationStatus::Accepted
        } else if self.revoked_at.is_some() {
            MembershipInvitationStatus::Revoked
        } else if canonical_timestamp(now) >= self.expires_at {
            MembershipInvitationStatus::Expired
        } else {
            MembershipInvitationStatus::Pending
        }
    }

    pub fn accept(
        &mut self,
        principal_id: PrincipalId,
        membership_id: MembershipId,
        accepted_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let accepted_at = canonical_timestamp(accepted_at);
        if principal_id != self.principal_id {
            return Err("membership invitation belongs to another principal".into());
        }
        if self.status_at(accepted_at) != MembershipInvitationStatus::Pending {
            return Err("membership invitation is not pending".into());
        }
        self.accepted_membership_id = Some(membership_id);
        self.accepted_at = Some(accepted_at.max(self.updated_at));
        self.updated_at = self.accepted_at.expect("accepted timestamp was assigned");
        self.aggregate_version += 1;
        Ok(())
    }

    pub fn revoke(&mut self, revoked_at: DateTime<Utc>) -> bool {
        if self.accepted_at.is_some() || self.revoked_at.is_some() {
            return false;
        }
        let revoked_at = canonical_timestamp(revoked_at).max(self.updated_at);
        self.revoked_at = Some(revoked_at);
        self.updated_at = revoked_at;
        self.aggregate_version += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invitation(now: DateTime<Utc>) -> MembershipInvitation {
        MembershipInvitation::create(
            MembershipInvitationId::new(),
            OrganizationId::new(),
            PrincipalId::new(),
            MembershipRole::Restricted,
            PrincipalId::new(),
            now,
            now + Duration::days(7),
        )
        .expect("invitation")
    }

    #[test]
    fn invitation_has_a_bounded_terminal_lifecycle() {
        let now = Utc::now();
        assert!(MembershipInvitation::create(
            MembershipInvitationId::new(),
            OrganizationId::new(),
            PrincipalId::new(),
            MembershipRole::Member,
            PrincipalId::new(),
            now,
            now + Duration::days(31),
        )
        .is_err());

        let mut invitation = invitation(now);
        assert_eq!(
            invitation.status_at(now),
            MembershipInvitationStatus::Pending
        );
        invitation
            .accept(
                invitation.principal_id,
                MembershipId::new(),
                now + Duration::hours(1),
            )
            .expect("accept");
        assert_eq!(
            invitation.status_at(now + Duration::days(40)),
            MembershipInvitationStatus::Accepted
        );
        assert!(!invitation.revoke(now + Duration::hours(2)));
        assert_eq!(invitation.aggregate_version, 2);
    }

    #[test]
    fn expired_or_wrong_principal_invitations_cannot_be_accepted() {
        let now = Utc::now();
        let mut invitation = invitation(now);
        assert!(invitation
            .accept(
                PrincipalId::new(),
                MembershipId::new(),
                now + Duration::hours(1),
            )
            .is_err());
        assert!(invitation
            .accept(
                invitation.principal_id,
                MembershipId::new(),
                now + Duration::days(8),
            )
            .is_err());
        assert_eq!(
            invitation.status_at(now + Duration::days(8)),
            MembershipInvitationStatus::Expired
        );
    }
}
