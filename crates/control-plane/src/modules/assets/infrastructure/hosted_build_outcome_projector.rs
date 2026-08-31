use crate::modules::artifacts::published::{
    HostedBuildOutcome, HOSTED_BUILD_OUTCOME_EVENT_KEY, HOSTED_BUILD_OUTCOME_SCHEMA,
    LEGACY_HOSTED_BUILD_OUTCOME_SCHEMA,
};
use crate::modules::assets::application::HostedBuildOutcomeApplicationService;
use crate::modules::assets::domain::IAssetRepository;
use crate::modules::integration_events::{IIntegrationEventProjector, OutboxMessage};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Generic Outbox adapter around the Assets-owned projection policy.
pub struct HostedBuildOutcomeProjector {
    application: HostedBuildOutcomeApplicationService,
}

impl HostedBuildOutcomeProjector {
    pub fn new(assets: Arc<dyn IAssetRepository>) -> Self {
        Self {
            application: HostedBuildOutcomeApplicationService::new(assets),
        }
    }
}

#[async_trait]
impl IIntegrationEventProjector for HostedBuildOutcomeProjector {
    async fn project(&self, message: &OutboxMessage) -> Result<(), RepositoryError> {
        if message.event_key != HOSTED_BUILD_OUTCOME_EVENT_KEY {
            return Ok(());
        }
        let outcome: HostedBuildOutcome = serde_json::from_value(message.payload.clone())
            .map_err(|error| invalid_message(format!("payload could not be decoded: {error}")))?;
        outcome.validate().map_err(invalid_message)?;
        let version_matches = matches!(
            (message.schema_version, outcome.schema()),
            (1, LEGACY_HOSTED_BUILD_OUTCOME_SCHEMA) | (2, HOSTED_BUILD_OUTCOME_SCHEMA)
        );
        if !version_matches
            || message.organization_id() != Some(outcome.organization_id().as_uuid())
            || message.aggregate_id != outcome.build_run_id().as_uuid()
            || message.aggregate_version != outcome.build_run_version()
            || message.occurred_at != outcome.finished_at()
            || message.correlation_id != outcome.operation_id().as_uuid()
            || message.causation_id.is_some()
        {
            return Err(invalid_message(
                "envelope and published outcome identity differ".into(),
            ));
        }
        self.application
            .project(outcome, message.event_id, message.correlation_id)
            .await
    }
}

fn invalid_message(error: String) -> RepositoryError {
    RepositoryError::Storage(format!("Artifacts hosted build fact is invalid: {error}"))
}
