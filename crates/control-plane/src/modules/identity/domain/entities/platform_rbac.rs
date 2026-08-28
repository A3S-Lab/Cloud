use crate::modules::identity::domain::value_objects::{
    PlatformPermission, PlatformRole, PlatformRolePolicyContract,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, InstallationId, PlatformRoleBindingId, PlatformRolePolicyId,
    PlatformRolePolicyRevisionId, PrincipalId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_PORTABLE_REVISION_NUMBER: u64 = 9_007_199_254_740_991;
const PLATFORM_ROLE_POLICY_REVISION_NAMESPACE: Uuid = Uuid::from_bytes([
    0xa9, 0xf6, 0xf3, 0x52, 0x2d, 0x43, 0x49, 0xca, 0x89, 0xad, 0xd4, 0xc7, 0xd9, 0x3d, 0x12, 0x44,
]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedPlatformRolePolicyRevision {
    pub installation_id: InstallationId,
    pub policy_id: PlatformRolePolicyId,
    pub id: PlatformRolePolicyRevisionId,
    pub revision_number: u64,
    pub contract: PlatformRolePolicyContract,
    pub accepted_by: PrincipalId,
    pub accepted_at: DateTime<Utc>,
}

impl AcceptedPlatformRolePolicyRevision {
    pub fn revision_id_for(
        policy_id: PlatformRolePolicyId,
        revision_number: u64,
        contract: &PlatformRolePolicyContract,
    ) -> Result<PlatformRolePolicyRevisionId, String> {
        contract.validate()?;
        validate_revision_number(revision_number)?;
        if policy_id.as_uuid().is_nil() || policy_id != contract.spec().policy_id {
            return Err("platform role policy revision owner identity is invalid".into());
        }
        let mut identity = Vec::with_capacity(24 + contract.digest().as_str().len());
        identity.extend_from_slice(policy_id.as_uuid().as_bytes());
        identity.extend_from_slice(&revision_number.to_be_bytes());
        identity.extend_from_slice(contract.digest().as_str().as_bytes());
        Ok(PlatformRolePolicyRevisionId::from_uuid(Uuid::new_v5(
            &PLATFORM_ROLE_POLICY_REVISION_NAMESPACE,
            &identity,
        )))
    }

    pub fn accept(
        contract: PlatformRolePolicyContract,
        revision_number: u64,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        contract.validate()?;
        let installation_id = contract.spec().installation_id;
        let policy_id = contract.spec().policy_id;
        let value = Self {
            installation_id,
            policy_id,
            id: Self::revision_id_for(policy_id, revision_number, &contract)?,
            revision_number,
            contract,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate()?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        installation_id: InstallationId,
        policy_id: PlatformRolePolicyId,
        id: PlatformRolePolicyRevisionId,
        revision_number: u64,
        canonical_acl: &str,
        stored_digest: &str,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            installation_id,
            policy_id,
            id,
            revision_number,
            contract: PlatformRolePolicyContract::restore(canonical_acl, stored_digest)?,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.contract.validate()?;
        validate_revision_number(self.revision_number)?;
        if self.installation_id.as_uuid().is_nil()
            || self.policy_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.accepted_by.as_uuid().is_nil()
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || self.installation_id != self.contract.spec().installation_id
            || self.policy_id != self.contract.spec().policy_id
            || self.id
                != Self::revision_id_for(self.policy_id, self.revision_number, &self.contract)?
        {
            return Err("accepted platform role policy revision state is invalid".into());
        }
        Ok(())
    }

    pub fn admits(&self, role: PlatformRole, permission: PlatformPermission) -> bool {
        self.contract.spec().admits(role, permission)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformRoleBinding {
    pub id: PlatformRoleBindingId,
    pub installation_id: InstallationId,
    pub principal_id: PrincipalId,
    pub role: PlatformRole,
    pub aggregate_version: u64,
    pub created_by: PrincipalId,
    pub updated_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl PlatformRoleBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: PlatformRoleBindingId,
        installation_id: InstallationId,
        principal_id: PrincipalId,
        role: PlatformRole,
        policy: &AcceptedPlatformRolePolicyRevision,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        policy.validate()?;
        if installation_id != policy.installation_id {
            return Err("platform role binding and policy installation do not match".into());
        }
        let created_at = canonical_timestamp(created_at);
        let value = Self {
            id,
            installation_id,
            principal_id,
            role,
            aggregate_version: 1,
            created_by,
            updated_by: created_by,
            created_at,
            updated_at: created_at,
            revoked_at: None,
        };
        value.validate_against_policy(policy)?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: PlatformRoleBindingId,
        installation_id: InstallationId,
        principal_id: PrincipalId,
        role: PlatformRole,
        aggregate_version: u64,
        created_by: PrincipalId,
        updated_by: PrincipalId,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        revoked_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let value = Self {
            id,
            installation_id,
            principal_id,
            role,
            aggregate_version,
            created_by,
            updated_by,
            created_at: canonical_timestamp(created_at),
            updated_at: canonical_timestamp(updated_at),
            revoked_at: revoked_at.map(canonical_timestamp),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.installation_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.updated_by.as_uuid().is_nil()
            || self.aggregate_version == 0
            || self.aggregate_version > MAX_PORTABLE_REVISION_NUMBER
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || self.revoked_at.is_some_and(|value| {
                value != canonical_timestamp(value) || value != self.updated_at
            })
        {
            return Err("platform role binding identity or lifecycle is invalid".into());
        }
        Ok(())
    }

    pub fn validate_against_policy(
        &self,
        policy: &AcceptedPlatformRolePolicyRevision,
    ) -> Result<(), String> {
        self.validate()?;
        policy.validate()?;
        if self.installation_id != policy.installation_id
            || policy.contract.spec().permissions_for(self.role).is_empty()
        {
            return Err("platform role binding is not admitted by the role policy".into());
        }
        Ok(())
    }

    pub fn permissions(
        &self,
        policy: &AcceptedPlatformRolePolicyRevision,
    ) -> Result<Vec<PlatformPermission>, String> {
        self.validate_against_policy(policy)?;
        if !self.is_active() {
            return Err("revoked platform role binding has no effective permissions".into());
        }
        Ok(policy.contract.spec().permissions_for(self.role).to_vec())
    }

    pub fn change_role(
        &mut self,
        role: PlatformRole,
        policy: &AcceptedPlatformRolePolicyRevision,
        changed_by: PrincipalId,
        changed_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        self.validate_against_policy(policy)?;
        if !self.is_active() {
            return Err("revoked platform role binding cannot change role".into());
        }
        if changed_by.as_uuid().is_nil() {
            return Err("platform role binding change actor is invalid".into());
        }
        let changed_at = canonical_timestamp(changed_at);
        if changed_at < self.updated_at {
            return Err("platform role binding change time moved backwards".into());
        }
        if self.role == role {
            return Ok(false);
        }
        if policy.contract.spec().permissions_for(role).is_empty() {
            return Err("new platform role is not admitted by the role policy".into());
        }
        self.role = role;
        self.aggregate_version = next_version(self.aggregate_version)?;
        self.updated_by = changed_by;
        self.updated_at = changed_at;
        self.validate_against_policy(policy)?;
        Ok(true)
    }

    pub fn revoke(
        &mut self,
        revoked_by: PrincipalId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        self.validate()?;
        if !self.is_active() {
            return Ok(false);
        }
        if revoked_by.as_uuid().is_nil() {
            return Err("platform role binding revocation actor is invalid".into());
        }
        let revoked_at = canonical_timestamp(revoked_at);
        if revoked_at < self.updated_at {
            return Err("platform role binding revocation time moved backwards".into());
        }
        self.aggregate_version = next_version(self.aggregate_version)?;
        self.updated_by = revoked_by;
        self.updated_at = revoked_at;
        self.revoked_at = Some(revoked_at);
        self.validate()?;
        Ok(true)
    }

    pub const fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }
}

fn validate_revision_number(revision_number: u64) -> Result<(), String> {
    if revision_number == 0 || revision_number > MAX_PORTABLE_REVISION_NUMBER {
        return Err("platform role policy revision number is outside portable bounds".into());
    }
    Ok(())
}

fn next_version(current: u64) -> Result<u64, String> {
    current
        .checked_add(1)
        .filter(|value| *value <= MAX_PORTABLE_REVISION_NUMBER)
        .ok_or_else(|| "platform role binding version is exhausted".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(installation_id: InstallationId) -> AcceptedPlatformRolePolicyRevision {
        AcceptedPlatformRolePolicyRevision::accept(
            PlatformRolePolicyContract::baseline(installation_id, PlatformRolePolicyId::new())
                .expect("contract"),
            1,
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("policy")
    }

    #[test]
    fn revision_identity_is_deterministic_and_forgery_resistant() {
        let installation_id = InstallationId::new();
        let contract =
            PlatformRolePolicyContract::baseline(installation_id, PlatformRolePolicyId::new())
                .expect("contract");
        let accepted_by = PrincipalId::new();
        let now = Utc::now();
        let first =
            AcceptedPlatformRolePolicyRevision::accept(contract.clone(), 1, accepted_by, now)
                .expect("first");
        let second = AcceptedPlatformRolePolicyRevision::accept(contract, 1, accepted_by, now)
            .expect("second");
        assert_eq!(first.id, second.id);
        let mut forged = first;
        forged.id = PlatformRolePolicyRevisionId::new();
        assert!(forged.validate().is_err());
    }

    #[test]
    fn binding_lifecycle_uses_policy_permissions_and_fails_closed_after_revoke() {
        let installation_id = InstallationId::new();
        let policy = policy(installation_id);
        let actor = PrincipalId::new();
        let mut binding = PlatformRoleBinding::create(
            PlatformRoleBindingId::new(),
            installation_id,
            PrincipalId::new(),
            PlatformRole::PlatformOperator,
            &policy,
            actor,
            Utc::now(),
        )
        .expect("binding");
        assert!(binding
            .permissions(&policy)
            .expect("permissions")
            .contains(&PlatformPermission::OperationsExecute));
        assert!(!binding
            .permissions(&policy)
            .expect("permissions")
            .contains(&PlatformPermission::WorkloadTrustManage));
        assert!(binding
            .change_role(PlatformRole::SecurityAuditor, &policy, actor, Utc::now())
            .expect("changed"));
        assert!(binding.revoke(actor, Utc::now()).expect("revoked"));
        assert!(binding.permissions(&policy).is_err());
    }

    #[test]
    fn binding_cannot_cross_installation_policy() {
        let installation_id = InstallationId::new();
        let policy = policy(installation_id);
        assert!(PlatformRoleBinding::create(
            PlatformRoleBindingId::new(),
            InstallationId::new(),
            PrincipalId::new(),
            PlatformRole::PlatformAdmin,
            &policy,
            PrincipalId::new(),
            Utc::now(),
        )
        .is_err());
    }
}
