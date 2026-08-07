use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipRole {
    Owner,
    Admin,
    Member,
    Restricted,
}

impl MembershipRole {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "owner" => Ok(Self::Owner),
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            "restricted" => Ok(Self::Restricted),
            _ => Err("membership role must be owner, admin, member, or restricted".into()),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Restricted => "restricted",
        }
    }

    pub const fn can_manage_memberships(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }

    pub const fn can_manage_role(self, target: Self) -> bool {
        matches!(self, Self::Owner) || !matches!(target, Self::Owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_closed_and_owner_management_is_explicit() {
        for role in [
            MembershipRole::Owner,
            MembershipRole::Admin,
            MembershipRole::Member,
            MembershipRole::Restricted,
        ] {
            assert_eq!(MembershipRole::parse(role.as_str()), Ok(role));
        }
        assert!(MembershipRole::Owner.can_manage_role(MembershipRole::Owner));
        assert!(!MembershipRole::Admin.can_manage_role(MembershipRole::Owner));
        assert!(MembershipRole::Admin.can_manage_role(MembershipRole::Member));
        assert!(!MembershipRole::Restricted.can_manage_memberships());
    }
}
