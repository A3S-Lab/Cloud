use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::projects::domain::entities::{Project, ProjectAttributionProfile};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, ProjectId};
use a3s_boot::Command;
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct UpdateProjectAttribution {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub actor_principal_id: PrincipalId,
    pub resource_access: ResourceAccessEvaluator,
    pub expected_project_version: u64,
    pub business_owner_reference: String,
    pub cost_attribution_code: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for UpdateProjectAttribution {
    type Output = ApplicationResult<UpdateProjectAttributionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateProjectAttributionResult {
    pub project: Project,
    pub attribution_profile: ProjectAttributionProfile,
    pub replayed: bool,
}
