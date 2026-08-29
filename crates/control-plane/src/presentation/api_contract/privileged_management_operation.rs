use super::privileged_management_components::{
    accept_platform_role_policy_request_schema, accept_trust_domain_revision_request_schema,
    accept_workload_identity_policy_revision_request_schema,
    approve_tenant_support_grant_request_schema, change_platform_role_binding_request_schema,
    create_platform_role_binding_request_schema, expected_version_request_schema,
    propose_tenant_support_grant_request_schema,
};
use crate::modules::identity::domain::repositories::{
    DEFAULT_WORKLOAD_IDENTITY_REVISIONS_PAGE, MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE,
};
use serde_json::{json, Value};

pub(super) const PLATFORM_ROLE_POLICY_PATH: &str = "/platform/role-policy";
pub(super) const PLATFORM_ROLE_POLICY_REVISIONS_PATH: &str = "/platform/role-policy/revisions";
pub(super) const PLATFORM_ROLE_POLICY_REVISION_PATH: &str =
    "/platform/role-policy/revisions/{revision_id}";
pub(super) const PLATFORM_ROLE_BINDINGS_PATH: &str = "/platform/role-bindings";
pub(super) const PLATFORM_ROLE_BINDING_PATH: &str = "/platform/role-bindings/{binding_id}";
pub(super) const PLATFORM_ROLE_BINDING_ROLE_PATH: &str =
    "/platform/role-bindings/{binding_id}/role";
pub(super) const PLATFORM_ROLE_BINDING_REVOCATION_PATH: &str =
    "/platform/role-bindings/{binding_id}/revocation";
pub(super) const PRINCIPAL_PLATFORM_ROLE_BINDING_PATH: &str =
    "/platform/principals/{principal_id}/role-binding";
pub(super) const TENANT_SUPPORT_GRANTS_PATH: &str = "/platform/tenant-support-grants";
pub(super) const TENANT_SUPPORT_GRANT_PATH: &str = "/platform/tenant-support-grants/{grant_id}";
pub(super) const TENANT_SUPPORT_GRANT_APPROVALS_PATH: &str =
    "/platform/tenant-support-grants/{grant_id}/approvals";
pub(super) const TENANT_SUPPORT_GRANT_REVOCATION_PATH: &str =
    "/platform/tenant-support-grants/{grant_id}/revocation";
pub(super) const TRUST_DOMAIN_PATH: &str = "/platform/trust-domains/{trust_domain_id}";
pub(super) const TRUST_DOMAIN_REVISIONS_PATH: &str =
    "/platform/trust-domains/{trust_domain_id}/revisions";
pub(super) const TRUST_DOMAIN_REVISION_PATH: &str =
    "/platform/trust-domains/{trust_domain_id}/revisions/{revision_id}";
pub(super) const WORKLOAD_IDENTITY_POLICY_PATH: &str =
    "/platform/organizations/{organization_id}/workload-identity-policies/{policy_id}";
pub(super) const WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH: &str =
    "/platform/organizations/{organization_id}/workload-identity-policies/{policy_id}/revisions";
pub(super) const WORKLOAD_IDENTITY_POLICY_REVISION_PATH: &str =
    "/platform/organizations/{organization_id}/workload-identity-policies/{policy_id}/revisions/{revision_id}";
pub(super) const WORKLOAD_IDENTITY_POLICY_FOR_WORKLOAD_PATH: &str =
    "/platform/organizations/{organization_id}/workloads/{workload_id}/identity-policy";

pub(super) const PRIVILEGED_MANAGEMENT_OPERATIONS: [(&str, &str); 21] = [
    ("get", PLATFORM_ROLE_POLICY_PATH),
    ("get", PLATFORM_ROLE_POLICY_REVISION_PATH),
    ("post", PLATFORM_ROLE_POLICY_REVISIONS_PATH),
    ("post", PLATFORM_ROLE_BINDINGS_PATH),
    ("get", PLATFORM_ROLE_BINDING_PATH),
    ("post", PLATFORM_ROLE_BINDING_ROLE_PATH),
    ("post", PLATFORM_ROLE_BINDING_REVOCATION_PATH),
    ("get", PRINCIPAL_PLATFORM_ROLE_BINDING_PATH),
    ("post", TENANT_SUPPORT_GRANTS_PATH),
    ("get", TENANT_SUPPORT_GRANT_PATH),
    ("post", TENANT_SUPPORT_GRANT_APPROVALS_PATH),
    ("post", TENANT_SUPPORT_GRANT_REVOCATION_PATH),
    ("get", TRUST_DOMAIN_PATH),
    ("get", TRUST_DOMAIN_REVISIONS_PATH),
    ("get", TRUST_DOMAIN_REVISION_PATH),
    ("post", TRUST_DOMAIN_REVISIONS_PATH),
    ("get", WORKLOAD_IDENTITY_POLICY_PATH),
    ("get", WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH),
    ("get", WORKLOAD_IDENTITY_POLICY_REVISION_PATH),
    ("post", WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH),
    ("get", WORKLOAD_IDENTITY_POLICY_FOR_WORKLOAD_PATH),
];

pub(super) fn is_privileged_management_path(path: &str) -> bool {
    PRIVILEGED_MANAGEMENT_OPERATIONS
        .iter()
        .any(|(_, candidate)| *candidate == path)
}

pub(super) fn is_privileged_management_mutation(method: &str, path: &str) -> bool {
    method == "post"
        && PRIVILEGED_MANAGEMENT_OPERATIONS
            .iter()
            .any(|(candidate_method, candidate_path)| {
                *candidate_method == method && *candidate_path == path
            })
}

pub(super) fn request_schema(path: &str) -> Option<Value> {
    match path {
        PLATFORM_ROLE_POLICY_REVISIONS_PATH => Some(accept_platform_role_policy_request_schema()),
        PLATFORM_ROLE_BINDINGS_PATH => Some(create_platform_role_binding_request_schema()),
        PLATFORM_ROLE_BINDING_ROLE_PATH => Some(change_platform_role_binding_request_schema()),
        PLATFORM_ROLE_BINDING_REVOCATION_PATH | TENANT_SUPPORT_GRANT_REVOCATION_PATH => {
            Some(expected_version_request_schema())
        }
        TENANT_SUPPORT_GRANTS_PATH => Some(propose_tenant_support_grant_request_schema()),
        TENANT_SUPPORT_GRANT_APPROVALS_PATH => Some(approve_tenant_support_grant_request_schema()),
        TRUST_DOMAIN_REVISIONS_PATH => Some(accept_trust_domain_revision_request_schema()),
        WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH => {
            Some(accept_workload_identity_policy_revision_request_schema())
        }
        _ => None,
    }
}

pub(super) fn success_component(method: &str, path: &str, status: u16) -> Option<&'static str> {
    if status != 200 {
        return None;
    }
    match (method, path) {
        ("get", PLATFORM_ROLE_POLICY_PATH | PLATFORM_ROLE_POLICY_REVISION_PATH) => {
            Some("PlatformRolePolicySuccess200")
        }
        ("post", PLATFORM_ROLE_POLICY_REVISIONS_PATH) => {
            Some("PlatformRolePolicyMutationSuccess200")
        }
        ("get", PLATFORM_ROLE_BINDING_PATH | PRINCIPAL_PLATFORM_ROLE_BINDING_PATH) => {
            Some("PlatformRoleBindingSuccess200")
        }
        (
            "post",
            PLATFORM_ROLE_BINDINGS_PATH
            | PLATFORM_ROLE_BINDING_ROLE_PATH
            | PLATFORM_ROLE_BINDING_REVOCATION_PATH,
        ) => Some("PlatformRoleBindingMutationSuccess200"),
        ("get", TENANT_SUPPORT_GRANT_PATH) => Some("TenantSupportGrantSuccess200"),
        ("post", TENANT_SUPPORT_GRANTS_PATH) => {
            Some("TenantSupportGrantProposalMutationSuccess200")
        }
        ("post", TENANT_SUPPORT_GRANT_APPROVALS_PATH) => {
            Some("TenantSupportGrantApprovalMutationSuccess200")
        }
        ("post", TENANT_SUPPORT_GRANT_REVOCATION_PATH) => {
            Some("TenantSupportGrantMutationSuccess200")
        }
        ("get", TRUST_DOMAIN_PATH | TRUST_DOMAIN_REVISION_PATH) => {
            Some("TrustDomainRevisionSuccess200")
        }
        ("get", TRUST_DOMAIN_REVISIONS_PATH) => Some("TrustDomainRevisionListSuccess200"),
        ("post", TRUST_DOMAIN_REVISIONS_PATH) => Some("TrustDomainRevisionMutationSuccess200"),
        (
            "get",
            WORKLOAD_IDENTITY_POLICY_PATH
            | WORKLOAD_IDENTITY_POLICY_REVISION_PATH
            | WORKLOAD_IDENTITY_POLICY_FOR_WORKLOAD_PATH,
        ) => Some("WorkloadIdentityPolicyRevisionSuccess200"),
        ("get", WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH) => {
            Some("WorkloadIdentityPolicyRevisionListSuccess200")
        }
        ("post", WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH) => {
            Some("WorkloadIdentityPolicyRevisionMutationSuccess200")
        }
        _ => None,
    }
}

pub(super) fn query_parameters(method: &str, path: &str) -> Vec<Value> {
    if method == "get"
        && matches!(
            path,
            TRUST_DOMAIN_REVISIONS_PATH | WORKLOAD_IDENTITY_POLICY_REVISIONS_PATH
        )
    {
        return vec![json!({
            "name": "limit",
            "in": "query",
            "required": false,
            "description": "Maximum immutable revisions to return in reverse revision order.",
            "schema": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE,
                "default": DEFAULT_WORKLOAD_IDENTITY_REVISIONS_PAGE
            }
        })];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn privileged_management_route_contract_is_exact_and_closed() {
        assert_eq!(PRIVILEGED_MANAGEMENT_OPERATIONS.len(), 21);
        for (method, path) in PRIVILEGED_MANAGEMENT_OPERATIONS {
            assert!(is_privileged_management_path(path));
            assert_eq!(
                is_privileged_management_mutation(method, path),
                method == "post"
            );
            assert!(success_component(method, path, 200).is_some());
        }
        for foreign in [
            "/platform",
            "/platform/role-policy/revisions/{revision_id}/other",
            "/organizations/{organization_id}/tenant-support-grants",
        ] {
            assert!(!is_privileged_management_path(foreign));
        }

        assert_eq!(
            request_schema(PLATFORM_ROLE_BINDING_ROLE_PATH).expect("role request")["required"],
            json!(["role", "expectedVersion", "expectedPolicyRevisionId"])
        );
        assert_eq!(
            request_schema(TENANT_SUPPORT_GRANT_APPROVALS_PATH).expect("approval request")
                ["required"],
            json!(["expectedContractDigest"])
        );
        assert!(request_schema(PLATFORM_ROLE_POLICY_PATH).is_none());
        assert_eq!(
            query_parameters("get", TRUST_DOMAIN_REVISIONS_PATH)[0]["schema"]["maximum"],
            MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE
        );
    }
}
