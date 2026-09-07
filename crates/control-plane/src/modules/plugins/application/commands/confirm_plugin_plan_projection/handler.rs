use super::ConfirmPluginPlanProjection;
use crate::modules::plugins::domain::entities::PluginPlanProjection;
use crate::modules::plugins::domain::repositories::IPluginPlanProjectionRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct ConfirmPluginPlanProjectionHandler {
    projections: Arc<dyn IPluginPlanProjectionRepository>,
}

impl ConfirmPluginPlanProjectionHandler {
    pub fn new(projections: Arc<dyn IPluginPlanProjectionRepository>) -> Self {
        Self { projections }
    }
}

impl CommandHandler<ConfirmPluginPlanProjection> for ConfirmPluginPlanProjectionHandler {
    fn execute(
        &self,
        command: ConfirmPluginPlanProjection,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PluginPlanProjection>>>
    {
        let projections = Arc::clone(&self.projections);
        Box::pin(async move {
            let existing = match projections
                .find(command.organization_id, command.projection_id)
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "plugin plan projection was not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };

            let expected_confirmation_digest = existing.confirmation_digest.clone();
            let confirmed = match existing.confirm(&command.confirmation, command.confirmed_at) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };

            match projections
                .update(confirmed, expected_confirmation_digest.as_ref())
                .await
            {
                Ok(value) => Ok(Ok(value)),
                Err(RepositoryError::Conflict(message)) => {
                    Ok(Err(ApplicationError::Conflict(message)))
                }
                Err(RepositoryError::NotFound) => Ok(Err(ApplicationError::NotFound(
                    "plugin plan projection was not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
