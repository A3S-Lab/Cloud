//! Projection fact: Principal membership in an opaque DirectoryProjection subject.
//!
//! Cloud does not own partner HR directory trees; this binding is the sync surface
//! partners replace so auth-time expansion can resolve directory Resource Grants.

use crate::modules::identity::domain::value_objects::DirectoryGrantSubjectRef;
use crate::modules::shared_kernel::domain::{canonical_timestamp, OrganizationId, PrincipalId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DirectoryMembershipProjectionBinding {
    pub organization_id: OrganizationId,
    pub subject: DirectoryGrantSubjectRef,
    pub principal_id: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl DirectoryMembershipProjectionBinding {
    pub fn new(
        organization_id: OrganizationId,
        subject: DirectoryGrantSubjectRef,
        principal_id: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            organization_id,
            subject,
            principal_id,
            created_at: canonical_timestamp(created_at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::{
        DirectoryGrantSubjectKind, OidcIssuer,
    };
    use uuid::Uuid;

    #[test]
    fn binding_is_a_projection_fact_not_directory_ownership() {
        let subject = DirectoryGrantSubjectRef::new(
            DirectoryGrantSubjectKind::Group,
            OidcIssuer::parse("https://kense.example/directory").expect("issuer"),
            Uuid::nil(),
        );
        let binding = DirectoryMembershipProjectionBinding::new(
            OrganizationId::new(),
            subject.clone(),
            PrincipalId::new(),
            Utc::now(),
        );
        assert_eq!(binding.subject, subject);
    }
}
