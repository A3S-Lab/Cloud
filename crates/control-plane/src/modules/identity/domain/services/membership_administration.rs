use crate::modules::identity::domain::entities::Membership;
use crate::modules::identity::domain::value_objects::MembershipRole;
use crate::modules::shared_kernel::domain::OrganizationId;

/// The single domain service for Organization membership administration.
///
/// Installation roles are intentionally absent: a platform role is not a
/// tenant membership and therefore cannot grant implicit Organization access.
pub struct MembershipAdministration;

impl MembershipAdministration {
    pub fn authorize(
        actor: Option<&Membership>,
        organization_id: OrganizationId,
        target_role: MembershipRole,
        replacement_role: Option<MembershipRole>,
    ) -> Result<(), String> {
        let actor = actor
            .filter(|membership| {
                membership.is_active() && membership.organization_id == organization_id
            })
            .ok_or_else(|| "actor is not an active organization member".to_owned())?;
        if !actor.role.can_manage_memberships()
            || !actor.role.can_manage_role(target_role)
            || replacement_role.is_some_and(|role| !actor.role.can_manage_role(role))
        {
            return Err("membership role does not permit this administration action".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::Membership;
    use crate::modules::shared_kernel::domain::{MembershipId, OrganizationId, PrincipalId};
    use chrono::Utc;

    fn membership(role: MembershipRole) -> Membership {
        Membership::create(
            MembershipId::new(),
            OrganizationId::new(),
            PrincipalId::new(),
            role,
            Utc::now(),
        )
    }

    #[test]
    fn owner_can_manage_every_membership_role() {
        let owner = membership(MembershipRole::Owner);
        for target in [
            MembershipRole::Owner,
            MembershipRole::Admin,
            MembershipRole::Member,
            MembershipRole::Restricted,
        ] {
            assert_eq!(
                MembershipAdministration::authorize(
                    Some(&owner),
                    owner.organization_id,
                    target,
                    Some(target),
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn administrator_cannot_manage_owner_membership() {
        let administrator = membership(MembershipRole::Admin);
        assert!(MembershipAdministration::authorize(
            Some(&administrator),
            administrator.organization_id,
            MembershipRole::Owner,
            None,
        )
        .is_err());
        assert!(MembershipAdministration::authorize(
            Some(&administrator),
            administrator.organization_id,
            MembershipRole::Member,
            Some(MembershipRole::Owner),
        )
        .is_err());
        assert_eq!(
            MembershipAdministration::authorize(
                Some(&administrator),
                administrator.organization_id,
                MembershipRole::Member,
                Some(MembershipRole::Admin),
            ),
            Ok(())
        );
    }

    #[test]
    fn absent_inactive_and_non_administrator_members_fail_closed() {
        assert!(MembershipAdministration::authorize(
            None,
            OrganizationId::new(),
            MembershipRole::Member,
            None,
        )
        .is_err());

        let mut revoked_owner = membership(MembershipRole::Owner);
        assert!(revoked_owner.revoke(Utc::now()));
        assert!(MembershipAdministration::authorize(
            Some(&revoked_owner),
            revoked_owner.organization_id,
            MembershipRole::Member,
            None,
        )
        .is_err());

        for role in [MembershipRole::Member, MembershipRole::Restricted] {
            let actor = membership(role);
            assert!(MembershipAdministration::authorize(
                Some(&actor),
                actor.organization_id,
                MembershipRole::Member,
                None,
            )
            .is_err());
        }
    }

    #[test]
    fn membership_from_another_organization_fails_closed() {
        let owner = membership(MembershipRole::Owner);
        assert!(MembershipAdministration::authorize(
            Some(&owner),
            OrganizationId::new(),
            MembershipRole::Member,
            None,
        )
        .is_err());
    }
}
