use crate::modules::applications::domain::ApplicationWorkflowRevisionEvidence;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    OrganizationId, ProjectId, WorkflowDefinitionId, WorkflowRevisionId,
};
use async_trait::async_trait;

/// Applications-owned read boundary for exact immutable Workflow evidence.
///
/// Implementations return metadata only. A graph, payload, mutable Workflow
/// head, plan, or run state must never cross this port.
#[async_trait]
pub trait IApplicationWorkflowRevisionPort: Send + Sync {
    async fn resolve_revision(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        workflow_definition_id: WorkflowDefinitionId,
        workflow_revision_id: WorkflowRevisionId,
    ) -> ApplicationResult<ApplicationWorkflowRevisionEvidence>;
}
