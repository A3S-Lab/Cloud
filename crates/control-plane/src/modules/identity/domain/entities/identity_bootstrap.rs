use super::{ApiToken, IdentityPrincipal, Membership, Organization, PlatformRbacBootstrap};
use crate::modules::identity::domain::value_objects::MembershipRole;
use serde::{Deserialize, Serialize};

/// The complete authority root created for a fresh Cloud installation.
///
/// Identity tenancy and Installation-scoped platform authorization are one
/// bootstrap invariant: neither side is useful or recoverable without the
/// other, so persistence must make the whole value visible atomically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityBootstrap {
    pub organization: Organization,
    pub principal: IdentityPrincipal,
    pub membership: Membership,
    pub api_token: ApiToken,
    pub platform_rbac: PlatformRbacBootstrap,
}

impl IdentityBootstrap {
    pub fn create(
        organization: Organization,
        principal: IdentityPrincipal,
        membership: Membership,
        api_token: ApiToken,
        platform_rbac: PlatformRbacBootstrap,
    ) -> Result<Self, String> {
        let value = Self {
            organization,
            principal,
            membership,
            api_token,
            platform_rbac,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.platform_rbac.validate()?;
        if self.organization.aggregate_version != 1
            || self.principal.aggregate_version != 1
            || !self.principal.is_active()
            || self.membership.organization_id != self.organization.id
            || self.membership.principal_id != self.principal.id
            || self.membership.role != MembershipRole::Owner
            || self.membership.aggregate_version != 1
            || !self.membership.is_active()
            || self.api_token.organization_id != self.organization.id
            || self.api_token.principal_id != self.principal.id
            || self.api_token.aggregate_version != 1
            || self.api_token.revoked_at.is_some()
            || self.platform_rbac.policy.accepted_by != self.principal.id
        {
            return Err(
                "identity bootstrap must bind one active tenant owner and platform owner authority"
                    .into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::{
        AcceptedPlatformRolePolicyRevision, IdentityPrincipalKind, PlatformRoleBinding,
    };
    use crate::modules::identity::domain::value_objects::{
        ApiTokenName, ApiTokenScope, MembershipRole, OrganizationName, PlatformRole,
        PlatformRolePolicyContract,
    };
    use crate::modules::shared_kernel::domain::{
        ApiTokenId, InstallationId, MembershipId, OrganizationId, PlatformRoleBindingId,
        PlatformRolePolicyId, PrincipalId, ResourceName,
    };
    use chrono::Utc;

    fn bootstrap() -> IdentityBootstrap {
        let now = Utc::now();
        let organization = Organization::create(
            OrganizationId::new(),
            OrganizationName::parse("bootstrap tenant").expect("organization name"),
            now,
        );
        let principal = IdentityPrincipal::create(
            PrincipalId::new(),
            IdentityPrincipalKind::Service,
            ResourceName::parse("bootstrap owner").expect("principal name"),
            now,
        );
        let membership = Membership::create(
            MembershipId::new(),
            organization.id,
            principal.id,
            MembershipRole::Owner,
            now,
        );
        let api_token = ApiToken::issue(
            ApiTokenId::new(),
            organization.id,
            principal.id,
            ApiTokenName::parse("bootstrap owner").expect("token name"),
            ApiTokenScope::bootstrap_scopes(),
            now,
            None,
        )
        .expect("API token");
        let installation_id = InstallationId::new();
        let policy = AcceptedPlatformRolePolicyRevision::accept(
            PlatformRolePolicyContract::baseline(installation_id, PlatformRolePolicyId::new())
                .expect("platform role policy"),
            1,
            principal.id,
            now,
        )
        .expect("accepted policy");
        let owner_binding = PlatformRoleBinding::create(
            PlatformRoleBindingId::new(),
            installation_id,
            principal.id,
            PlatformRole::PlatformOwner,
            &policy,
            principal.id,
            now,
        )
        .expect("owner binding");
        IdentityBootstrap::create(
            organization,
            principal,
            membership,
            api_token,
            PlatformRbacBootstrap {
                policy,
                owner_binding,
            },
        )
        .expect("Identity bootstrap")
    }

    #[test]
    fn tenant_and_platform_owners_form_one_valid_authority_root() {
        bootstrap().validate().expect("valid authority root");
    }

    #[test]
    fn platform_owner_cannot_cross_the_bootstrap_principal() {
        let mut value = bootstrap();
        value.platform_rbac.owner_binding.principal_id = PrincipalId::new();
        assert!(value.validate().is_err());
    }
}
