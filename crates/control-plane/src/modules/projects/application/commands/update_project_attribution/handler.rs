use super::{UpdateProjectAttribution, UpdateProjectAttributionResult};
use crate::modules::projects::application::resource_access::ProjectResourceAccess;
use crate::modules::projects::domain::entities::ProjectAttributionProfile;
use crate::modules::projects::domain::events::ProjectAttributionProfileUpdated;
use crate::modules::projects::domain::repositories::{
    IProjectRepository, ProjectAttributionRecord, UpdateProjectAttributionWrite,
};
use crate::modules::projects::domain::value_objects::{
    BusinessOwnerReference, CostAttributionCode, ProjectAttributionLabels,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, ProjectAttributionProfileId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct UpdateProjectAttributionHandler {
    projects: Arc<dyn IProjectRepository>,
}

impl UpdateProjectAttributionHandler {
    pub fn new(projects: Arc<dyn IProjectRepository>) -> Self {
        Self { projects }
    }
}

impl CommandHandler<UpdateProjectAttribution> for UpdateProjectAttributionHandler {
    fn execute(
        &self,
        command: UpdateProjectAttribution,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<UpdateProjectAttributionResult>>,
    > {
        let projects = Arc::clone(&self.projects);
        Box::pin(async move {
            let project = match ProjectResourceAccess::new(Arc::clone(&projects))
                .project(
                    command.organization_id,
                    command.project_id,
                    &command.resource_access,
                )
                .await
            {
                Ok(project) => project,
                Err(error) => return Ok(Err(error)),
            };
            if command.actor_principal_id.as_uuid().is_nil() || command.request_id.is_nil() {
                return Ok(Err(ApplicationError::Invalid(
                    "project attribution actor and request identifiers must be non-nil".into(),
                )));
            }
            if command.expected_project_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected project version must be greater than zero".into(),
                )));
            }
            let business_owner_reference =
                match BusinessOwnerReference::parse(command.business_owner_reference) {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            let cost_attribution_code = match command
                .cost_attribution_code
                .map(CostAttributionCode::parse)
                .transpose()
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let labels = match ProjectAttributionLabels::parse(command.labels) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "expectedProjectVersion": command.expected_project_version,
                "businessOwnerReference": business_owner_reference.as_str(),
                "costAttributionCode": cost_attribution_code.as_ref().map(|code| code.as_str()),
                "labels": labels.as_map(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/attribution-profiles",
                    command.organization_id, command.project_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match projects.replay_attribution_update(&idempotency).await {
                Ok(Some(replayed))
                    if replayed.value.project.organization_id == command.organization_id
                        && replayed.value.project.id == command.project_id
                        && replayed.value.attribution_profile.organization_id
                            == command.organization_id
                        && replayed.value.attribution_profile.project_id == command.project_id =>
                {
                    return Ok(Ok(UpdateProjectAttributionResult {
                        project: replayed.value.project,
                        attribution_profile: replayed.value.attribution_profile,
                        replayed: true,
                    }));
                }
                Ok(Some(_)) => {
                    return Err(BootError::Internal(
                        "project attribution replay changed its immutable identity".into(),
                    ));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let profile = match ProjectAttributionProfile::create(
                project.organization_id,
                project.id,
                ProjectAttributionProfileId::new(),
                project.current_attribution_profile_id,
                business_owner_reference,
                cost_attribution_code,
                labels,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(profile) => profile,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let project = match project
                .point_to_attribution_profile(command.expected_project_version, profile.id)
            {
                Ok(project) => project,
                Err(error) => return Ok(Err(ApplicationError::Conflict(error))),
            };
            let event = ProjectAttributionProfileUpdated::envelope(
                &profile,
                project.aggregate_version,
                command.request_id,
            )
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match projects
                .update_attribution(UpdateProjectAttributionWrite {
                    record: ProjectAttributionRecord {
                        project,
                        attribution_profile: profile,
                    },
                    expected_project_version: command.expected_project_version,
                    event,
                    idempotency,
                    request_id: command.request_id,
                })
                .await
            {
                Ok(result) => result,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(UpdateProjectAttributionResult {
                project: result.value.project,
                attribution_profile: result.value.attribution_profile,
                replayed: result.replayed,
            }))
        })
    }
}
