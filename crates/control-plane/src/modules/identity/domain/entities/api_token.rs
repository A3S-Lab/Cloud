use crate::modules::identity::domain::value_objects::{ApiTokenName, ApiTokenScope};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ApiTokenId, OrganizationId, PrincipalId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MIN_OIDC_LOGIN_TOKEN_LIFETIME: chrono::Duration = chrono::Duration::minutes(5);
pub const MAX_OIDC_LOGIN_TOKEN_LIFETIME: chrono::Duration = chrono::Duration::hours(24);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: ApiTokenId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub name: ApiTokenName,
    pub scopes: BTreeSet<ApiTokenScope>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityBootstrap {
    pub organization: super::Organization,
    pub principal: super::IdentityPrincipal,
    pub membership: super::Membership,
    pub api_token: ApiToken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedApiToken {
    pub api_token: ApiToken,
    pub principal: super::IdentityPrincipal,
    pub membership: Option<super::Membership>,
}

impl ApiToken {
    pub fn issue(
        id: ApiTokenId,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        name: ApiTokenName,
        scopes: BTreeSet<ApiTokenScope>,
        created_at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        let expires_at = expires_at.map(canonical_timestamp);
        if scopes.is_empty() {
            return Err("API token must grant at least one scope".into());
        }
        if expires_at.is_some_and(|expires_at| expires_at <= created_at) {
            return Err("API token expiry must be later than its creation time".into());
        }
        Ok(Self {
            id,
            organization_id,
            principal_id,
            name,
            scopes,
            aggregate_version: 1,
            created_at,
            expires_at,
            revoked_at: None,
        })
    }

    pub fn is_active_at(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.expires_at.is_none_or(|expires_at| expires_at > now)
    }

    pub fn revoke(&mut self, revoked_at: DateTime<Utc>) -> bool {
        if self.revoked_at.is_some() {
            return false;
        }
        self.revoked_at = Some(canonical_timestamp(revoked_at).max(self.created_at));
        self.aggregate_version += 1;
        true
    }

    pub fn issue_oidc_login(
        id: ApiTokenId,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        name: ApiTokenName,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        let expires_at = canonical_timestamp(expires_at);
        let lifetime = expires_at - created_at;
        if !(MIN_OIDC_LOGIN_TOKEN_LIFETIME..=MAX_OIDC_LOGIN_TOKEN_LIFETIME).contains(&lifetime) {
            return Err("OIDC login token lifetime must be between 5 minutes and 24 hours".into());
        }
        Self::issue(
            id,
            organization_id,
            principal_id,
            name,
            ApiTokenScope::interactive_scopes(),
            created_at,
            Some(expires_at),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_and_revocation_are_terminal_for_authentication() {
        let created_at = Utc::now();
        let mut scopes = BTreeSet::new();
        scopes.insert(ApiTokenScope::parse("project:write").expect("scope"));
        let mut token = ApiToken::issue(
            ApiTokenId::new(),
            OrganizationId::new(),
            PrincipalId::new(),
            ApiTokenName::parse("automation").expect("name"),
            scopes,
            created_at,
            Some(created_at + chrono::Duration::minutes(1)),
        )
        .expect("token");
        assert!(token.is_active_at(created_at));
        assert!(!token.is_active_at(created_at + chrono::Duration::minutes(1)));
        assert!(token.revoke(created_at));
        assert!(!token.revoke(created_at));
        assert_eq!(token.aggregate_version, 2);
    }

    #[test]
    fn oidc_login_tokens_are_short_lived_and_never_platform_tokens() {
        let created_at = Utc::now();
        let token = ApiToken::issue_oidc_login(
            ApiTokenId::new(),
            OrganizationId::new(),
            PrincipalId::new(),
            ApiTokenName::parse("OIDC login").expect("name"),
            created_at,
            created_at + chrono::Duration::hours(1),
        )
        .expect("token");
        assert!(token.scopes.iter().all(|scope| !matches!(
            scope.as_str(),
            ApiTokenScope::PLATFORM_WRITE | ApiTokenScope::TOKEN_WRITE
        )));
        assert!(ApiToken::issue_oidc_login(
            ApiTokenId::new(),
            OrganizationId::new(),
            PrincipalId::new(),
            ApiTokenName::parse("Too long").expect("name"),
            created_at,
            created_at + chrono::Duration::hours(25),
        )
        .is_err());
    }
}
