use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, MembershipId, OrganizationId, PrincipalId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    pub id: MembershipId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub role: MembershipRole,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl Membership {
    pub fn create(
        id: MembershipId,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        role: MembershipRole,
        created_at: DateTime<Utc>,
    ) -> Self {
        let created_at = canonical_timestamp(created_at);
        Self {
            id,
            organization_id,
            principal_id,
            role,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            revoked_at: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }

    pub fn change_role(&mut self, role: MembershipRole, changed_at: DateTime<Utc>) -> bool {
        if !self.is_active() || self.role == role {
            return false;
        }
        self.role = role;
        self.aggregate_version += 1;
        self.updated_at = canonical_timestamp(changed_at).max(self.updated_at);
        true
    }

    pub fn revoke(&mut self, revoked_at: DateTime<Utc>) -> bool {
        if !self.is_active() {
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

    #[test]
    fn role_change_and_revocation_are_versioned_and_terminal() {
        let now = Utc::now();
        let mut membership = Membership::create(
            MembershipId::new(),
            OrganizationId::new(),
            PrincipalId::new(),
            MembershipRole::Member,
            now,
        );
        assert!(!membership.change_role(MembershipRole::Member, now));
        assert!(membership.change_role(MembershipRole::Restricted, now));
        assert_eq!(membership.aggregate_version, 2);
        assert!(membership.revoke(now));
        assert!(!membership.revoke(now));
        assert!(!membership.change_role(MembershipRole::Admin, now));
        assert_eq!(membership.aggregate_version, 3);
    }
}
