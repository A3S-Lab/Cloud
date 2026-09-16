use super::asynchronous_observation_delivery_controller::application_asynchronous_observation_queries_controller;
use super::blocking_observation_delivery_controller::application_blocking_observation_queries_controller;
use super::controller::{application_commands_controller, application_queries_controller};
use super::delivery_controller::{
    application_delivery_commands_controller, application_delivery_queries_controller,
};
use super::delivery_credential_delivery_controller::{
    application_delivery_credential_commands_controller,
    application_delivery_credential_queries_controller,
};
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
use super::publication_route_intent_controller::{
    application_publication_route_intent_commands_controller,
    application_publication_route_intent_queries_controller,
};
use super::authenticated_delivery_module::ApplicationAuthenticatedDeliveryModule;
use super::delivery_process_drain::DeliveryProcessDrain;
use super::public_delivery_module::ApplicationPublicDeliveryModule;
use super::streaming_observation_delivery_controller::application_streaming_observation_queries_controller;
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default)]
pub struct ApplicationsModule;

impl Module for ApplicationsModule {
    fn name(&self) -> &'static str {
        "applications"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let command_bus = module_ref.get::<CommandBus>()?;
        let query_bus = module_ref.get::<QueryBus>()?;
        let mut controllers = vec![
            application_commands_controller(Arc::clone(&command_bus))?,
            application_queries_controller(Arc::clone(&query_bus))?,
            application_delivery_commands_controller(Arc::clone(&command_bus))?,
            application_delivery_queries_controller(Arc::clone(&query_bus))?,
            application_feedback_commands_controller(Arc::clone(&command_bus))?,
            application_feedback_queries_controller(Arc::clone(&query_bus))?,
            application_message_variant_commands_controller(Arc::clone(&command_bus))?,
            application_message_variant_queries_controller(Arc::clone(&query_bus))?,
            application_message_file_reference_commands_controller(Arc::clone(&command_bus))?,
            application_message_file_reference_queries_controller(Arc::clone(&query_bus))?,
            application_message_citation_commands_controller(Arc::clone(&command_bus))?,
            application_message_citation_queries_controller(Arc::clone(&query_bus))?,
            application_blocking_observation_queries_controller(Arc::clone(&query_bus))?,
            application_streaming_observation_queries_controller(Arc::clone(&query_bus))?,
            application_asynchronous_observation_queries_controller(Arc::clone(&query_bus))?,
            application_delivery_credential_commands_controller(Arc::clone(&command_bus))?,
            application_delivery_credential_queries_controller(Arc::clone(&query_bus))?,
            application_publication_route_intent_commands_controller(Arc::clone(&command_bus))?,
            application_publication_route_intent_queries_controller(Arc::clone(&query_bus))?,
        ];
        // APP0.3-C8: management mounts use a default non-draining latch so
        // OpenAPI/routes stay available; Delivery process owns the shared latch.
        let management_drain = DeliveryProcessDrain::default();
        controllers.extend(ApplicationPublicDeliveryModule::public_controllers(
            Arc::clone(&command_bus),
            Arc::clone(&query_bus),
            management_drain.clone(),
        )?);
        // APP0.3-C7: document/serve authenticated `/delivery` in the management
        // OpenAPI contract the same way anonymous `/anonymous-delivery` is shared.
        // ProcessRole::Delivery still owns the closed production composition.
        controllers.extend(
            ApplicationAuthenticatedDeliveryModule::authenticated_controllers(
                command_bus,
                query_bus,
                management_drain,
            )?,
        );
        Ok(controllers)
    }
}
