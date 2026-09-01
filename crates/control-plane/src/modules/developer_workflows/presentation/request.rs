use crate::modules::developer_workflows::DeveloperWorkflowAccess;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::presentation::{developer_workflow_access, resource_access_evaluator};
use a3s_boot::{BootRequest, Result};
use uuid::Uuid;

pub(super) fn organization_id(request: &BootRequest) -> Result<OrganizationId> {
    Ok(OrganizationId::from_uuid(
        request.param_as::<Uuid>("organization_id")?,
    ))
}

pub(super) fn project_id(request: &BootRequest) -> Result<ProjectId> {
    Ok(ProjectId::from_uuid(
        request.param_as::<Uuid>("project_id")?,
    ))
}

pub(super) fn environment_id(request: &BootRequest) -> Result<EnvironmentId> {
    Ok(EnvironmentId::from_uuid(
        request.param_as::<Uuid>("environment_id")?,
    ))
}

pub(super) fn workflow_access(request: &BootRequest) -> Result<DeveloperWorkflowAccess> {
    Ok(developer_workflow_access(&resource_access_evaluator(
        &request.require_auth_principal()?,
    )?))
}
