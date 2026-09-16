//! APP0.2-C57: Management claim path for proven APP0.2 toolkit surfaces.
//!
//! Freezes the non-invented contract: toolkit capabilities that already have
//! Verified CQRS + management REST stay claimable through Applications
//! `/organizations` delivery controllers. Does not invent opener/follow-up or
//! flip application-mode capabilities owned by APP0.4.

use super::feedback_delivery_controller::{
    application_feedback_commands_controller, application_feedback_queries_controller,
};
use super::message_citation_delivery_controller::{
    application_message_citation_commands_controller,
    application_message_citation_queries_controller,
};
use super::message_file_reference_delivery_controller::{
    application_message_file_reference_commands_controller,
    application_message_file_reference_queries_controller,
};
use super::message_variant_delivery_controller::{
    application_message_variant_commands_controller, application_message_variant_queries_controller,
};
use a3s_boot::{CommandBus, QueryBus};
use std::sync::Arc;

fn toolkit_management_paths() -> Vec<String> {
    let command_bus = Arc::new(CommandBus::new());
    let query_bus = Arc::new(QueryBus::new());
    let controllers = vec![
        application_feedback_commands_controller(Arc::clone(&command_bus))
            .expect("feedback commands"),
        application_feedback_queries_controller(Arc::clone(&query_bus)).expect("feedback queries"),
        application_message_variant_commands_controller(Arc::clone(&command_bus))
            .expect("variant commands"),
        application_message_variant_queries_controller(Arc::clone(&query_bus))
            .expect("variant queries"),
        application_message_file_reference_commands_controller(Arc::clone(&command_bus))
            .expect("file-reference commands"),
        application_message_file_reference_queries_controller(Arc::clone(&query_bus))
            .expect("file-reference queries"),
        application_message_citation_commands_controller(Arc::clone(&command_bus))
            .expect("citation commands"),
        application_message_citation_queries_controller(Arc::clone(&query_bus))
            .expect("citation queries"),
    ];

    assert!(
        controllers
            .iter()
            .all(|controller| controller.prefix() == "/organizations"),
        "proven toolkit claim path must stay on Applications management traffic owner"
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
fn proven_toolkit_surfaces_expose_management_claim_paths() {
    let paths = toolkit_management_paths();

    for required in [
        "feedbacks",
        "annotations",
        "message-variants",
        "message-file-references",
        "message-citations",
    ] {
        assert!(
            paths.iter().any(|path| path.contains(required)),
            "missing toolkit claim path fragment `{required}` in {paths:?}"
        );
    }
}

