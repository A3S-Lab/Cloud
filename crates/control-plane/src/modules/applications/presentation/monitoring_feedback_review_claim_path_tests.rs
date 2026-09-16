//! APP0.5-C1: Claim path for Applications-owned feedback/annotation review.
//!
//! Freezes the non-invented contract: `monitoring.feedback-review` reuses the
//! proven APP0.2 session-scoped feedback and annotation management routes under
//! `/organizations`. Does not invent run-history, usage/cost, telemetry export,
//! alerts, or a second run log.

use super::feedback_delivery_controller::{
    application_feedback_commands_controller, application_feedback_queries_controller,
};
use a3s_boot::{CommandBus, QueryBus};
use std::sync::Arc;

fn feedback_review_management_paths() -> Vec<String> {
    let command_bus = Arc::new(CommandBus::new());
    let query_bus = Arc::new(QueryBus::new());
    let controllers = vec![
        application_feedback_commands_controller(Arc::clone(&command_bus))
            .expect("feedback commands"),
        application_feedback_queries_controller(Arc::clone(&query_bus)).expect("feedback queries"),
    ];

    assert!(
        controllers
            .iter()
            .all(|controller| controller.prefix() == "/organizations"),
        "feedback-review claim path must stay on Applications management traffic owner"
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
fn monitoring_feedback_review_exposes_session_scoped_claim_paths() {
    let paths = feedback_review_management_paths();

    for required in ["feedbacks", "annotations"] {
        assert!(
            paths.iter().any(|path| path.contains(required)),
            "missing feedback-review claim path fragment `{required}` in {paths:?}"
        );
    }

    for required in ["sessions/{session_id}/feedbacks", "sessions/{session_id}/annotations"] {
        assert!(
            paths.iter().any(|path| path.contains(required)),
            "feedback-review paths must stay session-scoped; missing `{required}` in {paths:?}"
        );
    }
}

#[test]
fn monitoring_feedback_review_exposes_list_and_get_query_surfaces() {
    let paths = feedback_review_management_paths();
    let feedback_gets = paths
        .iter()
        .filter(|path| path.contains("feedbacks/{feedback_id}"))
        .count();
    let annotation_gets = paths
        .iter()
        .filter(|path| path.contains("annotations/{annotation_id}"))
        .count();
    assert!(
        feedback_gets >= 1,
        "feedback review requires get-by-id query surface, got {paths:?}"
    );
    assert!(
        annotation_gets >= 1,
        "annotation review requires get-by-id query surface, got {paths:?}"
    );
}

