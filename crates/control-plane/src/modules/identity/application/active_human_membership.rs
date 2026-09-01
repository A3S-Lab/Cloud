use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RepositoryError};
use async_trait::async_trait;

/// Exact Identity-owned scope used to prove that one actor is currently an
/// active human member of one organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveHumanMembershipScope {
    organization_id: OrganizationId,
    principal_id: PrincipalId,
}

impl ActiveHumanMembershipScope {
    pub fn new(organization_id: OrganizationId, principal_id: PrincipalId) -> Result<Self, String> {
        if organization_id.as_uuid().is_nil() || principal_id.as_uuid().is_nil() {
            return Err("active human membership scope requires non-nil identities".into());
        }
        Ok(Self {
            organization_id,
            principal_id,
        })
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn principal_id(self) -> PrincipalId {
        self.principal_id
    }
}

/// Narrow Identity owner query. Consumers receive only the membership fact;
/// Identity principals, memberships, roles, and persistence stay private.
#[async_trait]
pub trait IActiveHumanMembershipQueryPort: Send + Sync {
    async fn active_human_membership_exists(
        &self,
        scope: ActiveHumanMembershipScope,
    ) -> Result<bool, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_nil_owner_identities() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();

        assert!(ActiveHumanMembershipScope::new(organization_id, principal_id).is_ok());
        assert!(ActiveHumanMembershipScope::new(
            OrganizationId::from_uuid(Uuid::nil()),
            principal_id,
        )
        .is_err());
        assert!(ActiveHumanMembershipScope::new(
            organization_id,
            PrincipalId::from_uuid(Uuid::nil()),
        )
        .is_err());
    }
}
