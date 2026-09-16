//! APP0.5-C2: Claim path for Inference-owned usage/cost showback.
//!
//! Freezes the non-invented contract: `monitoring.usage-cost` reuses the proven
//! I0.2c environment-scoped inference usage query routes under `/organizations`.
//! Does not invent a second cost engine, Gateway showback mirror, or flip
//! operations_telemetry-owned monitoring surfaces.

use super::usage_queries_controller::usage_queries_controller;
use a3s_boot::QueryBus;
use std::sync::Arc;

fn usage_cost_showback_paths() -> Vec<String> {
    let query_bus = Arc::new(QueryBus::new());
    let controller = usage_queries_controller(query_bus).expect("usage queries");

    assert_eq!(
        controller.prefix(),
        "/organizations",
        "usage-cost claim path must stay on Inference management traffic owner"
    );

    controller
        .routes()
        .iter()
        .map(|route| route.path().to_string())
        .collect()
}

#[test]
fn monitoring_usage_cost_exposes_environment_scoped_showback_paths() {
    let paths = usage_cost_showback_paths();

    for required in [
        "inference-usage/daily-rollups",
        "inference-usage/requests/{request_id}",
        "environments/{environment_id}/inference-usage",
    ] {
        assert!(
            paths.iter().any(|path| path.contains(required)),
            "missing usage-cost claim path fragment `{required}` in {paths:?}"
        );
    }
}

#[test]
fn monitoring_usage_cost_stays_on_organizations_management_owner() {
    let paths = usage_cost_showback_paths();
    assert!(
        !paths.is_empty(),
        "usage-cost showback must expose at least one query route"
    );
    assert!(
        paths.iter().all(|path| path.contains("/projects/")
            && path.contains("/environments/")
            && path.contains("inference-usage")),
        "usage-cost paths must stay project/environment-scoped inference showback; got {paths:?}"
    );
}

