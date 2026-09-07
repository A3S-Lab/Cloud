use super::RecordPluginPlanProjection;
use crate::modules::plugins::domain::entities::{NewPluginPlanProjection, PluginPlanProjection};
use crate::modules::plugins::domain::repositories::{
    IPluginAssignmentRepository, IPluginPlanProjectionRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct RecordPluginPlanProjectionHandler {
    assignments: Arc<dyn IPluginAssignmentRepository>,
    projections: Arc<dyn IPluginPlanProjectionRepository>,
}

impl RecordPluginPlanProjectionHandler {
    pub fn new(
        assignments: Arc<dyn IPluginAssignmentRepository>,
        projections: Arc<dyn IPluginPlanProjectionRepository>,
    ) -> Self {
        Self {
            assignments,
            projections,
        }
    }
}

impl CommandHandler<RecordPluginPlanProjection> for RecordPluginPlanProjectionHandler {
    fn execute(
        &self,
        command: RecordPluginPlanProjection,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PluginPlanProjection>>>
    {
        let assignments = Arc::clone(&self.assignments);
        let projections = Arc::clone(&self.projections);
        Box::pin(async move {
            let assignment = match assignments
                .find(command.organization_id, command.assignment_id)
                .await
            {
                Ok(Some(assignment)) => assignment,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "plugin assignment was not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if assignment.organization_id != command.organization_id {
                return Ok(Err(ApplicationError::NotFound(
                    "plugin assignment was not found".into(),
                )));
            }

            let plan_digest = match crate::modules::shared_kernel::domain::Sha256Digest::parse(
                command.envelope.plan_digest.clone(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Ok(Some(existing)) = projections
                .find_by_plan_digest(command.organization_id, &plan_digest)
                .await
            {
                if existing.assignment_id == command.assignment_id
                    && existing.operation_id == command.operation_id
                    && existing.assignment_generation == assignment.assignment_generation
                {
                    return Ok(Ok(existing));
                }
                return Ok(Err(ApplicationError::Conflict(
                    "plugin plan projection already exists for this plan digest".into(),
                )));
            }

            let projection = match PluginPlanProjection::from_validated_envelope(
                NewPluginPlanProjection {
                    organization_id: command.organization_id,
                    id: command.projection_id,
                    assignment_id: command.assignment_id,
                    operation_id: command.operation_id,
                    assignment_generation: assignment.assignment_generation,
                    envelope: command.envelope,
                    created_at: command.recorded_at,
                },
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };

            match projections.create(projection).await {
                Ok(value) => Ok(Ok(value)),
                Err(RepositoryError::Conflict(message)) => {
                    Ok(Err(ApplicationError::Conflict(message)))
                }
                Err(RepositoryError::NotFound) => Ok(Err(ApplicationError::NotFound(
                    "plugin assignment was not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
