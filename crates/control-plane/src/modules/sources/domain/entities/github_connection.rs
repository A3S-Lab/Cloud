use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, SourceConnectionId,
};
use crate::modules::sources::domain::value_objects::{
    GithubAccountId, GithubAccountKind, GithubInstallationId, GithubLogin,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubConnectionStatus {
    Active,
    Suspended,
    VerificationRevoked,
    InstallationDeleted,
    AccountChanged,
}

impl GithubConnectionStatus {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            "verification_revoked" => Ok(Self::VerificationRevoked),
            "installation_deleted" => Ok(Self::InstallationDeleted),
            "account_changed" => Ok(Self::AccountChanged),
            _ => Err("GitHub connection status is invalid".into()),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::VerificationRevoked => "verification_revoked",
            Self::InstallationDeleted => "installation_deleted",
            Self::AccountChanged => "account_changed",
        }
    }

    pub const fn is_authoritative(self) -> bool {
        matches!(self, Self::Active)
    }

    pub const fn blocks_reconnection(self) -> bool {
        matches!(self, Self::Active | Self::Suspended)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubInstallationAccount {
    pub id: GithubAccountId,
    pub login: GithubLogin,
    pub kind: GithubAccountKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubConnectionLifecycleChange {
    InstallationSuspended {
        installation_id: GithubInstallationId,
        account: GithubInstallationAccount,
    },
    InstallationUnsuspended {
        installation_id: GithubInstallationId,
        account: GithubInstallationAccount,
    },
    InstallationDeleted {
        installation_id: GithubInstallationId,
        account: GithubInstallationAccount,
    },
    InstallationTargetRenamed {
        installation_id: GithubInstallationId,
        account: GithubInstallationAccount,
        previous_login: GithubLogin,
    },
    UserAuthorizationRevoked {
        user_id: GithubAccountId,
        user_login: GithubLogin,
    },
}

impl GithubConnectionLifecycleChange {
    pub const fn event_name(&self) -> &'static str {
        match self {
            Self::InstallationSuspended { .. }
            | Self::InstallationUnsuspended { .. }
            | Self::InstallationDeleted { .. } => "installation",
            Self::InstallationTargetRenamed { .. } => "installation_target",
            Self::UserAuthorizationRevoked { .. } => "github_app_authorization",
        }
    }

    pub const fn action_name(&self) -> &'static str {
        match self {
            Self::InstallationSuspended { .. } => "suspend",
            Self::InstallationUnsuspended { .. } => "unsuspend",
            Self::InstallationDeleted { .. } => "deleted",
            Self::InstallationTargetRenamed { .. } => "renamed",
            Self::UserAuthorizationRevoked { .. } => "revoked",
        }
    }

    pub const fn subject_kind(&self) -> &'static str {
        match self {
            Self::UserAuthorizationRevoked { .. } => "user",
            _ => "installation",
        }
    }

    pub const fn subject_id(&self) -> u64 {
        match self {
            Self::InstallationSuspended {
                installation_id, ..
            }
            | Self::InstallationUnsuspended {
                installation_id, ..
            }
            | Self::InstallationDeleted {
                installation_id, ..
            }
            | Self::InstallationTargetRenamed {
                installation_id, ..
            } => installation_id.as_u64(),
            Self::UserAuthorizationRevoked { user_id, .. } => user_id.as_u64(),
        }
    }
}

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
    pub status: GithubConnectionStatus,
    pub aggregate_version: u64,
    pub connected_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
            status: GithubConnectionStatus::Active,
            aggregate_version: 1,
            connected_at: input.connected_at,
            updated_at: input.connected_at,
        })
    }

    pub fn restore(mut connection: Self) -> Result<Self, String> {
        connection.connected_at = canonical_timestamp(connection.connected_at);
        connection.updated_at = canonical_timestamp(connection.updated_at);
        if connection.aggregate_version == 0 {
            return Err("GitHub connection aggregate version must be positive".into());
        }
        if connection.updated_at < connection.connected_at {
            return Err("GitHub connection update time predates connection".into());
        }
        Ok(connection)
    }

    pub const fn is_authoritative(&self) -> bool {
        self.status.is_authoritative()
    }

    pub const fn blocks_reconnection(&self) -> bool {
        self.status.blocks_reconnection()
    }

    pub fn reconcile(
        &mut self,
        change: &GithubConnectionLifecycleChange,
        reconciled_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        let reconciled_at = canonical_timestamp(reconciled_at);
        if reconciled_at < self.connected_at || reconciled_at < self.updated_at {
            return Ok(false);
        }
        if !self.blocks_reconnection() {
            return Ok(false);
        }
        let previous = self.clone();
        match change {
            GithubConnectionLifecycleChange::UserAuthorizationRevoked { user_id, .. } => {
                if *user_id != self.verified_by_user_id {
                    return Ok(false);
                }
                self.status = GithubConnectionStatus::VerificationRevoked;
            }
            GithubConnectionLifecycleChange::InstallationSuspended {
                installation_id,
                account,
            } => {
                if *installation_id != self.installation_id {
                    return Ok(false);
                }
                self.reconcile_account(account);
                if self.blocks_reconnection() {
                    self.status = GithubConnectionStatus::Suspended;
                }
            }
            GithubConnectionLifecycleChange::InstallationUnsuspended {
                installation_id,
                account,
            } => {
                if *installation_id != self.installation_id {
                    return Ok(false);
                }
                self.reconcile_account(account);
                if self.blocks_reconnection() {
                    self.status = GithubConnectionStatus::Active;
                }
            }
            GithubConnectionLifecycleChange::InstallationDeleted {
                installation_id,
                account,
            } => {
                if *installation_id != self.installation_id {
                    return Ok(false);
                }
                self.reconcile_account(account);
                if self.blocks_reconnection() {
                    self.status = GithubConnectionStatus::InstallationDeleted;
                }
            }
            GithubConnectionLifecycleChange::InstallationTargetRenamed {
                installation_id,
                account,
                previous_login,
            } => {
                if *installation_id != self.installation_id {
                    return Ok(false);
                }
                if account.id != self.account_id || account.kind != self.account_kind {
                    self.status = GithubConnectionStatus::AccountChanged;
                } else if self.account_login == *previous_login
                    || self.account_login == account.login
                {
                    self.account_login = account.login.clone();
                } else {
                    self.status = GithubConnectionStatus::AccountChanged;
                }
            }
        }
        if *self == previous {
            return Ok(false);
        }
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "GitHub connection aggregate version overflowed".to_owned())?;
        self.updated_at = reconciled_at;
        Ok(true)
    }

    fn reconcile_account(&mut self, account: &GithubInstallationAccount) {
        if account.id != self.account_id || account.kind != self.account_kind {
            self.status = GithubConnectionStatus::AccountChanged;
        } else {
            self.account_login = account.login.clone();
        }
    }
}
