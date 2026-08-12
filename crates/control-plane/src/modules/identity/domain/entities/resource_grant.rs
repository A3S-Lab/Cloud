use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, MembershipId, OrganizationId, ResourceGrantId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceGrant {
    pub id: ResourceGrantId,
    pub organization_id: OrganizationId,
    pub membership_id: MembershipId,
    pub scope: ResourceGrantScope,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl ResourceGrant {
    pub fn create(
        id: ResourceGrantId,
        organization_id: OrganizationId,
        membership_id: MembershipId,
        scope: ResourceGrantScope,
        created_at: DateTime<Utc>,
    ) -> Self {
        let created_at = canonical_timestamp(created_at);
        Self {
            id,
            organization_id,
            membership_id,
            scope,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            revoked_at: None,
        }
    }

    pub const fn is_active(&self) -> bool {
        self.revoked_at.is_none()
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
    use crate::modules::shared_kernel::domain::ProjectId;

    #[test]
    fn revocation_is_versioned_and_terminal() {
        let now = Utc::now();
        let mut grant = ResourceGrant::create(
            ResourceGrantId::new(),
            OrganizationId::new(),
            MembershipId::new(),
            ResourceGrantScope::Project {
                project_id: ProjectId::new(),
            },
            now,
        );
        assert!(grant.is_active());
        assert!(grant.revoke(now));
        assert!(!grant.revoke(now));
        assert_eq!(grant.aggregate_version, 2);
    }
}
