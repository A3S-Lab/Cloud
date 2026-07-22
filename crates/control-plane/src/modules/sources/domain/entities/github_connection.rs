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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubProviderCheckError {
    NotConfigured,
    Unavailable,
    Protocol,
}

impl GithubProviderCheckError {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "not_configured" => Ok(Self::NotConfigured),
            "unavailable" => Ok(Self::Unavailable),
            "protocol" => Ok(Self::Protocol),
            _ => Err("GitHub provider check error is invalid".into()),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::Unavailable => "unavailable",
            Self::Protocol => "protocol",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubProviderAuthorityState {
    Active,
    Suspended,
    Deleted,
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
pub struct GithubProviderAuthority {
    pub installation_id: GithubInstallationId,
    pub account: Option<GithubInstallationAccount>,
    pub state: GithubProviderAuthorityState,
}

impl GithubProviderAuthority {
    pub fn available(
        installation_id: GithubInstallationId,
        account: GithubInstallationAccount,
        suspended: bool,
    ) -> Self {
        Self {
            installation_id,
            account: Some(account),
            state: if suspended {
                GithubProviderAuthorityState::Suspended
            } else {
                GithubProviderAuthorityState::Active
            },
        }
    }

    pub const fn deleted(installation_id: GithubInstallationId) -> Self {
        Self {
            installation_id,
            account: None,
            state: GithubProviderAuthorityState::Deleted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GithubProviderReconciliation {
    pub lifecycle_changed: bool,
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
    pub provider_checked_at: DateTime<Utc>,
    pub provider_check_attempted_at: DateTime<Utc>,
    pub provider_next_check_at: DateTime<Utc>,
    pub provider_check_failures: u32,
    pub provider_check_error: Option<GithubProviderCheckError>,
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
            provider_checked_at: input.connected_at,
            provider_check_attempted_at: input.connected_at,
            provider_next_check_at: input.connected_at,
            provider_check_failures: 0,
            provider_check_error: None,
        })
    }

    pub fn restore(mut connection: Self) -> Result<Self, String> {
        connection.connected_at = canonical_timestamp(connection.connected_at);
        connection.updated_at = canonical_timestamp(connection.updated_at);
        connection.provider_checked_at = canonical_timestamp(connection.provider_checked_at);
        connection.provider_check_attempted_at =
            canonical_timestamp(connection.provider_check_attempted_at);
        connection.provider_next_check_at = canonical_timestamp(connection.provider_next_check_at);
        if connection.aggregate_version == 0 {
            return Err("GitHub connection aggregate version must be positive".into());
        }
        if connection.updated_at < connection.connected_at {
            return Err("GitHub connection update time predates connection".into());
        }
        if connection.provider_checked_at < connection.connected_at
            || connection.provider_check_attempted_at < connection.provider_checked_at
            || connection.provider_next_check_at < connection.provider_check_attempted_at
        {
            return Err("GitHub provider check timestamps are inconsistent".into());
        }
        if (connection.provider_check_failures == 0) != connection.provider_check_error.is_none() {
            return Err("GitHub provider check failure state is inconsistent".into());
        }
        Ok(connection)
    }

    pub const fn is_authoritative(&self) -> bool {
        self.status.is_authoritative()
    }

    pub const fn blocks_reconnection(&self) -> bool {
        self.status.blocks_reconnection()
    }

    pub fn needs_provider_check(&self) -> bool {
        self.blocks_reconnection()
            || (matches!(
                self.status,
                GithubConnectionStatus::InstallationDeleted
                    | GithubConnectionStatus::AccountChanged
            ) && self.provider_checked_at < self.updated_at)
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
        self.advance_version()?;
        self.updated_at = reconciled_at;
        self.provider_next_check_at = reconciled_at;
        Ok(true)
    }

    pub fn reconcile_provider_authority(
        &mut self,
        authority: GithubProviderAuthority,
        checked_at: DateTime<Utc>,
        next_check_at: DateTime<Utc>,
    ) -> Result<GithubProviderReconciliation, String> {
        let checked_at = canonical_timestamp(checked_at);
        let next_check_at = canonical_timestamp(next_check_at);
        if !self.needs_provider_check() {
            return Err("GitHub connection has no pending provider reconciliation".into());
        }
        if authority.installation_id != self.installation_id {
            return Err("GitHub provider returned another installation".into());
        }
        if checked_at < self.provider_check_attempted_at || next_check_at <= checked_at {
            return Err("GitHub provider check timing is invalid".into());
        }
        let previous_status = self.status;
        let previous_login = self.account_login.clone();
        match authority.state {
            GithubProviderAuthorityState::Deleted => {
                if authority.account.is_some() {
                    return Err("deleted GitHub provider authority included an account".into());
                }
                self.status = GithubConnectionStatus::InstallationDeleted;
            }
            GithubProviderAuthorityState::Active | GithubProviderAuthorityState::Suspended => {
                let account = authority.account.ok_or_else(|| {
                    "available GitHub provider authority omitted its account".to_owned()
                })?;
                if account.id != self.account_id || account.kind != self.account_kind {
                    self.status = GithubConnectionStatus::AccountChanged;
                } else {
                    self.account_login = account.login;
                    self.status = if authority.state == GithubProviderAuthorityState::Suspended {
                        GithubConnectionStatus::Suspended
                    } else {
                        GithubConnectionStatus::Active
                    };
                }
            }
        }
        self.provider_checked_at = checked_at;
        self.provider_check_attempted_at = checked_at;
        self.provider_next_check_at = next_check_at;
        self.provider_check_failures = 0;
        self.provider_check_error = None;
        self.updated_at = checked_at;
        self.advance_version()?;
        Ok(GithubProviderReconciliation {
            lifecycle_changed: self.status != previous_status
                || self.account_login != previous_login,
        })
    }

    pub fn record_provider_check_failure(
        &mut self,
        error: GithubProviderCheckError,
        attempted_at: DateTime<Utc>,
        retry_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let attempted_at = canonical_timestamp(attempted_at);
        let retry_at = canonical_timestamp(retry_at);
        if !self.needs_provider_check() {
            return Err("GitHub connection has no pending provider retry".into());
        }
        if attempted_at < self.provider_check_attempted_at || retry_at <= attempted_at {
            return Err("GitHub provider retry timing is invalid".into());
        }
        self.provider_check_attempted_at = attempted_at;
        self.provider_next_check_at = retry_at;
        self.provider_check_failures = self
            .provider_check_failures
            .checked_add(1)
            .ok_or_else(|| "GitHub provider check failure count overflowed".to_owned())?;
        self.provider_check_error = Some(error);
        self.updated_at = attempted_at;
        self.advance_version()
    }

    fn reconcile_account(&mut self, account: &GithubInstallationAccount) {
        if account.id != self.account_id || account.kind != self.account_kind {
            self.status = GithubConnectionStatus::AccountChanged;
        } else {
            self.account_login = account.login.clone();
        }
    }

    fn advance_version(&mut self) -> Result<(), String> {
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "GitHub connection aggregate version overflowed".to_owned())?;
        Ok(())
    }
}
