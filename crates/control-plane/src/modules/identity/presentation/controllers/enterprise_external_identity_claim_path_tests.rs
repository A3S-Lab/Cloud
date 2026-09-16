//! C0.3-C1: Claim path for Identity-owned external identity and fine-grained access.
//!
//! Freezes the non-invented contract: `enterprise.external-identity` reuses proven
//! Resource Grant and Workload Trust HTTP surfaces. Does not invent a second
//! directory-product IdP, SCIM sync UI, or SIEM-bound identity CMS.

use super::{
    resource_grant_controller, workload_trust_commands_controller, workload_trust_queries_controller,
};
use a3s_boot::{CommandBus, QueryBus};
use std::sync::Arc;

fn external_identity_paths() -> Vec<(String, String)> {
    let commands = Arc::new(CommandBus::new());
    let queries = Arc::new(QueryBus::new());
    let controllers = vec![
        resource_grant_controller(Arc::clone(&commands), Arc::clone(&queries))
            .expect("resource grants"),
        workload_trust_queries_controller(Arc::clone(&queries)).expect("workload trust queries"),
        workload_trust_commands_controller(Arc::clone(&commands)).expect("workload trust commands"),
    ];

    controllers
        .iter()
        .flat_map(|controller| {
            let prefix = controller.prefix().to_string();
            controller
                .routes()
                .iter()
                .map(move |route| (prefix.clone(), route.path().to_string()))
        })
        .collect()
}

#[test]
fn enterprise_external_identity_exposes_resource_grant_and_workload_trust_paths() {
    let paths = external_identity_paths();
    let joined: Vec<String> = paths
        .iter()
        .map(|(prefix, path)| format!("{prefix}{path}"))
        .collect();

    for required in [
        "/organizations/{organization_id}/memberships/{membership_id}/resource-grants",
        "/organizations/{organization_id}/resource-grants/{resource_grant_id}",
        "/organizations/{organization_id}/resource-grants/{resource_grant_id}/revocation",
        "/platform/trust-domains/{trust_domain_id}",
        "/platform/organizations/{organization_id}/workload-identity-policies/{policy_id}",
        "/platform/organizations/{organization_id}/workloads/{workload_id}/identity-policy",
    ] {
        assert!(
            joined.iter().any(|path| path.contains(required)),
            "missing external-identity claim path `{required}` in {joined:?}"
        );
    }
}

#[test]
fn enterprise_external_identity_keeps_tenant_grant_and_platform_trust_owners() {
    let paths = external_identity_paths();
    assert!(
        paths.iter().any(|(prefix, path)| prefix == "/organizations"
            && path.contains("resource-grants")),
        "tenant resource-grant owner required; got {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|(prefix, path)| prefix == "/platform" && path.contains("trust-domains")),
        "platform trust-domain owner required; got {paths:?}"
    );
}

