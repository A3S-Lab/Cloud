use crate::modules::edge::domain::{DomainNamePattern, RouteHostname};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, EnvironmentId, OrganizationId, ProjectId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainClaimState {
    Pending,
    Verified,
    Rejected,
    Revoked,
}

impl DomainClaimState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Verified => "verified",
            Self::Rejected => "rejected",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "verified" => Ok(Self::Verified),
            "rejected" => Ok(Self::Rejected),
            "revoked" => Ok(Self::Revoked),
            _ => Err(format!("unsupported domain claim state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainClaim {
    pub id: DomainClaimId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub pattern: DomainNamePattern,
    pub challenge_dns_name: String,
    pub challenge_value: String,
    pub state: DomainClaimState,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl DomainClaim {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: DomainClaimId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        pattern: DomainNamePattern,
        challenge_value: String,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        validate_challenge(&challenge_value)?;
        let created_at = canonical_timestamp(created_at);
        Ok(Self {
            id,
            organization_id,
            project_id,
            environment_id,
            challenge_dns_name: pattern.challenge_dns_name(),
            pattern,
            challenge_value,
            state: DomainClaimState::Pending,
            failure: None,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            verified_at: None,
            revoked_at: None,
        })
    }

    pub fn covers(&self, hostname: &RouteHostname) -> bool {
        self.state == DomainClaimState::Verified && self.pattern.covers(hostname)
    }

    pub fn verify(&mut self, verified_at: DateTime<Utc>) -> Result<(), String> {
        let verified_at = canonical_timestamp(verified_at);
        self.ensure_time(verified_at)?;
        if self.state == DomainClaimState::Verified {
            return Ok(());
        }
        if self.state != DomainClaimState::Pending {
            return Err("domain claim cannot be verified from its current state".into());
        }
        self.state = DomainClaimState::Verified;
        self.failure = None;
        self.aggregate_version += 1;
        self.updated_at = verified_at;
        self.verified_at = Some(verified_at);
        Ok(())
    }

    pub fn reject(
        &mut self,
        failure: impl Into<String>,
        rejected_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let failure = sanitize_failure(failure.into())?;
        let rejected_at = canonical_timestamp(rejected_at);
        self.ensure_time(rejected_at)?;
        if self.state == DomainClaimState::Rejected && self.failure.as_deref() == Some(&failure) {
            return Ok(());
        }
        if self.state != DomainClaimState::Pending {
            return Err("domain claim cannot be rejected from its current state".into());
        }
        self.state = DomainClaimState::Rejected;
        self.failure = Some(failure);
        self.aggregate_version += 1;
        self.updated_at = rejected_at;
        Ok(())
    }

    pub fn revoke(
        &mut self,
        reason: impl Into<String>,
        revoked_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let reason = sanitize_failure(reason.into())?;
        let revoked_at = canonical_timestamp(revoked_at);
        self.ensure_time(revoked_at)?;
        if self.state == DomainClaimState::Revoked && self.failure.as_deref() == Some(&reason) {
            return Ok(());
        }
        if self.state != DomainClaimState::Verified {
            return Err("only a verified domain claim can be revoked".into());
        }
        self.state = DomainClaimState::Revoked;
        self.failure = Some(reason);
        self.aggregate_version += 1;
        self.updated_at = revoked_at;
        self.revoked_at = Some(revoked_at);
        Ok(())
    }

    fn ensure_time(&self, at: DateTime<Utc>) -> Result<(), String> {
        if at < self.updated_at {
            return Err("domain claim transition time regressed".into());
        }
        Ok(())
    }
}

fn validate_challenge(value: &str) -> Result<(), String> {
    if value.len() < 32
        || value.len() > 512
        || value.trim() != value
        || value.contains(['\0', '\r', '\n'])
    {
        return Err("domain ownership challenge must be a bounded single-line value".into());
    }
    Ok(())
}

fn sanitize_failure(value: String) -> Result<String, String> {
    let value = value.replace(['\0', '\r', '\n'], " ");
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 {
        return Err("domain claim failure must be a bounded single-line value".into());
    }
    Ok(value.into())
}
