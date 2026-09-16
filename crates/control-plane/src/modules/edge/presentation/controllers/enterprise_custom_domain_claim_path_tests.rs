//! APP0.6-C1: Claim path for Edge-owned custom domain enterprise surface.
//!
//! Freezes the non-invented contract: `enterprise.custom-domain-branding`
//! reuses proven Edge domain-claim command/query routes under `/organizations`.
//! Does not invent a branding CMS, theme store, or second DNS ownership path.

use super::{domain_claim_commands_controller, domain_claim_queries_controller};
use a3s_boot::{CommandBus, QueryBus};
use std::sync::Arc;

fn custom_domain_management_paths() -> Vec<String> {
    let command_bus = Arc::new(CommandBus::new());
    let query_bus = Arc::new(QueryBus::new());
    let controllers = vec![
        domain_claim_commands_controller(Arc::clone(&command_bus)).expect("domain claim commands"),
        domain_claim_queries_controller(Arc::clone(&query_bus)).expect("domain claim queries"),
    ];

    assert!(
        controllers
            .iter()
            .all(|controller| controller.prefix() == "/organizations"),
        "custom-domain claim path must stay on Edge management traffic owner"
    );

    controllers
        .iter()
        .flat_map(|controller| {
            controller
                .routes()
                .iter()
                .map(|route| route.path().to_string())
        })
        .collect()
}

#[test]
fn enterprise_custom_domain_exposes_environment_scoped_claim_paths() {
    let paths = custom_domain_management_paths();

    for required in [
        "domain-claims",
        "environments/{environment_id}/domain-claims",
        "domain-claims/{claim_id}/verify",
    ] {
        assert!(
            paths.iter().any(|path| path.contains(required)),
            "missing custom-domain claim path fragment `{required}` in {paths:?}"
        );
    }
}

#[test]
fn enterprise_custom_domain_exposes_list_and_mutation_surfaces() {
    let paths = custom_domain_management_paths();
    assert!(
        paths
            .iter()
            .any(|path| path.contains("environments/{environment_id}/domain-claims")
                && !path.contains("verify")
                && !path.contains("revoke")),
        "custom-domain requires environment list/create surface; got {paths:?}"
    );
    assert!(
        paths.iter().any(|path| path.contains("verify")),
        "custom-domain requires verify mutation; got {paths:?}"
    );
}

