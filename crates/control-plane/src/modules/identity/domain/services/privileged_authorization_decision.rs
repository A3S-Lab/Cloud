use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, IdentityPrincipal, IdentityPrincipalKind,
    PlatformRoleBinding, TenantSupportGrant,
};
use crate::modules::identity::domain::value_objects::{
    PlatformPermission, PlatformRole, PlatformRolePolicyContract, TenantSupportGrantContract,
    TenantSupportPermission,
};
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, validate_audit_action,
    AuthorizationDecisionRef, DecisionEvidenceRef, InstallationId, PlatformRoleBindingId,
    PlatformRolePolicyId, PlatformRolePolicyRevisionId, PrincipalId,
    PrivilegedAuthorizationDecisionId, ScopeContext, Sha256Digest, TenantSupportGrantId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

const PRIVILEGED_AUTHORIZATION_DECISION_API_VERSION: &str =
    "a3s.dev/cloud/privileged-authorization-decision/v1";
const PRIVILEGED_AUTHORIZATION_DECISION_REFERENCE_PREFIX: &str =
    "urn:a3s:cloud:identity:privileged-authorization-decision:";
const PRIVILEGED_AUTHORIZATION_DECISION_MAX_BYTES: usize = 256 * 1024;
const MAX_BINDING_EVIDENCE: usize = 16;
const MAX_PORTABLE_VERSION: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrivilegedAuthorizationDecisionRequest {
    pub principal_id: PrincipalId,
    pub authentication: DecisionEvidenceRef,
    pub platform_permission: PlatformPermission,
    pub action: String,
    pub scope: ScopeContext,
    pub resource_id: Uuid,
    pub request_id: Uuid,
}

impl PrivilegedAuthorizationDecisionRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.authentication.validate()?;
        self.scope.validate()?;
        if self.principal_id.as_uuid().is_nil()
            || self.resource_id.is_nil()
            || self.request_id.is_nil()
            || validate_audit_action(&self.action).is_err()
        {
            return Err("privileged authorization request is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformRolePolicyDecisionEvidence {
    pub policy_id: PlatformRolePolicyId,
    pub revision_id: PlatformRolePolicyRevisionId,
    pub revision_number: u64,
    pub canonical_acl: String,
    pub digest: Sha256Digest,
}

impl PlatformRolePolicyDecisionEvidence {
    fn from_revision(value: &AcceptedPlatformRolePolicyRevision) -> Self {
        Self {
            policy_id: value.policy_id,
            revision_id: value.id,
            revision_number: value.revision_number,
            canonical_acl: value.contract.canonical_acl().to_owned(),
            digest: value.contract.digest().clone(),
        }
    }

    fn contract(&self) -> Result<PlatformRolePolicyContract, String> {
        PlatformRolePolicyContract::restore(&self.canonical_acl, self.digest.as_str())
    }

    fn validate(
        &self,
        installation_id: InstallationId,
    ) -> Result<PlatformRolePolicyContract, String> {
        let contract = self.contract()?;
        if self.policy_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
            || self.revision_number == 0
            || self.policy_id != contract.spec().policy_id
            || installation_id != contract.spec().installation_id
            || self.revision_id
                != AcceptedPlatformRolePolicyRevision::revision_id_for(
                    self.policy_id,
                    self.revision_number,
                    &contract,
                )?
        {
            return Err("platform role policy decision evidence is invalid".into());
        }
        Ok(contract)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformRoleBindingDecisionEvidence {
    pub id: PlatformRoleBindingId,
    pub installation_id: InstallationId,
    pub principal_id: PrincipalId,
    pub aggregate_version: u64,
    pub role: PlatformRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TenantSupportGrantDecisionEvidence {
    pub grant_id: TenantSupportGrantId,
    pub aggregate_version: u64,
    pub revocation_generation: u64,
    pub accepted_at: DateTime<Utc>,
    pub canonical_acl: String,
    pub digest: Sha256Digest,
}

impl TenantSupportGrantDecisionEvidence {
    fn from_grant(value: &TenantSupportGrant) -> Self {
        Self {
            grant_id: value.id,
            aggregate_version: value.aggregate_version,
            revocation_generation: value.revocation_generation,
            accepted_at: value.accepted_at,
            canonical_acl: value.contract.canonical_acl().to_owned(),
            digest: value.contract.digest().clone(),
        }
    }

    fn contract(&self) -> Result<TenantSupportGrantContract, String> {
        TenantSupportGrantContract::restore(&self.canonical_acl, self.digest.as_str())
    }
}

/// Immutable allow evidence for installation administration or explicitly
/// granted tenant support. Configuration remains ACL; this dynamic fact uses
/// the existing canonical-JSON decision mechanism.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrivilegedAuthorizationDecision {
    pub api_version: String,
    pub id: PrivilegedAuthorizationDecisionId,
    pub installation_id: InstallationId,
    pub principal_id: PrincipalId,
    pub principal_version: u64,
    pub principal_kind: IdentityPrincipalKind,
    pub authentication: DecisionEvidenceRef,
    pub platform_permission: PlatformPermission,
    pub support_permission: Option<TenantSupportPermission>,
    pub action: String,
    pub scope: ScopeContext,
    pub resource_id: Uuid,
    pub policy: PlatformRolePolicyDecisionEvidence,
    pub bindings: Vec<PlatformRoleBindingDecisionEvidence>,
    pub support_grant: Option<TenantSupportGrantDecisionEvidence>,
    pub request_id: Uuid,
    pub decided_at: DateTime<Utc>,
    pub digest: Sha256Digest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PrivilegedAuthorizationDecisionDigestContent<'a> {
    api_version: &'a str,
    id: PrivilegedAuthorizationDecisionId,
    installation_id: InstallationId,
    principal_id: PrincipalId,
    principal_version: u64,
    principal_kind: IdentityPrincipalKind,
    authentication: &'a DecisionEvidenceRef,
    platform_permission: PlatformPermission,
    support_permission: Option<TenantSupportPermission>,
    action: &'a str,
    scope: ScopeContext,
    resource_id: Uuid,
    policy: &'a PlatformRolePolicyDecisionEvidence,
    bindings: &'a [PlatformRoleBindingDecisionEvidence],
    support_grant: Option<&'a TenantSupportGrantDecisionEvidence>,
    request_id: Uuid,
    decided_at: DateTime<Utc>,
}

impl PrivilegedAuthorizationDecision {
    pub fn issue_platform(
        id: PrivilegedAuthorizationDecisionId,
        request: PrivilegedAuthorizationDecisionRequest,
        principal: &IdentityPrincipal,
        policy: &AcceptedPlatformRolePolicyRevision,
        bindings: &[PlatformRoleBinding],
        decided_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::issue(
            id, request, principal, policy, bindings, None, None, decided_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn issue_tenant_support(
        id: PrivilegedAuthorizationDecisionId,
        request: PrivilegedAuthorizationDecisionRequest,
        principal: &IdentityPrincipal,
        policy: &AcceptedPlatformRolePolicyRevision,
        bindings: &[PlatformRoleBinding],
        grant: &TenantSupportGrant,
        support_permission: TenantSupportPermission,
        decided_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::issue(
            id,
            request,
            principal,
            policy,
            bindings,
            Some(grant),
            Some(support_permission),
            decided_at,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        self.authentication.validate()?;
        self.scope.validate()?;
        let policy = self.policy.validate(self.installation_id)?;
        if self.api_version != PRIVILEGED_AUTHORIZATION_DECISION_API_VERSION
            || self.id.as_uuid().is_nil()
            || self.installation_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.principal_version == 0
            || self.principal_version > MAX_PORTABLE_VERSION
            || (self.support_grant.is_some() && self.principal_kind != IdentityPrincipalKind::Human)
            || self.scope.installation_id() != self.installation_id
            || self.resource_id.is_nil()
            || self.request_id.is_nil()
            || self.decided_at != canonical_timestamp(self.decided_at)
            || validate_audit_action(&self.action).is_err()
            || self.bindings.is_empty()
            || self.bindings.len() > MAX_BINDING_EVIDENCE
            || self
                .bindings
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
            || self.bindings.iter().any(|binding| {
                binding.id.as_uuid().is_nil()
                    || binding.installation_id != self.installation_id
                    || binding.principal_id != self.principal_id
                    || binding.aggregate_version == 0
                    || binding.aggregate_version > MAX_PORTABLE_VERSION
                    || !policy.spec().admits(binding.role, self.platform_permission)
            })
            || self.compute_digest()? != self.digest
        {
            return Err("privileged authorization decision is invalid".into());
        }
        self.validate_scope_and_support()?;
        Ok(())
    }

    pub fn reference(&self) -> Result<AuthorizationDecisionRef, String> {
        self.validate()?;
        AuthorizationDecisionRef::new(
            format!(
                "{PRIVILEGED_AUTHORIZATION_DECISION_REFERENCE_PREFIX}{}",
                self.id
            ),
            self.digest.clone(),
        )
    }

    pub const fn audit_action() -> &'static str {
        "identity.privileged-access.authorize"
    }

    #[allow(clippy::too_many_arguments)]
    fn issue(
        id: PrivilegedAuthorizationDecisionId,
        request: PrivilegedAuthorizationDecisionRequest,
        principal: &IdentityPrincipal,
        policy: &AcceptedPlatformRolePolicyRevision,
        bindings: &[PlatformRoleBinding],
        support_grant: Option<&TenantSupportGrant>,
        support_permission: Option<TenantSupportPermission>,
        decided_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        request.validate()?;
        policy.validate()?;
        let decided_at = canonical_timestamp(decided_at);
        if !principal_is_active_at(principal, request.principal_id, decided_at)
            || request.scope.installation_id() != policy.installation_id
        {
            return Err("privileged authorization identity evidence is invalid".into());
        }
        let binding_evidence = supporting_bindings(
            bindings,
            policy,
            request.principal_id,
            request.platform_permission,
        )?;
        match (support_grant, support_permission) {
            (None, None) => {
                if request.platform_permission == PlatformPermission::TenantSupportUse
                    || !platform_permission_allows_scope(request.platform_permission, request.scope)
                {
                    return Err("platform role alone cannot authorize the requested scope".into());
                }
            }
            (Some(grant), Some(permission)) => {
                if request.platform_permission != PlatformPermission::TenantSupportUse
                    || !request.scope.is_tenant_scope()
                    || grant.contract.spec().principal_id != request.principal_id
                    || !grant.admits(request.scope, permission, decided_at)?
                {
                    return Err("tenant support evidence does not authorize the request".into());
                }
            }
            _ => return Err("tenant support permission and grant evidence must be paired".into()),
        }
        let mut value = Self {
            api_version: PRIVILEGED_AUTHORIZATION_DECISION_API_VERSION.into(),
            id,
            installation_id: policy.installation_id,
            principal_id: principal.id,
            principal_version: principal.aggregate_version,
            principal_kind: principal.kind,
            authentication: request.authentication,
            platform_permission: request.platform_permission,
            support_permission,
            action: request.action,
            scope: request.scope,
            resource_id: request.resource_id,
            policy: PlatformRolePolicyDecisionEvidence::from_revision(policy),
            bindings: binding_evidence,
            support_grant: support_grant.map(TenantSupportGrantDecisionEvidence::from_grant),
            request_id: request.request_id,
            decided_at,
            digest: zero_digest()?,
        };
        value.digest = value.compute_digest()?;
        value.validate()?;
        Ok(value)
    }

    fn validate_scope_and_support(&self) -> Result<(), String> {
        match (&self.support_grant, self.support_permission) {
            (None, None) => {
                if self.platform_permission == PlatformPermission::TenantSupportUse
                    || !platform_permission_allows_scope(self.platform_permission, self.scope)
                {
                    return Err("platform authorization scope is invalid".into());
                }
            }
            (Some(evidence), Some(permission)) => {
                let contract = evidence.contract()?;
                if self.platform_permission != PlatformPermission::TenantSupportUse
                    || !self.scope.is_tenant_scope()
                    || evidence.grant_id.as_uuid().is_nil()
                    || evidence.grant_id != contract.spec().grant_id
                    || evidence.aggregate_version != 1
                    || evidence.revocation_generation != 0
                    || evidence.accepted_at != canonical_timestamp(evidence.accepted_at)
                    || evidence.accepted_at > self.decided_at
                    || contract.spec().principal_id != self.principal_id
                    || contract.spec().installation_id() != self.installation_id
                    || !contract.spec().scope.contains(self.scope)?
                    || !contract.spec().permissions.contains(&permission)
                    || self.decided_at < contract.spec().starts_at
                    || self.decided_at >= contract.spec().expires_at
                {
                    return Err("tenant support authorization evidence is invalid".into());
                }
            }
            _ => return Err("tenant support authorization evidence is incomplete".into()),
        }
        Ok(())
    }

    fn compute_digest(&self) -> Result<Sha256Digest, String> {
        let content = PrivilegedAuthorizationDecisionDigestContent {
            api_version: &self.api_version,
            id: self.id,
            installation_id: self.installation_id,
            principal_id: self.principal_id,
            principal_version: self.principal_version,
            principal_kind: self.principal_kind,
            authentication: &self.authentication,
            platform_permission: self.platform_permission,
            support_permission: self.support_permission,
            action: &self.action,
            scope: self.scope,
            resource_id: self.resource_id,
            policy: &self.policy,
            bindings: &self.bindings,
            support_grant: self.support_grant.as_ref(),
            request_id: self.request_id,
            decided_at: self.decided_at,
        };
        let canonical = canonical_json_bounded(
            &content,
            PRIVILEGED_AUTHORIZATION_DECISION_MAX_BYTES,
            "privileged authorization decision digest content",
        )?;
        Sha256Digest::parse(sha256_digest(&canonical))
    }
}

fn principal_is_active_at(
    principal: &IdentityPrincipal,
    principal_id: PrincipalId,
    decided_at: DateTime<Utc>,
) -> bool {
    principal.id == principal_id
        && !principal.id.as_uuid().is_nil()
        && principal.aggregate_version > 0
        && principal.created_at == canonical_timestamp(principal.created_at)
        && decided_at >= principal.created_at
        && principal.is_active()
}

fn supporting_bindings(
    bindings: &[PlatformRoleBinding],
    policy: &AcceptedPlatformRolePolicyRevision,
    principal_id: PrincipalId,
    permission: PlatformPermission,
) -> Result<Vec<PlatformRoleBindingDecisionEvidence>, String> {
    if bindings.is_empty() || bindings.len() > MAX_BINDING_EVIDENCE {
        return Err("platform role binding evidence count is outside bounds".into());
    }
    let mut evidence = Vec::new();
    let mut binding_ids = BTreeSet::new();
    for binding in bindings {
        binding.validate_against_policy(policy)?;
        if !binding_ids.insert(binding.id)
            || !binding.is_active()
            || binding.installation_id != policy.installation_id
            || binding.principal_id != principal_id
        {
            return Err("platform role binding evidence does not match the request".into());
        }
        if policy.admits(binding.role, permission) {
            evidence.push(PlatformRoleBindingDecisionEvidence {
                id: binding.id,
                installation_id: binding.installation_id,
                principal_id: binding.principal_id,
                aggregate_version: binding.aggregate_version,
                role: binding.role,
            });
        }
    }
    evidence.sort_by_key(|binding| binding.id);
    if evidence.is_empty() || evidence.windows(2).any(|pair| pair[0].id == pair[1].id) {
        return Err("no unique active platform role binding admits the permission".into());
    }
    Ok(evidence)
}

fn platform_permission_allows_scope(permission: PlatformPermission, scope: ScopeContext) -> bool {
    match scope {
        ScopeContext::Installation { .. } => permission != PlatformPermission::TenantSupportUse,
        ScopeContext::Organization { .. } => matches!(
            permission,
            PlatformPermission::TenantLifecycleRead
                | PlatformPermission::TenantLifecycleManage
                | PlatformPermission::TenantSupportRead
                | PlatformPermission::TenantSupportManage
        ),
        ScopeContext::Project { .. } | ScopeContext::Environment { .. } => matches!(
            permission,
            PlatformPermission::TenantSupportRead | PlatformPermission::TenantSupportManage
        ),
    }
}

fn zero_digest() -> Result<Sha256Digest, String> {
    Sha256Digest::parse(format!("sha256:{}", "0".repeat(64)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::{
        PlatformRolePolicyContract, TenantNotificationRequirement,
        TenantSupportApprovalRequirement, TenantSupportGrantContractSpec, TenantSupportGrantMode,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, PlatformRolePolicyId, ProjectId, ResourceName,
    };
    use chrono::{Duration, TimeZone};

    fn timestamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 29, 10, 0, 0)
            .single()
            .expect("timestamp")
    }

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn principal() -> IdentityPrincipal {
        IdentityPrincipal::create(
            PrincipalId::new(),
            crate::modules::identity::domain::entities::IdentityPrincipalKind::Human,
            ResourceName::parse("support operator").expect("name"),
            timestamp() - Duration::hours(1),
        )
    }

    fn policy(installation_id: InstallationId) -> AcceptedPlatformRolePolicyRevision {
        AcceptedPlatformRolePolicyRevision::accept(
            PlatformRolePolicyContract::baseline(installation_id, PlatformRolePolicyId::new())
                .expect("contract"),
            1,
            PrincipalId::new(),
            timestamp() - Duration::hours(1),
        )
        .expect("policy")
    }

    fn binding(
        installation_id: InstallationId,
        principal_id: PrincipalId,
        policy: &AcceptedPlatformRolePolicyRevision,
        role: PlatformRole,
    ) -> PlatformRoleBinding {
        PlatformRoleBinding::create(
            PlatformRoleBindingId::new(),
            installation_id,
            principal_id,
            role,
            policy,
            PrincipalId::new(),
            timestamp() - Duration::minutes(30),
        )
        .expect("binding")
    }

    fn authentication() -> DecisionEvidenceRef {
        DecisionEvidenceRef::new("urn:a3s:cloud:identity:authentication:test", digest('b'))
            .expect("authentication")
    }

    fn request(
        principal_id: PrincipalId,
        permission: PlatformPermission,
        scope: ScopeContext,
    ) -> PrivilegedAuthorizationDecisionRequest {
        PrivilegedAuthorizationDecisionRequest {
            principal_id,
            authentication: authentication(),
            platform_permission: permission,
            action: "identity.privileged-access.test".into(),
            scope,
            resource_id: Uuid::now_v7(),
            request_id: Uuid::now_v7(),
        }
    }

    #[test]
    fn platform_decision_binds_current_policy_principal_and_role_evidence() {
        let installation_id = InstallationId::new();
        let principal = principal();
        let policy = policy(installation_id);
        let binding = binding(
            installation_id,
            principal.id,
            &policy,
            PlatformRole::PlatformOperator,
        );
        let decision = PrivilegedAuthorizationDecision::issue_platform(
            PrivilegedAuthorizationDecisionId::new(),
            request(
                principal.id,
                PlatformPermission::OperationsExecute,
                ScopeContext::installation(installation_id).expect("scope"),
            ),
            &principal,
            &policy,
            &[binding],
            timestamp(),
        )
        .expect("decision");
        decision.validate().expect("valid");
        assert!(decision
            .reference()
            .expect("reference")
            .id
            .ends_with(&decision.id.to_string()));

        let mut forged = decision;
        forged.bindings[0].aggregate_version += 1;
        assert!(forged.validate().is_err());
    }

    #[test]
    fn platform_role_alone_cannot_cross_into_tenant_operations() {
        let installation_id = InstallationId::new();
        let principal = principal();
        let policy = policy(installation_id);
        let binding = binding(
            installation_id,
            principal.id,
            &policy,
            PlatformRole::PlatformOperator,
        );
        assert!(PrivilegedAuthorizationDecision::issue_platform(
            PrivilegedAuthorizationDecisionId::new(),
            request(
                principal.id,
                PlatformPermission::OperationsRead,
                ScopeContext::organization(installation_id, OrganizationId::new()).expect("scope"),
            ),
            &principal,
            &policy,
            &[binding],
            timestamp(),
        )
        .is_err());
    }

    #[test]
    fn inactive_principals_and_roles_without_the_permission_never_issue_evidence() {
        let installation_id = InstallationId::new();
        let mut disabled = principal();
        disabled.disabled_at = Some(timestamp() - Duration::minutes(1));
        let policy = policy(installation_id);
        let operator = binding(
            installation_id,
            disabled.id,
            &policy,
            PlatformRole::PlatformOperator,
        );
        assert!(PrivilegedAuthorizationDecision::issue_platform(
            PrivilegedAuthorizationDecisionId::new(),
            request(
                disabled.id,
                PlatformPermission::OperationsExecute,
                ScopeContext::installation(installation_id).expect("scope"),
            ),
            &disabled,
            &policy,
            &[operator],
            timestamp(),
        )
        .is_err());

        let auditor = principal();
        let auditor_binding = binding(
            installation_id,
            auditor.id,
            &policy,
            PlatformRole::SecurityAuditor,
        );
        assert!(PrivilegedAuthorizationDecision::issue_platform(
            PrivilegedAuthorizationDecisionId::new(),
            request(
                auditor.id,
                PlatformPermission::TenantSupportUse,
                ScopeContext::installation(installation_id).expect("scope"),
            ),
            &auditor,
            &policy,
            &[auditor_binding],
            timestamp(),
        )
        .is_err());
    }

    #[test]
    fn tenant_support_requires_role_and_active_exact_scope_grant() {
        let installation_id = InstallationId::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let principal = principal();
        let policy = policy(installation_id);
        let binding = binding(
            installation_id,
            principal.id,
            &policy,
            PlatformRole::PlatformOperator,
        );
        let contract = TenantSupportGrantContract::from_spec(TenantSupportGrantContractSpec {
            grant_id: TenantSupportGrantId::new(),
            principal_id: principal.id,
            scope: ScopeContext::project(installation_id, organization_id, project_id)
                .expect("grant scope"),
            permissions: vec![TenantSupportPermission::HealthRead],
            case_reference: "INC-88".into(),
            justification_digest: digest('c'),
            mode: TenantSupportGrantMode::Standard,
            approval_requirement: TenantSupportApprovalRequirement::Single,
            approver_ids: vec![PrincipalId::new()],
            tenant_notification: TenantNotificationRequirement::Required,
            security_alert_required: false,
            post_incident_review_required: false,
            starts_at: timestamp() - Duration::minutes(5),
            expires_at: timestamp() + Duration::minutes(30),
        })
        .expect("grant contract");
        let mut grant = TenantSupportGrant::accept(contract, timestamp() - Duration::minutes(10))
            .expect("grant");
        let environment_scope = ScopeContext::environment(
            installation_id,
            organization_id,
            project_id,
            EnvironmentId::new(),
        )
        .expect("scope");
        let decision = PrivilegedAuthorizationDecision::issue_tenant_support(
            PrivilegedAuthorizationDecisionId::new(),
            request(
                principal.id,
                PlatformPermission::TenantSupportUse,
                environment_scope,
            ),
            &principal,
            &policy,
            std::slice::from_ref(&binding),
            &grant,
            TenantSupportPermission::HealthRead,
            timestamp(),
        )
        .expect("support decision");
        decision.validate().expect("valid");

        let mut service_principal = principal.clone();
        service_principal.kind = IdentityPrincipalKind::Service;
        assert!(PrivilegedAuthorizationDecision::issue_tenant_support(
            PrivilegedAuthorizationDecisionId::new(),
            request(
                service_principal.id,
                PlatformPermission::TenantSupportUse,
                environment_scope,
            ),
            &service_principal,
            &policy,
            std::slice::from_ref(&binding),
            &grant,
            TenantSupportPermission::HealthRead,
            timestamp(),
        )
        .is_err());

        grant
            .revoke(PrincipalId::new(), timestamp() + Duration::minutes(1))
            .expect("revoke");
        assert!(PrivilegedAuthorizationDecision::issue_tenant_support(
            PrivilegedAuthorizationDecisionId::new(),
            request(
                principal.id,
                PlatformPermission::TenantSupportUse,
                environment_scope,
            ),
            &principal,
            &policy,
            &[binding],
            &grant,
            TenantSupportPermission::HealthRead,
            timestamp() + Duration::minutes(2),
        )
        .is_err());
    }
}
