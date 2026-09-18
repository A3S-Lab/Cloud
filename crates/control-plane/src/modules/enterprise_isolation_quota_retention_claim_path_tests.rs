//! APP0.6-C2: Claim path for platform isolation / quota / retention surface.
//!
//! Freezes the non-invented contract: `enterprise.isolation-quota-retention`
//! reuses proven owning-context HTTP under `/organizations`:
//! Files org quota, Audit retention status, and Inference usage retention.
//! Does not invent a platform isolation module, concurrency governor, or
//! second policy plane.

use crate::modules::audit::audit_query_controller;
use crate::modules::files::user_file_queries_controller;
use crate::modules::inference::usage_retention_controller;
use a3s_boot::QueryBus;
use std::sync::Arc;

fn isolation_quota_retention_paths() -> Vec<String> {
    let query_bus = Arc::new(QueryBus::new());
    let controllers = vec![
        user_file_queries_controller(Arc::clone(&query_bus)).expect("user file queries"),
        audit_query_controller(Arc::clone(&query_bus)).expect("audit queries"),
        usage_retention_controller(Arc::clone(&query_bus)).expect("inference usage retention"),
    ];

    assert!(
        controllers
            .iter()
            .all(|controller| controller.prefix() == "/organizations"),
        "isolation-quota-retention claim path must stay on owning-context management traffic"
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
fn enterprise_isolation_quota_retention_exposes_owning_context_claim_paths() {
    let paths = isolation_quota_retention_paths();

    for required in [
        "user-file-quota",
        "audit-records/retention",
        "inference-usage/retention",
    ] {
        assert!(
            paths.iter().any(|path| path.contains(required)),
            "missing isolation-quota-retention claim path fragment `{required}` in {paths:?}"
        );
    }
}

#[test]
fn enterprise_isolation_quota_retention_stays_organization_scoped() {
    let paths = isolation_quota_retention_paths();
    assert!(
        paths.iter().any(|path| path.contains("user-file-quota")),
        "quota surface required; got {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|path| path.contains("audit-records/retention")),
        "audit retention surface required; got {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|path| path.contains("inference-usage/retention")),
        "inference retention surface required; got {paths:?}"
    );
}

