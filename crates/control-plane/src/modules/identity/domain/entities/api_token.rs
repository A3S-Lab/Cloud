use crate::modules::identity::domain::entities::Organization;
use crate::modules::identity::domain::value_objects::{ApiTokenName, ApiTokenScope};
use crate::modules::shared_kernel::domain::{canonical_timestamp, ApiTokenId, OrganizationId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: ApiTokenId,
    pub organization_id: OrganizationId,
    pub name: ApiTokenName,
    pub scopes: BTreeSet<ApiTokenScope>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityBootstrap {
    pub organization: Organization,
    pub api_token: ApiToken,
}

impl ApiToken {
    pub fn issue(
        id: ApiTokenId,
        organization_id: OrganizationId,
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
}
