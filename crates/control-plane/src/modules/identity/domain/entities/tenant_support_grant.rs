use crate::modules::identity::domain::value_objects::{
    TenantSupportGrantContract, TenantSupportPermission,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, PrincipalId, ScopeContext, TenantSupportGrantId,
};
use chrono::{DateTime, Utc};

const MAX_PORTABLE_VERSION: u64 = 9_007_199_254_740_991;

/// One non-renewing, time-bounded privileged tenant-support authorization.
/// Its immutable ACL carries intent; this aggregate owns only acceptance and
/// terminal revocation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantSupportGrant {
    pub id: TenantSupportGrantId,
    pub contract: TenantSupportGrantContract,
    pub aggregate_version: u64,
    pub revocation_generation: u64,
    pub accepted_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_by: Option<PrincipalId>,
}

impl TenantSupportGrant {
    pub fn accept(
        contract: TenantSupportGrantContract,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        contract.validate()?;
        let value = Self {
            id: contract.spec().grant_id,
            contract,
            aggregate_version: 1,
            revocation_generation: 0,
            accepted_at: canonical_timestamp(accepted_at),
            revoked_at: None,
            revoked_by: None,
        };
        value.validate()?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: TenantSupportGrantId,
        canonical_acl: &str,
        stored_digest: &str,
        aggregate_version: u64,
        revocation_generation: u64,
        accepted_at: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
        revoked_by: Option<PrincipalId>,
    ) -> Result<Self, String> {
        let value = Self {
            id,
            contract: TenantSupportGrantContract::restore(canonical_acl, stored_digest)?,
            aggregate_version,
            revocation_generation,
            accepted_at: canonical_timestamp(accepted_at),
            revoked_at: revoked_at.map(canonical_timestamp),
            revoked_by,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.contract.validate()?;
        let revoked = self.revoked_at.is_some() && self.revoked_by.is_some();
        if self.id.as_uuid().is_nil()
            || self.id != self.contract.spec().grant_id
            || self.aggregate_version == 0
            || self.aggregate_version > MAX_PORTABLE_VERSION
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || self.accepted_at >= self.contract.spec().expires_at
            || (self.revoked_at.is_some() != self.revoked_by.is_some())
            || self
                .revoked_by
                .is_some_and(|principal_id| principal_id.as_uuid().is_nil())
            || self.revoked_at.is_some_and(|revoked_at| {
                revoked_at != canonical_timestamp(revoked_at) || revoked_at < self.accepted_at
            })
            || (!revoked && (self.aggregate_version != 1 || self.revocation_generation != 0))
            || (revoked && (self.aggregate_version != 2 || self.revocation_generation != 1))
        {
            return Err("tenant support grant lifecycle state is invalid".into());
        }
        Ok(())
    }

    pub const fn scope(&self) -> ScopeContext {
        self.contract.spec().scope
    }

    pub fn is_active_at(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none()
            && now >= self.accepted_at
            && now >= self.contract.spec().starts_at
            && now < self.contract.spec().expires_at
    }

    pub fn admits(
        &self,
        requested_scope: ScopeContext,
        permission: TenantSupportPermission,
        now: DateTime<Utc>,
    ) -> Result<bool, String> {
        self.validate()?;
        requested_scope.validate()?;
        Ok(self.is_active_at(now)
            && self.scope().contains(requested_scope)?
            && self.contract.spec().permissions.contains(&permission))
    }

    pub fn revoke(
        &mut self,
        revoked_by: PrincipalId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        self.validate()?;
        if self.revoked_at.is_some() {
            return Ok(false);
        }
        if revoked_by.as_uuid().is_nil() {
            return Err("tenant support revocation actor is invalid".into());
        }
        let revoked_at = canonical_timestamp(revoked_at);
        if revoked_at < self.accepted_at {
            return Err("tenant support revocation time moved backwards".into());
        }
        self.aggregate_version = 2;
        self.revocation_generation = 1;
        self.revoked_at = Some(revoked_at);
        self.revoked_by = Some(revoked_by);
        self.validate()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::{
        TenantNotificationRequirement, TenantSupportApprovalRequirement,
        TenantSupportGrantContractSpec, TenantSupportGrantMode,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, InstallationId, OrganizationId, ProjectId, Sha256Digest,
    };
    use chrono::{Duration, TimeZone};

    fn timestamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 29, 9, 0, 0)
            .single()
            .expect("timestamp")
    }

    fn contract(
        installation_id: InstallationId,
        organization_id: OrganizationId,
        project_id: ProjectId,
    ) -> TenantSupportGrantContract {
        TenantSupportGrantContract::from_spec(TenantSupportGrantContractSpec {
            grant_id: TenantSupportGrantId::new(),
            principal_id: PrincipalId::new(),
            scope: ScopeContext::project(installation_id, organization_id, project_id)
                .expect("scope"),
            permissions: vec![TenantSupportPermission::HealthRead],
            case_reference: "CASE-19".into(),
            justification_digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))
                .expect("digest"),
            mode: TenantSupportGrantMode::Standard,
            approval_requirement: TenantSupportApprovalRequirement::Single,
            approver_ids: vec![PrincipalId::new()],
            tenant_notification: TenantNotificationRequirement::Required,
            security_alert_required: false,
            post_incident_review_required: false,
            starts_at: timestamp(),
            expires_at: timestamp() + Duration::hours(1),
        })
        .expect("contract")
    }

    #[test]
    fn grant_only_narrows_scope_and_revocation_is_terminal() {
        let installation_id = InstallationId::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment = ScopeContext::environment(
            installation_id,
            organization_id,
            project_id,
            EnvironmentId::new(),
        )
        .expect("environment");
        let mut grant = TenantSupportGrant::accept(
            contract(installation_id, organization_id, project_id),
            timestamp() - Duration::minutes(1),
        )
        .expect("grant");
        assert!(grant
            .admits(
                environment,
                TenantSupportPermission::HealthRead,
                timestamp() + Duration::minutes(1)
            )
            .expect("decision"));
        assert!(!grant
            .admits(
                ScopeContext::organization(installation_id, OrganizationId::new())
                    .expect("foreign scope"),
                TenantSupportPermission::HealthRead,
                timestamp() + Duration::minutes(1)
            )
            .expect("decision"));
        assert!(grant
            .revoke(PrincipalId::new(), timestamp() + Duration::minutes(2))
            .expect("revoke"));
        assert!(!grant
            .admits(
                environment,
                TenantSupportPermission::HealthRead,
                timestamp() + Duration::minutes(3)
            )
            .expect("decision"));
        assert!(!grant
            .revoke(PrincipalId::new(), timestamp() + Duration::minutes(4))
            .expect("replay"));
    }

    #[test]
    fn grant_cannot_be_accepted_after_expiry_or_restored_with_forged_generation() {
        let contract = contract(
            InstallationId::new(),
            OrganizationId::new(),
            ProjectId::new(),
        );
        assert!(
            TenantSupportGrant::accept(contract.clone(), timestamp() + Duration::hours(1)).is_err()
        );
        assert!(TenantSupportGrant::restore(
            contract.spec().grant_id,
            contract.canonical_acl(),
            contract.digest().as_str(),
            1,
            1,
            timestamp(),
            None,
            None,
        )
        .is_err());
    }
}
