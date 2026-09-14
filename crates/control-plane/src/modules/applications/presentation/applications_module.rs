use super::controller::{application_commands_controller, application_queries_controller};
use super::delivery_controller::{
    application_delivery_commands_controller, application_delivery_queries_controller,
};
use super::feedback_delivery_controller::{
    application_feedback_commands_controller, application_feedback_queries_controller,
};
use super::blocking_observation_delivery_controller::application_blocking_observation_queries_controller;
use super::streaming_observation_delivery_controller::application_streaming_observation_queries_controller;
use super::asynchronous_observation_delivery_controller::application_asynchronous_observation_queries_controller;
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
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct ApplicationsModule;

impl Module for ApplicationsModule {
    fn name(&self) -> &'static str {
        "applications"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            application_commands_controller(module_ref.get::<CommandBus>()?)?,
            application_queries_controller(module_ref.get::<QueryBus>()?)?,
            application_delivery_commands_controller(module_ref.get::<CommandBus>()?)?,
            application_delivery_queries_controller(module_ref.get::<QueryBus>()?)?,
            application_feedback_commands_controller(module_ref.get::<CommandBus>()?)?,
            application_feedback_queries_controller(module_ref.get::<QueryBus>()?)?,
            application_message_variant_commands_controller(module_ref.get::<CommandBus>()?)?,
            application_message_variant_queries_controller(module_ref.get::<QueryBus>()?)?,
            application_message_file_reference_commands_controller(module_ref.get::<CommandBus>()?)?,
            application_message_file_reference_queries_controller(module_ref.get::<QueryBus>()?)?,
            application_message_citation_commands_controller(module_ref.get::<CommandBus>()?)?,
            application_message_citation_queries_controller(module_ref.get::<QueryBus>()?)?,
            application_blocking_observation_queries_controller(module_ref.get::<QueryBus>()?)?,
            application_streaming_observation_queries_controller(module_ref.get::<QueryBus>()?)?,
            application_asynchronous_observation_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
