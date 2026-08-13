use super::{ProjectAttributionProfileResponse, ProjectListItemResponse};
use crate::modules::projects::application::commands::update_project_attribution::UpdateProjectAttributionResult;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAttributionMutationResponse {
    pub project: ProjectListItemResponse,
    pub attribution_profile: ProjectAttributionProfileResponse,
    pub replayed: bool,
}

impl From<UpdateProjectAttributionResult> for ProjectAttributionMutationResponse {
    fn from(result: UpdateProjectAttributionResult) -> Self {
        Self {
            project: result.project.into(),
            attribution_profile: result.attribution_profile.into(),
            replayed: result.replayed,
        }
    }
}
