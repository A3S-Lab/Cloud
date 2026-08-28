use super::trust_domain_contract::{
    exact_child, normalize_source, require_exact_string, required_string_list, required_uuid,
    strict_block,
};
use crate::modules::shared_kernel::domain::{InstallationId, PlatformRolePolicyId, Sha256Digest};
use a3s_acl::builder::{list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Document};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;

pub const PLATFORM_ROLE_POLICY_SCHEMA: &str = "cloud.identity.platform-role-policy.v1";
pub const PLATFORM_ROLE_POLICY_MAX_ACL_BYTES: usize = 64 * 1024;

const POLICY_BLOCK: &str = "platform_role_policy";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformRole {
    PlatformOwner,
    PlatformAdmin,
    PlatformOperator,
    SecurityAuditor,
}

impl PlatformRole {
    pub const ALL: [Self; 4] = [
        Self::PlatformOwner,
        Self::PlatformAdmin,
        Self::PlatformOperator,
        Self::SecurityAuditor,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlatformOwner => "platform_owner",
            Self::PlatformAdmin => "platform_admin",
            Self::PlatformOperator => "platform_operator",
            Self::SecurityAuditor => "security_auditor",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "platform_owner" => Ok(Self::PlatformOwner),
            "platform_admin" => Ok(Self::PlatformAdmin),
            "platform_operator" => Ok(Self::PlatformOperator),
            "security_auditor" => Ok(Self::SecurityAuditor),
            _ => Err("platform role is unsupported".into()),
        }
    }

    pub const fn permits(self, permission: PlatformPermission) -> bool {
        match self {
            Self::PlatformOwner => true,
            Self::PlatformAdmin => !matches!(
                permission,
                PlatformPermission::RolePolicyManage
                    | PlatformPermission::IdentityRootManage
                    | PlatformPermission::RecoveryExecute
            ),
            Self::PlatformOperator => matches!(
                permission,
                PlatformPermission::PlatformRead
                    | PlatformPermission::RolePolicyRead
                    | PlatformPermission::RoleBindingRead
                    | PlatformPermission::IdentityRootRead
                    | PlatformPermission::WorkloadTrustRead
                    | PlatformPermission::NodePoolRead
                    | PlatformPermission::CapacityRead
                    | PlatformPermission::ProviderRead
                    | PlatformPermission::RegistryTrustRead
                    | PlatformPermission::ConfigurationRead
                    | PlatformPermission::UpgradeRead
                    | PlatformPermission::BackupRestoreRead
                    | PlatformPermission::OperationsRead
                    | PlatformPermission::OperationsExecute
            ),
            Self::SecurityAuditor => matches!(
                permission,
                PlatformPermission::PlatformRead
                    | PlatformPermission::RolePolicyRead
                    | PlatformPermission::RoleBindingRead
                    | PlatformPermission::IdentityRootRead
                    | PlatformPermission::WorkloadTrustRead
                    | PlatformPermission::NodePoolRead
                    | PlatformPermission::CapacityRead
                    | PlatformPermission::ProviderRead
                    | PlatformPermission::RegistryTrustRead
                    | PlatformPermission::ConfigurationRead
                    | PlatformPermission::UpgradeRead
                    | PlatformPermission::BackupRestoreRead
                    | PlatformPermission::TenantLifecycleRead
                    | PlatformPermission::OperationsRead
                    | PlatformPermission::AuditRead
                    | PlatformPermission::AuditExport
                    | PlatformPermission::SecurityFindingsRead
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlatformPermission {
    PlatformRead,
    RolePolicyRead,
    RolePolicyManage,
    RoleBindingRead,
    RoleBindingManage,
    IdentityRootRead,
    IdentityRootManage,
    WorkloadTrustRead,
    WorkloadTrustManage,
    NodePoolRead,
    NodePoolManage,
    CapacityRead,
    CapacityManage,
    ProviderRead,
    ProviderManage,
    ProviderCredentialManage,
    RegistryTrustRead,
    RegistryTrustManage,
    ConfigurationRead,
    ConfigurationManage,
    UpgradeRead,
    UpgradeManage,
    BackupRestoreRead,
    BackupRestoreManage,
    TenantLifecycleRead,
    TenantLifecycleManage,
    OperationsRead,
    OperationsExecute,
    AuditRead,
    AuditExport,
    AuditRetentionManage,
    SecurityFindingsRead,
    RecoveryExecute,
}

impl Serialize for PlatformPermission {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PlatformPermission {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

impl PlatformPermission {
    pub const ALL: [Self; 33] = [
        Self::PlatformRead,
        Self::RolePolicyRead,
        Self::RolePolicyManage,
        Self::RoleBindingRead,
        Self::RoleBindingManage,
        Self::IdentityRootRead,
        Self::IdentityRootManage,
        Self::WorkloadTrustRead,
        Self::WorkloadTrustManage,
        Self::NodePoolRead,
        Self::NodePoolManage,
        Self::CapacityRead,
        Self::CapacityManage,
        Self::ProviderRead,
        Self::ProviderManage,
        Self::ProviderCredentialManage,
        Self::RegistryTrustRead,
        Self::RegistryTrustManage,
        Self::ConfigurationRead,
        Self::ConfigurationManage,
        Self::UpgradeRead,
        Self::UpgradeManage,
        Self::BackupRestoreRead,
        Self::BackupRestoreManage,
        Self::TenantLifecycleRead,
        Self::TenantLifecycleManage,
        Self::OperationsRead,
        Self::OperationsExecute,
        Self::AuditRead,
        Self::AuditExport,
        Self::AuditRetentionManage,
        Self::SecurityFindingsRead,
        Self::RecoveryExecute,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlatformRead => "platform:read",
            Self::RolePolicyRead => "platform:role-policy:read",
            Self::RolePolicyManage => "platform:role-policy:manage",
            Self::RoleBindingRead => "platform:role-binding:read",
            Self::RoleBindingManage => "platform:role-binding:manage",
            Self::IdentityRootRead => "platform:identity-root:read",
            Self::IdentityRootManage => "platform:identity-root:manage",
            Self::WorkloadTrustRead => "platform:workload-trust:read",
            Self::WorkloadTrustManage => "platform:workload-trust:manage",
            Self::NodePoolRead => "platform:node-pool:read",
            Self::NodePoolManage => "platform:node-pool:manage",
            Self::CapacityRead => "platform:capacity:read",
            Self::CapacityManage => "platform:capacity:manage",
            Self::ProviderRead => "platform:provider:read",
            Self::ProviderManage => "platform:provider:manage",
            Self::ProviderCredentialManage => "platform:provider-credential:manage",
            Self::RegistryTrustRead => "platform:registry-trust:read",
            Self::RegistryTrustManage => "platform:registry-trust:manage",
            Self::ConfigurationRead => "platform:configuration:read",
            Self::ConfigurationManage => "platform:configuration:manage",
            Self::UpgradeRead => "platform:upgrade:read",
            Self::UpgradeManage => "platform:upgrade:manage",
            Self::BackupRestoreRead => "platform:backup-restore:read",
            Self::BackupRestoreManage => "platform:backup-restore:manage",
            Self::TenantLifecycleRead => "platform:tenant-lifecycle:read",
            Self::TenantLifecycleManage => "platform:tenant-lifecycle:manage",
            Self::OperationsRead => "platform:operations:read",
            Self::OperationsExecute => "platform:operations:execute",
            Self::AuditRead => "platform:audit:read",
            Self::AuditExport => "platform:audit:export",
            Self::AuditRetentionManage => "platform:audit-retention:manage",
            Self::SecurityFindingsRead => "platform:security-findings:read",
            Self::RecoveryExecute => "platform:recovery:execute",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|permission| permission.as_str() == value)
            .ok_or_else(|| "platform permission is unsupported".into())
    }

    pub const fn is_mutating(self) -> bool {
        matches!(
            self,
            Self::RolePolicyManage
                | Self::RoleBindingManage
                | Self::IdentityRootManage
                | Self::WorkloadTrustManage
                | Self::NodePoolManage
                | Self::CapacityManage
                | Self::ProviderManage
                | Self::ProviderCredentialManage
                | Self::RegistryTrustManage
                | Self::ConfigurationManage
                | Self::UpgradeManage
                | Self::BackupRestoreManage
                | Self::TenantLifecycleManage
                | Self::OperationsExecute
                | Self::AuditRetentionManage
                | Self::RecoveryExecute
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformRolePermissionSet {
    pub role: PlatformRole,
    pub permissions: Vec<PlatformPermission>,
}

impl PlatformRolePermissionSet {
    fn normalize(mut self) -> Result<Self, String> {
        if self.permissions.is_empty() || self.permissions.len() > PlatformPermission::ALL.len() {
            return Err("platform role permission count is outside bounds".into());
        }
        let unique = self.permissions.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != self.permissions.len() {
            return Err("platform role permissions contain duplicates".into());
        }
        self.permissions = unique.into_iter().collect();
        Ok(self)
    }

    fn validate(&self) -> Result<(), String> {
        if self.permissions.is_empty() || self.permissions.len() > PlatformPermission::ALL.len() {
            return Err("platform role permission count is outside bounds".into());
        }
        let canonical = self
            .permissions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if canonical != self.permissions {
            return Err("platform role permissions are not a canonical set".into());
        }
        if self
            .permissions
            .iter()
            .any(|permission| !self.role.permits(*permission))
        {
            return Err(
                "platform role policy exceeds the role's immutable permission ceiling".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformRolePolicySpec {
    pub installation_id: InstallationId,
    pub policy_id: PlatformRolePolicyId,
    pub role_permissions: Vec<PlatformRolePermissionSet>,
}

impl PlatformRolePolicySpec {
    pub fn baseline(
        installation_id: InstallationId,
        policy_id: PlatformRolePolicyId,
    ) -> Result<Self, String> {
        Self {
            installation_id,
            policy_id,
            role_permissions: PlatformRole::ALL
                .into_iter()
                .map(|role| PlatformRolePermissionSet {
                    role,
                    permissions: PlatformPermission::ALL
                        .into_iter()
                        .filter(|permission| role.permits(*permission))
                        .collect(),
                })
                .collect(),
        }
        .normalize()
    }

    fn normalize(mut self) -> Result<Self, String> {
        self.role_permissions = self
            .role_permissions
            .into_iter()
            .map(PlatformRolePermissionSet::normalize)
            .collect::<Result<Vec<_>, _>>()?;
        self.role_permissions.sort_by_key(|value| value.role);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.installation_id.as_uuid().is_nil() || self.policy_id.as_uuid().is_nil() {
            return Err("platform role policy identity is invalid".into());
        }
        let roles = self
            .role_permissions
            .iter()
            .map(|value| value.role)
            .collect::<Vec<_>>();
        if roles != PlatformRole::ALL {
            return Err("platform role policy must contain every closed role exactly once".into());
        }
        for value in &self.role_permissions {
            value.validate()?;
        }
        if self.permissions_for(PlatformRole::PlatformOwner) != PlatformPermission::ALL {
            return Err("platform owner must retain every closed platform permission".into());
        }
        Ok(())
    }

    pub fn permissions_for(&self, role: PlatformRole) -> &[PlatformPermission] {
        self.role_permissions
            .iter()
            .find(|value| value.role == role)
            .map_or(&[], |value| value.permissions.as_slice())
    }

    pub fn admits(&self, role: PlatformRole, permission: PlatformPermission) -> bool {
        self.permissions_for(role)
            .binary_search(&permission)
            .is_ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformRolePolicyContract {
    spec: PlatformRolePolicySpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl PlatformRolePolicyContract {
    pub fn from_spec(spec: PlatformRolePolicySpec) -> Result<Self, String> {
        let spec = spec.normalize()?;
        let document = contract_document(&spec);
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > PLATFORM_ROLE_POLICY_MAX_ACL_BYTES {
            return Err("platform role policy ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated platform role policy ACL is invalid: {error}"))?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("platform role policy ACL is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn baseline(
        installation_id: InstallationId,
        policy_id: PlatformRolePolicyId,
    ) -> Result<Self, String> {
        Self::from_spec(PlatformRolePolicySpec::baseline(
            installation_id,
            policy_id,
        )?)
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        let normalized = normalize_source(
            source,
            PLATFORM_ROLE_POLICY_MAX_ACL_BYTES,
            "platform role policy",
        )?;
        let document = parse_acl(&normalized)
            .map_err(|error| format!("platform role policy ACL is invalid: {error}"))?;
        let value = Self::from_spec(parse_contract(&document)?)?;
        if value.canonical_acl != normalized {
            return Err("platform role policy ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored platform role policy ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Self::restore(self.canonical_acl(), self.digest.as_str())? != *self {
            return Err("platform role policy drifted from canonical ACL".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &PlatformRolePolicySpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn contract_document(spec: &PlatformRolePolicySpec) -> Document {
    let mut root = BlockBuilder::new(POLICY_BLOCK)
        .attr("installation_id", string(&spec.installation_id.to_string()))
        .attr("policy_id", string(&spec.policy_id.to_string()))
        .attr("schema", string(PLATFORM_ROLE_POLICY_SCHEMA));
    for role in &spec.role_permissions {
        root = root.nested_block(
            BlockBuilder::new(role.role.as_str())
                .attr(
                    "permissions",
                    list(
                        role.permissions
                            .iter()
                            .map(|permission| string(permission.as_str()))
                            .collect(),
                    ),
                )
                .build(),
        );
    }
    Document {
        blocks: vec![root.build()],
    }
}

fn parse_contract(document: &Document) -> Result<PlatformRolePolicySpec, String> {
    if document.blocks.len() != 1 {
        return Err("platform role policy ACL must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    strict_block(
        root,
        POLICY_BLOCK,
        &["installation_id", "policy_id", "schema"],
        &[
            "platform_owner",
            "platform_admin",
            "platform_operator",
            "security_auditor",
        ],
    )?;
    require_exact_string(root, "schema", PLATFORM_ROLE_POLICY_SCHEMA)?;
    let mut role_permissions = Vec::with_capacity(PlatformRole::ALL.len());
    for role in PlatformRole::ALL {
        let block = exact_child(root, role.as_str())?;
        strict_block(block, role.as_str(), &["permissions"], &[])?;
        role_permissions.push(PlatformRolePermissionSet {
            role,
            permissions: required_string_list(block, "permissions")?
                .iter()
                .map(|value| PlatformPermission::parse(value))
                .collect::<Result<Vec<_>, _>>()?,
        });
    }
    Ok(PlatformRolePolicySpec {
        installation_id: InstallationId::from_uuid(required_uuid(root, "installation_id")?),
        policy_id: PlatformRolePolicyId::from_uuid(required_uuid(root, "policy_id")?),
        role_permissions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_policy_round_trips_as_canonical_acl() {
        let contract = PlatformRolePolicyContract::baseline(
            InstallationId::new(),
            PlatformRolePolicyId::new(),
        )
        .expect("policy");
        assert_eq!(
            PlatformRolePolicyContract::parse_acl(contract.canonical_acl()).expect("round trip"),
            contract
        );
        assert!(contract.spec().admits(
            PlatformRole::PlatformAdmin,
            PlatformPermission::WorkloadTrustManage
        ));
        assert!(!contract.spec().admits(
            PlatformRole::PlatformAdmin,
            PlatformPermission::IdentityRootManage
        ));
        assert!(!contract.spec().admits(
            PlatformRole::SecurityAuditor,
            PlatformPermission::OperationsExecute
        ));
        assert_eq!(
            serde_json::to_string(&PlatformPermission::WorkloadTrustManage)
                .expect("permission JSON"),
            "\"platform:workload-trust:manage\""
        );
    }

    #[test]
    fn policy_cannot_widen_a_role_or_remove_owner_recovery() {
        let mut spec =
            PlatformRolePolicySpec::baseline(InstallationId::new(), PlatformRolePolicyId::new())
                .expect("baseline");
        spec.role_permissions
            .iter_mut()
            .find(|value| value.role == PlatformRole::PlatformOperator)
            .expect("operator")
            .permissions
            .push(PlatformPermission::IdentityRootManage);
        assert!(PlatformRolePolicyContract::from_spec(spec).is_err());

        let mut spec =
            PlatformRolePolicySpec::baseline(InstallationId::new(), PlatformRolePolicyId::new())
                .expect("baseline");
        spec.role_permissions
            .iter_mut()
            .find(|value| value.role == PlatformRole::PlatformOwner)
            .expect("owner")
            .permissions
            .pop();
        assert!(PlatformRolePolicyContract::from_spec(spec).is_err());
    }

    #[test]
    fn parser_rejects_unknown_or_noncanonical_acl() {
        let contract = PlatformRolePolicyContract::baseline(
            InstallationId::new(),
            PlatformRolePolicyId::new(),
        )
        .expect("policy");
        assert!(PlatformRolePolicyContract::parse_acl(&format!(
            "{}\nunknown = true\n",
            contract.canonical_acl().trim_end()
        ))
        .is_err());
        assert!(PlatformRolePolicyContract::parse_acl(
            &contract
                .canonical_acl()
                .replace("platform:read", "platform:unknown")
        )
        .is_err());
    }
}
