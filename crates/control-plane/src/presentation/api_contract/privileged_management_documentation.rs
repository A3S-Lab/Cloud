use super::privileged_management_operation::{
    PLATFORM_ROLE_BINDINGS_PATH, PLATFORM_ROLE_BINDING_PATH, PLATFORM_ROLE_BINDING_REVOCATION_PATH,
    PLATFORM_ROLE_BINDING_ROLE_PATH, PLATFORM_ROLE_POLICY_PATH,
    PLATFORM_ROLE_POLICY_REVISIONS_PATH, PLATFORM_ROLE_POLICY_REVISION_PATH,
    PRINCIPAL_PLATFORM_ROLE_BINDING_PATH, TENANT_SUPPORT_GRANTS_PATH,
    TENANT_SUPPORT_GRANT_APPROVALS_PATH, TENANT_SUPPORT_GRANT_PATH,
    TENANT_SUPPORT_GRANT_REVOCATION_PATH, TRUST_DOMAIN_PATH, TRUST_DOMAIN_REVISIONS_PATH,
    TRUST_DOMAIN_REVISION_PATH, WORKLOAD_IDENTITY_POLICY_FOR_WORKLOAD_PATH,
    WORKLOAD_IDENTITY_POLICY_PATH, WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH,
    WORKLOAD_IDENTITY_POLICY_REVISION_PATH,
};

pub(super) fn component_description(name: &str) -> Option<&'static str> {
    match name {
        "PlatformRolePermission" => Some(
            "One closed installation role and its bounded canonical set of platform permissions.",
        ),
        "PlatformRolePolicy" => Some(
            "Immutable accepted Identity-owned platform RBAC policy revision with canonical A3S ACL and digest.",
        ),
        "PlatformRolePolicyMutation" => Some(
            "Accepted platform RBAC policy revision plus caller-owned idempotency replay state.",
        ),
        "PlatformRoleBinding" => Some(
            "Versioned installation-scoped binding between one Principal and one closed platform role.",
        ),
        "PlatformRoleBindingMutation" => Some(
            "Platform role binding mutation result plus caller-owned idempotency replay state.",
        ),
        "DecisionEvidence" => Some(
            "Bounded immutable reference to authentication or authorization evidence and its SHA-256 digest.",
        ),
        "TenantSupportScope" => Some(
            "Exact tenant organization, project, or environment scope resolved under the canonical installation.",
        ),
        "TenantSupportGrantProposal" => Some(
            "Canonical time-bounded tenant-support proposal, requested authority, approvers, and authentication evidence.",
        ),
        "TenantSupportGrantApproval" => Some(
            "Immutable approval evidence bound to the exact support contract, policy revision, role binding, and credential proof.",
        ),
        "TenantSupportGrantLifecycle" => Some(
            "Accepted tenant-support grant lifecycle with optimistic version and terminal revocation generation.",
        ),
        "TenantSupportGrant" => Some(
            "Authoritative aggregate view of one support proposal, its immutable approvals, and optional accepted lifecycle.",
        ),
        "TenantSupportGrantProposalMutation" => Some(
            "Tenant-support proposal result plus caller-owned idempotency replay state.",
        ),
        "TenantSupportGrantApprovalOutcome" => Some(
            "Approval outcome containing the proposal, immutable approval evidence, and optionally activated grant.",
        ),
        "TenantSupportGrantApprovalMutation" => Some(
            "Tenant-support approval outcome plus caller-owned idempotency replay state.",
        ),
        "TenantSupportGrantMutation" => Some(
            "Tenant-support lifecycle mutation result plus caller-owned idempotency replay state.",
        ),
        "PlatformRolePolicySuccessResponse" => Some(
            "Standard success envelope containing one immutable platform RBAC policy revision.",
        ),
        "PlatformRolePolicyMutationSuccessResponse" => Some(
            "Standard success envelope containing policy acceptance and replay state.",
        ),
        "PlatformRoleBindingSuccessResponse" => Some(
            "Standard success envelope containing one authoritative platform role binding.",
        ),
        "PlatformRoleBindingMutationSuccessResponse" => Some(
            "Standard success envelope containing a platform role binding mutation and replay state.",
        ),
        "TenantSupportGrantSuccessResponse" => Some(
            "Standard success envelope containing one authoritative tenant-support grant aggregate.",
        ),
        "TenantSupportGrantProposalMutationSuccessResponse" => Some(
            "Standard success envelope containing a tenant-support proposal and replay state.",
        ),
        "TenantSupportGrantApprovalMutationSuccessResponse" => Some(
            "Standard success envelope containing a tenant-support approval outcome and replay state.",
        ),
        "TenantSupportGrantMutationSuccessResponse" => Some(
            "Standard success envelope containing a tenant-support lifecycle mutation and replay state.",
        ),
        "TrustDomainRevision" => Some(
            "One immutable Identity-owned installation TrustDomain revision with canonical A3S ACL and digest.",
        ),
        "TrustDomainRevisionList" => Some(
            "Bounded reverse-ordered immutable revision history for one exact TrustDomain.",
        ),
        "TrustDomainRevisionMutation" => Some(
            "Accepted TrustDomain revision plus caller-owned idempotency replay state.",
        ),
        "WorkloadIdentityPolicyRevision" => Some(
            "One immutable Identity-owned Workload policy revision bound to exact owner lineage and TrustDomain revision.",
        ),
        "WorkloadIdentityPolicyRevisionList" => Some(
            "Bounded reverse-ordered immutable revision history for one exact Workload Identity Policy.",
        ),
        "WorkloadIdentityPolicyRevisionMutation" => Some(
            "Accepted Workload Identity Policy revision plus caller-owned idempotency replay state.",
        ),
        "TrustDomainRevisionSuccessResponse" => Some(
            "Standard success envelope containing one immutable TrustDomain revision.",
        ),
        "TrustDomainRevisionListSuccessResponse" => Some(
            "Standard success envelope containing bounded TrustDomain revision history.",
        ),
        "TrustDomainRevisionMutationSuccessResponse" => Some(
            "Standard success envelope containing TrustDomain acceptance and replay state.",
        ),
        "WorkloadIdentityPolicyRevisionSuccessResponse" => Some(
            "Standard success envelope containing one immutable Workload Identity Policy revision.",
        ),
        "WorkloadIdentityPolicyRevisionListSuccessResponse" => Some(
            "Standard success envelope containing bounded Workload Identity Policy revision history.",
        ),
        "WorkloadIdentityPolicyRevisionMutationSuccessResponse" => Some(
            "Standard success envelope containing Workload Identity Policy acceptance and replay state.",
        ),
        _ => None,
    }
}

pub(super) fn operation_summary(method: &str, path: &str) -> Option<&'static str> {
    match (method, path) {
        ("get", PLATFORM_ROLE_POLICY_PATH) => Some("Get the current platform role policy"),
        ("get", PLATFORM_ROLE_POLICY_REVISION_PATH) => Some("Get a platform role policy revision"),
        ("post", PLATFORM_ROLE_POLICY_REVISIONS_PATH) => {
            Some("Accept a platform role policy revision")
        }
        ("post", PLATFORM_ROLE_BINDINGS_PATH) => Some("Create a platform role binding"),
        ("get", PLATFORM_ROLE_BINDING_PATH) => Some("Get a platform role binding"),
        ("post", PLATFORM_ROLE_BINDING_ROLE_PATH) => Some("Change a platform role binding"),
        ("post", PLATFORM_ROLE_BINDING_REVOCATION_PATH) => Some("Revoke a platform role binding"),
        ("get", PRINCIPAL_PLATFORM_ROLE_BINDING_PATH) => {
            Some("Get a Principal platform role binding")
        }
        ("post", TENANT_SUPPORT_GRANTS_PATH) => Some("Propose a tenant-support grant"),
        ("get", TENANT_SUPPORT_GRANT_PATH) => Some("Get a tenant-support grant"),
        ("post", TENANT_SUPPORT_GRANT_APPROVALS_PATH) => Some("Approve a tenant-support grant"),
        ("post", TENANT_SUPPORT_GRANT_REVOCATION_PATH) => Some("Revoke a tenant-support grant"),
        ("get", TRUST_DOMAIN_PATH) => Some("Get the current TrustDomain revision"),
        ("get", TRUST_DOMAIN_REVISIONS_PATH) => Some("List TrustDomain revisions"),
        ("get", TRUST_DOMAIN_REVISION_PATH) => Some("Get a TrustDomain revision"),
        ("post", TRUST_DOMAIN_REVISIONS_PATH) => Some("Accept a TrustDomain revision"),
        ("get", WORKLOAD_IDENTITY_POLICY_PATH) => {
            Some("Get the current Workload Identity Policy revision")
        }
        ("get", WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH) => {
            Some("List Workload Identity Policy revisions")
        }
        ("get", WORKLOAD_IDENTITY_POLICY_REVISION_PATH) => {
            Some("Get a Workload Identity Policy revision")
        }
        ("post", WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH) => {
            Some("Accept a Workload Identity Policy revision")
        }
        ("get", WORKLOAD_IDENTITY_POLICY_FOR_WORKLOAD_PATH) => {
            Some("Get a Workload's current identity policy")
        }
        _ => None,
    }
}

pub(super) fn operation_description(method: &str, path: &str) -> Option<&'static str> {
    match (method, path) {
        ("get", PLATFORM_ROLE_POLICY_PATH) => Some(
            "Authorizes the exact verified Principal and API Token credential, then returns the canonical current Identity-owned platform RBAC policy revision in the same PostgreSQL authority transaction.",
        ),
        ("get", PLATFORM_ROLE_POLICY_REVISION_PATH) => Some(
            "Authorizes the exact verified Principal and credential, then returns one immutable platform RBAC policy revision through the sole Identity authority.",
        ),
        ("post", PLATFORM_ROLE_POLICY_REVISIONS_PATH) => Some(
            "Parses one canonical `cloud.identity.platform-role-policy.v1` A3S ACL and atomically accepts its next immutable revision using expected-current revision fencing and caller-owned idempotency.",
        ),
        ("post", PLATFORM_ROLE_BINDINGS_PATH) => Some(
            "Creates one installation-scoped Principal role binding only when the referenced immutable policy revision remains current; the exact verified credential is authorized and audited atomically.",
        ),
        ("get", PLATFORM_ROLE_BINDING_PATH) => Some(
            "Returns one exact platform role binding after atomic authorization by the canonical Identity-owned platform RBAC authority.",
        ),
        ("post", PLATFORM_ROLE_BINDING_ROLE_PATH) => Some(
            "Changes one role binding with aggregate-version and current-policy-revision compare-and-swap fencing, exact credential authorization, audit evidence, and caller-owned idempotency.",
        ),
        ("post", PLATFORM_ROLE_BINDING_REVOCATION_PATH) => Some(
            "Terminally revokes one role binding with aggregate-version fencing in the same PostgreSQL transaction that authorizes and audits the exact verified credential.",
        ),
        ("get", PRINCIPAL_PLATFORM_ROLE_BINDING_PATH) => Some(
            "Returns the active installation-scoped role binding for one exact Principal after atomic authorization; revoked bindings are never treated as ambient authority.",
        ),
        ("post", TENANT_SUPPORT_GRANTS_PATH) => Some(
            "Parses one canonical `cloud.identity.tenant-support-grant.v1` A3S ACL and records a bounded tenant-scoped proposal whose requested Principal, permissions, interval, approvers, and notification obligations are part of the signed digest.",
        ),
        ("get", TENANT_SUPPORT_GRANT_PATH) => Some(
            "Returns one proposal, immutable approval evidence, and optional accepted lifecycle after exact credential authorization by the sole Identity-owned tenant-support authority.",
        ),
        ("post", TENANT_SUPPORT_GRANT_APPROVALS_PATH) => Some(
            "Records one immutable human approval only when the expected contract digest, exact approver credential, current policy revision, and active role binding all agree; activation is atomic when the closed approval threshold is reached.",
        ),
        ("post", TENANT_SUPPORT_GRANT_REVOCATION_PATH) => Some(
            "Terminally revokes an accepted tenant-support grant with aggregate-version fencing, exact credential authorization, immutable audit evidence, and caller-owned idempotency.",
        ),
        ("get", TRUST_DOMAIN_PATH) => Some(
            "Authorizes the exact verified Principal and API Token, then reads the strongly consistent current revision of one installation-scoped TrustDomain through the sole Identity authority.",
        ),
        ("get", TRUST_DOMAIN_REVISIONS_PATH) => Some(
            "Authorizes the exact verified credential and returns a bounded reverse-ordered immutable history for one TrustDomain; no cache or provider projection is policy truth.",
        ),
        ("get", TRUST_DOMAIN_REVISION_PATH) => Some(
            "Authorizes the exact verified credential and returns one exact immutable TrustDomain revision identified by both aggregate and revision IDs.",
        ),
        ("post", TRUST_DOMAIN_REVISIONS_PATH) => Some(
            "Parses one canonical `cloud.identity.trust-domain.v1` A3S ACL, checks the path and canonical Installation identities, then atomically accepts the exact predecessor-fenced revision with authorization, Audit, Outbox, and idempotency.",
        ),
        ("get", WORKLOAD_IDENTITY_POLICY_PATH) => Some(
            "Authorizes the exact verified credential and reads the strongly consistent current revision of one Organization-owned Workload Identity Policy.",
        ),
        ("get", WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH) => Some(
            "Authorizes the exact verified credential and returns bounded immutable policy history under the exact Organization and policy identities.",
        ),
        ("get", WORKLOAD_IDENTITY_POLICY_REVISION_PATH) => Some(
            "Authorizes the exact verified credential and returns one immutable Workload Identity Policy revision including its exact TrustDomain-revision and owner-lineage bindings.",
        ),
        ("post", WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH) => Some(
            "Parses one canonical `cloud.identity.workload-policy.v1` A3S ACL, rejects path, Installation, Organization, policy, owner, or current TrustDomain drift, then atomically accepts its exact predecessor-fenced revision.",
        ),
        ("get", WORKLOAD_IDENTITY_POLICY_FOR_WORKLOAD_PATH) => Some(
            "Authorizes the exact verified credential and resolves the sole current identity policy for one logical Workload without copying policy truth into the Workloads context.",
        ),
        _ => None,
    }
}

pub(super) fn response_data_description(method: &str, path: &str) -> Option<&'static str> {
    match (method, path) {
        ("get", PLATFORM_ROLE_POLICY_PATH) => {
            Some("The authoritative current immutable platform RBAC policy revision.")
        }
        ("get", PLATFORM_ROLE_POLICY_REVISION_PATH) => {
            Some("The authoritative exact immutable platform RBAC policy revision.")
        }
        ("post", PLATFORM_ROLE_POLICY_REVISIONS_PATH) => {
            Some("The accepted immutable platform RBAC policy revision and replay state.")
        }
        ("get", PLATFORM_ROLE_BINDING_PATH | PRINCIPAL_PLATFORM_ROLE_BINDING_PATH) => {
            Some("The authoritative exact installation-scoped platform role binding.")
        }
        (
            "post",
            PLATFORM_ROLE_BINDINGS_PATH
            | PLATFORM_ROLE_BINDING_ROLE_PATH
            | PLATFORM_ROLE_BINDING_REVOCATION_PATH,
        ) => Some("The authoritative platform role binding mutation and replay state."),
        ("get", TENANT_SUPPORT_GRANT_PATH) => {
            Some("The authoritative proposal, approvals, and accepted tenant-support lifecycle.")
        }
        ("post", TENANT_SUPPORT_GRANTS_PATH) => {
            Some("The canonical tenant-support proposal and replay state.")
        }
        ("post", TENANT_SUPPORT_GRANT_APPROVALS_PATH) => {
            Some("The immutable approval outcome, optional activation, and replay state.")
        }
        ("post", TENANT_SUPPORT_GRANT_REVOCATION_PATH) => {
            Some("The terminally revoked tenant-support lifecycle and replay state.")
        }
        ("get", TRUST_DOMAIN_PATH | TRUST_DOMAIN_REVISION_PATH) => {
            Some("The authoritative immutable TrustDomain revision.")
        }
        ("get", TRUST_DOMAIN_REVISIONS_PATH) => {
            Some("The bounded authoritative TrustDomain revision history.")
        }
        ("post", TRUST_DOMAIN_REVISIONS_PATH) => {
            Some("The accepted immutable TrustDomain revision and replay state.")
        }
        (
            "get",
            WORKLOAD_IDENTITY_POLICY_PATH
            | WORKLOAD_IDENTITY_POLICY_REVISION_PATH
            | WORKLOAD_IDENTITY_POLICY_FOR_WORKLOAD_PATH,
        ) => Some("The authoritative immutable Workload Identity Policy revision."),
        ("get", WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH) => {
            Some("The bounded authoritative Workload Identity Policy revision history.")
        }
        ("post", WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH) => {
            Some("The accepted immutable Workload Identity Policy revision and replay state.")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::api_contract::privileged_management_operation::PRIVILEGED_MANAGEMENT_OPERATIONS;

    #[test]
    fn every_privileged_management_operation_has_domain_documentation() {
        for (method, path) in PRIVILEGED_MANAGEMENT_OPERATIONS {
            assert!(operation_summary(method, path).is_some());
            assert!(operation_description(method, path).is_some());
            assert!(response_data_description(method, path).is_some());
        }
    }

    #[test]
    fn every_privileged_management_component_has_a_domain_description() {
        for name in [
            "PlatformRolePermission",
            "PlatformRolePolicy",
            "PlatformRolePolicyMutation",
            "PlatformRoleBinding",
            "PlatformRoleBindingMutation",
            "DecisionEvidence",
            "TenantSupportScope",
            "TenantSupportGrantProposal",
            "TenantSupportGrantApproval",
            "TenantSupportGrantLifecycle",
            "TenantSupportGrant",
            "TenantSupportGrantProposalMutation",
            "TenantSupportGrantApprovalOutcome",
            "TenantSupportGrantApprovalMutation",
            "TenantSupportGrantMutation",
            "PlatformRolePolicySuccessResponse",
            "PlatformRolePolicyMutationSuccessResponse",
            "PlatformRoleBindingSuccessResponse",
            "PlatformRoleBindingMutationSuccessResponse",
            "TenantSupportGrantSuccessResponse",
            "TenantSupportGrantProposalMutationSuccessResponse",
            "TenantSupportGrantApprovalMutationSuccessResponse",
            "TenantSupportGrantMutationSuccessResponse",
            "TrustDomainRevision",
            "TrustDomainRevisionList",
            "TrustDomainRevisionMutation",
            "WorkloadIdentityPolicyRevision",
            "WorkloadIdentityPolicyRevisionList",
            "WorkloadIdentityPolicyRevisionMutation",
            "TrustDomainRevisionSuccessResponse",
            "TrustDomainRevisionListSuccessResponse",
            "TrustDomainRevisionMutationSuccessResponse",
            "WorkloadIdentityPolicyRevisionSuccessResponse",
            "WorkloadIdentityPolicyRevisionListSuccessResponse",
            "WorkloadIdentityPolicyRevisionMutationSuccessResponse",
        ] {
            assert!(component_description(name).is_some(), "missing {name}");
        }
    }
}
