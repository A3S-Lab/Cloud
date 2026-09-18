use super::ResourceGrant;
use crate::modules::identity::domain::value_objects::{
    DirectoryGrantSubjectRef, ResourceGrantScope,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, MembershipId, OrganizationId, ResourceGrantId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryResourceGrant {
    pub id: ResourceGrantId,
    pub organization_id: OrganizationId,
    pub subject: DirectoryGrantSubjectRef,
    pub scope: ResourceGrantScope,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl DirectoryResourceGrant {
    pub fn create(
        id: ResourceGrantId,
        organization_id: OrganizationId,
        subject: DirectoryGrantSubjectRef,
        scope: ResourceGrantScope,
        created_at: DateTime<Utc>,
    ) -> Self {
        let created_at = canonical_timestamp(created_at);
        Self {
            id,
            organization_id,
            subject,
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

    /// Projects this directory grant into membership Resource Grant evidence for auth evaluation.
    pub fn as_effective_membership_grant(&self, membership_id: MembershipId) -> ResourceGrant {
        ResourceGrant {
            id: self.id,
            organization_id: self.organization_id,
            membership_id,
            scope: self.scope,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            revoked_at: self.revoked_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::{
        DirectoryGrantSubjectKind, DirectoryGrantSubjectRef,
    };
    use crate::modules::shared_kernel::domain::ProjectId;
    use uuid::Uuid;

    #[test]
    fn revocation_is_versioned_and_terminal() {
        let now = Utc::now();
        let subject = DirectoryGrantSubjectRef::new(
            DirectoryGrantSubjectKind::Department,
            crate::modules::identity::domain::value_objects::OidcIssuer::parse(
                "https://kense.example/directory",
            )
            .expect("issuer"),
            Uuid::nil(),
        );
        let mut grant = DirectoryResourceGrant::create(
            ResourceGrantId::new(),
            OrganizationId::new(),
            subject,
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
