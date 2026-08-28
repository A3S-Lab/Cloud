use super::trust_domain_contract::{
    exact_child, normalize_source, require_exact_string, required_bool, required_string,
    required_string_list, required_uuid, strict_block,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, InstallationId, OrganizationId, PrincipalId, ProjectId,
    ScopeContext, Sha256Digest, TenantSupportGrantId,
};
use a3s_acl::builder::{boolean, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;

pub const TENANT_SUPPORT_GRANT_SCHEMA: &str = "cloud.identity.tenant-support-grant.v1";
pub const TENANT_SUPPORT_GRANT_MAX_ACL_BYTES: usize = 64 * 1024;
pub const TENANT_SUPPORT_STANDARD_MAX_SECONDS: i64 = 4 * 60 * 60;
pub const TENANT_SUPPORT_BREAK_GLASS_MAX_SECONDS: i64 = 30 * 60;

const TENANT_SUPPORT_GRANT_BLOCK: &str = "tenant_support_grant";
const MIN_STANDARD_LIFETIME_SECONDS: i64 = 5 * 60;
const MIN_BREAK_GLASS_LIFETIME_SECONDS: i64 = 60;
const MAX_CASE_REFERENCE_BYTES: usize = 128;
const MAX_APPROVERS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TenantSupportPermission {
    ResourceMetadataRead,
    HealthRead,
    AuditRead,
    DeploymentRead,
    DeploymentRecover,
    RouteRecover,
    RuntimeRestart,
}

impl Serialize for TenantSupportPermission {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TenantSupportPermission {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

impl TenantSupportPermission {
    pub const ALL: [Self; 7] = [
        Self::ResourceMetadataRead,
        Self::HealthRead,
        Self::AuditRead,
        Self::DeploymentRead,
        Self::DeploymentRecover,
        Self::RouteRecover,
        Self::RuntimeRestart,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResourceMetadataRead => "tenant-support:resource-metadata:read",
            Self::HealthRead => "tenant-support:health:read",
            Self::AuditRead => "tenant-support:audit:read",
            Self::DeploymentRead => "tenant-support:deployment:read",
            Self::DeploymentRecover => "tenant-support:deployment:recover",
            Self::RouteRecover => "tenant-support:route:recover",
            Self::RuntimeRestart => "tenant-support:runtime:restart",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|permission| permission.as_str() == value)
            .ok_or_else(|| "tenant support permission is unsupported".into())
    }

    pub const fn is_recovery(self) -> bool {
        matches!(
            self,
            Self::DeploymentRecover | Self::RouteRecover | Self::RuntimeRestart
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantSupportGrantMode {
    Standard,
    BreakGlass,
}

impl TenantSupportGrantMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::BreakGlass => "break_glass",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "standard" => Ok(Self::Standard),
            "break_glass" => Ok(Self::BreakGlass),
            _ => Err("tenant support grant mode is unsupported".into()),
        }
    }

    const fn lifetime_bounds(self) -> (i64, i64) {
        match self {
            Self::Standard => (
                MIN_STANDARD_LIFETIME_SECONDS,
                TENANT_SUPPORT_STANDARD_MAX_SECONDS,
            ),
            Self::BreakGlass => (
                MIN_BREAK_GLASS_LIFETIME_SECONDS,
                TENANT_SUPPORT_BREAK_GLASS_MAX_SECONDS,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantSupportApprovalRequirement {
    Single,
    Dual,
}

impl TenantSupportApprovalRequirement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Dual => "dual",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "single" => Ok(Self::Single),
            "dual" => Ok(Self::Dual),
            _ => Err("tenant support approval requirement is unsupported".into()),
        }
    }

    const fn required_approvers(self) -> usize {
        match self {
            Self::Single => 1,
            Self::Dual => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantNotificationRequirement {
    Required,
    PolicyExempt,
}

impl TenantNotificationRequirement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::PolicyExempt => "policy_exempt",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "required" => Ok(Self::Required),
            "policy_exempt" => Ok(Self::PolicyExempt),
            _ => Err("tenant notification requirement is unsupported".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantSupportGrantContractSpec {
    pub grant_id: TenantSupportGrantId,
    pub principal_id: PrincipalId,
    pub scope: ScopeContext,
    pub permissions: Vec<TenantSupportPermission>,
    pub case_reference: String,
    pub justification_digest: Sha256Digest,
    pub mode: TenantSupportGrantMode,
    pub approval_requirement: TenantSupportApprovalRequirement,
    pub approver_ids: Vec<PrincipalId>,
    pub tenant_notification: TenantNotificationRequirement,
    pub security_alert_required: bool,
    pub post_incident_review_required: bool,
    pub starts_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl TenantSupportGrantContractSpec {
    fn normalize(mut self) -> Result<Self, String> {
        self.starts_at = canonical_timestamp(self.starts_at);
        self.expires_at = canonical_timestamp(self.expires_at);
        let permission_count = self.permissions.len();
        let permissions = self.permissions.iter().copied().collect::<BTreeSet<_>>();
        if permissions.len() != permission_count {
            return Err("tenant support permissions contain duplicates".into());
        }
        self.permissions = permissions.into_iter().collect();
        let approver_count = self.approver_ids.len();
        let approvers = self.approver_ids.iter().copied().collect::<BTreeSet<_>>();
        if approvers.len() != approver_count {
            return Err("tenant support approvers contain duplicates".into());
        }
        self.approver_ids = approvers.into_iter().collect();
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.scope.validate()?;
        Sha256Digest::parse(self.justification_digest.as_str())?;
        if self.grant_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || !self.scope.is_tenant_scope()
            || self.permissions.is_empty()
            || self.permissions.len() > TenantSupportPermission::ALL.len()
            || self.permissions.windows(2).any(|pair| pair[0] >= pair[1])
            || self.approver_ids.is_empty()
            || self.approver_ids.len() > MAX_APPROVERS
            || self.approver_ids.windows(2).any(|pair| pair[0] >= pair[1])
            || self.approver_ids.contains(&self.principal_id)
            || self
                .approver_ids
                .iter()
                .any(|approver_id| approver_id.as_uuid().is_nil())
            || self.approver_ids.len() != self.approval_requirement.required_approvers()
            || !valid_case_reference(&self.case_reference)
            || self.starts_at != canonical_timestamp(self.starts_at)
            || self.expires_at != canonical_timestamp(self.expires_at)
        {
            return Err("tenant support grant contract identity or evidence is invalid".into());
        }
        let lifetime = self.expires_at.signed_duration_since(self.starts_at);
        let (minimum_seconds, maximum_seconds) = self.mode.lifetime_bounds();
        if lifetime < chrono::Duration::seconds(minimum_seconds)
            || lifetime > chrono::Duration::seconds(maximum_seconds)
        {
            return Err("tenant support grant lifetime is outside its mode bounds".into());
        }
        if self.mode == TenantSupportGrantMode::BreakGlass
            && (self.tenant_notification != TenantNotificationRequirement::Required
                || !self.security_alert_required
                || !self.post_incident_review_required)
        {
            return Err(
                "break-glass tenant support must notify tenant and security and require review"
                    .into(),
            );
        }
        Ok(())
    }

    pub const fn installation_id(&self) -> InstallationId {
        self.scope.installation_id()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantSupportGrantContract {
    spec: TenantSupportGrantContractSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl TenantSupportGrantContract {
    pub fn from_spec(spec: TenantSupportGrantContractSpec) -> Result<Self, String> {
        let spec = spec.normalize()?;
        let document = contract_document(&spec)?;
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > TENANT_SUPPORT_GRANT_MAX_ACL_BYTES {
            return Err("tenant support grant ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated tenant support grant ACL is invalid: {error}"))?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("tenant support grant ACL is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        let normalized = normalize_source(
            source,
            TENANT_SUPPORT_GRANT_MAX_ACL_BYTES,
            "tenant support grant",
        )?;
        let document = parse_acl(&normalized)
            .map_err(|error| format!("tenant support grant ACL is invalid: {error}"))?;
        let value = Self::from_spec(parse_contract(&document)?)?;
        if value.canonical_acl != normalized {
            return Err("tenant support grant ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored tenant support grant ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Self::restore(self.canonical_acl(), self.digest.as_str())? != *self {
            return Err("tenant support grant drifted from canonical ACL".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &TenantSupportGrantContractSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn contract_document(spec: &TenantSupportGrantContractSpec) -> Result<Document, String> {
    let scope = BlockBuilder::new("scope")
        .nested_block(scope_block(spec.scope)?)
        .build();
    Ok(Document {
        blocks: vec![BlockBuilder::new(TENANT_SUPPORT_GRANT_BLOCK)
            .attr(
                "approval_requirement",
                string(spec.approval_requirement.as_str()),
            )
            .attr(
                "approver_ids",
                list(
                    spec.approver_ids
                        .iter()
                        .map(|value| string(&value.to_string()))
                        .collect(),
                ),
            )
            .attr("case_reference", string(&spec.case_reference))
            .attr(
                "expires_at",
                string(&spec.expires_at.to_rfc3339_opts(SecondsFormat::Micros, true)),
            )
            .attr("grant_id", string(&spec.grant_id.to_string()))
            .attr(
                "justification_digest",
                string(spec.justification_digest.as_str()),
            )
            .attr("mode", string(spec.mode.as_str()))
            .attr(
                "permissions",
                list(
                    spec.permissions
                        .iter()
                        .map(|permission| string(permission.as_str()))
                        .collect(),
                ),
            )
            .attr("principal_id", string(&spec.principal_id.to_string()))
            .attr(
                "post_incident_review_required",
                boolean(spec.post_incident_review_required),
            )
            .attr("schema", string(TENANT_SUPPORT_GRANT_SCHEMA))
            .attr(
                "security_alert_required",
                boolean(spec.security_alert_required),
            )
            .attr(
                "starts_at",
                string(&spec.starts_at.to_rfc3339_opts(SecondsFormat::Micros, true)),
            )
            .attr(
                "tenant_notification",
                string(spec.tenant_notification.as_str()),
            )
            .nested_block(scope)
            .build()],
    })
}

fn scope_block(scope: ScopeContext) -> Result<Block, String> {
    Ok(match scope {
        ScopeContext::Organization {
            installation_id,
            organization_id,
        } => BlockBuilder::new("organization")
            .attr("installation_id", string(&installation_id.to_string()))
            .attr("organization_id", string(&organization_id.to_string()))
            .build(),
        ScopeContext::Project {
            installation_id,
            organization_id,
            project_id,
        } => BlockBuilder::new("project")
            .attr("installation_id", string(&installation_id.to_string()))
            .attr("organization_id", string(&organization_id.to_string()))
            .attr("project_id", string(&project_id.to_string()))
            .build(),
        ScopeContext::Environment {
            installation_id,
            organization_id,
            project_id,
            environment_id,
        } => BlockBuilder::new("environment")
            .attr("environment_id", string(&environment_id.to_string()))
            .attr("installation_id", string(&installation_id.to_string()))
            .attr("organization_id", string(&organization_id.to_string()))
            .attr("project_id", string(&project_id.to_string()))
            .build(),
        ScopeContext::Installation { .. } => {
            return Err("tenant support grant requires a tenant scope".into())
        }
    })
}

fn parse_contract(document: &Document) -> Result<TenantSupportGrantContractSpec, String> {
    if document.blocks.len() != 1 {
        return Err("tenant support grant must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    strict_block(
        root,
        TENANT_SUPPORT_GRANT_BLOCK,
        &[
            "approval_requirement",
            "approver_ids",
            "case_reference",
            "expires_at",
            "grant_id",
            "justification_digest",
            "mode",
            "permissions",
            "post_incident_review_required",
            "principal_id",
            "schema",
            "security_alert_required",
            "starts_at",
            "tenant_notification",
        ],
        &["scope"],
    )?;
    require_exact_string(root, "schema", TENANT_SUPPORT_GRANT_SCHEMA)?;
    Ok(TenantSupportGrantContractSpec {
        grant_id: TenantSupportGrantId::from_uuid(required_uuid(root, "grant_id")?),
        principal_id: PrincipalId::from_uuid(required_uuid(root, "principal_id")?),
        scope: parse_scope(exact_child(root, "scope")?)?,
        permissions: required_string_list(root, "permissions")?
            .iter()
            .map(|value| TenantSupportPermission::parse(value))
            .collect::<Result<Vec<_>, _>>()?,
        case_reference: required_string(root, "case_reference")?,
        justification_digest: Sha256Digest::parse(required_string(root, "justification_digest")?)?,
        mode: TenantSupportGrantMode::parse(&required_string(root, "mode")?)?,
        approval_requirement: TenantSupportApprovalRequirement::parse(&required_string(
            root,
            "approval_requirement",
        )?)?,
        approver_ids: required_string_list(root, "approver_ids")?
            .iter()
            .map(|value| {
                uuid::Uuid::parse_str(value)
                    .map(PrincipalId::from_uuid)
                    .map_err(|_| "tenant support approver ID must be a UUID".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?,
        post_incident_review_required: required_bool(root, "post_incident_review_required")?,
        security_alert_required: required_bool(root, "security_alert_required")?,
        tenant_notification: TenantNotificationRequirement::parse(&required_string(
            root,
            "tenant_notification",
        )?)?,
        starts_at: required_timestamp(root, "starts_at")?,
        expires_at: required_timestamp(root, "expires_at")?,
    })
}

fn parse_scope(scope: &Block) -> Result<ScopeContext, String> {
    if scope.name != "scope"
        || !scope.labels.is_empty()
        || !scope.attributes.is_empty()
        || scope.blocks.len() != 1
    {
        return Err("tenant support scope block shape is invalid".into());
    }
    let value = &scope.blocks[0];
    match value.name.as_str() {
        "organization" => {
            strict_block(
                value,
                "organization",
                &["installation_id", "organization_id"],
                &[],
            )?;
            ScopeContext::organization(
                InstallationId::from_uuid(required_uuid(value, "installation_id")?),
                OrganizationId::from_uuid(required_uuid(value, "organization_id")?),
            )
        }
        "project" => {
            strict_block(
                value,
                "project",
                &["installation_id", "organization_id", "project_id"],
                &[],
            )?;
            ScopeContext::project(
                InstallationId::from_uuid(required_uuid(value, "installation_id")?),
                OrganizationId::from_uuid(required_uuid(value, "organization_id")?),
                ProjectId::from_uuid(required_uuid(value, "project_id")?),
            )
        }
        "environment" => {
            strict_block(
                value,
                "environment",
                &[
                    "environment_id",
                    "installation_id",
                    "organization_id",
                    "project_id",
                ],
                &[],
            )?;
            ScopeContext::environment(
                InstallationId::from_uuid(required_uuid(value, "installation_id")?),
                OrganizationId::from_uuid(required_uuid(value, "organization_id")?),
                ProjectId::from_uuid(required_uuid(value, "project_id")?),
                EnvironmentId::from_uuid(required_uuid(value, "environment_id")?),
            )
        }
        _ => Err("tenant support scope kind is unsupported".into()),
    }
}

fn required_timestamp(block: &Block, name: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(&required_string(block, name)?)
        .map_err(|_| format!("tenant support field {name:?} must be an RFC 3339 timestamp"))
        .map(|value| value.with_timezone(&Utc))
}

fn valid_case_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CASE_REFERENCE_BYTES
        && value.trim() == value
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn timestamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 29, 8, 0, 0)
            .single()
            .expect("timestamp")
    }

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn spec() -> TenantSupportGrantContractSpec {
        let installation_id = InstallationId::new();
        TenantSupportGrantContractSpec {
            grant_id: TenantSupportGrantId::new(),
            principal_id: PrincipalId::new(),
            scope: ScopeContext::project(installation_id, OrganizationId::new(), ProjectId::new())
                .expect("scope"),
            permissions: vec![
                TenantSupportPermission::HealthRead,
                TenantSupportPermission::ResourceMetadataRead,
            ],
            case_reference: "INC-2026-0042".into(),
            justification_digest: digest('a'),
            mode: TenantSupportGrantMode::Standard,
            approval_requirement: TenantSupportApprovalRequirement::Single,
            approver_ids: vec![PrincipalId::new()],
            tenant_notification: TenantNotificationRequirement::Required,
            security_alert_required: false,
            post_incident_review_required: false,
            starts_at: timestamp(),
            expires_at: timestamp() + Duration::hours(1),
        }
    }

    #[test]
    fn grant_round_trips_as_canonical_acl_and_permission_json() {
        let contract = TenantSupportGrantContract::from_spec(spec()).expect("contract");
        assert_eq!(
            TenantSupportGrantContract::parse_acl(contract.canonical_acl()).expect("round trip"),
            contract
        );
        assert_eq!(
            serde_json::to_string(&TenantSupportPermission::RuntimeRestart)
                .expect("permission JSON"),
            "\"tenant-support:runtime:restart\""
        );
    }

    #[test]
    fn grant_rejects_installation_scope_self_approval_and_excessive_break_glass() {
        let mut installation = spec();
        installation.scope = ScopeContext::installation(InstallationId::new()).expect("scope");
        assert!(TenantSupportGrantContract::from_spec(installation).is_err());

        let mut self_approved = spec();
        self_approved.approver_ids = vec![self_approved.principal_id];
        assert!(TenantSupportGrantContract::from_spec(self_approved).is_err());

        let mut break_glass = spec();
        break_glass.mode = TenantSupportGrantMode::BreakGlass;
        break_glass.security_alert_required = true;
        break_glass.post_incident_review_required = true;
        break_glass.expires_at = break_glass.starts_at + Duration::minutes(31);
        assert!(TenantSupportGrantContract::from_spec(break_glass).is_err());

        let mut no_review = spec();
        no_review.mode = TenantSupportGrantMode::BreakGlass;
        no_review.security_alert_required = true;
        no_review.expires_at = no_review.starts_at + Duration::minutes(15);
        assert!(TenantSupportGrantContract::from_spec(no_review).is_err());

        let mut no_security_alert = spec();
        no_security_alert.mode = TenantSupportGrantMode::BreakGlass;
        no_security_alert.post_incident_review_required = true;
        no_security_alert.expires_at = no_security_alert.starts_at + Duration::minutes(15);
        assert!(TenantSupportGrantContract::from_spec(no_security_alert).is_err());

        let mut no_tenant_notification = spec();
        no_tenant_notification.mode = TenantSupportGrantMode::BreakGlass;
        no_tenant_notification.security_alert_required = true;
        no_tenant_notification.post_incident_review_required = true;
        no_tenant_notification.tenant_notification = TenantNotificationRequirement::PolicyExempt;
        no_tenant_notification.expires_at =
            no_tenant_notification.starts_at + Duration::minutes(15);
        assert!(TenantSupportGrantContract::from_spec(no_tenant_notification).is_err());
    }

    #[test]
    fn parser_rejects_unknown_or_noncanonical_grant_acl() {
        let contract = TenantSupportGrantContract::from_spec(spec()).expect("contract");
        assert!(TenantSupportGrantContract::parse_acl(&format!(
            "{}\nunknown = true\n",
            contract.canonical_acl().trim_end()
        ))
        .is_err());
        assert!(
            TenantSupportGrantContract::parse_acl(contract.canonical_acl().trim_end()).is_err()
        );
    }
}
