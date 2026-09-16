//! APP0.4-C4: Claim path for Workflow-owned variable inspection.
//!
//! Freezes the non-invented contract: `toolkit.variable-inspection` reuses the
//! proven WorkflowRun variables management route under `/organizations`
//! (`GET .../workflow-runs/{workflow_run_id}/variables`). Does not invent a
//! second variable map, Applications toolkit domain, or public advertisement.

use super::controllers::workflow_queries_controller;
use a3s_boot::QueryBus;
use std::sync::Arc;

fn variable_inspection_management_paths() -> Vec<String> {
    let query_bus = Arc::new(QueryBus::new());
    let controller =
        workflow_queries_controller(query_bus).expect("workflow queries controller");

    assert_eq!(
        controller.prefix(),
        "/organizations",
        "variable-inspection claim path must stay on Workflow management traffic owner"
    );

    controller
        .routes()
        .iter()
        .map(|route| route.path().to_string())
        .collect()
}

#[test]
fn toolkit_variable_inspection_exposes_workflow_run_variables_claim_path() {
    let paths = variable_inspection_management_paths();
    assert!(
        paths
            .iter()
            .any(|path| path.contains("workflow-runs/{workflow_run_id}/variables")),
        "missing variable-inspection claim path in {paths:?}"
    );
}

#[test]
fn toolkit_variable_inspection_stays_run_scoped_read_surface() {
    let paths = variable_inspection_management_paths();
    let variable_paths: Vec<_> = paths
        .iter()
        .filter(|path| path.contains("/variables"))
        .collect();
    assert_eq!(
        variable_paths.len(),
        1,
        "expected exactly one variables read surface, got {variable_paths:?}"
    );
    assert!(
        variable_paths[0].contains("workflow-runs/{workflow_run_id}/variables"),
        "variables surface must stay workflow-run scoped: {variable_paths:?}"
    );
}
