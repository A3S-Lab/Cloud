use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::workloads::domain::entities::SourceBuildAdmission;
use async_trait::async_trait;

/// Workloads-owned request for admitting one successful external source build.
///
/// Sources and Artifacts remain authoritative for revision and BuildRun state.
/// Workloads receives only the admission needed by its external-build revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkloadSourceBuildAdmissionRequest {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_revision_id: SourceRevisionId,
}

#[async_trait]
pub trait IWorkloadSourceBuildAdmissionPort: Send + Sync {
    async fn admit(
        &self,
        request: WorkloadSourceBuildAdmissionRequest,
    ) -> ApplicationResult<SourceBuildAdmission>;
}
