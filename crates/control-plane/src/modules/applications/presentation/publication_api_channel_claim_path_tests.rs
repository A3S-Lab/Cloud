//! APP0.3-C32: Delivery traffic-owner claim path for API publication channels.
//!
//! Freezes the non-invented contract: publication channel tokens align with
//! Gateway admit headers, and authenticated Delivery exposes blocking/streaming
//! observation under `/delivery`. Does not invent path->channel or Edge stamps.

use crate::modules::applications::domain::ApplicationPublicationChannel;
use crate::modules::applications::presentation::authenticated_delivery_module::ApplicationAuthenticatedDeliveryModule;
use crate::modules::applications::presentation::delivery_process_drain::DeliveryProcessDrain;
use a3s_boot::{CommandBus, QueryBus};
use std::sync::Arc;

#[test]
fn publication_api_channels_match_gateway_admit_tokens() {
    assert_eq!(
        ApplicationPublicationChannel::ApiBlocking.as_str(),
        "api_blocking"
    );
    assert_eq!(
        ApplicationPublicationChannel::ApiStreaming.as_str(),
        "api_streaming"
    );
    assert_eq!(
        ApplicationPublicationChannel::parse("api_blocking").expect("parse"),
        ApplicationPublicationChannel::ApiBlocking
    );
    assert_eq!(
        ApplicationPublicationChannel::parse("api_streaming").expect("parse"),
        ApplicationPublicationChannel::ApiStreaming
    );
}

#[test]
fn delivery_traffic_owner_exposes_blocking_and_streaming_observation() {
    let controllers = ApplicationAuthenticatedDeliveryModule::authenticated_controllers(
        Arc::new(CommandBus::new()),
        Arc::new(QueryBus::new()),
        DeliveryProcessDrain::default(),
    )
    .expect("authenticated delivery controllers");

    assert!(
        controllers
            .iter()
            .all(|controller| controller.prefix() == "/delivery"),
        "API publication claim path must stay on Delivery traffic owner"
    );

    let paths: Vec<&str> = controllers
        .iter()
        .flat_map(|controller| controller.routes().iter().map(|route| route.path()))
        .collect();
    assert!(
        paths.iter().any(|path| path.contains("blocking-observation")),
        "publication.api-blocking requires Delivery blocking-observation: {paths:?}"
    );
    assert!(
        paths.iter().any(|path| path.contains("streaming-observation")),
        "publication.api-streaming requires Delivery streaming-observation: {paths:?}"
    );
}
