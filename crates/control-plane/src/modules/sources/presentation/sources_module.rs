use super::controllers::{
    github_webhooks_controller, source_revision_queries_controller, source_revisions_controller,
};
use crate::modules::sources::domain::ISourceWebhookVerifier;
use a3s_boot::{CommandBus, ControllerDefinition, Module, ModuleRef, QueryBus, Result};
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct SourcesModule {
    webhook_verifier: Arc<dyn ISourceWebhookVerifier>,
}

impl SourcesModule {
    pub fn new(webhook_verifier: Arc<dyn ISourceWebhookVerifier>) -> Self {
        Self { webhook_verifier }
    }
}

impl fmt::Debug for SourcesModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourcesModule")
            .finish_non_exhaustive()
    }
}

impl Module for SourcesModule {
    fn name(&self) -> &'static str {
        "sources"
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            github_webhooks_controller(
                module_ref.get::<CommandBus>()?,
                Arc::clone(&self.webhook_verifier),
            )?,
            source_revisions_controller(module_ref.get::<CommandBus>()?)?,
            source_revision_queries_controller(module_ref.get::<QueryBus>()?)?,
        ])
    }
}
