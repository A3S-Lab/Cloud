use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, SourceConnectionId,
};
use crate::modules::sources::domain::value_objects::{
    GithubAccountId, GithubAccountKind, GithubInstallationId, GithubLogin,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubConnection {
    pub id: SourceConnectionId,
    pub organization_id: OrganizationId,
    pub installation_id: GithubInstallationId,
    pub account_id: GithubAccountId,
    pub account_login: GithubLogin,
    pub account_kind: GithubAccountKind,
    pub verified_by_user_id: GithubAccountId,
    pub verified_by_user_login: GithubLogin,
    pub aggregate_version: u64,
    pub connected_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewGithubConnection {
    pub id: SourceConnectionId,
    pub organization_id: OrganizationId,
    pub installation_id: GithubInstallationId,
    pub account_id: GithubAccountId,
    pub account_login: GithubLogin,
    pub account_kind: GithubAccountKind,
    pub verified_by_user_id: GithubAccountId,
    pub verified_by_user_login: GithubLogin,
    pub connected_at: DateTime<Utc>,
}

impl GithubConnection {
    pub fn connect(input: NewGithubConnection) -> Result<Self, String> {
        Self::restore(Self {
            id: input.id,
            organization_id: input.organization_id,
            installation_id: input.installation_id,
            account_id: input.account_id,
            account_login: input.account_login,
            account_kind: input.account_kind,
            verified_by_user_id: input.verified_by_user_id,
            verified_by_user_login: input.verified_by_user_login,
            aggregate_version: 1,
            connected_at: input.connected_at,
        })
    }

    pub fn restore(mut connection: Self) -> Result<Self, String> {
        if connection.aggregate_version == 0 {
            return Err("GitHub connection aggregate version must be positive".into());
        }
        connection.connected_at = canonical_timestamp(connection.connected_at);
        Ok(connection)
    }
}
